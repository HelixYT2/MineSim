//! The replay driver: run a scenario through a tick function and measure, tick by tick, how far the
//! simulation agrees with the recording.
//!
//! The recipe is the one in `docs/corpus.md`: initialise from row 0, then for each row apply the
//! recorded `pre` diff (what the server changed in between), run one tick with the row's input,
//! and compare the result with the row's `post`. [`replay`] never re-seeds from the recording
//! beyond those `pre` diffs, so a divergence compounds exactly as it would in a real run.
//! [`replay_with`] with [`Mode::Resync`] additionally overwrites the state with the recorded
//! `post` after every tick that diverged, which measures each tick on its own instead of only the
//! prefix before the first error.

use crate::canonical::fields_hash;
use crate::{apply_state, compare_state, describe, FieldDiff, Fields, Scenario};
use ms_kernel::attributes::Attribute;
use ms_kernel::{Input, PlayerState};
use ms_oracle::player::{field_rank, player_hash, RollingHash};
use ms_world::World;
use serde_json::Value;

/// How the replay treats a tick that diverged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// Keep the simulated state: errors compound (the default; what a real run does).
    #[default]
    FreeRun,
    /// After a tick that diverged, overwrite the state with the recorded `post` (via
    /// [`apply_state`]; attribute modifiers and the server-side velocity copy are the kernel's own
    /// and are kept), so every tick is judged from a correct start.
    Resync,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::FreeRun => "free-run",
            Mode::Resync => "resync",
        }
    }
}

/// The earliest disagreement of a replay: the first tick that differs and, within it, the first
/// differing field in contract order (`ms_oracle::player::LAYOUT`).
#[derive(Clone, Debug, PartialEq)]
pub struct Divergence {
    /// Row index for a client replay; ticks since spawn for a projectile.
    pub tick: usize,
    pub field: String,
    /// The recorded value, decoded (floats from their raw bits).
    pub expected: String,
    /// The simulated value, decoded the same way.
    pub actual: String,
    /// How many fields differ at that tick (including this one).
    pub differing: usize,
}

impl Divergence {
    pub(crate) fn from_diffs(tick: usize, diffs: &[FieldDiff]) -> Option<Self> {
        let d = diffs.first()?;
        Some(Self {
            tick,
            field: d.field.clone(),
            expected: d.expected.clone(),
            actual: d.actual.clone(),
            differing: diffs.len(),
        })
    }
}

impl std::fmt::Display for Divergence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "t={} {}: expected {}, actual {}",
            self.tick, self.field, self.expected, self.actual
        )?;
        if self.differing > 1 {
            write!(f, " (+{} more)", self.differing - 1)?;
        }
        Ok(())
    }
}

/// The outcome of replaying one scenario.
#[derive(Clone, Debug)]
pub struct Report {
    pub scenario: String,
    pub mode: Mode,
    /// Recorded ticks replayed.
    pub ticks: usize,
    /// Ticks whose end state matched the recording in every compared field.
    pub exact_ticks: usize,
    /// Longest run of consecutive exact ticks, and the tick it starts at.
    pub longest_streak: usize,
    pub longest_streak_start: usize,
    pub first_divergence: Option<Divergence>,
    /// Per tick, the fields that differ (empty for an exact tick), in contract field order.
    pub diffs: Vec<Vec<FieldDiff>>,
    /// `H(t)` of the simulated end-of-tick state, per tick.
    pub sim_hashes: Vec<u64>,
    /// `H(t)` of the recorded end-of-tick state, per tick.
    pub oracle_hashes: Vec<u64>,
}

impl Report {
    /// Every tick matched.
    pub fn is_exact(&self) -> bool {
        self.exact_ticks == self.ticks
    }

    /// Ticks at which the simulated and recorded state hashes are equal (contract §3, hash mode).
    pub fn hash_exact_ticks(&self) -> usize {
        self.sim_hashes
            .iter()
            .zip(&self.oracle_hashes)
            .filter(|(a, b)| a == b)
            .count()
    }

    /// The first tick whose simulated hash differs from the recorded one.
    pub fn first_hash_divergence(&self) -> Option<usize> {
        self.sim_hashes
            .iter()
            .zip(&self.oracle_hashes)
            .position(|(a, b)| a != b)
    }

    /// The rolling hash of the simulated states (the golden lock's value).
    pub fn sim_rolling(&self) -> u64 {
        rolling(&self.sim_hashes)
    }

