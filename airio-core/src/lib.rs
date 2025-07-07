pub mod connection;
pub mod either;
mod extensions;
mod identity;
pub mod muxing;
pub mod transport;
pub mod upgrade;
pub mod utils;

pub use connection::{ConnectedPoint, Endpoint};
pub use extensions::Extensions;
pub use identity::PeerId;
pub use muxing::StreamMuxer;
pub use transport::{ListenerEvent, Transport};
pub use upgrade::{Upgrade, UpgradeInfo};

pub type Negotiated<T> = airio_stream_select::Negotiated<T>;
