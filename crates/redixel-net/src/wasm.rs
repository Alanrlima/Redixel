use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    net::SocketAddr,
    rc::Rc,
};

use js_sys::Uint8Array;

use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};

use web_sys::{
    ReadableStreamDefaultReader, WebTransport, WebTransportBidirectionalStream, WebTransportDatagramDuplexStream,
    WebTransportHash, WebTransportOptions, WritableStreamDefaultWriter,
};

use redixel_core::{ClientId, NetworkChannel, NetworkEvent, NetworkManager, NoOpNetwork, SERVER_ID};

use crate::{
    config::{NetConfig, NetMode},
    frame::parse_welcome,
    inbound::{Inbound, InboundQueue},
    seq,
};

/// Hard cap on a single reliable frame — mirrors native's `MAX_FRAME_LEN`
/// (`webtransport.rs`, which doesn't exist on wasm32, so it isn't shared).
/// The receiver allocates exactly the length a peer announces, so this bounds
/// that allocation.
const MAX_FRAME_LEN: usize = 4 * 1024 * 1024;

/// Shared inbound buffer: spawned `spawn_local` tasks push into it, `update`
/// drains it into the client's [`InboundQueue`]. wasm32 has no OS threads, so
/// unlike native's mpsc channel bridge, this is a plain `Rc<RefCell<..>>` —
/// tasks and `update`/`poll` only ever interleave at `.await` points on the
/// same thread.
type Mailbox = Rc<RefCell<VecDeque<Inbound<Vec<u8>>>>>;

/// The reliable-stream writer, datagram writer, and the datagram duplex
/// stream itself (needed for `max_datagram_size()`), available once the
/// handshake completes. Cloning any of these clones the underlying JS
/// reference, not the stream itself.
#[derive(Clone)]
struct Writers {
    reliable: WritableStreamDefaultWriter,
    datagram_writer: WritableStreamDefaultWriter,
    datagrams: WebTransportDatagramDuplexStream,
}

/// Builds the `WebTransportOptions` pinning `hash` via
/// `serverCertificateHashes`, for connecting to a self-signed/LAN server (see
/// [`NetConfig::server_cert_hash`]).
fn build_options(hash: [u8; 32]) -> WebTransportOptions {
    let options: WebTransportOptions = WebTransportOptions::new();

    let value: Uint8Array = Uint8Array::from(hash.as_slice());
    let wt_hash: WebTransportHash = WebTransportHash::new();
    wt_hash.set_algorithm("sha-256");
    wt_hash.set_value_u8_array(&value);

    options.set_server_certificate_hashes(&[wt_hash]);
    options
}

/// Constructs the `WebTransport` session object for `url`, pinning
/// `config.server_cert_hash` when set (self-signed/LAN), otherwise leaving
/// default WebPKI validation (CA-trusted domain cert).
fn new_transport(url: &str, config: &NetConfig) -> Result<WebTransport, JsValue> {
    match config.server_cert_hash {
        Some(hash) => WebTransport::new_with_options(url, &build_options(hash)),
        None => WebTransport::new(url),
    }
}

/// Writes one length-prefixed (`[u32 len][payload]`, little-endian) frame to
/// a reliable stream writer — the wasm equivalent of native's `write_frame`.
async fn write_frame_wasm(writer: &WritableStreamDefaultWriter, payload: &[u8]) -> Result<(), JsValue> {
    let mut framed: Vec<u8> = Vec::with_capacity(4 + payload.len());
    framed.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    framed.extend_from_slice(payload);

    let chunk: Uint8Array = Uint8Array::from(framed.as_slice());
    JsFuture::from(writer.write_with_chunk(&JsValue::from(chunk))).await?;
    Ok(())
}

/// Awaits the next `{ done, value }` result from `reader.read()`, returning
/// the chunk as a `Uint8Array`, or `None` once the stream ends or errors.
/// Reads the result via `js_sys::Reflect` rather than a typed dictionary
/// binding, since `ReadableStreamReadResult` isn't otherwise needed.
async fn read_next_chunk(reader: &ReadableStreamDefaultReader) -> Option<Uint8Array> {
    let result: JsValue = JsFuture::from(reader.read()).await.ok()?;

    let done: bool = js_sys::Reflect::get(&result, &JsValue::from_str("done"))
        .ok()?
        .as_bool()
        .unwrap_or(true);

    if done {
        return None;
    }

    let value: JsValue = js_sys::Reflect::get(&result, &JsValue::from_str("value")).ok()?;
    Some(value.unchecked_into())
}

