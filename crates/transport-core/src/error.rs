//! The shared error type for the transport layer.
//!
//! Lifted verbatim (same shape) from the ptj CLI's `crate::error` so that every
//! `transport-<name>` crate can share one `Error`/`Result` through the hub
//! without any of them depending on ptj. It is a deliberately minimal,
//! stringly-typed error: a transport moves opaque bytes and only ever needs to
//! report "the wire said something I could not make sense of".

/// A transport-layer error carrying a human-readable message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    message: String,
}

/// The transport-layer result alias, re-exported at the crate root.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Build an error from anything that can become a `String`.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Borrow the human-readable message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// Convenience conversion so stream transports can bubble `std::io::Error`
/// (from the `Read`/`Write` framing helpers) up as a transport `Error` with `?`.
impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::new(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_the_message() {
        let err = Error::new("boom");
        assert_eq!(err.to_string(), "boom");
        assert_eq!(err.message(), "boom");
    }

    #[test]
    fn from_io_error_preserves_text() {
        let io = std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "short read");
        let err: Error = io.into();
        assert_eq!(err.to_string(), "short read");
    }
}
