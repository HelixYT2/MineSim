//! Every committed scenario parses, its arena builds, and its first row describes a complete state.

use ms_corpus::{client_scenarios, compare_state, Scenario};

#[test]
fn every_scenario_loads_and_round_trips_its_initial_state() {
    let names = client_scenarios();
    assert!(!names.is_empty(), "no corpus found under corpus/client");
    for name in names {
        let s = Scenario::load(&name).unwrap();
        assert_eq!(
            s.rows.len(),
            s.header["ticks"].as_u64().unwrap() as usize,
            "{name}"
        );
        let _world = s.world();
        let p = s.initial_state();
        // Writing row 0's full state into a PlayerState and reading it back must be lossless for
        // every compared field except derived attributes.
        let mut pre = s.rows[0].pre.clone();
        pre.remove("attrs");
        let diffs = compare_state(&p, &pre);
        assert!(diffs.is_empty(), "{name}: {diffs:?}");
    }
}
