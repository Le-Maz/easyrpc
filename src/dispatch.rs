use std::{marker::Tuple, sync::Arc};

use serde::{Serialize, de::DeserializeOwned};

#[cfg(feature = "server")]
use crate::registry::{CommandRegistry, ServerCommandBox};
use crate::{
    command::{IntoRpcCommand, RpcCommand},
    error::RpcError,
    transport::RpcTransport,
};

#[derive(Clone)]
pub struct RpcDispatch {
    transport: Arc<dyn RpcTransport + Send + Sync>,
    #[cfg(feature = "server")]
    commands: Arc<CommandRegistry>,
}

impl RpcDispatch {
    pub fn new(transport: impl RpcTransport) -> Self {
        Self {
            transport: Arc::new(transport),
            #[cfg(feature = "server")]
            commands: Default::default(),
        }
    }

    pub fn register<Command, Args, Fut, Output>(
        &self,
        name: &str,
        #[cfg_attr(not(feature = "server"), allow(unused))]
        command: Command,
    ) -> RpcCommand<Args, Output>
    where
        Command: IntoRpcCommand<Args, Fut, Output> + Clone + 'static,
        Args: Tuple + Serialize + DeserializeOwned,
        Fut: Future<Output = Result<Output, RpcError>> + Send + Sync + 'static,
        Output: Serialize + DeserializeOwned,
    {
        #[cfg(feature = "server")]
        self.commands.insert(
            name.as_bytes().to_vec(),
            ServerCommandBox::new(command.clone()),
        );
        #[cfg(feature = "client")]
        {
            let name = Arc::<[u8]>::from(name.as_bytes());
            let transport = self.transport.clone();
            RpcCommand::new(Box::new(move |args| {
                let transport = transport.clone();
                let mut data = Vec::new();
                let name = name.clone();
                let serialization_result = ciborium::into_writer(&args, &mut data);
                Box::pin(async move {
                    use std::io::Cursor;

                    serialization_result
                        .map_err(Into::into)
                        .map_err(RpcError::Serialization)?;
                    let fut = transport.call(name, data);

                    let response = fut.await?;
                    let output = ciborium::from_reader(Cursor::new(response))
                        .map_err(Into::into)
                        .map_err(RpcError::Deserialization)?;

                    Ok(output)
                })
            }))
        }
        #[cfg(not(feature = "client"))]
        {
            RpcCommand::new(Box::new(move |args| Box::pin(command.call(args))))
        }
    }

    #[cfg(feature = "server")]
    pub async fn listen(&self) -> Result<(), RpcError> {
        let commands = self.commands.clone();
        let handler = Arc::new(move |name, data| {
            use std::pin::Pin;

            let command = commands.get(name).ok_or(RpcError::ProcedureNotFound);
            Box::pin(async move {
                let command = command?;
                command(data).await
            }) as Pin<Box<dyn Future<Output = Result<Vec<u8>, RpcError>> + Sync + Send>>
        });
        self.transport.clone().listen(handler).await
    }
}

#[cfg(all(
    feature = "server",
    feature = "client",
    feature = "tokio-tcp-connector"
))]
#[cfg(test)]
mod tests {
    use tokio::{net::TcpListener, sync::Mutex};

    use crate::{
        dispatch::RpcDispatch,
        error::RpcError,
        transport::{tokio_io::TokioIoTransport, tokio_io::tcp::TokioTcpConnector},
    };

    async fn echo(text: String) -> Result<String, RpcError> {
        Ok(text)
    }

    #[tokio::test]
    async fn call_command() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let transport = TokioIoTransport::new(TokioTcpConnector::new(Mutex::new(listener), addr));
        let dispatch = RpcDispatch::new(transport);
        let command = dispatch.register("echo", echo);
        tokio::select! {
            _ = dispatch.listen() => {
                panic!("Listener finished first");
            }
            Ok(value) = command("Lorem ipsum".to_string()) => {
                assert_eq!(value, "Lorem ipsum");
            }
        }
    }
}
