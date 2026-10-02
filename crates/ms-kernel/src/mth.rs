//! `Mth.sin` / `Mth.cos` as 1.21.11 defines them.
//!
//! The game indexes a 65536-entry table of `(float) Math.sin(i / 10430.378350470453)`; since 1.21.x
//! the argument is a `double` and the index is taken in double precision
//! (`(long)(d * 10430.378350470453) & 65535`), so a float angle is widened before the multiply. The
//! table values are the same ones `ms_numerics::mth` embeds (its generator formula agrees with
//! this one on all 65536 entries), but its lookup truncates a *float* product, which lands on a
//! neighbouring entry whenever the float and double products straddle an integer (for instance at
//! exactly -45 degrees of yaw: the cosine index is one entry lower). Kernel code that mirrors a `Mth.sin(float)` call site uses these.

const SIN_TABLE: &[u8] = include_bytes!("../../ms-numerics/data/mth_sin_table.bin");

const _: () = assert!(SIN_TABLE.len() == 65536 * 4);

const SIN_SCALE: f64 = 10430.378350470453;

#[inline]
fn lookup(index: usize) -> f32 {
    let i = index * 4;
    f32::from_le_bytes([
        SIN_TABLE[i],
        SIN_TABLE[i + 1],
        SIN_TABLE[i + 2],
        SIN_TABLE[i + 3],
    ])
}

/// `Mth.sin(double)`.
#[inline]
pub fn sin(d: f64) -> f32 {
    lookup(((d * SIN_SCALE) as i64 & 65535) as usize)
}

/// `Mth.cos(double)`.
#[inline]
pub fn cos(d: f64) -> f32 {
    lookup(((d * SIN_SCALE + 16384.0) as i64 & 65535) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_turns() {
        assert_eq!(sin(0.0), 0.0);
        assert_eq!(cos(0.0), 1.0);
        assert_eq!(sin(std::f64::consts::FRAC_PI_2), 1.0);
    }

    #[test]
    fn minus_45_degrees_uses_the_double_product() {
        // -45 degrees as the game computes it: the float product of yaw and pi/180, widened.
        let f = -45.0_f32 * ((std::f64::consts::PI / 180.0) as f32);
        let wide = (f64::from(f) * SIN_SCALE + 16384.0) as i64 & 65535;
        let narrow = i64::from((f * 10430.378_f32 + 16384.0_f32) as i32 & 0xffff);
        assert_eq!(
            (wide, narrow),
            (8191, 8192),
            "the cos index differs by one entry"
        );
        assert_eq!(cos(f64::from(f)), lookup(8191));
        assert_ne!(cos(f64::from(f)), lookup(8192));
    }
}
