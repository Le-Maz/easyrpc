use crate::{command::IntoRpcCommand, error::RpcError};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::HashMap,
    io::Cursor,
    marker::Tuple,
    ops::Deref,
    pin::Pin,
    sync::{Arc, RwLock},
};

#[derive(Default)]
pub struct CommandRegistry {
    commands: RwLock<HashMap<Vec<u8>, ServerCommandBox>>,
}

impl CommandRegistry {
    pub fn insert(&self, name: Vec<u8>, command: ServerCommandBox) {
        self.commands.write().unwrap().insert(name, command);
    }

    pub fn get(&self, name: Vec<u8>) -> Option<ServerCommandBox> {
        self.commands.read().unwrap().get(&name).cloned()
    }
}

#[derive(Clone)]
pub struct ServerCommandBox(Arc<dyn Fn(Vec<u8>) -> CommandBoxFuture + Send + Sync>);

impl ServerCommandBox {
    pub fn new<Command, Args, Fut, Output>(command: Command) -> Self
    where
        Command: IntoRpcCommand<Args, Fut, Output> + Clone,
        Args: Tuple + Serialize + DeserializeOwned,
        Fut: Future<Output = Result<Output, RpcError>> + Send,
        Output: Serialize + DeserializeOwned,
    {
        Self(Arc::new(move |args_bytes| {
            let command_clone = command.clone();
            Box::pin(async move {
                let args = ciborium::from_reader(Cursor::new(args_bytes))
                    .map_err(Into::into)
                    .map_err(RpcError::Deserialization)?;
                let output = command_clone.call(args).await?;
                let mut output_bytes = Vec::new();
                ciborium::into_writer(&output, &mut output_bytes)
                    .map_err(Into::into)
                    .map_err(RpcError::Serialization)?;
                Ok(output_bytes)
            })
        }))
    }
}

impl Deref for ServerCommandBox {
    type Target = dyn Fn(Vec<u8>) -> CommandBoxFuture;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

type CommandBoxFuture = Pin<Box<dyn Future<Output = Result<Vec<u8>, RpcError>> + Send>>;
