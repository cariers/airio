use std::{
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};

use airio_core::{ListenerEvent, Transport, utils::RwStreamSink};
use airio_tcp::TcpStream;
use async_tungstenite::{
    accept_async_with_config, client_async_with_config,
    tungstenite::{self, protocol::WebSocketConfig},
};
use futures::{FutureExt, Stream, TryFutureExt};

use crate::framed::BytesWebSocketStream;
pub use tungstenite::Error;

mod framed;

#[derive(Debug, Clone)]
pub struct Config {
    pub websocket: WebSocketConfig,
    pub tcp: airio_tcp::Config,
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

impl Config {
    pub fn new() -> Self {
        Self {
            websocket: WebSocketConfig::default(),
            tcp: airio_tcp::Config::default(),
        }
    }

    /// Set [`Self::read_buffer_size`].
    pub fn read_buffer_size(mut self, read_buffer_size: usize) -> Self {
        self.websocket.read_buffer_size = read_buffer_size;
        self
    }

    /// Set [`Self::write_buffer_size`].
    pub fn write_buffer_size(mut self, write_buffer_size: usize) -> Self {
        self.websocket.write_buffer_size = write_buffer_size;
        self
    }

    /// Set [`Self::max_write_buffer_size`].
    pub fn max_write_buffer_size(mut self, max_write_buffer_size: usize) -> Self {
        self.websocket.max_write_buffer_size = max_write_buffer_size;
        self
    }

    /// Set [`Self::max_message_size`].
    pub fn max_message_size(mut self, max_message_size: Option<usize>) -> Self {
        self.websocket.max_message_size = max_message_size;
        self
    }

    /// Set [`Self::max_frame_size`].
    pub fn max_frame_size(mut self, max_frame_size: Option<usize>) -> Self {
        self.websocket.max_frame_size = max_frame_size;
        self
    }

    /// Set [`Self::accept_unmasked_frames`].
    pub fn accept_unmasked_frames(mut self, accept_unmasked_frames: bool) -> Self {
        self.websocket.accept_unmasked_frames = accept_unmasked_frames;
        self
    }
}

type ListenerUpgrade = Pin<
    Box<dyn Future<Output = Result<RwStreamSink<BytesWebSocketStream<TcpStream>>, Error>> + Send>,
>;

impl Transport for Config {
    type Output = RwStreamSink<BytesWebSocketStream<TcpStream>>;
    type Error = tungstenite::Error;
    type Dialer = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;
    type ListenerUpgrade = ListenerUpgrade;
    type Listener = ListenStream;

    fn connect(&self, addr: SocketAddr) -> Result<Self::Dialer, Self::Error> {
        let dialer = self.tcp.connect(addr)?;
        let config = self.websocket.clone();
        let request = tungstenite::http::Uri::builder()
            .scheme("ws")
            .authority(addr.to_string())
            .path_and_query("/")
            .build()
            .map_err(tungstenite::Error::from)?;
        tracing::debug!("Connecting to WebSocket at {}", request);
        Ok(dialer
            .map_err(tungstenite::Error::from)
            .and_then(move |stream| client_async_with_config(request, stream, Some(config)))
            .map_ok(|(s, response)| {
                tracing::debug!("WebSocket handshake response: {:?}", response);
                BytesWebSocketStream::new(s)
            })
            .map_ok(RwStreamSink::new)
            .boxed())
    }

    fn listen(&self, addr: SocketAddr) -> Result<Self::Listener, Self::Error> {
        let listener = self.tcp.listen(addr)?;
        tracing::debug!("Listening for WebSocket connections on {}", addr);
        Ok(ListenStream {
            config: self.websocket.clone(),
            inner: listener,
        })
    }
}

pub struct ListenStream {
    config: WebSocketConfig,
    inner: airio_tcp::ListenStream,
}

impl Stream for ListenStream {
    type Item = ListenerEvent<ListenerUpgrade, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let config = self.config.clone();
        let event = match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(event)) => event
                .map_upgrade(|u| {
                    u.map_err(Error::from)
                        .and_then(move |stream| accept_async_with_config(stream, Some(config)))
                        .map_ok(BytesWebSocketStream::new)
                        .map_ok(RwStreamSink::new)
                        .boxed()
                })
                .map_err(Error::from),
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending => return Poll::Pending,
        };
        Poll::Ready(Some(event))
    }
}
