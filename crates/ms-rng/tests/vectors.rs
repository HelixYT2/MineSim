//! Bit-for-bit checks of every generator against sequences recorded from the real classes.
//!
//! * `java_random.csv` (`seed,op,arg,bits`): `java.util.Random`, from `tools/refgen/RefGen.java`.
//! * `legacy_random.csv`, `xoroshiro_random.csv` (`op,a,b,c,s,bits`): the game's
//!   `LegacyRandomSource` / `XoroshiroRandomSource`, from `tools/refgen/GameGen.java`. Each
//!   sequence starts with `new`/`new128` and continues until the next one; `s` is the UTF-8 of a
//!   string argument in hex.
//! * `random_support.csv`: `RandomSupport`, `Mth.getSeed` and `String.hashCode` samples.

use ms_rng::support::{self, Seed128bit};
use ms_rng::{
    JavaRandom, LegacyPositionalRandomFactory, LegacyRandomSource, RandomSource,
    XoroshiroPositionalRandomFactory, XoroshiroRandomSource,
};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------------------------
// java.util.Random
// ---------------------------------------------------------------------------------------------

#[test]
fn java_random_matches_the_jvm() {
    let csv = include_str!("../testdata/java_random.csv");
    let mut rng = JavaRandom::new(0);
    let mut current: Option<i64> = None;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();

    for (n, line) in csv.lines().enumerate().skip(1) {
        let mut cols = line.split(',');
        let seed: i64 = cols.next().unwrap().parse().unwrap();
        let op = cols.next().unwrap();
        let arg: i32 = cols.next().unwrap().parse().unwrap();
        let want: u64 = cols.next().unwrap().parse().unwrap();

        if op == "reseed" {
            // setSeed on the live object, which must also drop a pending gaussian
            rng.set_seed(seed);
            current = Some(seed);
            continue;
        }
        if current != Some(seed) {
            rng = JavaRandom::new(seed);
            current = Some(seed);
        }

        let got = match op {
            "int" => u64::from(rng.next_int() as u32),
            "intb" => u64::from(rng.next_int_bound(arg) as u32),
            "long" => rng.next_long() as u64,
            "float" => u64::from(rng.next_float().to_bits()),
            "double" => rng.next_double().to_bits(),
            "bool" => u64::from(rng.next_bool()),
            "gauss" => rng.next_gaussian().to_bits(),
            "gaussskip" => {
                rng.next_gaussian();
                continue;
            }
            other => panic!("unknown op {other}"),
        };
        assert_eq!(got, want, "line {}: seed={seed} op={op} arg={arg}", n + 1);
        *counts.entry(op.to_string()).or_default() += 1;
    }
    assert!(counts.len() >= 7, "{counts:?}");
    assert!(counts["gauss"] >= 500, "{counts:?}");
}

// ---------------------------------------------------------------------------------------------
// The game's RandomSource implementations
// ---------------------------------------------------------------------------------------------

trait GameSource: RandomSource + Sized {
    type Factory;
    fn create(seed: i64) -> Self;
    fn create_halves(lo: i64, hi: i64) -> Self;
    fn fork(&mut self) -> Self;
    fn fork_positional(&mut self) -> Self::Factory;
    fn at(factory: &Self::Factory, x: i32, y: i32, z: i32) -> Self;
    fn from_hash_of(factory: &Self::Factory, name: &str) -> Self;
    fn from_seed(factory: &Self::Factory, seed: i64) -> Self;
}

impl GameSource for LegacyRandomSource {
    type Factory = LegacyPositionalRandomFactory;
    fn create(seed: i64) -> Self {
        LegacyRandomSource::new(seed)
    }
    fn create_halves(_: i64, _: i64) -> Self {
        panic!("LegacyRandomSource has no two-long constructor")
    }
    fn fork(&mut self) -> Self {
        LegacyRandomSource::fork(self)
    }
    fn fork_positional(&mut self) -> Self::Factory {
        LegacyRandomSource::fork_positional(self)
    }
    fn at(f: &Self::Factory, x: i32, y: i32, z: i32) -> Self {
        f.at(x, y, z)
    }
    fn from_hash_of(f: &Self::Factory, name: &str) -> Self {
        f.from_hash_of(name)
    }
    fn from_seed(f: &Self::Factory, seed: i64) -> Self {
        f.from_seed(seed)
    }
}

