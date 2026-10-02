//! Health and damage: `hurtServer` with the invulnerability window, knockback (computed from the
//! server's copy of the velocity, as vanilla does, then handed to the player), fall damage, and
//! death.
//!
//! Owned by the damage port.
//!
//! # Why there is a "server copy" of the velocity
//!
//! In vanilla the player's movement is simulated by the *client*; the server only checks it. When
//! the server hurts the player it computes the knockback from **its own** `deltaMovement` for that
//! player, which does not follow the client's: the server never sees the client's key presses, so
//! its copy only ever decays (`ServerPlayer.doTick` runs `LivingEntity.aiStep` -> `travel` with a
//! zero input vector from the position the client last reported), and it jumps when the client
//! leaves the ground (`ServerGamePacketListenerImpl.handleMovePlayer` calls `jumpFromGround`). The
//! result of the hit reaches the client as a `ClientboundSetEntityMotionPacket` whose vector is
//! squeezed through `FriendlyByteBuf.writeLpVec3`/`readLpVec3` ([`lp_vec3_quantize`]), and *replaces*
//! the client's velocity. [`PlayerState::server_vel`] is the server's copy; `PlayerState::vel` is
//! the client's.
//!
//! The same packet is sent for every damage that is not `NO_IMPACT` (`markHurt`), including fall
//! damage, so a landing that hurts also overwrites the client's velocity with the server's.
//!
//! # How the arena calls this (per game tick `N`)
//!
//! 1. Apply external changes to the state (teleports, effects, ...). Take
//!    `let start = TickStart::of(&p)` (position, rotation, ground flags the *server* last heard).
//! 2. Run the client tick (`player::tick`). At its start `baseTick` calls [`tick_timers`]; where
//!    the landing check calls `Block.fallOn` it calls [`cause_fall_damage`], which only *records*
//!    the landing (the real client never hurts itself: the server does, when the move packet
//!    arrives, after the client's tick is over).
//! 3. Run [`server_tick`]`(&mut p, &start, &world)`. It is vanilla's server tick for the player:
//!    handle the move packet the client just sent (jump, ground flag, a recorded landing becomes
//!    fall damage via [`hurt`]), then [`sync_motion`] (send the server's velocity to the client if
//!    the player was hurt), then the server's own tick (advances `server_vel`). An arena that also
//!    runs projectiles calls the three steps itself ([`server_move_packet`], [`sync_motion`],
//!    entities, [`server_do_tick`]) so that their hits happen where vanilla's entity phase is.
//! 4. Server-side events (a mob hit: [`hurt`], a bare [`knockback`]) can be applied at any point
//!    between ticks, e.g. after step 3 of tick `N - 1`: that is where the oracle's `srv` actions
//!    ran and what their `before` log shows. They change the server's state at once (`health`,
//!    timers, `server_vel`) and mark the velocity for sending; the client's velocity changes at
//!    the next [`sync_motion`], i.e. in step 3 of the same game tick, after the client already ran
//!    its tick, so the client's first tick after the hit still uses the old velocity, exactly as in
//!    the recordings (the damage event itself, `invulnerable_time = 20`, `hurt_time = 10`, and the
//!    new health reach the client with the same delay; they are written to the state at once
//!    because the single copy below stands for both sides).
//!
//! `health`, `absorption`, `invulnerable_time`, `hurt_time` and `last_hurt` are one copy standing
//! for the server's and the client's, which count down in step; `last_hurt` is the server's field
//! (the client's never changes meaningfully). Call [`reset_server_copy`] whenever the player is
//! placed (spawn, teleport with a velocity reset): it makes the server's copy agree with the
//! client's.
//!
//! Not modelled: armour and shields, difficulty scaling of mob damage, fire/drowning/in-wall/
//! freezing and other environmental damage sources (the caller passes whatever it wants through
//! [`hurt`]), death beyond `health == 0`, and the server-only food exhaustion that damage adds.

use crate::attributes::Attribute;
use crate::state::PlayerState;
use ms_numerics::{mth, Vec3};
use ms_rng::JavaRandom;
use ms_world::aabb::Aabb;
use ms_world::World;

// ---------------------------------------------------------------------------------------------
// Java numeric helpers (only the semantics the ported methods rely on)
// ---------------------------------------------------------------------------------------------

/// `Mth.floor(double)`.
fn mth_floor(d: f64) -> i32 {
    let i = d as i32;
    if d < f64::from(i) {
        i.wrapping_sub(1)
    } else {
        i
    }
}

/// `Math.min(double, double)` (NaN propagates, `-0.0 < 0.0`).
fn java_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() {
            a
        } else {
            b
        }
    } else if a < b {
        a
    } else {
        b
    }
}

/// `Math.max(double, double)`.
fn java_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() {
            b
        } else {
            a
        }
    } else if a > b {
        a
    } else {
        b
    }
}

/// `Math.max(float, float)`.
fn java_max_f32(a: f32, b: f32) -> f32 {
    java_max(f64::from(a), f64::from(b)) as f32
}

/// `Math.min(float, float)`.
fn java_min_f32(a: f32, b: f32) -> f32 {
    java_min(f64::from(a), f64::from(b)) as f32
}

/// `Mth.clamp(float, float, float)`.
fn clamp_f32(f: f32, lo: f32, hi: f32) -> f32 {
    if f < lo {
        lo
    } else {
        java_min_f32(f, hi)
    }
}

/// `Mth.wrapDegrees(float)`.
fn wrap_degrees(degrees: f32) -> f32 {
    let mut g = degrees % 360.0;
    if g >= 180.0 {
        g -= 360.0;
    }
    if g < -180.0 {
        g += 360.0;
    }
    g
}

/// `Mth.equal(double, double)`.
fn mth_equal(a: f64, b: f64) -> bool {
    (b - a).abs() < f64::from(1.0e-5_f32)
}

// ---------------------------------------------------------------------------------------------
// LpVec3: the wire format of velocity packets
// ---------------------------------------------------------------------------------------------

const LP_ABS_MAX: f64 = 1.717_986_918_3E10;
const LP_ABS_MIN: f64 = 3.051_944_088_384_301E-5;
const LP_MAX_QUANTIZED: f64 = 32766.0;

fn lp_sanitize(d: f64) -> f64 {
    if d.is_nan() {
        0.0
    } else {
        // Math.clamp(d, -MAX, MAX) = Math.min(MAX, Math.max(d, -MAX))
        java_min(LP_ABS_MAX, java_max(d, -LP_ABS_MAX))
    }
}

/// `Mth.absMax(double, double)`.
fn abs_max(a: f64, b: f64) -> f64 {
    java_max(a.abs(), b.abs())
}

