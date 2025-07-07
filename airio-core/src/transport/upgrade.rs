use std::{
    error,
    marker::PhantomData,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};

use airio_stream_select::Negotiated;
use futures::{AsyncRead, AsyncWrite, Stream, TryFuture, future, ready};

use crate::{
    ConnectedPoint, Endpoint, ListenerEvent, PeerId, StreamMuxer, Transport, Upgrade,
    muxing::StreamMuxerBox,
    transport::{Boxed, and_then::AndThen, boxed::boxed},
    upgrade::{UpgradeApply, UpgradeError},
};

#[derive(Clone)]
pub struct Builder<T> {
    inner: T,
}

impl<T> Builder<T>
where
    T: Transport,
    T::Error: 'static,
{
    pub fn new(inner: T) -> Builder<T> {
        Builder { inner }
    }

    /// 使用一个 [`Upgrade`] 对 [`Transport::Output`] 进行身份协商
    /// * I/O upgrade: `C -> (PeerId, D)`.
    /// * 转化 Transport output: `C -> (PeerId, D)`
    pub fn authenticate<C, D, U, E>(
        self,
        upgrade: U,
    ) -> AuthenticatedBuilder<
        AndThen<T, impl FnOnce(C, ConnectedPoint) -> Authenticate<C, U> + Clone>,
    >
    where
        T: Transport<Output = C>,
        C: AsyncRead + AsyncWrite + Unpin,
        D: AsyncRead + AsyncWrite + Unpin,
        U: Upgrade<Negotiated<C>, Output = (PeerId, D), Error = E> + Clone,
        E: error::Error + 'static,
    {
        AuthenticatedBuilder(Builder::new(self.inner.and_then(move |io, endpoint| {
            let inner = if endpoint.is_dialer() {
                UpgradeApply::new_outbound(io, upgrade)
            } else {
                UpgradeApply::new_inbound(io, upgrade)
            };

            Authenticate { inner }
        })))
    }
}

/// 身份认证 Builder
#[derive(Clone)]
pub struct AuthenticatedBuilder<T>(Builder<T>);

impl<T> AuthenticatedBuilder<T>
where
    T: Transport,
    T::Error: 'static,
{
    /// 在一个 [`Transport`] 身份协商后的流应用一个 [`Upgrade`]。
    /// 这将返回一个新的 [`AuthenticatedBuilder`]，其中包含了升级后的流
    /// 使用 [`Upgrade`] 作用 -> `(PeerId, C) -> (PeerId, D)`.
    pub fn apply<C, D, U, E>(self, upgrade: U) -> AuthenticatedBuilder<WithUpgrade<T, U>>
    where
        T: Transport<Output = (PeerId, C)>,
        C: AsyncRead + AsyncWrite + Unpin,
        D: AsyncRead + AsyncWrite + Unpin,
        U: Upgrade<Negotiated<C>, Output = D, Error = E> + Clone,
        E: error::Error + 'static,
    {
        AuthenticatedBuilder(Builder::new(WithUpgrade::new(self.0.inner, upgrade)))
    }

    /// 在一个 [`Transport`] 身份协商后的流应用一个多路复用 [`Upgrade`]。
    /// 实现了一个多路复用的连接升级。
    /// 使用 [`Upgrade`] 作用 -> `(PeerId, C) -> (PeerId, M)`.
    /// M 必须实现了 [`StreamMuxer`]
    pub fn multiplex<C, M, U, E>(
        self,
        upgrade: U,
    ) -> Multiplexed<AndThen<T, impl FnOnce((PeerId, C), ConnectedPoint) -> Multiplex<C, U> + Clone>>
    where
        T: Transport<Output = (PeerId, C)>,
        M: StreamMuxer,
        C: AsyncRead + AsyncWrite + Unpin,
        U: Upgrade<Negotiated<C>, Output = M, Error = E> + Clone,
        E: error::Error + 'static,
    {
        Multiplexed(self.0.inner.and_then(move |(id, io), endpoint| {
            let upgrade = if endpoint.is_dialer() {
                UpgradeApply::new_outbound(io, upgrade)
            } else {
                UpgradeApply::new_inbound(io, upgrade)
            };
            Multiplex {
                peer_id: Some(id),
                upgrade,
            }
        }))
    }
}

