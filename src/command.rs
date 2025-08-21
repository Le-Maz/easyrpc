#[cfg(feature = "client")]
use std::marker::PhantomData;
use std::{
    marker::Tuple,
    ops::Deref,
    pin::Pin,
    sync::{Arc, OnceLock},
};

use serde::{Serialize, de::DeserializeOwned};

use crate::{error::RpcError, get_global_dispatch, transport::RpcTransport};

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

pub type RpcCommandFuture<Output> = Pin<Box<dyn Future<Output = Result<Output, RpcError>> + Send>>;

pub enum RpcCommand<Args, Output>
where
    Args: Tuple + 'static,
    Output: 'static,
{
    #[cfg(feature = "server")]
    Server(&'static (dyn Fn<Args, Output = RpcCommandFuture<Output>> + Send + Sync)),
    #[cfg(feature = "client")]
    Client(Arc<dyn RpcTransport + Send + Sync + 'static>, Arc<[u8]>, PhantomData<(Args, Output)>),
}

impl<Args, Output> FnOnce<Args> for RpcCommand<Args, Output>
where
    Args: Tuple + Serialize + DeserializeOwned,
    Output: Serialize + DeserializeOwned,
{
    type Output = Pin<Box<dyn Future<Output = Result<Output, RpcError>> + Send>>;

    extern "rust-call" fn call_once(self, args: Args) -> Self::Output {
        match self {
            #[cfg(feature = "server")]
            RpcCommand::Server(closure) => closure.call_once(args),
            #[cfg(feature = "client")]
            RpcCommand::Client(transport, name, _) => {
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
            #[cfg(feature = "server")]
            RpcCommand::Server(closure) => closure.call_mut(args),
            #[cfg(feature = "client")]
            RpcCommand::Client(transport, name, _) => {
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
            #[cfg(feature = "server")]
            RpcCommand::Server(closure) => Fn::call(&closure, args),
            #[cfg(feature = "client")]
            RpcCommand::Client(transport, name, _) => {
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

pub struct GlobalRpcCommand<Args, Output>
where
    Args: Tuple + Serialize + DeserializeOwned + 'static,
    Output: Serialize + DeserializeOwned + 'static,
{
    pub(crate) name: &'static str,
    #[allow(dead_code)]
    pub(crate) inner: &'static (dyn Fn<Args, Output = RpcCommandFuture<Output>> + Send + Sync),
    pub(crate) command: OnceLock<RpcCommand<Args, Output>>,
}

impl<Args, Output> GlobalRpcCommand<Args, Output>
where
    Args: Tuple + Serialize + DeserializeOwned,
    Output: Serialize + DeserializeOwned,
{
    pub const fn new(
        name: &'static str,
        inner: &'static (dyn Fn<Args, Output = RpcCommandFuture<Output>> + Send + Sync),
    ) -> Self {
        Self {
            name,
            inner,
            command: OnceLock::new(),
        }
    }
    pub fn register(&self) -> &RpcCommand<Args, Output> {
        self.command
            .get_or_init(|| get_global_dispatch().unwrap().register_global(self))
    }
}

impl<Args, Output> Deref for GlobalRpcCommand<Args, Output>
where
    Args: Tuple + Serialize + DeserializeOwned + 'static,
    Output: Serialize + DeserializeOwned + 'static,
{
    type Target = RpcCommand<Args, Output>;

    fn deref(&self) -> &Self::Target {
        self.register()
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
