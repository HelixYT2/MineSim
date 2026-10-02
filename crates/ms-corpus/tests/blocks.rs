//! The block-behaviour port against the recorded client corpus: the climbable flag, the stuck-speed
//! multiplier and powder-snow flags set by `Entity.applyEffectsFromBlocks`, the slime/bed landing
//! velocity, and the entity-dependent collision shapes (scaffolding, powder snow) through the
//! vertical collision they produce.
//!
//! Each check replays the recorded ticks the way the integrated tick will call the module: the
//! state at the start of a tick is the previous tick's recorded end state plus the server's diff
//! (`pre`); the module under test is run on the pieces of the tick it owns, fed with the recorded
//! end-of-move position (the collision step belongs to the core tick); the result is compared with
//! the recorded `post`, bit for bit.

use ms_corpus::{apply_state, client_scenarios, Fields, Scenario};
use ms_kernel::attributes::Attribute;
use ms_kernel::blocks::{self, Movement, MovementLog};
use ms_kernel::PlayerState;
use ms_numerics::Vec3;
use ms_world::aabb::Aabb;
use serde_json::Value;

fn flag(f: &Fields, key: &str) -> bool {
    f.get(key).and_then(Value::as_i64) == Some(1)
}

fn double(f: &Fields, key: &str) -> f64 {
    f64::from_bits(f[key].as_i64().expect("f64 bits") as u64)
}

fn full_state(fields: &Fields) -> PlayerState {
    let mut p = PlayerState::new(Vec3::ZERO, 0.0);
    apply_state(&mut p, fields);
    p
}

/// Running count of what a check looked at, and what it got wrong.
#[derive(Default)]
struct Tally {
    checked: usize,
    interesting: usize,
    skipped: usize,
    failures: Vec<String>,
}

impl Tally {
    fn fail(&mut self, scenario: &str, tick: usize, what: String) {
        if self.failures.len() < 20 {
            self.failures
                .push(format!("{scenario} tick {tick}: {what}"));
        } else if self.failures.len() == 20 {
            self.failures.push("...".into());
        }
    }
}

// ---------------------------------------------------------------------------------------------
// onClimbable
// ---------------------------------------------------------------------------------------------

fn check_climbable(name: &str, tally: &mut Tally) {
    let scenario = Scenario::load(name).unwrap();
    let world = scenario.world();
    for row in &scenario.rows {
        // `climbable` is recorded after the tick: onClimbable() at the end-of-tick position.
        let p = full_state(&row.post);
        let want = flag(&row.post, "climbable");
        let got = blocks::on_climbable(&p, &world);
        tally.checked += 1;
        tally.interesting += usize::from(want);
        if got != want {
            tally.fail(
                name,
                row.t,
                format!("climbable: recorded {want}, simulated {got}"),
            );
        }
    }
}

#[test]
fn climbable_flag_matches_the_named_scenarios() {
    let mut tally = Tally::default();
    for name in [
        "ladder_climb",
        "vines_scaffolding",
        "random_course_a",
        "random_course_b",
    ] {
        let before = (tally.checked, tally.interesting);
        check_climbable(name, &mut tally);
        eprintln!(
            "climbable {name}: {} ticks, {} climbing",
            tally.checked - before.0,
            tally.interesting - before.1
        );
    }
    assert!(tally.failures.is_empty(), "{:#?}", tally.failures);
    // The scenarios must really exercise climbing, or the check proves nothing.
    assert!(
        tally.interesting > 200,
        "only {} climbing ticks",
        tally.interesting
    );
}

#[test]
fn climbable_flag_matches_every_scenario() {
    let mut tally = Tally::default();
    for name in client_scenarios() {
        check_climbable(&name, &mut tally);
    }
    eprintln!(
        "climbable, whole corpus: {} ticks, {} climbing",
        tally.checked, tally.interesting
    );
    assert!(tally.failures.is_empty(), "{:#?}", tally.failures);
}

// ---------------------------------------------------------------------------------------------
// Entity.applyEffectsFromBlocks: stuck multiplier and powder snow
// ---------------------------------------------------------------------------------------------

