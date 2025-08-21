use std::{marker::Tuple, sync::Arc};

use serde::{Serialize, de::DeserializeOwned};

#[cfg(feature = "server")]
use crate::registry::{CommandRegistry, ServerCommandBox};
use crate::{
    command::{GlobalRpcCommand, RpcCommand},
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

    pub(crate) fn register_global<Args, Output>(
        &'static self,
        global_command: &GlobalRpcCommand<Args, Output>,
    ) -> RpcCommand<Args, Output>
    where
        Args: Tuple + Serialize + DeserializeOwned,
        Output: Serialize + DeserializeOwned,
    {
        #[cfg(feature = "server")]
        self.commands.insert(
            global_command.name.as_bytes().to_vec(),
            ServerCommandBox::new(global_command.inner),
        );
        #[cfg(feature = "client")]
        {
            let name = Arc::<[u8]>::from(global_command.name.as_bytes());
            let transport = self.transport.clone();
            RpcCommand::Client(transport, name)
        }
        #[cfg(not(feature = "client"))]
        {
            RpcCommand::Server(global_command.inner)
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
            }) as Pin<Box<dyn Future<Output = Result<Vec<u8>, RpcError>> + Send>>
        });
        self.transport.clone().listen(handler).await
    }
}
