//! Lava, fire and sweet berry bushes against the oracle corpus (`lava_flow`, `lava_lanes`,
//! `lava_unresisted`, `lava_pool`, `cobweb_berries`).
//!
//! All of it is the *server's* business. `Entity.lavaHurt` and `SweetBerryBushBlock.entityInside`
//! only hurt when the level is a `ServerLevel`, and the fire counter that keeps burning a player
//! after it left the lava is the server's too (`Entity.baseTick` clears the client's every tick). The
//! recorded client sees the results as packets: a damage event with the new health, and, because
//! the hit marks the player (`hurtMarked`), the server's velocity squeezed through the `LpVec3`
//! codec, which *replaces* the client's. So the kernel's client tick must not hurt (that, together
//! with lava being pushed twice in the tick that entered it, was behind the oracle's last two
//! divergences), and `damage::server_do_tick` does:
//!
//! * the server's own fluid state at the position the client reported, and the push of flowing lava;
//! * the fire counter: 300 ticks from every lava contact, 1.0 of fire damage on every 20th tick
//!   outside lava, cleared by water, refused by fire resistance;
//! * the blocks its own movement (the move packets, then its `travel`) passes through: lava ignites
//!   and hurts with 4.0, a grown berry bush with 1.0.
//!
//! Three kinds of check follow.
//!
//! * the **client replay** (`player::tick` and the recorded `pre` diffs, which carry the server's
//!   packets) reproduces every tick of the five scenarios bit for bit;
//! * the **hit check** runs the server model beside the recorded client, with the model owning its
//!   health and invulnerability window, and compares every hit it deals with every health drop the
//!   recording shows: same number, same amounts, same health before and after;
//! * the **velocity check** compares every velocity packet of the recording with the model's
//!   `sync_motion` deliveries: where they agree they agree bit for bit, the `LpVec3`-quantized
//!   velocity of the model's server copy.
//!
//! # What the recording shows that the model does not reproduce
//!
//! Client and server run on their own clocks, so a server packet reaches the client a tick early or
//! late now and then (`freeze_hot_floor.rs`). The model delivers a hit's health at the client tick
//! after the server tick that dealt it and the velocity one tick after that; the recording shows
//! the health up to two ticks off that schedule ([`HIT_ROWS`]) and the velocity up to three ticks
//! after the hit ([`VELOCITY_PACKETS`]). Two velocity packets come out of the model's schedule
//! altogether ([`UNMATCHED_PACKETS`]). In `lava_flow` the last burn's packet (row 76, in the air
//! after a jump) carries the velocity of the server tick *before* the one the model burns in: the
//! server's jump of that hop was handled a tick later than the model handles it, or its burn came a
//! tick earlier (the model's burn rhythm of 20 ticks agrees with the recorded health changes, and
//! the intervals between all hits agree with the recording's to within a tick, but this packet is
//! the one observation that could tell the two apart). In `lava_lanes` (row 201, the sprint-jump
//! into the fourth lane) the server's horizontal velocity differs from the model's by two steps of
//! the codec (0.00012), a slightly different push by the flow.
//!
//! One rule comes from the recording alone. The reference reads as if the entity-inside check of a
//! lava block (a block with the default full-cube shape) marked the block as visited on first
//! contact, so that a server copy that touches a lava cell with the move packet (box overlapping the
//! cell above the fluid) and enters the fluid in its own `travel` of the same tick would not be
//! ignited. The recorded hurt packets say it is: with that rule the model burns `lava_unresisted`
//! (row 24 -> 25) and the fourth lane of `lava_lanes` (twice) a tick later than the recording's
//! velocity packets, which carry the velocity of the earlier tick, and 6 of the 19 packets match no
//! delivery of the model; without it 17 do (`blocks::Walk::visit_block`).
//!
//! The corpus has no recording of a berry bush hurting the player (`cobweb_berries` gets stuck in
//! its cobwebs before it reaches the bushes), so the berry rule is checked on a synthetic course
//! only, against the reference code's rule.