/// Replay the effects-from-blocks step of every recorded tick. The start-of-tick state is rebuilt
/// from the recording; the glue then does what the tick does before the effects run — the base
/// tick moves `isInPowderSnow` into `wasInPowderSnow` and clears it, and `Entity.move` consumes
/// the stuck multiplier — puts the player at the recorded end-of-move position, and records the
/// move. `with_axes` supplies the move's axis-dependent original motion (estimated as the move
/// itself); without it the move is walked as one straight segment.
fn check_inside_effects(name: &str, with_axes: bool, tally: &mut Tally) {
    let scenario = Scenario::load(name).unwrap();
    let world = scenario.world();
    let mut p = scenario.initial_state();
    for (i, row) in scenario.rows.iter().enumerate() {
        if i > 0 {
            apply_state(&mut p, &row.pre);
        }
        let from = p.pos;
        // Entity.baseTick
        p.was_in_powder_snow = p.in_powder_snow;
        p.in_powder_snow = false;
        // Entity.move: a non-negligible stuck multiplier is applied to the motion and cleared
        let s = p.stuck_speed_multiplier;
        if s.x * s.x + s.y * s.y + s.z * s.z > 1.0E-7 {
            p.stuck_speed_multiplier = Vec3::ZERO;
        }
        // the move's result
        let to = Vec3::new(
            double(&row.post, "x"),
            double(&row.post, "y"),
            double(&row.post, "z"),
        );
        p.pos = to;
        p.on_ground = flag(&row.post, "ground");
        let mut log = MovementLog::new();
        if to != from {
            let original =
                with_axes.then(|| Vec3::new(to.x - from.x, to.y - from.y, to.z - from.z));
            log.record(Movement::new(from, to, original));
        }
        blocks::apply_effects_from_blocks(&mut p, &world, &mut log, from);

        tally.checked += 1;
        let stuck = p.stuck_speed_multiplier;
        let want_stuck = (
            double(&row.post, "stuckX"),
            double(&row.post, "stuckY"),
            double(&row.post, "stuckZ"),
        );
        let got_stuck = (stuck.x, stuck.y, stuck.z);
        let interesting = want_stuck != (0.0, 0.0, 0.0)
            || flag(&row.post, "powder")
            || flag(&row.post, "wasPowder");
        tally.interesting += usize::from(interesting);
        let bits = |t: (f64, f64, f64)| (t.0.to_bits(), t.1.to_bits(), t.2.to_bits());
        if bits(got_stuck) != bits(want_stuck) {
            tally.fail(
                name,
                row.t,
                format!("stuck: recorded {want_stuck:?}, simulated {got_stuck:?}"),
            );
        }
        // Sticking in a block also resets the fall distance (`makeStuckInBlock`), which the
        // recording shows as a zero at the end of the tick.
        if got_stuck != (0.0, 0.0, 0.0)
            && p.fall_distance.to_bits() != double(&row.post, "fall").to_bits()
        {
            tally.fail(
                name,
                row.t,
                format!(
                    "fall distance: recorded {:e}, simulated {:e}",
                    double(&row.post, "fall"),
                    p.fall_distance
                ),
            );
        }
        if p.in_powder_snow != flag(&row.post, "powder") {
            tally.fail(
                name,
                row.t,
                format!(
                    "powder: recorded {}, simulated {}",
                    flag(&row.post, "powder"),
                    p.in_powder_snow
                ),
            );
        }
        if p.was_in_powder_snow != flag(&row.post, "wasPowder") {
            tally.fail(
                name,
                row.t,
                format!(
                    "wasPowder: recorded {}, simulated {}",
                    flag(&row.post, "wasPowder"),
                    p.was_in_powder_snow
                ),
            );
        }
        // continue from the recorded end state
        apply_state(&mut p, &row.post);
    }
}

#[test]
fn stuck_multiplier_and_powder_snow_match_cobwebs_and_powder_snow() {
    for with_axes in [false, true] {
        let mut tally = Tally::default();
        for name in ["cobweb_berries", "powder_snow"] {
            let before = (tally.checked, tally.interesting);
            check_inside_effects(name, with_axes, &mut tally);
            eprintln!(
                "inside effects ({}) {name}: {} ticks, {} with a stuck multiplier or powder flag",
                if with_axes {
                    "axis walk"
                } else {
                    "straight segment"
                },
                tally.checked - before.0,
                tally.interesting - before.1
            );
        }
        assert!(tally.failures.is_empty(), "{:#?}", tally.failures);
        assert!(
            tally.interesting > 200,
            "only {} interesting ticks",
            tally.interesting
        );
    }
}

