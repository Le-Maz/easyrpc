use std::pin::Pin;
use std::sync::Arc;

use crate::error::RpcError;

pub trait RpcTransport: Send + Sync + 'static {
    #[cfg(feature = "client")]
    fn call(self: Arc<Self>, name: Arc<[u8]>, data: Vec<u8>)
    -> Pin<Box<dyn Future<Output = Result<Vec<u8>, RpcError>> + Send>>;
    #[cfg(feature = "server")]
    fn listen(self: Arc<Self>, handler: HandlerFn) -> Pin<Box<dyn Future<Output = Result<(), RpcError>> + Send>>;
}

#[cfg(feature = "server")]
type HandlerFn = Arc<dyn Fn(Vec<u8>, Vec<u8>) -> HandlerFut + Send + Sync + 'static>;
#[cfg(feature = "server")]
type HandlerFut = Pin<Box<dyn Future<Output = Result<Vec<u8>, RpcError>> + Send + Sync + 'static>>;

#[cfg(feature = "tokio-io-transport")]
pub mod tokio_io;
#[cfg(feature = "tokio-tcp-connector")]
pub mod tokio_tcp;