use ms_corpus::{apply_pre, replay, Fields, Scenario};
use ms_kernel::damage::{self, TickStart};
use ms_kernel::player;
use ms_kernel::{Input, PlayerState};
use ms_numerics::Vec3;
use ms_world::{FlatWorld, GridWorld, World};
use serde_json::Value;

fn int(v: &Value) -> i64 {
    v.as_i64().expect("integer field")
}

fn health_of(f: &Fields) -> f32 {
    f32::from_bits(int(&f["health"]) as i32 as u32)
}

/// What the client holds at the start of tick `t + 1`: the end of tick `t` with the server's
/// changes of the next row applied.
fn start_of_next(post: &Fields, next_pre: &Fields) -> Fields {
    let mut out = post.clone();
    for (k, v) in next_pre {
        out.insert(k.clone(), v.clone());
    }
    out
}

fn is_teleport(pre: &Fields) -> bool {
    pre.contains_key("x") || pre.contains_key("y") || pre.contains_key("z")
}

/// A hit: the row (server tick) it was dealt in or shown at, and the health before and after.
type Hit = (usize, f32, f32);

/// The server model run beside the recorded client.
struct Run {
    /// The hits the model dealt, by the step (recorded row) whose move packet it handled.
    model_hits: Vec<Hit>,
    /// The health changes the recording shows, by the row whose start shows them.
    recorded_hits: Vec<Hit>,
    /// The client velocity the model's `sync_motion` delivered in each step, if it delivered one.
    delivered: Vec<Option<Vec3>>,
    /// Every velocity packet of the recording that is not part of a teleport: the row it arrives at
    /// and the velocity the client holds then.
    packets: Vec<(usize, Vec3)>,
    /// The model's server fire counter after each step.
    fire: Vec<i32>,
}

/// Run `name`. The client side is the recording (`apply_pre` on a replayed state); the server side is
/// the model, whose health, window and fire counter are its own from step to step (they are only
/// copied from the recording where the harness restores the player, which also teleports it).
fn run(name: &str) -> Run {
    let sc = Scenario::load(name).unwrap();
    let world = sc.world();
    let mut p = sc.initial_state();
    let mut seed = p.clone();
    damage::reset_server_copy(&mut seed);
    let (mut srv, mut srv_vel) = (seed.server, seed.server_vel);
    let owned = |p: &PlayerState| {
        (
            p.health,
            p.absorption,
            p.invulnerable_time,
            p.hurt_time,
            p.last_hurt,
        )
    };
    let mut dmg = owned(&p);
    let mut out = Run {
        model_hits: Vec::new(),
        recorded_hits: Vec::new(),
        delivered: Vec::new(),
        packets: Vec::new(),
        fire: Vec::new(),
    };
    for (t, row) in sc.rows.iter().enumerate() {
        let prev_post = (t > 0).then(|| &sc.rows[t - 1].post);
        apply_pre(&mut p, &row.pre, prev_post);
        let teleport = t > 0 && is_teleport(&row.pre);
        if let Some(prev) = prev_post {
            if let Some(h) = row.pre.get("health") {
                let h = f32::from_bits(int(h) as i32 as u32);
                if h != health_of(prev) {
                    out.recorded_hits.push((t, health_of(prev), h));
                }
            }
            if !teleport && ["dx", "dy", "dz"].iter().any(|k| row.pre.contains_key(*k)) {
                let st = start_of_next(prev, &row.pre);
                let g = |k: &str| f64::from_bits(int(&st[k]) as u64);
                out.packets.push((t, Vec3::new(g("dx"), g("dy"), g("dz"))));
            }
        }
        // The model's copy of the player: the client state of the replay, the server's own state.
        let mut m = p.clone();
        m.server = srv;
        m.server_vel = srv_vel;
        if teleport {
            damage::reset_server_copy(&mut m);
            // The harness put the player back (health, fire) between lanes: that is the recording's.
            dmg = owned(&p);
        }
        (
            m.health,
            m.absorption,
            m.invulnerable_time,
            m.hurt_time,
            m.last_hurt,
        ) = dmg;
        let start = TickStart::of(&p);
        player::tick(&mut p, &row.input, &world);
        let health_before = m.health;
        player::tick(&mut m, &row.input, &world);
        assert_eq!(m.pos, p.pos, "{name} row {t}: the copy follows the replay");
        damage::server_move_packet(&mut m, &start, &world);
        let marked = m.server.hurt_marked;
        damage::sync_motion(&mut m);
        out.delivered.push(marked.then_some(m.vel));
        damage::server_do_tick(&mut m, &world);
        if m.health != health_before {
            out.model_hits.push((t, health_before, m.health));
        }
        out.fire.push(m.server.fire_ticks);
        srv = m.server;
        srv_vel = m.server_vel;
        dmg = owned(&m);
    }
    out
}

