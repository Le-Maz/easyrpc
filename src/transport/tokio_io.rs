use std::{pin::Pin, sync::Arc};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, error};

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
    #[instrument(skip_all, fields(name = %String::from_utf8_lossy(&name)))]
    async fn call_inner(
        self: Arc<Self>,
        name: Arc<[u8]>,
        data: Vec<u8>,
    ) -> Result<Vec<u8>, RpcError> {
        let (mut read_half, mut write_half) = match self.connector.connect().await {
            Ok(conn) => {
                debug!("Connection established.");
                conn
            }
            Err(e) => {
                error!("Failed to connect: {:?}", e);
                return Err(RpcError::Connection(e.into()));
            }
        };

        if let Err(e) = write_half.write_u16(name.len() as u16).await {
            error!("Failed to write name length: {:?}", e);
            return Err(RpcError::Connection(e.into()));
        }

        if let Err(e) = write_half.write_u32(data.len() as u32).await {
            error!("Failed to write data length: {:?}", e);
            return Err(RpcError::Connection(e.into()));
        }

        if let Err(e) = write_half.write_all(&name).await {
            error!("Failed to write name bytes: {:?}", e);
            return Err(RpcError::Connection(e.into()));
        }

        if let Err(e) = write_half.write_all(&data).await {
            error!("Failed to write data bytes: {:?}", e);
            return Err(RpcError::Connection(e.into()));
        }

        if let Err(e) = write_half.flush().await {
            error!("Failed to flush writer: {:?}", e);
            return Err(RpcError::Connection(e.into()));
        }

        let response_len = match read_half.read_u32().await {
            Ok(len) => {
                debug!("Response length received: {}", len);
                len
            }
            Err(e) => {
                error!("Failed to read response length: {:?}", e);
                return Err(RpcError::Connection(e.into()));
            }
        };

        let mut response = vec![0; response_len as usize];
        if let Err(e) = read_half.read(&mut response).await {
            error!("Failed to read response data: {:?}", e);
            return Err(RpcError::Connection(e.into()));
        }

        // Optional EOT signal
        if let Err(e) = write_half.write_u8(4).await {
            debug!("Failed to write EOT (ignored): {:?}", e);
        }

        Ok(response)
    }

    #[cfg(feature = "server")]
    #[inline(always)]
    #[instrument(skip_all)]
    async fn listen_inner(
        self: Arc<Self>,
        handler: HandlerFn,
    ) -> Result<(), crate::error::RpcError> {
        loop {
            let (mut read_half, mut write_half) = match self.connector.accept().await {
                Ok(conn) => {
                    debug!("Accepted new connection");
                    conn
                }
                Err(e) => {
                    error!("Failed to accept connection: {:?}", e);
                    return Err(crate::error::RpcError::Connection(e.into()));
                }
            };

            let handler = handler.clone();
            tokio::spawn(async move {
                let name_len = match read_half.read_u16().await {
                    Ok(len) => len,
                    Err(e) => {
                        error!("Failed to read name length: {:?}", e);
                        return;
                    }
                };

                let data_len = match read_half.read_u32().await {
                    Ok(len) => len,
                    Err(e) => {
                        error!("Failed to read data length: {:?}", e);
                        return;
                    }
                };

                let mut name_buf = vec![0; name_len as usize];
                if let Err(e) = read_half.read_exact(&mut name_buf).await {
                    error!("Failed to read name buffer: {:?}", e);
                    return;
                }

                let mut data_buf = vec![0; data_len as usize];
                if let Err(e) = read_half.read_exact(&mut data_buf).await {
                    error!("Failed to read data buffer: {:?}", e);
                    return;
                }

                let response = match handler(name_buf.clone(), data_buf).await {
                    Ok(resp) => resp,
                    Err(e) => {
                        error!("Handler returned error: {:?}", e);
                        return;
                    }
                };

                if let Err(e) = write_half.write_u32(response.len() as u32).await {
                    error!("Failed to write response length: {:?}", e);
                    return;
                }

                if let Err(e) = write_half.write_all(&response).await {
                    error!("Failed to write response data: {:?}", e);
                    return;
                }

                if let Err(e) = write_half.flush().await {
                    error!("Failed to flush write stream: {:?}", e);
                    return;
                }

                if let Err(e) = read_half.read_u8().await {
                    debug!("Failed to read EOT (ignored): {:?}", e);
                }

                debug!(
                    "Successfully handled RPC: name = {}",
                    String::from_utf8_lossy(&name_buf)
                );
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
