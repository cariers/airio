mod connection;
mod error;
mod stream;

use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use airio_core::{ListenerEvent, PeerId, Transport};
use futures::{FutureExt, future::BoxFuture};
use socket2::{Domain, Socket, Type};

pub use connection::{Connecting, Connection};
pub use error::Error;
pub use stream::Stream;

pub struct Config {
    pub(crate) client_config: quinn::ClientConfig,
    pub(crate) server_config: quinn::ServerConfig,
    pub(crate) endpoint_config: quinn::EndpointConfig,
    handshake_timeout: Duration,
}

impl Config {
    pub fn new() -> Self {
        todo!()
    }

    pub fn handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout;
        self
    }
}
impl Transport for Config {
    type Output = (PeerId, Connection);
    type Error = Error;
    type Dialer = Connecting;
    type ListenerUpgrade = Connecting;
    type Listener = ListenerStream;

    fn listen(&self, addr: SocketAddr) -> Result<Self::Listener, Self::Error> {
        let socket = create_socket(addr)?;
        let local_addr = socket.local_addr()?;
        let runtime = Arc::new(quinn::TokioRuntime);
        let server_config = self.server_config.clone();
        let endpoint_config = self.endpoint_config.clone();
        let endpoint = quinn::Endpoint::new(endpoint_config, Some(server_config), socket, runtime)?;
        let endpoint_cloned = endpoint.clone();
        let accept = async move { endpoint_cloned.accept().await }.boxed();
        Ok(ListenerStream {
            local_addr,
            endpoint,
            accept,
            handshake_timeout: self.handshake_timeout,
        })
    }

    fn connect(&self, addr: SocketAddr) -> Result<Self::Dialer, Self::Error> {
        let local_listen_addr = match addr {
            SocketAddr::V4(_) => SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0),
            SocketAddr::V6(_) => SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 0),
        };
        let socket = create_socket(local_listen_addr)?;
        let runtime = Arc::new(quinn::TokioRuntime);
        let endpoint_config = self.endpoint_config.clone();
        let client_config = self.client_config.clone();
        let endpoint = quinn::Endpoint::new(endpoint_config, None, socket, runtime)?;
        let connecting = endpoint.connect_with(client_config, addr, "l")?;
        Ok(Connecting::new(connecting, self.handshake_timeout))
    }
}

pub struct ListenerStream {
    local_addr: SocketAddr,
    endpoint: quinn::Endpoint,
    accept: BoxFuture<'static, Option<quinn::Incoming>>,
    handshake_timeout: Duration,
}

impl futures::Stream for ListenerStream {
    type Item = ListenerEvent<Connecting, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match self.accept.poll_unpin(cx) {
                Poll::Ready(Some(incoming)) => {
                    let endpoint = self.endpoint.clone();
                    self.accept = async move { endpoint.accept().await }.boxed();
                    let connecting = match incoming.accept() {
                        Ok(connecting) => connecting,
                        Err(e) => return Poll::Ready(Some(ListenerEvent::Error(e.into()))),
                    };
                    let event: ListenerEvent<Connecting, Error> = ListenerEvent::Incoming {
                        local_addr: self.local_addr,
                        remote_addr: connecting.remote_address(),
                        upgrade: Connecting::new(connecting, self.handshake_timeout),
                    };
                    return Poll::Ready(Some(event));
                }
                Poll::Ready(None) => {
                    continue;
                }
                Poll::Pending => {}
            }
            return Poll::Pending;
        }
    }
}

fn create_socket(socket_addr: SocketAddr) -> std::io::Result<UdpSocket> {
    let socket = Socket::new(
        Domain::for_address(socket_addr),
        Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    if socket_addr.is_ipv6() {
        socket.set_only_v6(true)?;
    }

    socket.bind(&socket_addr.into())?;

    Ok(socket.into())
}
