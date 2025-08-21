use std::{pin::Pin, sync::Arc};

use tokio::io::{AsyncRead, AsyncWrite};

#[cfg(feature = "server")]
use crate::transport::HandlerFn;
use crate::{error::RpcError, transport::RpcTransport};

pub struct TokioIoTransport<Connector>
where
    Connector: TokioIoConnector + Send + Sync,
{
    connector: Connector,
}

impl<Connector> TokioIoTransport<Connector>
where
    Connector: TokioIoConnector + Send + Sync,
{
    pub fn new(connector: Connector) -> Self {
        Self { connector }
    }

    #[cfg(feature = "client")]
    #[inline(always)]
    async fn call_inner(
        self: Arc<Self>,
        name: Arc<[u8]>,
        data: Vec<u8>,
    ) -> Result<Vec<u8>, RpcError> {
        let (mut read_half, mut write_half) = self.connector.connect().await?;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        write_half
            .write_u16(name.len() as u16)
            .await
            .map_err(Into::into)
            .map_err(RpcError::Connection)?;
        write_half
            .write_u32(data.len() as u32)
            .await
            .map_err(Into::into)
            .map_err(RpcError::Connection)?;

        write_half
            .write_all(&name)
            .await
            .map_err(Into::into)
            .map_err(RpcError::Connection)?;
        write_half
            .write_all(&data)
            .await
            .map_err(Into::into)
            .map_err(RpcError::Connection)?;

        write_half
            .flush()
            .await
            .map_err(Into::into)
            .map_err(RpcError::Connection)?;

        let response_len = read_half
            .read_u32()
            .await
            .map_err(Into::into)
            .map_err(RpcError::Connection)?;
        let mut response = vec![0; response_len as usize];
        read_half
            .read(&mut response)
            .await
            .map_err(Into::into)
            .map_err(RpcError::Connection)?;

        const EOT: u8 = 4;
        let _ = write_half.write_u8(EOT).await;

        Ok(response)
    }

    #[cfg(feature = "server")]
    #[inline(always)]
    async fn listen_inner(
        self: Arc<Self>,
        handler: HandlerFn,
    ) -> Result<(), crate::error::RpcError> {
        loop {
            let (mut read_half, mut write_half) = self.connector.accept().await?;
            let handler = handler.clone();
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};

                let Ok(name_len) = read_half.read_u16().await else {
                    return;
                };
                let Ok(data_len) = read_half.read_u32().await else {
                    return;
                };

                let mut name_buf = vec![0; name_len as usize];
                let Ok(_) = read_half.read_exact(&mut name_buf).await else {
                    return;
                };
                let mut data_buf = vec![0; data_len as usize];
                let Ok(_) = read_half.read_exact(&mut data_buf).await else {
                    return;
                };

                let Ok(response) = handler(name_buf, data_buf).await else {
                    return;
                };

                let Ok(_) = write_half.write_u32(response.len() as u32).await else {
                    return;
                };
                let Ok(_) = write_half.write_all(&response).await else {
                    return;
                };

                let Ok(_) = write_half.flush().await else {
                    return;
                };
                let _ = read_half.read_u8().await;
            });
        }
    }
}

impl<Connector> RpcTransport for TokioIoTransport<Connector>
where
    Connector: TokioIoConnector + Send + Sync + 'static,
{
    #[cfg(feature = "client")]
    fn call(
        self: Arc<Self>,
        name: Arc<[u8]>,
        data: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, RpcError>> + Send>> {
        Box::pin(self.call_inner(name, data))
    }

    #[cfg(feature = "server")]
    fn listen(
        self: Arc<Self>,
        handler: HandlerFn,
    ) -> Pin<Box<dyn Future<Output = Result<(), RpcError>> + Send>> {
        Box::pin(self.listen_inner(handler))
    }
}

pub trait TokioIoConnector {
    #[cfg(feature = "server")]
    fn accept(&self) -> impl Future<Output = Result<StreamPair, RpcError>> + Send + Sync;
    #[cfg(feature = "client")]
    fn connect(&self) -> impl Future<Output = Result<StreamPair, RpcError>> + Send + Sync;
}

pub type StreamPair = (
    Pin<Box<dyn AsyncRead + Send + Sync>>,
    Pin<Box<dyn AsyncWrite + Send + Sync>>,
);

#[cfg(feature = "tokio-tcp-connector")]
pub mod tcp;

#[cfg(feature = "iroh-connector")]
pub mod iroh;