/// Reads one length-prefixed frame from `reader`, accumulating `buf` across
/// however many chunked `read()` calls it takes. Unlike native's
/// `RecvStream::read_exact`, the browser gives no guarantee that a stream
/// chunk aligns with a frame boundary. `None` once the stream ends, errors, or
/// announces a frame larger than [`MAX_FRAME_LEN`].
async fn read_frame_wasm(reader: &ReadableStreamDefaultReader, buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    loop {
        if buf.len() >= 4 {
            let len_bytes: [u8; 4] = buf[0..4].try_into().expect("checked buf.len() >= 4 above");
            let len: usize = u32::from_le_bytes(len_bytes) as usize;

            if len > MAX_FRAME_LEN {
                log::warn!(
                    "Peer announced a {len}-byte frame over the {MAX_FRAME_LEN}-byte limit; dropping the stream."
                );
                return None;
            }

            if buf.len() >= 4 + len {
                let payload: Vec<u8> = buf[4..4 + len].to_vec();
                buf.drain(0..4 + len);
                return Some(payload);
            }
        }

        let chunk: Uint8Array = read_next_chunk(reader).await?;
        buf.extend(chunk.to_vec());
    }
}

/// Forwards inbound reliable frames as [`Inbound::Message`]s until the stream
/// ends, then signals disconnection — the wasm counterpart to native's
/// `reliable_reader` plus its connection-closed watcher, since a browser
/// `WebTransport` exposes no separate connection-level close signal here.
async fn reliable_reader_wasm(reader: ReadableStreamDefaultReader, mut buf: Vec<u8>, mailbox: Mailbox) {
    while let Some(payload) = read_frame_wasm(&reader, &mut buf).await {
        mailbox.borrow_mut().push_back(Inbound::Message {
            client: SERVER_ID,
            channel: NetworkChannel::ReliableOrdered,
            payload,
        });
    }

    mailbox.borrow_mut().push_back(Inbound::Disconnected(SERVER_ID));
}

/// Reassembles inbound datagram fragments (newest-wins) into whole messages
/// and forwards them as [`Inbound::Message`]s — the wasm counterpart to
/// native's `datagram_reader`. Each `read()` yields one whole datagram
/// (message boundaries are preserved, unlike the byte-stream reader above).
async fn datagram_reader_wasm(reader: ReadableStreamDefaultReader, mailbox: Mailbox) {
    let mut reassembler: seq::Reassembler = seq::Reassembler::new();

    while let Some(chunk) = read_next_chunk(&reader).await {
        let bytes: Vec<u8> = chunk.to_vec();

        let Some((seq_num, index, count, fragment)) = seq::split(&bytes) else {
            continue;
        };

        let Some(payload) = reassembler.push(seq_num, index, count, fragment) else {
            continue;
        };

        mailbox.borrow_mut().push_back(Inbound::Message {
            client: SERVER_ID,
            channel: NetworkChannel::UnreliableSequenced,
            payload,
        });
    }
}

/// Fragments `payload` across one or more datagram-sized frames sized to
/// `max_datagram`, appending each already-framed fragment to `out` — the
/// synchronous half of native's `send_unreliable`. Browser datagram writes
/// are async (`WritableStreamDefaultWriter::write_with_chunk`), so wasm
/// splits framing (here, synchronous) from the actual write (`flush`'s
/// spawned task) — native doesn't need this split since `send_datagram` is
/// itself synchronous.
///
/// Unreliable by contract: a message that cannot be sent is **dropped**,
/// never promoted onto the reliable stream, matching native.
fn frame_unreliable(
    max_datagram: u32,
    scratch: &mut Vec<u8>,
    send_seq: &mut u32,
    payload: &[u8],
    out: &mut Vec<Vec<u8>>,
) {
    let max_datagram: usize = max_datagram as usize;
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
        out.push(scratch.clone());
        return;
    }

    for (index, fragment) in payload.chunks(chunk).enumerate() {
        seq::frame(scratch, current, index as u16, count as u16, fragment);
        out.push(scratch.clone());
    }
}

