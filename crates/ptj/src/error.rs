#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    message: String,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

// Bridge transport-core errors into the ptj error type. Foreign trait, foreign
// source, LOCAL target — allowed by the orphan rule. This lets ptj code call
// `transport.collect()?` / `Message::decode(..)?` (which return
// `transport_core::Result`) and get a ptj `Error`. Map by message: transport
// errors are already opaque strings.
impl From<transport_core::Error> for Error {
    fn from(error: transport_core::Error) -> Self {
        Error::new(error.message())
    }
}