const LAVA_SCENARIOS: [&str; 5] = [
    "lava_flow",
    "lava_lanes",
    "lava_unresisted",
    "lava_pool",
    "cobweb_berries",
];

#[test]
fn client_replay_is_bit_exact() {
    // The client's own tick ignites the player (`fire` 300 in the tick it touches lava) and is
    // pushed by flowing lava once, but never hurts itself.
    for name in LAVA_SCENARIOS {
        let s = Scenario::load(name).unwrap();
        let r = replay(&s, player::tick);
        assert!(r.is_exact(), "{name}: {:?}", r.first_divergence);
        assert_eq!(r.hash_exact_ticks(), r.ticks, "{name}");
    }
}

/// Every hit of the model against every health change of the recording, as
/// `(step the model dealt it in, row the recording shows it at, health before, health after)`.
/// The recording shows a hit at `step + 1` when the packets keep to the model's schedule.
type HitRow = (usize, usize, f32, f32);

const HIT_ROWS: &[(&str, &[HitRow])] = &[
    (
        "lava_flow",
        &[
            (21, 22, 20.0, 16.0),
            (31, 31, 16.0, 12.0),
            (54, 55, 12.0, 11.0),
            (74, 75, 11.0, 10.0),
        ],
    ),
    (
        "lava_unresisted",
        &[
            (24, 25, 20.0, 16.0),
            (34, 34, 16.0, 12.0),
            (59, 59, 12.0, 11.0),
        ],
    ),
    (
        "lava_lanes",
        &[
            (21, 22, 20.0, 16.0),
            (31, 32, 16.0, 12.0),
            (57, 58, 12.0, 11.0),
            (82, 84, 20.0, 16.0),
            (92, 93, 16.0, 12.0),
            (118, 119, 12.0, 11.0),
            (136, 137, 20.0, 16.0),
            (146, 146, 16.0, 12.0),
            (173, 174, 12.0, 11.0),
            (199, 201, 20.0, 16.0),
            (210, 212, 16.0, 12.0),
            (220, 221, 12.0, 8.0),
        ],
    ),
    ("lava_pool", &[]),
    ("cobweb_berries", &[]),
];

/// The health restores the harness of `lava_lanes` made between the lanes (the recording's `pre` of
/// these rows holds a higher health than the tick before).
const RESTORES: &[(&str, &[usize])] = &[("lava_lanes", &[62, 122, 179])];

