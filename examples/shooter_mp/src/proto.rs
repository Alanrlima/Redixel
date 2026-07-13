use redixel::prelude::{ClientId, NetworkChannel, NetworkManager, Vec2};
use serde::{Deserialize, Serialize};

pub const ARENA_W: f32 = 1280.0;
pub const ARENA_H: f32 = 720.0;

pub const ENTITY_SIZE: f32 = 24.0;
pub const PLAYER_SPEED: f32 = 400.0;
pub const BULLET_SIZE: f32 = 8.0;
pub const BULLET_SPEED: f32 = 700.0;
pub const BASE_COOLDOWN: f32 = 0.5;
pub const RAPID_FIRE_COOLDOWN: f32 = 0.15;
pub const POWERUP_DURATION: f32 = 5.0;
pub const POWERUP_SIZE: f32 = 16.0;

/// A serializable 2D vector for the wire, since `redixel_math::Vec2` is not
/// serde-aware. Convert to and from the engine vector at the boundary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct V2 {
    pub x: f32,
    pub y: f32,
}

impl V2 {
    /// Converts this wire vector into an engine [`Vec2`].
    pub fn vec(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    /// Builds a wire vector from an engine [`Vec2`].
    pub fn of(v: Vec2) -> Self {
        Self { x: v.x, y: v.y }
    }
}

/// The weapon an agent currently wields; selects the fire pattern and color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Weapon {
    Pistol,
    Shotgun,
    Flamethrower,
    Homing,
}

/// A client's sampled intent for one tick, sent to the server on the
/// unreliable-sequenced channel. The server stores the latest and re-applies it
/// every tick, so a dropped packet costs one tick of staleness — cheaper than
/// the head-of-line blocking a reliable stream would impose on every input
/// behind a retransmit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerInput {
    pub move_dir: V2,
    pub aim: V2,
    pub shoot: bool,
    pub dash: bool,
}

/// Authoritative per-agent state broadcast to clients each snapshot. `owner`
/// lets a client resolve the player's unique color locally via [`player_color`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AgentState {
    pub owner: ClientId,
    pub pos: V2,
    pub health: i32,
    pub weapon: Weapon,
    pub rapid_fire: bool,
    pub dashing: bool,
}

/// Authoritative per-bullet state for rendering.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BulletState {
    pub pos: V2,
    pub size: f32,
    pub weapon: Weapon,
}

/// The active powerup pickup; absent from a snapshot when none is available.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PowerupState {
    pub pos: V2,
    pub weapon: Weapon,
}

/// A one-shot cosmetic the server tells clients to play. Dash visuals are driven
/// separately by [`AgentState::dashing`], not by this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectKind {
    Hit,
    Death,
    Powerup,
    PowerupSpawn,
    BulletImpact,
}

/// A positioned, pre-colored one-shot cosmetic baked into a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub kind: EffectKind,
    pub pos: V2,
    pub color: (u8, u8, u8),
}

/// The full authoritative world state for one tick, broadcast to every client on
/// the unreliable-sequenced channel.
///
/// Continuous state only: every field here is superseded by the next tick, so
/// losing one snapshot costs nothing but a frame of staleness. One-shot
/// cosmetics travel separately in an [`EffectBatch`].
///
/// `tick` is the server's fixed tick: clients drop any snapshot not strictly
/// newer than the last one they applied, so a reordered or replayed packet can
/// never snap the world backwards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub tick: u64,
    pub agents: Vec<AgentState>,
    pub bullets: Vec<BulletState>,
    pub powerup: Option<PowerupState>,
}

/// The one-shot cosmetics produced during one tick, broadcast on the **reliable**
/// channel.
///
/// Effects are discrete events, not continuous state: there is no later value
/// that supersedes them, so a dropped effect is simply never played. That is
/// exactly the split `NetworkChannel` prescribes — discrete events go
/// `ReliableOrdered`, superseded state goes `UnreliableSequenced`. Keeping them
/// out of [`Snapshot`] also shrinks it, so it fragments less often.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectBatch {
    pub effects: Vec<Effect>,
}

