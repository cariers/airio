use airio::core::muxing::StreamMuxerExt;
use airio::core::{ListenerEvent, Transport};
use airio::identify::ed25519::SigningKey;
use airio::{identify, muxing, tcp};
use futures::channel::{mpsc, oneshot};
use futures::{AsyncReadExt, AsyncWriteExt, StreamExt, future};
use std::net::SocketAddr;
use std::task::Poll;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // init tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    tracing::info!("Starting TCP Echo Example");

    let addr = "0.0.0.0:8088".parse::<SocketAddr>()?;

    let key: [u8; 32] = rand::random();
    let local_key = SigningKey::from_bytes(&key);

    let identify_upgrade = identify::Config::new(local_key.verifying_key());
    let muxing_upgrade = muxing::Config::new();

    let tcp = tcp::Config::new()
        .upgrade()
        .authenticate(identify_upgrade)
        .multiplex(muxing_upgrade);

    let mut listener = tcp.listen(addr)?;

    let server_fut = tokio::spawn(async move {
        while let Some(event) = listener.next().await {
            tracing::info!("Listener event: {:?}", event);
            match event {
                ListenerEvent::Incoming {
                    local_addr,
                    remote_addr,
                    upgrade,
                } => {
                    tokio::spawn(async move {
                        tracing::debug!(
                            "Incoming connection from {} to {}",
                            remote_addr,
                            local_addr
                        );
                        let (peer, mut muxer) = upgrade.await.unwrap();
                        tracing::debug!(
                            "Peer({}), Upgraded stream from {} to {}",
                            peer,
                            remote_addr,
                            local_addr
                        );
                        let (mut sender, mut receiver) = mpsc::channel(10);

                        let muxer_fut = future::poll_fn(move |cx| {
                            match muxer.poll_inbound_unpin(cx) {
                                Poll::Ready(Ok(stream)) => {
                                    sender.try_send(stream).unwrap();
                                    cx.waker().wake_by_ref();
                                    return Poll::Pending;
                                }
                                Poll::Ready(Err(e)) => {
                                    tracing::error!("Error polling inbound stream: {:?}", e);
                                    return Poll::Ready(());
                                }
                                Poll::Pending => {}
                            }
                            match muxer.poll_unpin(cx) {
                                Poll::Ready(Ok(stream)) => {
                                    tracing::info!(
                                        "Accepted stream from: {} to: {}, stream: {:?}",
                                        remote_addr,
                                        local_addr,
                                        stream
                                    );
                                    cx.waker().wake_by_ref();
                                    Poll::Pending
                                }
                                Poll::Ready(Err(e)) => {
                                    tracing::error!("Error polling inbound stream: {:?}", e);
                                    return Poll::Ready(());
                                }
                                Poll::Pending => Poll::Pending,
                            }
                        });
                        tokio::spawn(muxer_fut);
                        loop {
                            let stream = receiver.next().await.unwrap();
                            let (mut reader, mut writer) = stream.split();
                            let mut buf = vec![0; 1024];
                            loop {
                                match reader.read(&mut buf).await {
                                    Ok(0) => break, // EOF
                                    Ok(n) => {
                                        if writer.write_all(&buf[..n]).await.is_err() {
                                            break; // Write error
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("Read error: {}", e);
                                        break;
                                    }
                                }
                            }
                        }
                    });
                }
                _ => {}
            }
        }
    });
    server_fut.await?;
    tracing::info!("TCP Echo Example completed");
    Ok(())
}
