//! The recorded projectiles of a scenario, grouped per entity, and a replay driver for them.
//!
//! The server-side events of a row (`srv`) include a `spawn` action (the projectile's id, type and
//! initial state, logged when it is created) and `projectiles` samples (every projectile's state
//! after each server tick). A [`ProjectileTrack`] gathers one entity's spawn and samples in order.
//!
//! [`replay_projectiles`] rebuilds each projectile from its spawn state, advances it one server
//! tick per recorded sample with a caller-supplied step function (so it works with whatever
//! `ms_kernel::projectile` ends up exposing), and compares the result with the sample.
//!
//! A projectile is not recorded after the server removes it (a hit, a splash): its track simply
//! ends, and arrows that stick in a block keep being sampled.

use crate::canonical::state_from_fields;
use crate::replay::Divergence;
use crate::{describe, FieldDiff, Fields, Scenario};
use ms_kernel::projectile::{Projectile, ProjectileKind};
use ms_kernel::PlayerState;
use ms_numerics::Vec3;
use ms_world::World;
use serde_json::Value;

/// One server-tick sample of a projectile.
#[derive(Clone, Debug)]
pub struct ProjectileSample {
    /// The client row whose `srv` list carried this sample.
    pub row: usize,
    pub server_tick: i64,
    /// The recorded state: `x y z dx dy dz yaw pitch ground ... age removed noGravity` (raw bits
    /// for floats, like every corpus state).
    pub state: Fields,
}

/// Everything recorded about one projectile entity.
#[derive(Clone, Debug)]
pub struct ProjectileTrack {
    pub id: i64,
    /// Registry id, e.g. `minecraft:arrow`.
    pub kind: String,
    /// The client row and server tick of the `spawn` action, if the recording has one.
    pub spawn_row: Option<usize>,
    pub spawn_server_tick: Option<i64>,
    /// The projectile's state right after it was created (age 0), if the recording has the spawn.
    pub spawn: Option<Fields>,
    /// Samples in server-tick order.
    pub samples: Vec<ProjectileSample>,
}

fn as_fields(v: &Value) -> Fields {
    v.as_object().cloned().unwrap_or_default()
}

/// Group a scenario's projectile events per entity id (in order of first appearance).
pub fn projectile_tracks(scenario: &Scenario) -> Vec<ProjectileTrack> {
    let mut tracks: Vec<ProjectileTrack> = Vec::new();
    let find = |tracks: &mut Vec<ProjectileTrack>, id: i64, kind: &str| -> usize {
        match tracks.iter().position(|t| t.id == id) {
            Some(i) => i,
            None => {
                tracks.push(ProjectileTrack {
                    id,
                    kind: kind.to_string(),
                    spawn_row: None,
                    spawn_server_tick: None,
                    spawn: None,
                    samples: Vec::new(),
                });
                tracks.len() - 1
            }
        }
    };
    for row in &scenario.rows {
        for ev in &row.srv {
            match ev["kind"].as_str() {
                Some("action") if ev["op"].as_str() == Some("spawn") => {
                    let (Some(id), Some(kind)) = (ev["id"].as_i64(), ev["type"].as_str()) else {
                        continue;
                    };
                    let i = find(&mut tracks, id, kind);
                    tracks[i].spawn_row = Some(row.t);
                    tracks[i].spawn_server_tick = ev["serverTick"].as_i64();
                    tracks[i].spawn = Some(as_fields(&ev["entity"]));
                }
                Some("projectiles") => {
                    let server_tick = ev["serverTick"].as_i64().unwrap_or(0);
                    for e in ev["entities"].as_array().into_iter().flatten() {
                        let (Some(id), Some(kind)) = (e["id"].as_i64(), e["type"].as_str()) else {
                            continue;
                        };
                        let i = find(&mut tracks, id, kind);
                        tracks[i].samples.push(ProjectileSample {
                            row: row.t,
                            server_tick,
                            state: as_fields(e),
                        });
                    }
                }
                _ => {}
            }
        }
    }
    for t in &mut tracks {
        t.samples.sort_by_key(|s| s.server_tick);
    }
    tracks
}

/// What a projectile step function is told besides the projectile itself.
pub struct ProjectileCtx<'a> {
    pub world: &'a World,
    /// The local player as the client recorded it at the end of the row that carried the sample.
    /// The server's own copy of the player differs by up to a client tick, so this is the best
    /// available approximation for hit tests, not an exact input.
    pub player: &'a PlayerState,
    /// The server tick being simulated.
    pub server_tick: i64,
    /// The client row that carried the sample.
    pub row: usize,
}