    /// The rolling hash of the recorded states.
    pub fn oracle_rolling(&self) -> u64 {
        rolling(&self.oracle_hashes)
    }
}

/// The rolling hash of a sequence of per-tick hashes.
pub fn rolling(hashes: &[u64]) -> u64 {
    let mut r = RollingHash::new();
    for &h in hashes {
        r.push(h);
    }
    r.value()
}

/// `H(t)` of every recorded end-of-tick state of `scenario`.
pub fn oracle_hashes(scenario: &Scenario) -> Vec<u64> {
    scenario
        .rows
        .iter()
        .map(|row| {
            fields_hash(&row.post)
                .unwrap_or_else(|e| panic!("{} row {}: {e}", scenario.name, row.t))
        })
        .collect()
}

/// Every field of `expected` (a recorded `post`) that `p` does not reproduce bit-for-bit, in the
/// order of the contract's field list.
///
/// This is [`compare_state`] with three refinements: the lifetime tick count `age` is compared (it
/// is part of the hashed state), attributes are reported one by one as `attrs.<name>` with decoded
/// values, and the result is sorted by contract field order so the first entry is the earliest
/// differing field.
pub fn diff_state(p: &PlayerState, expected: &Fields) -> Vec<FieldDiff> {
    let mut diffs: Vec<FieldDiff> = compare_state(p, expected)
        .into_iter()
        .filter(|d| d.field != "attrs")
        .map(|mut d| {
            if d.field == "effects" {
                // Replace the raw JSON with a compact readable list.
                let want = expected.get("effects").and_then(Value::as_array);
                d.expected = effects_text(want.map_or_else(Vec::new, |a| {
                    a.iter()
                        .map(|e| {
                            (
                                e["id"].as_str().unwrap_or("?").to_string(),
                                e["amp"].as_i64().unwrap_or(0),
                                e["dur"].as_i64().unwrap_or(0),
                            )
                        })
                        .collect()
                }));
                d.actual = effects_text(
                    p.effects
                        .iter()
                        .map(|e| (e.id.clone(), i64::from(e.amplifier), i64::from(e.duration)))
                        .collect(),
                );
            }
            d
        })
        .collect();
    if let Some(Value::Object(attrs)) = expected.get("attrs") {
        for a in Attribute::ALL {
            let Some(want) = attrs.get(a.name()).and_then(Value::as_i64) else {
                continue;
            };
            let got = p.attributes.value(a);
            if want as u64 != got.to_bits() {
                diffs.push(FieldDiff {
                    field: format!("attrs.{}", a.name()),
                    expected: format!("{:e}", f64::from_bits(want as u64)),
                    actual: format!("{got:e}"),
                });
            }
        }
    }
    if let Some(age) = expected.get("age").and_then(Value::as_i64) {
        if age != i64::from(p.tick_count) {
            diffs.push(FieldDiff {
                field: "age".into(),
                expected: age.to_string(),
                actual: p.tick_count.to_string(),
            });
        }
    }
    diffs.sort_by_key(|d| field_rank(&d.field));
    diffs
}

/// `[speed amp=1 dur=600, ...]`, sorted by id, with the `minecraft:` prefix dropped.
fn effects_text(mut list: Vec<(String, i64, i64)>) -> String {
    list.sort();
    let items: Vec<String> = list
        .iter()
        .map(|(id, amp, dur)| {
            format!(
                "{} amp={amp} dur={dur}",
                id.strip_prefix("minecraft:").unwrap_or(id)
            )
        })
        .collect();
    format!("[{}]", items.join(", "))
}

/// Free-run `scenario` through `tick`: `replay(&s, |p, input, world| ms_kernel::player::tick(p,
/// input, world))`. See [`replay_with`].
pub fn replay<F>(scenario: &Scenario, tick: F) -> Report
where
    F: FnMut(&mut PlayerState, &Input, &World),
{
    replay_with(scenario, Mode::FreeRun, tick)
}

