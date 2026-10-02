//! Bit-for-bit checks of `ms_numerics::mth` against the real `net.minecraft.util.Mth` class.
//!
//! The vectors (`testdata/mth_reference.csv`, columns `op,a,b,c,result`, all raw bits) come from
//! `tools/refgen/GameGen.java`, which calls the game's own methods. See `tools/refgen/README.md`.

use ms_numerics::mth;
use std::collections::BTreeMap;

fn f32b(bits: u64) -> f32 {
    f32::from_bits(bits as u32)
}

fn f64b(bits: u64) -> f64 {
    f64::from_bits(bits)
}

/// Floats compare bit-for-bit, except that any NaN equals any NaN.
fn same_f32(got: f32, want: u64) -> bool {
    let want = f32b(want);
    (got.is_nan() && want.is_nan()) || got.to_bits() == want.to_bits()
}

fn same_f64(got: f64, want: u64) -> bool {
    let want = f64b(want);
    (got.is_nan() && want.is_nan()) || got.to_bits() == want.to_bits()
}

#[test]
fn matches_game_mth() {
    let csv = include_str!("../testdata/mth_reference.csv");
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (n, line) in csv.lines().enumerate().skip(1) {
        let mut cols = line.split(',');
        let op = cols.next().unwrap();
        let a: u64 = cols.next().unwrap().parse().unwrap();
        let b: u64 = cols.next().unwrap().parse().unwrap();
        let c: u64 = cols.next().unwrap().parse().unwrap();
        let want: u64 = cols.next().unwrap().parse().unwrap();

        let ok = match op {
            // sin/cos: float arguments are widened to double exactly as Java does
            "sin_f" => same_f32(mth::sin(f32b(a)), want),
            "cos_f" => same_f32(mth::cos(f32b(a)), want),
            "sin_d" => same_f32(mth::sin(f64b(a)), want),
            "cos_d" => same_f32(mth::cos(f64b(a)), want),
            "sqrt_f" => same_f32(mth::sqrt_f32(f32b(a)), want),
            "floor_d" => mth::floor(f64b(a)) as u32 as u64 == want,
            "floor_f" => mth::floor_f32(f32b(a)) as u32 as u64 == want,
            "ceil_d" => mth::ceil(f64b(a)) as u32 as u64 == want,
            "ceil_f" => mth::ceil_f32(f32b(a)) as u32 as u64 == want,
            "lfloor" => mth::lfloor(f64b(a)) as u64 == want,
            "ceil_long" => mth::ceil_long(f64b(a)) as u64 == want,
            "frac_d" => same_f64(mth::frac(f64b(a)), want),
            "frac_f" => same_f32(mth::frac_f32(f32b(a)), want),
            "wrap_d" => same_f64(mth::wrap_degrees_f64(f64b(a)), want),
            "wrap_f" => same_f32(mth::wrap_degrees(f32b(a)), want),
            "wrap_i" => mth::wrap_degrees_i32(a as u32 as i32) as u32 as u64 == want,
            "wrap_l" => same_f32(mth::wrap_degrees_i64(a as i64), want),
            "min_f" => same_f32(mth::min_f32(f32b(a), f32b(b)), want),
            "max_f" => same_f32(mth::max_f32(f32b(a), f32b(b)), want),
            "min_d" => same_f64(mth::min_f64(f64b(a), f64b(b)), want),
            "max_d" => same_f64(mth::max_f64(f64b(a), f64b(b)), want),
            "clamp_f" => same_f32(mth::clamp(f32b(a), f32b(b), f32b(c)), want),
            "clamp_d" => same_f64(mth::clamp(f64b(a), f64b(b), f64b(c)), want),
            "clamp_i" => {
                mth::clamp(a as u32 as i32, b as u32 as i32, c as u32 as i32) as u32 as u64 == want
            }
            "clamp_l" => mth::clamp(a as i64, b as i64, c as i64) as u64 == want,
            "deg_diff" => same_f32(mth::degrees_difference(f32b(a), f32b(b)), want),
            "rot_lerp_f" => same_f32(mth::rot_lerp(f32b(a), f32b(b), f32b(c)), want),
            "rot_lerp_d" => same_f64(mth::rot_lerp_f64(f64b(a), f64b(b), f64b(c)), want),
            "approach" => same_f32(mth::approach(f32b(a), f32b(b), f32b(c)), want),
            "approach_deg" => same_f32(mth::approach_degrees(f32b(a), f32b(b), f32b(c)), want),
            "lerp_f" => same_f32(mth::lerp(f32b(a), f32b(b), f32b(c)), want),
            "lerp_d" => same_f64(mth::lerp_f64(f64b(a), f64b(b), f64b(c)), want),
            "inv_lerp_f" => same_f32(mth::inverse_lerp(f32b(a), f32b(b), f32b(c)), want),
            "inv_lerp_d" => same_f64(mth::inverse_lerp_f64(f64b(a), f64b(b), f64b(c)), want),
            "pmod_f" => same_f32(mth::positive_modulo_f32(f32b(a), f32b(b)), want),
            "pmod_d" => same_f64(mth::positive_modulo_f64(f64b(a), f64b(b)), want),
            "pmod_i" => {
                mth::positive_modulo_i32(a as u32 as i32, b as u32 as i32) as u32 as u64 == want
            }
            "fast_inv_sqrt" => same_f64(mth::fast_inv_sqrt(f64b(a)), want),
            "atan2" => same_f64(mth::atan2(f64b(a), f64b(b)), want),
            "get_seed" => {
                mth::get_seed(a as u32 as i32, b as u32 as i32, c as u32 as i32) as u64 == want
            }
            other => panic!("line {}: unknown op {other}", n + 1),
        };
        assert!(ok, "line {}: {line}", n + 1);
        *counts.entry(op.to_string()).or_default() += 1;
    }
    // Every operation is exercised by a real sweep.
    assert_eq!(counts.len(), 40, "{counts:?}");
    for (op, count) in &counts {
        assert!(*count >= 400, "{op}: only {count} vectors");
    }
}

