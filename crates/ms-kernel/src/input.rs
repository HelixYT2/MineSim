//! The local player's input handling: how the seven keys become the movement impulse
//! (`KeyboardInput.tick`, `LocalPlayer.modifyInput`), and the "is this wall hit nearly head-on"
//! test (`LocalPlayer.isHorizontalCollisionMinor`) that decides whether sprinting survives a bump.
//!
//! All arithmetic here is `float` in the game (`Vec2`), so it is `f32` here, with the reciprocal
//! multiplications and the `Mth.sqrt` double round trip kept exactly as they are.

// The comparisons mirror the reference's `!(a <= b)` forms, which differ from `a > b` for NaN.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::state::Input;

/// `(float)(Math.PI / 180.0)`.
pub const DEG_TO_RAD: f32 = (std::f64::consts::PI / 180.0) as f32;

/// `Mth.sqrt(float)`: `(float) Math.sqrt(f)` (the argument is widened, the root is exact in
/// double, then rounded to float).
#[inline]
pub fn mth_sqrt(f: f32) -> f32 {
    f64::from(f).sqrt() as f32
}

/// `KeyboardInput.calculateImpulse`.
#[inline]
fn calculate_impulse(positive: bool, negative: bool) -> f32 {
    if positive == negative {
        0.0
    } else if positive {
        1.0
    } else {
        -1.0
    }
}

/// `KeyboardInput.tick`'s `moveVector`: `new Vec2(left - right, forward - back).normalized()`,
/// as `(x = strafe, y = forward)`.
pub fn keyboard_move_vector(input: &Input) -> (f32, f32) {
    let f = calculate_impulse(input.forward, input.back);
    let g = calculate_impulse(input.left, input.right);
    // Vec2.normalized
    let len = mth_sqrt(g * g + f * f);
    if len < 1.0E-4_f32 {
        (0.0, 0.0)
    } else {
        (g / len, f / len)
    }
}

/// `ClientInput.hasForwardImpulse`.
#[inline]
pub fn has_forward_impulse(move_vector: (f32, f32)) -> bool {
    move_vector.1 > 1.0E-5_f32
}

/// `LocalPlayer.modifyInput` (without the using-item slowdown, which the simulation does not
/// model): scale by 0.98, by the sneaking-speed attribute when moving slowly, then the
/// square-movement correction. Returns `(xxa, zza)`.
pub fn modify_input(
    move_vector: (f32, f32),
    moving_slowly: bool,
    sneaking_speed: f32,
) -> (f32, f32) {
    let (x, y) = move_vector;
    if x * x + y * y == 0.0 {
        return move_vector;
    }
    let (mut x, mut y) = (x * 0.98_f32, y * 0.98_f32);
    if moving_slowly {
        x *= sneaking_speed;
        y *= sneaking_speed;
    }
    modify_input_speed_for_square_movement(x, y)
}

/// `LocalPlayer.modifyInputSpeedForSquareMovement`.
fn modify_input_speed_for_square_movement(x: f32, y: f32) -> (f32, f32) {
    let f = mth_sqrt(x * x + y * y);
    if f <= 0.0 {
        return (x, y);
    }
    // Vec2.scale(1.0F / f): a float reciprocal first, then a multiply.
    let inv = 1.0_f32 / f;
    let (ux, uy) = (x * inv, y * inv);
    let g = distance_to_unit_square(ux, uy);
    let h = java_min_f32(f * g, 1.0);
    (ux * h, uy * h)
}

/// `LocalPlayer.distanceToUnitSquare`.
fn distance_to_unit_square(x: f32, y: f32) -> f32 {
    let f = x.abs();
    let g = y.abs();
    let h = if g > f { f / g } else { g / f };
    mth_sqrt(1.0 + h * h)
}

/// `Math.min(float, float)`.
fn java_min_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 && b.to_bits() == (-0.0_f32).to_bits() {
        return b;
    }
    if a <= b {
        a
    } else {
        b
    }
}

/// `LocalPlayer.isHorizontalCollisionMinor`: whether the player's intended direction (its input
/// rotated by yaw) is within 8 degrees of the direction it actually moved this tick (`moved` is the
/// collided movement). `xxa`/`zza` are the current movement input.
pub fn is_horizontal_collision_minor(
    yaw: f32,
    xxa: f32,
    zza: f32,
    moved_x: f64,
    moved_z: f64,
) -> bool {
    let f = yaw * DEG_TO_RAD;
    let d = f64::from(crate::mth::sin(f64::from(f)));
    let e = f64::from(crate::mth::cos(f64::from(f)));
    let g = f64::from(xxa) * e - f64::from(zza) * d;
    let h = f64::from(zza) * e + f64::from(xxa) * d;
    let i = g * g + h * h;
    let j = moved_x * moved_x + moved_z * moved_z;
    let limit = f64::from(1.0E-5_f32);
    if !(i < limit) && !(j < limit) {
        let k = g * moved_x + h * moved_z;
        let l = acos(k / (i * j).sqrt());
        l < f64::from(0.139_626_34_f32)
    } else {
        false
    }
}

// ---------------------------------------------------------------------------------------------
// fdlibm acos (`StrictMath.acos`, which is what `Math.acos` runs on HotSpot)
// ---------------------------------------------------------------------------------------------

pub use fdlibm::acos;