/// Replay `scenario` through `tick` in the given [`Mode`].
pub fn replay_with<F>(scenario: &Scenario, mode: Mode, mut tick: F) -> Report
where
    F: FnMut(&mut PlayerState, &Input, &World),
{
    let world = scenario.world();
    let mut p = scenario.initial_state();
    let n = scenario.rows.len();
    let mut report = Report {
        scenario: scenario.name.clone(),
        mode,
        ticks: n,
        exact_ticks: 0,
        longest_streak: 0,
        longest_streak_start: 0,
        first_divergence: None,
        diffs: Vec::with_capacity(n),
        sim_hashes: Vec::with_capacity(n),
        oracle_hashes: oracle_hashes(scenario),
    };
    let (mut streak, mut streak_start) = (0usize, 0usize);
    for (t, row) in scenario.rows.iter().enumerate() {
        apply_state(&mut p, &row.pre);
        tick(&mut p, &row.input, &world);
        report.sim_hashes.push(player_hash(&p));
        let diffs = diff_state(&p, &row.post);
        if diffs.is_empty() {
            if streak == 0 {
                streak_start = t;
            }
            streak += 1;
            report.exact_ticks += 1;
            if streak > report.longest_streak {
                report.longest_streak = streak;
                report.longest_streak_start = streak_start;
            }
        } else {
            streak = 0;
            if report.first_divergence.is_none() {
                report.first_divergence = Divergence::from_diffs(t, &diffs);
            }
            if mode == Mode::Resync {
                apply_state(&mut p, &row.post);
                if !row.post.contains_key("support") {
                    p.supporting_block = None;
                }
            }
        }
        report.diffs.push(diffs);
    }
    report
}

