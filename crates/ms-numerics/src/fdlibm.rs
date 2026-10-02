//! Pure-Rust ports of the fdlibm 5.3 routines behind Java's `StrictMath`.
//!
//! `Math.acos`, `Math.atan`, `Math.atan2` simply delegate to `StrictMath` (fdlibm) in the JDK, and
//! `java.util.Random.nextGaussian` calls `StrictMath.log` / `StrictMath.sqrt` explicitly. These
//! functions reproduce fdlibm's operation order and constants exactly, so their results are bit
//! identical to the JVM on every platform (the platform libm is deliberately not used).
//!
//! `Math.log` is *not* `StrictMath.log` on HotSpot/x86_64 (it is an Intel LIBM stub that agrees
//! with fdlibm only ~93% of the time); see [`crate::hotspot`] for that one.
//!
//! The algorithms are Sun's fdlibm (freely licensed, see the netlib distribution); the code here
//! is an independent Rust rendering that follows its structure, including the hi/lo word tricks.
//! NaN results are "some NaN": the sign/payload of a generated NaN is whatever the hardware
//! produces, as in C, and is not guaranteed to match the JVM's bits.

#[inline]
fn hi(x: f64) -> i32 {
    (x.to_bits() >> 32) as u32 as i32
}

#[inline]
fn lo(x: f64) -> u32 {
    x.to_bits() as u32
}

#[inline]
fn with_hi(x: f64, high: i32) -> f64 {
    f64::from_bits((u64::from(high as u32) << 32) | (x.to_bits() & 0xffff_ffff))
}

#[inline]
fn with_lo(x: f64, low: u32) -> f64 {
    f64::from_bits((x.to_bits() & 0xffff_ffff_0000_0000) | u64::from(low))
}

// ---------------------------------------------------------------------------------------------
// log (e_log.c)
// ---------------------------------------------------------------------------------------------

const LN2_HI: f64 = from_bits(0x3fe6_2e42_fee0_0000);
const LN2_LO: f64 = from_bits(0x3dea_39ef_3579_3c76);
const TWO54: f64 = from_bits(0x4350_0000_0000_0000);
const LG1: f64 = from_bits(0x3fe5_5555_5555_5593);
const LG2: f64 = from_bits(0x3fd9_9999_9997_fa04);
const LG3: f64 = from_bits(0x3fd2_4924_9422_9359);
const LG4: f64 = from_bits(0x3fcc_71c5_1d8e_78af);
const LG5: f64 = from_bits(0x3fc7_4664_96cb_03de);
const LG6: f64 = from_bits(0x3fc3_9a09_d078_c69f);
const LG7: f64 = from_bits(0x3fc2_f112_df3e_5244);

const fn from_bits(bits: u64) -> f64 {
    f64::from_bits(bits)
}

/// `StrictMath.log(x)` (fdlibm `__ieee754_log`).
pub fn log(x: f64) -> f64 {
    let mut x = x;
    let mut hx = hi(x);
    let lx = lo(x);

    let mut k: i32 = 0;
    if hx < 0x0010_0000 {
        // x < 2**-1022
        if ((hx & 0x7fff_ffff) as u32 | lx) == 0 {
            return f64::NEG_INFINITY; // log(+-0) = -inf
        }
        if hx < 0 {
            return f64::NAN; // log(-#) = NaN
        }
        k -= 54;
        x *= TWO54; // subnormal: scale up
        hx = hi(x);
    }
    if hx >= 0x7ff0_0000 {
        return x + x;
    }
    k += (hx >> 20) - 1023;
    hx &= 0x000f_ffff;
    let i = (hx + 0x95f64) & 0x10_0000;
    x = with_hi(x, hx | (i ^ 0x3ff0_0000)); // normalize x or x/2
    k += i >> 20;
    let f = x - 1.0;
    if (0x000f_ffff & (2 + hx)) < 3 {
        // |f| < 2**-20
        if f == 0.0 {
            if k == 0 {
                return 0.0;
            }
            let dk = f64::from(k);
            return dk * LN2_HI + dk * LN2_LO;
        }
        let r = f * f * (0.5 - 0.333_333_333_333_333_3 * f);
        if k == 0 {
            return f - r;
        }
        let dk = f64::from(k);
        return dk * LN2_HI - ((r - dk * LN2_LO) - f);
    }
    let s = f / (2.0 + f);
    let dk = f64::from(k);
    let z = s * s;
    let i = hx - 0x6147a;
    let w = z * z;
    let j = 0x6b851 - hx;
    let t1 = w * (LG2 + w * (LG4 + w * LG6));
    let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
    let i = i | j;
    let r = t2 + t1;
    if i > 0 {
        let hfsq = 0.5 * f * f;
        if k == 0 {
            f - (hfsq - s * (hfsq + r))
        } else {
            dk * LN2_HI - ((hfsq - (s * (hfsq + r) + dk * LN2_LO)) - f)
        }
    } else if k == 0 {
        f - s * (f - r)
    } else {
        dk * LN2_HI - ((s * (f - r) - dk * LN2_LO) - f)
    }
}

