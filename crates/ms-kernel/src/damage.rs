//! Health and damage: `hurtServer` with the invulnerability window, knockback (computed from the
//! server's copy of the velocity, as vanilla does, then handed to the player), fall damage, and
//! death.
//!
//! Owned by the damage port.

use crate::state::PlayerState;

/// Where damage comes from, as far as the physics cares.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DamageSource {
    /// A hit from a point (a mob attack): knockback away from `(x, z)`.
    Point { x: f64, z: f64 },
    /// Fall damage (no knockback).
    Fall,
    /// Damage with no position and no knockback (generic).
    Generic,
}

/// `LivingEntity.hurtServer` for the player. Returns whether the damage landed.
pub fn hurt(p: &mut PlayerState, source: DamageSource, amount: f32) -> bool {
    let _ = (p, source, amount);
    false
}

/// `LivingEntity.knockback` applied the vanilla way: computed against the server's velocity copy
/// and delivered as the player's new velocity.
pub fn knockback(p: &mut PlayerState, strength: f64, dx: f64, dz: f64) {
    let _ = (p, strength, dx, dz);
}

/// `LivingEntity.causeFallDamage` / `calculateFallDamage` for a landing after `fall_distance`
/// blocks with the landed-on block's multiplier.
pub fn cause_fall_damage(p: &mut PlayerState, fall_distance: f64, multiplier: f32) {
    let _ = (p, fall_distance, multiplier);
}

/// The per-tick countdowns of `LivingEntity.baseTick` (hurt time, invulnerability).
pub fn tick_timers(p: &mut PlayerState) {
    let _ = p;
}
