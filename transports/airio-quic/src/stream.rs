use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{AsyncRead, AsyncWrite};

pub struct Stream {
    /// A send part of the stream
    send: quinn::SendStream,
    /// A receive part of the stream
    recv: quinn::RecvStream,
    /// Whether the stream is closed or not
    close_result: Option<Result<(), io::ErrorKind>>,
}

impl Stream {
    pub(crate) fn new(send: quinn::SendStream, recv: quinn::RecvStream) -> Self {
        Self {
            send,
            recv,
            close_result: None,
        }
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if let Some(close_result) = self.close_result {
            if close_result.is_err() {
                return Poll::Ready(Ok(0));
            }
        }
        Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.send)
            .poll_write(cx, buf)
            .map_err(Into::into)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut self.send).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        if let Some(close_result) = self.close_result {
            // For some reason poll_close needs to be 'fuse'able
            return Poll::Ready(close_result.map_err(Into::into));
        }
        let close_result = futures::ready!(Pin::new(&mut self.send).poll_close(cx));
        self.close_result = Some(close_result.as_ref().map_err(|e| e.kind()).copied());
        Poll::Ready(close_result)
    }
}