#[test]
fn nothing_is_stuck_or_frozen_anywhere_else_in_the_corpus() {
    // Every other scenario records a zero stuck multiplier and no powder snow throughout, so the
    // module must stay quiet on all of their blocks (honey, soul sand, slime, ladders, water, ...).
    let mut tally = Tally::default();
    for name in client_scenarios() {
        if name == "cobweb_berries" || name == "powder_snow" {
            continue;
        }
        check_inside_effects(&name, true, &mut tally);
    }
    eprintln!(
        "inside effects, rest of the corpus: {} ticks",
        tally.checked
    );
    assert!(tally.failures.is_empty(), "{:#?}", tally.failures);
}

// ---------------------------------------------------------------------------------------------
// Block.updateEntityMovementAfterFallOn: slime and bed bounce
// ---------------------------------------------------------------------------------------------

/// For every tick whose move ended in a downward collision (a landing), the block landed on
/// (`getOnPosLegacy` at the recorded end position) decides the vertical velocity, which the rest of
/// `travelInAir` then turns into the recorded end-of-tick velocity: `(vy - gravity) * 0.98F`.
/// The velocity going into the move is the recorded start-of-tick one, with the `|vy| < 0.003`
/// rounding of `aiStep`; ticks with a jump from the ground, in fluid, on a ladder, stuck, or
/// under a movement effect have other terms and are skipped.
fn check_landings(name: &str, tally: &mut Tally) {
    let scenario = Scenario::load(name).unwrap();
    let world = scenario.world();
    let mut p = scenario.initial_state();
    for (i, row) in scenario.rows.iter().enumerate() {
        if i > 0 {
            apply_state(&mut p, &row.pre);
        }
        let landed = flag(&row.post, "vc") && flag(&row.post, "vcb");
        let other_terms = [
            p.on_ground && row.input.jump,
            p.in_water,
            p.in_lava,
            flag(&row.post, "water"),
            flag(&row.post, "lava"),
            flag(&row.post, "climbable"),
            flag(&row.pre, "climbable"),
            blocks::on_climbable(&p, &world),
            !p.effects.is_empty(),
            p.flying,
            p.stuck_speed_multiplier != Vec3::ZERO,
            p.in_powder_snow,
        ];
        let simple = !other_terms.iter().any(|&term| term);
        if landed && !simple {
            tally.skipped += 1;
        }
        if landed && simple {
            let end = full_state(&row.post);
            let on = blocks::on_pos(&end, &world, 0.2);
            let mut vy = p.vel.y;
            if vy.abs() < 0.003 {
                vy = 0.0;
            }
            let mut q = end.clone();
            q.vel = Vec3::new(0.0, vy, 0.0);
            blocks::after_fall_on(&mut q, &world, on);
            let gravity = p.attributes.value(Attribute::Gravity);
            let got = (q.vel.y - gravity) * f64::from(0.98_f32);
            let want = double(&row.post, "dy");
            tally.checked += 1;
            let block = world.block_name(on.0, on.1, on.2);
            tally.interesting +=
                usize::from(block == "minecraft:slime_block" || block.ends_with("_bed"));
            if got.to_bits() != want.to_bits() {
                tally.fail(
                    name,
                    row.t,
                    format!("landing on {block} with vy {vy:e}: recorded dy {want:e}, simulated {got:e}"),
                );
            }
        }
        apply_state(&mut p, &row.post);
    }
}

#[test]
fn slime_bounce_velocity_matches_the_recording() {
    let mut tally = Tally::default();
    check_landings("slime_block", &mut tally);
    eprintln!(
        "slime_block landings: {} checked, {} on slime, {} skipped",
        tally.checked, tally.interesting, tally.skipped
    );
    assert!(tally.failures.is_empty(), "{:#?}", tally.failures);
    assert!(
        tally.interesting >= 30,
        "only {} slime landings",
        tally.interesting
    );
}