/// `Mth.ceilLong(double)`.
fn ceil_long(d: f64) -> i64 {
    let l = d as i64;
    if d > l as f64 {
        l + 1
    } else {
        l
    }
}

/// `LpVec3.pack`: `Math.round((d * 0.5 + 0.5) * 32766.0)`. The argument is never negative here
/// (|d| <= 1), where `Math.round` (ties toward +infinity) and `f64::round` (ties away from zero)
/// agree.
fn lp_pack(d: f64) -> i64 {
    ((d * 0.5 + 0.5) * LP_MAX_QUANTIZED).round() as i64
}

/// `LpVec3.unpack`.
fn lp_unpack(l: i64) -> f64 {
    java_min((l & 32767) as f64, LP_MAX_QUANTIZED) * 2.0 / LP_MAX_QUANTIZED - 1.0
}

/// The velocity a client ends up with after the server sends `v` in a
/// `ClientboundSetEntityMotionPacket`: `LpVec3.read(LpVec3.write(v))` without the bytes in between.
///
/// Each component is stored as a 15-bit fraction of a common integer scale `ceil(max |component|)`,
/// so the precision depends on the largest component (a server `dy` of 0.36000000149 arrives as
/// 0.36000001-ish, and the horizontal components are rounded on the same grid).
pub fn lp_vec3_quantize(v: Vec3) -> Vec3 {
    let d = lp_sanitize(v.x);
    let e = lp_sanitize(v.y);
    let f = lp_sanitize(v.z);
    let g = abs_max(d, abs_max(e, f));
    if g < LP_ABS_MIN {
        return Vec3::ZERO;
    }
    let l = ceil_long(g);
    let lf = l as f64;
    Vec3::new(
        lp_unpack(lp_pack(d / lf)) * lf,
        lp_unpack(lp_pack(e / lf)) * lf,
        lp_unpack(lp_pack(f / lf)) * lf,
    )
}

fn varint_write(out: &mut Vec<u8>, value: i32) {
    let mut i = value as u32;
    while i & !127 != 0 {
        out.push((i & 127) as u8 | 128);
        i >>= 7;
    }
    out.push(i as u8);
}

/// `LpVec3.write`: append the packet bytes for `v` to `out`.
pub fn lp_vec3_write(v: Vec3, out: &mut Vec<u8>) {
    let d = lp_sanitize(v.x);
    let e = lp_sanitize(v.y);
    let f = lp_sanitize(v.z);
    let g = abs_max(d, abs_max(e, f));
    if g < LP_ABS_MIN {
        out.push(0);
        return;
    }
    let l = ceil_long(g);
    let continuation = (l & 3) != l;
    let m = if continuation { (l & 3) | 4 } else { l };
    let lf = l as f64;
    let n = lp_pack(d / lf) << 3;
    let o = lp_pack(e / lf) << 18;
    let p = lp_pack(f / lf) << 33;
    let q = m | n | o | p;
    out.push(q as u8);
    out.push((q >> 8) as u8);
    out.extend_from_slice(&((q >> 16) as i32).to_be_bytes());
    if continuation {
        varint_write(out, (l >> 2) as i32);
    }
}

/// `LpVec3.read`: decode a vector from the front of `bytes`; returns it and the bytes consumed,
/// or `None` if `bytes` is too short.
pub fn lp_vec3_read(bytes: &[u8]) -> Option<(Vec3, usize)> {
    let i = i64::from(*bytes.first()?);
    if i == 0 {
        return Some((Vec3::ZERO, 1));
    }
    let j = i64::from(*bytes.get(1)?);
    let l = i64::from(u32::from_be_bytes(bytes.get(2..6)?.try_into().ok()?));
    let m = (l << 16) | (j << 8) | i;
    let mut n = i & 3;
    let mut used = 6;
    if i & 4 == 4 {
        let mut value: u32 = 0;
        let mut shift = 0;
        loop {
            let b = *bytes.get(used)?;
            used += 1;
            value |= u32::from(b & 127).checked_shl(shift).unwrap_or(0);
            if b & 128 == 0 {
                break;
            }
            shift += 7;
            if shift >= 35 {
                return None;
            }
        }
        n |= i64::from(value) << 2;
    }
    let nf = n as f64;
    Some((
        Vec3::new(
            lp_unpack(m >> 3) * nf,
            lp_unpack(m >> 18) * nf,
            lp_unpack(m >> 33) * nf,
        ),
        used,
    ))
}

// ---------------------------------------------------------------------------------------------
// Damage
// ---------------------------------------------------------------------------------------------

/// `0.4F` promoted to double: the strength `LivingEntity.hurtServer` hands to `knockback`.
pub const HIT_KNOCKBACK: f64 = 0.4_f32 as f64;

/// `invulnerableTime` right after a full hit.
const INVULNERABLE_TICKS: i32 = 20;
/// `hurtDuration`.
const HURT_DURATION: i32 = 10;

/// Where damage comes from, as far as the physics cares.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DamageSource {
    /// A hit from a point (a mob attack, `minecraft:mob_attack` with a source position):
    /// knockback away from `(x, z)`, relative to the player's position.
    Point { x: f64, z: f64 },
    /// Fall damage (no knockback).
    Fall,
    /// Damage with no position and no knockback (generic).
    Generic,
    /// A hit whose horizontal knockback direction was already resolved by the attacker and is
    /// handed to `knockback(0.4F, dx, dz)` unchanged (projectiles: `(-vx, -vz)` of the projectile).
    Directed { dx: f64, dz: f64 },
}

/// The server-side part of the player that is not a field of the client's physics state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ServerState {
    /// The server's `onGround()`. It is set from the client's flag when a move packet arrives and
    /// then overwritten by the server's own collision result in its tick, so it can differ from
    /// the client's. Knockback reads it (`onGround() ? min(0.4, vy / 2 + d) : vy`).
    pub on_ground: bool,
    /// A landing the client reported this tick and the server has not yet turned into damage:
    /// `(fall distance, block multiplier)`; see [`cause_fall_damage`].
    pub pending_fall: Option<(f64, f32)>,
    /// `Entity.hurtMarked`: the server's velocity has to be sent to the client; see
    /// [`sync_motion`].
    pub hurt_marked: bool,
}

/// Make the server's copy of the player agree with the client's (after spawning or placing the
/// player): velocity and ground flag.
pub fn reset_server_copy(p: &mut PlayerState) {
    p.server_vel = p.vel;
    p.server.on_ground = p.on_ground;
    p.server.pending_fall = None;
    p.server.hurt_marked = false;
}