/// Render a recorded value for messages (re-export for callers that format diffs themselves).
pub fn decode(field: &str, v: &Value) -> String {
    describe(field, v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::apply_attrs;
    use crate::client_scenarios;

    /// A "kernel" that is the recording itself: after the tick the state is the recorded `post`.
    fn oracle_tick<'a>(
        scenario: &'a Scenario,
    ) -> impl FnMut(&mut PlayerState, &Input, &World) + 'a {
        let mut row = 0usize;
        move |p, _input, _world| {
            let post = &scenario.rows[row].post;
            apply_state(p, post);
            apply_attrs(p, post);
            if !post.contains_key("support") {
                p.supporting_block = None;
            }
            row += 1;
        }
    }

    #[test]
    fn a_perfect_kernel_is_exact_with_equal_hashes_on_every_scenario() {
        for name in client_scenarios() {
            let s = Scenario::load(&name).unwrap();
            let r = replay(&s, oracle_tick(&s));
            assert_eq!(r.ticks, s.rows.len(), "{name}");
            assert!(r.is_exact(), "{name}: {:?}", r.first_divergence);
            assert_eq!(r.exact_ticks, r.ticks, "{name}");
            assert_eq!(r.longest_streak, r.ticks, "{name}");
            assert_eq!(r.longest_streak_start, 0, "{name}");
            assert_eq!(r.first_divergence, None, "{name}");
            assert_eq!(r.hash_exact_ticks(), r.ticks, "{name}: hash mode disagrees");
            assert_eq!(r.first_hash_divergence(), None, "{name}");
            assert_eq!(r.sim_rolling(), r.oracle_rolling(), "{name}");
        }
    }

    #[test]
    fn a_do_nothing_kernel_diverges_and_the_report_is_consistent() {
        let s = Scenario::load("walk_basic").unwrap();
        let r = replay(&s, |_, _, _| {});
        assert_eq!(r.ticks, 300);
        assert_eq!(r.diffs.len(), r.ticks);
        assert_eq!(r.sim_hashes.len(), r.ticks);
        assert_eq!(r.oracle_hashes.len(), r.ticks);
        assert!(r.exact_ticks < r.ticks);
        let exact_from_diffs = r.diffs.iter().filter(|d| d.is_empty()).count();
        assert_eq!(exact_from_diffs, r.exact_ticks);
        let first = r.first_divergence.as_ref().expect("diverges");
        // The first divergence is the first tick with any diff, and its field is that tick's
        // first diff (contract order).
        let first_tick = r.diffs.iter().position(|d| !d.is_empty()).unwrap();
        assert_eq!(first.tick, first_tick);
        assert_eq!(first.field, r.diffs[first_tick][0].field);
        assert_eq!(first.differing, r.diffs[first_tick].len());
        // Streak bookkeeping: the reported streak is the longest run of empty diff lists.
        let mut best = 0;
        let mut cur = 0;
        for d in &r.diffs {
            cur = if d.is_empty() { cur + 1 } else { 0 };
            best = best.max(cur);
        }
        assert_eq!(r.longest_streak, best);
        // Exact ticks (hash mode and field mode) agree on this run too: hash-exact implies
        // field-exact (the age counter is in both).
        assert!(r.hash_exact_ticks() <= r.exact_ticks);
    }

    /// A kernel that copies the recording but gets tick 3 slightly wrong: the report pins the
    /// first divergence (tick, field) and the streak structure around it.
    #[test]
    fn a_single_bad_tick_is_pinned_exactly() {
        let s = Scenario::load("walk_basic").unwrap();
        let mut row = 0usize;
        let r = replay(&s, |p, _input, _world| {
            let post = &s.rows[row].post;
            apply_state(p, post);
            apply_attrs(p, post);
            if row == 3 {
                p.pos.x += 1.0e-9;
            }
            row += 1;
        });
        assert_eq!(r.exact_ticks, r.ticks - 1);
        assert_eq!(r.longest_streak, r.ticks - 4);
        assert_eq!(r.longest_streak_start, 4);
        let d = r.first_divergence.as_ref().unwrap();
        assert_eq!((d.tick, d.field.as_str(), d.differing), (3, "x", 1));
        assert_eq!(r.first_hash_divergence(), Some(3));
        assert_eq!(r.hash_exact_ticks(), r.ticks - 1);
        assert_ne!(r.sim_rolling(), r.oracle_rolling());
    }

    /// A kernel that never moves the player: in free-run the error compounds for good, while
    /// resync judges every tick from a correct start and so recovers the ticks where nothing
    /// moves anyway.
    #[test]
    fn resync_measures_ticks_independently_of_earlier_errors() {
        let s = Scenario::load("walk_basic").unwrap();
        // (It does copy the recorded attributes, which no replay mode restores by itself.)
        let lazy = || {
            let mut row = 0usize;
            let s = s.clone();
            move |p: &mut PlayerState, _: &Input, _: &World| {
                apply_attrs(p, &s.rows[row].post);
                p.tick_count += 1;
                row += 1;
            }
        };
        let free = replay_with(&s, Mode::FreeRun, lazy());
        let resync = replay_with(&s, Mode::Resync, lazy());
        assert_eq!((free.mode, resync.mode), (Mode::FreeRun, Mode::Resync));
        assert_eq!(free.first_divergence, resync.first_divergence);
        assert!(
            resync.exact_ticks > free.exact_ticks,
            "free {} vs resync {}",
            free.exact_ticks,
            resync.exact_ticks
        );
        assert!(resync.longest_streak >= free.longest_streak);
    }

    #[test]
    fn diffs_are_ordered_by_contract_field_order() {
        let s = Scenario::load("walk_basic").unwrap();
        let mut p = s.initial_state();
        p.food = 3;
        p.pos.y += 1.0;
        p.vel.x += 1.0;
        let mut expected = s.rows[0].post.clone();
        expected.insert("age".into(), Value::from(p.tick_count + 5));
        let diffs = diff_state(&p, &expected);
        let names: Vec<&str> = diffs.iter().map(|d| d.field.as_str()).collect();
        let rank = |n: &str| names.iter().position(|x| *x == n).unwrap();
        assert!(rank("y") < rank("dx"));
        assert!(rank("dx") < rank("age"));
        assert!(rank("age") < rank("food"));
    }

    #[test]
    fn attribute_diffs_are_per_attribute_and_decoded() {
        let s = Scenario::load("walk_basic").unwrap();
        let mut p = crate::canonical::state_from_fields(&s.rows[0].post);
        assert_eq!(diff_state(&p, &s.rows[0].post), vec![]);
        p.attributes.set_base(Attribute::Gravity, 0.1);
        let diffs = diff_state(&p, &s.rows[0].post);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].field, "attrs.gravity");
        assert_eq!(diffs[0].expected, format!("{:e}", 0.08_f64));
        assert_eq!(diffs[0].actual, format!("{:e}", 0.1_f64));
    }

    #[test]
    fn divergence_display_is_compact() {
        let d = Divergence {
            tick: 12,
            field: "dy".into(),
            expected: "-7.84e-2".into(),
            actual: "0e0".into(),
            differing: 3,
        };
        assert_eq!(
            d.to_string(),
            "t=12 dy: expected -7.84e-2, actual 0e0 (+2 more)"
        );
    }
}
