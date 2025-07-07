use std::{
    error,
    marker::PhantomData,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{Stream, TryFutureExt, future};

use crate::{ListenerEvent, Transport};

/// 将对 Transport的错误类型进行映射。
#[derive(Debug, Copy, Clone)]
#[pin_project::pin_project]
pub struct MapErr<T, TMap> {
    #[pin]
    transport: T,
    fun: TMap,
}

impl<T, TMap> MapErr<T, TMap> {
    pub(crate) fn new(transport: T, fun: TMap) -> Self {
        MapErr { transport, fun }
    }
}

impl<T, TMap, TErr> Transport for MapErr<T, TMap>
where
    T: Transport,
    TMap: FnOnce(T::Error) -> TErr + Clone,
    TErr: error::Error,
{
    type Output = T::Output;
    type Error = TErr;
    type ListenerUpgrade = future::MapErr<T::ListenerUpgrade, TMap>;
    type Dialer = future::MapErr<T::Dialer, TMap>;
    type Listener = MapErrListener<T, TMap>;

    fn connect(&self, addr: SocketAddr) -> Result<Self::Dialer, Self::Error> {
        let map = self.fun.clone();
        let dialer = self.transport.connect(addr).map_err(map)?;
        Ok(dialer.map_err(self.fun.clone()))
    }

    fn listen(&self, addr: SocketAddr) -> Result<Self::Listener, Self::Error> {
        let listener = self.transport.listen(addr).map_err(self.fun.clone())?;
        Ok(MapErrListener {
            inner: listener,
            map: self.fun.clone(),
            _phantom: PhantomData,
        })
    }
}

#[pin_project::pin_project]
#[derive(Clone, Debug)]
pub struct MapErrListener<T, TMap>
where
    T: Transport,
{
    #[pin]
    inner: T::Listener,
    map: TMap,
    _phantom: PhantomData<T>,
}

impl<T, TMap, TErr> Stream for MapErrListener<T, TMap>
where
    T: Transport,
    TMap: FnOnce(T::Error) -> TErr + Clone,
{
    type Item = ListenerEvent<future::MapErr<T::ListenerUpgrade, TMap>, TErr>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        let event = match Pin::new(&mut this.inner).as_mut().poll_next(cx) {
            Poll::Ready(Some(event)) => event
                .map_upgrade_err(this.map.clone())
                .map_err(this.map.clone()),
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending => return Poll::Pending,
        };
        return Poll::Ready(Some(event));
    }
}