/// `getMaxAbsorption()`: the absorption effect adds `4 * (amplifier + 1)` to a base of 0.
fn max_absorption(p: &PlayerState) -> f32 {
    match p.effects.get("minecraft:absorption") {
        Some(e) => (0.0_f64 + 4.0 * (f64::from(e.amplifier) + 1.0)) as f32,
        None => 0.0,
    }
}

fn max_health(p: &PlayerState) -> f32 {
    p.attributes.value(Attribute::MaxHealth) as f32
}

/// `LivingEntity.setHealth`.
fn set_health(p: &mut PlayerState, health: f32) {
    p.health = clamp_f32(health, 0.0, max_health(p));
}

/// `LivingEntity.setAbsorptionAmount`.
fn set_absorption(p: &mut PlayerState, absorption: f32) {
    p.absorption = clamp_f32(absorption, 0.0, max_absorption(p));
}

/// `LivingEntity.getDamageAfterMagicAbsorb` for the effect part (resistance); the enchantment
/// protection of worn equipment is outside the "no armour" scope.
fn damage_after_magic_absorb(p: &PlayerState, mut f: f32) -> f32 {
    if let Some(e) = p.effects.get("minecraft:resistance") {
        let i = (e.amplifier + 1) * 5;
        let j = 25 - i;
        let g = f * j as f32;
        f = java_max_f32(g / 25.0, 0.0);
    }
    if f <= 0.0 {
        0.0
    } else {
        f
    }
}

/// `Player.actuallyHurt` for a player without armour: effect resistance, then absorption, then
/// health. (Armour absorption, which with zero armour and toughness is the identity, and the food
/// exhaustion the server adds are not modelled.)
fn actually_hurt(p: &mut PlayerState, f: f32) {
    let f = damage_after_magic_absorb(p, f);
    let var = java_max_f32(f - p.absorption, 0.0);
    set_absorption(p, p.absorption - (f - var));
    if var != 0.0 {
        set_health(p, p.health - var);
    }
}

/// `LivingEntity.knockback` on the server's copy of the velocity, with `next_double` standing in
/// for the entity's `RandomSource.nextDouble()` (used only when the direction is shorter than
/// `1.0E-5F`, i.e. the attacker is exactly above or on the player; vanilla seeds that source from
/// the clock, so it cannot be reproduced against the game).
///
/// This does not deliver the result to the client; [`knockback`] and [`hurt`] do.
pub fn knockback_server_copy(
    p: &mut PlayerState,
    strength: f64,
    dx: f64,
    dz: f64,
    next_double: &mut dyn FnMut() -> f64,
) {
    let d = strength * (1.0 - p.attributes.value(Attribute::KnockbackResistance));
    if d <= 0.0 {
        return;
    }
    let vel = p.server_vel;
    let (mut e, mut f) = (dx, dz);
    let min_len_sq = f64::from(1.0e-5_f32);
    // The loop condition reads `e * e + f * f < 1.0E-5F`; both draws of each component are made
    // left to right.
    while e * e + f * f < min_len_sq {
        let a = next_double();
        let b = next_double();
        e = (a - b) * 0.01;
        let a = next_double();
        let b = next_double();
        f = (a - b) * 0.01;
    }
    // new Vec3(e, 0.0, f).normalize().scale(d)
    let len = (e * e + 0.0 * 0.0 + f * f).sqrt();
    let (nx, nz) = if len < min_len_sq {
        (0.0, 0.0)
    } else {
        (e / len, f / len)
    };
    let (sx, sz) = (nx * d, nz * d);
    let new_y = if p.server.on_ground {
        java_min(0.4, vel.y / 2.0 + d)
    } else {
        vel.y
    };
    p.server_vel = Vec3::new(vel.x / 2.0 - sx, new_y, vel.z / 2.0 - sz);
}

/// A deterministic stand-in for the entity's unseeded random source (see
/// [`knockback_server_copy`]), created only if the tie-break is reached.
fn fallback_rng(p: &PlayerState) -> impl FnMut() -> f64 {
    let seed = i64::from(p.tick_count) ^ 0x5_DEEC_E66D;
    let mut rng: Option<JavaRandom> = None;
    move || {
        rng.get_or_insert_with(|| JavaRandom::new(seed))
            .next_double()
    }
}

/// `ServerEntity.sendChanges` for a `hurtMarked` player: the client's velocity becomes the server's
/// copy as it travels through the LpVec3 codec (`p.vel` = [`lp_vec3_quantize`] of `p.server_vel`).
/// Vanilla runs it at the start of every server tick's level phase, after the move packets of that
/// tick were handled and before the entities (projectiles, ...) and the player's own tick run;
/// [`server_tick`] does exactly that. Damage dealt later in a tick (by an entity) is therefore
/// sent one tick later, with the velocity after the player's tick.
pub fn sync_motion(p: &mut PlayerState) {
    if p.server.hurt_marked {
        p.server.hurt_marked = false;
        p.vel = lp_vec3_quantize(p.server_vel);
    }
}

/// `LivingEntity.knockback` the way the oracle's `knock` action applies it: computed against the
/// server's velocity copy and followed by `hurtMarked = true`, so that the next [`sync_motion`]
/// (the start of the next [`server_tick`]'s level phase) sends it to the client: `p.vel` then is
/// the quantized result and `p.server_vel` is the exact one.
pub fn knockback(p: &mut PlayerState, strength: f64, dx: f64, dz: f64) {
    let mut rng = fallback_rng(p);
    knockback_server_copy(p, strength, dx, dz, &mut rng);
    p.server.hurt_marked = true;
}

/// `LivingEntity.hurtServer` (through `ServerPlayer.hurtServer` and `Player.hurtServer`) for a
/// survival player without armour or a shield. Returns whether the damage landed.
///
/// On a full hit (the player was not inside the invulnerability window) it sets `invulnerable_time`
/// to 20, `hurt_time` to 10, `last_hurt`, applies the damage through resistance, absorption and
/// health, applies knockback for sources that have it (`Point`, `Directed`) and marks the server's
/// velocity to be sent to the client ([`sync_motion`]). Inside the window (`invulnerable_time > 10`) only the excess over
/// `last_hurt` is applied, with no knockback or velocity sync. Damage scaling by difficulty only
/// concerns mobs as the cause and is not modelled (the arena's world is assumed to be at a
/// difficulty where it is the identity).
pub fn hurt(p: &mut PlayerState, source: DamageSource, amount: f32) -> bool {
    let mut rng = fallback_rng(p);
    hurt_with_rng(p, source, amount, &mut rng)
}

