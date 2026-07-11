use std::{
    collections::HashMap,
    io,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;

use tokio::{
    runtime::{Builder, Runtime},
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
};

use wtransport::{
    ClientConfig, Connection, Endpoint, Identity, RecvStream, SendStream, ServerConfig, VarInt,
    datagram::Datagram,
    endpoint::{IncomingSession, SessionRequest, endpoint_side},
    error::StreamWriteError,
    stream::OpeningBiStream,
    tls::error::{InvalidSan, PemLoadError},
};

use redixel_core::{ClientId, NetworkChannel, NetworkEvent, NetworkManager, NoOpNetwork, SERVER_ID};

use crate::{
    config::{CertSource, NetConfig, NetMode},
    inbound::{Inbound, InboundQueue},
    seq,
};

/// How often the client refreshes its RTT reading from the connection.
const RTT_INTERVAL: Duration = Duration::from_millis(500);

/// Hard cap on a single reliable frame. The receiver allocates exactly the
/// length a peer announces, so this bounds that allocation: without it a corrupt
/// or hostile peer could request a 4 GiB reservation with four bytes.
const MAX_FRAME_LEN: usize = 4 * 1024 * 1024;

/// Bytes of the handshake hello frame: `[u64 protocol_id]`.
const HELLO_LEN: usize = 8;

/// Bytes of the handshake welcome frame: `[u64 client_id][f64 tickrate]`.
const WELCOME_LEN: usize = 16;

/// QUIC application error code sent when a peer fails the protocol handshake.
const PROTOCOL_MISMATCH_CODE: u32 = 1;

/// How long a peer has to complete the handshake before its slot is reclaimed.
///
/// A pending handshake already occupies one of `max_clients`, so without this a
/// peer could open the cap's worth of connections, never send its hello frame,
/// and lock every slot indefinitely.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Worker threads backing a server's transport runtime.
///
/// QUIC work is I/O-bound and the game simulation runs on the calling thread, so
/// tokio's default (one worker per core) only buys context switches — and works
/// against the headless runtime's goal of a small footprint on a modest VPS.
const SERVER_WORKER_THREADS: usize = 2;

/// Worker threads backing a client's transport runtime. A client drives exactly
/// one connection; extra workers would only contend with the render thread.
const CLIENT_WORKER_THREADS: usize = 1;

/// An inbound bridge event produced by an async task for the sync manager.
enum Incoming {
    Connected(ClientId),
    Disconnected(ClientId),
    Message {
        client: ClientId,
        channel: NetworkChannel,
        payload: Vec<u8>,
    },
}

/// An outbound bridge command produced by the sync manager for the dispatcher.
///
/// `payload` is a [`Bytes`] so a broadcast hands every peer a refcount bump
/// rather than a fresh copy of the message.
struct Outgoing {
    target: Target,
    channel: NetworkChannel,
    payload: Bytes,
}

/// Who an [`Outgoing`] command is addressed to.
enum Target {
    One(ClientId),
    All,
}

/// Writes one length-prefixed (`[u32 len][payload]`, little-endian) frame to
/// a reliable stream — the wire format backing [`NetworkChannel::ReliableOrdered`].
async fn write_frame(send: &mut SendStream, payload: &[u8]) -> Result<(), StreamWriteError> {
    let len: [u8; 4] = (payload.len() as u32).to_le_bytes();
    send.write_all(&len).await?;
    send.write_all(payload).await?;
    Ok(())
}

/// Reads one length-prefixed frame, or `None` once the stream ends, errors, or
/// announces a frame larger than [`MAX_FRAME_LEN`].
async fn read_frame(recv: &mut RecvStream) -> Option<Vec<u8>> {
    let mut len_buf: [u8; 4] = [0; 4];
    recv.read_exact(&mut len_buf).await.ok()?;

    let len: usize = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME_LEN {
        log::warn!("Peer announced a {len}-byte frame over the {MAX_FRAME_LEN}-byte limit; dropping the stream.");
        return None;
    }

    let mut buf: Vec<u8> = vec![0; len];
    recv.read_exact(&mut buf).await.ok()?;
    Some(buf)
}

/// Spawns the reliable writer and the reliable/datagram readers for one
/// connection, tagging inbound messages with `msg_client`.
fn spawn_connection_tasks(
    conn: Arc<Connection>,
    send: SendStream,
    recv: RecvStream,
    reliable_rx: UnboundedReceiver<Bytes>,
    msg_client: ClientId,
    inbound_tx: UnboundedSender<Incoming>,
) {
    tokio::spawn(reliable_writer(send, reliable_rx));
    tokio::spawn(reliable_reader(recv, msg_client, inbound_tx.clone()));
    tokio::spawn(datagram_reader(conn, msg_client, inbound_tx));
}

/// Drains the reliable-send channel onto the connection's stream.
async fn reliable_writer(mut send: SendStream, mut rx: UnboundedReceiver<Bytes>) {
    while let Some(payload) = rx.recv().await {
        if payload.len() > MAX_FRAME_LEN {
            log::error!(
                "Dropping a {}-byte reliable message over the {MAX_FRAME_LEN}-byte frame limit.",
                payload.len()
            );
            continue;
        }

        if write_frame(&mut send, &payload).await.is_err() {
            break;
        }
    }

    send.finish().await.ok();
}

/// Forwards inbound reliable frames as [`Incoming::Message`]s.
async fn reliable_reader(mut recv: RecvStream, client: ClientId, tx: UnboundedSender<Incoming>) {
    while let Some(payload) = read_frame(&mut recv).await {
        let sent: bool = tx
            .send(Incoming::Message {
                client,
                channel: NetworkChannel::ReliableOrdered,
                payload,
            })
            .is_ok();

        if !sent {
            break;
        }
    }
}

/// Reassembles inbound datagram fragments (newest-wins) into whole messages and
/// forwards them as [`Incoming::Message`]s.
async fn datagram_reader(conn: Arc<Connection>, client: ClientId, tx: UnboundedSender<Incoming>) {
    let mut reassembler: seq::Reassembler = seq::Reassembler::new();

    loop {
        let dgram: Datagram = match conn.receive_datagram().await {
            Ok(dgram) => dgram,
            Err(_) => break,
        };

        let data: Bytes = dgram.payload();
        let Some((seq_num, index, count, fragment)): Option<(u32, u16, u16, &[u8])> = seq::split(&data) else {
            continue;
        };

        let Some(payload): Option<Vec<u8>> = reassembler.push(seq_num, index, count, fragment) else {
            continue;
        };

        let sent: bool = tx
            .send(Incoming::Message {
                client,
                channel: NetworkChannel::UnreliableSequenced,
                payload,
            })
            .is_ok();

        if !sent {
            break;
        }
    }
}

/// Sends `payload` on `conn` as one or more sequenced datagrams, fragmenting it
/// across datagrams when it exceeds the connection's datagram limit and
/// consuming one sequence number from `send_seq` for the whole message.
///
/// Unreliable by contract: a message that cannot be sent is **dropped**, never
/// promoted onto the reliable stream. Promoting it would break the channel's
/// newest-wins ordering (the reliable path carries no sequence and is delivered
/// on a different channel) and would head-of-line block genuine reliable traffic
/// behind a snapshot that is already obsolete by the time it arrives.
///
/// `scratch` is reused across calls, so framing costs no allocation after warmup.
fn send_unreliable(conn: &Connection, scratch: &mut Vec<u8>, send_seq: &mut u32, payload: &[u8]) {
    let Some(max_datagram): Option<usize> = conn.max_datagram_size() else {
        log::debug!("Peer accepts no datagrams; dropping an unreliable message.");
        return;
    };

    if max_datagram <= seq::HEADER_LEN {
        log::warn!("Datagram limit of {max_datagram} bytes cannot fit the sequence header; dropping.");
        return;
    }

    if payload.len() > seq::MAX_MESSAGE_LEN {
        log::warn!(
            "Dropping a {}-byte unreliable message over the {}-byte reassembly limit.",
            payload.len(),
            seq::MAX_MESSAGE_LEN
        );
        return;
    }

    let chunk: usize = max_datagram - seq::HEADER_LEN;
    let count: usize = payload.len().div_ceil(chunk).max(1);
    if count > seq::MAX_FRAGMENTS as usize {
        log::warn!(
            "An unreliable message of {} bytes needs {count} fragments, over the {} limit; dropping.",
            payload.len(),
            seq::MAX_FRAGMENTS
        );
        return;
    }

    let current: u32 = *send_seq;
    *send_seq = send_seq.wrapping_add(1);

    if payload.is_empty() {
        seq::frame(scratch, current, 0, 1, &[]);
        conn.send_datagram(scratch.as_slice()).ok();
        return;
    }

    for (index, fragment) in payload.chunks(chunk).enumerate() {
        seq::frame(scratch, current, index as u16, count as u16, fragment);
        if conn.send_datagram(scratch.as_slice()).is_err() {
            break;
        }
    }
}

/// Resolves a [`CertSource`] into a wtransport [`Identity`].
async fn make_identity(cert: &CertSource) -> io::Result<Identity> {
    match cert {
        CertSource::SelfSigned => {
            Identity::self_signed(["localhost"]).map_err(|e: InvalidSan| io::Error::other(e.to_string()))
        }
        CertSource::Pem { cert, key } => Identity::load_pemfiles(cert, key)
            .await
            .map_err(|e: PemLoadError| io::Error::other(e.to_string())),
    }
}

/// Server-internal control: connection lifecycle events for the dispatcher's peer map.
enum Ctrl {
    Ready {
        client: ClientId,
        conn: Arc<Connection>,
        reliable_tx: UnboundedSender<Bytes>,
    },
    Closed {
        client: ClientId,
    },
}

/// A live peer as seen by the server's dispatcher. `send_seq` is per-peer so one
/// client's targeted traffic never advances another's sequence.
struct ConnHandle {
    conn: Arc<Connection>,
    reliable_tx: UnboundedSender<Bytes>,
    send_seq: u32,
}

/// Routes one outbound command to the addressed peer(s), reusing `scratch` to
/// frame datagrams.
fn route_outbound(conns: &mut HashMap<ClientId, ConnHandle>, out: Outgoing, scratch: &mut Vec<u8>) {
    match out.channel {
        NetworkChannel::ReliableOrdered => match out.target {
            Target::One(client) => {
                if let Some(handle) = conns.get(&client) {
                    handle.reliable_tx.send(out.payload).ok();
                }
            }
            Target::All => {
                for handle in conns.values() {
                    handle.reliable_tx.send(out.payload.clone()).ok();
                }
            }
        },
        NetworkChannel::UnreliableSequenced => match out.target {
            Target::One(client) => {
                if let Some(handle) = conns.get_mut(&client) {
                    send_unreliable(&handle.conn, scratch, &mut handle.send_seq, &out.payload);
                }
            }
            Target::All => {
                for handle in conns.values_mut() {
                    send_unreliable(&handle.conn, scratch, &mut handle.send_seq, &out.payload);
                }
            }
        },
    }
}

/// Server event loop: accepts connections up to `max_clients` and dispatches
/// outbound commands.
async fn run_server(
    endpoint: Endpoint<endpoint_side::Server>,
    tickrate: f64,
    protocol_id: u64,
    max_clients: usize,
    handshake_timeout: Duration,
    inbound_tx: UnboundedSender<Incoming>,
    mut outbound_rx: UnboundedReceiver<Outgoing>,
) {
    let mut conns: HashMap<ClientId, ConnHandle> = HashMap::new();
    let (ctrl_tx, mut ctrl_rx): (UnboundedSender<Ctrl>, UnboundedReceiver<Ctrl>) = unbounded_channel();
    let mut next_id: ClientId = 1;
    let mut active: usize = 0;
    let mut scratch: Vec<u8> = Vec::new();

    loop {
        tokio::select! {
            incoming = endpoint.accept() => {
                if active >= max_clients {
                    tokio::spawn(reject_session(incoming));
                    continue;
                }

                let id: ClientId = next_id;
                next_id = next_id.wrapping_add(1);
                if next_id == SERVER_ID {
                    next_id = 1;
                }

                active += 1;
                tokio::spawn(handshake_server(
                    incoming,
                    id,
                    tickrate,
                    protocol_id,
                    handshake_timeout,
                    inbound_tx.clone(),
                    ctrl_tx.clone(),
                ));
            }
            Some(ctrl) = ctrl_rx.recv() => match ctrl {
                Ctrl::Ready { client, conn, reliable_tx } => {
                    conns.insert(client, ConnHandle { conn, reliable_tx, send_seq: 0 });
                }
                Ctrl::Closed { client } => {
                    conns.remove(&client);
                    active = active.saturating_sub(1);
                }
            },
            Some(out) = outbound_rx.recv() => route_outbound(&mut conns, out, &mut scratch),
            else => break,
        }
    }
}

/// Turns away one session because the server is already at `max_clients`.
async fn reject_session(incoming: IncomingSession) {
    match incoming.await {
        Ok(request) => {
            log::warn!("Rejecting {}: server is full.", request.remote_address());
            request.too_many_requests().await;
        }
        Err(e) => log::warn!("webtransport incoming session: {e}"),
    }
}

/// Accepts one session, validates the client's protocol id, and answers with the
/// welcome frame carrying the assigned id and the server's authoritative
/// tickrate. Returns the live connection and its reliable stream, or `None` if
/// the peer failed the handshake.
///
/// Dropping this future (on timeout) drops the last `Arc<Connection>`, closing
/// the connection.
async fn establish(
    incoming: IncomingSession,
    id: ClientId,
    tickrate: f64,
    protocol_id: u64,
) -> Option<(Arc<Connection>, SendStream, RecvStream)> {
    let session_request: SessionRequest = match incoming.await {
        Ok(request) => request,
        Err(e) => {
            log::warn!("webtransport incoming session: {e}");
            return None;
        }
    };

    let connection: Connection = match session_request.accept().await {
        Ok(connection) => connection,
        Err(e) => {
            log::warn!("webtransport accept: {e}");
            return None;
        }
    };

    let conn: Arc<Connection> = Arc::new(connection);
    let (mut send, mut recv): (SendStream, RecvStream) = match conn.accept_bi().await {
        Ok(streams) => streams,
        Err(e) => {
            log::warn!("webtransport accept_bi: {e}");
            return None;
        }
    };

    let hello: Option<Vec<u8>> = read_frame(&mut recv).await;
    let peer_protocol: Option<u64> = hello
        .as_ref()
        .and_then(|frame: &Vec<u8>| frame.get(0..HELLO_LEN))
        .and_then(|bytes: &[u8]| bytes.try_into().ok())
        .map(u64::from_le_bytes);

    match peer_protocol {
        Some(peer) if peer == protocol_id => {}
        Some(peer) => {
            log::warn!("Rejecting client {id}: protocol id {peer:#018x} != {protocol_id:#018x}.");
            conn.close(VarInt::from_u32(PROTOCOL_MISMATCH_CODE), b"protocol mismatch");
            return None;
        }
        None => {
            log::warn!("Rejecting client {id}: malformed or missing hello frame.");
            conn.close(VarInt::from_u32(PROTOCOL_MISMATCH_CODE), b"malformed hello");
            return None;
        }
    }

    let mut welcome: Vec<u8> = Vec::with_capacity(WELCOME_LEN);
    welcome.extend_from_slice(&id.to_le_bytes());
    welcome.extend_from_slice(&tickrate.to_le_bytes());
    if write_frame(&mut send, &welcome).await.is_err() {
        return None;
    }

    Some((conn, send, recv))
}

/// Drives one incoming session through [`establish`] under [`HANDSHAKE_TIMEOUT`],
/// then registers the peer and spawns its tasks.
///
/// Exactly one [`Ctrl::Closed`] is emitted for `id` on every path — here on a
/// failed or timed-out handshake, or later by the connection watcher — so the
/// dispatcher's `max_clients` accounting stays balanced.
async fn handshake_server(
    incoming: IncomingSession,
    id: ClientId,
    tickrate: f64,
    protocol_id: u64,
    handshake_timeout: Duration,
    inbound_tx: UnboundedSender<Incoming>,
    ctrl_tx: UnboundedSender<Ctrl>,
) {
    let established: Option<(Arc<Connection>, SendStream, RecvStream)> =
        match tokio::time::timeout(handshake_timeout, establish(incoming, id, tickrate, protocol_id)).await {
            Ok(established) => established,
            Err(_) => {
                log::warn!("Rejecting client {id}: handshake did not complete within {handshake_timeout:?}.");
                None
            }
        };

    let Some((conn, send, recv)): Option<(Arc<Connection>, SendStream, RecvStream)> = established else {
        ctrl_tx.send(Ctrl::Closed { client: id }).ok();
        return;
    };

    let (reliable_tx, reliable_rx): (UnboundedSender<Bytes>, UnboundedReceiver<Bytes>) = unbounded_channel();
    inbound_tx.send(Incoming::Connected(id)).ok();
    ctrl_tx
        .send(Ctrl::Ready {
            client: id,
            conn: conn.clone(),
            reliable_tx,
        })
        .ok();

    spawn_connection_tasks(conn.clone(), send, recv, reliable_rx, id, inbound_tx.clone());

    tokio::spawn(async move {
        conn.closed().await;
        inbound_tx.send(Incoming::Disconnected(id)).ok();
        ctrl_tx.send(Ctrl::Closed { client: id }).ok();
    });
}

/// Authoritative server over WebTransport (desktop + Android), QUIC/HTTP-3
/// via [`wtransport`]. Owns a background tokio runtime bridging its async API
/// to the engine's synchronous [`redixel_core::NetworkManager`].
pub struct WebTransportServer {
    _runtime: Runtime,
    local_addr: SocketAddr,
    inbound_rx: UnboundedReceiver<Incoming>,
    outbound_tx: UnboundedSender<Outgoing>,
    queue: InboundQueue<Vec<u8>>,
}

impl WebTransportServer {
    /// Binds a WebTransport endpoint on `bind` using `cert` and starts serving on
    /// a background tokio runtime.
    ///
    /// `tickrate` is embedded in the welcome frame sent to every connecting
    /// client. `protocol_id` must match the one the client sends in its hello
    /// frame, or the connection is closed before it is ever announced to the
    /// game. At most `max_clients` peers are admitted; further sessions are
    /// turned away with an HTTP 429.
    pub fn new(
        bind: SocketAddr,
        cert: &CertSource,
        tickrate: f64,
        protocol_id: u64,
        max_clients: usize,
    ) -> io::Result<Self> {
        Self::with_handshake_timeout(bind, cert, tickrate, protocol_id, max_clients, HANDSHAKE_TIMEOUT)
    }

    /// [`new`](Self::new) with an explicit handshake timeout, so tests can drive
    /// the slot-reclaim path in milliseconds instead of the production 10 s.
    fn with_handshake_timeout(
        bind: SocketAddr,
        cert: &CertSource,
        tickrate: f64,
        protocol_id: u64,
        max_clients: usize,
        handshake_timeout: Duration,
    ) -> io::Result<Self> {
        let runtime: Runtime = Builder::new_multi_thread()
            .worker_threads(SERVER_WORKER_THREADS)
            .enable_all()
            .build()?;
        let (inbound_tx, inbound_rx): (UnboundedSender<Incoming>, UnboundedReceiver<Incoming>) = unbounded_channel();
        let (outbound_tx, outbound_rx): (UnboundedSender<Outgoing>, UnboundedReceiver<Outgoing>) = unbounded_channel();

        let cert: CertSource = cert.clone();
        let endpoint: Endpoint<endpoint_side::Server> = runtime.block_on(async move {
            let identity: Identity = make_identity(&cert).await?;
            let config: ServerConfig = ServerConfig::builder()
                .with_bind_address(bind)
                .with_identity(identity)
                .build();
            Endpoint::server(config)
        })?;

        let local_addr: SocketAddr = endpoint.local_addr()?;
        runtime.spawn(run_server(
            endpoint,
            tickrate,
            protocol_id,
            max_clients.max(1),
            handshake_timeout,
            inbound_tx,
            outbound_rx,
        ));

        Ok(Self {
            _runtime: runtime,
            local_addr,
            inbound_rx,
            outbound_tx,
            queue: InboundQueue::new(),
        })
    }

    /// The actual bound address (useful when binding to port 0).
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}

impl NetworkManager for WebTransportServer {
    fn poll(&mut self) -> Option<NetworkEvent<'_>> {
        self.queue.poll()
    }

    fn send(&mut self, client: ClientId, channel: NetworkChannel, payload: &[u8]) {
        self.outbound_tx
            .send(Outgoing {
                target: Target::One(client),
                channel,
                payload: Bytes::copy_from_slice(payload),
            })
            .ok();
    }

    fn broadcast(&mut self, channel: NetworkChannel, payload: &[u8]) {
        self.outbound_tx
            .send(Outgoing {
                target: Target::All,
                channel,
                payload: Bytes::copy_from_slice(payload),
            })
            .ok();
    }

    fn is_server(&self) -> bool {
        true
    }

    fn local_client(&self) -> Option<ClientId> {
        None
    }

    fn is_connected(&self) -> bool {
        true
    }

    fn update(&mut self) {
        self.queue.reset();
        while let Ok(msg) = self.inbound_rx.try_recv() {
            match msg {
                Incoming::Connected(id) => self.queue.push(Inbound::Connected(id)),
                Incoming::Disconnected(id) => self.queue.push(Inbound::Disconnected(id)),
                Incoming::Message {
                    client,
                    channel,
                    payload,
                } => self.queue.push(Inbound::Message {
                    client,
                    channel,
                    payload,
                }),
            }
        }
    }

    fn flush(&mut self) {}
}

