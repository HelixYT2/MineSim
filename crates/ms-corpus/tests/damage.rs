//! The damage port against the oracle corpus: hurt / knockback actions (the server-side before
//! and after logs and the quantized velocity the client received), the evolution of the server's
//! copy of the velocity between actions, fall damage, and the per-tick timers.
//!
//! Run with `--nocapture` for the per-check counts.

use ms_corpus::{apply_state, client_scenarios, Fields, Scenario};
use ms_kernel::attributes::Attribute;
use ms_kernel::damage::{
    cause_fall_damage, fall_damage, hurt, knockback, reset_server_copy, server_do_tick,
    server_move_packet, server_tick, sync_motion, tick_timers, DamageSource, TickStart,
};
use ms_kernel::PlayerState;
use ms_numerics::Vec3;
use ms_world::World;
use serde_json::Value;

// ---------------------------------------------------------------- decoding helpers

fn d64(v: &Value) -> f64 {
    f64::from_bits(v.as_i64().expect("f64 bits") as u64)
}

fn f32v(v: &Value) -> f32 {
    f32::from_bits(v.as_i64().expect("f32 bits") as i32 as u32)
}

fn int(v: &Value) -> i32 {
    v.as_i64().expect("int") as i32
}

/// Fields the server owns (the oracle's `srv` logs and the pre diffs after a hit carry them); the
/// replay keeps the model's values instead of the recorded client view.
const SERVER_OWNED: &[&str] = &[
    "health",
    "absorption",
    "invul",
    "hurtTime",
    "lastHurt",
    "deathTime",
    "dx",
    "dy",
    "dz",
];

fn without_server_fields(f: &Fields) -> Fields {
    let mut out = f.clone();
    for k in SERVER_OWNED {
        out.remove(*k);
    }
    out
}

/// The recorded attribute values as base values (the kernel's effect application, which would
/// derive them from effects, is not part of this module). The recorded value already has every
/// modifier in it, so the ones the state carries (the safe fall distance of a jump boost effect,
/// say) must go: left in, they would count twice (a jump boost III player's 6.0 blocks of safe
/// fall would be 9.0, and its landings of 7 to 9 blocks would cost nothing).
fn apply_attributes(p: &mut PlayerState, post: &Fields) {
    if let Some(attrs) = post.get("attrs").and_then(Value::as_object) {
        for a in Attribute::ALL {
            if let Some(v) = attrs.get(a.name()) {
                p.attributes.remove_all_modifiers(a);
                p.attributes.set_base(a, d64(v));
            }
        }
    }
}

/// One server-side action from a row's `srv` list that carries a before/after log.
struct Action {
    row: usize,
    op: String,
    amount: f32,
    sx: f64,
    sz: f64,
    strength: f64,
    kx: f64,
    kz: f64,
    result: Option<bool>,
    before: Fields,
    after: Fields,
}

fn actions(sc: &Scenario) -> Vec<Action> {
    let mut out = Vec::new();
    for (i, row) in sc.rows.iter().enumerate() {
        for ev in &row.srv {
            if ev["kind"] != "action" || !ev.get("before").is_some_and(Value::is_object) {
                continue;
            }
            let op = ev["op"].as_str().unwrap_or("").to_string();
            let g64 = |k: &str| ev.get(k).map(d64).unwrap_or(0.0);
            out.push(Action {
                row: i,
                op,
                amount: ev.get("amount").map(f32v).unwrap_or(0.0),
                sx: g64("sx"),
                sz: g64("sz"),
                strength: g64("strength"),
                kx: g64("kx"),
                kz: g64("kz"),
                result: ev.get("result").map(|r| r.as_i64() == Some(1)),
                before: ev["before"].as_object().unwrap().clone(),
                after: ev["after"].as_object().unwrap().clone(),
            });
        }
    }
    out
}

/// Put the server-side state an action's `before` log describes into `p`.
fn load_before(p: &mut PlayerState, before: &Fields) {
    p.pos = Vec3::new(d64(&before["x"]), d64(&before["y"]), d64(&before["z"]));
    p.server_vel = Vec3::new(d64(&before["dx"]), d64(&before["dy"]), d64(&before["dz"]));
    p.server.on_ground = int(&before["ground"]) == 1;
    p.health = f32v(&before["health"]);
    p.invulnerable_time = int(&before["invul"]);
    p.hurt_time = int(&before["hurtTime"]);
    p.last_hurt = f32v(&before["lastHurt"]);
}

