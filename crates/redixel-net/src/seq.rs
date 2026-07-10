/// Bytes of header prepended to each unreliable datagram:
/// `[u32 seq][u16 fragment index][u16 fragment count]`, little-endian.
#[cfg(not(target_arch = "wasm32"))]
pub const HEADER_LEN: usize = 8;

/// Most fragments one unreliable message may be split into.
///
/// A peer picks `count` freely, and the receiver sizes its slot table from it,
/// so this bounds the allocation a single datagram can provoke. At a ~1.2 KB
/// datagram limit this still covers messages past [`MAX_MESSAGE_LEN`].
#[cfg(not(target_arch = "wasm32"))]
pub const MAX_FRAGMENTS: u16 = 1024;

/// Upper bound on a reassembled unreliable message. Fragments accumulate in
/// memory until the message completes, so this bounds what a peer can pin per
/// connection before anything is delivered.
#[cfg(not(target_arch = "wasm32"))]
pub const MAX_MESSAGE_LEN: usize = 1024 * 1024;

/// Returns `true` if `a` is strictly newer than `b`, using serial-number
/// arithmetic (RFC 1982) so the comparison still works after `u32` wraps.
#[inline]
pub fn seq_greater(a: u32, b: u32) -> bool {
    const HALF: u32 = u32::MAX / 2;
    ((a > b) && (a - b <= HALF)) || ((a < b) && (b - a > HALF))
}

/// Accepts `seq` if it is newer than the last accepted sequence `last`, updating
/// `last` on acceptance.
#[inline]
pub fn accept_seq(last: &mut Option<u32>, seq: u32) -> bool {
    let accept: bool = match *last {
        None => true,
        Some(prev) => seq_greater(seq, prev),
    };

    if accept {
        *last = Some(seq);
    }

    accept
}

/// Writes the header for fragment `index` of `count` belonging to `seq` into
/// `out` (cleared first), then appends `payload`, reusing `out`'s capacity.
#[cfg(not(target_arch = "wasm32"))]
#[inline]
pub fn frame(out: &mut Vec<u8>, seq: u32, index: u16, count: u16, payload: &[u8]) {
    out.clear();
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&index.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(payload);
}

/// Splits a received datagram into `(seq, fragment index, fragment count, payload)`,
/// or `None` if it is too short to contain a header.
#[cfg(not(target_arch = "wasm32"))]
#[inline]
pub fn split(msg: &[u8]) -> Option<(u32, u16, u16, &[u8])> {
    if msg.len() < HEADER_LEN {
        return None;
    }

    let seq: u32 = u32::from_le_bytes(msg[0..4].try_into().ok()?);
    let index: u16 = u16::from_le_bytes(msg[4..6].try_into().ok()?);
    let count: u16 = u16::from_le_bytes(msg[6..8].try_into().ok()?);

    Some((seq, index, count, &msg[HEADER_LEN..]))
}

/// Reassembles the fragments of one unreliable message stream, enforcing
/// newest-wins: fragments of a sequence older than the one currently being
/// assembled are dropped, and a newer sequence abandons an incomplete older one.
///
/// A message is yielded exactly once, on the arrival of its final missing
/// fragment. Duplicate fragments — and any fragment of an already-delivered
/// sequence — are ignored, so an at-most-once delivery holds per sequence.
///
/// Both the fragment count and the accumulated payload are bounded
/// ([`MAX_FRAGMENTS`], [`MAX_MESSAGE_LEN`]): the counts come straight off the
/// wire, so a peer must not be able to size our allocations for us.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Default)]
pub struct Reassembler {
    seq: Option<u32>,
    parts: Vec<Option<Vec<u8>>>,
    remaining: usize,
    received_len: usize,
    complete: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl Reassembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drops the in-flight sequence and releases its buffers, marking it done so
    /// its remaining fragments are ignored instead of restarting it.
    fn abandon(&mut self) {
        self.complete = true;
        self.remaining = 0;
        self.received_len = 0;
        self.parts.clear();
        self.parts.shrink_to_fit();
    }