/// The fdlibm constants are kept at the precision the library publishes them.
#[allow(clippy::excessive_precision, clippy::approx_constant)]
mod fdlibm {
    const PIO2_HI: f64 = 1.570_796_326_794_896_558_00e+00;
    const PIO2_LO: f64 = 6.123_233_995_736_766_035_87e-17;
    const PI: f64 = 3.141_592_653_589_793_116_00e+00;
    const P_S0: f64 = 1.666_666_666_666_666_574_15e-01;
    const P_S1: f64 = -3.255_658_186_224_009_154_05e-01;
    const P_S2: f64 = 2.012_125_321_348_629_258_81e-01;
    const P_S3: f64 = -4.005_553_450_067_941_140_27e-02;
    const P_S4: f64 = 7.915_349_942_898_145_321_76e-04;
    const P_S5: f64 = 3.479_331_075_960_211_675_70e-05;
    const Q_S1: f64 = -2.403_394_911_734_414_218_78e+00;
    const Q_S2: f64 = 2.020_945_760_233_505_694_71e+00;
    const Q_S3: f64 = -6.882_839_716_054_532_930_30e-01;
    const Q_S4: f64 = 7.703_815_055_590_193_527_91e-02;

    /// fdlibm `__ieee754_acos`: a polynomial ratio for |x| < 0.5, with a square-root reduction
    /// outside that; NaN for |x| > 1.
    pub fn acos(x: f64) -> f64 {
        let bits = x.to_bits();
        let hx = (bits >> 32) as u32 as i32;
        let lo = bits as u32;
        let ix = hx & 0x7fff_ffff;
        if ix >= 0x3ff0_0000 {
            // |x| >= 1
            if ((ix - 0x3ff0_0000) as u32 | lo) == 0 {
                // |x| == 1
                return if hx > 0 { 0.0 } else { PI + 2.0 * PIO2_LO };
            }
            return f64::NAN; // |x| > 1, or NaN
        }
        let poly_p =
            |z: f64| z * (P_S0 + z * (P_S1 + z * (P_S2 + z * (P_S3 + z * (P_S4 + z * P_S5)))));
        let poly_q = |z: f64| 1.0 + z * (Q_S1 + z * (Q_S2 + z * (Q_S3 + z * Q_S4)));
        if ix < 0x3fe0_0000 {
            // |x| < 0.5
            if ix <= 0x3c60_0000 {
                return PIO2_HI + PIO2_LO; // |x| < 2**-57
            }
            let z = x * x;
            let p = poly_p(z);
            let q = poly_q(z);
            let r = p / q;
            PIO2_HI - (x - (PIO2_LO - x * r))
        } else if hx < 0 {
            // x < -0.5
            let z = (1.0 + x) * 0.5;
            let p = poly_p(z);
            let q = poly_q(z);
            let s = z.sqrt();
            let r = p / q;
            let w = r * s - PIO2_LO;
            PI - 2.0 * (s + w)
        } else {
            // x > 0.5
            let z = (1.0 - x) * 0.5;
            let s = z.sqrt();
            let df = f64::from_bits(s.to_bits() & 0xffff_ffff_0000_0000);
            let c = (z - df * df) / (s + df);
            let p = poly_p(z);
            let q = poly_q(z);
            let r = p / q;
            let w = r * s + c;
            2.0 * (df + w)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acos_matches_the_jvm_bit_for_bit() {
        let csv = include_str!("../testdata/acos_reference.csv");
        let mut n = 0;
        for line in csv.lines().skip(1) {
            let (a, r) = line.split_once(',').unwrap();
            let x = f64::from_bits(a.parse::<i64>().unwrap() as u64);
            let want = f64::from_bits(r.parse::<i64>().unwrap() as u64);
            let got = acos(x);
            if want.is_nan() {
                assert!(got.is_nan(), "acos({x:e})");
            } else {
                assert_eq!(got.to_bits(), want.to_bits(), "acos({x:e})");
            }
            n += 1;
        }
        assert!(n > 3000);
    }

    #[test]
    fn move_vector_is_normalised() {
        let mut i = Input::default();
        assert_eq!(keyboard_move_vector(&i), (0.0, 0.0));
        i.forward = true;
        assert_eq!(keyboard_move_vector(&i), (0.0, 1.0));
        i.left = true;
        let (x, y) = keyboard_move_vector(&i);
        assert_eq!(x, 0.707_106_77_f32);
        assert_eq!(y, 0.707_106_77_f32);
        i.back = true; // forward and back cancel
        assert_eq!(keyboard_move_vector(&i), (1.0, 0.0));
    }

    #[test]
    fn modify_input_scales_and_squares() {
        // Straight forward: 0.98 of the input.
        let (x, z) = modify_input((0.0, 1.0), false, 0.3);
        assert_eq!(x, 0.0);
        assert!((z - 0.98).abs() < 1e-6);
        // Diagonal reaches the edge of the unit square: length 0.98 * sqrt(2) clamps to ... 1.0.
        let v = keyboard_move_vector(&Input {
            forward: true,
            left: true,
            ..Input::default()
        });
        let (x, z) = modify_input(v, false, 0.3);
        assert!(x > 0.69 && z > 0.69);
        // Sneaking slows it.
        let (_, z) = modify_input((0.0, 1.0), true, 0.3);
        assert!(z < 0.3);
    }
}
