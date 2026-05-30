//! Numeric primitives shared across the simulator.
//!
//! Matching the game bit-for-bit requires strict IEEE-754 arithmetic: no fast-math, no fused
//! multiply-add except where the game uses it explicitly, and a vendored fdlibm for
//! transcendentals rather than the platform math library. Position and velocity are `f64`,
//! rotation is `f32`, and the conversions between the two must follow the game exactly.

#![forbid(unsafe_code)]

pub mod mth;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rot {
    pub yaw: f32,
    pub pitch: f32,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
}

impl Rot {
    pub const fn new(yaw: f32, pitch: f32) -> Self {
        Self { yaw, pitch }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_preserve_raw_bits() {
        let v = Vec3::new(0.08, -0.0, f64::from_bits(0x7ff8_0000_0000_0000));
        assert_eq!(v.x.to_bits(), 0.08_f64.to_bits());
        assert_ne!(v.y.to_bits(), 0.0_f64.to_bits());
        assert!(v.z.is_nan());
    }
}
