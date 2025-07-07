use std::{
    error,
    marker::{PhantomData, PhantomPinned},
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};

use either::Either;
use futures::{Stream, TryFuture};

use crate::{ConnectedPoint, ListenerEvent, Transport};

/// 在T::Output上应用函数C, 生成一个新的 Future<Output = D>。
#[pin_project::pin_project]
#[derive(Debug, Clone)]
pub struct AndThen<T, TMap> {
    #[pin]
    transport: T,
    map: TMap,
}

impl<T, TMap> AndThen<T, TMap> {
    pub(crate) fn new(transport: T, fun: TMap) -> Self {
        AndThen {
            transport,
            map: fun,
        }
    }
}

impl<T, TMap, TFut, O> Transport for AndThen<T, TMap>
where
    T: Transport,
    TMap: FnOnce(T::Output, ConnectedPoint) -> TFut + Clone,
    TFut: TryFuture<Ok = O>,
    TFut::Error: error::Error,
{
    type Output = O;
    type Error = Either<T::Error, TFut::Error>;
    type ListenerUpgrade = AndThenFuture<T::ListenerUpgrade, TMap, TFut>;
    type Dialer = AndThenFuture<T::Dialer, TMap, TFut>;
    type Listener = AndThenListener<T, TMap>;

    fn connect(&self, addr: SocketAddr) -> Result<Self::Dialer, Self::Error> {
        let dialer = self.transport.connect(addr).map_err(Either::Left)?;
        let connected_point = ConnectedPoint::Dialer { addr };
        Ok(AndThenFuture {
            inner: Either::Left(Box::pin(dialer)),
            args: Some((self.map.clone(), connected_point)),
            _marker: PhantomPinned,
        })
    }

    fn listen(&self, addr: SocketAddr) -> Result<Self::Listener, Self::Error> {
        let listener = self.transport.listen(addr).map_err(Either::Left)?;
        Ok(AndThenListener {
            inner: listener,
            map: self.map.clone(),
            _phantom: PhantomData,
        })
    }
}

#[pin_project::pin_project]
#[derive(Clone, Debug)]
pub struct AndThenListener<T, TMap>
where
    T: Transport,
{
    #[pin]
    inner: T::Listener,
    map: TMap,
    _phantom: PhantomData<T>,
}

impl<O, T, TMap, TFut> Stream for AndThenListener<T, TMap>
where
    T: Transport,
    TMap: FnOnce(T::Output, ConnectedPoint) -> TFut + Clone,
    TFut: TryFuture<Ok = O>,
    TFut::Error: error::Error,
{
    type Item =
        ListenerEvent<AndThenFuture<T::ListenerUpgrade, TMap, TFut>, Either<T::Error, TFut::Error>>;

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
                    upgrade: AndThenFuture {
                        inner: Either::Left(Box::pin(upgrade)),
                        args: Some((
                            this.map.clone(),
                            ConnectedPoint::Listener {
                                local_addr,
                                remote_addr,
                            },
                        )),
                        _marker: PhantomPinned,
                    },
                },
                ListenerEvent::Closed(result) => {
                    ListenerEvent::Closed(result.map_err(Either::Left))
                }
                ListenerEvent::Error(err) => ListenerEvent::Error(Either::Left(err)),
            },
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending => return Poll::Pending,
        };

        return Poll::Ready(Some(event));
    }
}

#[derive(Debug)]
pub struct AndThenFuture<TFut, TMap, TMapFut> {
    inner: Either<Pin<Box<TFut>>, Pin<Box<TMapFut>>>,
    args: Option<(TMap, ConnectedPoint)>,
    _marker: PhantomPinned,
}

impl<TFut, TMap, TMapFut> Unpin for AndThenFuture<TFut, TMap, TMapFut> {}

impl<TFut, TMap, TMapFut> Future for AndThenFuture<TFut, TMap, TMapFut>
where
    TFut: TryFuture,
    TMapFut: TryFuture,
    TMap: FnOnce(TFut::Ok, ConnectedPoint) -> TMapFut,
{
    type Output = Result<TMapFut::Ok, Either<TFut::Error, TMapFut::Error>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let future = match &mut self.inner {
                Either::Left(fut) => {
                    let output = match fut.as_mut().try_poll(cx) {
                        Poll::Ready(Ok(v)) => v,
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(Either::Left(e))),
                        Poll::Pending => return Poll::Pending,
                    };
                    let (map, connection_point) = self.args.take().expect("args should be set");
                    map(output, connection_point)
                }
                Either::Right(fut) => {
                    return match fut.as_mut().try_poll(cx) {
                        Poll::Ready(Ok(v)) => Poll::Ready(Ok(v)),
                        Poll::Ready(Err(e)) => Poll::Ready(Err(Either::Right(e))),
                        Poll::Pending => Poll::Pending,
                    };
                }
            };
            self.inner = Either::Right(Box::pin(future));
        }
    }
}
