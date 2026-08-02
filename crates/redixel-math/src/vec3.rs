use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

/// A 3-component float vector. Used for 3D positions and directions.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0, z: 0.0 };
    pub const ONE: Self = Self { x: 1.0, y: 1.0, z: 1.0 };
    pub const X: Self = Self { x: 1.0, y: 0.0, z: 0.0 };
    pub const Y: Self = Self { x: 0.0, y: 1.0, z: 0.0 };
    pub const Z: Self = Self { x: 0.0, y: 0.0, z: 1.0 };

    #[inline]
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    #[inline]
    pub const fn splat(v: f32) -> Self {
        Self { x: v, y: v, z: v }
    }

    #[inline]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    #[inline]
    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    #[inline]
    pub fn length_sq(self) -> f32 {
        self.dot(self)
    }

    #[inline]
    pub fn length(self) -> f32 {
        self.length_sq().sqrt()
    }

    #[inline]
    pub fn normalise(self) -> Self {
        let len: f32 = self.length();
        if len > f32::EPSILON { self / len } else { Self::ZERO }
    }

    #[inline]
    pub fn abs(self) -> Self {
        Self::new(self.x.abs(), self.y.abs(), self.z.abs())
    }

    #[inline]
    pub fn min(self, other: Self) -> Self {
        Self::new(self.x.min(other.x), self.y.min(other.y), self.z.min(other.z))
    }

    #[inline]
    pub fn max(self, other: Self) -> Self {
        Self::new(self.x.max(other.x), self.y.max(other.y), self.z.max(other.z))
    }

    #[inline]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        self + (other - self) * t
    }

    #[inline]
    pub fn to_array(self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }
}

impl Add for Vec3 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl Sub for Vec3 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl Mul<f32> for Vec3 {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

impl Mul<Vec3> for f32 {
    type Output = Vec3;
    fn mul(self, rhs: Vec3) -> Vec3 {
        Vec3::new(self * rhs.x, self * rhs.y, self * rhs.z)
    }
}

impl Div<f32> for Vec3 {
    type Output = Self;
    fn div(self, rhs: f32) -> Self {
        Self::new(self.x / rhs, self.y / rhs, self.z / rhs)
    }
}

impl Neg for Vec3 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
        self.z += rhs.z;
    }
}

impl SubAssign for Vec3 {
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
        self.z -= rhs.z;
    }
}

impl MulAssign<f32> for Vec3 {
    fn mul_assign(&mut self, rhs: f32) {
        self.x *= rhs;
        self.y *= rhs;
        self.z *= rhs;
    }
}

impl DivAssign<f32> for Vec3 {
    fn div_assign(&mut self, rhs: f32) {
        self.x /= rhs;
        self.y /= rhs;
        self.z /= rhs;
    }
}

impl From<(f32, f32, f32)> for Vec3 {
    fn from((x, y, z): (f32, f32, f32)) -> Self {
        Self::new(x, y, z)
    }
}

impl From<[f32; 3]> for Vec3 {
    fn from([x, y, z]: [f32; 3]) -> Self {
        Self::new(x, y, z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-6;

    #[test]
    fn arithmetic() {
        let a: Vec3 = Vec3::new(1.0, 2.0, 3.0);
        let b: Vec3 = Vec3::new(4.0, 5.0, 6.0);
        assert_eq!(a + b, Vec3::new(5.0, 7.0, 9.0));
        assert_eq!(b - a, Vec3::new(3.0, 3.0, 3.0));
        assert_eq!(a * 2.0, Vec3::new(2.0, 4.0, 6.0));
        assert_eq!(b / 2.0, Vec3::new(2.0, 2.5, 3.0));
        assert_eq!(-a, Vec3::new(-1.0, -2.0, -3.0));
    }

    #[test]
    fn length_and_normalise() {
        let v: Vec3 = Vec3::new(2.0, 3.0, 6.0);
        assert!((v.length() - 7.0).abs() < EPS);

        let n: Vec3 = v.normalise();
        assert!((n.length() - 1.0).abs() < EPS);

        assert_eq!(Vec3::ZERO.normalise(), Vec3::ZERO);
    }

    #[test]
    fn dot() {
        assert!((Vec3::X.dot(Vec3::Y)).abs() < EPS);
        assert!((Vec3::X.dot(Vec3::X) - 1.0).abs() < EPS);
    }

    #[test]
    fn cross_follows_the_right_hand_rule() {
        assert_eq!(Vec3::X.cross(Vec3::Y), Vec3::Z);
        assert_eq!(Vec3::Y.cross(Vec3::Z), Vec3::X);
        assert_eq!(Vec3::Z.cross(Vec3::X), Vec3::Y);
    }

    #[test]
    fn cross_is_anticommutative_and_vanishes_on_parallel_vectors() {
        let a: Vec3 = Vec3::new(1.0, 2.0, 3.0);
        let b: Vec3 = Vec3::new(-4.0, 5.0, 6.0);

        assert_eq!(a.cross(b), -b.cross(a));
        assert_eq!(a.cross(a), Vec3::ZERO);
        assert_eq!(a.cross(a * 2.0), Vec3::ZERO);
    }

    #[test]
    fn cross_is_perpendicular_to_both_operands() {
        let a: Vec3 = Vec3::new(1.0, 2.0, 3.0);
        let b: Vec3 = Vec3::new(-4.0, 5.0, 6.0);
        let n: Vec3 = a.cross(b);

        assert!(n.dot(a).abs() < EPS);
        assert!(n.dot(b).abs() < EPS);
    }

    #[test]
    fn splat_abs_min_max() {
        assert_eq!(Vec3::splat(2.0), Vec3::new(2.0, 2.0, 2.0));
        assert_eq!(Vec3::new(-1.0, 2.0, -3.0).abs(), Vec3::new(1.0, 2.0, 3.0));

        let a: Vec3 = Vec3::new(1.0, 5.0, -2.0);
        let b: Vec3 = Vec3::new(4.0, 2.0, 0.0);
        assert_eq!(a.min(b), Vec3::new(1.0, 2.0, -2.0));
        assert_eq!(a.max(b), Vec3::new(4.0, 5.0, 0.0));
    }

    #[test]
    fn lerp() {
        let a: Vec3 = Vec3::ZERO;
        let b: Vec3 = Vec3::new(10.0, 10.0, 10.0);
        assert_eq!(a.lerp(b, 0.5), Vec3::new(5.0, 5.0, 5.0));
        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 1.0), b);
    }
}