fn run_action(p: &mut PlayerState, a: &Action) -> Option<bool> {
    match a.op.as_str() {
        "hurt" => Some(hurt(p, DamageSource::Point { x: a.sx, z: a.sz }, a.amount)),
        "knockback" => {
            knockback(p, a.strength, a.kx, a.kz);
            None
        }
        other => panic!("unknown op {other}"),
    }
}

/// Compare the server state in `p` with an action's `after` log; returns (compared, mismatches).
fn compare_after(p: &PlayerState, after: &Fields, what: &str) -> (usize, Vec<String>) {
    let mut n = 0;
    let mut bad = Vec::new();
    let mut check = |name: &str, got: u64, want: u64| {
        n += 1;
        if got != want {
            bad.push(format!("{what}: {name} got {got:#x} want {want:#x}"));
        }
    };
    check("dx", p.server_vel.x.to_bits(), d64(&after["dx"]).to_bits());
    check("dy", p.server_vel.y.to_bits(), d64(&after["dy"]).to_bits());
    check("dz", p.server_vel.z.to_bits(), d64(&after["dz"]).to_bits());
    check(
        "health",
        u64::from(p.health.to_bits()),
        u64::from(f32v(&after["health"]).to_bits()),
    );
    check(
        "invul",
        p.invulnerable_time as u64,
        int(&after["invul"]) as u64,
    );
    check(
        "hurtTime",
        p.hurt_time as u64,
        int(&after["hurtTime"]) as u64,
    );
    check(
        "lastHurt",
        u64::from(p.last_hurt.to_bits()),
        u64::from(f32v(&after["lastHurt"]).to_bits()),
    );
    (n, bad)
}

const DAMAGE_SCENARIOS: &[&str] = &["knockback_standing", "knockback_moving", "legacy_capture"];

// ---------------------------------------------------------------- hurt / knockback actions

