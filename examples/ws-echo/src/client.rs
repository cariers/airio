use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use airio::core::Transport;
use airio::core::muxing::StreamMuxerExt;
use airio::identify::ed25519::SigningKey;
use airio::{identify, muxing, ws};
use futures::{AsyncReadExt, AsyncWriteExt, future};

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

    let tcp = ws::Config::new()
        .upgrade()
        .authenticate(identify_upgrade)
        .multiplex(muxing_upgrade);

    tracing::info!("Listener is ready and listening on {}", addr);
    let (peer, mut muxer) = tcp.connect(addr).unwrap().await.unwrap();
    tracing::info!("Client connected to {}, Peer:{}", addr, peer);

    let stream = future::poll_fn(|cx| muxer.poll_outbound_unpin(cx))
        .await
        .unwrap();

    let client_fut = tokio::spawn(future::poll_fn(move |cx| muxer.poll_unpin(cx)));

    let stream_fut = tokio::spawn(async move {
        let (mut reader, mut writer) = stream.split();

        loop {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let data = format!("Hello, world! at {}", now);
            writer.write_all(data.as_bytes()).await.unwrap();
            writer.flush().await.unwrap();
            tracing::info!("Sent: {}", data);
            let mut buf = vec![0; data.len()];
            reader.read_exact(&mut buf).await.unwrap();
            tracing::info!("Received: {}", String::from_utf8_lossy(&buf));

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });

    tokio::select! {
        _ = client_fut => {
            tracing::info!("Client future completed");
        }
        _ = stream_fut => {
            tracing::info!("Stream future completed");
        }
    }

    Ok(())
}
