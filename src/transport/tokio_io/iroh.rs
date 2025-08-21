use std::fmt::{self, Display};

use tokio::sync::Mutex;

use tokio::sync::mpsc;

use crate::{error::RpcError, transport::tokio_io::TokioIoConnector};

use super::StreamPair;

use iroh::{
    Endpoint, NodeAddr,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};

#[cfg(feature = "server")]
use iroh::protocol::Router;

pub struct IrohConnector {
    alpn: Vec<u8>,
    address: NodeAddr,

    #[cfg(feature = "client")]
    client_endpoint: Endpoint,

    #[cfg(feature = "client")]
    connection: Mutex<Option<Connection>>,

    #[cfg(feature = "server")]
    #[allow(unused)]
    router: Router,

    #[cfg(feature = "server")]
    incoming_streams: Mutex<mpsc::Receiver<StreamPair>>,
}

impl IrohConnector {
    pub async fn new(
        client_endpoint: impl FnOnce() -> iroh::endpoint::Builder,
        server_endpoint: impl FnOnce() -> iroh::endpoint::Builder,
        address: NodeAddr,
        alpn: Vec<u8>,
    ) -> Result<Self, RpcError> {
        #[cfg(feature = "client")]
        let client_endpoint = Self::init_client(client_endpoint, &alpn).await?;

        #[cfg(feature = "server")]
        let (router, recv) = Self::init_server(server_endpoint, &alpn).await?;

        Ok(Self {
            alpn,
            address,
            #[cfg(feature = "client")]
            client_endpoint,
            #[cfg(feature = "client")]
            connection: Mutex::new(None),
            #[cfg(feature = "server")]
            router,
            #[cfg(feature = "server")]
            incoming_streams: Mutex::new(recv),
        })
    }

    #[cfg(feature = "client")]
    async fn init_client(
        builder: impl FnOnce() -> iroh::endpoint::Builder,
        alpn: &[u8],
    ) -> Result<Endpoint, RpcError> {
        builder()
            .alpns(vec![alpn.to_vec()])
            .bind()
            .await
            .map_err(Into::into)
            .map_err(RpcError::Connection)
    }

    #[cfg(feature = "server")]
    async fn init_server(
        builder: impl FnOnce() -> iroh::endpoint::Builder,
        alpn: &[u8],
    ) -> Result<(Router, mpsc::Receiver<StreamPair>), RpcError> {
        use iroh::Watcher;

        let server_endpoint = builder()
            .alpns(vec![alpn.to_vec()])
            .bind()
            .await
            .map_err(Into::into)
            .map_err(RpcError::Connection)?;

        server_endpoint.node_addr().initialized().await;

        let (send, recv) = mpsc::channel(16);
        let protocol = EasyRpcProtocol::from(send);
        let router = Router::builder(server_endpoint)
            .accept(alpn, protocol)
            .spawn();

        Ok((router, recv))
    }
}

impl TokioIoConnector for IrohConnector {
    #[cfg(feature = "client")]
    async fn connect(&self) -> Result<StreamPair, RpcError> {
        let connection = {
            let mut guard = self.connection.lock().await;

            match &*guard {
                Some(existing) => existing.clone(),
                None => {
                    let conn = self
                        .client_endpoint
                        .connect(self.address.clone(), &self.alpn)
                        .await
                        .map_err(Into::into)
                        .map_err(RpcError::Connection)?;

                    guard.insert(conn).clone()
                }
            }
        };

        let (send, recv) = connection
            .open_bi()
            .await
            .map_err(Into::into)
            .map_err(RpcError::Connection)?;

        Ok((Box::pin(recv), Box::pin(send)))
    }

    #[cfg(feature = "server")]
    async fn accept(&self) -> Result<StreamPair, RpcError> {
        self.incoming_streams
            .lock()
            .await
            .recv()
            .await
            .ok_or_else(|| Box::new(EndpointClosed).into())
            .map_err(RpcError::Connection)
    }
}

#[derive(Debug)]
struct EndpointClosed;

impl Display for EndpointClosed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Endpoint was closed")
    }
}

impl std::error::Error for EndpointClosed {}

#[derive(Debug)]
struct EasyRpcProtocol {
    sender: mpsc::Sender<StreamPair>,
}

impl From<mpsc::Sender<StreamPair>> for EasyRpcProtocol {
    fn from(sender: mpsc::Sender<StreamPair>) -> Self {
        Self { sender }
    }
}

impl ProtocolHandler for EasyRpcProtocol {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        loop {
            let (send, recv) = connection.accept_bi().await?;

            // Exit if channel is closed
            if self
                .sender
                .send((Box::pin(recv), Box::pin(send)))
                .await
                .is_err()
            {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
#[cfg(all(feature = "server", feature = "client"))]
mod tests {
    use std::{
        net::{Ipv4Addr, SocketAddr, SocketAddrV4},
        sync::Arc,
    };

    use crate::transport::{
        HandlerFn, RpcTransport,
        tokio_io::{TokioIoTransport, iroh::IrohConnector},
    };

    use iroh::{NodeAddr, SecretKey, endpoint::Endpoint};
    use rand::rngs::OsRng;

    #[tokio::test]
    async fn iroh_io() {
        // Define the ALPN protocol identifier
        let alpn = b"easy-rpc".to_vec();

        let server_key = SecretKey::generate(&mut OsRng);
        let server_sockaddr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 55555);
        let server_id = server_key.public();

        let connector = IrohConnector::new(
            || Endpoint::builder(),
            move || {
                Endpoint::builder()
                    .bind_addr_v4(server_sockaddr)
                    .secret_key(server_key)
            },
            NodeAddr::new(server_id).with_direct_addresses([SocketAddr::V4(server_sockaddr)]),
            alpn.clone(),
        )
        .await
        .expect("Failed to create IrohConnector (client)");

        // Wrap connector in transports
        let transport = Arc::new(TokioIoTransport::new(connector));

        // Define a simple echo handler
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

        // Spawn server listen task
        let server_clone = transport.clone();
        let server_handle = tokio::spawn(async move {
            server_clone.listen(handler).await.unwrap();
        });

        // Small delay to let the server start
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Send an RPC call from the client
        let name = b"echo".to_vec();
        let data = b"hello world".to_vec();

        let response = transport
            .call(Arc::from(name), data.clone())
            .await
            .expect("RPC call failed");

        let expected = b"response: echo = hello world".to_vec();
        assert_eq!(response, expected);

        // Shutdown the server
        server_handle.abort();
        let _ = server_handle.await;
    }
}
