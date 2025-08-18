use std::net::SocketAddr;

use tokio::{
    net::{TcpListener, TcpStream},
    sync::Mutex,
};

#[cfg(feature = "client")]
use crate::transport::tokio_io::StreamPair;
use crate::{error::RpcError, transport::tokio_io::TokioIoConnector};

pub struct TokioTcpConnector {
    #[cfg(feature = "server")]
    listener: Mutex<TcpListener>,
    #[cfg(feature = "client")]
    address: SocketAddr,
}

impl TokioTcpConnector {
    pub fn new(listener: Mutex<TcpListener>, address: SocketAddr) -> Self {
        Self {
            #[cfg(feature = "server")]
            listener,
            #[cfg(feature = "client")]
            address,
        }
    }
}

impl TokioIoConnector for TokioTcpConnector {
    #[cfg(feature = "server")]
    async fn accept(&self) -> Result<StreamPair, RpcError> {
        let (stream, _) = self
            .listener
            .lock()
            .await
            .accept()
            .await
            .map_err(Into::into)
            .map_err(RpcError::Connection)?;
        let (read_half, write_half) = tokio::io::split(stream);
        Ok((Box::pin(read_half), Box::pin(write_half)))
    }

    #[cfg(feature = "client")]
    async fn connect(&self) -> Result<StreamPair, RpcError> {
        let stream = TcpStream::connect(self.address)
            .await
            .map_err(Into::into)
            .map_err(RpcError::Connection)?;
        let (read_half, write_half) = tokio::io::split(stream);
        Ok((Box::pin(read_half), Box::pin(write_half)))
    }
}

#[cfg(test)]
#[cfg(all(feature = "server", feature = "client"))]
mod tests {
    use std::sync::Arc;

    use tokio::{net::TcpListener, sync::Mutex};

    use crate::transport::{
        HandlerFn, RpcTransport, tokio_io::TokioIoTransport, tokio_tcp::TokioTcpConnector,
    };

    #[tokio::test]
    async fn tokio_io() {
        // Start a server on a random port (port 0)
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind server");
        let address = listener.local_addr().unwrap();

        // Create the server connector and wrap it in TokioIoTransport
        let connector = TokioTcpConnector {
            listener: Mutex::new(listener),
            address: address,
        };
        let transport = Arc::new(TokioIoTransport::new(connector));

        // Create a handler that echoes back the request data with a prefix
        let handler: HandlerFn = Arc::new(move |name, data| {
            Box::pin(async move {
                let mut response = Vec::new();
                response.extend(b"response: ");
                response.extend(&name);
                response.extend(b" = ");
                response.extend(&data);
                Ok(response)
            })
        });

        // Start server in background
        let transport_clone = transport.clone();
        let handle = tokio::spawn(async move {
            transport_clone.listen(handler).await.unwrap();
        });

        // Small delay to ensure the server is ready
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Send an RPC call
        let name = b"echo".to_vec();
        let data = b"hello world".to_vec();

        let response = transport
            .call(Arc::from(name), data.clone())
            .await
            .expect("RPC call failed");

        let expected = b"response: echo = hello world".to_vec();
        assert_eq!(response, expected);

        handle.abort();
        let _ = handle.await;
    }
}
