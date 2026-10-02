//! The golden-hash lock and the checks that make the state hash trustworthy.
//!
//! `golden_lock_holds` replays every corpus scenario through `ms_kernel::player::tick` and checks
//! the rolling hash of the simulated states against `corpus/golden-hashes.json` (see
//! `docs/corpus.md`). While the file lacks an entry the test passes and prints how to bless one;
//! once `cargo xtask oracle --bless` has armed the lock, a missing entry fails too.

use ms_corpus::canonical::{fields_bytes, fields_hash, state_from_fields};
use ms_corpus::golden::{
    entry_for, oracle_entry, replay_kernel, verify_all, Golden, CHECKPOINT_INTERVAL, STRICT_ENV,
};
use ms_corpus::{client_scenarios, Scenario};
use ms_oracle::player::{pack_block_pos, player_bytes, player_hash, LAYOUT};

fn strict() -> bool {
    std::env::var(STRICT_ENV).is_ok_and(|v| v != "0" && !v.is_empty())
}

#[test]
fn golden_lock_holds() {
    let verdict = verify_all(strict()).expect("golden-hashes.json is readable");
    for note in &verdict.notes {
        eprintln!("golden: {note}");
    }
    assert!(
        verdict.failures.is_empty(),
        "golden lock broken:\n  {}\nIf the change is intended, re-bless with `cargo xtask oracle --bless` \
         and commit corpus/golden-hashes.json (docs/corpus.md, freeze rule).",
        verdict.failures.join("\n  ")
    );
}

/// The recorded corpus (and the contract serialization applied to it) still fingerprint to the
/// committed values. Independent of the kernel, so a failure here means the corpus or the
/// serialization moved, never the physics.
#[test]
fn corpus_matches_its_committed_fingerprints() {
    let Some(golden) = Golden::load().expect("readable") else {
        eprintln!("golden: no corpus/golden-hashes.json yet; `cargo xtask oracle --bless --oracle-only` writes the corpus fingerprints");
        return;
    };
    for name in client_scenarios() {
        let Some(e) = golden.entries.get(&name) else {
            assert!(!golden.locked, "{name}: no golden entry in a locked file");
            continue;
        };
        let fresh = oracle_entry(&Scenario::load(&name).unwrap());
        assert_eq!(fresh.ticks, e.ticks, "{name}: tick count");
        assert_eq!(
            fresh.oracle, e.oracle,
            "{name}: recorded states no longer fingerprint to the golden value"
        );
        assert_eq!(fresh.checkpoints, e.checkpoints, "{name}: checkpoints");
    }
}

#[test]
fn golden_file_is_consistent_with_the_corpus() {
    let Some(golden) = Golden::load().expect("readable") else {
        return;
    };
    assert_eq!(golden.contract, ms_oracle::player::CONTRACT_VERSION);
    assert_eq!(golden.hash_seed, ms_oracle::HASH_SEED);
    let names = client_scenarios();
    for (name, e) in &golden.entries {
        assert!(
            names.contains(name),
            "{name}: golden entry without a scenario"
        );
        assert_eq!(
            e.checkpoints.len(),
            e.ticks.div_ceil(CHECKPOINT_INTERVAL),
            "{name}: checkpoint count"
        );
        if e.frozen {
            assert_eq!(e.sim, Some(e.oracle), "{name}: frozen means sim == oracle");
        }
    }
    if golden.locked {
        for n in &names {
            assert!(
                golden.entries.get(n).is_some_and(|e| e.sim.is_some()),
                "{n}: locked file without a simulated hash"
            );
        }
    }
}

/// The two independent serializers (simulator state → bytes in `ms-oracle`, recorded fields →
/// bytes in `ms-corpus`) agree on every recorded state of every scenario, so a bit-exact
/// simulation has exactly the recorded hashes.
#[test]
fn simulator_and_recording_serialize_identically_on_every_recorded_state() {
    let mut states = 0usize;
    for name in client_scenarios() {
        let s = Scenario::load(&name).unwrap();
        let mut check = |what: &str, f: &ms_corpus::Fields| {
            let from_fields = fields_bytes(f).unwrap_or_else(|e| panic!("{name} {what}: {e}"));
            let p = state_from_fields(f);
            let from_state = player_bytes(&p);
            assert_eq!(from_state, from_fields, "{name} {what}");
            assert_eq!(player_hash(&p), fields_hash(f).unwrap(), "{name} {what}");
            states += 1;
        };
        check("row 0 pre", &s.rows[0].pre);
        for row in &s.rows {
            check(&format!("row {} post", row.t), &row.post);
        }
    }
    assert!(states > 8000, "only {states} states checked");
}

/// The tick counter is part of the hashed state, so even a player standing still hashes
/// differently every tick.
#[test]
fn consecutive_recorded_states_never_hash_alike() {
    for name in client_scenarios() {
        let s = Scenario::load(&name).unwrap();
        let hashes = ms_corpus::replay::oracle_hashes(&s);
        assert!(
            hashes.windows(2).all(|w| w[0] != w[1]),
            "{name}: two consecutive ticks have the same hash"
        );
    }
}

/// Every key of the frozen layout appears in every recorded post state (so a layout field can
/// never silently default), except the documented optional `support`.
#[test]
fn recorded_states_carry_every_layout_field() {
    for name in client_scenarios() {
        let s = Scenario::load(&name).unwrap();
        for row in &s.rows {
            for (field, _) in LAYOUT {
                if *field == "support" {
                    continue;
                }
                assert!(
                    row.post.contains_key(*field),
                    "{name} row {}: missing `{field}`",
                    row.t
                );
            }
        }
    }
}

#[test]
fn block_pos_packing_matches_the_world_crate() {
    for (x, y, z) in [
        (0, 0, 0),
        (4, -63, 0),
        (-1, -64, -1),
        (-30_000_000, 319, 29_999_999),
        (12, 200, -7),
    ] {
        assert_eq!(
            pack_block_pos(x, y, z),
            ms_world::coords::BlockPos::new(x, y, z).as_long(),
            "({x}, {y}, {z})"
        );
    }
}

/// With whatever kernel is currently in the tree, every scenario replays without panicking and the
/// report accounts for every tick. (Whether the kernel is exact is `golden_lock_holds`' concern.)
#[test]
fn every_scenario_replays_through_the_kernel() {
    for name in client_scenarios() {
        let s = Scenario::load(&name).unwrap();
        let r = replay_kernel(&s);
        assert_eq!(r.ticks, s.rows.len(), "{name}");
        assert_eq!(r.diffs.len(), r.ticks, "{name}");
        assert_eq!(r.sim_hashes.len(), r.ticks, "{name}");
        assert_eq!(
            r.exact_ticks,
            r.diffs.iter().filter(|d| d.is_empty()).count(),
            "{name}"
        );
        assert!(r.longest_streak <= r.exact_ticks, "{name}");
        // The golden entry derived from a replay is internally consistent.
        let e = entry_for(&r);
        assert_eq!(e.frozen, r.sim_rolling() == r.oracle_rolling(), "{name}");
        if r.is_exact() {
            assert_eq!(
                r.hash_exact_ticks(),
                r.ticks,
                "{name}: exact but hashes differ"
            );
        }
    }
}