    /// Feeds one fragment. Returns the fully reassembled payload once the last
    /// missing fragment of the newest sequence arrives, `None` otherwise.
    pub fn push(&mut self, seq: u32, index: u16, count: u16, payload: &[u8]) -> Option<Vec<u8>> {
        if count == 0 || count > MAX_FRAGMENTS || index >= count {
            return None;
        }

        let fresh: bool = match self.seq {
            None => true,
            Some(current) if seq == current => false,
            Some(current) => {
                if !seq_greater(seq, current) {
                    return None;
                }
                true
            }
        };

        if fresh {
            self.seq = Some(seq);
            self.complete = false;
            self.remaining = count as usize;
            self.received_len = 0;
            self.parts.clear();
            self.parts.resize_with(count as usize, || None);
        } else if self.complete || self.parts.len() != count as usize {
            return None;
        }

        let slot: &mut Option<Vec<u8>> = &mut self.parts[index as usize];
        if slot.is_some() {
            return None;
        }

        if self.received_len + payload.len() > MAX_MESSAGE_LEN {
            self.abandon();
            return None;
        }

        self.received_len += payload.len();
        *slot = Some(payload.to_vec());
        self.remaining -= 1;

        if self.remaining > 0 {
            return None;
        }

        self.complete = true;

        let total: usize = self
            .parts
            .iter()
            .map(|p: &Option<Vec<u8>>| p.as_ref().map_or(0, Vec::len))
            .sum();
        let mut out: Vec<u8> = Vec::with_capacity(total);
        for part in self.parts.iter_mut() {
            if let Some(bytes) = part.take() {
                out.extend_from_slice(&bytes);
            }
        }

        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_greater_basic_ordering() {
        assert!(seq_greater(2, 1));
        assert!(!seq_greater(1, 2));
        assert!(!seq_greater(5, 5));
    }

    #[test]
    fn seq_greater_handles_wraparound() {
        assert!(seq_greater(1, u32::MAX));
        assert!(!seq_greater(u32::MAX, 1));
    }

    #[test]
    fn accept_seq_drops_stale_and_duplicate() {
        let mut last: Option<u32> = None;
        assert!(accept_seq(&mut last, 10));
        assert!(accept_seq(&mut last, 11));
        assert!(!accept_seq(&mut last, 11));
        assert!(!accept_seq(&mut last, 9));
        assert!(accept_seq(&mut last, 12));
        assert_eq!(last, Some(12));
    }

    #[test]
    fn frame_and_split_round_trip() {
        let mut buf: Vec<u8> = Vec::new();
        frame(&mut buf, 0xDEAD_BEEF, 2, 5, b"hello");
        let (seq, index, count, body): (u32, u16, u16, &[u8]) = split(&buf).unwrap();
        assert_eq!(seq, 0xDEAD_BEEF);
        assert_eq!(index, 2);
        assert_eq!(count, 5);
        assert_eq!(body, b"hello");
    }

    #[test]
    fn split_rejects_short_message() {
        assert!(split(&[1, 2, 3]).is_none());
    }

    #[test]
    fn reassembler_passes_through_single_fragment() {
        let mut r: Reassembler = Reassembler::new();
        assert_eq!(r.push(1, 0, 1, b"solo").unwrap(), b"solo".to_vec());
    }

    #[test]
    fn reassembler_joins_fragments_in_any_arrival_order() {
        let mut r: Reassembler = Reassembler::new();
        assert!(r.push(7, 2, 3, b"c").is_none());
        assert!(r.push(7, 0, 3, b"a").is_none());
        assert_eq!(r.push(7, 1, 3, b"b").unwrap(), b"abc".to_vec());
    }

    #[test]
    fn reassembler_drops_stale_sequence() {
        let mut r: Reassembler = Reassembler::new();
        assert_eq!(r.push(5, 0, 1, b"new").unwrap(), b"new".to_vec());
        assert!(r.push(4, 0, 1, b"old").is_none());
    }

    #[test]
    fn reassembler_abandons_incomplete_older_sequence() {
        let mut r: Reassembler = Reassembler::new();
        assert!(r.push(1, 0, 2, b"partial").is_none());
        assert_eq!(r.push(2, 0, 1, b"newer").unwrap(), b"newer".to_vec());
        assert!(r.push(1, 1, 2, b"late").is_none());
    }

    #[test]
    fn reassembler_ignores_duplicate_fragments_and_redelivery() {
        let mut r: Reassembler = Reassembler::new();
        assert!(r.push(3, 0, 2, b"a").is_none());
        assert!(r.push(3, 0, 2, b"a").is_none());
        assert_eq!(r.push(3, 1, 2, b"b").unwrap(), b"ab".to_vec());
        assert!(r.push(3, 1, 2, b"b").is_none());
    }

    #[test]
    fn reassembler_rejects_malformed_counts() {
        let mut r: Reassembler = Reassembler::new();
        assert!(r.push(1, 0, 0, b"x").is_none());
        assert!(r.push(1, 3, 3, b"x").is_none());
    }

    #[test]
    fn reassembler_refuses_a_peer_dictated_fragment_count() {
        let mut r: Reassembler = Reassembler::new();
        assert!(
            r.push(1, 0, MAX_FRAGMENTS + 1, b"x").is_none(),
            "a peer must not size our slot table past MAX_FRAGMENTS"
        );
        assert!(r.parts.is_empty(), "the oversized count must allocate nothing");
    }

    #[test]
    fn reassembler_abandons_a_message_past_the_size_limit() {
        let mut r: Reassembler = Reassembler::new();
        let chunk: Vec<u8> = vec![0u8; 4096];
        let count: u16 = MAX_FRAGMENTS;

        let mut delivered: Option<Vec<u8>> = None;
        for index in 0..count {
            if let Some(msg) = r.push(1, index, count, &chunk) {
                delivered = Some(msg);
            }
        }

        assert!(delivered.is_none(), "a message past MAX_MESSAGE_LEN must never assemble");
        assert!(r.parts.is_empty(), "the abandoned sequence must release its buffers");
        assert_eq!(r.push(2, 0, 1, b"ok").unwrap(), b"ok".to_vec());
    }

    #[test]
    fn reassembler_survives_sequence_wraparound() {
        let mut r: Reassembler = Reassembler::new();
        assert_eq!(r.push(u32::MAX, 0, 1, b"last").unwrap(), b"last".to_vec());
        assert_eq!(r.push(0, 0, 1, b"wrapped").unwrap(), b"wrapped".to_vec());
    }
}
