use redixel_core::ClientId;

/// Parses the server's welcome frame — `[u64 ClientId LE][f64 tickrate LE]`,
/// sent immediately after a client's hello — into the assigned id and the
/// server's authoritative tickrate. Pure and platform-agnostic so every
/// backend (native, wasm) shares one parser for the wire format. `None` if
/// `bytes` is too short to contain both fields, or the tickrate isn't finite
/// and positive (a malformed or hostile server must not be able to wedge the
/// fixed-update loop with a non-positive or NaN/infinite step).
pub(crate) fn parse_welcome(bytes: &[u8]) -> Option<(ClientId, f64)> {
    let id: ClientId = bytes
        .get(0..8)
        .and_then(|b: &[u8]| b.try_into().ok())
        .map(u64::from_le_bytes)?;

    let tickrate: f64 = bytes
        .get(8..16)
        .and_then(|b: &[u8]| b.try_into().ok())
        .map(f64::from_le_bytes)?;
    (tickrate.is_finite() && tickrate > 0.0).then_some((id, tickrate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_welcome_round_trips() {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&42u64.to_le_bytes());
        bytes.extend_from_slice(&60.0f64.to_le_bytes());
        assert_eq!(parse_welcome(&bytes), Some((42, 60.0)));
    }

    #[test]
    fn parse_welcome_rejects_short_frame() {
        assert_eq!(parse_welcome(&[1, 2, 3]), None);
        assert_eq!(parse_welcome(&[]), None);
    }

    #[test]
    fn parse_welcome_rejects_non_positive_tickrate() {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&1u64.to_le_bytes());
        bytes.extend_from_slice(&0.0f64.to_le_bytes());
        assert_eq!(parse_welcome(&bytes), None);
    }
}