/// [`hurt`] with an explicit random source for the knockback tie-break.
pub fn hurt_with_rng(
    p: &mut PlayerState,
    source: DamageSource,
    amount: f32,
    next_double: &mut dyn FnMut() -> f64,
) -> bool {
    // Player.hurtServer: dead players are not hurt; a zero amount is rejected before the
    // LivingEntity logic sees it.
    if !p.is_alive() {
        return false;
    }
    if amount == 0.0 {
        return false;
    }
    let mut f = amount;
    // LivingEntity.hurtServer
    if f < 0.0 {
        f = 0.0;
    }
    if f.is_nan() || f.is_infinite() {
        f = f32::MAX;
    }
    let full_hit;
    if p.invulnerable_time as f32 > 10.0 {
        if f <= p.last_hurt {
            return false;
        }
        actually_hurt(p, f - p.last_hurt);
        p.last_hurt = f;
        full_hit = false;
    } else {
        p.last_hurt = f;
        p.invulnerable_time = INVULNERABLE_TICKS;
        actually_hurt(p, f);
        p.hurt_time = HURT_DURATION;
        full_hit = true;
    }
    if full_hit {
        // broadcastDamageEvent / markHurt / knockback. The damage-event packet is what sets the
        // client's invulnerable_time and hurt_time to the values above.
        let direction = match source {
            DamageSource::Point { x, z } => Some((x - p.pos.x, z - p.pos.z)),
            DamageSource::Directed { dx, dz } => Some((dx, dz)),
            DamageSource::Fall | DamageSource::Generic => None,
        };
        if let Some((dx, dz)) = direction {
            knockback_server_copy(p, HIT_KNOCKBACK, dx, dz, next_double);
        }
        p.server.hurt_marked = true;
    }
    // Death: health <= 0 is `isDeadOrDying`; nothing else of the death sequence acts on the
    // physics state.
    true
}

/// `LivingEntity.calculateFallDamage`: `Mth.floor((d + 1.0E-6 - safe_fall_distance) * f *
/// fall_damage_multiplier)`.
pub fn fall_damage(p: &PlayerState, fall_distance: f64, multiplier: f32) -> i32 {
    let power = fall_distance + 1.0E-6 - p.attributes.value(Attribute::SafeFallDistance);
    mth_floor(power * f64::from(multiplier) * p.attributes.value(Attribute::FallDamageMultiplier))
}

/// `Player.causeFallDamage` -> `LivingEntity.causeFallDamage` carried out at once: damage the
/// player with a `fall` source if the landing after `fall_distance` blocks on a block with
/// `multiplier` (`Block.fallOn`: 1.0 for most blocks, 0.0 slime, 0.5 beds, 0.2 hay and honey)
/// costs at least one point. Returns whether it hurt.
pub fn apply_fall_damage(p: &mut PlayerState, fall_distance: f64, multiplier: f32) -> bool {
    if fall_distance.is_nan() || fall_distance <= 0.0 {
        return false;
    }
    let i = fall_damage(p, fall_distance, multiplier);
    if i > 0 {
        hurt(p, DamageSource::Fall, i as f32)
    } else {
        false
    }
}

/// `LivingEntity.causeFallDamage` as the client's landing check reaches it (`Entity.move` ->
/// `checkFallDamage` -> `Block.fallOn`): the client does not hurt itself, so this only records the
/// landing; the server applies it in [`server_tick`] when the client's move packet for this tick
/// arrives (after the client's tick, so the client's end-of-tick state is untouched, as in the
/// recordings).
pub fn cause_fall_damage(p: &mut PlayerState, fall_distance: f64, multiplier: f32) {
    p.server.pending_fall = Some((fall_distance, multiplier));
}

/// The per-tick countdowns of `LivingEntity.baseTick` (hurt time, invulnerability), plus the death
/// counter. For a server-side `ServerPlayer` the invulnerability countdown is in `ServerPlayer.tick`
/// instead; both run once per tick, and this state stands for both.
pub fn tick_timers(p: &mut PlayerState) {
    if p.hurt_time > 0 {
        p.hurt_time -= 1;
    }
    if p.invulnerable_time > 0 {
        p.invulnerable_time -= 1;
    }
    if !p.is_alive() {
        p.death_time += 1;
    }
}

// ---------------------------------------------------------------------------------------------
// The server's copy of the player
// ---------------------------------------------------------------------------------------------

/// What the server last knew about the client at the start of this game tick: the previous move
/// packet's contents. Capture it with [`TickStart::of`] after external changes and before running
/// the client tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TickStart {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub horizontal_collision: bool,
}

impl TickStart {
    pub fn of(p: &PlayerState) -> Self {
        Self {
            pos: p.pos,
            yaw: p.yaw,
            pitch: p.pitch,
            on_ground: p.on_ground,
            horizontal_collision: p.horizontal_collision,
        }
    }
}

/// `Entity.getOnPos(offset)`: the block the player is considered to stand on (friction, bounce).
fn on_pos(p: &PlayerState, pos: Vec3, offset: f32) -> (i32, i32, i32) {
    let y = mth_floor(pos.y - f64::from(offset));
    match p.supporting_block {
        Some((sx, _, sz)) => (sx, y, sz),
        None => (mth_floor(pos.x), y, mth_floor(pos.z)),
    }
}

fn player_box(p: &PlayerState, pos: Vec3) -> Aabb {
    let (w, h) = p.dimensions();
    let half = f64::from(w / 2.0);
    Aabb::new(
        Vec3::new(pos.x - half, pos.y, pos.z - half),
        Vec3::new(pos.x + half, pos.y + f64::from(h), pos.z + half),
    )
}

fn gather_colliders(p: &PlayerState, world: &World, bb: Aabb, region: Aabb) -> Vec<Aabb> {
    let (x0, x1) = (mth_floor(region.min.x), mth_floor(region.max.x));
    let (y0, y1) = (mth_floor(region.min.y), mth_floor(region.max.y));
    let (z0, z1) = (mth_floor(region.min.z), mth_floor(region.max.z));
    let mut out = Vec::new();
    for bx in x0..=x1 {
        for by in y0..=y1 {
            for bz in z0..=z1 {
                for s in crate::blocks::collision_boxes(p, bb, world, bx, by, bz) {
                    out.push(Aabb::new(
                        Vec3::new(
                            f64::from(bx) + s[0],
                            f64::from(by) + s[1],
                            f64::from(bz) + s[2],
                        ),
                        Vec3::new(
                            f64::from(bx) + s[3],
                            f64::from(by) + s[4],
                            f64::from(bz) + s[5],
                        ),
                    ));
                }
            }
        }
    }
    out
}

