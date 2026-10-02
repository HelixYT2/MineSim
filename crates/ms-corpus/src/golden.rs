//! The golden-hash lock (`docs/contract.md` §5, `docs/corpus.md`): `corpus/golden-hashes.json`
//! records, per scenario, the rolling hash of the recorded states, the rolling hash of the states
//! the kernel produced, and whether the scenario is *frozen* (the kernel reproduces it
//! bit-for-bit). CI replays every scenario and fails if a frozen scenario's hash moves or the
//! recorded corpus no longer matches its fingerprint.
//!
//! ```text
//! cargo xtask oracle --bless                # replay everything, (re)write the file
//! cargo xtask oracle --bless walk_basic     # refresh only the named scenarios
//! cargo xtask oracle --bless --oracle-only  # record corpus fingerprints without running the kernel
//! ```

use crate::replay::{oracle_hashes, rolling, Mode, Report};
use crate::{client_scenarios, corpus_dir, Scenario};
use ms_oracle::player::{RollingHash, CONTRACT_VERSION};
use ms_oracle::HASH_SEED;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// A checkpoint is recorded every this many ticks (and after the last tick), so a failure can name
/// the window of ticks that changed without storing a hash per tick.
pub const CHECKPOINT_INTERVAL: usize = 16;

/// Environment variable: when set (to anything but `0`), unfrozen scenarios must also reproduce
/// their blessed simulated hash, i.e. any change in kernel behaviour fails until re-blessed.
pub const STRICT_ENV: &str = "MINESIM_GOLDEN_STRICT";

/// The kernel entry point the lock replays, in one place: when the kernel's tick signature
/// changes, adapt this function (and nothing else in the harness).
pub fn replay_kernel(scenario: &Scenario) -> Report {
    replay_kernel_with(scenario, Mode::FreeRun)
}

/// [`replay_kernel`] in a chosen [`Mode`].
pub fn replay_kernel_with(scenario: &Scenario, mode: Mode) -> Report {
    crate::replay_with(scenario, mode, |p, input, world| {
        ms_kernel::player::tick(p, input, world);
    })
}

/// One scenario's golden record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub ticks: usize,
    /// Rolling hash of the recorded states: fingerprints the corpus file and the serialization.
    pub oracle: u64,
    /// High 32 bits of the recorded rolling hash after each [`CHECKPOINT_INTERVAL`] ticks and
    /// after the last tick.
    pub checkpoints: Vec<u32>,
    /// Rolling hash of the kernel's states when blessed (absent for an oracle-only bless).
    pub sim: Option<u64>,
    /// The kernel reproduced every tick when blessed (`sim == oracle`): the scenario is locked.
    pub frozen: bool,
}

/// The whole file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Golden {
    pub contract: u8,
    pub hash_seed: u64,
    /// Every corpus scenario has an entry with a simulated hash: a missing entry is a failure.
    pub locked: bool,
    pub entries: BTreeMap<String, Entry>,
}

impl Default for Golden {
    fn default() -> Self {
        Self {
            contract: CONTRACT_VERSION,
            hash_seed: HASH_SEED,
            locked: false,
            entries: BTreeMap::new(),
        }
    }
}

/// `corpus/golden-hashes.json`.
pub fn golden_path() -> PathBuf {
    corpus_dir().join("golden-hashes.json")
}

fn hex64(v: u64) -> String {
    format!("{v:#018x}")
}

fn parse_hex64(v: &Value, what: &str) -> Result<u64, String> {
    let s = v
        .as_str()
        .ok_or_else(|| format!("{what}: expected a hex string"))?;
    u64::from_str_radix(s.trim_start_matches("0x"), 16).map_err(|e| format!("{what}: {e}"))
}

