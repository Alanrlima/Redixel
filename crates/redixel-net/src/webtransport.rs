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
    ClientConfig, Connection, Endpoint, Identity, RecvStream, SendStream, ServerConfig,
    datagram::Datagram,
    endpoint::{IncomingSession, SessionRequest, endpoint_side},
    error::{SendDatagramError, StreamWriteError},
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
struct Outgoing {
    target: Target,
    channel: NetworkChannel,
    payload: Vec<u8>,
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

/// Reads one length-prefixed frame, or `None` once the stream ends or errors.
async fn read_frame(recv: &mut RecvStream) -> Option<Vec<u8>> {
    let mut len_buf: [u8; 4] = [0; 4];
    recv.read_exact(&mut len_buf).await.ok()?;
    let len: usize = u32::from_le_bytes(len_buf) as usize;
    let mut buf: Vec<u8> = vec![0; len];
    recv.read_exact(&mut buf).await.ok()?;
    Some(buf)
}

/// Spawns the reliable writer and reliable/datagram readers for one connection,
/// tagging inbound messages with `msg_client`. Returns the reliable-send channel.
fn spawn_connection_tasks(
    conn: Arc<Connection>,
    send: SendStream,
    recv: RecvStream,
    msg_client: ClientId,
    inbound_tx: UnboundedSender<Incoming>,
) -> UnboundedSender<Vec<u8>> {
    let (reliable_tx, reliable_rx): (UnboundedSender<Vec<u8>>, UnboundedReceiver<Vec<u8>>) = unbounded_channel();
    tokio::spawn(reliable_writer(send, reliable_rx));
    tokio::spawn(reliable_reader(recv, msg_client, inbound_tx.clone()));
    tokio::spawn(datagram_reader(conn, msg_client, inbound_tx));
    reliable_tx
}

/// Drains the reliable-send channel onto the connection's stream.
async fn reliable_writer(mut send: SendStream, mut rx: UnboundedReceiver<Vec<u8>>) {
    while let Some(payload) = rx.recv().await {
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

/// Forwards inbound datagrams (newest-wins) as [`Incoming::Message`]s.
async fn datagram_reader(conn: Arc<Connection>, client: ClientId, tx: UnboundedSender<Incoming>) {
    let mut last_seq: Option<u32> = None;
    loop {
        let dgram: Datagram = match conn.receive_datagram().await {
            Ok(dgram) => dgram,
            Err(_) => break,
        };

        let data: Bytes = dgram.payload();
        let seq_num: Option<u32> = seq::split(&data).map(|(s, _): (u32, &[u8])| s);
        if let Some(seq_num) = seq_num
            && seq::accept_seq(&mut last_seq, seq_num)
        {
            let payload: Vec<u8> = data[seq::SEQ_LEN..].to_vec();

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
        reliable_tx: UnboundedSender<Vec<u8>>,
    },
    Closed {
        client: ClientId,
    },
}

/// A live peer as seen by the server's dispatcher.
struct ConnHandle {
    conn: Arc<Connection>,
    reliable_tx: UnboundedSender<Vec<u8>>,
}

/// Sends `framed` as a datagram, falling back to the reliable stream (with the
/// unframed `raw` payload) when the datagram is too large.
fn send_datagram_or_reliable(handle: &ConnHandle, framed: &[u8], raw: &[u8]) {
    match handle.conn.send_datagram(framed) {
        Ok(()) => {}
        Err(SendDatagramError::TooLarge) => {
            handle.reliable_tx.send(raw.to_vec()).ok();
        }
        Err(_) => {}
    }
}

/// Routes one outbound command to the addressed peer(s).
fn route_outbound(conns: &HashMap<ClientId, ConnHandle>, out: Outgoing, send_seq: &mut u32) {
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
        NetworkChannel::UnreliableSequenced => {
            let mut framed: Vec<u8> = Vec::new();
            seq::frame(&mut framed, *send_seq, &out.payload);
            *send_seq = send_seq.wrapping_add(1);

            match out.target {
                Target::One(client) => {
                    if let Some(handle) = conns.get(&client) {
                        send_datagram_or_reliable(handle, &framed, &out.payload);
                    }
                }
                Target::All => {
                    for handle in conns.values() {
                        send_datagram_or_reliable(handle, &framed, &out.payload);
                    }
                }
            }
        }
    }
}

/// Server event loop: accepts connections and dispatches outbound commands.
async fn run_server(
    endpoint: Endpoint<endpoint_side::Server>,
    tickrate: f64,
    inbound_tx: UnboundedSender<Incoming>,
    mut outbound_rx: UnboundedReceiver<Outgoing>,
) {
    let mut conns: HashMap<ClientId, ConnHandle> = HashMap::new();
    let (ctrl_tx, mut ctrl_rx): (UnboundedSender<Ctrl>, UnboundedReceiver<Ctrl>) = unbounded_channel();
    let mut next_id: ClientId = 1;
    let mut send_seq: u32 = 0;

    loop {
        tokio::select! {
            incoming = endpoint.accept() => {
                let id: ClientId = next_id;
                next_id += 1;
                tokio::spawn(handshake_server(incoming, id, tickrate, inbound_tx.clone(), ctrl_tx.clone()));
            }
            Some(ctrl) = ctrl_rx.recv() => match ctrl {
                Ctrl::Ready { client, conn, reliable_tx } => {
                    conns.insert(client, ConnHandle { conn, reliable_tx });
                }
                Ctrl::Closed { client } => {
                    conns.remove(&client);
                }
            },
            Some(out) = outbound_rx.recv() => route_outbound(&conns, out, &mut send_seq),
            else => break,
        }
    }
}

/// Completes one incoming session: accepts the connection and reliable stream,
/// sends the assigned id plus the server's authoritative tickrate, spawns
/// per-connection tasks, and registers the peer.
async fn handshake_server(
    incoming: IncomingSession,
    id: ClientId,
    tickrate: f64,
    inbound_tx: UnboundedSender<Incoming>,
    ctrl_tx: UnboundedSender<Ctrl>,
) {
    let session_request: SessionRequest = match incoming.await {
        Ok(request) => request,
        Err(e) => {
            log::warn!("webtransport incoming session: {e}");
            return;
        }
    };

    let connection: Connection = match session_request.accept().await {
        Ok(connection) => connection,
        Err(e) => {
            log::warn!("webtransport accept: {e}");
            return;
        }
    };

    let conn: Arc<Connection> = Arc::new(connection);
    let (mut send, mut recv): (SendStream, RecvStream) = match conn.accept_bi().await {
        Ok(streams) => streams,
        Err(e) => {
            log::warn!("webtransport accept_bi: {e}");
            return;
        }
    };

    let _hello: Option<Vec<u8>> = read_frame(&mut recv).await;
    let mut welcome: Vec<u8> = Vec::with_capacity(16);

    welcome.extend_from_slice(&id.to_le_bytes());
    welcome.extend_from_slice(&tickrate.to_le_bytes());
    if write_frame(&mut send, &welcome).await.is_err() {
        return;
    }

    let reliable_tx: UnboundedSender<Vec<u8>> =
        spawn_connection_tasks(conn.clone(), send, recv, id, inbound_tx.clone());

    let watch_conn: Arc<Connection> = conn.clone();
    let watch_tx: UnboundedSender<Incoming> = inbound_tx.clone();
    let watch_ctrl: UnboundedSender<Ctrl> = ctrl_tx.clone();

    tokio::spawn(async move {
        watch_conn.closed().await;
        watch_tx.send(Incoming::Disconnected(id)).ok();
        watch_ctrl.send(Ctrl::Closed { client: id }).ok();
    });

    ctrl_tx
        .send(Ctrl::Ready {
            client: id,
            conn,
            reliable_tx,
        })
        .ok();

    inbound_tx.send(Incoming::Connected(id)).ok();
}

/// Authoritative server over WebTransport (desktop + Android), QUIC/HTTP-3
/// via [`wtransport`]. Owns a background tokio runtime bridging its async API
/// to the engine's synchronous [`NetworkManager`](redixel_core::NetworkManager).
pub struct WebTransportServer {
    _runtime: Runtime,
    local_addr: SocketAddr,
    inbound_rx: UnboundedReceiver<Incoming>,
    outbound_tx: UnboundedSender<Outgoing>,
    queue: InboundQueue<Vec<u8>>,
}

impl WebTransportServer {
    /// Binds a WebTransport endpoint on `bind` using `cert` and starts serving on
    /// a background tokio runtime. `tickrate` is embedded in the welcome frame
    /// sent to every connecting client.
    pub fn new(bind: SocketAddr, cert: &CertSource, tickrate: f64) -> io::Result<Self> {
        let runtime: Runtime = Builder::new_multi_thread().enable_all().build()?;
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
        runtime.spawn(run_server(endpoint, tickrate, inbound_tx, outbound_rx));

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
                payload: payload.to_vec(),
            })
            .ok();
    }

    fn broadcast(&mut self, channel: NetworkChannel, payload: &[u8]) {
        self.outbound_tx
            .send(Outgoing {
                target: Target::All,
                channel,
                payload: payload.to_vec(),
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

    fn update(&mut self, _dt: f64) {
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

/// Client event loop: connects, learns its assigned id and the server's
/// tickrate, then dispatches outbound traffic.
async fn run_client(
    endpoint: Endpoint<endpoint_side::Client>,
    url: String,
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

    if write_frame(&mut send, &[]).await.is_err() {
        return;
    }

    let welcome: Vec<u8> = match read_frame(&mut recv).await {
        Some(frame) => frame,
        None => {
            log::error!("webtransport: server closed before welcome");
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

    let tickrate: f64 = match welcome.get(8..16).and_then(|b: &[u8]| b.try_into().ok()) {
        Some(bytes) => f64::from_le_bytes(bytes),
        None => {
            log::error!("webtransport: welcome frame missing tickrate");
            return;
        }
    };

    server_tickrate.store(tickrate.to_bits(), Ordering::Relaxed);
    inbound_tx.send(Incoming::Connected(id)).ok();

    let reliable_tx: UnboundedSender<Vec<u8>> =
        spawn_connection_tasks(conn.clone(), send, recv, SERVER_ID, inbound_tx.clone());

    let watch_conn: Arc<Connection> = conn.clone();
    let watch_tx: UnboundedSender<Incoming> = inbound_tx.clone();
    tokio::spawn(async move {
        watch_conn.closed().await;
        watch_tx.send(Incoming::Disconnected(id)).ok();
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

    let mut send_seq: u32 = 0;
    while let Some(out) = outbound_rx.recv().await {
        match out.channel {
            NetworkChannel::ReliableOrdered => {
                reliable_tx.send(out.payload).ok();
            }
            NetworkChannel::UnreliableSequenced => {
                let mut framed: Vec<u8> = Vec::new();
                seq::frame(&mut framed, send_seq, &out.payload);
                send_seq = send_seq.wrapping_add(1);
                match conn.send_datagram(&framed) {
                    Ok(()) => {}
                    Err(SendDatagramError::TooLarge) => {
                        reliable_tx.send(out.payload).ok();
                    }
                    Err(_) => {}
                }
            }
        }
    }
}

/// Client connection to a [`WebTransportServer`], same async/sync bridge.
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
    /// Connects to `connect`. With `server_name` set, uses `https://{name}:{port}`
    /// and validates the cert (production); otherwise `https://{ip}:{port}` with
    /// validation disabled (LAN / self-signed).
    pub fn new(connect: SocketAddr, server_name: Option<String>) -> io::Result<Self> {
        let runtime: Runtime = Builder::new_multi_thread().enable_all().build()?;
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
                payload: payload.to_vec(),
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

    fn update(&mut self, _dt: f64) {
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
        NetMode::Server { bind } => match WebTransportServer::new(*bind, &config.cert, tickrate) {
            Ok(server) => {
                log::info!("WebTransport server listening on {bind}.");
                Box::new(server)
            }
            Err(e) => {
                log::error!("Failed to start WebTransport server on {bind}: {e}. Falling back to no-op network.");
                Box::new(NoOpNetwork)
            }
        },
        NetMode::Client { connect } => match WebTransportClient::new(*connect, config.server_name.clone()) {
            Ok(client) => {
                log::info!("WebTransport client connecting to {connect}.");
                Box::new(client)
            }
            Err(e) => {
                log::error!("Failed to start WebTransport client to {connect}: {e}. Falling back to no-op network.");
                Box::new(NoOpNetwork)
            }
        },
        NetMode::Offline => Box::new(NoOpNetwork),
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::thread::sleep;
    use std::time::Duration;

    use redixel_core::{NetworkChannel, NetworkEvent, NetworkManager, SERVER_ID};

    use crate::config::CertSource;

    use super::{WebTransportClient, WebTransportServer};

    const DT: f64 = 1.0 / 60.0;

    fn loopback_pair() -> (WebTransportServer, WebTransportClient) {
        let bind: SocketAddr = "127.0.0.1:0".parse().expect("valid address");
        let server: WebTransportServer =
            WebTransportServer::new(bind, &CertSource::SelfSigned, 60.0).expect("server binds");
        let client: WebTransportClient = WebTransportClient::new(server.local_addr(), None).expect("client constructs");
        (server, client)
    }

    #[test]
    fn server_binds_and_reports_role() {
        let bind: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let server: WebTransportServer =
            WebTransportServer::new(bind, &CertSource::SelfSigned, 60.0).expect("server binds");
        assert!(server.is_server());
        assert_eq!(server.local_client(), None);
    }

    #[test]
    fn webtransport_handshake_over_localhost() {
        let (mut server, mut client): (WebTransportServer, WebTransportClient) = loopback_pair();

        let mut connected: bool = false;
        for _ in 0..2000 {
            server.update(DT);
            client.update(DT);
            server.flush();
            client.flush();
            if client.is_connected() {
                connected = true;
                break;
            }
            sleep(Duration::from_millis(5));
        }

        assert!(connected, "client failed to connect over localhost WebTransport");
        assert_eq!(client.server_tickrate(), Some(60.0));
    }

    #[test]
    fn webtransport_round_trip_over_localhost() {
        let (mut server, mut client): (WebTransportServer, WebTransportClient) = loopback_pair();

        let mut client_to_server_ok: bool = false;
        let mut server_to_client_ok: bool = false;
        let mut sent_from_client: bool = false;

        for _ in 0..3000 {
            server.update(DT);
            client.update(DT);

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

            server.flush();
            client.flush();

            if client_to_server_ok && server_to_client_ok {
                break;
            }
            sleep(Duration::from_millis(5));
        }

        assert!(client_to_server_ok, "server never received the client's reliable message");
        assert!(server_to_client_ok, "client never received the server's unreliable reply");
    }
}