/// `Entity.getBlockSpeedFactor` with the `LivingEntity` override (movement efficiency), at `pos`.
fn block_speed_factor(p: &PlayerState, world: &World, pos: Vec3) -> f32 {
    let (x, y, z) = (mth_floor(pos.x), mth_floor(pos.y), mth_floor(pos.z));
    let here = ms_data::block_speed_factor(world.block(x, y, z));
    let name = world.block_name(x, y, z);
    let base = if name != "minecraft:water" && name != "minecraft:bubble_column" {
        if here == 1.0 {
            let (bx, by, bz) = on_pos(p, pos, 0.500_001);
            ms_data::block_speed_factor(world.block(bx, by, bz))
        } else {
            here
        }
    } else {
        here
    };
    let efficiency = p.attributes.value(Attribute::MovementEfficiency) as f32;
    // Mth.lerp(efficiency, base, 1.0F)
    base + efficiency * (1.0 - base)
}

/// `Entity.getBlockJumpFactor` at `pos`.
fn block_jump_factor(p: &PlayerState, world: &World, pos: Vec3) -> f32 {
    let (x, y, z) = (mth_floor(pos.x), mth_floor(pos.y), mth_floor(pos.z));
    let here = ms_data::block_jump_factor(world.block(x, y, z));
    let (bx, by, bz) = on_pos(p, pos, 0.500_001);
    let below = ms_data::block_jump_factor(world.block(bx, by, bz));
    if here == 1.0 {
        below
    } else {
        here
    }
}

/// `LivingEntity.jumpFromGround` on the server's copy, which is still at the previous packet's
/// position `pos` and has that packet's yaw.
fn server_jump(p: &mut PlayerState, world: &World, pos: Vec3, server_yaw: f32) {
    let jump_strength = p.attributes.value(Attribute::JumpStrength) as f32;
    let f = jump_strength * 1.0 * block_jump_factor(p, world, pos)
        + crate::effects::jump_boost_power(p);
    if f <= 1.0E-5 {
        return;
    }
    let v = p.server_vel;
    p.server_vel = Vec3::new(v.x, java_max(f64::from(f), v.y), v.z);
    if p.sprinting {
        let g = server_yaw * (std::f32::consts::PI / 180.0);
        let v = p.server_vel;
        p.server_vel = Vec3::new(
            v.x + f64::from(-mth::sin(g)) * 0.2,
            v.y + 0.0,
            v.z + f64::from(mth::cos(g)) * 0.2,
        );
    }
}

/// `ServerGamePacketListenerImpl.handleMovePlayer` as far as the server's copy of the player is
/// concerned.
fn handle_move_packet(p: &mut PlayerState, start: &TickStart, world: &World) {
    // What LocalPlayer.sendPosition would have sent this tick (the 20-tick position reminder is not
    // tracked).
    let d = (
        p.pos.x - start.pos.x,
        p.pos.y - start.pos.y,
        p.pos.z - start.pos.z,
    );
    let sends_position = d.0 * d.0 + d.1 * d.1 + d.2 * d.2 > 2.0E-4 * 2.0E-4;
    let sends_rotation = p.yaw != start.yaw || p.pitch != start.pitch;
    let sends_status =
        start.on_ground != p.on_ground || start.horizontal_collision != p.horizontal_collision;
    if sends_position || sends_rotation || sends_status {
        // The packet's y relative to the server's last good y.
        let m = if sends_position { d.1 } else { 0.0 };
        if p.server.on_ground && !p.on_ground && m > 0.0 {
            // The server's rotation is still the previous packet's (wrapped like `absSnapTo` did).
            server_jump(p, world, start.pos, wrap_degrees(start.yaw));
        }
        // Entity.move(PLAYER, delta) followed by setOnGroundWithMovement(packet.onGround)
        p.server.on_ground = p.on_ground;
    }
    // A landing always travels in a move packet (the ground flag flips or the player moves);
    // `doCheckFallDamage` then runs on the server's own fall distance, which the recorded landing
    // stands for.
    if let Some((fall, multiplier)) = p.server.pending_fall.take() {
        apply_fall_damage(p, fall, multiplier);
    }
}

/// `Entity.move(MoverType.SELF, deltaMovement)` of the server's copy at `pos`; updates the
/// server's ground flag and velocity (collision zeroing, `updateEntityMovementAfterFallOn`, block
/// speed factor). The position the move reaches is discarded by the caller, as
/// `ServerGamePacketListenerImpl.tickPlayer` restores the position after `doTick`.
fn server_move_self(p: &mut PlayerState, world: &World, pos: Vec3) {
    let motion = p.server_vel;
    let bb = player_box(p, pos);
    let step = p.attributes.value(Attribute::StepHeight) as f32;
    let region = bb
        .expand_towards(motion)
        .expand_towards(Vec3::new(0.0, f64::from(step), 0.0));
    let colliders = gather_colliders(p, world, bb, region);
    let moved = crate::collision::collide(motion, bb, p.server.on_ground, step, &colliders);

    let moved_len_sq = moved.x * moved.x + moved.y * moved.y + moved.z * moved.z;
    let motion_len_sq = motion.x * motion.x + motion.y * motion.y + motion.z * motion.z;
    let new_pos = if moved_len_sq > 1.0E-7 || motion_len_sq - moved_len_sq < 1.0E-7 {
        Vec3::new(pos.x + moved.x, pos.y + moved.y, pos.z + moved.z)
    } else {
        pos
    };

    let hit_x = !mth_equal(motion.x, moved.x);
    let hit_z = !mth_equal(motion.z, moved.z);
    let horizontal_collision = hit_x || hit_z;
    let vertical_collision = motion.y != moved.y;
    // On the server `isLocalInstanceAuthoritative()` is false: the ground flag is only refreshed
    // by a move with a vertical component.
    if motion.y.abs() > 0.0 {
        p.server.on_ground = vertical_collision && motion.y < 0.0;
    }
    let mut vel = motion;
    if horizontal_collision {
        if hit_x {
            vel.x = 0.0;
        }
        if hit_z {
            vel.z = 0.0;
        }
    }
    p.server_vel = vel;
    if vertical_collision {
        // Block.updateEntityMovementAfterFallOn for the block under the new position. The blocks
        // module works on `vel`, so lend it the server's copy.
        let landed_on = on_pos(p, new_pos, 0.2);
        let client_vel = p.vel;
        p.vel = p.server_vel;
        crate::blocks::after_fall_on(p, world, landed_on);
        p.server_vel = p.vel;
        p.vel = client_vel;
    }
    let f = block_speed_factor(p, world, new_pos);
    let v = p.server_vel;
    p.server_vel = Vec3::new(v.x * f64::from(f), v.y * 1.0, v.z * f64::from(f));
}