/// Client event loop: connects, announces its protocol id, learns its assigned
/// id and the server's tickrate, then dispatches outbound traffic.
async fn run_client(
    endpoint: Endpoint<endpoint_side::Client>,
    url: String,
    protocol_id: u64,
    inbound_tx: UnboundedSender<Incoming>,
    mut outbound_rx: UnboundedReceiver<Outgoing>,
    rtt: Arc<AtomicU64>,
    server_tickrate: Arc<AtomicU64>,
) {
    let connection: Connection = match endpoint.connect(url.as_str()).await {
        Ok(connection) => connection,
        Err(e) => {
            log::error!("webtransport connect to {url}: {e}");
            return;
        }
    };

    let conn: Arc<Connection> = Arc::new(connection);
    let opening: OpeningBiStream = match conn.open_bi().await {
        Ok(opening) => opening,
        Err(e) => {
            log::error!("webtransport open_bi: {e}");
            return;
        }
    };

    let (mut send, mut recv): (SendStream, RecvStream) = match opening.await {
        Ok(streams) => streams,
        Err(e) => {
            log::error!("webtransport open_bi finish: {e}");
            return;
        }
    };

    if write_frame(&mut send, &protocol_id.to_le_bytes()).await.is_err() {
        return;
    }

    let welcome: Vec<u8> = match read_frame(&mut recv).await {
        Some(frame) => frame,
        None => {
            log::error!("webtransport: server closed before the welcome frame (protocol id mismatch?)");
            return;
        }
    };

    let id: ClientId = match welcome.get(0..8).and_then(|b: &[u8]| b.try_into().ok()) {
        Some(bytes) => u64::from_le_bytes(bytes),
        None => {
            log::error!("webtransport: malformed welcome");
            return;
        }
    };

    let tickrate: f64 = match welcome.get(8..WELCOME_LEN).and_then(|b: &[u8]| b.try_into().ok()) {
        Some(bytes) => f64::from_le_bytes(bytes),
        None => {
            log::error!("webtransport: welcome frame missing tickrate");
            return;
        }
    };

    if !tickrate.is_finite() || tickrate <= 0.0 {
        log::error!("webtransport: server announced a non-positive tickrate of {tickrate}");
        return;
    }

    server_tickrate.store(tickrate.to_bits(), Ordering::Relaxed);
    inbound_tx.send(Incoming::Connected(id)).ok();

    let (reliable_tx, reliable_rx): (UnboundedSender<Bytes>, UnboundedReceiver<Bytes>) = unbounded_channel();
    spawn_connection_tasks(conn.clone(), send, recv, reliable_rx, SERVER_ID, inbound_tx.clone());

    let watch_conn: Arc<Connection> = conn.clone();
    tokio::spawn(async move {
        watch_conn.closed().await;
        inbound_tx.send(Incoming::Disconnected(id)).ok();
    });

    let rtt_conn: Arc<Connection> = conn.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(RTT_INTERVAL) => {
                    rtt.store(rtt_conn.rtt().as_micros() as u64, Ordering::Relaxed);
                }
                _ = rtt_conn.closed() => break,
            }
        }
    });

    let mut scratch: Vec<u8> = Vec::new();
    let mut send_seq: u32 = 0;

    while let Some(out) = outbound_rx.recv().await {
        match out.channel {
            NetworkChannel::ReliableOrdered => {
                reliable_tx.send(out.payload).ok();
            }
            NetworkChannel::UnreliableSequenced => send_unreliable(&conn, &mut scratch, &mut send_seq, &out.payload),
        }
    }
}

