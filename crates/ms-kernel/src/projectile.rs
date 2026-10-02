//! Projectiles: thrown items (snowball, egg, ender pearl) and arrows (arrow, spectral arrow) —
//! flight under gravity and drag, water drag, collision with block shapes along the flight path,
//! arrows sticking in blocks, and hits on the player (damage and knockback).
//!
//! Owned by the projectile port. Projectiles are server-side entities: the arena ticks them in the
//! entity phase of a server tick, after the server has handled the player's move packet and sent
//! pending knockback (see the module documentation of [`crate::damage`]) and before the player's
//! own server tick.
//!
//! # What one tick does
//!
//! [`Projectile::tick`] is `ServerLevel.tickNonPassenger` for one projectile: remember the old
//! position and rotation, bump the tick counter, then the entity's own `tick`.
//!
//! * Thrown items (`ThrowableProjectile.tick`): the first-tick bubble-column check, gravity (0.03),
//!   inertia (0.99 in air, 0.8 when the *previous* tick's water check said "in water"), the hit
//!   search along the new velocity (block clip with the collision shapes, then the entities'
//!   inflated boxes), the move to the hit point (or the full step), the rotation update, the
//!   block-inside effects (bubble columns), then the shared entity tick (water and lava state with
//!   fluid pushing) and, on a hit, the impact: a block hit discards the item; an entity hit hurts
//!   the entity for 0 damage (snowball, egg, pearl) and discards it.
//! * Arrows (`AbstractArrow.tick`): the "is the arrow inside a collision box" check, the shake and
//!   life counters, and then either the stuck branch (re-check whether the block it hit is still
//!   there; fall out of the block, or count towards the 1200 tick despawn) or flight: water
//!   inertia (0.6) applied *before* the step, the rotation update from the velocity at the start
//!   of the tick, the block clip, the step with entity hits (`stepMoveAndHit`), air inertia
//!   (0.99), gravity (0.05) and the shared entity tick. A block hit snaps the arrow back by
//!   `0.05 * signum(velocity)` from the hit point, zeroes the velocity and marks it stuck.
//!
//! # Randomness
//!
//! Vanilla draws from the entity's own `RandomSource`, which is seeded from the clock, so nothing
//! that depends on it can be reproduced against the game: the extra damage of a critical arrow,
//! the random speed-up when a stuck arrow's block disappears, the bounce direction of an arrow
//! that fails to hurt its target, and the egg's chicken spawn. Each projectile carries an
//! [`EntityRandom`] (a `java.util.Random` stand-in, seed settable with
//! [`Projectile::with_seed`]) so that these cases are deterministic *within* the simulator; all
//! non-random paths (everything the oracle corpus exercises) are exact. Eggs never spawn chickens
//! (mobs are not simulated).
//!
//! # Numerics worth knowing
//!
//! * `Mth.atan2` is *not* `Math.atan2`: it is a 257-entry table interpolation ([`mth_atan2`]).
//! * The "radians to degrees" factor in the rotation code is the double `57.2957763671875`, the
//!   float `180.0F / (float) Math.PI` widened (the decompiled text hides this); it makes a pure
//!   `-z` flight round to 179.99998 degrees instead of 180, so the first rotation lerp does not
//!   wrap.
//! * Rotation uses `lerpRotation` with `Mth.lerp(0.2F, ...)` in `f32`.
//!
//! # Not modelled
//!
//! Block side effects of `onProjectileHit` (buttons, targets, bells, chorus flowers, decorated
//! pots, TNT, campfires: the world is read-only here), fire arrows and burning, tipped-arrow
//! potion effects and the spectral arrow's glowing, piercing and multishot, tridents and the other
//! projectile types, portals and dimension changes, the world border bounce, entities other than
//! the one target passed to the tick (the player), and the pearl's `Endermite`/portal-ticket
//! details. Ender pearls report where they would teleport their owner ([`TickReport::owner_teleport`])
//! and leave moving the owner to the caller.

use crate::damage::{self, DamageSource};
use crate::state::PlayerState;
use ms_numerics::Vec3;
use ms_world::aabb::Aabb;
use ms_world::coords::MIN_Y;
use ms_world::World;

// ---------------------------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------------------------

/// `ThrowableProjectile.getDefaultGravity`.
const THROWABLE_GRAVITY: f64 = 0.03;
/// `AbstractArrow.getDefaultGravity`.
const ARROW_GRAVITY: f64 = 0.05;
/// Inertia of every projectile in air (`0.99F`).
const AIR_INERTIA: f32 = 0.99;
/// `ThrowableProjectile.applyInertia` in water (`0.8F`).
const THROWABLE_WATER_INERTIA: f32 = 0.8;
/// `AbstractArrow.WATER_INERTIA` (`0.6F`).
const ARROW_WATER_INERTIA: f32 = 0.6;
/// The double the rotation code multiplies radians by: `(double) (180.0F / (float) Math.PI)`.
const RAD_TO_DEG: f64 = 57.2957763671875;
/// `AbstractArrow.SHAKE_TIME`.
const SHAKE_TIME: i32 = 7;
/// The tick on which a stuck arrow's `life` counter discards it.
const ARROW_LIFE_LIMIT: i32 = 1200;
/// `AbstractArrow.ARROW_BASE_DAMAGE`.
const ARROW_BASE_DAMAGE: f64 = 2.0;
/// `0.05F`, the distance an arrow is pushed back out of the block it hit.
const ARROW_BACK_OFF: f64 = 0.05_f32 as f64;
/// Strength `LivingEntity.hurtServer` hands to `knockback` (`0.4F`).
const HURT_KNOCKBACK: f64 = 0.4_f32 as f64;
/// Overworld lava fluid push strength (`FAST_LAVA` is a nether attribute).
const LAVA_PUSH: f64 = 0.0023333333333333335;
/// Water fluid push strength.
const WATER_PUSH: f64 = 0.014;
/// Entities below `minY - 64` are discarded by `Entity.checkBelowWorld`.
const BELOW_WORLD_Y: f64 = (MIN_Y - 64) as f64;

// ---------------------------------------------------------------------------------------------
// Java numeric helpers
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

/// `Mth.ceil(double)`.
fn mth_ceil(d: f64) -> i32 {
    let i = d as i32;
    if d > f64::from(i) {
        i.wrapping_add(1)
    } else {
        i
    }
}

/// `Mth.lfloor(double)`.
fn mth_lfloor(d: f64) -> i64 {
    let l = d as i64;
    if d < l as f64 {
        l.wrapping_sub(1)
    } else {
        l
    }
}

/// `Mth.frac(double)`.
fn mth_frac(d: f64) -> f64 {
    d - mth_lfloor(d) as f64
}

/// `Mth.sign(double)`.
fn mth_sign(d: f64) -> i32 {
    if d == 0.0 {
        0
    } else if d > 0.0 {
        1
    } else {
        -1
    }
}

/// `Math.min(double, double)`: NaN propagates and `-0.0 < 0.0`.
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

/// `Math.min(float, float)`.
fn java_min_f32(a: f32, b: f32) -> f32 {
    java_min(f64::from(a), f64::from(b)) as f32
}

/// `Math.max(float, float)`.
fn java_max_f32(a: f32, b: f32) -> f32 {
    java_max(f64::from(a), f64::from(b)) as f32
}

