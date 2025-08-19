#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(tuple_trait)]

use std::sync::OnceLock;

use crate::{dispatch::RpcDispatch, error::RpcError, transport::RpcTransport};

pub mod command;
pub mod dispatch;
pub mod error;
#[cfg(feature = "server")]
mod registry;
pub mod transport;

static GLOBAL_DISPATCH: OnceLock<RpcDispatch> = OnceLock::new();

pub fn init_global_dispatch(transport: impl RpcTransport + 'static) {
    GLOBAL_DISPATCH.get_or_init(|| RpcDispatch::new(transport));
}

pub fn get_global_dispatch() -> Result<&'static RpcDispatch, RpcError> {
    GLOBAL_DISPATCH.get().ok_or(RpcError::MissingDispatch)
}

#[macro_export]
macro_rules! rpc {
    ($($vis:vis async fn $name:ident ($($arg:ident : $arg_type:ty),*) -> Result<$output:ty, $error:ty> $body: block)*) => {
        $($vis async fn $name($($arg: $arg_type),*) -> Result<$output, $crate::error::RpcError> {
            #[cfg(feature = "server")]
            async fn inner($($arg: $arg_type),*) -> Result<$output, $error> $body
            #[cfg(not(feature = "server"))]
            async fn inner($($arg: $arg_type),*) -> Result<$output, $error> { unimplemented!(); }
            static COMMAND: ::std::sync::OnceLock<$crate::command::RpcCommand<($($arg_type,)*), $output>> = ::std::sync::OnceLock::new();
            let command = COMMAND.get_or_init(|| {
                $crate::get_global_dispatch().unwrap().register(concat!(module_path!(), "::", stringify!($name)), inner)
            });
            command($($arg),*).await
        })*
    };
}
