use quinn::{ConnectError, ConnectionError};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Connect(#[from] ConnectError),

    #[error(transparent)]
    Connection(#[from] ConnectionError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("Handshake with the remote timed out.")]
    HandshakeTimedOut,
}