/// Drives the handshake to completion: awaits the session, opens the
/// bidirectional stream, announces `protocol_id` and exchanges the
/// hello/welcome (reusing the same wire format as native via
/// [`parse_welcome`]), then hands off to the reliable and datagram reader
/// tasks. Any failure along the way is logged and simply leaves the client
/// permanently unconnected — matching native's fail-soft behavior for a
/// connection that never completes.
async fn connect_wasm(
    transport: WebTransport,
    protocol_id: u64,
    mailbox: Mailbox,
    writers: Rc<RefCell<Option<Writers>>>,
    server_tickrate: Rc<Cell<f64>>,
) {
    if let Err(e) = JsFuture::from(transport.ready()).await {
        log::error!("wasm webtransport: connection failed: {e:?}");
        return;
    }

    let bidi: WebTransportBidirectionalStream = match JsFuture::from(transport.create_bidirectional_stream()).await {
        Ok(stream) => stream.unchecked_into(),
        Err(e) => {
            log::error!("wasm webtransport: create_bidirectional_stream failed: {e:?}");
            return;
        }
    };

    let send_writer: WritableStreamDefaultWriter = match bidi.writable().get_writer() {
        Ok(writer) => writer,
        Err(e) => {
            log::error!("wasm webtransport: get_writer failed: {e:?}");
            return;
        }
    };

    let recv_reader: ReadableStreamDefaultReader = bidi.readable().get_reader().unchecked_into();

    if write_frame_wasm(&send_writer, &protocol_id.to_le_bytes())
        .await
        .is_err()
    {
        log::error!("wasm webtransport: failed to send hello");
        return;
    }

    let mut buf: Vec<u8> = Vec::new();
    let welcome: Vec<u8> = match read_frame_wasm(&recv_reader, &mut buf).await {
        Some(frame) => frame,
        None => {
            log::error!("wasm webtransport: server closed before the welcome frame (protocol id mismatch?)");
            return;
        }
    };

    let (id, tickrate): (ClientId, f64) = match parse_welcome(&welcome) {
        Some(parsed) => parsed,
        None => {
            log::error!("wasm webtransport: malformed welcome");
            return;
        }
    };

    server_tickrate.set(tickrate);

    let datagrams: WebTransportDatagramDuplexStream = transport.datagrams();
    let dgram_writer: WritableStreamDefaultWriter = match datagrams.writable().get_writer() {
        Ok(writer) => writer,
        Err(e) => {
            log::error!("wasm webtransport: datagram get_writer failed: {e:?}");
            return;
        }
    };

    let dgram_reader: ReadableStreamDefaultReader = datagrams.readable().get_reader().unchecked_into();

    *writers.borrow_mut() = Some(Writers {
        reliable: send_writer,
        datagram_writer: dgram_writer,
        datagrams,
    });

    mailbox.borrow_mut().push_back(Inbound::Connected(id));
    spawn_local(reliable_reader_wasm(recv_reader, buf, mailbox.clone()));
    spawn_local(datagram_reader_wasm(dgram_reader, mailbox));
}

/// Browser client connection to a [`WebTransportServer`](crate::WebTransportServer),
/// speaking the same wire protocol over `web-sys`'s `WebTransport` bindings.
/// wasm32 is single-threaded, so unlike native/mobile there is no background
/// runtime here: `spawn_local` tasks and the synchronous [`NetworkManager`]
/// calls interleave on the same thread.
pub struct WasmWebTransportClient {
    mailbox: Mailbox,
    writers: Rc<RefCell<Option<Writers>>>,
    outbound_reliable: Vec<Vec<u8>>,
    outbound_unreliable: Vec<Vec<u8>>,
    queue: InboundQueue<Vec<u8>>,
    id: Option<ClientId>,
    connected: bool,
    server_tickrate: Rc<Cell<f64>>,
    send_seq: u32,
}

impl WasmWebTransportClient {
    /// Starts connecting to `connect`, announcing `config.protocol_id` in its
    /// hello frame — a server configured with a different id closes the
    /// connection immediately. With `config.server_name` set, uses
    /// `https://{name}:{port}` (production, WebPKI-validated); otherwise
    /// `https://{connect}`, pinning `config.server_cert_hash` when present
    /// (LAN / self-signed — see [`NetConfig::with_server_cert_hash`]).
    /// Connection itself happens asynchronously; construction only fails if
    /// the browser rejects the `WebTransport` session synchronously.
    pub fn new(connect: SocketAddr, config: &NetConfig) -> Result<Self, JsValue> {
        let url: String = match &config.server_name {
            Some(name) => format!("https://{name}:{}", connect.port()),
            None => format!("https://{connect}"),
        };

        let transport: WebTransport = new_transport(&url, config)?;
        let mailbox: Mailbox = Rc::new(RefCell::new(VecDeque::new()));
        let writers: Rc<RefCell<Option<Writers>>> = Rc::new(RefCell::new(None));
        let server_tickrate: Rc<Cell<f64>> = Rc::new(Cell::new(0.0));

        spawn_local(connect_wasm(
            transport,
            config.protocol_id,
            mailbox.clone(),
            writers.clone(),
            server_tickrate.clone(),
        ));

        Ok(Self {
            mailbox,
            writers,
            outbound_reliable: Vec::new(),
            outbound_unreliable: Vec::new(),
            queue: InboundQueue::new(),
            id: None,
            connected: false,
            server_tickrate,
            send_seq: 0,
        })
    }
}

