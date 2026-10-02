//! `ms_numerics::hotspot::log` against `Math.log` on the reference JVM and against the
//! correctly rounded logarithm.
//!
//! `testdata/hotspot_log.csv` has, per argument, the bits of `Math.log(x)` as HotSpot/x86_64
//! computed it and the bits of the correctly rounded `ln(x)` (computed with 80-digit BigDecimal
//! arithmetic by `tools/refgen/RefGen.java`).

use ms_numerics::{fdlibm, hotspot};

#[test]
fn log_is_correctly_rounded_and_tracks_hotspot() {
    let csv = include_str!("../testdata/hotspot_log.csv");
    let mut rows = 0usize;
    let mut stub_misroundings = 0usize;
    let mut fdlibm_misroundings = 0usize;
    for (n, line) in csv.lines().enumerate().skip(1) {
        let mut cols = line.split(',');
        let x = f64::from_bits(cols.next().unwrap().parse().unwrap());
        let math_log: u64 = cols.next().unwrap().parse().unwrap();
        let exact: u64 = cols.next().unwrap().parse().unwrap();

        let got = hotspot::log(x).to_bits();
        assert_eq!(
            got,
            exact,
            "line {}: log({x:e}) = {:e}, correctly rounded is {:e}",
            n + 1,
            f64::from_bits(got),
            f64::from_bits(exact)
        );
        rows += 1;
        if math_log != exact {
            // the Intel stub misrounded: it is then one ulp away from the exact answer
            assert_eq!(math_log.abs_diff(exact), 1, "line {}", n + 1);
            stub_misroundings += 1;
        }
        if fdlibm::log(x).to_bits() != exact {
            fdlibm_misroundings += 1;
        }
    }
    assert!(rows >= 10_000, "{rows}");
    // The stub misrounds only a tiny fraction of arguments (619 in 50 million mixed samples in
    // an offline sweep); fdlibm misrounds several percent, which is why gaussians use `hotspot::log`.
    assert!(
        stub_misroundings * 1000 < rows,
        "{stub_misroundings} of {rows}"
    );
    assert!(
        fdlibm_misroundings * 100 > rows,
        "{fdlibm_misroundings} of {rows}"
    );
}

#[test]
fn special_values() {
    assert!(hotspot::log(f64::NAN).is_nan());
    assert!(hotspot::log(-1.0).is_nan());
    assert!(hotspot::log(f64::NEG_INFINITY).is_nan());
    assert_eq!(hotspot::log(0.0), f64::NEG_INFINITY);
    assert_eq!(hotspot::log(-0.0), f64::NEG_INFINITY);
    assert_eq!(hotspot::log(f64::INFINITY), f64::INFINITY);
    assert_eq!(hotspot::log(1.0).to_bits(), 0.0_f64.to_bits());
    assert_eq!(
        hotspot::log(std::f64::consts::E).to_bits(),
        1.0_f64.to_bits()
    );
}
