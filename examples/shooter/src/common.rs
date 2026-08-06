use redixel::prelude::Vec2;

pub const ENTITY_SIZE: f32 = 24.0;
pub const PLAYER_SPEED: f32 = 400.0;
pub const BOT_SPEED: f32 = 220.0;
pub const BULLET_SIZE: f32 = 8.0;
pub const BULLET_SPEED: f32 = 700.0;
pub const BASE_COOLDOWN: f32 = 0.5;
pub const RAPID_FIRE_COOLDOWN: f32 = 0.15;
pub const POWERUP_DURATION: f32 = 5.0;
pub const POWERUP_SIZE: f32 = 16.0;

/// The weapon an agent currently wields; selects the fire pattern and color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponType {
    Pistol,
    Shotgun,
    Flamethrower,
    Homing,
}

/// Axis-aligned overlap test between two squares given top-left corners and sizes.
pub fn check_collision(pos1: Vec2, size1: f32, pos2: Vec2, size2: f32) -> bool {
    pos1.x + size1 > pos2.x && pos1.x < pos2.x + size2 && pos1.y + size1 > pos2.y && pos1.y < pos2.y + size2
}

/// Rotates `v` by `angle` radians in screen space.
pub fn rotate_vec(v: Vec2, angle: f32) -> Vec2 {
    let cos_a: f32 = angle.cos();
    let sin_a: f32 = angle.sin();
    Vec2::new(v.x * cos_a - v.y * sin_a, v.x * sin_a + v.y * cos_a)
}

/// The display color for a weapon's bullets and its powerup pickup.
pub fn get_weapon_color(weapon: WeaponType) -> (u8, u8, u8) {
    match weapon {
        WeaponType::Pistol => (255, 255, 0),
        WeaponType::Shotgun => (255, 210, 0),
        WeaponType::Flamethrower => (255, 143, 0),
        WeaponType::Homing => (122, 255, 255),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_collision_detects_and_rejects() {
        assert!(check_collision(Vec2::new(0.0, 0.0), 10.0, Vec2::new(5.0, 5.0), 10.0));
        assert!(!check_collision(Vec2::new(0.0, 0.0), 10.0, Vec2::new(20.0, 20.0), 5.0));
    }
}