/// The yaw of exactly -45 degrees is the case that tells the double-precision index of
/// `Mth.cos(double)` apart from the single-precision product this crate used to compute: the
/// float product lands on table entry 8192 (0.70710677) where the game picks 8191 (0.707039).
#[test]
fn cos_at_minus_45_degrees_uses_the_double_product() {
    let rad = -45.0_f32 * (std::f32::consts::PI / 180.0_f32);
    assert_eq!(rad.to_bits(), 0xbf49_0fdb);
    assert_eq!(mth::cos(rad).to_bits(), 0.707_039_f32.to_bits());
    // what the single-precision formula would have returned
    let float_index = ((rad * 10430.378_f32 + 16384.0_f32) as i32 & 0xffff) as usize;
    assert_eq!(float_index, 8192);
    assert_ne!(mth::cos(rad).to_bits(), 0.707_106_77_f32.to_bits());
}

#[test]
fn constants_match_the_game() {
    assert_eq!(mth::PI.to_bits(), 0x4049_0fdb);
    assert_eq!(mth::HALF_PI.to_bits(), 0x3fc9_0fdb);
    assert_eq!(mth::TWO_PI.to_bits(), 0x40c9_0fdb);
    assert_eq!(mth::DEG_TO_RAD.to_bits(), 0x3c8e_fa35);
    assert_eq!(mth::RAD_TO_DEG.to_bits(), 57.295776_f32.to_bits());
}

/// Callers may pass `f32` (widened like a Java `float` argument) or `f64`.
#[test]
fn sin_and_cos_accept_float_and_double_callers() {
    let yaw: f32 = 123.456 * mth::DEG_TO_RAD;
    assert_eq!(mth::sin(yaw).to_bits(), mth::sin(f64::from(yaw)).to_bits());
    assert_eq!(mth::cos(yaw).to_bits(), mth::cos(f64::from(yaw)).to_bits());
    assert_eq!(mth::sin(1.0).to_bits(), mth::sin(1.0_f64).to_bits());
    assert_eq!(mth::cos(0).to_bits(), 1.0_f32.to_bits());
    // exact multiples of the table step land on their own entries
    assert_eq!(mth::sin(0.0_f32), 0.0);
    assert_eq!(mth::cos(0.0_f32), 1.0);
}
