use iroh::{
    Endpoint, EndpointAddr,
    endpoint::{Connection, RecvStream, SendStream},
    protocol::{AcceptError, ProtocolHandler, Router},
};
use n0_error::{Result, StdResultExt};
use std::sync::Arc;

const ALPN: &[u8] = b"nixdelta/compare/0";

/// Maximum summary size we'll accept (1 MiB).
const MAX_SUMMARY_SIZE: usize = 1024 * 1024;

/// Send a length-prefixed message on a QUIC send stream.
async fn send_msg(stream: &mut SendStream, data: &[u8]) -> std::result::Result<(), AcceptError> {
    let len = (data.len() as u32).to_le_bytes();
    stream
        .write_all(&len)
        .await
        .map_err(AcceptError::from_err)?;
    stream
        .write_all(data)
        .await
        .map_err(AcceptError::from_err)?;
    Ok(())
}

/// Receive a length-prefixed message from a QUIC recv stream.
async fn recv_msg(stream: &mut RecvStream) -> std::result::Result<Vec<u8>, AcceptError> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .map_err(AcceptError::from_err)?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_SUMMARY_SIZE {
        return Err(AcceptError::from_err(std::io::Error::other(format!(
            "message too large: {len} bytes"
        ))));
    }
    let mut buf = vec![0u8; len];
    stream
        .read_exact(&mut buf)
        .await
        .map_err(AcceptError::from_err)?;
    Ok(buf)
}

/// Start an endpoint, print a ticket, wait for a peer to connect,
/// exchange summaries bidirectionally, then return the peer's data.
pub async fn share(my_summary: Vec<u8>) -> Result<Vec<u8>> {
    let endpoint = Endpoint::builder()
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await?;

    endpoint.online().await;

    let addr = endpoint.addr();
    let ticket = serde_json::to_string(&addr).anyerr()?;
    let ticket_b64 = data_encoding::BASE64URL_NOPAD.encode(ticket.as_bytes());

    eprintln!();
    eprintln!("share this ticket with your peer:");
    eprintln!();
    println!("{ticket_b64}");
    eprintln!();
    eprintln!("waiting for peer to connect...");

    let (done_tx, mut done_rx) = tokio::sync::oneshot::channel::<Vec<u8>>();

    let handler = ShareHandler {
        my_summary: Arc::new(my_summary),
        done: Arc::new(tokio::sync::Mutex::new(Some(done_tx))),
    };
    let router = Router::builder(endpoint).accept(ALPN, handler).spawn();

    let peer_data = tokio::select! {
        result = &mut done_rx => result.ok(),
        _ = tokio::signal::ctrl_c() => None,
    };

    router.shutdown().await.anyerr()?;

    match peer_data {
        Some(data) => Ok(data),
        None => Err(n0_error::anyerr!("no peer data received")),
    }
}

#[derive(Debug, Clone)]
struct ShareHandler {
    my_summary: Arc<Vec<u8>>,
    done: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<Vec<u8>>>>>,
}

impl ProtocolHandler for ShareHandler {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let remote = connection.remote_id();
        eprintln!("peer connected: {}", remote.fmt_short());

        let (mut send, mut recv) = connection.accept_bi().await?;

        // Exchange: send ours, receive theirs
        send_msg(&mut send, &self.my_summary).await?;
        send.finish()?;
        let peer_data = recv_msg(&mut recv).await?;

        // Wait for the peer to close the connection (meaning it finished reading)
        connection.closed().await;

        // Only signal done after the peer has confirmed receipt
        if let Some(tx) = self.done.lock().await.take() {
            tx.send(peer_data).ok();
        }

        Ok(())
    }
}

/// Connect to a peer's endpoint, exchange summaries, return theirs.
pub async fn compare(my_summary: Vec<u8>, ticket_b64: &str) -> Result<Vec<u8>> {
    let ticket_json = data_encoding::BASE64URL_NOPAD
        .decode(ticket_b64.as_bytes())
        .map_err(|e| n0_error::anyerr!("invalid ticket encoding: {e}"))?;
    let addr: EndpointAddr = serde_json::from_slice(&ticket_json)
        .map_err(|e| n0_error::anyerr!("invalid ticket format: {e}"))?;

    let endpoint = Endpoint::bind().await?;

    eprintln!("connecting to peer...");
    let conn = endpoint.connect(addr, ALPN).await?;
    eprintln!("connected to {}", conn.remote_id().fmt_short());

    let (mut send, mut recv) = conn.open_bi().await.anyerr()?;

    send_msg(&mut send, &my_summary)
        .await
        .map_err(|e| n0_error::anyerr!("send failed: {e}"))?;
    send.finish().anyerr()?;

    let peer_data = recv_msg(&mut recv)
        .await
        .map_err(|e| n0_error::anyerr!("recv failed: {e}"))?;

    conn.close(0u32.into(), b"done");
    endpoint.close().await;

    Ok(peer_data)
}