/// `Math.signum(double)`: `-0.0` and `0.0` stay themselves, NaN stays NaN.
fn java_signum(d: f64) -> f64 {
    if d == 0.0 || d.is_nan() {
        d
    } else if d > 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// `Mth.clamp(double, double, double)`.
fn mth_clamp(d: f64, lo: f64, hi: f64) -> f64 {
    if d < lo {
        lo
    } else {
        java_min(d, hi)
    }
}

/// `Mth.lerp(double, double, double)`.
fn mth_lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

/// `Mth.lerp(float, float, float)`.
fn mth_lerp_f32(t: f32, a: f32, b: f32) -> f32 {
    a + t * (b - a)
}

/// `Double.compare(a, b) == 0` (bitwise equality with one NaN, which is what `Vec3.equals` uses).
fn java_double_eq(a: f64, b: f64) -> bool {
    if a.is_nan() && b.is_nan() {
        true
    } else {
        a.to_bits() == b.to_bits()
    }
}

// ---------------------------------------------------------------------------------------------
// Vec3 helpers (the game's `Vec3` operations, operation for operation)
// ---------------------------------------------------------------------------------------------

fn v_add(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

fn v_sub(a: Vec3, b: Vec3) -> Vec3 {
    // `subtract` is `add(-x, -y, -z)`, which is the same IEEE operation as a subtraction.
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

fn v_scale(a: Vec3, s: f64) -> Vec3 {
    Vec3::new(a.x * s, a.y * s, a.z * s)
}

fn v_length_sqr(a: Vec3) -> f64 {
    a.x * a.x + a.y * a.y + a.z * a.z
}

fn v_length(a: Vec3) -> f64 {
    v_length_sqr(a).sqrt()
}

fn v_horizontal_distance(a: Vec3) -> f64 {
    (a.x * a.x + a.z * a.z).sqrt()
}

/// `Vec3.normalize`.
fn v_normalize(a: Vec3) -> Vec3 {
    let d = (a.x * a.x + a.y * a.y + a.z * a.z).sqrt();
    if d < f64::from(1.0E-5_f32) {
        Vec3::ZERO
    } else {
        Vec3::new(a.x / d, a.y / d, a.z / d)
    }
}

/// `a.distanceToSqr(b)`.
fn v_distance_sqr(a: Vec3, b: Vec3) -> f64 {
    let d = b.x - a.x;
    let e = b.y - a.y;
    let f = b.z - a.z;
    d * d + e * e + f * f
}

/// `Vec3.equals`.
fn v_equals(a: Vec3, b: Vec3) -> bool {
    java_double_eq(a.x, b.x) && java_double_eq(a.y, b.y) && java_double_eq(a.z, b.z)
}

/// `BlockPos.containing(Vec3)`.
fn block_pos_of(p: Vec3) -> (i32, i32, i32) {
    (mth_floor(p.x), mth_floor(p.y), mth_floor(p.z))
}

// ---------------------------------------------------------------------------------------------
// Mth.atan2
// ---------------------------------------------------------------------------------------------

/// `Mth.ASIN_TAB` and `Mth.COS_TAB` (257 entries each, interleaved `[asin(i / 256), cos(asin(i / 256))]`)
/// as raw `f64` bit patterns. The game fills them at class load with `Math.asin` / `Math.cos`; the
/// values here were dumped from a JVM running the 1.21.11 classes.
#[rustfmt::skip]
const ATAN_TABLE: [u64; 514] = [
    0x0000000000000000, 0x3ff0000000000000,
    0x3f700002aaabdddf, 0x3fefffeffffbfffe,
    0x3f80000aaabdde0c, 0x3fefffbfffbfff80,
    0x3f8800240091cfda, 0x3fefff6ffebbfa4e,
    0x3f90002aabdde94c, 0x3feffefffbffdfff,
    0x3f94005358ff0bd8, 0x3feffe6ff63b85e7,
    0x3f980090091d9024, 0x3feffdbfebbe9360,
    0x3f9c00e4be5f0304, 0x3feffcefda786870,
    0x3fa000aabde0b9c8, 0x3feffbffbff7fec0,
    0x3fa200f3229fdf2c, 0x3feffaef996bc4e8,
    0x3fa4014d8ffaf8af, 0x3feff9bf63a1740b,
    0x3fa601bc0922e634, 0x3feff86f1b05dfb0,
    0x3fa8024091fdb0a9, 0x3feff6febba4bfea,
    0x3faa02dd2f38ebf6, 0x3feff56e412875ab,
    0x3fac0393e65c2c93, 0x3feff3bda6d9c950,
    0x3fae0466bddb929c, 0x3feff1ece79fa355,
    0x3fb002abde953619, 0x3fefeffbfdfebf1f,
    0x3fb103347666f892, 0x3fefedeae41957e6,
    0x3fb203ce2b380cd3, 0x3fefebb993aecf99,
    0x3fb3047a02794911, 0x3fefe968061b4fc8,
    0x3fb405390240e6fd, 0x3fefe6f634576477,
    0x3fb5060c31541da5, 0x3fefe46416f790d1,
    0x3fb606f49730ccc5, 0x3fefe1b1a62bddad,
    0x3fb707f33c173a99, 0x3fefdeded9bf61d3,
    0x3fb809092913e52e, 0x3fefdbeba917c3f5,
    0x3fb90a3768096840, 0x3fefd8d80b34b63e,
    0x3fba0b7f03ba78ac, 0x3fefd5a3f6af6b74,
    0x3fbb0ce107d3f690, 0x3fefd24f61ba0590,
    0x3fbc0e5e80f7172d, 0x3fefceda421efdb5,
    0x3fbd0ff87cc3a7a5, 0x3fefcb448d40857c,
    0x3fbe11b009e269b5, 0x3fefc78e3817e16e,
    0x3fbf1386380f8b9a, 0x3fefc3b73734bca2,
    0x3fc00abe0c129e1e, 0x3fefbfbf7ebc755f,
    0x3fc08bc95e132e6f, 0x3fefbba7026962a7,
    0x3fc10ce59ba4a8c4, 0x3fefb76db58a1299,
    0x3fc18e134f0178af, 0x3fefb3138b008181,
    0x3fc20f530308cc20, 0x3fefae987541497f,
    0x3fc290a543442d6a, 0x3fefa9fc6652caa7,
    0x3fc3120a9bed2f46, 0x3fefa53f4fcc4b79,
    0x3fc3938399f32b5c, 0x3fefa06122d5118c,
    0x3fc41510cb011423, 0x3fef9b61d0237250,
    0x3fc496b2bd835ab9, 0x3fef964147fbdbbf,
    0x3fc5186a00ade974, 0x3fef90ff7a2fd4d2,
    0x3fc59a37248233ea, 0x3fef8b9c561cf5a1,
    0x3fc61c1ab9d55d30, 0x3fef8617caabd6f6,
    0x3fc69e1552567517, 0x3fef8071c64ef938,
    0x3fc720278094cd3c, 0x3fef7aaa3701a270,
    0x3fc7a251d80666ab, 0x3fef74c10a46b354,
    0x3fc82494ed0e78fc, 0x3fef6eb62d27730d,
    0x3fc8a6f1550413c4, 0x3fef68898c325199,
    0x3fc92967a638db38, 0x3fef623b1379a09b,
    0x3fc9abf877ffe0e8, 0x3fef5bcaae92424e,
    0x3fca2ea462b4998e, 0x3fef553848924e81,
    0x3fcab16bffc1f0dd, 0x3fef4e83cc0fad49,
    0x3fcb344fe9a97c4d, 0x3fef47ad231ea746,
    0x3fcbb750bc0acdef, 0x3fef40b437506b2f,
    0x3fcc3a6f13aae84b, 0x3fef3998f1b1886c,
    0x3fccbdab8e7bd466, 0x3fef325b3ac85e81,
    0x3fcd4106cba45b08, 0x3fef2afafa9380f9,
    0x3fcdc4816b87e25e, 0x3fef237818880fa4,
    0x3fce481c0fce7134, 0x3fef1bd27b9002c4,
    0x3fcecbd75b6cd8f0, 0x3fef140a0a086af1,
    0x3fcf4fb3f2ad079b, 0x3fef0c1ea9bfa45f,
    0x3fcfd3b27b368330, 0x3fef04103ff37d41,
    0x3fd02be9ce0b87cd, 0x3feefbdeb14f4eda,
    0x3fd06e0bfee5c057, 0x3feef389e1ea090d,
    0x3fd0b04025245ccc, 0x3feeeb11b5442ff1,
    0x3fd0f28696826d95, 0x3feee2760e45cb23,
    0x3fd134dfa9805147, 0x3feed9b6cf3c4663,
    0x3fd1774bb5687cf9, 0x3feed0d3d9d8432f,
    0x3fd1b9cb12545e62, 0x3feec7cd0f2b5adf,
    0x3fd1fc5e19315893, 0x3feebea24fa5d0ec,
    0x3fd23f0523c5dc2b, 0x3feeb5537b1434da,
    0x3fd281c08cb69be8, 0x3feeabe0709cf371,
    0x3fd2c490af8bde81, 0x3feea2490ebdd6b8,
    0x3fd30775e8b6eeb9, 0x3fee988d33497437,
    0x3fd34a709597aab1, 0x3fee8eacbb648910,
    0x3fd38d8114823369, 0x3fee84a78383435d,
    0x3fd3d0a7c4c4bd9c, 0x3fee7a7d6766784b,
    0x3fd413e506ad84ee, 0x3fee702e4218c668,
    0x3fd457393b90e2aa, 0x3fee65b9edeba38e,
    0x3fd49aa4c5cf8926, 0x3fee5b20447455cb,
    0x3fd4de2808dce513, 0x3fee50611e88d6b5,
    0x3fd521c36945a5f2, 0x3fee457c543ca073,
    0x3fd565774cb66f02, 0x3fee3a71bcdd63de,
    0x3fd5a9441a02b1fd, 0x3fee2f412eefa6f9,
    0x3fd5ed2a392bb50f, 0x3fee23ea802b4b1a,
    0x3fd6312a1367c57c, 0x3fee186d8577f9ec,
    0x3fd675441329986e, 0x3fee0cca12e97895,
    0x3fd6b978a427db95, 0x3fee00fffbbbe023,
    0x3fd6fdc83364f719, 0x3fedf50f124fba75,
    0x3fd742332f3702b4, 0x3fede8f7282602ae,
    0x3fd786ba074fef93, 0x3feddcb80ddc085b,
    0x3fd7cb5d2cc5e8ed, 0x3fedd05193273445,
    0x3fd8101d121bed2d, 0x3fedc3c386d0ae09,
    0x3fd854fa2b4aa1a3, 0x3fedb70db6b0e156,
    0x3fd899f4edc962d3, 0x3fedaa2fefaae1d8,
    0x3fd8df0dd0979384, 0x3fed9d29fda7ac9c,
    0x3fd924454c462cc4, 0x3fed8ffbab9145d5,
    0x3fd9699bdb019139, 0x3fed82a4c34db1c6,
    0x3fd9af11f89ba61c, 0x3fed75250db9c792,
    0x3fd9f4a82296347b, 0x3fed677c52a3dc9b,
    0x3fda3a5ed82d9537, 0x3fed59aa58c6471c,
    0x3fda80369a63aaa8, 0x3fed4baee5c1b694,
    0x3fdac62fec0b2a92, 0x3fed3d89be176072,
    0x3fdb0c4b51d33b86, 0x3fed2f3aa522ff95,
    0x3fdb5289525368ab, 0x3fed20c15d14a4e5,
    0x3fdb98ea7617ef3a, 0x3fed121da6ea5769,
    0x3fdbdf6f47ae6904, 0x3fed034f42698214,
    0x3fdc261853b2d785, 0x3fecf455ee182d6d,
    0x3fdc6ce628dd132c, 0x3fece5316736032e,
    0x3fdcb3d9580ea2b8, 0x3fecd5e169b519d7,
    0x3fdcfaf27460fe9f, 0x3fecc665b0328622,
    0x3fdd4232133444ad, 0x3fecb6bdf3eeb01f,
    0x3fdd8998cc3e6049, 0x3feca6e9ecc569b9,
    0x3fddd127399aabe7, 0x3fec96e95125c43e,
    0x3fde18ddf7da106b, 0x3fec86bbd609a260,
    0x3fde60bda613a78f, 0x3fec76612eed0424,
    0x3fdea8c6e5f5e67f, 0x3fec65d90dc509f4,
    0x3fdef0fa5bd85625, 0x3fec552322f6abf5,
    0x3fdf3958aecddef4, 0x3fec443f1d4d22af,
    0x3fdf81e288b7ae20, 0x3fec332ca9effdd0,
    0x3fdfca989658baaf, 0x3fec21eb7458e5cc,
    0x3fe009bdc3b4f877, 0x3fec107b264904d9,
    0x3fe02e46075785a1, 0x3febfedb67be13b3,
    0x3fe052e571060fd4, 0x3febed0bdee7064d,
    0x3fe0779c5d4df4b8, 0x3febdb0c30185485,
    0x3fe09c6b2a636bb7, 0x3febc8dbfdbfda88,
    0x3fe0c152382d7366, 0x3febb67ae8584caa,
    0x3fe0e651e85229ce, 0x3feba3e88e5c39ec,
    0x3fe10b6a9e43942f, 0x3feb91248c38986b,
    0x3fe1309cbf4cdb24, 0x3feb7e2e7c3ed68e,
    0x3fe155e8b2a00052, 0x3feb6b05f6966b9b,
    0x3fe17b4ee1641318, 0x3feb57aa912de205,
    0x3fe1a0cfb6c3e9eb, 0x3feb441bdfab5580,
    0x3fe1c66b9ffd666d, 0x3feb3059735c5e90,
    0x3fe1ec230c714a96, 0x3feb1c62db2564ff,
    0x3fe211f66db3a5a1, 0x3feb0837a370523a,
    0x3fe237e6379cdfc6, 0x3feaf3d7561a9c44,
    0x3fe25df2e05b6c41, 0x3feadf417a62a16d,
    0x3fe2841ce0862974, 0x3feaca7594d44cbd,
    0x3fe2aa64b32f7783, 0x3feab5732734fa47,
    0x3fe2d0cad5f90e20, 0x3feaa039b06e926d,
    0x3fe2f74fc9289adc, 0x3fea8ac8ac79d249,
    0x3fe31df40fbd31cd, 0x3fea751f9447b724,
    0x3fe344b82f859adf, 0x3fea5f3dddaa0225,
    0x3fe36b9cb13786e1, 0x3fea4922fb3ac8c2,
    0x3fe392a22087b7e9, 0x3fea32ce5c4305ef,
    0x3fe3b9c90c43296d, 0x3fea1c3f6ca01f29,
    0x3fe3e11206694523, 0x3fea057594a84fc8,
    0x3fe4087da4473296, 0x3fe9ee70390dec3c,
    0x3fe4300c7e945024, 0x3fe9d72ebac16dd6,
    0x3fe457bf318fe517, 0x3fe9bfb076d236eb,
    0x3fe47f965d201d78, 0x3fe9a7f4c64dfe15,
    0x3fe4a792a4f26152, 0x3fe98ffafe1ece30,
    0x3fe4cfb4b09d1a3e, 0x3fe977c26ee7878a,
    0x3fe4f7fd2bc2fb34, 0x3fe95f4a64decda8,
    0x3fe5206cc637e012, 0x3fe9469227a84b4c,
    0x3fe5490434275b92, 0x3fe92d98fa2c355d,
    0x3fe571c42e3d0be7, 0x3fe9145e1a6cf381,
    0x3fe59aad71ced00f, 0x3fe8fae0c15ad38a,
    0x3fe5c3c0c108f95c, 0x3fe8e12022a5ab3a,
    0x3fe5ecfee31c96e7, 0x3fe8c71b6c8c49b4,
    0x3fe61668a46ffa82, 0x3fe8acd1c7a997f1,
    0x3fe63ffed6d198f6, 0x3fe8924256bf4545,
    0x3fe669c251ad69e7, 0x3fe8776c367dda86,
    0x3fe693b3f244ee17, 0x3fe85c4e7d4a0bb1,
    0x3fe6bdd49bea05ce, 0x3fe840e83aff1d1d,
    0x3fe6e825383cc40b, 0x3fe8253878ae2e09,
    0x3fe712a6b76c6e92, 0x3fe8093e385a3700,
    0x3fe73d5a107bde74, 0x3fe7ecf874b086df,
    0x3fe76840418978a7, 0x3fe7d06620bd8524,
    0x3fe7935a501afa78, 0x3fe7b386279d7bf3,
    0x3fe7bea9496d5a54, 0x3fe796576c292765,
    0x3fe7ea2e42c9027a, 0x3fe778d8c89dc27c,
    0x3fe815ea59dab0a2, 0x3fe75b090e40447a,
    0x3fe841deb5114bb4, 0x3fe73ce704fb7b23,
    0x3fe86e0c84010764, 0x3fe71e716af8a794,
    0x3fe89a74ffcc34a4, 0x3fe6ffa6f4323c0d,
    0x3fe8c7196b9225de, 0x3fe6e0864a0050d6,
    0x3fe8f3fb14e496b4, 0x3fe6c10e0a9e5d66,
    0x3fe9211b54441083, 0x3fe6a13cc8a9b946,
    0x3fe94e7b8da3cf7a, 0x3fe681110a985d4d,
    0x3fe97c1d30f5b7d2, 0x3fe660894a2751cc,
    0x3fe9aa01babef75e, 0x3fe63fa3f3c02962,
    0x3fe9d82ab4b5fdfd, 0x3fe61e5f65d4d978,
    0x3fea0699b66a8718, 0x3fe5fcb9f031317b,
    0x3fea355065f87fa4, 0x3fe5dab1d341202a,
    0x3fea645078c6a78c, 0x3fe5b8453f4ae294,
    0x3fea939bb451e2a0, 0x3fe59572539c229b,
    0x3feac333ef06451a, 0x3fe572371da8f26e,
    0x3feaf31b1127022e, 0x3fe54e91981b7778,
    0x3feb235315c680dc, 0x3fe52a7fa9d2f8ea,
    0x3feb53de0bcffc24, 0x3fe505ff24d0e46b,
    0x3feb84be172438ef, 0x3fe4e10dc51235a7,
    0x3febb5f571cb0571, 0x3fe4bba92f53830a,
    0x3febe7866d3b6480, 0x3fe495ceefbdc28b,
    0x3fec197373bc7bf1, 0x3fe46f7c7879a3bc,
    0x3fec4bbf09e19830, 0x3fe448af2027201c,
    0x3fec7e6bd023da76, 0x3fe4216420369e50,
    0x3fecb17c849c7288, 0x3fe3f9989320b7f7,
    0x3fece4f404e29b3a, 0x3fe3d14972795a11,
    0x3fed18d55010f295, 0x3fe3a87394da947a,
    0x3fed4d2388f6360b, 0x3fe37f13aba2fbb6,
    0x3fed81e1f875ea8d, 0x3fe355264082fea0,
    0x3fedb714101e0a0e, 0x3fe32aa7b2d3fd8f,
    0x3fedecbd6cf77786, 0x3fe2ff9434b34639,
    0x3fee22e1da97bb17, 0x3fe2d3e7c7da540a,
    0x3fee5985567b665d, 0x3fe2a79e3a2cd2e6,
    0x3fee90ac13b18234, 0x3fe27ab321f3dcf1,
    0x3feec85a7ee191da, 0x3fe24d21d9bcbd2d,
    0x3fef009542b712f2, 0x3fe21ee57bd01fd8,
    0x3fef39614cbef7d4, 0x3fe1eff8dd34fde3,
    0x3fef72c3d2c5752a, 0x3fe1c0568830ae9d,
    0x3fefacc258c4aaf9, 0x3fe18ff8b63353dc,
    0x3fefe762b77744d5, 0x3fe15ed9491d3888,
    0x3ff0115591d29d12, 0x3fe12cf1c3c6a214,
    0x3ff02f511b223c0f, 0x3fe0fa3b41afe8ac,
    0x3ff04da77ac5c9e5, 0x3fe0c6ae6dbb479d,
    0x3ff06c5c6f8ce9cc, 0x3fe0924377cc95c0,
    0x3ff08b73f9af1058, 0x3fe05cf20924c255,
    0x3ff0aaf2613700b3, 0x3fe026b137474aca,
    0x3ff0cadc3d4378b1, 0x3fdfdeeeea5d2083,
    0x3ff0eb367c3fd618, 0x3fdf6e75051127be,
    0x3ff10c066d3e6932, 0x3fdefbdeb14f4ed9,
    0x3ff12d51caa6b58b, 0x3fde87142982b1b2,
    0x3ff14f1ec67484ed, 0x3fde0ffbbe00c34f,
    0x3ff1717418520341, 0x3fdd96799ba6f66c,
    0x3ff194590de7e7f6, 0x3fdd1a6f89821640,
    0x3ff1b7d59dd40ba2, 0x3fdc9bbc9bb903fc,
    0x3ff1dbf27dd2221a, 0x3fdc1a3cd9862dfc,
    0x3ff200b93cc5a540, 0x3fdb95c8d37c8daa,
    0x3ff2263461820ad8, 0x3fdb0e35269b38f4,
    0x3ff24c6f8f6affb2, 0x3fda8351e7be222b,
    0x3ff27377b2570a1e, 0x3fd9f4e9f1b5859e,
    0x3ff29b5b338b7c8d, 0x3fd962c20e982fea,
    0x3ff2c42a3a3c7a87, 0x3fd8cc97f2912db7,
    0x3ff2edf6fac7f5b8, 0x3fd83220fb33564e,
    0x3ff318d619008ed9, 0x3fd79308a1dd964b,
    0x3ff344df237486d7, 0x3fd6eeee8953be07,
    0x3ff3722d2feb24c8, 0x3fd645640568c1c3,
    0x3ff3a0dfa4bb4aff, 0x3fd595e8ede2700c,
    0x3ff3d11b3fc3b697, 0x3fd4dfe7790ba697,
    0x3ff4030b73c55372, 0x3fd422aeba618155,
    0x3ff436e4418e69c0, 0x3fd35d6b2ed1914a,
    0x3ff46ce4c738c4ea, 0x3fd28f1c6c540fb9,
    0x3ff4a55ae332c7a5, 0x3fd1b6867c415165,
    0x3ff4e0a887c40a9c, 0x3fd0d21c6aeb714f,
    0x3ff51f4bd13f8591, 0x3fcfbfbf7ebc755d,
    0x3ff561ebd9c18cd3, 0x3fcdba59d10bfe0a,
    0x3ff5a96e34bc532b, 0x3fcb8cc9d3952a43,
    0x3ff5f71d7ff42c8a, 0x3fc92ca4f010700e,
    0x3ff64cf55148366e, 0x3fc689f26c6b01d5,
    0x3ff6ae4c63222736, 0x3fc389d6226c1299,
    0x3ff721a5d8718655, 0x3fbfeffbfdfebf21,
    0x3ff7b7d33b928c5b, 0x3fb69af589b35960,
    0x3ff921fb54442d18, 0x3c91a62633145c07,
];

const FRAC_BIAS: f64 = f64::from_bits(4805340802404319232);

/// `Mth.fastInvSqrt`.
fn fast_inv_sqrt(d: f64) -> f64 {
    let e = 0.5 * d;
    let mut l = d.to_bits() as i64;
    l = 6910469410427058090_i64.wrapping_sub(l >> 1);
    let d = f64::from_bits(l as u64);
    d * (1.5 - e * d * d)
}

/// `Mth.atan2(y, x)`: the game's own table-driven arctangent (not `Math.atan2`). It normalises
/// the pair with an approximate inverse square root, reads the angle's sine and cosine from a
/// 1/256-step table at the rounded ratio and finishes with a cubic correction, then folds the
/// result back into the right octant. Its output differs from a correctly rounded `atan2` in the
/// low bits, which is exactly what the rotation fields record.
pub fn mth_atan2(d: f64, e: f64) -> f64 {
    let (mut d, mut e) = (d, e);
    let f = e * e + d * d;
    if f.is_nan() {
        return f64::NAN;
    }
    let negative_y = d < 0.0;
    if negative_y {
        d = -d;
    }
    let negative_x = e < 0.0;
    if negative_x {
        e = -e;
    }
    let swapped = d > e;
    if swapped {
        std::mem::swap(&mut d, &mut e);
    }
    let g = fast_inv_sqrt(f);
    e *= g;
    d *= g;
    let h = FRAC_BIAS + d;
    // `(int) Double.doubleToRawLongBits(h)`: the low 32 bits hold the rounded 1/256 index.
    let i = h.to_bits() as u32 as i32;
    let Some(idx) = usize::try_from(i).ok().filter(|&i| i < 257) else {
        // The game would throw `ArrayIndexOutOfBoundsException` here (only reachable with
        // non-finite input).
        return f64::NAN;
    };
    let j = f64::from_bits(ATAN_TABLE[idx * 2]);
    let k = f64::from_bits(ATAN_TABLE[idx * 2 + 1]);
    let l = h - FRAC_BIAS;
    let m = d * k - e * l;
    let n = (6.0 + m * m) * m * 0.16666666666666666;
    let mut o = j + n;
    if swapped {
        o = std::f64::consts::FRAC_PI_2 - o;
    }
    if negative_x {
        o = std::f64::consts::PI - o;
    }
    if negative_y {
        o = -o;
    }
    o
}

/// `(float) (Mth.atan2(..) * 57.2957763671875)`: radians to the `f32` degrees the rotation fields
/// hold.
fn degrees(rad: f64) -> f32 {
    (rad * RAD_TO_DEG) as f32
}

/// `Projectile.lerpRotation`: bring `from` within half a turn of `to` and move 20% of the way.
/// (The loops are bounded: the game's would spin forever on a rotation too large for `f32` to
/// step by 360.)
pub fn lerp_rotation(from: f32, to: f32) -> f32 {
    let mut f = from;
    let mut guard = 0;
    while to - f < -180.0 && guard < 100_000 {
        f -= 360.0;
        guard += 1;
    }
    while to - f >= 180.0 && guard < 200_000 {
        f += 360.0;
        guard += 1;
    }
    mth_lerp_f32(0.2, f, to)
}

// ---------------------------------------------------------------------------------------------
// The entity random stand-in
// ---------------------------------------------------------------------------------------------

/// A `java.util.Random` stand-in for the entity's `RandomSource` (see the module documentation:
/// vanilla seeds the real one from the clock, so there is nothing to match).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityRandom {
    seed: u64,
}

impl EntityRandom {
    const MULTIPLIER: u64 = 0x5_DEEC_E66D;
    const MASK: u64 = (1 << 48) - 1;

    pub fn new(seed: i64) -> Self {
        Self {
            seed: (seed as u64 ^ Self::MULTIPLIER) & Self::MASK,
        }
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.seed = self.seed.wrapping_mul(Self::MULTIPLIER).wrapping_add(0xB) & Self::MASK;
        (self.seed >> (48 - bits)) as i32
    }

    /// `RandomSource.nextFloat`.
    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / (1u32 << 24) as f32
    }

    /// `RandomSource.nextInt(bound)`.
    pub fn next_int_bound(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "bound must be positive");
        let m = bound - 1;
        if bound & m == 0 {
            return ((i64::from(bound) * i64::from(self.next(31))) >> 31) as i32;
        }
        let mut u = self.next(31);
        loop {
            let r = u % bound;
            if u.wrapping_sub(r).wrapping_add(m) >= 0 {
                return r;
            }
            u = self.next(31);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Boxes and clipping
// ---------------------------------------------------------------------------------------------

/// `net.minecraft.world.phys.AABB`: the constructor orders each pair of corners.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Bb {
    min_x: f64,
    min_y: f64,
    min_z: f64,
    max_x: f64,
    max_y: f64,
    max_z: f64,
}

impl Bb {
    fn new(x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64) -> Self {
        Self {
            min_x: java_min(x0, x1),
            min_y: java_min(y0, y1),
            min_z: java_min(z0, z1),
            max_x: java_max(x0, x1),
            max_y: java_max(y0, y1),
            max_z: java_max(z0, z1),
        }
    }

    fn from_aabb(b: Aabb) -> Self {
        Self::new(b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z)
    }

    /// `AABB.inflate(double)`.
    fn inflate(self, d: f64) -> Self {
        Self::new(
            self.min_x - d,
            self.min_y - d,
            self.min_z - d,
            self.max_x + d,
            self.max_y + d,
            self.max_z + d,
        )
    }

    /// `AABB.expandTowards(Vec3)`.
    fn expand_towards(self, v: Vec3) -> Self {
        let (mut g, mut h, mut i) = (self.min_x, self.min_y, self.min_z);
        let (mut j, mut k, mut l) = (self.max_x, self.max_y, self.max_z);
        if v.x < 0.0 {
            g += v.x;
        } else if v.x > 0.0 {
            j += v.x;
        }
        if v.y < 0.0 {
            h += v.y;
        } else if v.y > 0.0 {
            k += v.y;
        }
        if v.z < 0.0 {
            i += v.z;
        } else if v.z > 0.0 {
            l += v.z;
        }
        Self::new(g, h, i, j, k, l)
    }

    /// `AABB.intersects(AABB)`.
    fn intersects(self, o: Bb) -> bool {
        self.min_x < o.max_x
            && self.max_x > o.min_x
            && self.min_y < o.max_y
            && self.max_y > o.min_y
            && self.min_z < o.max_z
            && self.max_z > o.min_z
    }

    /// `AABB.contains(Vec3)`: half-open on the upper side.
    fn contains(self, p: Vec3) -> bool {
        p.x >= self.min_x
            && p.x < self.max_x
            && p.y >= self.min_y
            && p.y < self.max_y
            && p.z >= self.min_z
            && p.z < self.max_z
    }

    /// `AABB.getCenter`.
    fn center(self) -> Vec3 {
        Vec3::new(
            mth_lerp(0.5, self.min_x, self.max_x),
            mth_lerp(0.5, self.min_y, self.max_y),
            mth_lerp(0.5, self.min_z, self.max_z),
        )
    }

    /// `AABB.clip(Vec3, Vec3)`: the first point where the segment enters the box.
    fn clip(self, from: Vec3, to: Vec3) -> Option<Vec3> {
        let mut ds = 1.0;
        let mut hit = false;
        let j = to.x - from.x;
        let k = to.y - from.y;
        let l = to.z - from.z;
        clip_box(
            [
                self.min_x, self.min_y, self.min_z, self.max_x, self.max_y, self.max_z,
            ],
            from,
            &mut ds,
            &mut hit,
            (j, k, l),
        );
        if hit {
            Some(Vec3::new(from.x + ds * j, from.y + ds * k, from.z + ds * l))
        } else {
            None
        }
    }
}

/// `AABB.clipPoint`: test the plane at `plane` on the primary axis (the segment advances `d` per
/// unit of `t` along it, starting at `l`), and accept it if the crossing lies inside the two
/// other axes' ranges (with `1.0E-7` slack) and is nearer than the best so far.
#[allow(clippy::too_many_arguments)]
fn clip_point(
    ds: &mut f64,
    hit: &mut bool,
    d: f64,
    e: f64,
    f: f64,
    plane: f64,
    (h, i): (f64, f64),
    (j, k): (f64, f64),
    (l, m, n): (f64, f64, f64),
) {
    let o = (plane - l) / d;
    let p = m + o * e;
    let q = n + o * f;
    if 0.0 < o && o < *ds && h - 1.0E-7 < p && p < i + 1.0E-7 && j - 1.0E-7 < q && q < k + 1.0E-7 {
        *ds = o;
        *hit = true;
    }
}

/// `AABB.getDirection`: clip the segment (`from`, delta `(j, k, l)`) against one box
/// `[min_x, min_y, min_z, max_x, max_y, max_z]`, updating the best parameter `ds` and whether any
/// face was hit.
fn clip_box(b: [f64; 6], from: Vec3, ds: &mut f64, hit: &mut bool, (j, k, l): (f64, f64, f64)) {
    let [d, e, f, g, h, i] = b;
    if j > 1.0E-7 {
        clip_point(
            ds,
            hit,
            j,
            k,
            l,
            d,
            (e, h),
            (f, i),
            (from.x, from.y, from.z),
        );
    } else if j < -1.0E-7 {
        clip_point(
            ds,
            hit,
            j,
            k,
            l,
            g,
            (e, h),
            (f, i),
            (from.x, from.y, from.z),
        );
    }
    if k > 1.0E-7 {
        clip_point(
            ds,
            hit,
            k,
            l,
            j,
            e,
            (f, i),
            (d, g),
            (from.y, from.z, from.x),
        );
    } else if k < -1.0E-7 {
        clip_point(
            ds,
            hit,
            k,
            l,
            j,
            h,
            (f, i),
            (d, g),
            (from.y, from.z, from.x),
        );
    }
    if l > 1.0E-7 {
        clip_point(
            ds,
            hit,
            l,
            j,
            k,
            f,
            (d, g),
            (e, h),
            (from.z, from.x, from.y),
        );
    } else if l < -1.0E-7 {
        clip_point(
            ds,
            hit,
            l,
            j,
            k,
            i,
            (d, g),
            (e, h),
            (from.z, from.x, from.y),
        );
    }
}

/// A block the clip stopped at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockHit {
    pub pos: (i32, i32, i32),
    pub location: Vec3,
    /// The segment started inside the block's collision shape.
    pub inside: bool,
}

type Boxes = &'static [[f64; 6]];

/// `UNSTABLE_BOTTOM` of the scaffolding block: `Block.column(16.0, 0.0, 2.0)`.
const SCAFFOLDING_UNSTABLE_BOTTOM: [[f64; 6]; 1] = [[0.0, 0.0, 0.0, 1.0, 0.125, 1.0]];

/// The collision boxes of the block state at `(x, y, z)` for a projectile at height `entity_y`
/// (`ClipContext.Block.COLLIDER` with the projectile's `EntityCollisionContext`). The recorded
/// shapes are the context-free ones; the only collision shape that depends on a non-living entity
/// is the scaffolding's (a thrown item sees the solid top from above, the thin bottom slab when
/// the scaffolding is unsupported and bottomed, and nothing from the side), and powder snow and
/// liquids are empty for it, as they are in the recorded data.
fn collision_boxes_for_projectile(state: u32, y: i32, entity_y: f64) -> Boxes {
    let boxes = ms_data::collision_boxes(state);
    if boxes.is_empty()
        || ms_data::block_name(ms_data::block_of_state(state)) != "minecraft:scaffolding"
    {
        return boxes;
    }
    let above = |max_y: f64| entity_y > f64::from(y) + max_y - f64::from(1.0E-5_f32);
    if above(1.0) {
        boxes
    } else if ms_data::property(state, "distance") != Some("0")
        && ms_data::property(state, "bottom") == Some("true")
        && above(0.0)
    {
        &SCAFFOLDING_UNSTABLE_BOTTOM
    } else {
        &[]
    }
}

/// `VoxelShape.clip(from, to, pos)` for a shape given as boxes (`toAabbs`, in the order the game
/// lists them). `None` is "no hit".
fn shape_clip(boxes: Boxes, from: Vec3, to: Vec3, pos: (i32, i32, i32)) -> Option<BlockHit> {
    if boxes.is_empty() {
        return None;
    }
    let delta = v_sub(to, from);
    if v_length_sqr(delta) < 1.0E-7 {
        return None;
    }
    // A point just past the start (0.1% along the way): if it is inside the shape the segment
    // starts inside the block and the hit is that point.
    let probe = v_add(from, v_scale(delta, 0.001));
    let rel = Vec3::new(
        probe.x - f64::from(pos.0),
        probe.y - f64::from(pos.1),
        probe.z - f64::from(pos.2),
    );
    let inside = boxes.iter().any(|b| {
        rel.x >= b[0]
            && rel.x < b[3]
            && rel.y >= b[1]
            && rel.y < b[4]
            && rel.z >= b[2]
            && rel.z < b[5]
    });
    if inside {
        return Some(BlockHit {
            pos,
            location: probe,
            inside: true,
        });
    }
    // `AABB.clip(Iterable<AABB>, ...)`: the nearest face crossing over all boxes.
    let d = to.x - from.x;
    let e = to.y - from.y;
    let f = to.z - from.z;
    let mut ds = 1.0;
    let mut hit = false;
    for b in boxes {
        let moved = [
            b[0] + f64::from(pos.0),
            b[1] + f64::from(pos.1),
            b[2] + f64::from(pos.2),
            b[3] + f64::from(pos.0),
            b[4] + f64::from(pos.1),
            b[5] + f64::from(pos.2),
        ];
        clip_box(moved, from, &mut ds, &mut hit, (d, e, f));
    }
    if hit {
        Some(BlockHit {
            pos,
            location: Vec3::new(from.x + ds * d, from.y + ds * e, from.z + ds * f),
            inside: false,
        })
    } else {
        None
    }
}

/// `BlockGetter.traverseBlocks`: the voxel walk from `from` to `to` (both nudged `1.0E-7` of the
/// way inwards), calling `visit` on every block cell in order until it returns something.
fn traverse_blocks<T>(
    from: Vec3,
    to: Vec3,
    mut visit: impl FnMut(i32, i32, i32) -> Option<T>,
) -> Option<T> {
    if v_equals(from, to) {
        return None;
    }
    let d = mth_lerp(-1.0E-7, to.x, from.x);
    let e = mth_lerp(-1.0E-7, to.y, from.y);
    let f = mth_lerp(-1.0E-7, to.z, from.z);
    let g = mth_lerp(-1.0E-7, from.x, to.x);
    let h = mth_lerp(-1.0E-7, from.y, to.y);
    let i = mth_lerp(-1.0E-7, from.z, to.z);
    let mut j = mth_floor(g);
    let mut k = mth_floor(h);
    let mut l = mth_floor(i);
    if let Some(r) = visit(j, k, l) {
        return Some(r);
    }
    let m = d - g;
    let n = e - h;
    let o = f - i;
    let p = mth_sign(m);
    let q = mth_sign(n);
    let r = mth_sign(o);
    let s = if p == 0 { f64::MAX } else { f64::from(p) / m };
    let t = if q == 0 { f64::MAX } else { f64::from(q) / n };
    let u = if r == 0 { f64::MAX } else { f64::from(r) / o };
    let mut v = s * if p > 0 {
        1.0 - mth_frac(g)
    } else {
        mth_frac(g)
    };
    let mut w = t * if q > 0 {
        1.0 - mth_frac(h)
    } else {
        mth_frac(h)
    };
    let mut x = u * if r > 0 {
        1.0 - mth_frac(i)
    } else {
        mth_frac(i)
    };
    while v <= 1.0 || w <= 1.0 || x <= 1.0 {
        if v < w {
            if v < x {
                j = j.wrapping_add(p);
                v += s;
            } else {
                l = l.wrapping_add(r);
                x += u;
            }
        } else if w < x {
            k = k.wrapping_add(q);
            w += t;
        } else {
            l = l.wrapping_add(r);
            x += u;
        }
        if let Some(res) = visit(j, k, l) {
            return Some(res);
        }
    }
    None
}

/// `BlockGetter.clip(ClipContext)` with `ClipContext.Block.COLLIDER` and `Fluid.NONE` for a
/// projectile at height `entity_y`: the first block along the segment whose collision shape the
/// segment touches. `None` is a miss. (The world border clamp of `clipIncludingBorder` is not
/// applied: the default border is thirty million blocks out.)
pub fn clip_blocks(world: &World, from: Vec3, to: Vec3, entity_y: f64) -> Option<BlockHit> {
    traverse_blocks(from, to, |x, y, z| {
        let state = world.block_state(x, y, z);
        let boxes = collision_boxes_for_projectile(state, y, entity_y);
        shape_clip(boxes, from, to, (x, y, z))
    })
}

// ---------------------------------------------------------------------------------------------
// Block queries the projectile code needs
// ---------------------------------------------------------------------------------------------

/// `BlockState.hasLargeCollisionShape`: the collision shape sticks out of the block cell.
fn has_large_collision_shape(state: u32) -> bool {
    ms_data::collision_boxes(state)
        .iter()
        .any(|b| b[0] < 0.0 || b[1] < 0.0 || b[2] < 0.0 || b[3] > 1.0 || b[4] > 1.0 || b[5] > 1.0)
}

/// `Shapes.joinIsNotEmpty(a, b, AND)` for two boxes: the index mergers treat coordinates within
/// `1.0E-7` as one, so the boxes overlap only if each one's lower edge is more than that below
/// the other's upper edge.
fn shapes_overlap(a: Bb, b: Bb) -> bool {
    b.min_x < a.max_x - 1.0E-7
        && a.min_x < b.max_x - 1.0E-7
        && b.min_y < a.max_y - 1.0E-7
        && a.min_y < b.max_y - 1.0E-7
        && b.min_z < a.max_z - 1.0E-7
        && a.min_z < b.max_z - 1.0E-7
}

/// `CollisionGetter.noBlockCollision(null, box)` (`BlockCollisions` with the empty context): no
/// block's collision shape touches `bb`. Blocks one cell outside the box's range only count if
/// their shape is larger than a cell (fences, walls), as in the game.
fn no_block_collision(world: &World, bb: Bb) -> bool {
    let x0 = mth_floor(bb.min_x - 1.0E-7).wrapping_sub(1);
    let x1 = mth_floor(bb.max_x + 1.0E-7).wrapping_add(1);
    let y0 = mth_floor(bb.min_y - 1.0E-7).wrapping_sub(1);
    let y1 = mth_floor(bb.max_y + 1.0E-7).wrapping_add(1);
    let z0 = mth_floor(bb.min_z - 1.0E-7).wrapping_sub(1);
    let z1 = mth_floor(bb.max_z + 1.0E-7).wrapping_add(1);
    // `Cursor3D`: x fastest, then y, then z; the type counts the axes on the outer shell.
    for z in z0..=z1 {
        for y in y0..=y1 {
            for x in x0..=x1 {
                let ty = i32::from(x == x0 || x == x1)
                    + i32::from(y == y0 || y == y1)
                    + i32::from(z == z0 || z == z1);
                if ty == 3 {
                    continue;
                }
                let state = world.block_state(x, y, z);
                let boxes = ms_data::collision_boxes(state);
                if boxes.is_empty() {
                    continue;
                }
                if ty == 1 && !has_large_collision_shape(state) {
                    continue;
                }
                if ty == 2 {
                    // Only moving pistons count on the edges, and their shape comes from a
                    // block entity this model does not have.
                    continue;
                }
                let full_cube = boxes.len() == 1 && boxes[0] == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
                for b in boxes {
                    let moved = Bb::new(
                        b[0] + f64::from(x),
                        b[1] + f64::from(y),
                        b[2] + f64::from(z),
                        b[3] + f64::from(x),
                        b[4] + f64::from(y),
                        b[5] + f64::from(z),
                    );
                    let hit = if full_cube {
                        bb.intersects(moved)
                    } else {
                        shapes_overlap(moved, bb)
                    };
                    if hit {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// Fluids the shared entity tick pushes projectiles with.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FluidTag {
    Water,
    Lava,
}

fn fluid_kind(tag: FluidTag) -> ms_data::FluidKind {
    match tag {
        FluidTag::Water => ms_data::FluidKind::Water,
        FluidTag::Lava => ms_data::FluidKind::Lava,
    }
}

/// `BlockState.blocksMotion`, derived from the recorded collision shape the way the game derives
/// its `legacySolid` cache: the shape's bounds are large (average extent at least 0.729) or a
/// full block tall. (Blocks the game forces solid or non-solid by property, such as signs and
/// ladders, are not told apart.)
fn blocks_motion(state: u32) -> bool {
    let name = ms_data::block_name(ms_data::block_of_state(state));
    if name == "minecraft:cobweb" || name == "minecraft:bamboo_sapling" {
        return false;
    }
    let boxes = ms_data::collision_boxes(state);
    if boxes.is_empty() {
        return false;
    }
    let mut mn = [f64::INFINITY; 3];
    let mut mx = [f64::NEG_INFINITY; 3];
    for b in boxes {
        for a in 0..3 {
            mn[a] = java_min(mn[a], b[a]);
            mx[a] = java_max(mx[a], b[a + 3]);
        }
    }
    let (dx, dy, dz) = (mx[0] - mn[0], mx[1] - mn[1], mx[2] - mn[2]);
    let size = (dx + dy + dz) / 3.0;
    size >= 0.7291666666666666 || dy >= 1.0
}

/// `BlockState.isFaceSturdy(level, pos, direction)` as far as the water-flow code needs it: some
/// single collision box covers the whole face on that side.
fn face_is_full(state: u32, face: Face) -> bool {
    ms_data::collision_boxes(state).iter().any(|b| match face {
        Face::North => b[2] <= 0.0 && b[0] <= 0.0 && b[3] >= 1.0 && b[1] <= 0.0 && b[4] >= 1.0,
        Face::South => b[5] >= 1.0 && b[0] <= 0.0 && b[3] >= 1.0 && b[1] <= 0.0 && b[4] >= 1.0,
        Face::West => b[0] <= 0.0 && b[2] <= 0.0 && b[5] >= 1.0 && b[1] <= 0.0 && b[4] >= 1.0,
        Face::East => b[3] >= 1.0 && b[2] <= 0.0 && b[5] >= 1.0 && b[1] <= 0.0 && b[4] >= 1.0,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Face {
    North,
    East,
    South,
    West,
}

/// `Direction.Plane.HORIZONTAL`, in the game's order, with the step along x and z.
const HORIZONTAL: [(Face, i32, i32); 4] = [
    (Face::North, 0, -1),
    (Face::East, 1, 0),
    (Face::South, 0, 1),
    (Face::West, -1, 0),
];

/// `FlowingFluid.isSolidFace` for the falling-water wall test.
fn is_solid_face(world: &World, tag: FluidTag, x: i32, y: i32, z: i32, face: Option<Face>) -> bool {
    let state = world.block_state(x, y, z);
    if ms_data::fluid(state).kind == fluid_kind(tag) {
        return false;
    }
    let Some(face) = face else {
        // `Direction.UP`
        return true;
    };
    let class = ms_data::block_class(ms_data::block_of_state(state));
    if class == "IceBlock" || class == "FrostedIceBlock" {
        return false;
    }
    face_is_full(state, face)
}

/// `FlowingFluid.getFlow`: the horizontal direction the fluid at `(x, y, z)` flows (normalised),
/// from the height differences to its four neighbours, bent straight down when it is a falling
/// column against a wall.
fn fluid_flow(world: &World, tag: FluidTag, x: i32, y: i32, z: i32, own: ms_data::Fluid) -> Vec3 {
    let kind = fluid_kind(tag);
    let affects_flow = |f: ms_data::Fluid| f.is_empty() || f.kind == kind;
    let mut d = 0.0_f64;
    let mut e = 0.0_f64;
    for (_, sx, sz) in HORIZONTAL {
        let (nx, nz) = (x + sx, z + sz);
        let neighbour = ms_data::fluid(world.block_state(nx, y, nz));
        if !affects_flow(neighbour) {
            continue;
        }
        let mut f = neighbour.own_height();
        let mut g = 0.0_f32;
        if f == 0.0 {
            if !blocks_motion(world.block_state(nx, y, nz)) {
                let below = ms_data::fluid(world.block_state(nx, y - 1, nz));
                if affects_flow(below) {
                    f = below.own_height();
                    if f > 0.0 {
                        g = own.own_height() - (f - 0.8888889_f32);
                    }
                }
            }
        } else if f > 0.0 {
            g = own.own_height() - f;
        }
        if g != 0.0 {
            d += f64::from(sx) * f64::from(g);
            e += f64::from(sz) * f64::from(g);
        }
    }
    let mut flow = Vec3::new(d, 0.0, e);
    if own.falling {
        for (face, sx, sz) in HORIZONTAL {
            let (nx, nz) = (x + sx, z + sz);
            if is_solid_face(world, tag, nx, y, nz, Some(face))
                || is_solid_face(world, tag, nx, y + 1, nz, Some(face))
            {
                flow = v_add(v_normalize(flow), Vec3::new(0.0, -6.0, 0.0));
                break;
            }
        }
    }
    v_normalize(flow)
}

/// `FluidState.getHeight(level, pos)` for water and lava (`FlowingFluid.getHeight`): a full cell
/// when the same fluid is directly above, otherwise the state's own height.
fn fluid_height(world: &World, tag: FluidTag, x: i32, y: i32, z: i32, own: ms_data::Fluid) -> f32 {
    let above = ms_data::fluid(world.block_state(x, y + 1, z));
    if above.kind == fluid_kind(tag) {
        1.0
    } else {
        own.own_height()
    }
}

// ---------------------------------------------------------------------------------------------
// Hit targets
// ---------------------------------------------------------------------------------------------

/// What a projectile hands to the entity it hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectileHit {
    pub kind: ProjectileKind,
    /// The damage passed to `hurtServer` (the arrow's `Mth.ceil(speed * baseDamage)` plus the
    /// critical bonus; 0 for snowballs, eggs and pearls).
    pub amount: f32,
    /// The horizontal direction handed to `LivingEntity.knockback(0.4F, dx, dz)`: the negated
    /// horizontal velocity of the projectile at the moment of impact
    /// (`Projectile.calculateHorizontalHurtKnockbackDirection`, negated by `hurtServer`).
    pub knockback_dx: f64,
    pub knockback_dz: f64,
}

/// An entity a projectile can hit. The simulation has one such entity, the player
/// ([`PlayerState`] implements this).
pub trait HitTarget {
    /// `Entity.getBoundingBox` of the target as the server has it.
    fn hit_box(&self) -> Aabb;

    /// `Entity.canBeHitByProjectile`: alive and pickable (spectators are not).
    fn can_be_hit_by_projectile(&self) -> bool;

    /// `Entity.hurtOrSimulate(source, amount)` followed, when it lands, by the knockback
    /// `LivingEntity.hurtServer` applies. Returns whether the damage landed (an arrow whose damage
    /// does not land bounces off).
    fn hurt_by_projectile(&mut self, hit: &ProjectileHit) -> bool;
}

impl HitTarget for PlayerState {
    fn hit_box(&self) -> Aabb {
        let (w, h) = self.dimensions();
        // EntityDimensions.makeBoundingBox: float half-width and height widened to double.
        let g = f64::from(w / 2.0_f32);
        let h = f64::from(h);
        Aabb::new(
            Vec3::new(self.pos.x - g, self.pos.y, self.pos.z - g),
            Vec3::new(self.pos.x + g, self.pos.y + h, self.pos.z + g),
        )
    }

    fn can_be_hit_by_projectile(&self) -> bool {
        self.is_alive()
    }

    /// `ServerPlayer.hurtServer` with the projectile as direct entity. The damage goes through
    /// [`damage::hurt`]; the knockback `(−vx, −vz)` is applied through [`damage::knockback`] when
    /// the hit was a full one (it opened a new invulnerability window; inside the window only the
    /// excess damage lands, with no knockback), which is what `hurt` does itself for sources that
    /// carry a direction.
    fn hurt_by_projectile(&mut self, hit: &ProjectileHit) -> bool {
        let in_window = self.invulnerable_time as f32 > 10.0;
        let landed = damage::hurt(self, DamageSource::Generic, hit.amount);
        if landed && !in_window {
            damage::knockback(self, HURT_KNOCKBACK, hit.knockback_dx, hit.knockback_dz);
        }
        landed
    }
}

// ---------------------------------------------------------------------------------------------
// The projectile
// ---------------------------------------------------------------------------------------------

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

    /// Thrown items (`ThrowableProjectile`) as opposed to arrows (`AbstractArrow`).
    pub fn is_arrow(self) -> bool {
        matches!(self, Self::Arrow | Self::SpectralArrow)
    }

    /// The entity type's bounding-box width and height (`sized(0.25F, 0.25F)` / `sized(0.5F, 0.5F)`).
    fn dimensions(self) -> (f32, f32) {
        if self.is_arrow() {
            (0.5, 0.5)
        } else {
            (0.25, 0.25)
        }
    }
}

/// One projectile's state: everything the game keeps across ticks that the flight depends on.
#[derive(Clone, Debug, PartialEq)]
pub struct Projectile {
    pub kind: ProjectileKind,
    pub pos: Vec3,
    /// `deltaMovement`.
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// `onGround()`: projectiles never use `Entity.move`, so this stays false.
    pub on_ground: bool,
    /// `AbstractArrow.isInGround`.
    pub in_ground: bool,
    /// The entity was discarded (it hit something, despawned or fell out of the world).
    pub removed: bool,
    /// `tickCount`.
    pub tick_count: i32,
    /// `Entity.xRotO` / `yRotO`: the rotation at the start of the current tick.
    pub x_rot_o: f32,
    pub y_rot_o: f32,
    /// `Entity.xOld/yOld/zOld`: the position at the start of the current tick.
    pub old_pos: Vec3,
    /// `Entity.firstTick`: cleared by the end of the first full entity tick.
    pub first_tick: bool,
    /// `Entity.wasTouchingWater` (`isInWater()`), as of the end of the previous tick.
    pub was_touching_water: bool,
    /// `Entity.isNoGravity`.
    pub no_gravity: bool,
    /// `AbstractArrow.noPhysics` (arrows only).
    pub no_physics: bool,
    /// `AbstractArrow.life`: ticks spent stuck.
    pub life: i32,
    /// `AbstractArrow.shakeTime`.
    pub shake_time: i32,
    /// `AbstractArrow.inGroundTime`.
    pub in_ground_time: i32,
    /// `AbstractArrow.isCritArrow`.
    pub crit: bool,
    /// `AbstractArrow.baseDamage`.
    pub base_damage: f64,
    /// `AbstractArrow.lastState`: the block state the arrow hit, by state id.
    pub last_state: Option<u32>,
    /// The simulated player is this projectile's owner (the thrower or shooter): it can only hit
    /// them after it has left their collision range (`Projectile.leftOwner`).
    pub owner_is_target: bool,
    /// `Projectile.leftOwner`.
    pub left_owner: bool,
    left_owner_checked: bool,
    /// The entity random (see the module documentation).
    pub rng: EntityRandom,
}

/// What a tick of a projectile did to the outside world.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TickReport {
    /// What the projectile struck this tick, if anything.
    pub impact: Option<Impact>,
    /// An ender pearl with the player as its owner hit something: the position the game would
    /// teleport the owner to (`oldPosition()`, the pearl's position at the start of the tick),
    /// followed by 5 points of fall-type damage and a fall-distance reset. Applying it is left to
    /// the caller.
    pub owner_teleport: Option<Vec3>,
}

/// A projectile impact.
#[derive(Clone, Debug, PartialEq)]
pub enum Impact {
    /// A block's collision shape.
    Block { hit: BlockHit },
    /// The target entity.
    Target {
        location: Vec3,
        /// What was handed to the target's `hurt`.
        hit: ProjectileHit,
        /// Whether the damage landed (an arrow whose damage does not land bounces).
        landed: bool,
    },
}

/// The result of the hit search of a tick.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    Miss,
    Block(BlockHit),
    Target(Vec3),
}

impl Projectile {
    /// A projectile spawned at `pos` with velocity `vel` and zero rotation, like an entity created
    /// with `snapTo(pos, 0, 0)` and `setDeltaMovement(vel)` (what the oracle's `spawn` action does).
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
            x_rot_o: 0.0,
            y_rot_o: 0.0,
            old_pos: pos,
            first_tick: true,
            was_touching_water: false,
            no_gravity: false,
            no_physics: false,
            life: 0,
            shake_time: 0,
            in_ground_time: 0,
            crit: false,
            base_damage: ARROW_BASE_DAMAGE,
            last_state: None,
            owner_is_target: false,
            left_owner: false,
            left_owner_checked: false,
            rng: EntityRandom::new(0),
        }
    }

    /// Seed the entity random (see the module documentation).
    pub fn with_seed(mut self, seed: i64) -> Self {
        self.rng = EntityRandom::new(seed);
        self
    }

    /// `Entity.getBoundingBox`: the entity type's box around the position.
    pub fn bounding_box(&self) -> Aabb {
        let b = self.bb();
        Aabb::new(
            Vec3::new(b.min_x, b.min_y, b.min_z),
            Vec3::new(b.max_x, b.max_y, b.max_z),
        )
    }

    fn bb(&self) -> Bb {
        bb_around(self.kind, self.pos)
    }

    /// `Entity.setPos`.
    fn set_pos(&mut self, p: Vec3) {
        self.pos = p;
    }

    /// One server tick of this projectile (see the module documentation). `player` is the entity
    /// projectiles can hit; pass `None` to fly in a world without one.
    pub fn tick(&mut self, world: &World, player: Option<&mut PlayerState>) -> TickReport {
        match player {
            Some(p) => self.tick_with_target(world, Some(p as &mut dyn HitTarget)),
            None => self.tick_with_target(world, None),
        }
    }

    /// [`Projectile::tick`] against any [`HitTarget`].
    pub fn tick_with_target(
        &mut self,
        world: &World,
        target: Option<&mut dyn HitTarget>,
    ) -> TickReport {
        let mut report = TickReport::default();
        if self.removed {
            return report;
        }
        // ServerLevel.tickNonPassenger: remember where and how it stood, count the tick.
        self.old_pos = self.pos;
        self.x_rot_o = self.pitch;
        self.y_rot_o = self.yaw;
        self.tick_count = self.tick_count.wrapping_add(1);
        if self.kind.is_arrow() {
            self.tick_arrow(world, target, &mut report);
        } else {
            self.tick_throwable(world, target, &mut report);
        }
        report
    }

    // ----- shared entity behaviour -----

    /// `Entity.getGravity`.
    fn gravity(&self) -> f64 {
        if self.no_gravity {
            0.0
        } else if self.kind.is_arrow() {
            ARROW_GRAVITY
        } else {
            THROWABLE_GRAVITY
        }
    }

    /// `Entity.applyGravity`.
    fn apply_gravity(&mut self) {
        let d = self.gravity();
        if d != 0.0 {
            self.vel = v_add(self.vel, Vec3::new(0.0, -d, 0.0));
        }
    }

    /// `Entity.isPushedByFluid` (`AbstractArrow` overrides it: not while stuck).
    fn is_pushed_by_fluid(&self) -> bool {
        !(self.kind.is_arrow() && self.in_ground)
    }

    /// `Entity.baseTick` as far as a projectile's state goes: fluid state and pushing, the below
    /// world check, and the end of the first tick.
    fn base_tick(&mut self, world: &World) {
        // updateInWaterStateAndDoFluidPushing
        let in_water =
            self.update_fluid_height_and_do_fluid_pushing(world, FluidTag::Water, WATER_PUSH);
        self.was_touching_water = in_water;
        self.update_fluid_height_and_do_fluid_pushing(world, FluidTag::Lava, LAVA_PUSH);
        // checkBelowWorld -> onBelowWorld -> discard
        if self.pos.y < BELOW_WORLD_Y {
            self.removed = true;
        }
        self.first_tick = false;
    }

    /// `Entity.updateFluidHeightAndDoFluidPushing`: returns whether the (slightly shrunken) box
    /// touches `tag` fluid at or above its bottom, and pushes the velocity along the fluid's flow.
    /// (Chunk loading is not modelled: every cell counts as loaded.)
    fn update_fluid_height_and_do_fluid_pushing(
        &mut self,
        world: &World,
        tag: FluidTag,
        push: f64,
    ) -> bool {
        let bb = self.bb().inflate(-0.001);
        let i = mth_floor(bb.min_x);
        let j = mth_ceil(bb.max_x);
        let k = mth_floor(bb.min_y);
        let l = mth_ceil(bb.max_y);
        let m = mth_floor(bb.min_z);
        let n = mth_ceil(bb.max_z);
        let mut e = 0.0_f64;
        let pushed = self.is_pushed_by_fluid();
        let mut touching = false;
        let mut flow_sum = Vec3::ZERO;
        let mut count = 0_i32;
        let kind = fluid_kind(tag);
        for x in i..j {
            for y in k..l {
                for z in m..n {
                    let own = ms_data::fluid(world.block_state(x, y, z));
                    if own.kind != kind {
                        continue;
                    }
                    let f = f64::from(y) + f64::from(fluid_height(world, tag, x, y, z, own));
                    if f >= bb.min_y {
                        touching = true;
                        e = java_max(f - bb.min_y, e);
                        if pushed {
                            let mut flow = fluid_flow(world, tag, x, y, z, own);
                            if e < 0.4 {
                                flow = v_scale(flow, e);
                            }
                            flow_sum = v_add(flow_sum, flow);
                            count += 1;
                        }
                    }
                }
            }
        }
        if v_length(flow_sum) > 0.0 {
            if count > 0 {
                flow_sum = v_scale(flow_sum, 1.0 / f64::from(count));
            }
            // Only the player keeps the raw average; every other entity gets a unit vector.
            flow_sum = v_normalize(flow_sum);
            let cur = self.vel;
            flow_sum = v_scale(flow_sum, push);
            if cur.x.abs() < 0.003
                && cur.z.abs() < 0.003
                && v_length(flow_sum) < 0.0045000000000000005
            {
                flow_sum = v_scale(v_normalize(flow_sum), 0.0045000000000000005);
            }
            self.vel = v_add(self.vel, flow_sum);
        }
        touching
    }

    /// `Entity.applyEffectsFromBlocks()` for an entity that did not use `move`: the blocks the
    /// bounding box swept between the old and the new position.
    fn apply_effects_from_blocks(&mut self, world: &World) {
        let (from, to) = (self.old_pos, self.pos);
        self.apply_effects_from_blocks_between(world, from, to);
    }

    /// `Entity.applyEffectsFromBlocks(from, to)` / `checkInsideBlocks`: `entityInside` of every
    /// block the box passed through. Of those only the bubble column moves a projectile.
    fn apply_effects_from_blocks_between(&mut self, world: &World, from: Vec3, to: Vec3) {
        // `onGround()` is never true for a projectile, so no `stepOn`.
        let used = self.check_inside_blocks(world, from, to, 16);
        if 16 - used <= 0 {
            self.check_inside_blocks(world, to, to, 1);
        }
    }

    /// `Entity.checkInsideBlocks(from, to, collector, visited, budget)`: returns `lastStep + 1`.
    fn check_inside_blocks(&mut self, world: &World, from: Vec3, to: Vec3, budget: i32) -> i32 {
        let bb = bb_around(self.kind, to).inflate(-f64::from(1.0E-5_f32));
        let moved_far = v_distance_sqr(from, to) > 0.9999900000002526_f64 * 0.9999900000002526_f64;
        let mut visited: Vec<(i32, i32, i32)> = Vec::new();
        let mut last_step = 0;
        let mut blocks: Vec<((i32, i32, i32), i32)> = Vec::new();
        for_each_block_intersected_between(from, to, bb, &mut |pos, step| {
            if step >= budget {
                return false;
            }
            blocks.push((pos, step));
            true
        });
        for (pos, step) in blocks {
            last_step = step;
            if self.removed {
                break;
            }
            let state = world.block_state(pos.0, pos.1, pos.2);
            if !is_bubble_column(state) || visited.contains(&pos) {
                continue;
            }
            visited.push(pos);
            let inside = moved_far
                || bb.intersects(Bb::new(
                    f64::from(pos.0),
                    f64::from(pos.1),
                    f64::from(pos.2),
                    f64::from(pos.0) + 1.0,
                    f64::from(pos.1) + 1.0,
                    f64::from(pos.2) + 1.0,
                ));
            self.bubble_column_inside(world, pos, state, inside);
        }
        last_step + 1
    }

    /// `BubbleColumnBlock.entityInside`: a column with something solid or wet on top drags the
    /// entity inside it, an open one lifts or sinks it above it.
    fn bubble_column_inside(
        &mut self,
        world: &World,
        pos: (i32, i32, i32),
        state: u32,
        inside: bool,
    ) {
        if !inside {
            return;
        }
        let above = world.block_state(pos.0, pos.1 + 1, pos.2);
        let open_above =
            ms_data::collision_boxes(above).is_empty() && ms_data::fluid(above).is_empty();
        let drag_down = ms_data::property(state, "drag") == Some("true");
        if open_above {
            self.on_above_bubble_column(drag_down);
        } else {
            self.on_inside_bubble_column(drag_down);
        }
    }

    /// `Entity.onAboveBubbleColumn` for this kind (`Projectile` adds a fixed push, the ender pearl
    /// uses the clamped `Entity` version, a stuck arrow ignores it).
    fn on_above_bubble_column(&mut self, drag_down: bool) {
        match self.kind {
            ProjectileKind::EnderPearl => {
                let d = if drag_down {
                    java_max(-0.9, self.vel.y - 0.03)
                } else {
                    java_min(1.8, self.vel.y + 0.1)
                };
                self.vel = Vec3::new(self.vel.x, d, self.vel.z);
            }
            _ => {
                if self.kind.is_arrow() && self.in_ground {
                    return;
                }
                let d = if drag_down { -0.03 } else { 0.1 };
                self.vel = v_add(self.vel, Vec3::new(0.0, d, 0.0));
            }
        }
    }

    /// `Entity.onInsideBubbleColumn` for this kind.
    fn on_inside_bubble_column(&mut self, drag_down: bool) {
        match self.kind {
            ProjectileKind::EnderPearl => {
                let d = if drag_down {
                    java_max(-0.3, self.vel.y - 0.03)
                } else {
                    java_min(0.7, self.vel.y + 0.06)
                };
                self.vel = Vec3::new(self.vel.x, d, self.vel.z);
            }
            _ => {
                if self.kind.is_arrow() && self.in_ground {
                    return;
                }
                let d = if drag_down { -0.03 } else { 0.06 };
                self.vel = v_add(self.vel, Vec3::new(0.0, d, 0.0));
            }
        }
    }

    /// `Projectile.checkLeftOwner`.
    fn check_left_owner(&mut self, target: Option<&dyn HitTarget>) {
        if !self.left_owner && !self.left_owner_checked {
            self.left_owner = self.is_outside_owner_collision_range(target);
            self.left_owner_checked = true;
        }
    }

    /// `Projectile.isOutsideOwnerCollisionRange`: with no owner it is trivially true; otherwise the
    /// owner's box must be clear of this projectile's box swept along its velocity and grown by 1.
    fn is_outside_owner_collision_range(&self, target: Option<&dyn HitTarget>) -> bool {
        match (self.owner_is_target, target) {
            (true, Some(t)) => {
                let swept = self.bb().expand_towards(self.vel).inflate(1.0);
                !(t.can_be_hit_by_projectile() && swept.intersects(Bb::from_aabb(t.hit_box())))
            }
            _ => true,
        }
    }

    /// `Projectile.canHitEntity` (plus the arrow's extra rule, which only concerns player-versus-
    /// player protection and is always satisfied here).
    fn can_hit_entity(&self, target: &dyn HitTarget) -> bool {
        target.can_be_hit_by_projectile() && (!self.owner_is_target || self.left_owner)
    }

    /// `Projectile.updateRotation`: ease the rotation 20% of the way to the velocity's direction.
    fn update_rotation(&mut self) {
        let v = self.vel;
        let d = v_horizontal_distance(v);
        self.pitch = lerp_rotation(self.x_rot_o, degrees(mth_atan2(v.y, d)));
        self.yaw = lerp_rotation(self.y_rot_o, degrees(mth_atan2(v.x, v.z)));
    }

    /// `ProjectileUtil.computeMargin`: how far the target's box is grown for the entity hit test;
    /// zero for the first two ticks, then growing to 0.3.
    fn margin(&self) -> f32 {
        java_max_f32(
            0.0,
            java_min_f32(0.3, (self.tick_count.wrapping_sub(2)) as f32 / 20.0),
        )
    }

    // ----- thrown items -----

    /// `ThrowableProjectile.tick`.
    fn tick_throwable(
        &mut self,
        world: &World,
        mut target: Option<&mut dyn HitTarget>,
        report: &mut TickReport,
    ) {
        if self.first_tick {
            self.handle_first_tick_bubble_column(world);
        }
        self.apply_gravity();
        // applyInertia: water drag from the previous tick's water check.
        let inertia = if self.was_touching_water {
            THROWABLE_WATER_INERTIA
        } else {
            AIR_INERTIA
        };
        self.vel = v_scale(self.vel, f64::from(inertia));

        let hit = self.hit_result_on_move_vector(world, target.as_deref());
        let new_pos = match hit {
            Hit::Miss => v_add(self.pos, self.vel),
            Hit::Block(b) => b.location,
            Hit::Target(p) => p,
        };
        self.set_pos(new_pos);
        self.update_rotation();
        self.apply_effects_from_blocks(world);
        // Projectile.tick -> Entity.tick
        self.check_left_owner(target.as_deref());
        self.base_tick(world);
        self.left_owner_checked = false;
        if hit != Hit::Miss && !self.removed {
            self.hit_target_or_deflect_self(world, hit, reborrow(&mut target), report);
        }
    }

    /// `ThrowableProjectile.handleFirstTickBubbleColumn`: a throwable created inside a bubble
    /// column feels it on its first tick (every cell of its box, `entityInside` with `bl = true`).
    fn handle_first_tick_bubble_column(&mut self, world: &World) {
        let bb = self.bb();
        let (x0, y0, z0) = block_pos_of(Vec3::new(bb.min_x, bb.min_y, bb.min_z));
        let (x1, y1, z1) = block_pos_of(Vec3::new(bb.max_x, bb.max_y, bb.max_z));
        let (x0, x1) = (x0.min(x1), x0.max(x1));
        let (y0, y1) = (y0.min(y1), y0.max(y1));
        let (z0, z1) = (z0.min(z1), z0.max(z1));
        for z in z0..=z1 {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let state = world.block_state(x, y, z);
                    if is_bubble_column(state) {
                        self.bubble_column_inside(world, (x, y, z), state, true);
                    }
                }
            }
        }
    }

    /// `ProjectileUtil.getHitResultOnMoveVector` with `ClipContext.Block.COLLIDER`: clip the
    /// block shapes along the velocity, then look for the target within the (shortened) segment.
    fn hit_result_on_move_vector(&self, world: &World, target: Option<&dyn HitTarget>) -> Hit {
        let delta = self.vel;
        let start = self.pos;
        let mut end = v_add(start, delta);
        let mut result = Hit::Miss;
        if let Some(b) = clip_blocks(world, start, end, self.pos.y) {
            end = b.location;
            result = Hit::Block(b);
        }
        if let Some(t) = target {
            if self.can_hit_entity(t) {
                let search = self.bb().expand_towards(delta).inflate(1.0);
                if let Some(p) = entity_hit_location(t, start, end, search, self.margin()) {
                    result = Hit::Target(p);
                }
            }
        }
        result
    }

    /// `Projectile.hitTargetOrDeflectSelf` followed by `onHit` and the throwables' override that
    /// discards the item.
    fn hit_target_or_deflect_self(
        &mut self,
        world: &World,
        hit: Hit,
        target: Option<&mut dyn HitTarget>,
        report: &mut TickReport,
    ) {
        match hit {
            Hit::Miss => {}
            Hit::Target(location) => {
                if let Some(t) = target {
                    self.on_hit_entity(location, t, report);
                }
            }
            Hit::Block(b) => self.on_hit_block(world, b, report),
        }
        if !self.kind.is_arrow() {
            // Snowball/Egg/ThrownEnderpearl.onHit: the item is spent (the pearl first moves its
            // owner; the egg may hatch a chicken, which is not simulated).
            if self.kind == ProjectileKind::EnderPearl && self.owner_is_target && !self.removed {
                report.owner_teleport = Some(self.old_pos);
            }
            self.removed = true;
        }
    }

    // ----- hitting things -----

    fn on_hit_entity(
        &mut self,
        location: Vec3,
        target: &mut dyn HitTarget,
        report: &mut TickReport,
    ) {
        if self.kind.is_arrow() {
            self.arrow_on_hit_entity(location, target, report);
            return;
        }
        // Snowball, egg and pearl hurt the entity for 0 damage; the knockback direction is
        // unused because 0 damage never lands.
        let hit = ProjectileHit {
            kind: self.kind,
            amount: 0.0,
            knockback_dx: -self.vel.x,
            knockback_dz: -self.vel.z,
        };
        let landed = target.hurt_by_projectile(&hit);
        report.impact = Some(Impact::Target {
            location,
            hit,
            landed,
        });
    }

    fn on_hit_block(&mut self, world: &World, b: BlockHit, report: &mut TickReport) {
        let _ = world;
        report.impact = Some(Impact::Block { hit: b });
        if self.kind.is_arrow() {
            self.arrow_on_hit_block(world, b);
        }
    }

    // ----- arrows -----

    /// `AbstractArrow.tick`.
    fn tick_arrow(
        &mut self,
        world: &World,
        mut target: Option<&mut dyn HitTarget>,
        report: &mut TickReport,
    ) {
        let physics = !self.no_physics;
        let vec3 = self.vel;
        let (bx, by, bz) = block_pos_of(self.pos);
        let state = world.block_state(bx, by, bz);
        if state != ms_data::AIR && physics {
            // An arrow found inside a collision box of the block it occupies is stuck in it.
            let p = self.pos;
            for b in ms_data::collision_boxes(state) {
                let moved = Bb::new(
                    b[0] + f64::from(bx),
                    b[1] + f64::from(by),
                    b[2] + f64::from(bz),
                    b[3] + f64::from(bx),
                    b[4] + f64::from(by),
                    b[5] + f64::from(bz),
                );
                if moved.contains(p) {
                    self.vel = Vec3::ZERO;
                    self.in_ground = true;
                    break;
                }
            }
        }
        if self.shake_time > 0 {
            self.shake_time -= 1;
        }
        if self.in_ground && physics {
            if self.last_state != Some(state) && self.should_fall(world) {
                self.start_falling();
            } else {
                self.tick_despawn();
            }
            self.in_ground_time = self.in_ground_time.wrapping_add(1);
            if !self.removed {
                self.apply_effects_from_blocks(world);
            }
            return;
        }

        self.in_ground_time = 0;
        let start = self.pos;
        if self.was_touching_water {
            // The drag lands on the velocity *after* `vec3` was captured: the step below uses the
            // undragged velocity.
            self.vel = v_scale(self.vel, f64::from(ARROW_WATER_INERTIA));
        }
        let f = if physics {
            degrees(mth_atan2(vec3.x, vec3.z))
        } else {
            degrees(mth_atan2(-vec3.x, -vec3.z))
        };
        let g = degrees(mth_atan2(vec3.y, v_horizontal_distance(vec3)));
        self.pitch = lerp_rotation(self.pitch, g);
        self.yaw = lerp_rotation(self.yaw, f);
        self.check_left_owner(target.as_deref());
        if physics {
            let end = v_add(start, vec3);
            let block_hit = clip_blocks(world, start, end, self.pos.y);
            self.step_move_and_hit(world, end, block_hit, reborrow(&mut target), report);
        } else {
            self.set_pos(v_add(start, vec3));
            self.apply_effects_from_blocks(world);
        }
        if !self.was_touching_water {
            self.vel = v_scale(self.vel, f64::from(AIR_INERTIA));
        }
        if physics && !self.in_ground {
            self.apply_gravity();
        }
        // Projectile.tick -> Entity.tick
        self.check_left_owner(target.as_deref());
        self.base_tick(world);
        self.left_owner_checked = false;
    }

    /// `AbstractArrow.shouldFall`: stuck, and nothing solid within 0.06 of the arrow's position.
    fn should_fall(&self, world: &World) -> bool {
        let p = self.pos;
        self.in_ground
            && no_block_collision(world, Bb::new(p.x, p.y, p.z, p.x, p.y, p.z).inflate(0.06))
    }

    /// `AbstractArrow.startFalling`: out of the block, slowed by three random factors.
    fn start_falling(&mut self) {
        self.in_ground = false;
        let a = self.rng.next_float() * 0.2_f32;
        let b = self.rng.next_float() * 0.2_f32;
        let c = self.rng.next_float() * 0.2_f32;
        self.vel = Vec3::new(
            self.vel.x * f64::from(a),
            self.vel.y * f64::from(b),
            self.vel.z * f64::from(c),
        );
        self.life = 0;
    }

    /// `AbstractArrow.tickDespawn`.
    fn tick_despawn(&mut self) {
        self.life += 1;
        if self.life >= ARROW_LIFE_LIMIT {
            self.removed = true;
        }
    }

    /// `AbstractArrow.stepMoveAndHit` for a single target and no piercing: move to the entity hit
    /// (the one the segment up to the block hit reaches) or else to the block hit or the end of the
    /// step, apply the block effects of the step, and hit what was reached.
    fn step_move_and_hit(
        &mut self,
        world: &World,
        end: Vec3,
        block_hit: Option<BlockHit>,
        target: Option<&mut dyn HitTarget>,
        report: &mut TickReport,
    ) {
        let from = self.pos;
        // `blockHitResult.getLocation()`; for a miss that is the end of the segment.
        let block_end = block_hit.map_or(end, |b| b.location);
        let entity_hit = match target.as_deref() {
            Some(t) if self.can_hit_entity(t) => {
                let search = self.bb().expand_towards(self.vel).inflate(1.0);
                self.find_hit_entity(world, t, from, block_end, search)
            }
            _ => None,
        };
        let reach = entity_hit.unwrap_or(block_end);
        self.set_pos(reach);
        self.apply_effects_from_blocks_between(world, from, reach);
        match entity_hit {
            None => {
                if let Some(b) = block_hit {
                    if !self.removed {
                        self.on_hit_block(world, b, report);
                    }
                }
            }
            Some(location) => {
                if !self.removed && !self.no_physics {
                    if let Some(t) = target {
                        self.on_hit_entity(location, t, report);
                    }
                }
            }
        }
    }

    /// `ProjectileUtil.getManyEntityHitResult` (without the "start inside" case) for one target:
    /// the point where the segment first meets the target's own box, or, when it only grazes the
    /// target's box grown by the margin, where the line from that grazing point to the target's
    /// centre (stopped by blocks) meets the real box.
    fn find_hit_entity(
        &self,
        world: &World,
        target: &dyn HitTarget,
        start: Vec3,
        end: Vec3,
        search: Bb,
    ) -> Option<Vec3> {
        let tb = Bb::from_aabb(target.hit_box());
        if !search.intersects(tb) {
            return None;
        }
        if let Some(p) = tb.clip(start, end) {
            return Some(p);
        }
        let margin = self.margin();
        if margin <= 0.0 {
            return None;
        }
        let grazed = tb.inflate(f64::from(margin)).clip(start, end)?;
        let mut aim = tb.center();
        if let Some(b) = clip_blocks(world, grazed, aim, self.pos.y) {
            aim = b.location;
        }
        tb.clip(grazed, aim)
    }

    /// `AbstractArrow.onHitBlock`: remember the block, back off from the hit point, stop and stick.
    fn arrow_on_hit_block(&mut self, world: &World, b: BlockHit) {
        self.last_state = Some(world.block_state(b.pos.0, b.pos.1, b.pos.2));
        let v = self.vel;
        let sign = Vec3::new(java_signum(v.x), java_signum(v.y), java_signum(v.z));
        let back = v_scale(sign, ARROW_BACK_OFF);
        self.set_pos(v_sub(self.pos, back));
        self.vel = Vec3::ZERO;
        // playSound(..., 1.2F / (random.nextFloat() * 0.2F + 0.9F)) draws one float.
        self.rng.next_float();
        self.in_ground = true;
        self.shake_time = SHAKE_TIME;
        self.crit = false;
    }

    /// `AbstractArrow.onHitEntity` for the target: damage `ceil(speed * baseDamage)` (plus the
    /// random critical bonus) and, when it lands, the arrow is spent; when it does not (the target
    /// is inside its invulnerability window) the arrow bounces off with 20% of the reversed
    /// half-speed velocity.
    fn arrow_on_hit_entity(
        &mut self,
        location: Vec3,
        target: &mut dyn HitTarget,
        report: &mut TickReport,
    ) {
        // (float) getDeltaMovement().length(), times the double base damage.
        let speed = v_length(self.vel) as f32;
        let d = self.base_damage;
        let mut i = mth_ceil(mth_clamp(f64::from(speed) * d, 0.0, 2.147483647E9));
        if self.crit {
            let l = i64::from(self.rng.next_int_bound(i / 2 + 2));
            i = (l + i64::from(i)).min(2147483647) as i32;
        }
        let hit = ProjectileHit {
            kind: self.kind,
            amount: i as f32,
            knockback_dx: -self.vel.x,
            knockback_dz: -self.vel.z,
        };
        let landed = target.hurt_by_projectile(&hit);
        report.impact = Some(Impact::Target {
            location,
            hit,
            landed,
        });
        if landed {
            // The hit sound draws a float; the arrow is discarded.
            self.rng.next_float();
            self.removed = true;
        } else {
            // deflect(REVERSE): turn around at half speed and spin the yaw by 170..190 degrees,
            // then slow to 20%; a nearly stopped arrow is discarded.
            let f = 170.0_f32 + self.rng.next_float() * 20.0_f32;
            self.vel = v_scale(self.vel, -0.5);
            self.yaw += f;
            self.y_rot_o += f;
            self.vel = v_scale(self.vel, 0.2);
            if v_length_sqr(self.vel) < 1.0E-7 {
                self.removed = true;
            }
        }
    }
}

/// `EntityDimensions.makeBoundingBox(pos)` for a projectile kind: a float half-width and height
/// widened to double.
fn bb_around(kind: ProjectileKind, pos: Vec3) -> Bb {
    let (w, h) = kind.dimensions();
    let g = f64::from(w / 2.0_f32);
    let h = f64::from(h);
    Bb::new(pos.x - g, pos.y, pos.z - g, pos.x + g, pos.y + h, pos.z + g)
}

/// Reborrow an optional target for a shorter lifetime (trait-object lifetimes are invariant behind
/// `&mut`, so `as_deref_mut` would pin the borrow to the whole function).
fn reborrow<'a>(t: &'a mut Option<&mut dyn HitTarget>) -> Option<&'a mut dyn HitTarget> {
    match t {
        Some(x) => Some(&mut **x),
        None => None,
    }
}