/// Rolling hash checkpoints of a per-tick hash sequence (see [`Entry::checkpoints`]).
pub fn checkpoints(hashes: &[u64]) -> Vec<u32> {
    let mut r = RollingHash::new();
    let mut out = Vec::new();
    for (i, &h) in hashes.iter().enumerate() {
        let v = r.push(h);
        if (i + 1) % CHECKPOINT_INTERVAL == 0 || i + 1 == hashes.len() {
            out.push((v >> 32) as u32);
        }
    }
    out
}

/// The golden entry for a replayed scenario.
pub fn entry_for(report: &Report) -> Entry {
    let sim = report.sim_rolling();
    let oracle = report.oracle_rolling();
    Entry {
        ticks: report.ticks,
        oracle,
        checkpoints: checkpoints(&report.oracle_hashes),
        sim: Some(sim),
        frozen: sim == oracle,
    }
}

/// The golden entry computed from the recording alone (no kernel).
pub fn oracle_entry(scenario: &Scenario) -> Entry {
    let hashes = oracle_hashes(scenario);
    Entry {
        ticks: hashes.len(),
        oracle: rolling(&hashes),
        checkpoints: checkpoints(&hashes),
        sim: None,
        frozen: false,
    }
}

impl Golden {
    /// Load the committed file; `Ok(None)` if it does not exist.
    pub fn load() -> Result<Option<Golden>, String> {
        let path = golden_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).map(Some),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    pub fn parse(text: &str) -> Result<Golden, String> {
        let root: Value = serde_json::from_str(text).map_err(|e| format!("golden-hashes: {e}"))?;
        let contract = root["contract"]
            .as_u64()
            .ok_or("golden-hashes: missing `contract`")? as u8;
        let hash_seed = parse_hex64(&root["hash_seed"], "hash_seed")?;
        let locked = root["locked"].as_bool().unwrap_or(false);
        let mut entries = BTreeMap::new();
        for (name, e) in root["scenarios"]
            .as_object()
            .ok_or("golden-hashes: missing `scenarios`")?
        {
            let ticks = e["ticks"]
                .as_u64()
                .ok_or_else(|| format!("{name}: missing `ticks`"))?
                as usize;
            let oracle = parse_hex64(&e["oracle"], &format!("{name}.oracle"))?;
            let checkpoints = e["checkpoints"]
                .as_str()
                .ok_or_else(|| format!("{name}: missing `checkpoints`"))?
                .split_whitespace()
                .map(|h| u32::from_str_radix(h, 16).map_err(|x| format!("{name}.checkpoints: {x}")))
                .collect::<Result<Vec<_>, _>>()?;
            let sim = match e.get("sim") {
                Some(v) => Some(parse_hex64(v, &format!("{name}.sim"))?),
                None => None,
            };
            let frozen = e["frozen"].as_bool().unwrap_or(false);
            if frozen && sim != Some(oracle) {
                return Err(format!("{name}: frozen but sim != oracle"));
            }
            entries.insert(
                name.clone(),
                Entry {
                    ticks,
                    oracle,
                    checkpoints,
                    sim,
                    frozen,
                },
            );
        }
        Ok(Golden {
            contract,
            hash_seed,
            locked,
            entries,
        })
    }

    /// The file's text (stable: sorted keys, two-space indent, trailing newline).
    pub fn to_json(&self) -> String {
        let mut scenarios = Map::new();
        for (name, e) in &self.entries {
            let mut o = Map::new();
            o.insert("ticks".into(), json!(e.ticks));
            o.insert("frozen".into(), json!(e.frozen));
            o.insert("oracle".into(), json!(hex64(e.oracle)));
            if let Some(sim) = e.sim {
                o.insert("sim".into(), json!(hex64(sim)));
            }
            let cps: Vec<String> = e.checkpoints.iter().map(|c| format!("{c:08x}")).collect();
            o.insert("checkpoints".into(), json!(cps.join(" ")));
            scenarios.insert(name.clone(), Value::Object(o));
        }
        let root = json!({
            "_comment": "Golden hash lock (docs/contract.md section 5). Generated by `cargo xtask oracle --bless`; do not edit by hand.",
            "contract": self.contract,
            "hash_seed": hex64(self.hash_seed),
            "checkpoint_interval": CHECKPOINT_INTERVAL,
            "locked": self.locked,
            "scenarios": Value::Object(scenarios),
        });
        let mut text = serde_json::to_string_pretty(&root).expect("serializable");
        text.push('\n');
        text
    }