/// How one projectile's replay went.
#[derive(Clone, Debug)]
pub struct ProjectileReport {
    pub id: i64,
    pub kind: String,
    pub spawn_row: Option<usize>,
    /// Samples replayed.
    pub samples: usize,
    pub exact_samples: usize,
    pub longest_streak: usize,
    /// First differing sample (`tick` = ticks since spawn, i.e. the sample's age) and field.
    pub first_divergence: Option<Divergence>,
    /// Per sample, `(server tick, differing fields)`.
    pub diffs: Vec<(i64, Vec<FieldDiff>)>,
    /// Why the track was not replayed (unsupported kind, no recorded spawn).
    pub skipped: Option<String>,
}

fn bits64(v: &Value) -> f64 {
    f64::from_bits(v.as_i64().unwrap_or(0) as u64)
}

fn bits32(v: &Value) -> f32 {
    f32::from_bits(v.as_i64().unwrap_or(0) as i32 as u32)
}

/// Build a [`Projectile`] from a recorded entity state.
fn projectile_from(kind: ProjectileKind, f: &Fields) -> Projectile {
    let get = |k: &str| f.get(k).cloned().unwrap_or(Value::Null);
    let mut p = Projectile::new(
        kind,
        Vec3::new(bits64(&get("x")), bits64(&get("y")), bits64(&get("z"))),
        Vec3::new(bits64(&get("dx")), bits64(&get("dy")), bits64(&get("dz"))),
    );
    p.yaw = bits32(&get("yaw"));
    p.pitch = bits32(&get("pitch"));
    p.on_ground = get("ground").as_i64() == Some(1);
    p.tick_count = get("age").as_i64().unwrap_or(0) as i32;
    p
}

/// The fields of `expected` (a recorded projectile sample) that `p` does not reproduce.
pub fn diff_projectile(p: &Projectile, expected: &Fields) -> Vec<FieldDiff> {
    let d = |x: f64| Value::from(x.to_bits() as i64);
    let f = |x: f32| Value::from(x.to_bits() as i32);
    let b = |x: bool| Value::from(i64::from(x));
    let actual: [(&str, Value); 11] = [
        ("x", d(p.pos.x)),
        ("y", d(p.pos.y)),
        ("z", d(p.pos.z)),
        ("dx", d(p.vel.x)),
        ("dy", d(p.vel.y)),
        ("dz", d(p.vel.z)),
        ("yaw", f(p.yaw)),
        ("pitch", f(p.pitch)),
        ("ground", b(p.on_ground)),
        ("age", Value::from(p.tick_count)),
        ("removed", b(p.removed)),
    ];
    let mut out = Vec::new();
    for (name, got) in actual {
        let Some(want) = expected.get(name) else {
            continue;
        };
        if *want != got {
            out.push(FieldDiff {
                field: name.to_string(),
                expected: describe(name, want),
                actual: describe(name, &got),
            });
        }
    }
    out
}