fn is_bubble_column(state: u32) -> bool {
    state != ms_data::AIR
        && ms_data::block_name(ms_data::block_of_state(state)) == "minecraft:bubble_column"
}

/// `ProjectileUtil.getEntityHitResult(level, entity, start, end, searchBox, predicate, margin)` for
/// one candidate: the point where the segment enters the target's box grown by `margin`.
fn entity_hit_location(
    target: &dyn HitTarget,
    start: Vec3,
    end: Vec3,
    search: Bb,
    margin: f32,
) -> Option<Vec3> {
    let tb = Bb::from_aabb(target.hit_box());
    if !search.intersects(tb) {
        return None;
    }
    tb.inflate(f64::from(margin)).clip(start, end)
}

/// `BlockGetter.forEachBlockIntersectedBetween`: visit the blocks a box intersects while moving
/// from `from` to `to`, as `(pos, step)`, in the game's order; `visit` returns whether to go on.
fn for_each_block_intersected_between(
    from: Vec3,
    to: Vec3,
    bb: Bb,
    visit: &mut dyn FnMut((i32, i32, i32), i32) -> bool,
) -> bool {
    let delta = v_sub(to, from);
    let tiny = 1.0E-5_f32 * 1.0E-5_f32;
    if v_length_sqr(delta) < f64::from(tiny) {
        let (x0, y0, z0) = block_pos_of(Vec3::new(bb.min_x, bb.min_y, bb.min_z));
        let (x1, y1, z1) = block_pos_of(Vec3::new(bb.max_x, bb.max_y, bb.max_z));
        for z in z0.min(z1)..=z0.max(z1) {
            for y in y0.min(y1)..=y0.max(y1) {
                for x in x0.min(x1)..=x0.max(x1) {
                    if !visit((x, y, z), 0) {
                        return false;
                    }
                }
            }
        }
        return true;
    }
    let mut seen: Vec<(i32, i32, i32)> = Vec::new();
    let back = Bb::new(
        bb.min_x + -delta.x,
        bb.min_y + -delta.y,
        bb.min_z + -delta.z,
        bb.max_x + -delta.x,
        bb.max_y + -delta.y,
        bb.max_z + -delta.z,
    );
    for pos in between_corners_in_direction_bb(back, delta) {
        if !visit(pos, 0) {
            return false;
        }
        seen.push(pos);
    }
    let steps = add_collisions_along_travel(&mut seen, delta, bb, visit);
    if steps < 0 {
        return false;
    }
    for pos in between_corners_in_direction_bb(bb, delta) {
        if !seen.contains(&pos) {
            seen.push(pos);
            if !visit(pos, steps + 1) {
                return false;
            }
        }
    }
    true
}

