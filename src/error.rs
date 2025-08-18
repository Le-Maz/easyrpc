#[derive(Debug)]
pub enum RpcError {
    NotFound,
    Deserialization(Box<dyn std::error::Error + Send + Sync>),
    Procedure(Box<dyn std::error::Error + Send + Sync>),
    Serialization(Box<dyn std::error::Error + Send + Sync>),
    Connection(Box<dyn std::error::Error + Send + Sync>),
}

impl<ErrType> From<ErrType> for RpcError
where
    ErrType: std::error::Error + Send + Sync + 'static,
{
    fn from(value: ErrType) -> Self {
        Self::Procedure(Box::new(value))
    }
}