// ---------------------------------------------------------------------------------------------
// atan (s_atan.c)
// ---------------------------------------------------------------------------------------------

const ATAN_HI: [f64; 4] = [
    from_bits(0x3fdd_ac67_0561_bb4f), // atan(0.5)hi
    from_bits(0x3fe9_21fb_5444_2d18), // atan(1.0)hi
    from_bits(0x3fef_730b_d281_f69b), // atan(1.5)hi
    from_bits(0x3ff9_21fb_5444_2d18), // atan(inf)hi
];

const ATAN_LO: [f64; 4] = [
    from_bits(0x3c7a_2b7f_222f_65e2), // atan(0.5)lo
    from_bits(0x3c81_a626_3314_5c07), // atan(1.0)lo
    from_bits(0x3c70_0788_7af0_cbbd), // atan(1.5)lo
    from_bits(0x3c91_a626_3314_5c07), // atan(inf)lo
];

const AT: [f64; 11] = [
    from_bits(0x3fd5_5555_5555_550d),
    from_bits(0xbfc9_9999_9998_ebc4),
    from_bits(0x3fc2_4924_9200_83ff),
    from_bits(0xbfbc_71c6_fe23_1671),
    from_bits(0x3fb7_45cd_c54c_206e),
    from_bits(0xbfb3_b0f2_af74_9a6d),
    from_bits(0x3fb1_0d66_a0d0_3d51),
    from_bits(0xbfad_de2d_52de_fd9a),
    from_bits(0x3fa9_7b4b_2476_0deb),
    from_bits(0xbfa2_b444_2c6a_6c2f),
    from_bits(0x3f90_ad3a_e322_da11),
];

/// `StrictMath.atan(x)` (fdlibm `atan`).
pub fn atan(x: f64) -> f64 {
    let mut x = x;
    let hx = hi(x);
    let ix = hx & 0x7fff_ffff;
    let id: i32;
    if ix >= 0x4410_0000 {
        // |x| >= 2^66
        if ix > 0x7ff0_0000 || (ix == 0x7ff0_0000 && lo(x) != 0) {
            return x + x; // NaN
        }
        return if hx > 0 {
            ATAN_HI[3] + ATAN_LO[3]
        } else {
            -ATAN_HI[3] - ATAN_LO[3]
        };
    }
    if ix < 0x3fdc_0000 {
        // |x| < 0.4375
        if ix < 0x3e20_0000 {
            // |x| < 2^-29: atan(x) = x (huge + x > 1 always)
            return x;
        }
        id = -1;
    } else {
        x = x.abs();
        if ix < 0x3ff3_0000 {
            // |x| < 1.1875
            if ix < 0x3fe6_0000 {
                // 7/16 <= |x| < 11/16
                id = 0;
                x = (2.0 * x - 1.0) / (2.0 + x);
            } else {
                // 11/16 <= |x| < 19/16
                id = 1;
                x = (x - 1.0) / (x + 1.0);
            }
        } else if ix < 0x4003_8000 {
            // |x| < 2.4375
            id = 2;
            x = (x - 1.5) / (1.0 + 1.5 * x);
        } else {
            // 2.4375 <= |x| < 2^66
            id = 3;
            x = -1.0 / x;
        }
    }
    let z = x * x;
    let w = z * z;
    // break the sum of aT[i] z**(i+1) into odd and even polynomials
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));
    if id < 0 {
        x - x * (s1 + s2)
    } else {
        let id = id as usize;
        let z = ATAN_HI[id] - ((x * (s1 + s2) - ATAN_LO[id]) - x);
        if hx < 0 {
            -z
        } else {
            z
        }
    }
}

// ---------------------------------------------------------------------------------------------
// atan2 (e_atan2.c)
// ---------------------------------------------------------------------------------------------

const TINY: f64 = 1.0e-300;
const PI_O_4: f64 = from_bits(0x3fe9_21fb_5444_2d18);
const PI_O_2: f64 = from_bits(0x3ff9_21fb_5444_2d18);
const PI: f64 = from_bits(0x4009_21fb_5444_2d18);
const PI_LO: f64 = from_bits(0x3ca1_a626_3314_5c07);