#[test]
fn the_server_deals_the_hits_the_recording_shows() {
    let mut total = 0;
    for (name, expected) in HIT_ROWS {
        let r = run(name);
        let restores = RESTORES
            .iter()
            .find(|(n, _)| n == name)
            .map_or(&[][..], |(_, rows)| rows);
        let shown: Vec<Hit> = r
            .recorded_hits
            .iter()
            .copied()
            .filter(|(row, ..)| !restores.contains(row))
            .collect();
        assert_eq!(shown.len(), expected.len(), "{name}: recorded {shown:?}");
        assert_eq!(
            r.model_hits.len(),
            expected.len(),
            "{name}: model {:?}",
            r.model_hits
        );
        for ((step, row, before, after), (m, s)) in
            expected.iter().zip(r.model_hits.iter().zip(&shown))
        {
            assert_eq!(*m, (*step, *before, *after), "{name}: model hit");
            assert_eq!(*s, (*row, *before, *after), "{name}: recorded hit");
            // The same hit: same health before and after, at most two ticks off the schedule.
            let late = *row as i64 - (*step as i64 + 1);
            assert!(
                (-2..=1).contains(&late),
                "{name}: step {step} shown at {row}"
            );
            total += 1;
        }
        for &row in restores {
            assert!(
                r.recorded_hits.iter().any(|(t, ..)| *t == row),
                "{name}: restore at {row}"
            );
        }
    }
    assert_eq!(total, 19, "4 + 3 + 12 hits");
}

/// The recorded velocity packets that the model's deliveries reproduce:
/// `(row the packet arrives at, step of the model's `sync_motion` that delivers the same bits)`.
const VELOCITY_PACKETS: &[(&str, &[(usize, usize)])] = &[
    ("lava_flow", &[(22, 22), (33, 32), (55, 55)]),
    ("lava_unresisted", &[(27, 25), (36, 35), (60, 60)]),
    (
        "lava_lanes",
        &[
            (23, 22),
            (33, 32),
            (59, 58),
            (84, 83),
            (95, 93),
            (121, 119),
            (138, 137),
            (147, 147),
            (176, 174),
            (212, 211),
            (222, 221),
        ],
    ),
];

/// Packets of the recording that no delivery of the model reproduces (see the module docs).
const UNMATCHED_PACKETS: &[(&str, usize)] = &[("lava_flow", 76), ("lava_lanes", 201)];

#[test]
fn the_hurt_packets_carry_the_servers_velocity() {
    let mut matched = 0;
    let mut unmatched = Vec::new();
    for name in ["lava_flow", "lava_unresisted", "lava_lanes"] {
        let r = run(name);
        let want = VELOCITY_PACKETS.iter().find(|(n, _)| *n == name).unwrap().1;
        let mut got = Vec::new();
        for (row, vel) in &r.packets {
            // The model delivers a hit's velocity in the step after the hit; the recording shows
            // it between none and three client ticks later (the latest delivery of the model that
            // has the packet's bits is the one it carries).
            let step = (row.saturating_sub(4)..=*row)
                .rev()
                .find(|&k| r.delivered.get(k).copied().flatten() == Some(*vel));
            match step {
                Some(k) => got.push((*row, k)),
                None => unmatched.push((name, *row)),
            }
        }
        assert_eq!(got, want, "{name}");
        matched += got.len();
    }
    assert_eq!(unmatched, UNMATCHED_PACKETS);
    assert_eq!(matched, 17, "of 19 velocity packets");
}

#[test]
fn the_server_burns_on_for_twenty_ticks_a_hit_outside_lava_and_water_puts_it_out() {
    for (name, last_contact) in [("lava_unresisted", 38), ("lava_flow", 33)] {
        let r = run(name);
        // The fire counter is set to 300 in every tick of lava contact and counts down from the
        // next one, so the first burn after the last contact is 21 ticks after it (the burn of the
        // tick after the contact falls into the invulnerability window of the lava hit and is
        // refused) and the next one 20 ticks later.
        assert_eq!(r.fire[last_contact], 300, "{name}");
        assert_eq!(r.fire[last_contact + 1], 299, "{name}");
        let burns: Vec<usize> = r
            .model_hits
            .iter()
            .filter(|(_, before, after)| (before - after - 1.0).abs() < f32::EPSILON)
            .map(|(step, ..)| *step)
            .collect();
        assert_eq!(burns[0], last_contact + 21, "{name}: {burns:?}");
        assert!(
            burns.windows(2).all(|w| w[1] - w[0] == 20),
            "{name}: {burns:?}"
        );
        // Water ends it (these scenarios end in the pool).
        assert_eq!(*r.fire.last().unwrap(), -20, "{name}");
    }
}

