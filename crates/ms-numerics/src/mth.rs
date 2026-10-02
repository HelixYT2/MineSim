//! The game's `net.minecraft.util.Mth`: lookup-table trigonometry and the small numeric helpers
//! the simulation uses, with Java's exact casting, rounding and NaN semantics.
//!
//! Rather than calling `Math.sin` per tick, the game indexes a 65536-entry table of
//! `(float) Math.sin(i / 10430.378350470453)` values. Matching movement therefore means matching
//! that exact table, so the tables here are dumped from the real `Mth` class by
//! `tools/refgen/GameGen.java` and embedded verbatim (`data/`). `Mth.sin` / `Mth.cos` take a
//! `double` and index with `(int)((long)(d * 10430.378350470453 [+ 16384.0]) & 65535)`; callers
//! that have a `float` simply widen it first (exactly what Java does), which `sin`/`cos` here do
//! for any argument convertible to `f64`.
//!
//! Every function documents the Java overload it mirrors. Integer arithmetic wraps like Java's,
//! float-to-int conversions saturate (NaN becomes 0), and `Math.min`/`Math.max` keep their NaN and
//! signed-zero rules, which differ from Rust's `f32::min`/`f32::max`.

const SIN_TABLE: &[u8] = include_bytes!("../data/mth_sin_table.bin");
/// `ASIN_TAB` followed by `COS_TAB` (257 little-endian doubles each), used by [`atan2`].
const ATAN_TABLE: &[u8] = include_bytes!("../data/mth_atan_tables.bin");

const _: () = assert!(SIN_TABLE.len() == 65536 * 4);
const _: () = assert!(ATAN_TABLE.len() == 2 * 257 * 8);

/// `Mth.PI` = `(float) Math.PI`.
pub const PI: f32 = std::f32::consts::PI;
/// `Mth.HALF_PI` = `(float) (Math.PI / 2)`.
pub const HALF_PI: f32 = std::f32::consts::FRAC_PI_2;
/// `Mth.TWO_PI` = `(float) (Math.PI * 2)`.
pub const TWO_PI: f32 = (std::f64::consts::PI * 2.0) as f32;
/// `Mth.DEG_TO_RAD` = `(float) (Math.PI / 180.0)`.
pub const DEG_TO_RAD: f32 = (std::f64::consts::PI / 180.0) as f32;
/// `Mth.RAD_TO_DEG` = `180.0F / (float) Math.PI`.
pub const RAD_TO_DEG: f32 = 180.0_f32 / PI;
/// `Mth.EPSILON`.
pub const EPSILON: f32 = 1.0e-5;

const SIN_SCALE: f64 = 10430.378350470453;
const COS_OFFSET: f64 = 16384.0;

fn sin_entry(index: usize) -> f32 {
    let i = index * 4;
    f32::from_le_bytes([
        SIN_TABLE[i],
        SIN_TABLE[i + 1],
        SIN_TABLE[i + 2],
        SIN_TABLE[i + 3],
    ])
}

fn atan_entry(table: usize, index: usize) -> f64 {
    // Java would throw ArrayIndexOutOfBounds; never fall through into the neighbouring table.
    assert!(index < 257, "Mth.atan2 table index {index} out of range");
    let i = (table * 257 + index) * 8;
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&ATAN_TABLE[i..i + 8]);
    f64::from_le_bytes(bytes)
}

/// `Mth.sin(double)`. A `f32` argument is widened to `f64` first, like a Java `float` argument.
pub fn sin(value: impl Into<f64>) -> f32 {
    let d: f64 = value.into();
    // `(long)(d * scale) & 65535`: the cast saturates and maps NaN to 0, as Rust's `as` does.
    sin_entry(((d * SIN_SCALE) as i64 & 0xffff) as usize)
}

/// `Mth.cos(double)`. A `f32` argument is widened to `f64` first, like a Java `float` argument.
pub fn cos(value: impl Into<f64>) -> f32 {
    let d: f64 = value.into();
    sin_entry(((d * SIN_SCALE + COS_OFFSET) as i64 & 0xffff) as usize)
}

/// `Mth.sqrt(float)` = `(float) Math.sqrt(f)`.
pub fn sqrt_f32(x: f32) -> f32 {
    f64::from(x).sqrt() as f32
}

// ---------------------------------------------------------------------------------------------
// Math.min / Math.max with Java's NaN and signed-zero rules
// ---------------------------------------------------------------------------------------------

/// `Math.min(float, float)`: NaN if either is NaN, and `-0.0` is smaller than `0.0`.
pub fn min_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 && b.is_sign_negative() {
        return b;
    }
    if a <= b {
        a
    } else {
        b
    }
}

/// `Math.max(float, float)`: NaN if either is NaN, and `0.0` is larger than `-0.0`.
pub fn max_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 && a.is_sign_negative() {
        return b;
    }
    if a >= b {
        a
    } else {
        b
    }
}

