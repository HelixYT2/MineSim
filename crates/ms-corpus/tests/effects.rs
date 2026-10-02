//! Status effects and attributes against the oracle corpus.
//!
//! Two replays of every scenario, each driven only by the recorded `pre` diffs (what the server
//! changed between client ticks), the recorded sprint flag and the recorded inputs' effect on it:
//!
//! * the **client replay** applies effect changes the way the client does: effect packets change the
//!   effect list only ([`effects::client_sync_effects`]), a recorded change of the `attrs` block is
//!   an attribute-update packet ([`effects::client_sync_attributes`]), durations count down with
//!   [`effects::client_tick_effects`]. It must match the recording exactly on every row.
//! * the **server-view replay** applies the same effect changes through `force_add_effect` /
//!   `remove_effect` (attribute modifiers applied at once) and counts down with `tick_effects`. It
//!   matches everywhere except the rows where the recorded client's attribute packet arrived a tick
//!   after its effect packet.
//!
//! Per row: apply the `pre` diff, check that the attributes equal the recorded start-of-tick values,
//! apply the tick's sprint change through `set_sprinting`, count the durations down once, and check
//! the effects and every attribute value against the recorded `post`, bit for bit.

use ms_corpus::{client_scenarios, Fields, Scenario};
use ms_kernel::attributes::Attribute;
use ms_kernel::effects::{self, EffectInstance};
use ms_kernel::PlayerState;
use ms_numerics::Vec3;
use serde_json::Value;
use std::collections::BTreeMap;

/// The scenarios whose effects/attributes the module must reproduce exactly.
const REQUIRED: &[&str] = &[
    "effect_speed",
    "effect_jump_boost",
    "effect_slow_falling",
    "effect_levitation",
    "legacy_capture",
    "lava_pool",
    "sprint_rules",
];

fn effects_of(v: &Value) -> Vec<EffectInstance> {
    v.as_array()
        .expect("effects array")
        .iter()
        .map(|x| EffectInstance {
            id: x["id"].as_str().unwrap().to_string(),
            amplifier: x["amp"].as_i64().unwrap() as i32,
            duration: x["dur"].as_i64().unwrap() as i32,
        })
        .collect()
}

fn attrs_of(v: &Value) -> BTreeMap<String, u64> {
    v.as_object()
        .expect("attrs object")
        .iter()
        .map(|(k, b)| (k.clone(), b.as_i64().unwrap() as u64))
        .collect()
}

fn flag(f: &Fields, key: &str) -> Option<bool> {
    f.get(key).map(|v| v.as_i64() == Some(1))
}

fn sorted(mut v: Vec<EffectInstance>) -> Vec<EffectInstance> {
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v
}

fn actual_effects(p: &PlayerState) -> Vec<EffectInstance> {
    sorted(p.effects.iter().cloned().collect())
}

/// The attribute names where `p` differs from the recorded values, as `(name, expected, actual)`.
fn attr_diffs(p: &PlayerState, want: &BTreeMap<String, u64>) -> Vec<(String, f64, f64)> {
    let mut out = Vec::new();
    for a in Attribute::ALL {
        if let Some(&bits) = want.get(a.name()) {
            let got = p.attributes.value(a);
            if got.to_bits() != bits {
                out.push((a.name().to_string(), f64::from_bits(bits), got));
            }
        }
    }
    out
}

#[derive(Default, Debug)]
struct Report {
    rows: usize,
    /// Attribute values compared (pre and post, per row).
    attr_values: usize,
    /// Effect entries compared (id, amplifier and duration).
    effect_entries: usize,
    /// Rows whose recorded `pre` carried an effect or attribute change.
    external_changes: usize,
    /// First few mismatches, as `(row, what)`.
    mismatches: Vec<(usize, String)>,
    mismatch_rows: Vec<usize>,
    /// Which kinds of field ever mismatched: an attribute name, or "effects".
    mismatch_kinds: Vec<String>,
}