// Uses the trait's default `rtt()` (`0.0`): `WebTransportStats`
// (`transport.get_stats()`) is new and inconsistent across browsers relative
// to base WebTransport support — not worth a second polling task for a
// debug-only metric in this first cut.
impl NetworkManager for WasmWebTransportClient {
    fn poll(&mut self) -> Option<NetworkEvent<'_>> {
        self.queue.poll()
    }

    fn send(&mut self, _client: ClientId, channel: NetworkChannel, payload: &[u8]) {
        match channel {
            NetworkChannel::ReliableOrdered => self.outbound_reliable.push(payload.to_vec()),
            NetworkChannel::UnreliableSequenced => self.outbound_unreliable.push(payload.to_vec()),
        }
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

    fn server_tickrate(&self) -> Option<f64> {
        let rate: f64 = self.server_tickrate.get();
        (rate > 0.0).then_some(rate)
    }

    fn update(&mut self) {
        self.queue.reset();

        let mut mailbox: std::cell::RefMut<'_, VecDeque<Inbound<Vec<u8>>>> = self.mailbox.borrow_mut();
        while let Some(item) = mailbox.pop_front() {
            match &item {
                Inbound::Connected(id) => {
                    self.id = Some(*id);
                    self.connected = true;
                }
                Inbound::Disconnected(_) => self.connected = false,
                Inbound::Message { .. } => {}
            }

            self.queue.push(item);
        }
    }

    fn flush(&mut self) {
        if self.outbound_reliable.is_empty() && self.outbound_unreliable.is_empty() {
            return;
        }

        let writers: Option<Writers> = self.writers.borrow().clone();
        let Some(writers) = writers else {
            return;
        };

        let reliable: Vec<Vec<u8>> = std::mem::take(&mut self.outbound_reliable);
        let unreliable_raw: Vec<Vec<u8>> = std::mem::take(&mut self.outbound_unreliable);

        let max_datagram: u32 = writers.datagrams.max_datagram_size();
        let mut framed_fragments: Vec<Vec<u8>> = Vec::new();
        let mut scratch: Vec<u8> = Vec::new();
        for payload in &unreliable_raw {
            frame_unreliable(max_datagram, &mut scratch, &mut self.send_seq, payload, &mut framed_fragments);
        }

        spawn_local(async move {
            for payload in reliable {
                if payload.len() > MAX_FRAME_LEN {
                    log::error!(
                        "Dropping a {}-byte reliable message over the {MAX_FRAME_LEN}-byte frame limit.",
                        payload.len()
                    );

                    continue;
                }

                if write_frame_wasm(&writers.reliable, &payload).await.is_err() {
                    break;
                }
            }

            for framed in framed_fragments {
                let chunk: Uint8Array = Uint8Array::from(framed.as_slice());
                if let Err(e) = JsFuture::from(writers.datagram_writer.write_with_chunk(&JsValue::from(chunk))).await {
                    log::warn!("wasm webtransport: datagram write failed: {e:?}");
                }
            }
        });
    }
}

/// Builds the browser WebTransport [`NetworkManager`] for `config`, degrading
/// to a [`NoOpNetwork`] (with a logged error) on failure. `tickrate` is
/// unused here (unlike native, a browser can never be `NetMode::Server`, so
/// there is no welcome frame for this side to embed a tickrate into) but kept
/// for signature parity with [`crate::webtransport::build`].
pub fn build(config: &NetConfig, _tickrate: f64) -> Box<dyn NetworkManager> {
    match &config.mode {
        NetMode::Server { .. } => {
            log::error!("WebTransport server mode is not supported in the browser. Falling back to no-op network.");
            Box::new(NoOpNetwork)
        }
        NetMode::Client { connect } => match WasmWebTransportClient::new(*connect, config) {
            Ok(client) => {
                log::info!("WebTransport client connecting to {connect}.");
                Box::new(client)
            }
            Err(e) => {
                log::error!("Failed to start WebTransport client to {connect}: {e:?}. Falling back to no-op network.");
                Box::new(NoOpNetwork)
            }
        },
        NetMode::Offline => Box::new(NoOpNetwork),
    }
}