#[test]
fn landing_velocity_matches_everywhere_else_it_is_simple() {
    // Landings on ordinary blocks (and the bed scenario, whose recording never leaves the tower)
    // reduce to the default zeroing; this guards the fall-on dispatch against misfiring.
    let mut tally = Tally::default();
    for name in client_scenarios() {
        check_landings(&name, &mut tally);
    }
    eprintln!(
        "landings, whole corpus: {} checked ({} on slime or bed), {} skipped",
        tally.checked, tally.interesting, tally.skipped
    );
    assert!(tally.failures.is_empty(), "{:#?}", tally.failures);
}

// ---------------------------------------------------------------------------------------------
// Entity-dependent collision shapes
// ---------------------------------------------------------------------------------------------
//
// The core tick owns the collision sweep; the tests below carry a small one of their own (the
// game's per-axis clamp, Y first) so that the shapes this module hands to the sweep can be checked
// against the recorded movement without the rest of the tick.

const COLLIDE_EPSILON: f64 = 1.0E-7;

#[derive(Clone, Copy, PartialEq)]
enum Axis {
    X,
    Y,
    Z,
}

fn lo(b: Aabb, a: Axis) -> f64 {
    match a {
        Axis::X => b.min.x,
        Axis::Y => b.min.y,
        Axis::Z => b.min.z,
    }
}

fn hi(b: Aabb, a: Axis) -> f64 {
    match a {
        Axis::X => b.max.x,
        Axis::Y => b.max.y,
        Axis::Z => b.max.z,
    }
}

fn component(v: Vec3, a: Axis) -> f64 {
    match a {
        Axis::X => v.x,
        Axis::Y => v.y,
        Axis::Z => v.z,
    }
}

/// `Shapes.collide` along one axis: clamp `d` against every collider the box overlaps on the other
/// two axes (with the game's `1.0E-7` insets).
fn collide_axis(axis: Axis, bb: Aabb, colliders: &[Aabb], mut d: f64) -> f64 {
    let others = match axis {
        Axis::X => [Axis::Y, Axis::Z],
        Axis::Y => [Axis::X, Axis::Z],
        Axis::Z => [Axis::X, Axis::Y],
    };
    for c in colliders {
        if d.abs() < COLLIDE_EPSILON {
            return 0.0;
        }
        let overlaps = others.iter().all(|&o| {
            hi(bb, o) - COLLIDE_EPSILON > lo(*c, o) && lo(bb, o) + COLLIDE_EPSILON < hi(*c, o)
        });
        if !overlaps {
            continue;
        }
        if d > 0.0 {
            let gap = lo(*c, axis) - hi(bb, axis);
            if gap >= -COLLIDE_EPSILON {
                d = d.min(gap);
            }
        } else if d < 0.0 {
            let gap = hi(*c, axis) - lo(bb, axis);
            if gap <= COLLIDE_EPSILON {
                d = d.max(gap);
            }
        }
    }
    d
}

/// `Entity.collideWithShapes`: the three axes in the game's order (Y, then the larger horizontal
/// motion first), each against the box advanced by the axes already resolved. No step-up.
fn collide3(motion: Vec3, bb: Aabb, colliders: &[Aabb]) -> Vec3 {
    let order = if motion.x.abs() < motion.z.abs() {
        [Axis::Y, Axis::Z, Axis::X]
    } else {
        [Axis::Y, Axis::X, Axis::Z]
    };
    let mut moved = [0.0_f64; 3];
    for axis in order {
        let d = component(motion, axis);
        if d != 0.0 {
            let shifted = bb.move_by(Vec3::new(moved[0], moved[1], moved[2]));
            let clamped = collide_axis(axis, shifted, colliders, d);
            match axis {
                Axis::X => moved[0] = clamped,
                Axis::Y => moved[1] = clamped,
                Axis::Z => moved[2] = clamped,
            }
        }
    }
    Vec3::new(moved[0], moved[1], moved[2])
}

fn player_box(p: &PlayerState) -> Aabb {
    let (w, h) = p.dimensions();
    let half = f64::from(w / 2.0_f32);
    Aabb::new(
        Vec3::new(p.pos.x - half, p.pos.y, p.pos.z - half),
        Vec3::new(p.pos.x + half, p.pos.y + f64::from(h), p.pos.z + half),
    )
}

type Shape<'a> = &'a dyn Fn(i32, i32, i32) -> Vec<[f64; 6]>;

