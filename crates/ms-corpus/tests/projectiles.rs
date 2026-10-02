//! Projectile flights against the oracle: every `spawn` action in `projectile_flights` and
//! `projectile_hits` is re-simulated from its logged initial state with `ms_kernel::projectile`,
//! and each server-tick sample of that projectile is compared bit for bit (position, velocity,
//! rotation, ground/in-ground flags and, for arrows, `life`).
//!
//! The test prints a per-projectile table (`cargo test -p ms-corpus --test projectiles --
//! --nocapture`). It asserts that the corpus has samples to compare and that the simulator
//! reproduces every sample bit for bit (3497 samples of 35 projectiles in the flights, 15 of 3 in
//! the hits scenario at the time of writing).

use ms_corpus::Scenario;
use ms_kernel::projectile::{Impact, Projectile, ProjectileKind};
use ms_numerics::Vec3;
use serde_json::Value;

fn f64_of(v: &Value, k: &str) -> f64 {
    f64::from_bits(v[k].as_i64().expect(k) as u64)
}

fn i64_of(v: &Value, k: &str) -> i64 {
    v[k].as_i64().unwrap_or(0)
}

struct Track {
    id: i64,
    kind: ProjectileKind,
    spawn_tick: i64,
    spawn: Value,
    /// (server tick, row index, sample)
    samples: Vec<(i64, usize, Value)>,
}

