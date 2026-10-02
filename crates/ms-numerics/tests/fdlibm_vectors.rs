//! Bit-for-bit checks of the fdlibm ports against `StrictMath` on the JVM.
//!
//! The vectors are produced by `tools/refgen/RefGen.java` (see `tools/refgen/README.md`).

use ms_numerics::fdlibm;

/// "Same result": identical bits, or both NaN (the payload and sign of a generated NaN are
/// whatever the hardware produced, which no caller can depend on).
fn same(got: f64, want: f64) -> bool {
    (got.is_nan() && want.is_nan()) || got.to_bits() == want.to_bits()
}

#[test]
fn matches_strictmath_vectors() {
    let csv = include_str!("../testdata/fdlibm_reference.csv");
    let mut counts = [0usize; 4];
    for (n, line) in csv.lines().enumerate().skip(1) {
        let mut cols = line.split(',');
        let name = cols.next().unwrap();
        let a = f64::from_bits(cols.next().unwrap().parse().unwrap());
        let b = f64::from_bits(cols.next().unwrap().parse().unwrap());
        let want = f64::from_bits(cols.next().unwrap().parse().unwrap());
        let (got, slot) = match name {
            "acos" => (fdlibm::acos(a), 0),
            "atan" => (fdlibm::atan(a), 1),
            "atan2" => (fdlibm::atan2(a, b), 2),
            "log" => (fdlibm::log(a), 3),
            other => panic!("unknown function {other}"),
        };
        counts[slot] += 1;
        assert!(
            same(got, want),
            "line {}: {name}({a:e}, {b:e}) = {got:e} ({:#x}), want {want:e} ({:#x})",
            n + 1,
            got.to_bits(),
            want.to_bits()
        );
    }
    // Every function has a substantial sweep behind it.
    assert!(counts.iter().all(|&c| c >= 2500), "{counts:?}");
}