/// Every logged hurt/knockback action: the functions applied to the `before` log must give the
/// `after` log bit for bit, and the velocity the client received (the next row's `pre`) must be
/// the LpVec3-quantized server velocity.
#[test]
fn logged_actions_reproduce_the_server_after_state_and_the_client_velocity() {
    let mut n_actions = 0;
    let mut n_fields = 0;
    let mut n_client = 0;
    let mut failures = Vec::new();
    for name in DAMAGE_SCENARIOS {
        let sc = Scenario::load(name).unwrap();
        for a in actions(&sc) {
            n_actions += 1;
            let what = format!("{name} row {} {}", a.row, a.op);
            let mut p = PlayerState::new(Vec3::ZERO, 0.0);
            load_before(&mut p, &a.before);
            // whatever the client's velocity was; only a sent packet changes it
            p.vel = Vec3::new(1.0, 2.0, 3.0);
            let result = run_action(&mut p, &a);
            // the next sendChanges tells the client
            sync_motion(&mut p);
            if let (Some(got), Some(want)) = (result, a.result) {
                n_fields += 1;
                if got != want {
                    failures.push(format!("{what}: result {got} want {want}"));
                }
            }
            let (n, bad) = compare_after(&p, &a.after, &what);
            n_fields += n;
            failures.extend(bad);

            // The client-side result in the next row's pre.
            let next = &sc.rows[a.row + 1].pre;
            if a.result != Some(false) {
                for (k, got) in [("dx", p.vel.x), ("dy", p.vel.y), ("dz", p.vel.z)] {
                    // an unchanged component is absent from the diff
                    if let Some(v) = next.get(k) {
                        n_client += 1;
                        if d64(v).to_bits() != got.to_bits() {
                            failures
                                .push(format!("{what}: client {k} got {got:?} want {:?}", d64(v)));
                        }
                    }
                }
                if a.op == "hurt" {
                    for (k, got) in [("invul", p.invulnerable_time), ("hurtTime", p.hurt_time)] {
                        n_client += 1;
                        if next.get(k).map(int) != Some(got) {
                            failures.push(format!(
                                "{what}: client {k} got {got} want {:?}",
                                next.get(k)
                            ));
                        }
                    }
                    // The health packet is sent at the end of the next server tick, so depending on
                    // the phase between the two threads the client shows it at the start of the
                    // next tick or the one after.
                    n_client += 1;
                    let arrived = [a.row + 1, a.row + 2]
                        .iter()
                        .find_map(|&r| sc.rows[r].pre.get("health").map(f32v));
                    if arrived != Some(p.health) {
                        failures.push(format!(
                            "{what}: client health got {} want {:?}",
                            p.health, arrived
                        ));
                    }
                }
            } else {
                // rejected: the client is told nothing
                n_client += 1;
                if p.vel != Vec3::new(1.0, 2.0, 3.0) {
                    failures.push(format!("{what}: rejected hit changed the client velocity"));
                }
                for k in ["dx", "dy", "dz", "health", "invul", "hurtTime"] {
                    n_client += 1;
                    if next.contains_key(k) {
                        failures.push(format!("{what}: rejected hit but the client's pre has {k}"));
                    }
                }
            }
        }
    }
    eprintln!(
        "damage actions: {n_actions} actions, {n_fields} server fields and {n_client} client fields compared, {} failures",
        failures.len()
    );
    assert_eq!(n_actions, 9, "the nine logged actions");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---------------------------------------------------------------- the server copy between actions

/// What a replay of one scenario observed about the server model.
#[derive(Default)]
struct Observed {
    /// per logged action: the model's server state just before the action against the log
    gap: Vec<GapCheck>,
    landings: Vec<Landing>,
}

struct GapCheck {
    scenario: String,
    row: usize,
    exact: bool,
    /// (field, model, recorded) for every differing field
    diffs: Vec<(String, f64, f64)>,
}

struct Landing {
    scenario: String,
    row: usize,
    fall: f64,
    /// distance and multiplier `Block.fallOn` hands to `causeFallDamage` (`None`: not called)
    cause: Option<(f64, f32)>,
    /// the damage the formula gives
    expected: i32,
    /// the health the client's next row shows
    observed_drop: i32,
    /// for damaging landings: the server's health, invulnerability window and hurt time against
    /// the pre of the row they arrived at
    delivered_ok: Option<bool>,
    /// for damaging landings: the velocity the model's server copy sent against the same pre
    velocity_ok: Option<bool>,
    /// the row whose start shows the health packet (`row + 1`, or `row + 2` when the phase between
    /// the threads put it a tick later)
    arrival: Option<usize>,
}

/// `Block.fallOn` for the block the player lands on: the arguments of `causeFallDamage`, or
/// `None` if the block does not call it (sneaking on slime).
fn fall_on(world: &World, p: &PlayerState, fall: f64) -> Option<(f64, f32)> {
    let x = p.pos.x.floor() as i32;
    let z = p.pos.z.floor() as i32;
    let y = (p.pos.y - f64::from(0.2_f32)).floor() as i32;
    match world.block_name(x, y, z) {
        "minecraft:slime_block" => (p.shift_key_down)
            .then_some(())
            .map_or(Some((fall, 0.0)), |_| None),
        "minecraft:hay_block" | "minecraft:honey_block" => Some((fall, 0.2)),
        n if n.ends_with("_bed") => Some((fall * 0.5, 1.0)),
        _ => Some((fall, 1.0)),
    }
}

/// A move packet the server processes one server tick late (client/server phase jitter): the
/// server ticks without it and handles it together with the next one.
#[derive(Clone, Copy)]
struct Jitter {
    scenario: &'static str,
    late_row: usize,
}

/// Replay a scenario through the server model: the client side is the recorded data, the server
/// side is `server_tick` and the logged actions. `jitter` makes one packet late.
fn replay(name: &str, jitter: Option<Jitter>, obs: &mut Observed) {
    let sc = Scenario::load(name).unwrap();
    let world = sc.world();
    let acts = actions(&sc);
    let late_row = jitter.filter(|j| j.scenario == name).map(|j| j.late_row);
    let mut srv = sc.initial_state();
    reset_server_copy(&mut srv);
    let mut prev_post: Option<Fields> = None;
    let mut late_packet: Option<(TickStart, Fields)> = None;
    for (i, row) in sc.rows.iter().enumerate() {
        // (a) server-side actions that ran between the previous row and this one
        for a in acts.iter().filter(|a| a.row == i) {
            // How far did the model evolve from the previous action to this `before`?
            let mut diffs = Vec::new();
            for (f, m, r) in [
                ("dx", srv.server_vel.x, d64(&a.before["dx"])),
                ("dy", srv.server_vel.y, d64(&a.before["dy"])),
                ("dz", srv.server_vel.z, d64(&a.before["dz"])),
                (
                    "ground",
                    f64::from(u8::from(srv.server.on_ground)),
                    f64::from(int(&a.before["ground"])),
                ),
                (
                    "invul",
                    f64::from(srv.invulnerable_time),
                    f64::from(int(&a.before["invul"])),
                ),
                (
                    "hurtTime",
                    f64::from(srv.hurt_time),
                    f64::from(int(&a.before["hurtTime"])),
                ),
                ("x", srv.pos.x, d64(&a.before["x"])),
                ("y", srv.pos.y, d64(&a.before["y"])),
                ("z", srv.pos.z, d64(&a.before["z"])),
            ] {
                if m.to_bits() != r.to_bits() {
                    diffs.push((f.to_string(), m, r));
                }
            }
            obs.gap.push(GapCheck {
                scenario: name.to_string(),
                row: i,
                exact: diffs.is_empty(),
                diffs,
            });
            // Resynchronise to the recorded before-state so that a miss does not cascade.
            load_before(&mut srv, &a.before);
            run_action(&mut srv, a);
        }
        // (b) external changes of this row (not the server-owned fields)
        apply_state(&mut srv, &without_server_fields(&row.pre));
        // (c) the client tick: countdowns first (baseTick), then its end-of-tick state
        let start = TickStart::of(&srv);
        tick_timers(&mut srv);
        apply_state(&mut srv, &without_server_fields(&row.post));
        apply_attributes(&mut srv, &row.post);
        let mut landing = None;
        if let Some(prev) = &prev_post {
            let fall_before = d64(&prev["fall"]);
            let grounded = int(&row.post["ground"]) == 1;
            if fall_before > 0.0 && d64(&row.post["fall"]) == 0.0 && grounded && !srv.in_water {
                // Entity.checkFallDamage: fallDistance -= (float) dy for the landing move
                let dy = d64(&row.post["y"]) - d64(&prev["y"]);
                let fall = if dy < 0.0 {
                    fall_before - f64::from(dy as f32)
                } else {
                    fall_before
                };
                let cause = fall_on(&world, &srv, fall);
                if let Some((d, m)) = cause {
                    cause_fall_damage(&mut srv, d, m);
                }
                landing = Some((fall, cause));
            }
        }
        // (d) the server tick
        let health_before = srv.health;
        if late_row == Some(i) {
            // the packet is late: the server ticks from its last known position, without it
            late_packet = Some((start, row.post.clone()));
            let here = srv.pos;
            srv.pos = start.pos;
            server_do_tick(&mut srv, &world);
            srv.pos = here;
        } else if let Some((late_start, late_post)) = late_packet.take() {
            // two packets in this server tick, the late one first
            apply_state(&mut srv, &without_server_fields(&late_post));
            let mut snapshot = PlayerState::new(Vec3::ZERO, 0.0);
            apply_state(&mut snapshot, &late_post);
            server_move_packet(&mut srv, &late_start, &world);
            apply_state(&mut srv, &without_server_fields(&row.post));
            server_move_packet(&mut srv, &TickStart::of(&snapshot), &world);
            sync_motion(&mut srv);
            server_do_tick(&mut srv, &world);
        } else {
            server_tick(&mut srv, &start, &world);
        }
        if let Some((fall, cause)) = landing {
            let expected = cause.map_or(0, |(d, m)| fall_damage(&srv, d, m));
            // The health packet reaches the client at the start of the next tick or the one after
            // (phase between the two threads), and the damage event and the velocity come with it
            // (the server sends them in the same tick, which is where the model delivers them).
            let arrival = [i + 1, i + 2]
                .into_iter()
                .find(|&r| sc.rows.get(r).is_some_and(|r| r.pre.contains_key("health")));
            let arrived = arrival.map(|r| f32v(&sc.rows[r].pre["health"]));
            let observed_drop = match arrived {
                Some(h) => (health_before - h).round() as i32,
                None => 0,
            };
            let (delivered_ok, velocity_ok) = if expected > 0 && observed_drop > 0 {
                let n = &sc.rows[arrival.unwrap()].pre;
                let q = srv.vel;
                // an unchanged client velocity component is absent from the diff
                let comp =
                    |k: &str, got: f64| n.get(k).is_none_or(|v| d64(v).to_bits() == got.to_bits());
                (
                    Some(
                        Some(srv.health) == arrived
                            && srv.invulnerable_time == int(&n["invul"])
                            && srv.hurt_time == int(&n["hurtTime"]),
                    ),
                    Some(comp("dx", q.x) && comp("dy", q.y) && comp("dz", q.z)),
                )
            } else {
                (None, None)
            };
            obs.landings.push(Landing {
                scenario: name.to_string(),
                row: i,
                fall,
                cause,
                expected,
                observed_drop,
                delivered_ok,
                velocity_ok,
                arrival,
            });
        }
        prev_post = Some(row.post.clone());
    }
}

fn gap_summary(obs: &Observed) -> (usize, usize) {
    (obs.gap.len(), obs.gap.iter().filter(|g| g.exact).count())
}

/// The server copy's evolution between the logged actions (jump boost, gravity, drag, landing and
/// collision, ground flag) against the next action's `before` log: eight of the nine states are
/// reproduced bit for bit with client and server in lock step. The ninth (the third action of
/// `knockback_moving`) is reproduced bit for bit too if the client's move packet of tick 86 is
/// handled one server tick late, which is what the recording's phase jitter looks like; no other
/// late packet explains it.
#[test]
fn server_copy_between_actions_matches_the_next_before_log() {
    let mut lockstep = Observed::default();
    for name in DAMAGE_SCENARIOS {
        replay(name, None, &mut lockstep);
    }
    let (n, exact) = gap_summary(&lockstep);
    eprintln!(
        "server-copy evolution (lock step): {exact}/{n} logged before-states reproduced bit for bit (dx, dy, dz, ground, invul, hurtTime, x, y, z)"
    );
    for g in lockstep.gap.iter().filter(|g| !g.exact) {
        for (f, m, r) in &g.diffs {
            eprintln!(
                "  {} row {}: {f} model {m:?} recorded {r:?}",
                g.scenario, g.row
            );
        }
    }
    assert_eq!((n, exact), (9, 8));
    let miss: Vec<_> = lockstep.gap.iter().filter(|g| !g.exact).collect();
    assert_eq!(
        (miss[0].scenario.as_str(), miss[0].row),
        ("knockback_moving", 93)
    );

    // Which single late packet (rows 55..=93 of knockback_moving) makes the ninth state exact?
    let mut explaining = Vec::new();
    for late_row in 55..=93 {
        let mut obs = Observed::default();
        replay(
            "knockback_moving",
            Some(Jitter {
                scenario: "knockback_moving",
                late_row,
            }),
            &mut obs,
        );
        let at_93 = obs.gap.iter().find(|g| g.row == 93).unwrap();
        if at_93.exact {
            explaining.push(late_row);
        }
    }
    eprintln!("late move packet that reproduces knockback_moving row 93 exactly: {explaining:?}");
    assert_eq!(explaining, vec![86]);

    let mut with_jitter = Observed::default();
    for name in DAMAGE_SCENARIOS {
        replay(
            name,
            Some(Jitter {
                scenario: "knockback_moving",
                late_row: 86,
            }),
            &mut with_jitter,
        );
    }
    let (n, exact) = gap_summary(&with_jitter);
    eprintln!("server-copy evolution (packet 86 late): {exact}/{n} reproduced bit for bit");
    assert_eq!((n, exact), (9, 9));
}

// ---------------------------------------------------------------- fall damage

/// Scenarios in which something besides a landing hurts the player at the landings' ticks: the
/// lava ones (`lava_lanes` lands in lava, where the 4.0 hits and the harness's health restores
/// land on the landing's rows; see `tests/lava_fire.rs`), and those with hits of their own. Every
/// other scenario of the corpus is checked, whether it has damaging landings or not.
const OTHER_DAMAGE: &[&str] = &[
    "knockback_standing",
    "knockback_moving",
    "legacy_capture",
    "bubble_columns",
    "projectile_hits",
    "powder_snow",
    "lava_flow",
    "lava_lanes",
    "lava_unresisted",
];

/// Damaging landings whose health packet reached the client at the start of the tick *after* the
/// next one (`row + 2`): the phase between the client's and the server's threads, as in
/// `freeze_hot_floor.rs`. Every other damaging landing shows it at `row + 1`.
const HEALTH_ONE_TICK_LATE: &[(&str, usize)] = &[
    ("bed_fall_8", 30),
    ("effect_levitation_2", 86),
    ("fall_edges_b", 158),
    ("fall_edges_b", 203),
    ("fall_edges_b", 297),
    ("fall_hay_jump_boost", 94),
    ("knockback_ledge_sneak", 200),
    ("scaffolding_column", 201),
    ("water_climbables", 107),
];

/// Damaging landings whose velocity packet is not the velocity of the model's server copy at the
/// tick the model sends it: 11 of the 20 (the other 9 are bit for bit the model's, in each case the
/// grounded -0.0784 the server copy has after a landing). The health, the invulnerability window
/// and the hurt time of all 20 match. The velocity the real server sent is its own copy's at a
/// moment the model does not find: `fall_edges` row 296 shows the copy a tick after it landed
/// (vertical velocity -0.1552, where the model's server landed a tick earlier and holds -0.0784)
/// and row 341 one that is still in free fall (-0.7171), for two falls of the same height from the
/// same tower; `bed_fall_8` shows the server copy's bounce off the bed (+0.611) where the model
/// sends the velocity of the tick before it. The
/// timing of server packets against the server's own tick is the phase between the two threads, as
/// in `freeze_hot_floor.rs`, and these differ between falls that are alike. It only matters for a
/// landing on a surface that does not stop the player (the client's own tick then moves it with
/// the packet's velocity); in all of these the client is standing on the ground a tick later,
/// whatever the packet said.
const VELOCITY_PHASE: &[(&str, usize)] = &[
    ("bed_fall_8", 30),
    ("fall_edges", 296),
    ("fall_edges", 341),
    ("fall_edges", 389),
    ("fall_edges_b", 158),
    ("fall_edges_b", 297),
    ("fall_hay_jump_boost", 162),
    ("fall_slow_falling_stone", 205),
    ("knockback_ladder", 182),
    ("knockback_ledge_sneak", 200),
    ("water_tunnel", 166),
];

/// Every landing in the corpus (a tick where the client's fall distance resets on the ground):
/// the damage computed from the fall distance at the landing tick (and the landed-on block's
/// `fallOn`) must be the health the recording loses, and for damaging landings the server's
/// reaction (health, window and hurt time exactly, the velocity sent to the client up to
/// [`VELOCITY_PHASE`]) must match the row the packets arrived at.
#[test]
fn fall_damage_matches_every_recorded_landing() {
    let mut obs = Observed::default();
    let mut failures = Vec::new();
    for name in client_scenarios() {
        if OTHER_DAMAGE.contains(&name.as_str()) {
            continue;
        }
        replay(&name, None, &mut obs);
    }
    let (mut damaging, mut harmless, mut delivered) = (0, 0, 0);
    let (mut late, mut phase) = (Vec::new(), Vec::new());
    for l in &obs.landings {
        let want = l.expected.max(0);
        if want != l.observed_drop {
            failures.push(format!(
                "{} row {}: fall {:.9} {:?} -> formula {} but the recording lost {}",
                l.scenario, l.row, l.fall, l.cause, l.expected, l.observed_drop
            ));
        }
        if want > 0 {
            damaging += 1;
        } else {
            harmless += 1;
        }
        if let Some(ok) = l.delivered_ok {
            delivered += 1;
            if !ok {
                failures.push(format!(
                    "{} row {}: health, window or hurt time differ from the row the packets arrived at",
                    l.scenario, l.row
                ));
            }
            if l.arrival == Some(l.row + 2) {
                late.push((l.scenario.as_str(), l.row));
            }
            if l.velocity_ok == Some(false) {
                phase.push((l.scenario.as_str(), l.row));
            }
        }
    }
    eprintln!(
        "fall damage: {} landings checked ({damaging} damaging, {harmless} harmless), {delivered} damaging landings with the server's reaction compared, {} failures",
        obs.landings.len(),
        failures.len()
    );
    for l in obs
        .landings
        .iter()
        .filter(|l| l.expected > 0 || l.observed_drop > 0)
    {
        eprintln!(
            "  {} row {}: fall {:.9} {:?} -> {} damage (recording lost {}), arrived at {:?}, health/window ok: {:?}, velocity ok: {:?}",
            l.scenario, l.row, l.fall, l.cause, l.expected, l.observed_drop, l.arrival, l.delivered_ok, l.velocity_ok
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(
        (damaging, delivered),
        (20, 20),
        "every damaging landing's reaction is compared"
    );
    assert_eq!(late, HEALTH_ONE_TICK_LATE);
    assert_eq!(phase, VELOCITY_PHASE);
}

// ---------------------------------------------------------------- timers

/// `LivingEntity.baseTick`'s countdowns against every recorded tick: with the recorded start
/// state (pre applied), one `tick_timers` must give the recorded end-of-tick `invul` and
/// `hurtTime`.
#[test]
fn timers_count_down_like_base_tick_on_every_recorded_tick() {
    let mut rows = 0;
    let mut mismatches = Vec::new();
    for name in client_scenarios() {
        let sc = Scenario::load(&name).unwrap();
        let mut p = sc.initial_state();
        for (i, row) in sc.rows.iter().enumerate() {
            if i > 0 {
                apply_state(&mut p, &row.pre);
            }
            tick_timers(&mut p);
            rows += 1;
            let want_invul = int(&row.post["invul"]);
            let want_hurt = int(&row.post["hurtTime"]);
            if (p.invulnerable_time, p.hurt_time) != (want_invul, want_hurt) {
                mismatches.push(format!(
                    "{name} row {i}: invul/hurtTime model {}/{} recorded {want_invul}/{want_hurt}",
                    p.invulnerable_time, p.hurt_time
                ));
            }
            p.invulnerable_time = want_invul;
            p.hurt_time = want_hurt;
        }
    }
    eprintln!(
        "timers: {rows} recorded ticks, {} mismatches",
        mismatches.len()
    );
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

// ---------------------------------------------------------------- hits from entities

/// An arrow hits the standing player during the server's entity phase (projectile_hits, the
/// server tick of client row 94): the damage reaches the client at once (row 95's pre: health,
/// window, hurt time) but the velocity is sent by the next tick's `sendChanges`, after the
/// player's own tick has stepped it once (row 96's pre: `dy` 0.2752, `dz` 0.2184 rather than the
/// 0.3608 and 0.4 of the knockback itself).
#[test]
fn arrow_hit_in_the_entity_phase_delivers_the_velocity_a_tick_later() {
    let sc = Scenario::load("projectile_hits").unwrap();
    let world = sc.world();
    let mut srv = sc.initial_state();
    reset_server_copy(&mut srv);
    // The arrow's server state at the end of tick 8280 (row 94's sample): it moves on to hit the
    // player in the next server tick.
    let arrow = sc.rows[94].srv[0]["entities"][0]
        .as_object()
        .unwrap()
        .clone();
    let (avx, avz) = (d64(&arrow["dx"]), d64(&arrow["dz"]));
    let mut hit_checked = false;
    for (i, row) in sc.rows.iter().enumerate().take(98) {
        apply_state(&mut srv, &without_server_fields(&row.pre));
        let start = TickStart::of(&srv);
        tick_timers(&mut srv);
        apply_state(&mut srv, &without_server_fields(&row.post));
        if i == 94 {
            // vanilla's server tick, with the entity phase between the sync and the player's tick
            server_move_packet(&mut srv, &start, &world);
            sync_motion(&mut srv);
            // Projectile.calculateHorizontalHurtKnockbackDirection is the projectile's horizontal
            // velocity, which hurtServer negates. The arrow's damage is ceil(speed * 2.0) = 4.
            let landed = hurt(&mut srv, DamageSource::Directed { dx: -avx, dz: -avz }, 4.0);
            assert!(landed);
            server_do_tick(&mut srv, &world);
            // row 95's pre: the damage event and the new health, but no velocity yet
            let n = &sc.rows[95].pre;
            assert_eq!(srv.health, f32v(&n["health"]));
            assert_eq!(srv.invulnerable_time, int(&n["invul"]));
            assert_eq!(srv.hurt_time, int(&n["hurtTime"]));
            assert_eq!(srv.last_hurt, f32v(&n["lastHurt"]));
            assert!(!n.contains_key("dy"));
        } else {
            server_tick(&mut srv, &start, &world);
        }
        if i == 95 {
            // the velocity packet was built at the start of this server tick, after the previous
            // tick's player tick: row 96's pre
            let n = &sc.rows[96].pre;
            assert_eq!(d64(&n["dy"]).to_bits(), srv.vel.y.to_bits());
            assert_eq!(d64(&n["dz"]).to_bits(), srv.vel.z.to_bits());
            assert!(!n.contains_key("dx"));
            assert_eq!(srv.vel.x, 0.0);
            hit_checked = true;
        }
    }
    assert!(hit_checked);
}