/// Client connection to a [`WebTransportServer`], same async/sync bridge.
///
/// A dropped connection is terminal: [`is_connected`](NetworkManager::is_connected)
/// latches to `false` and the transport does **not** attempt to reconnect.
/// Reconnection is the game's call — construct a fresh client.
pub struct WebTransportClient {
    _runtime: Runtime,
    inbound_rx: UnboundedReceiver<Incoming>,
    outbound_tx: UnboundedSender<Outgoing>,
    queue: InboundQueue<Vec<u8>>,
    id: Option<ClientId>,
    connected: bool,
    rtt: Arc<AtomicU64>,
    server_tickrate: Arc<AtomicU64>,
}

impl WebTransportClient {
    /// Connects to `connect`, announcing `protocol_id` in its hello frame — a
    /// server configured with a different id closes the connection immediately.
    ///
    /// With `server_name` set, uses `https://{name}:{port}` and validates the
    /// cert (production); otherwise `https://{ip}:{port}` with validation
    /// disabled (LAN / self-signed).
    pub fn new(connect: SocketAddr, server_name: Option<String>, protocol_id: u64) -> io::Result<Self> {
        let runtime: Runtime = Builder::new_multi_thread()
            .worker_threads(CLIENT_WORKER_THREADS)
            .enable_all()
            .build()?;
        let (inbound_tx, inbound_rx): (UnboundedSender<Incoming>, UnboundedReceiver<Incoming>) = unbounded_channel();
        let (outbound_tx, outbound_rx): (UnboundedSender<Outgoing>, UnboundedReceiver<Outgoing>) = unbounded_channel();
        let rtt: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
        let server_tickrate: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));

        let (url, insecure): (String, bool) = match &server_name {
            Some(name) => (format!("https://{name}:{}", connect.port()), false),
            None => (format!("https://{connect}"), true),
        };

        let endpoint: Endpoint<endpoint_side::Client> = runtime.block_on(async move {
            let config: ClientConfig = if insecure {
                ClientConfig::builder()
                    .with_bind_default()
                    .with_no_cert_validation()
                    .build()
            } else {
                ClientConfig::builder().with_bind_default().with_native_certs().build()
            };

            Endpoint::client(config)
        })?;

        runtime.spawn(run_client(
            endpoint,
            url,
            protocol_id,
            inbound_tx,
            outbound_rx,
            rtt.clone(),
            server_tickrate.clone(),
        ));

        Ok(Self {
            _runtime: runtime,
            inbound_rx,
            outbound_tx,
            queue: InboundQueue::new(),
            id: None,
            connected: false,
            rtt,
            server_tickrate,
        })
    }
}