/// 身份认证 Future
/// 内部为 [`UpgradeApply`] 结构体
#[pin_project::pin_project]
pub struct Authenticate<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: Upgrade<Negotiated<C>>,
{
    #[pin]
    inner: UpgradeApply<C, U>,
}

impl<C, U> Future for Authenticate<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: Upgrade<Negotiated<C>>,
{
    type Output = <UpgradeApply<C, U> as Future>::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        this.inner.poll(cx)
    }
}

#[derive(Debug, Copy, Clone)]
#[pin_project::pin_project]
pub struct WithUpgrade<T, U> {
    #[pin]
    inner: T,
    upgrade: U,
}

impl<T, U> WithUpgrade<T, U> {
    pub fn new(inner: T, upgrade: U) -> Self {
        WithUpgrade { inner, upgrade }
    }
}

impl<T, C, D, U, E> Transport for WithUpgrade<T, U>
where
    T: Transport<Output = (PeerId, C)>,
    T::Error: 'static,
    C: AsyncRead + AsyncWrite + Unpin,
    U: Upgrade<Negotiated<C>, Output = D, Error = E> + Clone,
    E: error::Error + 'static,
{
    type Output = (PeerId, D);
    type Error = TransportUpgradeError<T::Error, E>;
    type Dialer = UpgradeFuture<T::Dialer, U, C>;
    type ListenerUpgrade = UpgradeFuture<T::ListenerUpgrade, U, C>;
    type Listener = MapListener<T, U>;

    fn connect(&self, addr: SocketAddr) -> Result<Self::Dialer, Self::Error> {
        let fut = self
            .inner
            .connect(addr)
            .map_err(TransportUpgradeError::Transport)?;
        Ok(UpgradeFuture {
            inner_fut: Box::pin(fut),
            role: Endpoint::Dialer,
            upgrade: future::Either::Left(Some(self.upgrade.clone())),
        })
    }

    fn listen(&self, addr: SocketAddr) -> Result<Self::Listener, Self::Error> {
        let listener = self
            .inner
            .listen(addr)
            .map_err(TransportUpgradeError::Transport)?;
        Ok(MapListener {
            inner: listener,
            upgrade: self.upgrade.clone(),
            _phantom: PhantomData,
        })
    }
}

#[pin_project::pin_project]
#[derive(Clone, Debug)]
pub struct MapListener<T, U>
where
    T: Transport,
{
    #[pin]
    inner: T::Listener,
    upgrade: U,
    _phantom: PhantomData<T>,
}