/// Axis-aligned overlap test between two squares given top-left corners and sizes.
pub fn overlaps(p1: Vec2, s1: f32, p2: Vec2, s2: f32) -> bool {
    p1.x + s1 > p2.x && p1.x < p2.x + s2 && p1.y + s1 > p2.y && p1.y < p2.y + s2
}

/// Rotates `v` by `angle` radians in screen space.
pub fn rotate_vec(v: Vec2, angle: f32) -> Vec2 {
    let cos_a: f32 = angle.cos();
    let sin_a: f32 = angle.sin();
    Vec2::new(v.x * cos_a - v.y * sin_a, v.x * sin_a + v.y * cos_a)
}

/// The display color for a weapon's bullets and its powerup pickup.
pub fn weapon_color(weapon: Weapon) -> (u8, u8, u8) {
    match weapon {
        Weapon::Pistol => (255, 255, 0),
        Weapon::Shotgun => (255, 165, 0),
        Weapon::Flamethrower => (255, 70, 0),
        Weapon::Homing => (50, 255, 255),
    }
}

/// The shoot recoil magnitude for `weapon`, shared by the server (movement
/// impulse) and the client (screen-shake) so rebalancing a weapon can't silently
/// desync the two.
pub fn recoil_for(weapon: Weapon) -> f32 {
    match weapon {
        Weapon::Pistol => 100.0,
        Weapon::Shotgun => 400.0,
        Weapon::Flamethrower => 25.0,
        Weapon::Homing => 150.0,
    }
}

/// Encodes `value` and sends it on `channel` — to `target` if given, or broadcast
/// to everyone otherwise. Logs and drops the message on encode failure (never
/// panics on a malformed payload).
pub fn send_encoded<T: Serialize>(
    net: &mut dyn NetworkManager,
    channel: NetworkChannel,
    target: Option<ClientId>,
    value: &T,
    what: &str,
) {
    match postcard::to_stdvec(value) {
        Ok(bytes) => match target {
            Some(id) => net.send(id, channel, &bytes),
            None => net.broadcast(channel, &bytes),
        },
        Err(e) => log::error!("Failed to encode {what}: {e}"),
    }
}

/// Decodes the SHA-256 certificate digest the native server logs on startup —
/// exactly 64 lowercase hex characters, no separators — into the 32 raw bytes a
/// browser pins via `WebTransportOptions.serverCertificateHashes`.
///
/// `None` for anything that is not 64 hex characters (a truncated paste, a
/// doubled paste, or an unfilled placeholder). Returning `None` rather than
/// panicking keeps a misconfigured build running: the caller falls back to
/// connecting without pinning, which fails cleanly against a self-signed server
/// instead of taking the whole game down at startup.
pub fn parse_cert_hash_hex(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.bytes().all(|b: u8| b.is_ascii_hexdigit()) {
        return None;
    }

    let mut hash: [u8; 32] = [0; 32];
    for (i, byte) in hash.iter_mut().enumerate() {
        let start: usize = i * 2;
        *byte = u8::from_str_radix(&hex[start..start + 2], 16).ok()?;
    }

    Some(hash)
}