    pub fn save(&self) -> Result<(), String> {
        let path = golden_path();
        std::fs::write(&path, self.to_json()).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// The result of checking one scenario against its golden entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Verdict {
    /// Reasons the lock is broken (the test fails if any).
    pub failures: Vec<String>,
    /// Informational lines (e.g. a scenario that became exact and can be frozen).
    pub notes: Vec<String>,
}

/// Check a replay of one scenario against `golden` (see the module docs for the rules):
///
/// - the recorded corpus must still fingerprint to the golden value;
/// - a frozen scenario must reproduce the recording hash-for-hash, and a failure names the first
///   diverging tick and field;
/// - an unfrozen scenario is checked against its blessed simulated hash only in `strict` mode, and
///   a scenario that has become exact is reported so it can be frozen.
pub fn verify(golden: &Golden, report: &Report, strict: bool) -> Verdict {
    let name = &report.scenario;
    let mut v = Verdict::default();
    let Some(e) = golden.entries.get(name) else {
        let msg = format!(
            "{name}: no golden entry; run `cargo xtask oracle --bless` and commit corpus/golden-hashes.json"
        );
        if golden.locked {
            v.failures.push(msg);
        } else {
            v.notes.push(msg);
        }
        return v;
    };
    if report.ticks != e.ticks {
        v.failures.push(format!(
            "{name}: the corpus has {} ticks, the golden file says {}; the recording changed — re-bless",
            report.ticks, e.ticks
        ));
        return v;
    }
    let oracle = report.oracle_rolling();
    if oracle != e.oracle {
        let now = checkpoints(&report.oracle_hashes);
        v.failures.push(format!(
            "{name}: the recorded states no longer hash to the golden value ({} vs {}): the corpus file or the contract serialization changed{}; re-bless only if that was intended",
            hex64(oracle),
            hex64(e.oracle),
            window(&now, &e.checkpoints, report.ticks),
        ));
        return v;
    }
    let sim = report.sim_rolling();
    if e.frozen {
        if sim != e.oracle {
            let now = checkpoints(&report.sim_hashes);
            let mut msg = format!(
                "{name}: frozen scenario no longer reproduces the recording (rolling hash {} vs {}){}",
                hex64(sim),
                hex64(e.oracle),
                window(&now, &e.checkpoints, report.ticks),
            );
            if let Some(d) = &report.first_divergence {
                msg.push_str(&format!("; first divergence {d}"));
            } else if let Some(t) = report.first_hash_divergence() {
                msg.push_str(&format!("; first differing state hash at tick {t}"));
            }
            v.failures.push(msg);
        }
    } else {
        if sim == e.oracle {
            v.notes.push(format!(
                "{name}: now bit-exact over {} ticks; run `cargo xtask oracle --bless` to freeze it",
                report.ticks
            ));
        }
        if strict {
            match e.sim {
                Some(blessed) if blessed == sim => {}
                Some(blessed) => v.failures.push(format!(
                    "{name}: simulated hash changed ({} vs blessed {}); strict mode requires re-blessing",
                    hex64(sim),
                    hex64(blessed)
                )),
                None => v.failures.push(format!(
                    "{name}: no blessed simulated hash; run `cargo xtask oracle --bless`"
                )),
            }
        }
    }
    v
}

/// ", the first changed window is ticks a..=b" given freshly computed and stored checkpoints.
fn window(now: &[u32], stored: &[u32], ticks: usize) -> String {
    let i = (0..now.len().max(stored.len())).find(|&i| now.get(i) != stored.get(i));
    match i {
        Some(i) => {
            let a = i * CHECKPOINT_INTERVAL;
            let b = ((i + 1) * CHECKPOINT_INTERVAL).min(ticks).saturating_sub(1);
            format!(", first changed window is ticks {a}..={b}")
        }
        None => String::new(),
    }
}

/// Verify every corpus scenario against the committed file, replaying through the kernel.
/// Returns the failures and notes; with no golden file nothing fails.
pub fn verify_all(strict: bool) -> Result<Verdict, String> {
    check_scenarios(
        Golden::load()?.as_ref(),
        &client_scenarios(),
        strict,
        &replay_kernel,
    )
}

/// [`verify_all`] over an explicit scenario list, golden file and replay function (the lock's
/// whole logic, without file or kernel dependencies).
pub fn check_scenarios(
    golden: Option<&Golden>,
    names: &[String],
    strict: bool,
    replay: &dyn Fn(&Scenario) -> Report,
) -> Result<Verdict, String> {
    let mut total = Verdict::default();
    let Some(golden) = golden else {
        total.notes.push(
            "corpus/golden-hashes.json does not exist yet: nothing is locked. Run `cargo xtask oracle --bless` once the kernel is exact and commit the file".to_string(),
        );
        return Ok(total);
    };
    if golden.contract != CONTRACT_VERSION || golden.hash_seed != HASH_SEED {
        total.failures.push(format!(
            "golden file is for contract {} / seed {}, the code is contract {} / seed {}: a new contract version requires re-blessing every hash",
            golden.contract,
            hex64(golden.hash_seed),
            CONTRACT_VERSION,
            hex64(HASH_SEED)
        ));
        return Ok(total);
    }
    for name in names {
        let scenario = Scenario::load(name)?;
        let v = verify(golden, &replay(&scenario), strict);
        total.failures.extend(v.failures);
        total.notes.extend(v.notes);
    }
    for name in golden.entries.keys() {
        if !names.contains(name) {
            total.failures.push(format!(
                "{name}: golden entry for a scenario that is not in corpus/client; re-bless to drop it"
            ));
        }
    }
    Ok(total)
}

/// What a bless did.
#[derive(Clone, Debug, Default)]
pub struct BlessSummary {
    pub scenarios: usize,
    pub frozen: usize,
    pub locked: bool,
}

/// (Re)write `corpus/golden-hashes.json`. With `names` empty every corpus scenario is refreshed
/// (and entries for scenarios no longer in the corpus are dropped); otherwise only the named ones.
/// `oracle_only` records corpus fingerprints without running the kernel and keeps any existing
/// simulated hashes. The file becomes `locked` when every corpus scenario has a simulated hash.
pub fn bless(names: &[String], oracle_only: bool) -> Result<BlessSummary, String> {
    let existing = match Golden::load()? {
        Some(g) if g.contract == CONTRACT_VERSION && g.hash_seed == HASH_SEED => g,
        _ => Golden::default(),
    };
    let golden = bless_into(
        existing,
        &client_scenarios(),
        names,
        oracle_only,
        &replay_kernel,
    )?;
    golden.save()?;
    Ok(BlessSummary {
        scenarios: golden.entries.len(),
        frozen: golden.entries.values().filter(|e| e.frozen).count(),
        locked: golden.locked,
    })
}

/// The pure part of [`bless`]: `all` is every scenario in the corpus, `names` the ones to refresh
/// (empty = all).
pub fn bless_into(
    mut golden: Golden,
    all: &[String],
    names: &[String],
    oracle_only: bool,
    replay: &dyn Fn(&Scenario) -> Report,
) -> Result<Golden, String> {
    let targets: Vec<String> = if names.is_empty() {
        all.to_vec()
    } else {
        for n in names {
            if !all.contains(n) {
                return Err(format!("unknown scenario `{n}`"));
            }
        }
        names.to_vec()
    };
    if names.is_empty() {
        golden.entries.retain(|k, _| all.contains(k));
    }
    for name in &targets {
        let scenario = Scenario::load(name)?;
        let entry = if oracle_only {
            let mut fresh = oracle_entry(&scenario);
            // Keep a previous simulated hash only if the recording it was taken against is
            // unchanged.
            if let Some(old) = golden.entries.get(name) {
                if old.oracle == fresh.oracle {
                    fresh.sim = old.sim;
                    fresh.frozen = old.frozen;
                }
            }
            fresh
        } else {
            entry_for(&replay(&scenario))
        };
        golden.entries.insert(name.clone(), entry);
    }
    golden.locked = all
        .iter()
        .all(|n| golden.entries.get(n).is_some_and(|e| e.sim.is_some()));
    Ok(golden)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(name: &str, sim: &[u64], oracle: &[u64]) -> Report {
        Report {
            scenario: name.to_string(),
            mode: Mode::FreeRun,
            ticks: oracle.len(),
            exact_ticks: 0,
            longest_streak: 0,
            longest_streak_start: 0,
            first_divergence: None,
            diffs: vec![Vec::new(); oracle.len()],
            sim_hashes: sim.to_vec(),
            oracle_hashes: oracle.to_vec(),
        }
    }

    fn golden_for(r: &Report, frozen_override: Option<bool>) -> Golden {
        let mut g = Golden::default();
        let mut e = entry_for(r);
        if let Some(f) = frozen_override {
            e.frozen = f;
        }
        g.entries.insert(r.scenario.clone(), e);
        g
    }

    #[test]
    fn json_round_trips() {
        let oracle: Vec<u64> = (0..40u64)
            .map(|i| i.wrapping_mul(0x9e37_79b9_7f4a_7c15))
            .collect();
        let r = report("a", &oracle, &oracle);
        let mut g = golden_for(&r, None);
        g.locked = true;
        g.entries.insert(
            "b".into(),
            Entry {
                ticks: 3,
                oracle: 7,
                checkpoints: vec![0xdead_beef],
                sim: None,
                frozen: false,
            },
        );
        let text = g.to_json();
        assert!(text.ends_with("}\n"));
        assert_eq!(Golden::parse(&text).unwrap(), g);
        // Key order is stable: scenarios sorted by name.
        assert!(text.find("\"a\"").unwrap() < text.find("\"b\"").unwrap());
    }

    #[test]
    fn checkpoints_cover_every_window_and_the_tail() {
        assert_eq!(checkpoints(&[]).len(), 0);
        assert_eq!(checkpoints(&[1; 16]).len(), 1);
        assert_eq!(checkpoints(&[1; 17]).len(), 2);
        assert_eq!(checkpoints(&[1; 33]).len(), 3);
        // The last checkpoint is the high half of the final rolling value.
        let h: Vec<u64> = (1..=20).collect();
        assert_eq!(*checkpoints(&h).last().unwrap(), (rolling(&h) >> 32) as u32);
    }

    #[test]
    fn matching_replay_passes_and_unfrozen_exactness_is_noted() {
        let oracle: Vec<u64> = (1..=40).collect();
        let r = report("s", &oracle, &oracle);
        let v = verify(&golden_for(&r, Some(true)), &r, false);
        assert_eq!(v, Verdict::default());
        let v = verify(&golden_for(&r, Some(false)), &r, false);
        assert!(v.failures.is_empty());
        assert!(v.notes[0].contains("now bit-exact"), "{:?}", v.notes);
    }

    #[test]
    fn a_perturbed_frozen_scenario_fails_with_the_window() {
        let oracle: Vec<u64> = (1..=40).collect();
        let good = report("s", &oracle, &oracle);
        let golden = golden_for(&good, None);
        assert!(golden.entries["s"].frozen);
        let mut sim = oracle.clone();
        sim[20] ^= 1;
        let mut bad = report("s", &sim, &oracle);
        bad.first_divergence = Some(crate::Divergence {
            tick: 20,
            field: "dy".into(),
            expected: "1".into(),
            actual: "2".into(),
            differing: 1,
        });
        let v = verify(&golden, &bad, false);
        assert_eq!(v.failures.len(), 1, "{v:?}");
        let msg = &v.failures[0];
        assert!(msg.contains("frozen"), "{msg}");
        assert!(msg.contains("ticks 16..=31"), "{msg}");
        assert!(msg.contains("t=20 dy"), "{msg}");
    }

    #[test]
    fn a_changed_corpus_fails_even_for_unfrozen_scenarios() {
        let oracle: Vec<u64> = (1..=40).collect();
        let r = report("s", &[9; 40], &oracle);
        let golden = golden_for(&r, None);
        assert!(!golden.entries["s"].frozen);
        let mut other = oracle.clone();
        other[39] += 1;
        let changed = report("s", &[9; 40], &other);
        let v = verify(&golden, &changed, false);
        assert_eq!(v.failures.len(), 1, "{v:?}");
        assert!(
            v.failures[0].contains("recorded states"),
            "{:?}",
            v.failures
        );
        assert!(v.failures[0].contains("ticks 32..=39"), "{:?}", v.failures);
        // A different tick count is caught before hashing.
        let shorter = report("s", &[9; 30], &oracle[..30]);
        assert!(verify(&golden, &shorter, false).failures[0].contains("ticks"));
    }

    #[test]
    fn unfrozen_scenarios_are_only_pinned_in_strict_mode() {
        let oracle: Vec<u64> = (1..=40).collect();
        let blessed = report("s", &[9; 40], &oracle);
        let golden = golden_for(&blessed, None);
        let moved = report("s", &[8; 40], &oracle);
        assert!(verify(&golden, &moved, false).failures.is_empty());
        assert_eq!(verify(&golden, &moved, true).failures.len(), 1);
        assert!(verify(&golden, &blessed, true).failures.is_empty());
    }

    #[test]
    fn missing_entries_pass_unless_the_file_is_locked() {
        let oracle: Vec<u64> = (1..=5).collect();
        let r = report("new_scenario", &oracle, &oracle);
        let mut g = Golden::default();
        let v = verify(&g, &r, false);
        assert!(v.failures.is_empty());
        assert!(v.notes[0].contains("--bless"));
        g.locked = true;
        assert_eq!(verify(&g, &r, false).failures.len(), 1);
    }

    #[test]
    fn frozen_entries_must_be_exact_when_parsed() {
        let text = r#"{"contract":1,"hash_seed":"0x4d494e4553494d00","locked":false,
            "scenarios":{"x":{"ticks":1,"frozen":true,"oracle":"0x01","sim":"0x02","checkpoints":"00000000"}}}"#;
        assert!(Golden::parse(text).unwrap_err().contains("frozen"));
    }

