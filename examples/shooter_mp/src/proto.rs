use redixel::prelude::{ClientId, Vec2};
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
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct PlayerInput {
    pub move_dir: V2,
    pub aim: V2,
    pub shoot: bool,
    pub dash: bool,
}

/// Authoritative per-agent state broadcast to clients each snapshot. `owner`
/// lets a client resolve the player's unique color locally via [`player_color`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AgentState {
    pub owner: ClientId,
    pub pos: V2,
    pub health: i32,
    pub weapon: Weapon,
    pub rapid_fire: bool,
    pub dashing: bool,
}

/// Authoritative per-bullet state for rendering.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BulletState {
    pub pos: V2,
    pub size: f32,
    pub weapon: Weapon,
}

/// The active powerup pickup; absent from a snapshot when none is available.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}