/// The colliders the game's block-collision query returns for the player's box swept by `motion`
/// (only shapes that strictly intersect the swept region count), with `shape(x, y, z)` giving
/// each block's boxes in block-local coordinates.
fn colliders_around(p: &PlayerState, motion: Vec3, shape: Shape) -> Vec<Aabb> {
    let region = player_box(p).expand_towards(motion);
    let floor = |d: f64| d.floor() as i32;
    let mut out = Vec::new();
    for x in floor(region.min.x)..=floor(region.max.x) {
        for y in floor(region.min.y) - 1..=floor(region.max.y) {
            for z in floor(region.min.z)..=floor(region.max.z) {
                for b in shape(x, y, z) {
                    let collider = Aabb::new(
                        Vec3::new(
                            f64::from(x) + b[0],
                            f64::from(y) + b[1],
                            f64::from(z) + b[2],
                        ),
                        Vec3::new(
                            f64::from(x) + b[3],
                            f64::from(y) + b[4],
                            f64::from(z) + b[5],
                        ),
                    );
                    if region.intersects(collider) {
                        out.push(collider);
                    }
                }
            }
        }
    }
    out
}

/// Vertical collision of every plain tick: the motion into the move is the recorded start-of-tick
/// vertical velocity (after the `aiStep` rounding), the colliders are the blocks' shapes for this
/// player ([`blocks::collision_boxes`]), and the move's vertical result and vertical-collision
/// flags must be the recorded ones (two-sided: a wrongly missing and a wrongly present collider
/// both show). Ticks where something else shapes the motion — jumps, climbing, fluids, effects, a
/// stuck multiplier, a horizontal collision or a step-up — are skipped. `tally.interesting` counts
/// the ticks where the context-free table would have given another result.
fn check_vertical_collision(name: &str, tally: &mut Tally) {
    let scenario = Scenario::load(name).unwrap();
    let world = scenario.world();
    let mut p = scenario.initial_state();
    for (i, row) in scenario.rows.iter().enumerate() {
        if i > 0 {
            apply_state(&mut p, &row.pre);
        }
        // A rise without upward velocity can only be an auto step-up onto a ledge, which the
        // sweep below does not model.
        let stepped_up = double(&row.post, "y") > p.pos.y && p.vel.y <= 0.0;
        let other_terms = [
            p.on_ground && row.input.jump,
            p.in_water,
            p.in_lava,
            flag(&row.post, "water"),
            flag(&row.post, "lava"),
            blocks::on_climbable(&p, &world),
            !p.effects.is_empty(),
            p.flying,
            p.stuck_speed_multiplier != Vec3::ZERO,
            flag(&row.post, "hc"),
            stepped_up,
            flag(&row.post, "shift") != row.input.shift,
        ];
        let plain = !other_terms.iter().any(|&term| term);
        if !plain {
            tally.skipped += 1;
        } else {
            let mut vy = p.vel.y;
            if vy.abs() < 0.003 {
                vy = 0.0;
            }
            // The shape query sees the player as it is while the tick runs: sneaking as per the
            // input, fall distance as recorded at the start of the tick.
            let mut q = p.clone();
            q.shift_key_down = row.input.shift;
            let bb = player_box(&q);
            let motion = Vec3::new(0.0, vy, 0.0);
            let sweep = |shape: Shape| collide3(motion, bb, &colliders_around(&q, motion, shape)).y;
            let moved = sweep(&|x, y, z| blocks::collision_boxes(&q, bb, &world, x, y, z));
            let moved_static =
                sweep(&|x, y, z| ms_data::collision_boxes(world.block_state(x, y, z)).to_vec());
            let end_y = p.pos.y + moved;
            let vertical = vy != moved;
            let below = vertical && vy < 0.0;
            tally.checked += 1;
            tally.interesting += usize::from(moved.to_bits() != moved_static.to_bits());
            if end_y.to_bits() != double(&row.post, "y").to_bits() {
                tally.fail(
                    name,
                    row.t,
                    format!(
                        "y: recorded {:e}, simulated {:e} (start {:e}, vy {:e})",
                        double(&row.post, "y"),
                        end_y,
                        p.pos.y,
                        vy
                    ),
                );
            }
            if vertical != flag(&row.post, "vc") || below != flag(&row.post, "vcb") {
                tally.fail(
                    name,
                    row.t,
                    format!(
                        "collision flags: recorded vc={} vcb={}, simulated vc={vertical} vcb={below}",
                        flag(&row.post, "vc"),
                        flag(&row.post, "vcb")
                    ),
                );
            }
        }
        apply_state(&mut p, &row.post);
    }
}