    // ---- the whole lock lifecycle, on real scenarios, with stand-in kernels

    use crate::canonical::state_from_fields;
    use crate::replay;

    fn names() -> Vec<String> {
        vec!["walk_basic".to_string(), "fall_damage_4".to_string()]
    }

    /// A kernel that reproduces the recording, optionally corrupting one tick of one scenario.
    fn kernel(bad: Option<(&'static str, usize, f64)>) -> impl Fn(&Scenario) -> Report {
        move |s: &Scenario| {
            let mut row = 0usize;
            replay(s, |p, _input, _world| {
                *p = state_from_fields(&s.rows[row].post);
                if let Some((name, tick, dx)) = bad {
                    if s.name == name && row == tick {
                        p.pos.x += dx;
                    }
                }
                row += 1;
            })
        }
    }

    #[test]
    fn lifecycle_oracle_only_then_full_bless_then_perturbation() {
        let all = names();
        // Oracle-only bless: fingerprints, no simulated hashes, lock not armed.
        let g = bless_into(Golden::default(), &all, &[], true, &kernel(None)).unwrap();
        assert!(!g.locked);
        assert!(g.entries.values().all(|e| e.sim.is_none() && !e.frozen));
        let v = check_scenarios(Some(&g), &all, false, &kernel(None)).unwrap();
        assert!(v.failures.is_empty(), "{v:?}");
        // The kernel is (here) exact, so the notes tell us to freeze.
        assert_eq!(v.notes.len(), 2, "{v:?}");

        // Full bless freezes every exact scenario and arms the lock.
        let g = bless_into(g, &all, &[], false, &kernel(None)).unwrap();
        assert!(g.locked);
        assert!(g
            .entries
            .values()
            .all(|e| e.frozen && e.sim == Some(e.oracle)));
        let reloaded = Golden::parse(&g.to_json()).unwrap();
        assert_eq!(reloaded, g);
        let v = check_scenarios(Some(&reloaded), &all, true, &kernel(None)).unwrap();
        assert_eq!(v, Verdict::default());

        // A one-tick, tiny perturbation of a frozen scenario breaks the lock and the message
        // pins the window, tick and field.
        let v = check_scenarios(
            Some(&g),
            &all,
            false,
            &kernel(Some(("walk_basic", 37, 1e-12))),
        )
        .unwrap();
        assert_eq!(v.failures.len(), 1, "{v:?}");
        assert!(v.failures[0].starts_with("walk_basic:"), "{v:?}");
        assert!(v.failures[0].contains("ticks 32..=47"), "{v:?}");
        assert!(v.failures[0].contains("t=37 x"), "{v:?}");

        // A scenario missing from a locked file fails; an extra one fails too.
        let mut missing = g.clone();
        missing.entries.remove("fall_damage_4");
        let v = check_scenarios(Some(&missing), &all, false, &kernel(None)).unwrap();
        assert_eq!(v.failures.len(), 1, "{v:?}");
        let v = check_scenarios(Some(&g), &all[..1], false, &kernel(None)).unwrap();
        assert_eq!(v.failures.len(), 1, "{v:?}");
        assert!(v.failures[0].contains("not in corpus"));
    }

    #[test]
    fn partial_bless_refreshes_only_the_named_scenarios() {
        let all = names();
        let g = bless_into(Golden::default(), &all, &[], false, &kernel(None)).unwrap();
        // Re-bless one scenario with a broken kernel: it becomes unfrozen, the other stays frozen.
        let broken = kernel(Some(("walk_basic", 5, 1.0)));
        let g2 = bless_into(g.clone(), &all, &["walk_basic".to_string()], false, &broken).unwrap();
        assert!(!g2.entries["walk_basic"].frozen);
        assert!(g2.entries["fall_damage_4"].frozen);
        assert!(g2.locked, "every scenario still has a simulated hash");
        // The unfrozen scenario no longer fails the lock for kernel changes (non-strict)...
        let v = check_scenarios(Some(&g2), &all, false, &kernel(None)).unwrap();
        assert!(v.failures.is_empty(), "{v:?}");
        assert!(v.notes.iter().any(|n| n.contains("now bit-exact")));
        // ...but strict mode pins its blessed simulated hash.
        let v = check_scenarios(Some(&g2), &all, true, &kernel(None)).unwrap();
        assert_eq!(v.failures.len(), 1, "{v:?}");
        assert!(bless_into(g, &all, &["nope".to_string()], false, &broken).is_err());
    }

    #[test]
    fn oracle_only_bless_keeps_matching_simulated_hashes() {
        let all = names();
        let g = bless_into(Golden::default(), &all, &[], false, &kernel(None)).unwrap();
        let again = bless_into(g.clone(), &all, &[], true, &kernel(None)).unwrap();
        assert_eq!(
            again, g,
            "oracle-only bless of an unchanged corpus is a no-op"
        );
    }

    #[test]
    fn a_golden_file_for_another_contract_fails_loudly() {
        let g = Golden {
            contract: 2,
            ..Golden::default()
        };
        let v = check_scenarios(Some(&g), &names(), false, &kernel(None)).unwrap();
        assert_eq!(v.failures.len(), 1);
        assert!(v.failures[0].contains("new contract version"));
    }
}