/// `StrictMath.atan2(y, x)` (fdlibm `__ieee754_atan2`).
pub fn atan2(y: f64, x: f64) -> f64 {
    let hx = hi(x);
    let ix = hx & 0x7fff_ffff;
    let lx = lo(x);
    let hy = hi(y);
    let iy = hy & 0x7fff_ffff;
    let ly = lo(y);
    // x or y is NaN
    if ((ix as u32) | ((lx | lx.wrapping_neg()) >> 31)) > 0x7ff0_0000
        || ((iy as u32) | ((ly | ly.wrapping_neg()) >> 31)) > 0x7ff0_0000
    {
        return x + y;
    }
    if (hx.wrapping_sub(0x3ff0_0000) as u32 | lx) == 0 {
        return atan(y); // x = 1.0
    }
    let m = ((hy >> 31) & 1) | ((hx >> 30) & 2); // 2*sign(x) + sign(y)

    // when y = 0
    if (iy as u32 | ly) == 0 {
        match m {
            0 | 1 => return y,      // atan(+-0, +anything) = +-0
            2 => return PI + TINY,  // atan(+0, -anything) = pi
            _ => return -PI - TINY, // atan(-0, -anything) = -pi
        }
    }
    // when x = 0
    if (ix as u32 | lx) == 0 {
        return if hy < 0 {
            -PI_O_2 - TINY
        } else {
            PI_O_2 + TINY
        };
    }
    // when x is INF
    if ix == 0x7ff0_0000 {
        if iy == 0x7ff0_0000 {
            return match m {
                0 => PI_O_4 + TINY,
                1 => -PI_O_4 - TINY,
                2 => 3.0 * PI_O_4 + TINY,
                _ => -3.0 * PI_O_4 - TINY,
            };
        }
        return match m {
            0 => 0.0,
            1 => -0.0,
            2 => PI + TINY,
            _ => -PI - TINY,
        };
    }
    // when y is INF
    if iy == 0x7ff0_0000 {
        return if hy < 0 {
            -PI_O_2 - TINY
        } else {
            PI_O_2 + TINY
        };
    }

    // compute y/x
    let k = (iy - ix) >> 20;
    let z = if k > 60 {
        PI_O_2 + 0.5 * PI_LO // |y/x| > 2**60
    } else if hx < 0 && k < -60 {
        0.0 // |y|/x < -2**60
    } else {
        atan((y / x).abs())
    };
    match m {
        0 => z,
        1 => with_hi(z, hi(z) ^ i32::MIN),
        2 => PI - (z - PI_LO),
        _ => (z - PI_LO) - PI,
    }
}

// ---------------------------------------------------------------------------------------------
// acos (e_acos.c)
// ---------------------------------------------------------------------------------------------

const PIO2_HI: f64 = from_bits(0x3ff9_21fb_5444_2d18);
const PIO2_LO: f64 = from_bits(0x3c91_a626_3314_5c07);
const PS0: f64 = from_bits(0x3fc5_5555_5555_5555);
const PS1: f64 = from_bits(0xbfd4_d612_03eb_6f7d);
const PS2: f64 = from_bits(0x3fc9_c155_0e88_4455);
const PS3: f64 = from_bits(0xbfa4_8228_b568_8f3b);
const PS4: f64 = from_bits(0x3f49_efe0_7501_b288);
const PS5: f64 = from_bits(0x3f02_3de1_0dfd_f709);
const QS1: f64 = from_bits(0xc003_3a27_1c8a_2d4b);
const QS2: f64 = from_bits(0x4000_2ae5_9c59_8ac8);
const QS3: f64 = from_bits(0xbfe6_066c_1b8d_0159);
const QS4: f64 = from_bits(0x3fb3_b8c5_b12e_9282);

#[inline]
fn acos_poly_p(z: f64) -> f64 {
    z * (PS0 + z * (PS1 + z * (PS2 + z * (PS3 + z * (PS4 + z * PS5)))))
}

#[inline]
fn acos_poly_q(z: f64) -> f64 {
    1.0 + z * (QS1 + z * (QS2 + z * (QS3 + z * QS4)))
}

/// `StrictMath.acos(x)` (fdlibm `__ieee754_acos`).
pub fn acos(x: f64) -> f64 {
    let hx = hi(x);
    let ix = hx & 0x7fff_ffff;
    if ix >= 0x3ff0_0000 {
        // |x| >= 1
        if ((ix - 0x3ff0_0000) as u32 | lo(x)) == 0 {
            // |x| == 1
            return if hx > 0 { 0.0 } else { PI + 2.0 * PIO2_LO };
        }
        return f64::NAN; // acos(|x| > 1) is NaN
    }
    if ix < 0x3fe0_0000 {
        // |x| < 0.5
        if ix <= 0x3c60_0000 {
            return PIO2_HI + PIO2_LO; // |x| < 2**-57
        }
        let z = x * x;
        let p = acos_poly_p(z);
        let q = acos_poly_q(z);
        let r = p / q;
        PIO2_HI - (x - (PIO2_LO - x * r))
    } else if hx < 0 {
        // x < -0.5
        let z = (1.0 + x) * 0.5;
        let p = acos_poly_p(z);
        let q = acos_poly_q(z);
        let s = z.sqrt();
        let r = p / q;
        let w = r * s - PIO2_LO;
        PI - 2.0 * (s + w)
    } else {
        // x > 0.5
        let z = (1.0 - x) * 0.5;
        let s = z.sqrt();
        let df = with_lo(s, 0);
        let c = (z - df * df) / (s + df);
        let p = acos_poly_p(z);
        let q = acos_poly_q(z);
        let r = p / q;
        let w = r * s + c;
        2.0 * (df + w)
    }
}
