#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(tuple_trait)]

pub mod command;
pub mod dispatch;
pub mod error;
#[cfg(feature = "server")]
mod registry;
pub mod transport;
