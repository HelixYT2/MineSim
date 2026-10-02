//! Powder-snow freezing and magma's hot floor against the oracle corpus (`powder_snow`,
//! `ladder_climb`, `bubble_columns`).
//!
//! Both are the *server's* business: `LivingEntity.aiStep` runs the freeze step (decay, the
//! movement-speed modifier, the damage) only when `level() instanceof ServerLevel`, and
//! `MagmaBlock.stepOn` hurts through `Entity.hurt`, which on the client is a no-op. The recorded
//! client sees the results as packets. Three kinds of check follow:
//!
//! * the **client replay** (`ms_corpus::replay`: `player::tick` plus the recorded `pre` diffs as the
//!   server's packets, the freeze modifier taken from the recorded movement speed) reproduces every
//!   tick of the three scenarios bit for bit;
//! * the **delivery check** runs the server model (`damage::server_move_packet`, `sync_motion`,
//!   `server_do_tick`, which the arena runs after every client tick) beside the recorded client and
//!   compares what it sends with what the recording shows the client received: the freeze counter,
//!   the speed modifier, health and the hurt window. It matches on every row but a handful where the
//!   recorded packet came a client tick away from its nominal tick (below);
//! * the **arena free-run** feeds only the recorded inputs to the whole arena step and compares the
//!   complete state with the recording's.
//!
//! # The residual timing jitter
//!
//! Client and server are two threads ticking at 20 Hz on their own clocks. The model delivers each
//! server result at its nominal client tick: the tracker pass of the next server tick sends
//! `ticksFrozen` and the freeze modifier, so they reach the client two client ticks after the
//! movement they follow; a burn's health and damage event go out at once, one tick. In the
//! recording the phase between the threads drifts by a tick every few dozen ticks, so a packet
//! arrives one client tick late or early and, the other way round, two server ticks' packets are
//! merged into one client tick. That cannot be derived from the game's rules; the rows where it
//! happens are listed in [`POWDER_SNOW_JITTER`] and [`BUBBLE_COLUMNS_JITTER`], checked to be exactly
//! those, and each is checked to be a pure shift (the recorded value is the model's value of the
//! neighbouring row). A free-run of the arena stays exact until the first such row: the speed then
//! differs by one tick's freeze step for one tick, and the resulting difference in position never
//! goes away (`powder_snow`), while `ladder_climb` (no jitter) is exact over all 310 ticks and
//! `bubble_columns` is exact for 163 ticks, through the first seven burns.

use ms_corpus::{apply_pre, diff_state, replay, Fields, Scenario};
use ms_kernel::attributes::Attribute;
use ms_kernel::damage::{self, TickStart};
use ms_kernel::effects;
use ms_kernel::player;
use ms_kernel::{Input, PlayerState};
use ms_world::World;
use serde_json::Value;

fn int(v: &Value) -> i64 {
    v.as_i64().expect("integer field")
}

/// What the client holds at the start of tick `t + 1`: its end-of-tick state `t` with the server's
/// changes of the next row's `pre` applied. (The arena's state after a step is this one: the
/// server's handling of the tick, including what the tracker sends, is already in it.)
fn start_of_next(post: &Fields, next_pre: Option<&Fields>) -> Fields {
    let mut out = post.clone();
    if let Some(pre) = next_pre {
        for (k, v) in pre {
            match (k.as_str(), out.get_mut("attrs"), v) {
                ("attrs", Some(Value::Object(a)), Value::Object(b)) => {
                    for (n, x) in b {
                        a.insert(n.clone(), x.clone());
                    }
                }
                _ => {
                    out.insert(k.clone(), v.clone());
                }
            }
        }
    }
    out
}

fn is_teleport(pre: &Fields) -> bool {
    pre.contains_key("x") || pre.contains_key("y") || pre.contains_key("z")
}

/// The server player's `tickCount` runs 7 ahead of the client's `age` in the recording session
/// (mod 40, which is all the freeze damage looks at). The corpus shows `MinecraftServer.tickCount`
/// exactly 6 ahead of the client's `age` in every scenario of the session (the `serverTick` of the
/// `srv` events), so the offset is a property of when the player joined; the one freeze damage in
/// the corpus (`powder_snow`, delivered at the start of row 176) fixes it.
const SERVER_TICK_COUNT_OFFSET: i32 = 7;

/// Rows (`t`, comparing at the start of `t + 1`) of `powder_snow` where the recorded freeze packet
/// came a client tick off the nominal schedule. Late by a tick: the packet with server count 7
/// (rows 32-33; the next one, 9, is merged into the one after it) and those with 17, 43, 65 and 83
/// (rows 41, 66, 87, 105). Early by a tick: the one with 35 (rows 57-58).
const POWDER_SNOW_JITTER: &[usize] = &[32, 33, 41, 57, 58, 66, 87, 105];

