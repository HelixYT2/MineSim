//! Projectiles: thrown items (snowball, egg, ender pearl) and arrows (arrow, spectral arrow) —
//! flight under gravity and drag, water drag, collision with block shapes along the flight path,
//! arrows sticking in blocks, and hits on the player (damage and knockback).
//!
//! Owned by the projectile port. Projectiles are server-side entities; the arena ticks them after
//! the player each tick.

use ms_numerics::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectileKind {
    Snowball,
    Egg,
    EnderPearl,
    Arrow,
    SpectralArrow,
}

impl ProjectileKind {
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id.trim_start_matches("minecraft:") {
            "snowball" => Self::Snowball,
            "egg" => Self::Egg,
            "ender_pearl" => Self::EnderPearl,
            "arrow" => Self::Arrow,
            "spectral_arrow" => Self::SpectralArrow,
            _ => return None,
        })
    }
}

/// One projectile's state.
#[derive(Clone, Debug, PartialEq)]
pub struct Projectile {
    pub kind: ProjectileKind,
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub in_ground: bool,
    pub removed: bool,
    pub tick_count: i32,
}

impl Projectile {
    pub fn new(kind: ProjectileKind, pos: Vec3, vel: Vec3) -> Self {
        Self {
            kind,
            pos,
            vel,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: false,
            in_ground: false,
            removed: false,
            tick_count: 0,
        }
    }
}