/// Every recorded displacement must be one the shapes allow: sweeping the box by exactly the
/// distance the game moved the player (start-of-tick position to end-of-tick position) may not be
/// clamped by any collider. One-sided — a collider that is wrongly missing goes unnoticed — but it
/// covers every tick, including the horizontal ones, and catches shapes that block what the game
/// let through (a scaffold the player walks into, powder snow it sinks in). `tally.interesting`
/// counts the ticks where the context-free table would have blocked the move.
fn check_displacement_is_free(name: &str, tally: &mut Tally) {
    let scenario = Scenario::load(name).unwrap();
    let world = scenario.world();
    let mut p = scenario.initial_state();
    for (i, row) in scenario.rows.iter().enumerate() {
        if i > 0 {
            apply_state(&mut p, &row.pre);
        }
        let to = Vec3::new(
            double(&row.post, "x"),
            double(&row.post, "y"),
            double(&row.post, "z"),
        );
        let motion = Vec3::new(to.x - p.pos.x, to.y - p.pos.y, to.z - p.pos.z);
        let mut q = p.clone();
        q.shift_key_down = row.input.shift;
        let bb = player_box(&q);
        let sweep = |shape: Shape| collide3(motion, bb, &colliders_around(&q, motion, shape));
        let moved = sweep(&|x, y, z| blocks::collision_boxes(&q, bb, &world, x, y, z));
        let moved_static =
            sweep(&|x, y, z| ms_data::collision_boxes(world.block_state(x, y, z)).to_vec());
        let blocked = |m: Vec3| {
            (m.x - motion.x).abs() > 1.0E-9
                || (m.y - motion.y).abs() > 1.0E-9
                || (m.z - motion.z).abs() > 1.0E-9
        };
        tally.checked += 1;
        tally.interesting += usize::from(blocked(moved_static));
        if blocked(moved) {
            tally.fail(
                name,
                row.t,
                format!("moved {motion:?} but the shapes only allow {moved:?}"),
            );
        }
        apply_state(&mut p, &row.post);
    }
}

#[test]
fn scaffolding_collides_like_the_recording() {
    let mut tally = Tally::default();
    check_vertical_collision("vines_scaffolding", &mut tally);
    eprintln!(
        "vines_scaffolding vertical collision: {} checked, {} where the entity-dependent shape decided, {} skipped",
        tally.checked, tally.interesting, tally.skipped
    );
    assert!(tally.failures.is_empty(), "{:#?}", tally.failures);
    assert!(tally.checked > 50, "only {} plain ticks", tally.checked);

    let mut tally = Tally::default();
    check_displacement_is_free("vines_scaffolding", &mut tally);
    eprintln!(
        "vines_scaffolding displacements: {} checked, {} that the context-free table would block",
        tally.checked, tally.interesting
    );
    assert!(tally.failures.is_empty(), "{:#?}", tally.failures);
    // Walking into the scaffold tower is only possible with the entity-dependent shape.
    assert!(
        tally.interesting >= 3,
        "only {} deciding ticks",
        tally.interesting
    );
}

#[test]
fn powder_snow_does_not_block_the_walker() {
    let mut tally = Tally::default();
    check_displacement_is_free("powder_snow", &mut tally);
    eprintln!("powder_snow displacements: {} checked", tally.checked);
    assert!(tally.failures.is_empty(), "{:#?}", tally.failures);
}

#[test]
fn vertical_collision_and_displacements_match_the_whole_corpus() {
    let mut vertical = Tally::default();
    let mut free = Tally::default();
    for name in client_scenarios() {
        check_vertical_collision(&name, &mut vertical);
        check_displacement_is_free(&name, &mut free);
    }
    eprintln!(
        "vertical collision, whole corpus: {} checked, {} skipped; displacements: {} checked",
        vertical.checked, vertical.skipped, free.checked
    );
    assert!(vertical.failures.is_empty(), "{:#?}", vertical.failures);
    assert!(free.failures.is_empty(), "{:#?}", free.failures);
}