/// Rows of `bubble_columns` where a burn's packets came a tick off the model's schedule. The
/// recorded burns reach the client at rows 95, 105, ..., 155 (every ten ticks), then 164 and 174
/// (a tick ahead of that grid: the model keeps to the grid, so at row 163 the recording already has
/// the burn that the model has not), then 185 and 195 (back on the original grid: at row 183 the
/// model has the burn that the recording does not yet).
const BUBBLE_COLUMNS_JITTER: &[usize] = &[163, 183];

#[test]
fn client_replay_is_bit_exact() {
    for name in ["powder_snow", "ladder_climb", "bubble_columns"] {
        let s = Scenario::load(name).unwrap();
        let r = replay(&s, player::tick);
        assert!(r.is_exact(), "{name}: {:?}", r.first_divergence);
        assert_eq!(r.hash_exact_ticks(), r.ticks, "{name}");
    }
}

/// One arena step on `p`: the client tick, then the server's handling of it.
fn arena_step(p: &mut PlayerState, input: &Input, world: &World) {
    let start = TickStart::of(p);
    player::tick(p, input, world);
    damage::server_move_packet(p, &start, world);
    damage::sync_motion(p);
    damage::server_do_tick(p, world);
}

/// The server's state at the start of a scenario: level with the client, except that the freeze
/// state of a scenario that starts frozen is one server tick ahead (the recording's second tick
/// already shows the first packet's content).
fn seed_server(p: &mut PlayerState, sc: &Scenario, ahead: bool) {
    damage::reset_server_copy(p);
    p.server.tick_count = p.tick_count + SERVER_TICK_COUNT_OFFSET;
    if !ahead {
        return;
    }
    if let Some(row1) = sc.rows.get(1) {
        if let Some(f) = row1.pre.get("frozen").and_then(Value::as_i64) {
            p.server.ticks_frozen = f as i32;
        }
        let ms = row1
            .pre
            .get("attrs")
            .and_then(|a| a.get(Attribute::MovementSpeed.name()))
            .and_then(Value::as_i64);
        if let Some(k) = ms.and_then(|b| effects::frost_ticks_for_movement_speed(p, b as u64)) {
            p.server.frost = k;
        }
    }
}

/// The fields that differ, per row, between the free-running arena and the recording. `lastHurt` is
/// left out: it is the server's field, which the client never receives (the recorded client keeps
/// whatever an earlier scenario left in it) and the arena's single copy keeps up to date.
fn arena_free_run(name: &str, start_frozen_ahead: bool) -> Vec<Vec<String>> {
    let sc = Scenario::load(name).unwrap();
    let world = sc.world();
    let mut p = sc.initial_state();
    seed_server(&mut p, &sc, start_frozen_ahead);
    let mut out = Vec::new();
    for (t, row) in sc.rows.iter().enumerate() {
        if t > 0 && is_teleport(&row.pre) {
            // A teleport: the new position and velocity reach both copies.
            apply_pre(&mut p, &row.pre, Some(&sc.rows[t - 1].post));
            damage::reset_server_copy(&mut p);
        }
        arena_step(&mut p, &row.input, &world);
        let next = sc
            .rows
            .get(t + 1)
            .map(|r| &r.pre)
            .filter(|pre| !is_teleport(pre));
        let want = start_of_next(&row.post, next);
        out.push(
            diff_state(&p, &want)
                .into_iter()
                .map(|d| d.field)
                .filter(|f| f != "lastHurt")
                .collect(),
        );
    }
    out
}

fn rows_with_diffs(diffs: &[Vec<String>]) -> Vec<usize> {
    diffs
        .iter()
        .enumerate()
        .filter(|(_, d)| !d.is_empty())
        .map(|(i, _)| i)
        .collect()
}

/// The freeze slowdown as `frost_ticks_for_movement_speed` reports it.
type Frost = Option<Option<i32>>;

/// What the model sent to the client after row `t` against what the recording shows the client
/// holds at the start of row `t + 1` (`(recorded, model)` in each pair).
#[derive(Clone, Debug, PartialEq)]
struct Delivery {
    /// The freeze counter the client holds.
    frozen: (i64, i64),
    /// The freeze slowdown on its movement speed, as the server `ticksFrozen` it was computed from.
    frost: (Frost, Frost),
    health: (i64, i64),
    invul: (i64, i64),
    hurt_time: (i64, i64),
}

impl Delivery {
    fn matches(&self) -> bool {
        self.frozen.0 == self.frozen.1
            && self.frost.0 == self.frost.1
            && self.health.0 == self.health.1
            && self.invul.0 == self.invul.1
            && self.hurt_time.0 == self.hurt_time.1
    }
}