/// `LivingEntity.getEffectiveGravity` for the server's copy.
fn effective_gravity(p: &PlayerState) -> f64 {
    let gravity = p.attributes.value(Attribute::Gravity);
    if p.server_vel.y <= 0.0 && p.effects.has("minecraft:slow_falling") {
        java_min(gravity, 0.01)
    } else {
        gravity
    }
}

/// `ServerPlayer.doTick` as it acts on the server's copy of the velocity: `LivingEntity.aiStep`
/// (velocity thresholds, then `travel` with a zero input vector).
fn server_ai_step(p: &mut PlayerState, world: &World) {
    // Velocities below the thresholds are snapped to zero.
    let v = p.server_vel;
    let (mut x, mut y, mut z) = (v.x, v.y, v.z);
    if v.x * v.x + v.z * v.z < 9.0E-6 {
        x = 0.0;
        z = 0.0;
    }
    if v.y.abs() < 0.003 {
        y = 0.0;
    }
    p.server_vel = Vec3::new(x, y, z);

    let pos = p.pos;
    if crate::fluids::should_travel_in_fluid(p, world) {
        // Lend the fluid port a copy of the player carrying the server's velocity and ground flag.
        let mut scratch = p.clone();
        scratch.vel = p.server_vel;
        scratch.on_ground = p.server.on_ground;
        crate::fluids::travel_in_fluid(&mut scratch, world, Vec3::ZERO);
        p.server_vel = scratch.vel;
        p.server.on_ground = scratch.on_ground;
        return;
    }

    // LivingEntity.travelInAir with a zero input vector.
    let friction: f32 = if p.server.on_ground {
        let (bx, by, bz) = on_pos(p, pos, 0.500_001);
        ms_data::block_friction(world.block(bx, by, bz))
    } else {
        1.0
    };
    let g = friction * 0.91;
    // moveRelative with a zero input vector still does `deltaMovement.add(Vec3.ZERO)`, which turns
    // a negative zero into a positive one.
    let v = p.server_vel;
    p.server_vel = Vec3::new(v.x + 0.0, v.y + 0.0, v.z + 0.0);
    server_move_self(p, world, pos);
    let v = p.server_vel;
    let mut d = v.y;
    if let Some(levitation) = p.effects.get("minecraft:levitation") {
        d += (0.05 * (f64::from(levitation.amplifier) + 1.0) - v.y) * 0.2;
    } else {
        d -= effective_gravity(p);
    }
    p.server_vel = Vec3::new(
        v.x * f64::from(g),
        d * f64::from(0.98_f32),
        v.z * f64::from(g),
    );
}

/// One server tick for the player, run after the client's tick of the same game tick (see the
/// module docs for where it sits in the arena's loop).
///
/// 1. The move packet the client sent at the end of its tick is handled (`handleMovePlayer`): if
///    the client left the ground upward while the server thought it was on the ground the server
///    jumps (`jumpFromGround`, including the sprint boost), the server's ground flag is set from
///    the client's, and a landing recorded by [`cause_fall_damage`] becomes fall damage ([`hurt`]).
/// 2. [`sync_motion`]: if the player was hurt (by an action since the last tick, or by that fall
///    damage) the server's velocity is sent to the client, replacing `p.vel`.
/// 3. The server's own tick for the player (`ServerPlayer.doTick` -> `aiStep` -> `travel`): its
///    velocity decays and falls under gravity from the position the client reported, colliding
///    with the world (so the server "lands" before the client does: its copy of the velocity leads
///    the client's by up to a tick).
///
/// An arena with entities that hurt the player (projectiles) runs them between 2 and 3, i.e. calls
/// [`server_move_packet`], [`sync_motion`], the entities, then [`server_do_tick`].
///
/// Modelled: travel in air and on ground (block friction, gravity attribute, levitation, slow
/// falling, block speed factor, collision zeroing, `updateEntityMovementAfterFallOn` through
/// `blocks::after_fall_on`); fluids through `fluids::travel_in_fluid` on a copy. Not modelled:
/// climbing, cobweb-style stuck multipliers, sneaking's edge back-off, fluid pushing, entity
/// pushes, and server-side damage sources other than the ones passed to [`hurt`].
pub fn server_tick(p: &mut PlayerState, start: &TickStart, world: &World) {
    server_move_packet(p, start, world);
    sync_motion(p);
    server_do_tick(p, world);
}

/// Step 1 of [`server_tick`] alone: the server handles the move packet the client sent at the end
/// of its tick (`start` is what the previous packet said). In vanilla packets are processed at the
/// start of a server tick, so two client ticks' packets can land in one server tick when their
/// timing slips; call this once per packet and [`server_do_tick`] once per server tick to model
/// that.
pub fn server_move_packet(p: &mut PlayerState, start: &TickStart, world: &World) {
    handle_move_packet(p, start, world);
}

