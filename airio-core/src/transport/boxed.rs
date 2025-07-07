use crate::{ListenerEvent, Transport};
use futures::{Stream, StreamExt, TryFutureExt};
use std::{error, io, net::SocketAddr, pin::Pin};

pub struct Boxed<O> {
    inner: Box<dyn Abstract<O> + Send + Unpin>,
}

trait Abstract<O> {
    fn connect(&self, addr: SocketAddr) -> io::Result<Dial<O>>;
    fn listen(&self, addr: SocketAddr) -> io::Result<BoxedListener<O>>;
}

impl<T, O> Abstract<O> for T
where
    T: Transport<Output = O> + 'static,
    T::Error: Send + Sync,
    T::Dialer: Send + 'static,
    T::ListenerUpgrade: Send + 'static,
    T::Listener: Send,
{
    fn connect(&self, addr: SocketAddr) -> io::Result<Dial<O>> {
        let fut = Transport::connect(self, addr)
            .map_err(box_err)?
            .map_err(|e| box_err(e));
        Ok(Box::pin(fut) as Dial<O>)
    }

    fn listen(&self, addr: SocketAddr) -> io::Result<BoxedListener<O>> {
        let listener = Transport::listen(self, addr).map_err(box_err)?;
        let stream = listener
            .map(|event| {
                event
                    .map_upgrade_err(box_err)
                    .map_err(box_err)
                    .map_upgrade(|up| Box::pin(up) as ListenerUpgrade<O>)
            })
            .boxed();
        Ok(stream)
    }
}

type Dial<O> = Pin<Box<dyn Future<Output = io::Result<O>> + Send>>;
type ListenerUpgrade<O> = Pin<Box<dyn Future<Output = io::Result<O>> + Send>>;
type BoxedListener<O> =
    Pin<Box<dyn Stream<Item = ListenerEvent<ListenerUpgrade<O>, io::Error>> + Send>>;

impl<O> Transport for Boxed<O> {
    type Output = O;
    type Error = io::Error;
    type ListenerUpgrade = ListenerUpgrade<O>;
    type Dialer = Dial<O>;
    type Listener = BoxedListener<O>;

    fn connect(&self, addr: SocketAddr) -> Result<Self::Dialer, Self::Error> {
        self.inner.connect(addr)
    }

    fn listen(&self, addr: SocketAddr) -> Result<Self::Listener, Self::Error> {
        self.inner.listen(addr)
    }
}

fn box_err<E: error::Error + Send + Sync + 'static>(e: E) -> io::Error {
    io::Error::other(e)
}

pub(crate) fn boxed<T>(transport: T) -> Boxed<T::Output>
where
    T: Transport + Send + Unpin + 'static,
    T::Error: Send + Sync,
    T::Dialer: Send + 'static,
    T::ListenerUpgrade: Send + 'static,
    T::Listener: Send,
{
    Boxed {
        inner: Box::new(transport) as Box<_>,
    }
}