/// Run the server model next to the recorded client. The client side is the recording (the
/// bit-exact replay, `p`); a copy of it carries the model's server state from row to row and is
/// what the server's packets are delivered to, so a late or early recorded packet cannot leak into
/// the next row. `server_ahead` seeds a scenario that starts frozen.
fn deliveries(name: &str, server_ahead: bool) -> Vec<Delivery> {
    let sc = Scenario::load(name).unwrap();
    let world = sc.world();
    let mut p = sc.initial_state();
    let mut seed = p.clone();
    seed_server(&mut seed, &sc, server_ahead);
    let (mut srv, mut srv_vel) = (seed.server, seed.server_vel);
    let mut out = Vec::new();
    for (t, row) in sc.rows.iter().enumerate() {
        let prev_post = (t > 0).then(|| &sc.rows[t - 1].post);
        apply_pre(&mut p, &row.pre, prev_post);
        let mut m = p.clone();
        m.server = srv;
        m.server_vel = srv_vel;
        if t > 0 && is_teleport(&row.pre) {
            damage::reset_server_copy(&mut m);
        }
        let start = TickStart::of(&p);
        player::tick(&mut p, &row.input, &world);
        player::tick(&mut m, &row.input, &world);
        assert_eq!(m.pos, p.pos, "{name} row {t}: the copy follows the replay");
        damage::server_move_packet(&mut m, &start, &world);
        damage::sync_motion(&mut m);
        damage::server_do_tick(&mut m, &world);
        srv = m.server;
        srv_vel = m.server_vel;
        let Some(next) = sc.rows.get(t + 1) else {
            break;
        };
        let want = start_of_next(&row.post, Some(&next.pre));
        let ms = int(&want["attrs"]["movement_speed"]) as u64;
        let frost = |bits: u64| effects::frost_ticks_for_movement_speed(&m, bits);
        out.push(Delivery {
            frozen: (int(&want["frozen"]), i64::from(m.ticks_frozen)),
            frost: (
                frost(ms),
                frost(m.attributes.value(Attribute::MovementSpeed).to_bits()),
            ),
            health: (int(&want["health"]), i64::from(m.health.to_bits() as i32)),
            invul: (int(&want["invul"]), i64::from(m.invulnerable_time)),
            hurt_time: (int(&want["hurtTime"]), i64::from(m.hurt_time)),
        });
    }
    out
}

fn mismatching(d: &[Delivery]) -> Vec<usize> {
    d.iter()
        .enumerate()
        .filter(|(_, x)| !x.matches())
        .map(|(i, _)| i)
        .collect()
}

/// Every mismatch is a pure shift in time: the recorded freeze modifier (or health) is what the
/// model delivers one row earlier or later, or the row belongs to a mismatch that is (the freeze
/// counter and the modifier travel together, the client's own increments in between make the counter
/// differ on a neighbouring row).
fn assert_pure_shifts(name: &str, d: &[Delivery]) {
    let bad = mismatching(d);
    let shifted = |t: usize| {
        let around = |f: &dyn Fn(&Delivery) -> bool| {
            (t > 0 && f(&d[t - 1])) || (t + 1 < d.len() && f(&d[t + 1]))
        };
        let frost_is_shifted =
            d[t].frost.0 != d[t].frost.1 && around(&|n| n.frost.1 == d[t].frost.0);
        let health_is_shifted =
            d[t].health.0 != d[t].health.1 && around(&|n| n.health.1 == d[t].health.0);
        frost_is_shifted || health_is_shifted
    };
    for &t in &bad {
        let neighbour = (t > 0 && bad.contains(&(t - 1))) || bad.contains(&(t + 1));
        assert!(
            shifted(t) || neighbour,
            "{name} row {t} is not a shifted packet: {:?}",
            d[t]
        );
    }
    // And every group of mismatching rows contains a shifted one.
    for &t in &bad {
        let mut lo = t;
        while lo > 0 && bad.contains(&(lo - 1)) {
            lo -= 1;
        }
        let mut hi = t;
        while bad.contains(&(hi + 1)) {
            hi += 1;
        }
        assert!(
            (lo..=hi).any(shifted),
            "{name} rows {lo}-{hi} hold no shifted packet"
        );
    }
}

#[test]
fn ladder_climb_decay_is_delivered_exactly() {
    // Starts frozen (56 ticks) with no powder snow around: the server thaws two ticks per tick, the
    // client sees each value and the matching modifier a client tick later, for all 310 rows and
    // until the modifier is gone again; no jitter in this recording.
    let d = deliveries("ladder_climb", true);
    assert_eq!(d.len(), 309);
    assert_eq!(mismatching(&d), Vec::<usize>::new());
    assert!(d.iter().any(|x| x.frost.0 == Some(Some(54))));
    assert_eq!(
        d[27].frost,
        (Some(None), Some(None)),
        "thawed after 28 ticks"
    );
    let free = arena_free_run("ladder_climb", true);
    assert_eq!(rows_with_diffs(&free), Vec::<usize>::new());
}