fn between_corners_in_direction_bb(bb: Bb, v: Vec3) -> Vec<(i32, i32, i32)> {
    let (i, j, k) = (
        mth_floor(bb.min_x),
        mth_floor(bb.min_y),
        mth_floor(bb.min_z),
    );
    let (l, m, n) = (
        mth_floor(bb.max_x),
        mth_floor(bb.max_y),
        mth_floor(bb.max_z),
    );
    between_corners_in_direction(i, j, k, l, m, n, v)
}

/// `BlockPos.betweenCornersInDirection`: the cells of the box between two corners, starting at
/// the corner opposite to the direction of travel and sweeping along the dominant axes first.
fn between_corners_in_direction(
    i: i32,
    j: i32,
    k: i32,
    l: i32,
    m: i32,
    n: i32,
    v: Vec3,
) -> Vec<(i32, i32, i32)> {
    let (o, p, q) = (i.min(l), j.min(m), k.min(n));
    let (r, s, t) = (i.max(l), j.max(m), k.max(n));
    let (u, vv, w) = (r - o, s - p, t - q);
    let x = if v.x >= 0.0 { o } else { r };
    let y = if v.y >= 0.0 { p } else { s };
    let z = if v.z >= 0.0 { q } else { t };
    // Direction.axisStepOrder: Y, then the horizontal axis with the larger movement (X on ties).
    let order: [usize; 3] = if v.x.abs() < v.z.abs() {
        [1, 2, 0]
    } else {
        [1, 0, 2]
    };
    let comp = |a: usize| [v.x, v.y, v.z][a];
    let extent = |a: usize| [u, vv, w][a];
    let step = |a: usize| -> (i32, i32, i32) {
        let sgn = if comp(a) >= 0.0 { 1 } else { -1 };
        match a {
            0 => (sgn, 0, 0),
            1 => (0, sgn, 0),
            _ => (0, 0, sgn),
        }
    };
    let (d1, d2, d3) = (step(order[0]), step(order[1]), step(order[2]));
    let (aa, ab, ac) = (extent(order[0]), extent(order[1]), extent(order[2]));
    let mut out = Vec::new();
    for fi in 0..=aa {
        for si in 0..=ab {
            for ti in 0..=ac {
                out.push((
                    x + d1.0 * fi + d2.0 * si + d3.0 * ti,
                    y + d1.1 * fi + d2.1 * si + d3.1 * ti,
                    z + d1.2 * fi + d2.2 * si + d3.2 * ti,
                ));
            }
        }
    }
    out
}

