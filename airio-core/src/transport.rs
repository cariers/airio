pub mod and_then;
pub mod map;
pub mod map_err;
pub mod upgrade;

mod boxed;

use std::{error, fmt, net::SocketAddr};

use futures::{Stream, TryFuture, TryFutureExt, future};

use crate::ConnectedPoint;

pub use boxed::Boxed;

pub trait Transport {
    type Output;
    type Error: error::Error;
    type Dialer: Future<Output = Result<Self::Output, Self::Error>>;
    type ListenerUpgrade: Future<Output = Result<Self::Output, Self::Error>>;
    type Listener: Stream<Item = ListenerEvent<Self::ListenerUpgrade, Self::Error>>;

    fn listen(&self, addr: SocketAddr) -> Result<Self::Listener, Self::Error>;

    fn connect(&self, addr: SocketAddr) -> Result<Self::Dialer, Self::Error>;

    fn map<F, O>(self, f: F) -> map::Map<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Output, ConnectedPoint) -> O,
    {
        map::Map::new(self, f)
    }

    fn and_then<TMap, TFut, O>(self, f: TMap) -> and_then::AndThen<Self, TMap>
    where
        Self: Sized,
        TMap: FnOnce(Self::Output, ConnectedPoint) -> TFut,
        TFut: TryFuture<Ok = O>,
        <TFut as TryFuture>::Error: error::Error + 'static,
    {
        and_then::AndThen::new(self, f)
    }

    fn map_err<F, O>(self, f: F) -> map_err::MapErr<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Error) -> O,
        O: error::Error + 'static,
    {
        map_err::MapErr::new(self, f)
    }

    fn upgrade(self) -> upgrade::Builder<Self>
    where
        Self: Sized,
        Self::Error: 'static,
    {
        upgrade::Builder::new(self)
    }
}

pub enum ListenerEvent<T, E> {
    Listened(SocketAddr),
    Incoming {
        local_addr: SocketAddr,
        remote_addr: SocketAddr,
        upgrade: T,
    },
    Closed(Result<(), E>),
    Error(E),
}

impl<U, E> ListenerEvent<U, E> {
    pub fn map_upgrade<F, O>(self, f: F) -> ListenerEvent<O, E>
    where
        F: FnOnce(U) -> O,
    {
        match self {
            ListenerEvent::Listened(addr) => ListenerEvent::Listened(addr),
            ListenerEvent::Incoming {
                local_addr,
                remote_addr,
                upgrade,
            } => ListenerEvent::Incoming {
                local_addr,
                remote_addr,
                upgrade: f(upgrade),
            },
            ListenerEvent::Closed(result) => ListenerEvent::Closed(result),
            ListenerEvent::Error(err) => ListenerEvent::Error(err),
        }
    }

    pub fn map_err<F, O>(self, f: F) -> ListenerEvent<U, O>
    where
        F: FnOnce(E) -> O,
    {
        match self {
            ListenerEvent::Listened(addr) => ListenerEvent::Listened(addr),
            ListenerEvent::Incoming {
                local_addr,
                remote_addr,
                upgrade,
            } => ListenerEvent::Incoming {
                local_addr,
                remote_addr,
                upgrade,
            },
            ListenerEvent::Closed(result) => ListenerEvent::Closed(result.map_err(f)),
            ListenerEvent::Error(err) => ListenerEvent::Error(f(err)),
        }
    }

    pub fn map_upgrade_err<F, O>(self, f: F) -> ListenerEvent<future::MapErr<U, F>, E>
    where
        U: TryFuture<Error = E>,
        F: FnOnce(E) -> O,
    {
        match self {
            ListenerEvent::Listened(addr) => ListenerEvent::Listened(addr),
            ListenerEvent::Incoming {
                local_addr,
                remote_addr,
                upgrade,
            } => ListenerEvent::Incoming {
                local_addr,
                remote_addr,
                upgrade: upgrade.map_err(f),
            },
            ListenerEvent::Closed(result) => ListenerEvent::Closed(result),
            ListenerEvent::Error(err) => ListenerEvent::Error(err),
        }
    }
}

impl<U, E> fmt::Debug for ListenerEvent<U, E>
where
    E: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ListenerEvent::Listened(addr) => write!(f, "Listened({})", addr),
            ListenerEvent::Incoming {
                local_addr,
                remote_addr,
                upgrade: _,
            } => write!(
                f,
                "Incoming(local: {}, remote: {})",
                local_addr, remote_addr
            ),
            ListenerEvent::Closed(result) => write!(f, "Closed({:?})", result),
            ListenerEvent::Error(err) => write!(f, "Error({:?})", err),
        }
    }
}