/// A stable, vibrant color for a connected player, derived from their
/// [`ClientId`] so every client renders the same player the same color.
pub fn player_color(id: ClientId) -> (u8, u8, u8) {
    const PALETTE: [(u8, u8, u8); 6] = [
        (50, 150, 255),
        (80, 220, 100),
        (255, 105, 180),
        (60, 230, 230),
        (255, 150, 40),
        (180, 110, 255),
    ];

    PALETTE[(id % PALETTE.len() as u64) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_color_wraps_palette_and_distinguishes_neighbors() {
        assert_eq!(player_color(1), player_color(7));
        assert_ne!(player_color(1), player_color(2));
    }

    #[test]
    fn overlaps_detects_and_rejects() {
        assert!(overlaps(Vec2::new(0.0, 0.0), 10.0, Vec2::new(5.0, 5.0), 10.0));
        assert!(!overlaps(Vec2::new(0.0, 0.0), 10.0, Vec2::new(20.0, 20.0), 5.0));
    }

    fn round_trip<T>(value: &T)
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    {
        let bytes: Vec<u8> = postcard::to_stdvec(value).expect("encodes");
        let decoded: T = postcard::from_bytes(&bytes).expect("decodes");
        assert_eq!(&decoded, value);
    }

    #[test]
    fn player_input_round_trips() {
        round_trip(&PlayerInput::default());
        round_trip(&PlayerInput {
            move_dir: V2 { x: -1.0, y: 0.5 },
            aim: V2 { x: 0.0, y: -1.0 },
            shoot: true,
            dash: true,
        });
    }

    #[test]
    fn snapshot_round_trips_empty_and_populated() {
        round_trip(&Snapshot {
            tick: 0,
            agents: Vec::new(),
            bullets: Vec::new(),
            powerup: None,
        });

        round_trip(&Snapshot {
            tick: u64::MAX,
            agents: vec![AgentState {
                owner: 7,
                pos: V2 { x: 100.0, y: 200.0 },
                health: -3,
                weapon: Weapon::Homing,
                rapid_fire: true,
                dashing: false,
            }],
            bullets: vec![BulletState {
                pos: V2 { x: 1.5, y: 2.5 },
                size: BULLET_SIZE,
                weapon: Weapon::Shotgun,
            }],
            powerup: Some(PowerupState {
                pos: V2 { x: 640.0, y: 360.0 },
                weapon: Weapon::Flamethrower,
            }),
        });
    }

    #[test]
    fn effect_batch_round_trips() {
        round_trip(&EffectBatch { effects: Vec::new() });
        round_trip(&EffectBatch {
            effects: vec![
                Effect {
                    kind: EffectKind::Death,
                    pos: V2 { x: 10.0, y: 20.0 },
                    color: (255, 0, 128),
                },
                Effect {
                    kind: EffectKind::BulletImpact,
                    pos: V2 { x: -5.0, y: 0.0 },
                    color: (0, 0, 0),
                },
            ],
        });
    }

    #[test]
    fn parse_cert_hash_hex_decodes_a_full_digest() {
        let hex: String = "ab".repeat(32);
        assert_eq!(parse_cert_hash_hex(&hex), Some([0xAB; 32]));

        let mixed: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let hash: [u8; 32] = parse_cert_hash_hex(mixed).expect("64 hex chars decode");
        assert_eq!(hash[0], 0x01, "the first hex pair must land in the first byte");
        assert_eq!(hash[31], 0xEF, "the last hex pair must land in the last byte");
    }

    #[test]
    fn parse_cert_hash_hex_rejects_anything_but_64_hex_chars() {
        assert_eq!(parse_cert_hash_hex(""), None, "an empty hash is not a hash");
        assert_eq!(parse_cert_hash_hex(&"ab".repeat(31)), None, "62 chars: truncated paste");
        assert_eq!(parse_cert_hash_hex(&"a".repeat(63)), None, "63 chars: one short");
        assert_eq!(parse_cert_hash_hex(&"a".repeat(65)), None, "65 chars: one long");
        assert_eq!(parse_cert_hash_hex(&"ab".repeat(64)), None, "128 chars: doubled paste");
        assert_eq!(parse_cert_hash_hex(&"g".repeat(64)), None, "'g' is not a hex digit");
        assert_eq!(
            parse_cert_hash_hex("GENERATED_CERT_HASH"),
            None,
            "the unfilled placeholder must not take the game down"
        );
    }
}