impl NetworkManager for WebTransportClient {
    fn poll(&mut self) -> Option<NetworkEvent<'_>> {
        self.queue.poll()
    }

    fn send(&mut self, _client: ClientId, channel: NetworkChannel, payload: &[u8]) {
        self.outbound_tx
            .send(Outgoing {
                target: Target::One(SERVER_ID),
                channel,
                payload: Bytes::copy_from_slice(payload),
            })
            .ok();
    }

    fn broadcast(&mut self, channel: NetworkChannel, payload: &[u8]) {
        self.send(SERVER_ID, channel, payload);
    }

    fn is_server(&self) -> bool {
        false
    }

    fn local_client(&self) -> Option<ClientId> {
        self.id
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn rtt(&self) -> f32 {
        self.rtt.load(Ordering::Relaxed) as f32 / 1_000_000.0
    }

    fn server_tickrate(&self) -> Option<f64> {
        let rate: f64 = f64::from_bits(self.server_tickrate.load(Ordering::Relaxed));
        (rate > 0.0).then_some(rate)
    }

    fn update(&mut self) {
        self.queue.reset();
        while let Ok(msg) = self.inbound_rx.try_recv() {
            match msg {
                Incoming::Connected(id) => {
                    self.id = Some(id);
                    self.connected = true;
                    self.queue.push(Inbound::Connected(id));
                }
                Incoming::Disconnected(id) => {
                    self.connected = false;
                    self.queue.push(Inbound::Disconnected(id));
                }
                Incoming::Message {
                    client,
                    channel,
                    payload,
                } => self.queue.push(Inbound::Message {
                    client,
                    channel,
                    payload,
                }),
            }
        }
    }

    fn flush(&mut self) {}
}

