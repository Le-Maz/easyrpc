use std::{marker::Tuple, pin::Pin};

use serde::{Serialize, de::DeserializeOwned};

use crate::error::RpcError;

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

type RpcCommandFuture<Output> = Pin<Box<dyn Future<Output = Result<Output, RpcError>>>>;

pub struct RpcCommand<Args, Output>
where
    Args: Tuple,
{
    inner: Box<dyn Fn(Args) -> RpcCommandFuture<Output>>,
}

impl<Args, Output> RpcCommand<Args, Output>
where
    Args: Tuple,
{
    pub(crate) fn new(inner: Box<dyn Fn(Args) -> RpcCommandFuture<Output>>) -> Self {
        Self { inner }
    }
}

impl<Args, Output> FnOnce<Args> for RpcCommand<Args, Output>
where
    Args: Tuple,
{
    type Output = Pin<Box<dyn Future<Output = Result<Output, RpcError>>>>;

    extern "rust-call" fn call_once(self, args: Args) -> Self::Output {
        (self.inner)(args)
    }
}

impl<Args, Output> FnMut<Args> for RpcCommand<Args, Output>
where
    Args: Tuple,
{
    extern "rust-call" fn call_mut(&mut self, args: Args) -> Self::Output {
        (self.inner)(args)
    }
}

impl<Args, Output> Fn<Args> for RpcCommand<Args, Output>
where
    Args: Tuple,
{
    extern "rust-call" fn call(&self, args: Args) -> Self::Output {
        (self.inner)(args)
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