/// `Math.min(double, double)`.
pub fn min_f64(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 && b.is_sign_negative() {
        return b;
    }
    if a <= b {
        a
    } else {
        b
    }
}

/// `Math.max(double, double)`.
pub fn max_f64(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 && a.is_sign_negative() {
        return b;
    }
    if a >= b {
        a
    } else {
        b
    }
}

// ---------------------------------------------------------------------------------------------
// floor / ceil / clamp
// ---------------------------------------------------------------------------------------------

/// `Mth.floor(double)`: `(int) d`, minus one when `d` is below that integer.
///
/// This is *not* `(int) Math.floor(d)`: for `d < -2^31` the truncating cast saturates at
/// `i32::MIN`, `d < i` holds, and the `i - 1` wraps to `i32::MAX`.
pub fn floor(d: f64) -> i32 {
    let i = d as i32;
    if d < f64::from(i) {
        i.wrapping_sub(1)
    } else {
        i
    }
}

/// `Mth.floor(float)`.
pub fn floor_f32(f: f32) -> i32 {
    let i = f as i32;
    if f < i as f32 {
        i.wrapping_sub(1)
    } else {
        i
    }
}

/// `Mth.lfloor(double)`.
pub fn lfloor(d: f64) -> i64 {
    let l = d as i64;
    if d < l as f64 {
        l.wrapping_sub(1)
    } else {
        l
    }
}

/// `Mth.ceil(double)`.
pub fn ceil(d: f64) -> i32 {
    let i = d as i32;
    if d > f64::from(i) {
        i.wrapping_add(1)
    } else {
        i
    }
}

/// `Mth.ceil(float)`.
pub fn ceil_f32(f: f32) -> i32 {
    let i = f as i32;
    if f > i as f32 {
        i.wrapping_add(1)
    } else {
        i
    }
}

/// `Mth.ceilLong(double)`.
pub fn ceil_long(d: f64) -> i64 {
    let l = d as i64;
    if d > l as f64 {
        l.wrapping_add(1)
    } else {
        l
    }
}

/// Types with a `Mth.clamp` overload.
pub trait Clamp: Copy {
    /// `Mth.clamp(self, min, max)`.
    fn clamp_java(self, min: Self, max: Self) -> Self;
}

impl Clamp for i32 {
    fn clamp_java(self, min: i32, max: i32) -> i32 {
        self.max(min).min(max)
    }
}

impl Clamp for i64 {
    fn clamp_java(self, min: i64, max: i64) -> i64 {
        self.max(min).min(max)
    }
}

impl Clamp for f32 {
    fn clamp_java(self, min: f32, max: f32) -> f32 {
        if self < min {
            min
        } else {
            min_f32(self, max)
        }
    }
}

impl Clamp for f64 {
    fn clamp_java(self, min: f64, max: f64) -> f64 {
        if self < min {
            min
        } else {
            min_f64(self, max)
        }
    }
}

/// `Mth.clamp` for `int`, `long`, `float` and `double`. Unlike `Ord::clamp`/`f32::clamp` this
/// never panics: `min > max` and NaN follow the game's formulas (`Math.min(Math.max(v, lo), hi)`
/// for integers, `v < lo ? lo : Math.min(v, hi)` for floating point).
pub fn clamp<T: Clamp>(value: T, min: T, max: T) -> T {
    value.clamp_java(min, max)
}

// ---------------------------------------------------------------------------------------------
// Angles
// ---------------------------------------------------------------------------------------------

/// `Mth.wrapDegrees(float)`: brings an angle into `[-180, 180)`.
pub fn wrap_degrees(f: f32) -> f32 {
    let mut g = f % 360.0;
    if g >= 180.0 {
        g -= 360.0;
    }
    if g < -180.0 {
        g += 360.0;
    }
    g
}

/// `Mth.wrapDegrees(double)`.
pub fn wrap_degrees_f64(d: f64) -> f64 {
    let mut e = d % 360.0;
    if e >= 180.0 {
        e -= 360.0;
    }
    if e < -180.0 {
        e += 360.0;
    }
    e
}

/// `Mth.wrapDegrees(int)`.
pub fn wrap_degrees_i32(i: i32) -> i32 {
    let mut j = i % 360;
    if j >= 180 {
        j -= 360;
    }
    if j < -180 {
        j += 360;
    }
    j
}

/// `Mth.wrapDegrees(long)`, which yields a `float`.
pub fn wrap_degrees_i64(l: i64) -> f32 {
    let mut f = (l % 360) as f32;
    if f >= 180.0 {
        f -= 360.0;
    }
    if f < -180.0 {
        f += 360.0;
    }
    f
}

/// `Mth.degreesDifference(float from, float to)` = `wrapDegrees(to - from)`.
pub fn degrees_difference(from: f32, to: f32) -> f32 {
    wrap_degrees(to - from)
}

