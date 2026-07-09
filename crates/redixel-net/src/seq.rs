/// Bytes of sequence header prepended to each unreliable payload.
#[cfg(not(target_arch = "wasm32"))]
pub const SEQ_LEN: usize = 4;

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

/// Writes the little-endian sequence header for `seq` into `out` (cleared
/// first), then appends `payload`, reusing `out`'s capacity.
#[cfg(not(target_arch = "wasm32"))]
#[inline]
pub fn frame(out: &mut Vec<u8>, seq: u32, payload: &[u8]) {
    out.clear();
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(payload);
}

/// Splits a received unreliable message into its sequence number and payload, or
/// `None` if it is too short to contain a header.
#[cfg(not(target_arch = "wasm32"))]
#[inline]
pub fn split(msg: &[u8]) -> Option<(u32, &[u8])> {
    if msg.len() < SEQ_LEN {
        return None;
    }

    let (head, body): (&[u8], &[u8]) = msg.split_at(SEQ_LEN);
    let seq: u32 = u32::from_le_bytes(head.try_into().expect("header is SEQ_LEN bytes"));
    Some((seq, body))
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
        frame(&mut buf, 0xDEAD_BEEF, b"hello");
        let (seq, body): (u32, &[u8]) = split(&buf).unwrap();
        assert_eq!(seq, 0xDEAD_BEEF);
        assert_eq!(body, b"hello");
    }

    #[test]
    fn split_rejects_short_message() {
        assert!(split(&[1, 2, 3]).is_none());
    }
}
