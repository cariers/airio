use airio_core::ListenerEvent;
use futures::{
    Stream,
    future::{self, Ready},
};
use std::{
    io,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::net::TcpListener;

use crate::TcpStream;

pub struct ListenStream {
    listener_addr: SocketAddr,
    listener: TcpListener,
    pending_event: Option<ListenerEvent<Ready<Result<TcpStream, io::Error>>, io::Error>>,
}

impl ListenStream {
    pub fn new(listener: TcpListener, listener_addr: SocketAddr) -> Self {
        let listened_event = ListenerEvent::Listened(listener_addr);
        ListenStream {
            listener_addr,
            listener,
            pending_event: Some(listened_event),
        }
    }
}

impl Stream for ListenStream {
    type Item = ListenerEvent<Ready<Result<TcpStream, io::Error>>, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(event) = self.pending_event.take() {
            return Poll::Ready(Some(event));
        }
        tracing::trace!(
            "ListenStream::poll_next: Polling for new connections on {}",
            self.listener_addr
        );
        match Pin::new(&mut self.listener).poll_accept(cx) {
            Poll::Ready(Ok((stream, remote_addr))) => {
                return Poll::Ready(Some(ListenerEvent::Incoming {
                    local_addr: self.listener_addr,
                    remote_addr,
                    upgrade: future::ok(stream.into()),
                }));
            }
            Poll::Ready(Err(e)) => {
                return Poll::Ready(Some(ListenerEvent::Error(e)));
            }
            Poll::Pending => {}
        }
        Poll::Pending
    }
}
