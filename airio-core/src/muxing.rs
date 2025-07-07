use futures::{AsyncRead, AsyncWrite};
use std::{
    pin::Pin,
    task::{Context, Poll},
};

mod boxed;

pub use boxed::{StreamMuxerBox, SubstreamBox};

pub trait StreamMuxer {
    type Substream: AsyncRead + AsyncWrite;
    type Error: std::error::Error;

    /// Poll 进站子流
    fn poll_inbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>>;

    /// Poll 出站子流
    fn poll_outbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>>;

    /// Poll 关闭多路复用器
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;

    /// Poll 多路复用器事件
    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>>;
}

#[derive(Debug)]
pub enum StreamMuxerEvent {}

pub trait StreamMuxerExt: StreamMuxer + Sized {
    /// Convenience function for calling [`StreamMuxer::poll_inbound`]
    /// for [`StreamMuxer`]s that are `Unpin`.
    fn poll_inbound_unpin(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>>
    where
        Self: Unpin,
    {
        Pin::new(self).poll_inbound(cx)
    }

    /// Convenience function for calling [`StreamMuxer::poll_outbound`]
    /// for [`StreamMuxer`]s that are `Unpin`.
    fn poll_outbound_unpin(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>>
    where
        Self: Unpin,
    {
        Pin::new(self).poll_outbound(cx)
    }

    /// Convenience function for calling [`StreamMuxer::poll`]
    /// for [`StreamMuxer`]s that are `Unpin`.
    fn poll_unpin(&mut self, cx: &mut Context<'_>) -> Poll<Result<StreamMuxerEvent, Self::Error>>
    where
        Self: Unpin,
    {
        Pin::new(self).poll(cx)
    }

    /// Convenience function for calling [`StreamMuxer::poll_close`]
    /// for [`StreamMuxer`]s that are `Unpin`.
    fn poll_close_unpin(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>
    where
        Self: Unpin,
    {
        Pin::new(self).poll_close(cx)
    }

    /// Returns a future for closing this [`StreamMuxer`].
    fn close(self) -> Close<Self> {
        Close(self)
    }
}

impl<S> StreamMuxerExt for S where S: StreamMuxer {}

pub struct Close<S>(S);

impl<S> Future for Close<S>
where
    S: StreamMuxer + Unpin,
{
    type Output = Result<(), S::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.poll_close_unpin(cx)
    }
}
