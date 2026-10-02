//! `Math.log` as the HotSpot JVM on x86_64 executes it.
//!
//! `Math.log` is not `StrictMath.log`: HotSpot replaces it with a hand-written Intel LIBM stub
//! (interpreter, C1 and C2 alike), and that stub is far more accurate than fdlibm. Measured on
//! this project's reference JVM (OpenJDK 21, x86_64) over 50 million mixed arguments: `Math.log`
//! differs from the *correctly rounded* logarithm for 619 of them (about 1 in 80 000), whereas
//! fdlibm's `StrictMath.log` differs from it for about 7 in 100. The game calls `Math.log` in the
//! polar-method `nextGaussian` of its random sources, so the closest portable reproduction is the
//! correctly rounded logarithm, which [`log`] computes with exact 128-bit fixed-point arithmetic
//! (no platform libm involved, fully deterministic). All 619 disagreements were checked against
//! an 80-digit reference: [`log`] is the correctly rounded value in every one of them, the stub
//! is one ulp off.
//!
//! Residual platform dependence, stated plainly:
//! * On x86_64 HotSpot, [`log`] agrees with `Math.log` except where the Intel stub itself
//!   misrounds (about 1 in 80 000 arguments; `tests/hotspot_log_vectors.rs` pins the exact
//!   behaviour on the committed sweep). Through `nextGaussian` that is about 4 differing
//!   gaussians per million draws (measured against the real random sources).
//! * Other architectures were not measured. A JVM whose `Math.log` is fdlibm-based would match
//!   [`crate::fdlibm::log`] exactly instead; the random sources accept either through
//!   `next_gaussian_with_log`.

const MASK64: u128 = 0xffff_ffff_ffff_ffff;

/// ln 2 in 116-bit fixed point: `round(ln(2) * 2^116)`.
const LN2_Q116: i128 = 57584414849978831576646519229529903;
const FIX: u32 = 116;

/// Terms of the series `sum w^k / (2k+1)`; `w <= 0.0295`, so 27 terms leave `< 2^-130`.
const TERMS: usize = 27;

/// `1 / (2k + 1)` in Q124.
const INV_ODD_Q124: [u128; TERMS] = {
    let mut table = [0u128; TERMS];
    let mut k = 0;
    while k < TERMS {
        table[k] = (1u128 << 124) / (2 * k as u128 + 1);
        k += 1;
    }
    table
};

/// The high 128 bits of the 256-bit product `a * b`.
fn mul_hi(a: u128, b: u128) -> u128 {
    let (a1, a0) = (a >> 64, a & MASK64);
    let (b1, b0) = (b >> 64, b & MASK64);
    let p00 = a0 * b0;
    let p01 = a0 * b1;
    let p10 = a1 * b0;
    let p11 = a1 * b1;
    let mid = (p00 >> 64) + (p01 & MASK64) + (p10 & MASK64);
    p11 + (p01 >> 64) + (p10 >> 64) + (mid >> 64)
}

/// Rounds `mant * 2^exp2` (a positive value) to the nearest `f64`, ties to even.
fn to_f64(negative: bool, mant: u128, exp2: i32) -> f64 {
    if mant == 0 {
        return if negative { -0.0 } else { 0.0 };
    }
    let lz = mant.leading_zeros();
    let norm = mant << lz;
    let mut top = (norm >> 75) as u64; // 53 significant bits
    let rest = norm & ((1u128 << 75) - 1);
    let half = 1u128 << 74;
    if rest > half || (rest == half && top & 1 == 1) {
        top += 1;
    }
    // value = top * 2^(exp2 - lz + 75)
    let mut e = exp2 - lz as i32 + 75;
    if top == 1 << 53 {
        top >>= 1;
        e += 1;
    }
    // top is in [2^52, 2^53): the leading bit has weight 2^(52 + e)
    let biased = (52 + e + 1023) as u64;
    debug_assert!((1..0x7ff).contains(&biased));
    let bits = (biased << 52) | (top & ((1u64 << 52) - 1));
    let magnitude = f64::from_bits(bits);
    if negative {
        -magnitude
    } else {
        magnitude
    }
}

/// `Math.log(x)` as HotSpot/x86_64 computes it, reproduced as the correctly rounded natural
/// logarithm (see the module documentation for the exact agreement guarantees).
pub fn log(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x < 0.0 {
        return f64::NAN;
    }
    if x == f64::INFINITY {
        return f64::INFINITY;
    }
    if x == 1.0 {
        return 0.0;
    }

    // x = mant * 2^(exp - 52) with mant in [2^52, 2^53), normalising subnormals.
    let bits = x.to_bits();
    let exp_field = (bits >> 52) & 0x7ff;
    let frac = bits & ((1u64 << 52) - 1);
    let (mant, mut exp) = if exp_field == 0 {
        let lz = frac.leading_zeros() as i32 - 11;
        (frac << lz, -1022 - lz)
    } else {
        (frac | (1u64 << 52), exp_field as i32 - 1023)
    };

    // Reduce to m in (sqrt(2)/2, sqrt(2)]: m = mant / 2^p.
    let mut p = 52;
    if u128::from(mant) * u128::from(mant) > 1u128 << 105 {
        p = 53;
        exp += 1;
    }
    let one = 1u64 << p;
    let negative_z = mant < one;
    let num = mant.abs_diff(one);
    let den = mant + one; // z = (m - 1) / (m + 1), |z| <= 0.1716

    // L = ln(m) = 2 * atanh(z), as `hi * 2^-(123 + shift)`, with `hi` carrying ~124 bits.
    let (hi, shift) = if num == 0 {
        (0u128, 0u32)
    } else {
        // Normalise num so that r = num'/den lies in [1/2, 1), then z = r * 2^-shift.
        let mut shift = num.leading_zeros() - den.leading_zeros();
        if (num << shift) >= den {
            shift -= 1;
        }
        let numer = u128::from(num << shift);
        let den128 = u128::from(den);
        let q1 = (numer << 64) / den128;
        let rem = (numer << 64) % den128;
        let q2 = (rem << 64) / den128;
        let q = (q1 << 64) | q2; // r * 2^128, top bit set

        let w_full = mul_hi(q, q); // r^2 in Q128
        let w = if 2 * shift >= 128 {
            0
        } else {
            w_full >> (2 * shift)
        };

        // P(w) = sum w^k / (2k+1) in Q124 (Horner), then s = z * P(w).
        let mut series = INV_ODD_Q124[TERMS - 1];
        for k in (0..TERMS - 1).rev() {
            series = INV_ODD_Q124[k] + mul_hi(w, series);
        }
        (mul_hi(q, series), shift)
    };

    if exp == 0 {
        // ln(x) = ln(m) = 2s, no ln(2) term: keep full relative precision.
        return to_f64(negative_z, hi, 1 - 124 - shift as i32);
    }

    // |ln(x)| >= 0.34 here, so absolute precision 2^-116 is plenty.
    let l_fixed = {
        // L = hi * 2^-(123 + shift), so in Q116 it is `hi >> (7 + shift)`.
        let drop = 7 + shift;
        let magnitude = if drop >= 128 { 0 } else { (hi >> drop) as i128 };
        if negative_z {
            -magnitude
        } else {
            magnitude
        }
    };
    let total = i128::from(exp) * LN2_Q116 + l_fixed;
    to_f64(total < 0, total.unsigned_abs(), -(FIX as i32))
}
