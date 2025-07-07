use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use airio_core::{PeerId, StreamMuxer, muxing::StreamMuxerEvent};
use futures::{
    FutureExt,
    future::{BoxFuture, Either, Select, select},
    ready,
};
use futures_timer::Delay;
use quinn::rustls::pki_types::CertificateDer;
use sha2::Digest;
use x509_parser::prelude::{FromDer, X509Certificate};

use crate::{Error, Stream};

#[derive(Debug)]
pub struct Connecting {
    connecting: Select<quinn::Connecting, Delay>,
}

impl Connecting {
    pub(crate) fn new(connecting: quinn::Connecting, timeout: Duration) -> Self {
        Connecting {
            connecting: select(connecting, Delay::new(timeout)),
        }
    }
}

fn remote_peer_id(connecting: &quinn::Connection) -> PeerId {
    let identity = connecting
        .peer_identity()
        .expect("peer identity should be set by the remote peer");

    let certificates: Box<Vec<CertificateDer>> = identity
        .downcast()
        .expect("peer identity should be set by the remote peer");

    let end_entity = certificates
        .first()
        .expect("peer identity should have at least one certificate");

    let (_, cert) = X509Certificate::from_der(&end_entity)
        .expect("peer identity should have a valid X509 certificate");
    let digest = sha2::Sha256::digest(cert.public_key().raw);
    let mut bytes: [u8; 32] = [0_u8; 32];
    bytes.copy_from_slice(&digest);

    PeerId::from_bytes(bytes)
}

impl Future for Connecting {
    type Output = Result<(PeerId, Connection), Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let connection = match futures::ready!(self.connecting.poll_unpin(cx)) {
            Either::Right(_) => return Poll::Ready(Err(Error::HandshakeTimedOut)),
            Either::Left((connection, _)) => connection.map_err(Error::from)?,
        };
        let peer_id = remote_peer_id(&connection);
        let muxer = Connection::new(connection);
        Poll::Ready(Ok((peer_id, muxer)))
    }
}

pub struct Connection {
    connection: quinn::Connection,
    incoming: Option<
        BoxFuture<'static, Result<(quinn::SendStream, quinn::RecvStream), quinn::ConnectionError>>,
    >,
    outgoing: Option<
        BoxFuture<'static, Result<(quinn::SendStream, quinn::RecvStream), quinn::ConnectionError>>,
    >,
    closing: Option<BoxFuture<'static, quinn::ConnectionError>>,
}

impl Connection {
    fn new(connection: quinn::Connection) -> Self {
        Connection {
            connection,
            incoming: None,
            outgoing: None,
            closing: None,
        }
    }
}

impl StreamMuxer for Connection {
    type Substream = Stream;

    type Error = Error;

    fn poll_inbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let this = self.get_mut();
        let incoming = this.incoming.get_or_insert_with(|| {
            let connection = this.connection.clone();
            // 创建一个双向流的 Future
            async move { connection.accept_bi().await }.boxed()
        });
        let (send, recv) = ready!(incoming.poll_unpin(cx)).map_err(Error::from)?;
        // 清除 incoming，以便下次调用 poll_inbound 时重新创建
        this.incoming.take();
        let stream = Stream::new(send, recv);
        Poll::Ready(Ok(stream))
    }

    fn poll_outbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let this = self.get_mut();
        let outgoing = this.outgoing.get_or_insert_with(|| {
            let connection = this.connection.clone();
            // 创建一个双向流的 Future
            async move { connection.open_bi().await }.boxed()
        });
        let (send, recv) = ready!(outgoing.poll_unpin(cx)).map_err(Error::from)?;
        // 清除 outgoing，以便下次调用 poll_outbound 时重新创建
        this.outgoing.take();
        let stream = Stream::new(send, recv);
        Poll::Ready(Ok(stream))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        let closing = this.closing.get_or_insert_with(|| {
            let connection = this.connection.clone();
            // 创建一个关闭连接的 Future
            async move { connection.closed().await }.boxed()
        });
        match ready!(closing.poll_unpin(cx)) {
            // 本地关闭连接
            quinn::ConnectionError::LocallyClosed => {}
            err => return Poll::Ready(Err(err.into())),
        }
        return Poll::Ready(Ok(()));
    }

    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        Poll::Pending
    }
}