impl<T, C, D, U, E> Stream for MapListener<T, U>
where
    T: Transport<Output = (PeerId, C)>,
    T::Error: 'static,
    C: AsyncRead + AsyncWrite + Unpin,
    U: Upgrade<Negotiated<C>, Output = D, Error = E> + Clone,
{
    type Item =
        ListenerEvent<UpgradeFuture<T::ListenerUpgrade, U, C>, TransportUpgradeError<T::Error, E>>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.as_mut().project();
        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(event)) => {
                let upgrade = self.upgrade.clone();
                let event = event
                    .map_upgrade(move |up| UpgradeFuture {
                        inner_fut: Box::pin(up),
                        role: Endpoint::Listener,
                        upgrade: future::Either::Left(Some(upgrade)),
                    })
                    .map_err(TransportUpgradeError::Transport);

                Poll::Ready(Some(event))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub struct UpgradeFuture<Fut, U, C>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: Upgrade<Negotiated<C>>,
{
    inner_fut: Pin<Box<Fut>>,
    role: Endpoint,
    upgrade: future::Either<Option<U>, (PeerId, UpgradeApply<C, U>)>,
}

impl<Fut, U, C> Unpin for UpgradeFuture<Fut, U, C>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: Upgrade<Negotiated<C>>,
{
}

impl<Fut, U, C, D> Future for UpgradeFuture<Fut, U, C>
where
    Fut: TryFuture<Ok = (PeerId, C)>,
    C: AsyncRead + AsyncWrite + Unpin,
    U: Upgrade<Negotiated<C>, Output = D>,
    U::Error: error::Error,
{
    type Output = Result<(PeerId, D), TransportUpgradeError<Fut::Error, U::Error>>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        loop {
            this.upgrade = match this.upgrade {
                future::Either::Left(ref mut up) => {
                    // 底层的进站升级中Poll出 `(PeerId, C)`.
                    let (peer_id, io) = ready!(this.inner_fut.as_mut().try_poll(cx))
                        .map_err(TransportUpgradeError::Transport)?;
                    let upgrade = up.take().expect("upgrade should be set");
                    // 使用 `UpgradeApply` 来应用升级。
                    let upgrade = match this.role {
                        Endpoint::Dialer => UpgradeApply::new_outbound(io, upgrade),
                        Endpoint::Listener => UpgradeApply::new_inbound(io, upgrade),
                    };
                    future::Either::Right((peer_id, upgrade))
                }
                future::Either::Right((i, ref mut up)) => {
                    let output = ready!(Pin::new(up).try_poll(cx))?;
                    return Poll::Ready(Ok((i, output)));
                }
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TransportUpgradeError<TE, UE> {
    #[error("Transport error: {0}")]
    Transport(TE),
    #[error(transparent)]
    Upgrade(#[from] UpgradeError<UE>),
}

#[derive(Clone)]
#[pin_project::pin_project]
pub struct Multiplexed<T>(#[pin] T);

#[pin_project::pin_project]
pub struct Multiplex<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: Upgrade<Negotiated<C>>,
{
    peer_id: Option<PeerId>,
    #[pin]
    upgrade: UpgradeApply<C, U>,
}

impl<T> Multiplexed<T> {
    pub fn boxed<M>(self) -> Boxed<(PeerId, StreamMuxerBox)>
    where
        T: Transport<Output = (PeerId, M)> + Sized + Send + Unpin + 'static,
        T::Dialer: Send + 'static,
        T::ListenerUpgrade: Send + 'static,
        T::Listener: Send + 'static,
        T::Error: Send + Sync,
        M: StreamMuxer + Send + 'static,
        M::Substream: Send + 'static,
        M::Error: Send + Sync + 'static,
    {
        boxed(self.map(|(i, m), _| (i, StreamMuxerBox::new(m))))
    }
}

impl<T> Transport for Multiplexed<T>
where
    T: Transport,
{
    type Output = T::Output;
    type Error = T::Error;
    type ListenerUpgrade = T::ListenerUpgrade;
    type Dialer = T::Dialer;
    type Listener = T::Listener;

    fn connect(&self, addr: SocketAddr) -> Result<Self::Dialer, Self::Error> {
        self.0.connect(addr)
    }

    fn listen(&self, addr: SocketAddr) -> Result<Self::Listener, Self::Error> {
        self.0.listen(addr)
    }
}

impl<C, U, M, E> Future for Multiplex<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: Upgrade<Negotiated<C>, Output = M, Error = E>,
{
    type Output = Result<(PeerId, M), UpgradeError<E>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        let m = match ready!(Pin::new(&mut this.upgrade).poll(cx)) {
            Ok(m) => m,
            Err(err) => return Poll::Ready(Err(err)),
        };
        let i = this
            .peer_id
            .take()
            .expect("Multiplex future polled after completion.");
        Poll::Ready(Ok((i, m)))
    }
}
