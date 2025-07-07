use std::{
    marker::PhantomData,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{Stream, TryFuture};

use crate::{ConnectedPoint, ListenerEvent, Transport};

/// 将 Transport 的输出使用函数映射到另一个类型。
#[derive(Debug, Copy, Clone)]
#[pin_project::pin_project]
pub struct Map<T, TMap> {
    #[pin]
    transport: T,
    fun: TMap,
}

impl<T, TMap> Map<T, TMap> {
    pub(crate) fn new(transport: T, fun: TMap) -> Self {
        Map { transport, fun }
    }

    pub fn inner(&self) -> &T {
        &self.transport
    }
}

impl<T, TMap, O> Transport for Map<T, TMap>
where
    T: Transport,
    TMap: FnOnce(T::Output, ConnectedPoint) -> O + Clone,
{
    type Output = O;
    type Error = T::Error;
    type ListenerUpgrade = MapFuture<T::ListenerUpgrade, TMap>;
    type Dialer = MapFuture<T::Dialer, TMap>;
    type Listener = MapListener<T, TMap>;

    fn connect(&self, addr: SocketAddr) -> Result<Self::Dialer, Self::Error> {
        let dialer = self.transport.connect(addr)?;
        let connected_point = ConnectedPoint::Dialer { addr };
        Ok(MapFuture {
            inner: dialer,
            args: Some((self.fun.clone(), connected_point)),
        })
    }

    fn listen(&self, addr: SocketAddr) -> Result<Self::Listener, Self::Error> {
        let listener = self.transport.listen(addr)?;
        Ok(MapListener {
            inner: listener,
            fun: self.fun.clone(),
            _phantom: PhantomData,
        })
    }
}

#[pin_project::pin_project]
#[derive(Clone, Debug)]
pub struct MapListener<T, TMap>
where
    T: Transport,
{
    #[pin]
    inner: T::Listener,
    fun: TMap,
    _phantom: PhantomData<T>,
}

impl<D, T, TMap> Stream for MapListener<T, TMap>
where
    T: Transport,
    TMap: FnOnce(T::Output, ConnectedPoint) -> D + Clone,
{
    type Item = ListenerEvent<MapFuture<T::ListenerUpgrade, TMap>, T::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        let event = match Pin::new(&mut this.inner).as_mut().poll_next(cx) {
            Poll::Ready(Some(event)) => match event {
                ListenerEvent::Listened(addr) => ListenerEvent::Listened(addr),
                ListenerEvent::Incoming {
                    local_addr,
                    remote_addr,
                    upgrade,
                } => ListenerEvent::Incoming {
                    local_addr,
                    remote_addr,
                    upgrade: MapFuture {
                        inner: upgrade,
                        args: Some((
                            this.fun.clone(),
                            ConnectedPoint::Listener {
                                local_addr,
                                remote_addr,
                            },
                        )),
                    },
                },
                ListenerEvent::Closed(result) => ListenerEvent::Closed(result),
                ListenerEvent::Error(err) => ListenerEvent::Error(err),
            },
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending => return Poll::Pending,
        };

        return Poll::Ready(Some(event));
    }
}

#[pin_project::pin_project]
#[derive(Clone, Debug)]
pub struct MapFuture<T, F> {
    #[pin]
    inner: T,
    args: Option<(F, ConnectedPoint)>,
}

impl<T, A, F, B> Future for MapFuture<T, F>
where
    T: TryFuture<Ok = A>,
    F: FnOnce(A, ConnectedPoint) -> B,
{
    type Output = Result<B, T::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let item = match TryFuture::try_poll(this.inner, cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(v)) => v,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
        };
        let (f, a) = this.args.take().expect("MapFuture has already finished.");
        Poll::Ready(Ok(f(item, a)))
    }
}