#[test]
fn fire_resistance_refuses_the_damage_but_not_the_fire() {
    let r = run("lava_pool");
    assert!(r.model_hits.is_empty());
    assert!(r.recorded_hits.is_empty());
    // It still catches fire (300 ticks in lava).
    assert!(r.fire.contains(&300));
}

// ---------------------------------------------------------------------------------------------
// Sweet berry bushes (synthetic: the corpus never reaches one)
// ---------------------------------------------------------------------------------------------

/// A flat world with a sweet berry bush of the given age in front of the player.
fn berry_world(age: u8) -> World {
    let mut grid = GridWorld::new(FlatWorld::new(
        0,
        ms_data::parse_state("minecraft:stone").unwrap(),
    ));
    let bush = ms_data::parse_state(&format!("minecraft:sweet_berry_bush[age={age}]")).unwrap();
    grid.set_block(0, 0, 3, bush);
    World::grid(grid)
}

/// One game tick as the arena runs it.
fn step(p: &mut PlayerState, input: &Input, world: &World) {
    let start = TickStart::of(p);
    player::tick(p, input, world);
    damage::server_move_packet(p, &start, world);
    damage::sync_motion(p);
    damage::server_do_tick(p, world);
}

fn walk() -> Input {
    Input {
        forward: true,
        ..Input::default()
    }
}

#[test]
fn a_grown_bush_hurts_the_walking_player_on_the_server_only() {
    let world = berry_world(3);
    let mut p = PlayerState::new(Vec3::new(0.5, 0.0, 0.5), 0.0);
    let mut hits = Vec::new();
    for t in 0..60 {
        let before = p.health;
        // The client's own tick never hurts.
        let start = TickStart::of(&p);
        player::tick(&mut p, &walk(), &world);
        assert_eq!(
            p.health, before,
            "tick {t}: the client does not hurt itself"
        );
        damage::server_move_packet(&mut p, &start, &world);
        damage::sync_motion(&mut p);
        damage::server_do_tick(&mut p, &world);
        if p.health != before {
            hits.push((t, before, p.health));
        }
    }
    // The first contact hurts with 1.0; the player is slowed by the bush (0.8 x 0.75 x 0.8) and
    // keeps moving in it, so it is hurt again each time the invulnerability window (10 ticks of it
    // block the 1.0) is over.
    assert!(hits.len() >= 2, "{hits:?}");
    assert_eq!(hits[0].1 - hits[0].2, 1.0);
    assert!(hits.windows(2).all(|w| w[1].0 - w[0].0 == 10), "{hits:?}");
}

#[test]
fn a_young_bush_does_not_hurt_and_a_player_standing_in_a_grown_one_is_not_moving() {
    // age 0: no damage at all.
    let world = berry_world(0);
    let mut p = PlayerState::new(Vec3::new(0.5, 0.0, 0.5), 0.0);
    for _ in 0..60 {
        step(&mut p, &walk(), &world);
    }
    assert_eq!(p.health, 20.0);
    assert!(p.pos.z > 3.0, "it walked through the bush");

    // A grown bush with the player standing still inside it: the server knows of no movement
    // (`getKnownMovement` is zero), so nothing happens.
    let world = berry_world(3);
    let mut p = PlayerState::new(Vec3::new(0.5, 0.0, 3.5), 0.0);
    for _ in 0..40 {
        step(&mut p, &Input::default(), &world);
    }
    assert_eq!(p.health, 20.0);
}
