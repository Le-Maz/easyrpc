use std::{marker::Tuple, pin::Pin, sync::Arc};

use serde::{Serialize, de::DeserializeOwned};

use crate::{error::RpcError, transport::RpcTransport};

pub trait IntoRpcCommand<Args, Fut, Output>: Send + Sync + 'static
where
    Args: Tuple + Serialize + DeserializeOwned,
    Fut: Future<Output = Result<Output, RpcError>>,
    Output: Serialize + DeserializeOwned,
{
    fn call(&self, args: Args) -> Fut;
}

impl<F, Args, Fut, Output> IntoRpcCommand<Args, Fut, Output> for F
where
    F: Fn<Args, Output = Fut> + Send + Sync + 'static,
    Args: Tuple + Serialize + DeserializeOwned,
    Fut: Future<Output = Result<Output, RpcError>>,
    Output: Serialize + DeserializeOwned,
{
    fn call(&self, args: Args) -> Fut {
        self.call(args)
    }
}

type RpcCommandFuture<Output> = Pin<Box<dyn Future<Output = Result<Output, RpcError>> + Send>>;

pub enum RpcCommand<Args, Output>
where
    Args: Tuple,
{
    Server(Box<dyn Fn<Args, Output = RpcCommandFuture<Output>> + Send + Sync>),
    Client(Arc<dyn RpcTransport + Send + Sync>, Arc<[u8]>),
}

impl<Args, Output> FnOnce<Args> for RpcCommand<Args, Output>
where
    Args: Tuple + Serialize + DeserializeOwned,
    Output: Serialize + DeserializeOwned,
{
    type Output = Pin<Box<dyn Future<Output = Result<Output, RpcError>> + Send>>;

    extern "rust-call" fn call_once(self, args: Args) -> Self::Output {
        match self {
            RpcCommand::Server(closure) => closure.call_once(args),
            RpcCommand::Client(transport, name) => {
                let mut data = Vec::new();
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
            }
        }
    }
}

impl<Args, Output> FnMut<Args> for RpcCommand<Args, Output>
where
    Args: Tuple + Serialize + DeserializeOwned,
    Output: Serialize + DeserializeOwned,
{
    extern "rust-call" fn call_mut(&mut self, args: Args) -> Self::Output {
        match self {
            RpcCommand::Server(closure) => closure.call_mut(args),
            RpcCommand::Client(transport, name) => {
                let mut data = Vec::new();
                let serialization_result = ciborium::into_writer(&args, &mut data);
                let transport = transport.clone();
                let name = name.clone();
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
            }
        }
    }
}

impl<Args, Output> Fn<Args> for RpcCommand<Args, Output>
where
    Args: Tuple + Serialize + DeserializeOwned,
    Output: Serialize + DeserializeOwned,
{
    extern "rust-call" fn call(&self, args: Args) -> Self::Output {
        match self {
            RpcCommand::Server(closure) => Fn::call(&closure, args),
            RpcCommand::Client(transport, name) => {
                let mut data = Vec::new();
                let serialization_result = ciborium::into_writer(&args, &mut data);
                let transport = transport.clone();
                let name = name.clone();
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
            }
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{command::IntoRpcCommand, error::RpcError};

    async fn do_nothing() -> Result<(), RpcError> {
        Ok(())
    }

    async fn echo(text: String) -> Result<String, RpcError> {
        Ok(text)
    }

    #[tokio::test]
    async fn call_command() {
        let empty = IntoRpcCommand::call(&do_nothing, ()).await.unwrap();
        assert_eq!(empty, ());
        let lorem = "Lorem ipsum";
        let response = IntoRpcCommand::call(&echo, (lorem.to_string(),))
            .await
            .unwrap();
        assert_eq!(response, lorem);
    }
}