impl GameSource for XoroshiroRandomSource {
    type Factory = XoroshiroPositionalRandomFactory;
    fn create(seed: i64) -> Self {
        XoroshiroRandomSource::new(seed)
    }
    fn create_halves(lo: i64, hi: i64) -> Self {
        XoroshiroRandomSource::from_halves(lo, hi)
    }
    fn fork(&mut self) -> Self {
        XoroshiroRandomSource::fork(self)
    }
    fn fork_positional(&mut self) -> Self::Factory {
        XoroshiroRandomSource::fork_positional(self)
    }
    fn at(f: &Self::Factory, x: i32, y: i32, z: i32) -> Self {
        f.at(x, y, z)
    }
    fn from_hash_of(f: &Self::Factory, name: &str) -> Self {
        f.from_hash_of(name)
    }
    fn from_seed(f: &Self::Factory, seed: i64) -> Self {
        f.from_seed(seed)
    }
}

fn unhex(s: &str) -> String {
    let bytes: Vec<u8> = (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
        .collect();
    String::from_utf8(bytes).unwrap()
}

fn run_game_sequences<R: GameSource>(csv: &str) -> BTreeMap<String, usize> {
    let mut rng: Option<R> = None;
    let mut child: Option<R> = None;
    let mut factory: Option<R::Factory> = None;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();

    for (n, line) in csv.lines().enumerate().skip(1) {
        let mut cols = line.split(',');
        let op = cols.next().unwrap();
        let a: i64 = cols.next().unwrap().parse().unwrap();
        let b: i64 = cols.next().unwrap().parse().unwrap();
        let c: i64 = cols.next().unwrap().parse().unwrap();
        let s = unhex(cols.next().unwrap());
        let want: u64 = cols.next().unwrap().parse().unwrap();
        let ctx = || format!("line {}: {line}", n + 1);

        match op {
            "new" => {
                rng = Some(R::create(a));
                child = None;
                factory = None;
                continue;
            }
            "new128" => {
                rng = Some(R::create_halves(a, b));
                child = None;
                factory = None;
                continue;
            }
            _ => {}
        }
        let r = rng.as_mut().expect("sequence starts with new");
        let got: u64 = match op {
            "setseed" => {
                r.set_seed(a);
                continue;
            }
            "consume" => {
                r.consume_count(a as i32);
                continue;
            }
            "fp_new" => {
                factory = Some(r.fork_positional());
                continue;
            }
            "int" => u64::from(r.next_int() as u32),
            "intb" => u64::from(r.next_int_bound(a as i32) as u32),
            "intbi" => u64::from(r.next_int_between_inclusive(a as i32, b as i32) as u32),
            "intr" => u64::from(r.next_int_range(a as i32, b as i32) as u32),
            "long" => r.next_long() as u64,
            "bool" => u64::from(r.next_boolean()),
            "float" => u64::from(r.next_float().to_bits()),
            "double" => r.next_double().to_bits(),
            "gauss" => r.next_gaussian().to_bits(),
            "tri_d" => r
                .triangle(f64::from_bits(a as u64), f64::from_bits(b as u64))
                .to_bits(),
            "tri_f" => u64::from(
                r.triangle_f32(f32::from_bits(a as u32), f32::from_bits(b as u32))
                    .to_bits(),
            ),
            "fork" => {
                let mut forked = r.fork();
                let first = forked.next_long() as u64;
                child = Some(forked);
                first
            }
            "child_double" => child.as_mut().unwrap().next_double().to_bits(),
            "child_gauss" => child.as_mut().unwrap().next_gaussian().to_bits(),
            "fp_at" => {
                R::at(factory.as_ref().unwrap(), a as i32, b as i32, c as i32).next_long() as u64
            }
            "fp_hash" => R::from_hash_of(factory.as_ref().unwrap(), &s).next_long() as u64,
            "fp_seed" => R::from_seed(factory.as_ref().unwrap(), a).next_long() as u64,
            other => panic!("{}: unknown op {other}", ctx()),
        };
        assert_eq!(got, want, "{}", ctx());
        *counts.entry(op.to_string()).or_default() += 1;
    }
    counts
}

fn check_counts(counts: &BTreeMap<String, usize>) {
    for op in [
        "int",
        "intb",
        "intbi",
        "intr",
        "long",
        "bool",
        "float",
        "double",
        "gauss",
        "tri_d",
        "tri_f",
        "fork",
        "child_double",
        "child_gauss",
        "fp_at",
        "fp_hash",
        "fp_seed",
    ] {
        assert!(
            counts.get(op).copied().unwrap_or(0) >= 50,
            "{op}: {counts:?}"
        );
    }
    assert!(counts["gauss"] >= 500, "{counts:?}");
}

#[test]
fn legacy_random_source_matches_the_game() {
    let counts =
        run_game_sequences::<LegacyRandomSource>(include_str!("../testdata/legacy_random.csv"));
    check_counts(&counts);
}

#[test]
fn xoroshiro_random_source_matches_the_game() {
    let counts = run_game_sequences::<XoroshiroRandomSource>(include_str!(
        "../testdata/xoroshiro_random.csv"
    ));
    check_counts(&counts);
}

// ---------------------------------------------------------------------------------------------
// RandomSupport, Mth.getSeed, String.hashCode
// ---------------------------------------------------------------------------------------------

#[test]
fn random_support_matches_the_game() {
    let csv = include_str!("../testdata/random_support.csv");
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (n, line) in csv.lines().enumerate().skip(1) {
        let mut cols = line.split(',');
        let op = cols.next().unwrap();
        let a: i64 = cols.next().unwrap().parse().unwrap();
        let _b: i64 = cols.next().unwrap().parse().unwrap();
        let _c: i64 = cols.next().unwrap().parse().unwrap();
        let s = unhex(cols.next().unwrap());
        let want: u64 = cols.next().unwrap().parse().unwrap();

        let got: u64 = match op {
            "mix13" => support::mix_stafford13(a) as u64,
            "up128u_lo" => support::upgrade_seed_to_128bit_unmixed(a).seed_lo as u64,
            "up128u_hi" => support::upgrade_seed_to_128bit_unmixed(a).seed_hi as u64,
            "up128_lo" => support::upgrade_seed_to_128bit(a).seed_lo as u64,
            "up128_hi" => support::upgrade_seed_to_128bit(a).seed_hi as u64,
            "md5_lo" => support::seed_from_hash_of(&s).seed_lo as u64,
            "md5_hi" => support::seed_from_hash_of(&s).seed_hi as u64,
            "jhash" => u64::from(support::java_string_hash(&s) as u32),
            other => panic!("unknown op {other}"),
        };
        assert_eq!(got, want, "line {}: {line}", n + 1);
        *counts.entry(op.to_string()).or_default() += 1;
    }
    assert_eq!(counts.len(), 8, "{counts:?}");
    for (op, count) in &counts {
        assert!(*count >= 100, "{op}: {count}");
    }
}

#[test]
fn seed128bit_helpers() {
    let seed = Seed128bit::new(1, 2);
    assert_eq!(seed.xor(3, 4), Seed128bit::new(2, 6));
    assert_eq!(
        seed.mixed(),
        Seed128bit::new(support::mix_stafford13(1), support::mix_stafford13(2))
    );
}

#[test]
fn unit_constants_are_the_expected_powers_of_two() {
    // nextFloat and nextDouble scale by exactly 2^-24 and 2^-53.
    let mut r = LegacyRandomSource::new(7);
    for _ in 0..1000 {
        let f = r.next_float();
        assert_eq!((f * (1u32 << 24) as f32).fract(), 0.0);
        let d = r.next_double();
        assert_eq!((d * (1u64 << 53) as f64).fract(), 0.0);
        assert!((0.0..1.0).contains(&d));
    }
}

#[test]
#[should_panic(expected = "Bound must be positive")]
fn legacy_rejects_non_positive_bounds() {
    LegacyRandomSource::new(1).next_int_bound(0);
}

#[test]
#[should_panic(expected = "Bound must be positive")]
fn xoroshiro_rejects_non_positive_bounds() {
    XoroshiroRandomSource::new(1).next_int_bound(-3);
}

#[test]
#[should_panic(expected = "bound - origin is non positive")]
fn range_requires_origin_below_bound() {
    LegacyRandomSource::new(1).next_int_range(5, 5);
}