fn tracks(s: &Scenario) -> Vec<Track> {
    let mut out: Vec<Track> = Vec::new();
    for (ri, row) in s.rows.iter().enumerate() {
        for ev in &row.srv {
            match ev["kind"].as_str() {
                Some("action") if ev["op"] == "spawn" => {
                    let Some(kind) = ev["type"].as_str().and_then(ProjectileKind::from_id) else {
                        continue;
                    };
                    out.push(Track {
                        id: ev["id"].as_i64().unwrap(),
                        kind,
                        spawn_tick: ev["serverTick"].as_i64().unwrap(),
                        spawn: ev["entity"].clone(),
                        samples: Vec::new(),
                    });
                }
                Some("projectiles") => {
                    let tick = ev["serverTick"].as_i64().unwrap();
                    for e in ev["entities"].as_array().into_iter().flatten() {
                        let id = e["id"].as_i64().unwrap();
                        if let Some(t) = out.iter_mut().find(|t| t.id == id) {
                            t.samples.push((tick, ri, e.clone()));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// The fields of a sample the simulated projectile does not reproduce.
fn diffs(p: &Projectile, e: &Value) -> Vec<&'static str> {
    let mut d = Vec::new();
    let f64s: [(&'static str, f64); 6] = [
        ("x", p.pos.x),
        ("y", p.pos.y),
        ("z", p.pos.z),
        ("dx", p.vel.x),
        ("dy", p.vel.y),
        ("dz", p.vel.z),
    ];
    for (k, v) in f64s {
        if e[k].as_i64() != Some(v.to_bits() as i64) {
            d.push(k);
        }
    }
    for (k, v) in [("yaw", p.yaw), ("pitch", p.pitch)] {
        if e[k].as_i64() != Some(i64::from(v.to_bits() as i32)) {
            d.push(k);
        }
    }
    if i64_of(e, "ground") != i64::from(p.on_ground) {
        d.push("ground");
    }
    if i64_of(e, "removed") != i64::from(p.removed) {
        d.push("removed");
    }
    if e.get("inGround").is_some() {
        if i64_of(e, "inGround") != i64::from(p.in_ground) {
            d.push("inGround");
        }
        if i64_of(e, "life") != i64::from(p.life) {
            d.push("life");
        }
    }
    d
}

/// Replay one scenario; returns (samples compared, samples matching before the first divergence
/// summed over projectiles, projectiles fully matching).
fn replay(name: &str, with_player: bool) -> (usize, usize, usize) {
    let s = Scenario::load(name).unwrap();
    let world = s.world();
    let mut total = 0;
    let mut matched = 0;
    let mut perfect = 0;
    println!("{name}:");
    for t in tracks(&s) {
        let e = &t.spawn;
        let mut p = Projectile::new(
            t.kind,
            Vec3::new(f64_of(e, "x"), f64_of(e, "y"), f64_of(e, "z")),
            Vec3::new(f64_of(e, "dx"), f64_of(e, "dy"), f64_of(e, "dz")),
        );
        let mut tick = t.spawn_tick;
        let mut ok = 0;
        let mut first_bad: Option<(i64, Vec<&'static str>)> = None;
        let mut impact_tick: Option<i64> = None;
        for (st, ri, sample) in &t.samples {
            while tick < *st {
                tick += 1;
                let report = if with_player {
                    // The server's view of the player: the position the client reported at the end
                    // of the previous client tick.
                    let mut player = s.initial_state();
                    if *ri > 0 {
                        ms_corpus::apply_state(&mut player, &s.rows[ri - 1].post);
                    }
                    p.tick(&world, Some(&mut player))
                } else {
                    p.tick(&world, None)
                };
                if impact_tick.is_none() && matches!(report.impact, Some(Impact::Target { .. })) {
                    impact_tick = Some(tick);
                }
            }
            total += 1;
            let d = diffs(&p, sample);
            if d.is_empty() && first_bad.is_none() {
                ok += 1;
            } else if first_bad.is_none() {
                first_bad = Some((*st - t.spawn_tick, d));
            }
        }
        // The tick after the last sample: the vanilla projectile was gone by then (if the
        // recording went on). Check whether the simulation also removes it there.
        let last = t.samples.last().map_or(t.spawn_tick, |x| x.0);
        let last_row = t.samples.last().map_or(0, |x| x.1);
        let removal = if !p.removed && (with_player || !t.kind.is_arrow()) {
            let r = if with_player {
                let mut player = s.initial_state();
                ms_corpus::apply_state(&mut player, &s.rows[last_row].post);
                p.tick(&world, Some(&mut player))
            } else {
                p.tick(&world, None)
            };
            if impact_tick.is_none() && matches!(r.impact, Some(Impact::Target { .. })) {
                impact_tick = Some(last + 1);
            }
            p.removed
        } else {
            p.removed
        };
        matched += ok;
        if first_bad.is_none() {
            perfect += 1;
        }
        println!(
            "  id {:>3} {:<14?} samples {:>3} matching {:>3}  first divergence {:?}  removed-after-last {} impact {:?} (last sample at age {})",
            t.id,
            t.kind,
            t.samples.len(),
            ok,
            first_bad,
            removal,
            impact_tick.map(|i| i - t.spawn_tick),
            last - t.spawn_tick,
        );
    }
    (total, matched, perfect)
}

#[test]
fn projectile_flights_replay() {
    let (total, matched, perfect) = replay("projectile_flights", false);
    println!("projectile_flights: {matched}/{total} samples, {perfect} projectiles fully matching");
    assert!(total > 0);
    assert_eq!(matched, total, "every sample bit-exact");
}

#[test]
fn projectile_hits_replay() {
    let (total, matched, perfect) = replay("projectile_hits", true);
    println!("projectile_hits: {matched}/{total} samples, {perfect} projectiles fully matching");
    assert!(total > 0);
    assert_eq!(matched, total, "every sample bit-exact");
}

#[test]
fn atan2_reference_points() {
    use ms_kernel::projectile::mth_atan2;
    // Mth.atan2 on the JVM (1.21.11 classes): (y, x) -> result, raw bits.
    let cases: [(u64, u64, u64); 5] = [
        (0x0000000000000000, 0x0000000000000000, 0x0000000000000000),
        (0x8000000000000000, 0xbff0000000000000, 0x400921fb54442d18),
        (0x3ee4cb4e4ee658c3, 0xbf4e67c12f12e9ec, 0x40090c18a8391214),
        (0x3f8cad419b959180, 0xbeffd1e38d67c554, 0x3ff92adc321f32ab),
        (0x3ff3acd490371f5c, 0xbf17fc8b2464409f, 0x3ff922493de9a2cd),
    ];
    for (y, x, want) in cases {
        let got = mth_atan2(f64::from_bits(y), f64::from_bits(x));
        assert_eq!(got.to_bits(), want, "atan2({y:#x}, {x:#x})");
    }
}