#[test]
fn powder_snow_freezing_is_delivered_as_recorded_up_to_the_phase_jitter() {
    let d = deliveries("powder_snow", false);
    assert_eq!(d.len(), 184);
    assert_eq!(mismatching(&d), POWDER_SNOW_JITTER);
    assert_pure_shifts("powder_snow", &d);
    // 176 rows are exact; among them the whole fully-frozen stretch with its freeze damage at the
    // 40th server tick (the player is hurt at the start of row 176: 20 -> 19, window 20, hurt 10).
    let hit = &d[175];
    assert_eq!(
        hit.health,
        (
            i64::from(19.0_f32.to_bits() as i32),
            i64::from(19.0_f32.to_bits() as i32)
        )
    );
    assert_eq!((hit.invul, hit.hurt_time), ((20, 20), (10, 10)));
    assert!(d[161..175].iter().all(|x| x.frost.0 == Some(Some(140))));
    // The delay of the first packets: the player enters powder snow in row 25 (client count 1 and
    // `powder`); the slowdown first shows at the start of row 27.
    let sc = Scenario::load("powder_snow").unwrap();
    assert_eq!(int(&sc.rows[24].post["powder"]), 0);
    assert_eq!(int(&sc.rows[25].post["powder"]), 1);
    assert!(!sc.rows[26].pre.contains_key("attrs"));
    assert!(sc.rows[27].pre.contains_key("attrs"));
    assert_eq!(int(&sc.rows[27].pre["frozen"]), 1);
}

#[test]
fn powder_snow_arena_free_run_is_exact_until_the_first_packet_slips() {
    let d = arena_free_run("powder_snow", false);
    let rows = rows_with_diffs(&d);
    assert_eq!(rows.first(), Some(&POWDER_SNOW_JITTER[0]));
    assert_eq!(
        d[POWDER_SNOW_JITTER[0]],
        ["frozen", "speed", "attrs.movement_speed"]
    );
}

#[test]
fn magma_burns_happen_in_the_server_tick_that_lands_ahead_of_the_client() {
    let sc = Scenario::load("bubble_columns").unwrap();
    // The player sinks in the downward bubble column: the client's first tick on the magma is row
    // 95, but the first burn (health 19, hurt window 20, hurt time 10) is already in the packets the
    // client gets at the start of row 95: the server, whose copy follows the column's pull and the
    // position the client reported at the end of row 94, landed in its tick of row 94.
    assert_eq!(int(&sc.rows[94].post["ground"]), 0);
    assert_eq!(int(&sc.rows[95].post["ground"]), 1);
    assert_eq!(
        f32::from_bits(int(&sc.rows[94].post["health"]) as i32 as u32),
        20.0
    );
    assert_eq!(
        f32::from_bits(int(&sc.rows[95].pre["health"]) as i32 as u32),
        19.0
    );
    assert_eq!(int(&sc.rows[95].pre["invul"]), 20);
    assert_eq!(int(&sc.rows[95].pre["hurtTime"]), 10);
    // The model reproduces it, and the burns that follow every ten ticks while the player stands
    // on the magma (the window lets a hit through again at 10): the first seven burns, bit for bit
    // over the whole state.
    let free = arena_free_run("bubble_columns", false);
    assert!(free[..BUBBLE_COLUMNS_JITTER[0]].iter().all(Vec::is_empty));
    let burns: Vec<usize> = (1..sc.rows.len())
        .filter(|&t| {
            sc.rows[t].pre.get("health").is_some_and(|h| {
                f32::from_bits(int(h) as i32 as u32)
                    < f32::from_bits(int(&sc.rows[t - 1].post["health"]) as i32 as u32)
            })
        })
        .collect();
    assert_eq!(
        burns,
        [95, 105, 115, 125, 135, 145, 155, 164, 174, 185, 195],
        "the recorded burns reach the client at these rows"
    );
}

#[test]
fn magma_burn_packets_slip_by_single_ticks_in_the_recording() {
    let d = deliveries("bubble_columns", false);
    assert_eq!(d.len(), 199);
    assert_eq!(mismatching(&d), BUBBLE_COLUMNS_JITTER);
    assert_pure_shifts("bubble_columns", &d);
    // Everything else, burns and hurt windows included, is as recorded.
    let free = arena_free_run("bubble_columns", false);
    let rows = rows_with_diffs(&free);
    assert_eq!(rows.first(), Some(&BUBBLE_COLUMNS_JITTER[0]));
    let allowed = ["invul", "health", "hurtTime", "dx", "dy", "dz"];
    for &r in &rows {
        assert!(
            free[r].iter().all(|f| allowed.contains(&f.as_str())),
            "row {r}: {:?}",
            free[r]
        );
    }
}
