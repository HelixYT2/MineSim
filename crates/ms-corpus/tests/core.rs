//! The core player tick against the recorded 1.21.11 client: for every tick of a scenario, apply what
//! the server changed since the previous tick (the row's `pre` diff), run [`ms_kernel::player::tick`]
//! with the row's input, and compare the complete resulting state with the recorded `post`, field
//! by field and bit for bit.
//!
//! `cargo test -p ms-corpus --test core -- --nocapture` prints the per-scenario tally and, for any
//! scenario that diverges, the first diverging ticks.

use ms_corpus::{apply_pre, apply_state, compare_state, FieldDiff, Scenario};
use ms_kernel::player::tick;
use std::collections::BTreeMap;

/// Fields written by other subsystems' modules (status effects, health and damage timers, attribute
/// values): they are replayed from the recording (`pre` diffs), not checked here. For the land
/// scenarios below none of them changes during the scenario except what the player's own sprinting
/// does to the movement speed (which `speed` still checks).
const OTHER_MODULES: &[&str] = &[
    "effects",
    "health",
    "absorption",
    "hurtTime",
    "lastHurt",
    "deathTime",
    "invul",
    // Attribute values are the attribute module's (its base for jump strength is 0.42 where the
    // game's is the float 0.42f widened); the movement-speed attribute is checked through `speed`.
    "attrs",
];

/// The land-movement scenarios the core tick must reproduce exactly.
const CORE: &[&str] = &[
    "walk_basic",
    "sprint_jump",
    "sprint_rules",
    "sprint_hungry",
    "sneak_edges",
    "crouch_tunnel",
    "steps_course",
    "wall_slide",
    "ledge_fall",
    "ice_slip",
];

struct Report {
    name: String,
    ticks: usize,
    exact: usize,
    first_ticks: Vec<(usize, Vec<FieldDiff>)>,
    by_field: BTreeMap<String, usize>,
}

/// Replays a scenario. With `resync`, the state is overwritten with the recorded end of the previous
/// tick before every tick (so each tick is judged on its own, and one wrong tick cannot spoil the
/// ones after it); otherwise the simulation free-runs, taking only the server's `pre` changes.
fn replay_with(name: &str, resync: bool) -> Report {
    let scenario = Scenario::load(name).unwrap();
    let world = scenario.world();
    let mut p = scenario.initial_state();
    let mut report = Report {
        name: name.to_string(),
        ticks: scenario.rows.len(),
        exact: 0,
        first_ticks: Vec::new(),
        by_field: BTreeMap::new(),
    };
    for (i, row) in scenario.rows.iter().enumerate() {
        if resync && i > 0 {
            let prev = &scenario.rows[i - 1].post;
            apply_state(&mut p, prev);
            // A complete state without a `support` entry means there is no supporting block.
            if !prev.contains_key("support") {
                p.supporting_block = None;
            }
        }
        let prev = if i == 0 {
            None
        } else {
            Some(&scenario.rows[i - 1].post)
        };
        apply_pre(&mut p, &row.pre, prev);
        tick(&mut p, &row.input, &world);
        let diffs: Vec<FieldDiff> = compare_state(&p, &row.post)
            .into_iter()
            .filter(|d| !OTHER_MODULES.contains(&d.field.as_str()))
            .collect();
        if diffs.is_empty() {
            report.exact += 1;
        } else {
            for d in &diffs {
                *report.by_field.entry(d.field.clone()).or_default() += 1;
            }
            if report.first_ticks.len() < 4 {
                report.first_ticks.push((i, diffs));
            }
        }
    }
    report
}

fn replay(name: &str) -> Report {
    replay_with(name, false)
}

fn print(r: &Report) {
    println!("{:<20} {:>4}/{:<4} ticks exact", r.name, r.exact, r.ticks);
    if r.exact != r.ticks {
        println!("    diverging fields (ticks): {:?}", r.by_field);
        for (t, diffs) in &r.first_ticks {
            println!("    tick {t}:");
            for d in diffs.iter().take(8) {
                println!(
                    "      {:<12} expected {} got {}",
                    d.field, d.expected, d.actual
                );
            }
        }
    }
}

#[test]
fn core_scenarios_are_bit_exact() {
    let mut failures = Vec::new();
    for name in CORE {
        let r = replay(name);
        print(&r);
        if r.exact != r.ticks {
            failures.push(format!("{name}: {}/{}", r.exact, r.ticks));
        }
    }
    assert!(failures.is_empty(), "diverging scenarios: {failures:?}");
}

/// The same scenarios with every tick judged on its own (resynchronised to the recording first).
#[test]
fn core_scenarios_are_bit_exact_per_tick() {
    for name in CORE {
        let r = replay_with(name, true);
        assert_eq!(r.exact, r.ticks, "{name}: {:?}", r.first_ticks);
    }
}

/// Every other recorded scenario mixes in subsystems owned by other modules (fluids, climbing, block
/// effects, slime bounces, damage, effects): reported, not asserted.
#[test]
fn report_remaining_scenarios() {
    for name in ms_corpus::client_scenarios() {
        if CORE.contains(&name.as_str()) {
            continue;
        }
        print(&replay(&name));
    }
}

/// The same, resynchronised before every tick: the per-tick view of the scenarios that other
/// modules' stubs spoil in free-run. Reported, not asserted.
#[test]
fn report_remaining_scenarios_per_tick() {
    for name in ms_corpus::client_scenarios() {
        if CORE.contains(&name.as_str()) {
            continue;
        }
        print(&replay_with(&name, true));
    }
}