/// `Mth.rotLerp(float, float, float)`.
pub fn rot_lerp(delta: f32, start: f32, end: f32) -> f32 {
    start + delta * wrap_degrees(end - start)
}

/// `Mth.rotLerp(double, double, double)`.
pub fn rot_lerp_f64(delta: f64, start: f64, end: f64) -> f64 {
    start + delta * wrap_degrees_f64(end - start)
}

/// `Mth.approach(float, float, float)`.
pub fn approach(current: f32, target: f32, step: f32) -> f32 {
    let step = step.abs();
    if current < target {
        clamp(current + step, current, target)
    } else {
        clamp(current - step, target, current)
    }
}

/// `Mth.approachDegrees(float, float, float)`.
pub fn approach_degrees(current: f32, target: f32, step: f32) -> f32 {
    let diff = degrees_difference(current, target);
    approach(current, current + diff, step)
}

// ---------------------------------------------------------------------------------------------
// Interpolation and misc
// ---------------------------------------------------------------------------------------------

/// `Mth.lerp(float delta, float start, float end)`.
pub fn lerp(delta: f32, start: f32, end: f32) -> f32 {
    start + delta * (end - start)
}

/// `Mth.lerp(double delta, double start, double end)`.
pub fn lerp_f64(delta: f64, start: f64, end: f64) -> f64 {
    start + delta * (end - start)
}

/// `Mth.inverseLerp(float, float, float)`.
pub fn inverse_lerp(value: f32, start: f32, end: f32) -> f32 {
    (value - start) / (end - start)
}

/// `Mth.inverseLerp(double, double, double)`.
pub fn inverse_lerp_f64(value: f64, start: f64, end: f64) -> f64 {
    (value - start) / (end - start)
}

/// `Mth.frac(float)` = `f - floor(f)`.
pub fn frac_f32(f: f32) -> f32 {
    f - floor_f32(f) as f32
}

/// `Mth.frac(double)` = `d - lfloor(d)`.
pub fn frac(d: f64) -> f64 {
    d - lfloor(d) as f64
}

/// `Mth.positiveModulo(float, float)`.
pub fn positive_modulo_f32(x: f32, y: f32) -> f32 {
    (x % y + y) % y
}

/// `Mth.positiveModulo(double, double)`.
pub fn positive_modulo_f64(x: f64, y: f64) -> f64 {
    (x % y + y) % y
}

/// `Mth.positiveModulo(int, int)` = `Math.floorMod(int, int)`.
pub fn positive_modulo_i32(x: i32, y: i32) -> i32 {
    let m = x.wrapping_rem(y);
    if m != 0 && (m ^ y) < 0 {
        m.wrapping_add(y)
    } else {
        m
    }
}

/// `Mth.fastInvSqrt(double)`: the bit-hack reciprocal square root with one Newton step.
pub fn fast_inv_sqrt(d: f64) -> f64 {
    let half = 0.5 * d;
    let bits = d.to_bits() as i64;
    let guess = f64::from_bits(6910469410427058090_i64.wrapping_sub(bits >> 1) as u64);
    guess * (1.5 - half * guess * guess)
}

const FRAC_BIAS: f64 = 17592186044416.0; // Double.longBitsToDouble(4805340802404319232L) = 2^44

/// `Mth.atan2(double y, double x)`: the game's table-driven approximation (not `Math.atan2`).
/// Projectiles and mobs use it to turn velocities into rotations.
///
/// The `ASIN_TAB`/`COS_TAB` tables are embedded as built by the game on x86_64 HotSpot. They are
/// computed with `Math.asin`/`Math.cos`, and 10 of the 257 `COS_TAB` entries differ between
/// HotSpot's `Math.cos` stub and `StrictMath.cos`, so a JVM on another architecture could build
/// a (very slightly) different table.
pub fn atan2(y: f64, x: f64) -> f64 {
    let (mut d, mut e) = (y, x);
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
    // `(int) Double.doubleToRawLongBits(h)`: the low word selects the table row.
    let index = h.to_bits() as u32 as i32 as usize;
    let asin = atan_entry(0, index);
    let cos = atan_entry(1, index);
    let l = h - FRAC_BIAS;
    let m = d * cos - e * l;
    let n = (6.0 + m * m) * m * 0.16666666666666666;
    let mut o = asin + n;
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

/// `Mth.getSeed(int x, int y, int z)`: the position hash behind the legacy positional random
/// factories.
pub fn get_seed(x: i32, y: i32, z: i32) -> i64 {
    let l =
        i64::from(x.wrapping_mul(3129871)) ^ i64::from(z).wrapping_mul(116129781) ^ i64::from(y);
    l.wrapping_mul(l)
        .wrapping_mul(42317861)
        .wrapping_add(l.wrapping_mul(11))
        >> 16
}