/// Step 2 of [`server_tick`] alone: `ServerPlayer.doTick`'s effect on the server's copy of the
/// velocity, from the position currently in `p.pos` (the last position a packet reported).
pub fn server_do_tick(p: &mut PlayerState, world: &World) {
    server_ai_step(p, world);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player() -> PlayerState {
        PlayerState::new(Vec3::new(0.5, -63.0, 0.5), 0.0)
    }

    #[test]
    fn lp_vec3_zero_and_tiny_vectors_become_zero() {
        assert_eq!(lp_vec3_quantize(Vec3::ZERO), Vec3::ZERO);
        assert_eq!(
            lp_vec3_quantize(Vec3::new(1.0e-5, -2.0e-5, 3.0e-5)),
            Vec3::ZERO
        );
        let mut out = Vec::new();
        lp_vec3_write(Vec3::new(1.0e-5, 0.0, 0.0), &mut out);
        assert_eq!(out, vec![0]);
    }

    #[test]
    fn lp_vec3_matches_recorded_server_to_client_velocities() {
        // Server value -> the velocity the 1.21.11 client showed after the motion packet
        // (corpus: knockback_standing / knockback_moving / fall_damage_4).
        let cases: &[(f64, f64, f64, f64, f64, f64)] = &[
            // (server dx, dy, dz) -> (client dx, dy, dz)
            (
                0.0,
                0.36080000519752503,
                -0.4000000059604645,
                0.0,
                0.36080083012879194,
                -0.39998779222364644,
            ),
            (0.0, -0.0784000015258789, 0.0, 0.0, -0.0783739241897089, 0.0),
        ];
        for &(sx, sy, sz, cx, cy, cz) in cases {
            let q = lp_vec3_quantize(Vec3::new(sx, sy, sz));
            assert_eq!(
                (q.x.to_bits(), q.y.to_bits(), q.z.to_bits()),
                (cx.to_bits(), cy.to_bits(), cz.to_bits()),
                "{sx} {sy} {sz}"
            );
        }
    }

    #[test]
    fn lp_vec3_write_read_agrees_with_quantize() {
        // A deterministic spread of magnitudes, signs and scales (including the continuation
        // varint for components above 3).
        let mut rng = JavaRandom::new(12345);
        for i in 0..20000 {
            let scale = [
                1.0e-4, 0.01, 0.4, 3.0, 4.0, 77.5, 1.0e5, 1.0e9, 1.7e10, 2.0e10,
            ][i % 10];
            let v = Vec3::new(
                (rng.next_double() * 2.0 - 1.0) * scale,
                (rng.next_double() * 2.0 - 1.0) * scale * if i % 3 == 0 { 0.01 } else { 1.0 },
                (rng.next_double() * 2.0 - 1.0) * scale,
            );
            let mut bytes = Vec::new();
            lp_vec3_write(v, &mut bytes);
            let (back, used) = lp_vec3_read(&bytes).unwrap();
            assert_eq!(used, bytes.len());
            let q = lp_vec3_quantize(v);
            assert_eq!(
                (back.x.to_bits(), back.y.to_bits(), back.z.to_bits()),
                (q.x.to_bits(), q.y.to_bits(), q.z.to_bits()),
                "{v:?}"
            );
        }
    }

    #[test]
    fn lp_vec3_wire_format() {
        // 0.5 on one axis: scale 1, x packs to round((0.5 * 0.5 + 0.5) * 32766) = 24575.
        let mut out = Vec::new();
        lp_vec3_write(Vec3::new(0.5, 0.0, 0.0), &mut out);
        let packed_x: i64 = 24575;
        let packed_zero: i64 = 16383; // round(0.5 * 32766)
        let q: i64 = 1 | (packed_x << 3) | (packed_zero << 18) | (packed_zero << 33);
        let mut want = vec![q as u8, (q >> 8) as u8];
        want.extend_from_slice(&((q >> 16) as i32).to_be_bytes());
        assert_eq!(out, want);
        // 10.0 needs the continuation bit and a varint of 10 >> 2 = 2.
        let mut out = Vec::new();
        lp_vec3_write(Vec3::new(10.0, 0.0, 0.0), &mut out);
        assert_eq!(out.len(), 7);
        assert_eq!(out[0] & 7, (10 & 3) | 4);
        assert_eq!(out[6], 2);
    }

    #[test]
    fn knockback_pushes_away_and_halves_the_old_velocity() {
        let mut p = player();
        p.server.on_ground = true;
        p.server_vel = Vec3::new(0.2, -0.0784, -0.2);
        let mut rng = || 0.5;
        knockback_server_copy(&mut p, 0.4, 0.0, 3.0, &mut rng);
        // direction (0, 3) normalises to (0, 1): z = -0.2 / 2 - 0.4
        assert_eq!(p.server_vel.x, 0.1);
        assert_eq!(p.server_vel.z, -0.2 / 2.0 - 0.4);
        // on the ground: min(0.4, -0.0784 / 2 + 0.4)
        assert_eq!(p.server_vel.y, -0.0784 / 2.0 + 0.4);
        // in the air the vertical velocity is untouched
        p.server.on_ground = false;
        p.server_vel = Vec3::new(0.0, 0.25, 0.0);
        knockback_server_copy(&mut p, 0.4, 1.0, 0.0, &mut rng);
        assert_eq!(p.server_vel.y, 0.25);
        assert_eq!(p.server_vel.x, -0.4);
    }

    #[test]
    fn knockback_resistance_scales_and_can_cancel_it() {
        let mut p = player();
        p.server.on_ground = true;
        p.attributes.set_base(Attribute::KnockbackResistance, 1.0);
        p.server_vel = Vec3::new(0.3, -0.0784, 0.0);
        let before = p.server_vel;
        let mut rng = || 0.5;
        knockback_server_copy(&mut p, 0.4, 1.0, 0.0, &mut rng);
        assert_eq!(p.server_vel, before);
        p.attributes.set_base(Attribute::KnockbackResistance, 0.5);
        knockback_server_copy(&mut p, 0.4, 1.0, 0.0, &mut rng);
        assert_eq!(p.server_vel.x, 0.3 / 2.0 - 1.0 * (0.4 * (1.0 - 0.5)));
    }

    #[test]
    fn knockback_tie_break_draws_both_components_left_to_right() {
        let mut p = player();
        p.server.on_ground = false;
        let draws = std::cell::RefCell::new(vec![0.9, 0.1, 0.3, 0.8].into_iter());
        let mut next = || draws.borrow_mut().next().expect("four draws");
        knockback_server_copy(&mut p, 0.4, 0.0, 0.0, &mut next);
        // e = (0.9 - 0.1) * 0.01, f = (0.3 - 0.8) * 0.01 -> direction (+, -)
        assert!(p.server_vel.x < 0.0 && p.server_vel.z > 0.0);
        assert!(draws.borrow_mut().next().is_none());
    }

    #[test]
    fn hurt_applies_damage_timers_and_knockback() {
        let mut p = player();
        p.server.on_ground = true;
        p.server_vel = Vec3::new(0.0, -0.0784, 0.0);
        let landed = hurt(&mut p, DamageSource::Point { x: 0.5, z: 3.5 }, 2.0);
        assert!(landed);
        assert_eq!(p.health, 18.0);
        assert_eq!(p.invulnerable_time, 20);
        assert_eq!(p.hurt_time, 10);
        assert_eq!(p.last_hurt, 2.0);
        assert!(p.server_vel.z < -0.39);
        // the client hears about the velocity at the next sendChanges
        assert!(p.server.hurt_marked);
        assert_eq!(p.vel, Vec3::ZERO);
        sync_motion(&mut p);
        assert!(!p.server.hurt_marked);
        assert_eq!(p.vel, lp_vec3_quantize(p.server_vel));
    }

    #[test]
    fn hurt_inside_the_window_only_applies_the_excess() {
        let mut p = player();
        p.server.on_ground = true;
        assert!(hurt(&mut p, DamageSource::Point { x: 0.5, z: 3.5 }, 4.0));
        sync_motion(&mut p);
        let (vel, server_vel) = (p.vel, p.server_vel);
        // weaker or equal: rejected, nothing changes
        assert!(!hurt(&mut p, DamageSource::Point { x: 3.5, z: 0.5 }, 4.0));
        assert!(!hurt(&mut p, DamageSource::Point { x: 3.5, z: 0.5 }, 1.0));
        assert_eq!((p.health, p.last_hurt), (16.0, 4.0));
        // stronger: only the difference, no knockback, no velocity sync, window untouched
        p.invulnerable_time = 15;
        p.hurt_time = 5;
        assert!(hurt(&mut p, DamageSource::Point { x: 3.5, z: 0.5 }, 6.0));
        assert_eq!(p.health, 14.0);
        assert_eq!(p.last_hurt, 6.0);
        assert_eq!((p.invulnerable_time, p.hurt_time), (15, 5));
        sync_motion(&mut p);
        assert_eq!((p.vel, p.server_vel), (vel, server_vel));
        // at exactly 10 the window is over
        p.invulnerable_time = 10;
        assert!(hurt(&mut p, DamageSource::Point { x: 3.5, z: 0.5 }, 1.0));
        assert_eq!(
            (p.health, p.invulnerable_time, p.last_hurt),
            (13.0, 20, 1.0)
        );
    }

    #[test]
    fn hurt_absorption_resistance_and_death() {
        let mut p = player();
        p.absorption = 0.0;
        crate::effects::add_effect(&mut p, "minecraft:absorption", 0, 100);
        p.absorption = 4.0;
        assert!(hurt(&mut p, DamageSource::Generic, 3.0));
        assert_eq!((p.absorption, p.health), (1.0, 20.0));
        p.invulnerable_time = 0;
        assert!(hurt(&mut p, DamageSource::Generic, 3.0));
        assert_eq!((p.absorption, p.health), (0.0, 18.0));
        // resistance II (amplifier 1) removes 50%
        p.invulnerable_time = 0;
        crate::effects::add_effect(&mut p, "minecraft:resistance", 1, 100);
        assert!(hurt(&mut p, DamageSource::Generic, 4.0));
        // 4 * (25 - 10) / 25 = 2.4 in float arithmetic
        assert_eq!(p.health, 18.0_f32 - 4.0_f32 * 15.0 / 25.0);
        // lethal damage clamps at zero and nothing hurts a dead player
        p.invulnerable_time = 0;
        assert!(hurt(&mut p, DamageSource::Generic, 1000.0));
        assert_eq!(p.health, 0.0);
        assert!(!p.is_alive());
        p.invulnerable_time = 0;
        assert!(!hurt(&mut p, DamageSource::Generic, 1.0));
    }

    #[test]
    fn hurt_zero_negative_and_non_finite_amounts() {
        let mut p = player();
        assert!(!hurt(&mut p, DamageSource::Generic, 0.0));
        assert_eq!((p.invulnerable_time, p.health), (0, 20.0));
        // negative: clamped to zero, still opens the window and counts as a hit
        assert!(hurt(&mut p, DamageSource::Generic, -3.0));
        assert_eq!(
            (p.invulnerable_time, p.health, p.last_hurt),
            (20, 20.0, 0.0)
        );
        let mut p = player();
        assert!(hurt(&mut p, DamageSource::Generic, f32::NAN));
        assert_eq!(p.last_hurt, f32::MAX);
        assert_eq!(p.health, 0.0);
    }

    #[test]
    fn fall_damage_formula() {
        let p = player();
        assert_eq!(fall_damage(&p, 3.0, 1.0), 0); // 3 + 1e-6 - 3 = 1e-6 -> 0
        assert_eq!(fall_damage(&p, 4.0, 1.0), 1);
        assert_eq!(fall_damage(&p, 5.0, 1.0), 2);
        assert_eq!(fall_damage(&p, 7.0, 1.0), 4);
        assert_eq!(fall_damage(&p, 10.0, 0.2), 1); // 7 * 0.2 = 1.4
        assert_eq!(fall_damage(&p, 10.0, 0.5), 3); // 3.5
        assert_eq!(fall_damage(&p, 50.0, 0.0), 0); // slime
        assert_eq!(fall_damage(&p, 2.0, 1.0), -1);
        let mut p = player();
        p.attributes.set_base(Attribute::SafeFallDistance, 10.0);
        p.attributes.set_base(Attribute::FallDamageMultiplier, 0.5);
        assert_eq!(fall_damage(&p, 16.0, 1.0), 3); // (6 + 1e-6) * 0.5
    }

    #[test]
    fn fall_damage_is_recorded_then_applied_by_the_server() {
        let mut p = player();
        cause_fall_damage(&mut p, 7.0, 1.0);
        assert_eq!(p.health, 20.0);
        assert_eq!(p.server.pending_fall, Some((7.0, 1.0)));
        let world = World::flat(-63);
        let start = TickStart::of(&p);
        server_tick(&mut p, &start, &world);
        assert_eq!(p.health, 16.0);
        assert_eq!(p.server.pending_fall, None);
        // the hurt was sent to the client in the same tick
        assert!(!p.server.hurt_marked);
        assert_eq!((p.invulnerable_time, p.hurt_time), (20, 10));
    }

    #[test]
    fn timers_count_down_and_stop_at_zero() {
        let mut p = player();
        p.invulnerable_time = 2;
        p.hurt_time = 1;
        tick_timers(&mut p);
        assert_eq!((p.invulnerable_time, p.hurt_time), (1, 0));
        tick_timers(&mut p);
        tick_timers(&mut p);
        assert_eq!((p.invulnerable_time, p.hurt_time, p.death_time), (0, 0, 0));
        p.health = 0.0;
        tick_timers(&mut p);
        assert_eq!(p.death_time, 1);
    }

    #[test]
    fn server_copy_of_a_standing_player_settles_at_gravity() {
        let world = World::flat(-63);
        let mut p = player();
        p.on_ground = true;
        reset_server_copy(&mut p);
        for _ in 0..5 {
            let start = TickStart::of(&p);
            server_tick(&mut p, &start, &world);
        }
        assert_eq!(
            p.server_vel,
            Vec3::new(0.0, -0.08 * f64::from(0.98_f32), 0.0)
        );
        assert!(p.server.on_ground);
    }

    #[test]
    fn server_jumps_when_the_client_leaves_the_ground() {
        let world = World::flat(-63);
        let mut p = player();
        p.on_ground = true;
        p.sprinting = true;
        reset_server_copy(&mut p);
        let start = TickStart::of(&p);
        // the client jumped: moved up, off the ground
        p.pos = Vec3::new(0.5, -63.0 + 0.42, 0.5);
        p.on_ground = false;
        server_tick(&mut p, &start, &world);
        // after the jump (0.42, +0.2 along +z for yaw 0) the server's own tick applied one step
        assert_eq!(
            p.server_vel.y,
            (f64::from(0.42_f32) - 0.08) * f64::from(0.98_f32)
        );
        assert_eq!(p.server_vel.z, 0.2 * f64::from(1.0_f32 * 0.91));
        assert!(!p.server.on_ground);
    }
}
