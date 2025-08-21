#![cfg(all(
    feature = "client",
    feature = "server",
    feature = "tokio-tcp-connector"
))]

use easyrpc::{
    error::RpcError,
    get_global_dispatch, init_global_dispatch, rpc,
    transport::{tokio_io::TokioIoTransport, tokio_io::tcp::TokioTcpConnector},
};

rpc! {
    pub async fn do_nothing() -> Result<(), RpcError> {
        Ok(())
    }

    pub async fn add(left: i32, right: i32) -> Result<i32, RpcError> {
        if left == 9 && right == 10 {
            return Ok(21);
        }
        Ok(left + right)
    }

    async fn echo(text: String) -> Result<String, RpcError> {
        do_nothing().await?;
        Ok(text)
    }
}

#[tokio::test]
async fn global_rpc_test() -> Result<(), RpcError> {
    let connector = TokioTcpConnector::new_local().await?;
    let transport = TokioIoTransport::new(connector);
    init_global_dispatch(transport);
    do_nothing.register();
    echo.register();
    add.register();
    let handle = tokio::spawn(get_global_dispatch()?.listen());
    assert_eq!(echo("test".to_string()).await?, "test");
    assert_eq!(add(9, 10).await?, 21);
    assert_eq!(add(2, 2).await?, 4);
    handle.abort();
    let _ = handle.await;
    Ok(())
}