impl Report {
    fn note(&mut self, row: usize, kind: &str, what: String) {
        if !self.mismatch_rows.contains(&row) {
            self.mismatch_rows.push(row);
        }
        if !self.mismatch_kinds.iter().any(|k| k == kind) {
            self.mismatch_kinds.push(kind.to_string());
        }
        if self.mismatches.len() < 4 {
            self.mismatches.push((row, what));
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Client,
    Server,
}

fn replay(s: &Scenario, view: View) -> Report {
    let mut p = PlayerState::new(Vec3::ZERO, 0.0);
    let mut rep = Report::default();
    // The recorded attribute values as of the start of the current tick.
    let mut known_attrs: BTreeMap<String, u64> = BTreeMap::new();
    for (r, row) in s.rows.iter().enumerate() {
        rep.rows += 1;
        // --- external changes recorded at the start of the tick
        let server_effects = row.pre.get("effects").map(effects_of);
        let attrs_packet = row.pre.get("attrs");
        let sprint_change = flag(&row.pre, "sprinting");
        if server_effects.is_some() || attrs_packet.is_some() {
            rep.external_changes += 1;
        }
        // The attributes the recorded attribute packet concerns: those whose value changed.
        let mut updated: Vec<Attribute> = Vec::new();
        if let Some(a) = attrs_packet {
            let new = attrs_of(a);
            for attribute in Attribute::ALL {
                if new.get(attribute.name()) != known_attrs.get(attribute.name()) {
                    updated.push(attribute);
                }
            }
            known_attrs = new;
        }
        match view {
            View::Client => effects::client_apply_server_changes(
                &mut p,
                server_effects.as_deref(),
                sprint_change,
                &updated,
            ),
            View::Server => {
                if let Some(server) = &server_effects {
                    let stale: Vec<String> = p
                        .effects
                        .iter()
                        .filter(|x| !server.iter().any(|y| y.id == x.id))
                        .map(|x| x.id.clone())
                        .collect();
                    for id in stale {
                        effects::remove_effect(&mut p, &id);
                    }
                    for e in server {
                        if p.effects.get(&e.id) != Some(e) {
                            effects::force_add_effect(&mut p, &e.id, e.amplifier, e.duration);
                        }
                    }
                }
                if let Some(s) = sprint_change {
                    if s != p.sprinting {
                        effects::set_sprinting(&mut p, s);
                    }
                }
            }
        }
        // --- the attributes at the start of the tick (the server view legitimately differs on the
        // rows where the client has not received the attribute packet yet, which shows at `post`).
        if view == View::Client {
            rep.attr_values += known_attrs.len();
            for (n, want, got) in attr_diffs(&p, &known_attrs) {
                rep.note(
                    r,
                    &n,
                    format!("pre attr {n}: expected {want:e}, got {got:e}"),
                );
            }
        }
        // --- the tick: the sprint decision, then the countdown
        if let Some(sprinting) = flag(&row.post, "sprinting") {
            if sprinting != p.sprinting {
                effects::set_sprinting(&mut p, sprinting);
            }
        }
        match view {
            View::Client => effects::client_tick_effects(&mut p),
            View::Server => effects::tick_effects(&mut p),
        }
        // --- compare with the recorded end of the tick
        let want_effects = sorted(effects_of(&row.post["effects"]));
        rep.effect_entries += want_effects.len();
        let got_effects = actual_effects(&p);
        if got_effects != want_effects {
            rep.note(
                r,
                "effects",
                format!("effects: expected {want_effects:?}, got {got_effects:?}"),
            );
        }
        let want_attrs = attrs_of(&row.post["attrs"]);
        rep.attr_values += want_attrs.len();
        for (n, want, got) in attr_diffs(&p, &want_attrs) {
            rep.note(
                r,
                &n,
                format!("post attr {n}: expected {want:e}, got {got:e}"),
            );
        }
        // The recorded end state is the next tick's start state unless a diff says otherwise.
        known_attrs = want_attrs;
    }
    rep
}

fn print(name: &str, view: &str, rep: &Report) {
    println!(
        "{name:24} {view:7} rows {:4}  attr values {:5}  effect entries {:5}  external changes {:2}  mismatching rows {:?}",
        rep.rows, rep.attr_values, rep.effect_entries, rep.external_changes, rep.mismatch_rows
    );
    for (row, what) in &rep.mismatches {
        println!("    row {row}: {what}");
    }
}

#[test]
fn client_replay_is_bit_exact_on_the_required_scenarios() {
    let mut rows = 0;
    let mut values = 0;
    let mut entries = 0;
    for name in REQUIRED {
        let s = Scenario::load(name).unwrap();
        let rep = replay(&s, View::Client);
        print(name, "client", &rep);
        assert!(
            rep.mismatch_rows.is_empty(),
            "{name}: first mismatches {:?}",
            rep.mismatches
        );
        rows += rep.rows;
        values += rep.attr_values;
        entries += rep.effect_entries;
    }
    println!("required scenarios, client view: {rows} rows, {values} attribute values, {entries} effect entries, all bit-exact");
    assert!(rows > 1000 && values > 20_000 && entries > 1000);
}

/// The rows where the recorded client's attribute packet came one tick after its effect packet, so
/// that an immediately-applied (server view) modifier is one tick early.
fn known_lag_rows(name: &str) -> &'static [usize] {
    match name {
        "effect_speed" => &[151],
        "legacy_capture" => &[11],
        _ => &[],
    }
}

#[test]
fn server_view_replay_differs_only_where_the_attribute_packet_lags() {
    for name in REQUIRED {
        let s = Scenario::load(name).unwrap();
        let rep = replay(&s, View::Server);
        print(name, "server", &rep);
        assert_eq!(
            rep.mismatch_rows,
            known_lag_rows(name),
            "{name}: {:?}",
            rep.mismatches
        );
    }
}

/// Scenarios whose recorded movement speed carries the powder-snow freeze slowdown. That modifier
/// is applied by the *server's* `aiStep` and reaches the client in attribute packets timed by the
/// server's own tick, so it is not derivable from the client-side state; see
/// `effects::try_add_frost`. Everything else in these scenarios (every other attribute, the effects)
/// must still be exact.
const FROST: &[&str] = &["ladder_climb", "powder_snow"];

/// Every other recorded scenario: the sprint modifier and any effects must line up everywhere, apart
/// from the freeze slowdown above.
#[test]
fn other_scenarios_are_exact_apart_from_the_freeze_slowdown() {
    let mut exact = 0;
    let mut total = 0;
    for name in client_scenarios() {
        if REQUIRED.contains(&name.as_str()) {
            continue;
        }
        let s = Scenario::load(&name).unwrap();
        let rep = replay(&s, View::Client);
        print(&name, "client", &rep);
        total += 1;
        if rep.mismatch_rows.is_empty() {
            exact += 1;
        } else {
            assert!(
                FROST.contains(&name.as_str()),
                "{name}: {:?}",
                rep.mismatches
            );
            assert_eq!(
                rep.mismatch_kinds,
                ["movement_speed"],
                "{name}: only the movement speed may differ"
            );
        }
    }
    println!("{exact} of {total} other scenarios reproduce effects and attributes exactly");
    assert_eq!(total - exact, FROST.len());
}