/// `BlockGetter.getFurthestCorner`.
fn furthest_corner(v: Vec3) -> (i32, i32, i32) {
    let d = v.x.abs();
    let e = v.y.abs();
    let f = v.z.abs();
    let i = if v.x >= 0.0 { 1 } else { -1 };
    let j = if v.y >= 0.0 { 1 } else { -1 };
    let k = if v.z >= 0.0 { 1 } else { -1 };
    if d <= e && d <= f {
        (-i, -k, j)
    } else if e <= f {
        (k, -j, -i)
    } else {
        (-j, i, -k)
    }
}

/// `BlockGetter.addCollisionsAlongTravel`: the cells the box's leading corner crosses on the way,
/// each expanded back to the box's extent. Returns the number of crossings, or -1 if the visitor
/// stopped the walk.
fn add_collisions_along_travel(
    seen: &mut Vec<(i32, i32, i32)>,
    v: Vec3,
    bb: Bb,
    visit: &mut dyn FnMut((i32, i32, i32), i32) -> bool,
) -> i32 {
    let d = bb.max_x - bb.min_x;
    let e = bb.max_y - bb.min_y;
    let f = bb.max_z - bb.min_z;
    let corner = furthest_corner(v);
    let center = bb.center();
    let leading = Vec3::new(
        center.x + d * 0.5 * f64::from(corner.0),
        center.y + e * 0.5 * f64::from(corner.1),
        center.z + f * 0.5 * f64::from(corner.2),
    );
    let rel = v_sub(leading, v);
    let mut i = mth_floor(rel.x);
    let mut j = mth_floor(rel.y);
    let mut k = mth_floor(rel.z);
    let l = mth_sign(v.x);
    let m = mth_sign(v.y);
    let n = mth_sign(v.z);
    let g = if l == 0 { f64::MAX } else { f64::from(l) / v.x };
    let h = if m == 0 { f64::MAX } else { f64::from(m) / v.y };
    let o = if n == 0 { f64::MAX } else { f64::from(n) / v.z };
    let mut p = g * if l > 0 {
        1.0 - mth_frac(rel.x)
    } else {
        mth_frac(rel.x)
    };
    let mut q = h * if m > 0 {
        1.0 - mth_frac(rel.y)
    } else {
        mth_frac(rel.y)
    };
    let mut r = o * if n > 0 {
        1.0 - mth_frac(rel.z)
    } else {
        mth_frac(rel.z)
    };
    let mut s = 0;
    while p <= 1.0 || q <= 1.0 || r <= 1.0 {
        if p < q {
            if p < r {
                i = i.wrapping_add(l);
                p += g;
            } else {
                k = k.wrapping_add(n);
                r += o;
            }
        } else if q < r {
            j = j.wrapping_add(m);
            q += h;
        } else {
            k = k.wrapping_add(n);
            r += o;
        }
        let cell = Bb::new(
            f64::from(i),
            f64::from(j),
            f64::from(k),
            f64::from(i) + 1.0,
            f64::from(j) + 1.0,
            f64::from(k) + 1.0,
        );
        if let Some(hit) = cell.clip(rel, leading) {
            s += 1;
            let slack = f64::from(1.0E-5_f32);
            // `i + 1.0E-5F` is an int plus a float: a float addition, widened afterwards.
            let low = |c: i32| f64::from(c as f32 + 1.0E-5_f32);
            let high = |c: i32| f64::from(c) + 1.0 - slack;
            let t = mth_clamp(hit.x, low(i), high(i));
            let u = mth_clamp(hit.y, low(j), high(j));
            let w = mth_clamp(hit.z, low(k), high(k));
            let wx = mth_floor(t - d * f64::from(corner.0));
            let wy = mth_floor(u - e * f64::from(corner.1));
            let wz = mth_floor(w - f * f64::from(corner.2));
            for pos in between_corners_in_direction(i, j, k, wx, wy, wz, v) {
                if !seen.contains(&pos) {
                    seen.push(pos);
                    if !visit(pos, s) {
                        return -1;
                    }
                }
            }
        }
    }
    s
}