/// Builds the native WebTransport [`NetworkManager`] for `config`, degrading to a
/// [`NoOpNetwork`] (with a logged error) on failure. `tickrate` is the local
/// authoritative rate; a server embeds it in its welcome frame, a client
/// ignores its own value and adopts whatever the server announces instead.
pub fn build(config: &NetConfig, tickrate: f64) -> Box<dyn NetworkManager> {
    match &config.mode {
        NetMode::Server { bind } => {
            match WebTransportServer::new(*bind, &config.cert, tickrate, config.protocol_id, config.max_clients) {
                Ok(server) => {
                    log::info!("WebTransport server listening on {bind} (max {} clients).", config.max_clients);
                    Box::new(server)
                }
                Err(e) => {
                    log::error!("Failed to start WebTransport server on {bind}: {e}. Falling back to no-op network.");
                    Box::new(NoOpNetwork)
                }
            }
        }
        NetMode::Client { connect } => {
            match WebTransportClient::new(*connect, config.server_name.clone(), config.protocol_id) {
                Ok(client) => {
                    log::info!("WebTransport client connecting to {connect}.");
                    Box::new(client)
                }
                Err(e) => {
                    log::error!(
                        "Failed to start WebTransport client to {connect}: {e}. Falling back to no-op network."
                    );
                    Box::new(NoOpNetwork)
                }
            }
        }
        NetMode::Offline => Box::new(NoOpNetwork),
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    use redixel_core::{NetworkChannel, NetworkEvent, NetworkManager, SERVER_ID};

    use crate::config::{CertSource, DEFAULT_PROTOCOL_ID};

    use super::{HANDSHAKE_TIMEOUT, WebTransportClient, WebTransportServer};

    const SETTLE_BUDGET: Duration = Duration::from_secs(HANDSHAKE_TIMEOUT.as_secs() * 3);
    const REJECT_BUDGET: Duration = Duration::from_secs(3);
    const MAX_CLIENTS: usize = 4;

    fn server_on_free_port(tickrate: f64) -> WebTransportServer {
        server_with_capacity(tickrate, MAX_CLIENTS)
    }

    fn server_with_capacity(tickrate: f64, max_clients: usize) -> WebTransportServer {
        let bind: SocketAddr = "127.0.0.1:0".parse().expect("valid address");
        WebTransportServer::new(bind, &CertSource::SelfSigned, tickrate, DEFAULT_PROTOCOL_ID, max_clients)
            .expect("server binds")
    }

    fn loopback_pair() -> (WebTransportServer, WebTransportClient) {
        let server: WebTransportServer = server_on_free_port(60.0);
        let client: WebTransportClient =
            WebTransportClient::new(server.local_addr(), None, DEFAULT_PROTOCOL_ID).expect("client constructs");
        (server, client)
    }

    fn pump(
        server: &mut WebTransportServer,
        client: &mut WebTransportClient,
        budget: Duration,
        mut done: impl FnMut(&mut WebTransportServer, &mut WebTransportClient) -> bool,
    ) -> bool {
        let deadline: Instant = Instant::now() + budget;

        while Instant::now() < deadline {
            server.update();
            client.update();

            if done(server, client) {
                return true;
            }

            server.flush();
            client.flush();
            sleep(Duration::from_millis(5));
        }

        false
    }

    #[test]
    fn server_binds_and_reports_role() {
        let server: WebTransportServer = server_on_free_port(60.0);
        assert!(server.is_server());
        assert_eq!(server.local_client(), None);
    }

    #[test]
    fn webtransport_handshake_over_localhost() {
        let (mut server, mut client): (WebTransportServer, WebTransportClient) = loopback_pair();

        let connected: bool = pump(
            &mut server,
            &mut client,
            SETTLE_BUDGET,
            |_s: &mut WebTransportServer, c: &mut WebTransportClient| c.is_connected(),
        );

        assert!(connected, "client failed to connect over localhost WebTransport");
        assert_eq!(client.server_tickrate(), Some(60.0));
    }

    #[test]
    fn server_at_capacity_turns_away_further_clients() {
        let mut server: WebTransportServer = server_with_capacity(60.0, 1);
        let addr: SocketAddr = server.local_addr();

        let mut first: WebTransportClient =
            WebTransportClient::new(addr, None, DEFAULT_PROTOCOL_ID).expect("first client constructs");
        assert!(
            pump(
                &mut server,
                &mut first,
                SETTLE_BUDGET,
                |_s: &mut WebTransportServer, c: &mut WebTransportClient| c.is_connected()
            ),
            "the first client must fill the single slot"
        );

        let mut second: WebTransportClient =
            WebTransportClient::new(addr, None, DEFAULT_PROTOCOL_ID).expect("second client constructs");
        let admitted: bool = pump(
            &mut server,
            &mut second,
            REJECT_BUDGET,
            |_s: &mut WebTransportServer, c: &mut WebTransportClient| c.is_connected(),
        );

        assert!(!admitted, "server admitted a client past max_clients");
        assert!(first.is_connected(), "rejecting a new peer must not disturb the seated one");
    }

    #[allow(dead_code)]
    struct StalledPeer {
        runtime: tokio::runtime::Runtime,
        endpoint: wtransport::Endpoint<wtransport::endpoint::endpoint_side::Client>,
        connection: wtransport::Connection,
        streams: (wtransport::SendStream, wtransport::RecvStream),
    }

    fn stall_handshake(addr: SocketAddr) -> StalledPeer {
        use wtransport::{ClientConfig, Connection, Endpoint, RecvStream, SendStream, endpoint::endpoint_side};

        let runtime: tokio::runtime::Runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("stall runtime builds");

        let (endpoint, connection, streams): (Endpoint<endpoint_side::Client>, Connection, (SendStream, RecvStream)) =
            runtime.block_on(async move {
                let config: ClientConfig = ClientConfig::builder()
                    .with_bind_default()
                    .with_no_cert_validation()
                    .build();
                let endpoint: Endpoint<endpoint_side::Client> = Endpoint::client(config).expect("stall endpoint");
                let connection: Connection = endpoint
                    .connect(format!("https://{addr}"))
                    .await
                    .expect("stall connects");

                let streams: (SendStream, RecvStream) = connection
                    .open_bi()
                    .await
                    .expect("stall open_bi")
                    .await
                    .expect("stall bi ready");

                (endpoint, connection, streams)
            });

        StalledPeer {
            runtime,
            endpoint,
            connection,
            streams,
        }
    }

    #[test]
    fn stalled_handshake_frees_its_slot_after_the_timeout() {
        let handshake_timeout: Duration = Duration::from_millis(500);
        let bind: SocketAddr = "127.0.0.1:0".parse().expect("valid address");
        let mut server: WebTransportServer = WebTransportServer::with_handshake_timeout(
            bind,
            &CertSource::SelfSigned,
            60.0,
            DEFAULT_PROTOCOL_ID,
            1,
            handshake_timeout,
        )
        .expect("server binds");
        let addr: SocketAddr = server.local_addr();

        let stalled: StalledPeer = stall_handshake(addr);
        sleep(handshake_timeout * 3);

        let mut client: WebTransportClient =
            WebTransportClient::new(addr, None, DEFAULT_PROTOCOL_ID).expect("client constructs");
        let connected: bool = pump(
            &mut server,
            &mut client,
            SETTLE_BUDGET,
            |_s: &mut WebTransportServer, c: &mut WebTransportClient| c.is_connected(),
        );

        assert!(connected, "the slot was never reclaimed after the stalled handshake timed out");

        drop(stalled);
    }

    #[test]
    fn client_with_mismatched_protocol_id_is_rejected() {
        let server: WebTransportServer = server_on_free_port(60.0);
        let mut client: WebTransportClient =
            WebTransportClient::new(server.local_addr(), None, DEFAULT_PROTOCOL_ID ^ 0xFFFF)
                .expect("client constructs");
        let mut server: WebTransportServer = server;

        let connected: bool = pump(
            &mut server,
            &mut client,
            REJECT_BUDGET,
            |_s: &mut WebTransportServer, c: &mut WebTransportClient| c.is_connected(),
        );
        assert!(!connected, "server admitted a client with a mismatched protocol id");
    }

    #[test]
    fn server_announces_connect_before_the_first_message() {
        let (mut server, mut client): (WebTransportServer, WebTransportClient) = loopback_pair();
        let mut events: Vec<&'static str> = Vec::new();
        let mut sent: bool = false;

        pump(
            &mut server,
            &mut client,
            SETTLE_BUDGET,
            |s: &mut WebTransportServer, c: &mut WebTransportClient| {
                if c.is_connected() && !sent {
                    c.send(SERVER_ID, NetworkChannel::ReliableOrdered, b"first");
                    sent = true;
                }

                while let Some(event) = s.poll() {
                    match event {
                        NetworkEvent::Connected(..) => events.push("connected"),
                        NetworkEvent::Message(..) => events.push("message"),
                        NetworkEvent::Disconnected(..) => events.push("disconnected"),
                    }
                }

                events.contains(&"message")
            },
        );

        assert_eq!(
            events.first(),
            Some(&"connected"),
            "the server must observe Connected before the client's first Message, got {events:?}"
        );
    }

    #[test]
    fn webtransport_round_trip_over_localhost() {
        let (mut server, mut client): (WebTransportServer, WebTransportClient) = loopback_pair();

        let mut client_to_server_ok: bool = false;
        let mut server_to_client_ok: bool = false;
        let mut sent_from_client: bool = false;

        pump(
            &mut server,
            &mut client,
            SETTLE_BUDGET,
            |server: &mut WebTransportServer, client: &mut WebTransportClient| {
                if client.is_connected() && !sent_from_client {
                    client.send(SERVER_ID, NetworkChannel::ReliableOrdered, b"ping");
                    sent_from_client = true;
                }

                while let Some(event) = server.poll() {
                    if let NetworkEvent::Message(_, NetworkChannel::ReliableOrdered, payload) = event
                        && payload == b"ping"
                    {
                        client_to_server_ok = true;
                    }
                }

                if client_to_server_ok && !server_to_client_ok {
                    server.broadcast(NetworkChannel::UnreliableSequenced, b"pong");
                }

                while let Some(event) = client.poll() {
                    if let NetworkEvent::Message(from, NetworkChannel::UnreliableSequenced, payload) = event
                        && from == SERVER_ID
                        && payload == b"pong"
                    {
                        server_to_client_ok = true;
                    }
                }

                client_to_server_ok && server_to_client_ok
            },
        );

        assert!(client_to_server_ok, "server never received the client's reliable message");
        assert!(server_to_client_ok, "client never received the server's unreliable reply");
    }

    #[test]
    fn oversized_unreliable_message_is_fragmented_and_reassembled() {
        let (mut server, mut client): (WebTransportServer, WebTransportClient) = loopback_pair();

        let payload: Vec<u8> = (0..32_768u32).map(|i: u32| (i % 251) as u8).collect();
        let mut delivered: bool = false;
        let mut sent: bool = false;

        pump(
            &mut server,
            &mut client,
            SETTLE_BUDGET,
            |server: &mut WebTransportServer, client| {
                if client.is_connected() && !sent {
                    sent = true;
                }

                if sent {
                    server.broadcast(NetworkChannel::UnreliableSequenced, &payload);
                }

                while let Some(event) = client.poll() {
                    if let NetworkEvent::Message(_, channel, received) = event {
                        assert_eq!(
                            channel,
                            NetworkChannel::UnreliableSequenced,
                            "a fragmented unreliable message must stay on its own channel"
                        );
                        if received == payload.as_slice() {
                            delivered = true;
                        }
                    }
                }

                delivered
            },
        );

        assert!(delivered, "client never reassembled the fragmented unreliable message");
    }
}