/// Replay every projectile of `scenario` through `step`, which must advance the projectile by one
/// server tick. Each track starts from its recorded spawn state and is stepped once per server
/// tick up to each sample (normally exactly one step per sample), then compared with the sample.
/// The projectile is never re-seeded from the recording, so errors compound as in a real run;
/// a projectile that reports `removed` is no longer stepped.
pub fn replay_projectiles<F>(scenario: &Scenario, mut step: F) -> Vec<ProjectileReport>
where
    F: FnMut(&mut Projectile, &ProjectileCtx),
{
    let world = scenario.world();
    let mut reports = Vec::new();
    for track in projectile_tracks(scenario) {
        let mut report = ProjectileReport {
            id: track.id,
            kind: track.kind.clone(),
            spawn_row: track.spawn_row,
            samples: 0,
            exact_samples: 0,
            longest_streak: 0,
            first_divergence: None,
            diffs: Vec::new(),
            skipped: None,
        };
        let Some(kind) = ProjectileKind::from_id(&track.kind) else {
            report.skipped = Some(format!("unsupported projectile kind {}", track.kind));
            reports.push(report);
            continue;
        };
        let (Some(spawn), Some(spawn_tick)) = (&track.spawn, track.spawn_server_tick) else {
            report.skipped = Some("no recorded spawn".to_string());
            reports.push(report);
            continue;
        };
        let mut proj = projectile_from(kind, spawn);
        let mut tick = spawn_tick;
        let mut streak = 0usize;
        for sample in &track.samples {
            let player = state_from_fields(&scenario.rows[sample.row].post);
            while tick < sample.server_tick {
                tick += 1;
                if !proj.removed {
                    step(
                        &mut proj,
                        &ProjectileCtx {
                            world: &world,
                            player: &player,
                            server_tick: tick,
                            row: sample.row,
                        },
                    );
                }
            }
            let diffs = diff_projectile(&proj, &sample.state);
            report.samples += 1;
            if diffs.is_empty() {
                report.exact_samples += 1;
                streak += 1;
                report.longest_streak = report.longest_streak.max(streak);
            } else {
                streak = 0;
                if report.first_divergence.is_none() {
                    let age = sample.state.get("age").and_then(Value::as_u64).unwrap_or(0);
                    report.first_divergence = Divergence::from_diffs(age as usize, &diffs);
                }
            }
            report.diffs.push((sample.server_tick, diffs));
        }
        reports.push(report);
    }
    reports
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_group_samples_per_entity_with_their_spawn() {
        let s = Scenario::load("projectile_flights").unwrap();
        let tracks = projectile_tracks(&s);
        assert_eq!(tracks.len(), 35, "spawned projectiles in the recording");
        for t in &tracks {
            let spawn = t.spawn.as_ref().expect("every projectile has a spawn");
            assert_eq!(spawn["age"].as_i64(), Some(0), "spawn state is age 0");
            assert!(!t.samples.is_empty(), "id {}", t.id);
            // Samples are consecutive server ticks starting right after the spawn tick.
            let first = t.spawn_server_tick.unwrap() + 1;
            for (i, smp) in t.samples.iter().enumerate() {
                assert_eq!(smp.server_tick, first + i as i64, "id {} sample {i}", t.id);
                assert_eq!(smp.state["age"].as_i64(), Some(i as i64 + 1));
                assert_eq!(smp.state["id"].as_i64(), Some(t.id));
                assert!(smp.row >= t.spawn_row.unwrap());
            }
        }
        // The first projectile is the snowball thrown at row 11.
        assert_eq!(tracks[0].kind, "minecraft:snowball");
        assert_eq!(tracks[0].spawn_row, Some(11));
        assert_eq!(tracks[0].samples.len(), 9);
    }

    #[test]
    fn scenarios_without_projectiles_have_no_tracks() {
        let s = Scenario::load("walk_basic").unwrap();
        assert!(projectile_tracks(&s).is_empty());
    }

    /// A step function that replays the recording itself reproduces every sample.
    #[test]
    fn a_perfect_stepper_is_exact() {
        for name in ["projectile_flights", "projectile_hits"] {
            let s = Scenario::load(name).unwrap();
            let tracks = projectile_tracks(&s);
            // Index the recorded samples by (id, server tick) for the stepper to copy from.
            let mut recorded = std::collections::HashMap::new();
            for t in &tracks {
                for smp in &t.samples {
                    recorded.insert((t.id, smp.server_tick), smp.state.clone());
                }
            }
            // The stepper only sees the projectile, so recover its id from the spawn order: the
            // replay visits tracks in order and steps each sample's ticks consecutively.
            let order: Vec<(i64, i64)> = tracks
                .iter()
                .flat_map(|t| t.samples.iter().map(move |s| (t.id, s.server_tick)))
                .collect();
            let mut next = 0usize;
            let reports = replay_projectiles(&s, |p, _ctx| {
                let key = order[next];
                next += 1;
                let f = &recorded[&key];
                p.pos = Vec3::new(bits64(&f["x"]), bits64(&f["y"]), bits64(&f["z"]));
                p.vel = Vec3::new(bits64(&f["dx"]), bits64(&f["dy"]), bits64(&f["dz"]));
                p.yaw = bits32(&f["yaw"]);
                p.pitch = bits32(&f["pitch"]);
                p.on_ground = f["ground"].as_i64() == Some(1);
                p.tick_count += 1;
            });
            assert_eq!(reports.len(), tracks.len());
            for r in &reports {
                assert_eq!(r.skipped, None, "{name} id {}", r.id);
                assert_eq!(r.exact_samples, r.samples, "{name} id {}", r.id);
                assert_eq!(r.first_divergence, None);
                assert_eq!(r.longest_streak, r.samples);
            }
        }
    }

    #[test]
    fn a_stepper_that_does_nothing_diverges_on_the_first_sample() {
        let s = Scenario::load("projectile_flights").unwrap();
        let reports = replay_projectiles(&s, |_, _| {});
        let r = &reports[0];
        let d = r.first_divergence.as_ref().expect("diverges");
        assert_eq!(d.tick, 1, "first sample is age 1");
        assert_ne!(r.exact_samples, r.samples);
    }
}
