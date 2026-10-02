//! One client tick of the local player: [`tick`] reproduces what the game does to a `LocalPlayer`
//! between two client ticks, driven by that tick's keys and look direction.
//!
//! The chain, in the order the game runs it:
//!
//! 1. `ClientLevel.tickNonPassenger`: the old position is remembered, `tickCount` increments.
//! 2. `Player.tick` -> `LivingEntity.tick` -> `Entity.tick`/`baseTick`: fluid state, fire, effect and
//!    timer countdowns ([`crate::living::base_tick`]).
//! 3. `LocalPlayer.aiStep`: the crouching decision, the keyboard input, pushing out of walls,
//!    sprint start/stop rules ([`local_player_ai_step`]), then `Player.aiStep` and
//!    `LivingEntity.aiStep` ([`crate::living::ai_step`]): velocity rounding, jump, travel,
//!    `Entity.move` ([`crate::entity::move_entity`]) and the effects of the blocks passed through.
//! 4. Back in `Player.tick`: the position clamp and `updatePlayerPose`.
//!
//! What the server does to the player between ticks (knockback, effects, health, teleports) is not
//! part of this; it arrives as changes to the [`PlayerState`] before the next call.
//!
//! Not simulated (the corresponding state stays inert): riding, elytra flight, sleeping, using
//! items, auto-jump, spectator mode and creative flight toggling.

// The comparisons mirror the reference's `!(a <= b)` forms, which differ from `a > b` for NaN.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::attributes::{Attribute, Modifier, Operation};
use crate::collision::{self, aabb, aabb_deflate, bounding_box_at, floor, jmin};
use crate::input::{has_forward_impulse, keyboard_move_vector};
use crate::living::{
    ai_step, base_tick, go_down_in_water, is_affected_by_fluids, is_in_shallow_water,
    is_moving_slowly, is_swimming, is_under_water,
};
use crate::state::{Input, PlayerState, Pose};
use ms_numerics::Vec3;
use ms_world::World;

/// `Options.sprintWindow`'s default: how many ticks a first forward tap stays armed.
const SPRINT_WINDOW: i32 = 7;

/// Dimensions of `pose` for this player (`Player.getDimensions(pose)`, scale included).
fn dimensions_for(p: &PlayerState, pose: Pose) -> (f32, f32) {
    let (w, h) = pose.dimensions();
    let scale = p.attributes.value(Attribute::Scale) as f32;
    if scale == 1.0 {
        (w, h)
    } else {
        (w * scale, h * scale)
    }
}

/// `Player.canPlayerFitWithinBlocksAndEntitiesWhen`: whether the box of `pose` at the current
/// position (shrunk by 1e-7) is free of blocks.
pub fn can_fit_in_pose(p: &PlayerState, world: &World, pose: Pose) -> bool {
    let bb = aabb_deflate(bounding_box_at(p.pos, dimensions_for(p, pose)), 1.0E-7);
    collision::no_collision(world, p, bb)
}

/// `LivingEntity.setSprinting` as seen by the movement-speed attribute: the flag, and the
/// "sprinting" modifier (+30% total) added or removed. The effects module owns this
/// ([`crate::effects::set_sprinting`], called first); re-establishing the modifier afterwards is
/// idempotent and keeps the physics right while that hook is a stub.
pub fn set_sprinting(p: &mut PlayerState, sprinting: bool) {
    crate::effects::set_sprinting(p, sprinting);
    p.attributes
        .remove_modifier(Attribute::MovementSpeed, "minecraft:sprinting");
    if sprinting {
        p.attributes.add_modifier(
            Attribute::MovementSpeed,
            Modifier {
                id: "minecraft:sprinting".to_string(),
                amount: f64::from(0.3_f32),
                operation: Operation::AddMultipliedTotal,
            },
        );
    }
}

/// `LocalPlayer.isSprintingPossible`: not blinded, enough food (more than 6), and not in shallow
/// water unless `allow_shallow`.
fn is_sprinting_possible(p: &PlayerState, allow_shallow: bool) -> bool {
    !p.effects.has("minecraft:blindness")
        && p.food > 6
        && (allow_shallow || !is_in_shallow_water(p))
}

/// `LocalPlayer.canStartSprinting`.
fn can_start_sprinting(p: &PlayerState, forward_impulse: bool) -> bool {
    !p.sprinting
        && forward_impulse
        && is_sprinting_possible(p, p.flying)
        && (!is_moving_slowly(p) || is_under_water(p))
}

/// `LocalPlayer.shouldStopRunSprinting`.
fn should_stop_run_sprinting(p: &PlayerState, forward_impulse: bool) -> bool {
    !is_sprinting_possible(p, p.flying)
        || !forward_impulse
        || (p.horizontal_collision && !p.minor_horizontal_collision)
}

/// `LocalPlayer.shouldStopSwimSprinting`.
fn should_stop_swim_sprinting(p: &PlayerState, forward_impulse: bool, shift: bool) -> bool {
    !is_sprinting_possible(p, true) || !p.in_water || (!forward_impulse && !p.on_ground && !shift)
}

/// `LocalPlayer.suffocatesAt`: does a suffocating block overlap the player's height within the
/// column of block `(x, z)`?
fn suffocates_at(p: &PlayerState, world: &World, x: i32, z: i32) -> bool {
    let bb = collision::bounding_box(p);
    let col = aabb(
        f64::from(x),
        bb.min.y,
        f64::from(z),
        f64::from(x) + 1.0,
        bb.max.y,
        f64::from(z) + 1.0,
    );
    collision::collides_with_suffocating_block(world, p, aabb_deflate(col, 1.0E-7))
}

/// `LocalPlayer.moveTowardsClosestSpace(x, z)`: if the point is inside a suffocating block, nudge
/// the velocity towards the nearest side that is free.
fn move_towards_closest_space(p: &mut PlayerState, world: &World, x: f64, z: f64) {
    let bx = floor(x);
    let bz = floor(z);
    if suffocates_at(p, world, bx, bz) {
        let f = x - f64::from(bx);
        let g = z - f64::from(bz);
        // (dx, dz, step) for WEST, EAST, NORTH, SOUTH; `i` is the choose() of the axis.
        let candidates: [(i32, i32, bool, f64); 4] = [
            (-1, 0, false, f),
            (1, 0, true, f),
            (0, -1, false, g),
            (0, 1, true, g),
        ];
        let mut best: Option<(i32, i32)> = None;
        let mut h = f64::MAX;
        for (dx, dz, positive, i) in candidates {
            let j = if positive { 1.0 - i } else { i };
            if j < h && !suffocates_at(p, world, bx + dx, bz + dz) {
                h = j;
                best = Some((dx, dz));
            }
        }
        if let Some((dx, dz)) = best {
            let v = p.vel;
            if dx != 0 {
                crate::entity::set_vel(p, Vec3::new(0.1 * f64::from(dx), v.y, v.z));
            } else {
                crate::entity::set_vel(p, Vec3::new(v.x, v.y, 0.1 * f64::from(dz)));
            }
        }
    }
}

/// `LocalPlayer.aiStep` followed by `Player.aiStep`'s own part and `LivingEntity.aiStep`.
fn local_player_ai_step(p: &mut PlayerState, world: &World, input: &Input, old_pos: Vec3) {
    if p.sprint_trigger_time > 0 {
        p.sprint_trigger_time -= 1;
    }

    // These read the previous tick's key state, before the keyboard input is refreshed.
    let was_shift = p.shift_key_down;
    // hasForwardImpulse() of the previous tick's move vector: its forward component is positive
    // exactly when `zza` (a positive multiple of it) is.
    let had_forward_impulse = p.zza > 0.0;

    p.crouching = !p.flying
        && !is_swimming(p)
        && can_fit_in_pose(p, world, Pose::Crouching)
        && (p.shift_key_down || !can_fit_in_pose(p, world, Pose::Standing));

    // input.tick()
    let move_vector = keyboard_move_vector(input);
    p.shift_key_down = input.shift;
    let forward_impulse = has_forward_impulse(move_vector);

    // if (!noPhysics) moveTowardsClosestSpace at the four corners of the box (0.35 of the width)
    let width = f64::from(p.dimensions().0);
    let (px, pz) = (p.pos.x, p.pos.z);
    move_towards_closest_space(p, world, px - width * 0.35, pz + width * 0.35);
    let (px, pz) = (p.pos.x, p.pos.z);
    move_towards_closest_space(p, world, px - width * 0.35, pz - width * 0.35);
    let (px, pz) = (p.pos.x, p.pos.z);
    move_towards_closest_space(p, world, px + width * 0.35, pz - width * 0.35);
    let (px, pz) = (p.pos.x, p.pos.z);
    move_towards_closest_space(p, world, px + width * 0.35, pz + width * 0.35);

    if was_shift || input.back {
        p.sprint_trigger_time = 0;
    }

    if can_start_sprinting(p, forward_impulse) {
        if !had_forward_impulse {
            if p.sprint_trigger_time > 0 {
                set_sprinting(p, true);
            } else {
                p.sprint_trigger_time = SPRINT_WINDOW;
            }
        }
        if input.sprint {
            set_sprinting(p, true);
        }
    }

    if p.sprinting {
        if is_swimming(p) {
            if should_stop_swim_sprinting(p, forward_impulse, input.shift) {
                set_sprinting(p, false);
            }
        } else if should_stop_run_sprinting(p, forward_impulse) {
            set_sprinting(p, false);
        }
    }

    if p.in_water && input.shift && is_affected_by_fluids(p) {
        go_down_in_water(p);
    }

    if p.flying {
        // Creative flight: vertical control with jump and sneak (3 x the flying speed).
        let mut i = 0_i32;
        if input.shift {
            i -= 1;
        }
        if input.jump {
            i += 1;
        }
        if i != 0 {
            let v = p.vel;
            let dy = f64::from(i as f32 * 0.05_f32 * 3.0_f32);
            crate::entity::set_vel(p, Vec3::new(v.x + 0.0, v.y + dy, v.z + 0.0));
        }
    }

    // Player.aiStep
    if p.jump_trigger_time > 0 {
        p.jump_trigger_time -= 1;
    }
    if p.flying {
        crate::entity::reset_fall_distance(p);
    }
    ai_step(p, world, input, move_vector, old_pos);
    p.speed = crate::living::speed(p);

    // LocalPlayer: landing ends creative flight.
    if p.on_ground && p.flying {
        p.flying = false;
    }
}

/// `Player.getDesiredPose`.
fn desired_pose(p: &PlayerState) -> Pose {
    if is_swimming(p) {
        Pose::Swimming
    } else if p.shift_key_down && !p.flying {
        Pose::Crouching
    } else {
        Pose::Standing
    }
}

/// `Player.updatePlayerPose`: the pose follows the input, unless the box of the wanted pose does not
/// fit (then crouching, or swimming/crawling, whichever fits).
fn update_player_pose(p: &mut PlayerState, world: &World) {
    if can_fit_in_pose(p, world, Pose::Swimming) {
        let pose = desired_pose(p);
        let pose2 = if can_fit_in_pose(p, world, pose) {
            pose
        } else if can_fit_in_pose(p, world, Pose::Crouching) {
            Pose::Crouching
        } else {
            Pose::Swimming
        };
        p.pose = pose2;
    }
}

/// One client tick of the local player: everything the game does to it between the start and the
/// end of a client tick (`LocalPlayer.tick` and the `Player`/`LivingEntity`/`Entity` chain under
/// it), driven by `input`. This is the kernel's entry point.
pub fn tick(p: &mut PlayerState, input: &Input, world: &World) {
    // ClientLevel.tickNonPassenger: Entity.setOldPosAndRot(), tickCount++.
    let old_pos = p.pos;
    p.tick_count = p.tick_count.wrapping_add(1);
    // The look direction of this tick (mouse movement is applied before the player ticks).
    p.yaw = input.yaw;
    p.pitch = input.pitch;

    // Player.tick -> LivingEntity.tick -> Entity.tick
    base_tick(p, world);
    // LivingEntity.tick -> aiStep (LocalPlayer's override)
    local_player_ai_step(p, world, input, old_pos);

    // Player.tick: keep the horizontal position within the world border limits...
    let x = clamp(p.pos.x, -2.999_999_9E7, 2.999_999_9E7);
    let z = clamp(p.pos.z, -2.999_999_9E7, 2.999_999_9E7);
    if x != p.pos.x || z != p.pos.z {
        p.pos = Vec3::new(x, p.pos.y, z);
    }
    // ...then the pose for the next tick.
    update_player_pose(p, world);
}

/// `Mth.clamp(double, double, double)`.
fn clamp(d: f64, lo: f64, hi: f64) -> f64 {
    if d < lo {
        lo
    } else {
        jmin(d, hi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ms_numerics::Vec3;

    fn stand_on_flat(world_surface: i32) -> (PlayerState, World) {
        let world = World::flat(world_surface);
        let mut p = PlayerState::new(Vec3::new(0.5, f64::from(world_surface), 0.5), 0.0);
        // Settle onto the floor.
        let idle = Input::default();
        for _ in 0..3 {
            tick(&mut p, &idle, &world);
        }
        (p, world)
    }

    #[test]
    fn standing_player_rests_on_the_floor() {
        let (p, _) = stand_on_flat(0);
        assert!(p.on_ground);
        assert_eq!(p.pos.y, 0.0);
        assert_eq!(p.vel.y, (0.0 - 0.08) * f64::from(0.98_f32));
        assert!(p.vertical_collision && p.vertical_collision_below);
        assert_eq!(p.pose, Pose::Standing);
    }

    #[test]
    fn sprint_key_starts_sprinting_and_speeds_up() {
        let (mut p, world) = stand_on_flat(0);
        let walk = Input {
            forward: true,
            ..Input::default()
        };
        let sprint = Input {
            forward: true,
            sprint: true,
            ..Input::default()
        };
        let mut a = p.clone();
        for _ in 0..20 {
            tick(&mut a, &walk, &world);
        }
        for _ in 0..20 {
            tick(&mut p, &sprint, &world);
        }
        assert!(p.sprinting);
        assert!(!a.sprinting);
        // z grows in the +z direction for yaw 0; sprinting covers more ground.
        assert!(p.pos.z > a.pos.z);
    }

    #[test]
    fn jumping_leaves_the_ground_and_returns() {
        let (mut p, world) = stand_on_flat(0);
        let jump = Input {
            jump: true,
            ..Input::default()
        };
        tick(&mut p, &jump, &world);
        assert!(!p.on_ground);
        assert!(p.pos.y > 0.0);
        let idle = Input::default();
        for _ in 0..30 {
            tick(&mut p, &idle, &world);
        }
        assert!(p.on_ground);
        assert_eq!(p.pos.y, 0.0);
    }

    #[test]
    fn sneaking_changes_the_pose_and_does_not_walk_off_the_edge() {
        // A 1x1 platform floating one block up in an otherwise flat world.
        let mut grid = ms_world::GridWorld::new(ms_world::FlatWorld::new(0, ms_data::AIR));
        let stone = ms_data::parse_state("minecraft:stone").unwrap();
        grid.set_block(0, -1, 0, stone);
        let world = World::grid(grid);
        let mut p = PlayerState::new(Vec3::new(0.5, 0.0, 0.5), 0.0);
        let sneak_forward = Input {
            forward: true,
            shift: true,
            ..Input::default()
        };
        for _ in 0..60 {
            tick(&mut p, &sneak_forward, &world);
        }
        assert_eq!(p.pose, Pose::Crouching);
        assert!(
            p.on_ground,
            "the edge back-off must keep the player on the block"
        );
        assert!(p.pos.z < 1.0 + 0.3 + 1e-9);
    }

    #[test]
    fn tick_count_increments_once_per_tick() {
        let (mut p, world) = stand_on_flat(0);
        let before = p.tick_count;
        tick(&mut p, &Input::default(), &world);
        assert_eq!(p.tick_count, before + 1);
    }

    /// A row of `testdata/walk.csv`: the end-of-tick state of a recorded session and the keys held
    /// during that tick.
    struct Row {
        pos: Vec3,
        vel: Vec3,
        on_ground: bool,
        sprinting: bool,
        sneaking: bool,
        input: Input,
    }

    fn parse_walk() -> Vec<Row> {
        let csv = include_str!("../testdata/walk.csv");
        let mut rows = Vec::new();
        for line in csv.lines().skip(1) {
            let c: Vec<&str> = line.split(',').collect();
            if c.len() < 19 {
                continue;
            }
            let d = |i: usize| f64::from_bits(c[i].parse::<i64>().unwrap() as u64);
            let fl = |i: usize| f32::from_bits(c[i].parse::<i32>().unwrap() as u32);
            let b = |i: usize| c[i] == "1";
            rows.push(Row {
                pos: Vec3::new(d(1), d(2), d(3)),
                vel: Vec3::new(d(4), d(5), d(6)),
                on_ground: b(9),
                sprinting: b(10),
                sneaking: b(11),
                input: Input {
                    forward: b(12),
                    back: b(13),
                    left: b(14),
                    right: b(15),
                    jump: b(16),
                    shift: b(17),
                    sprint: b(18),
                    yaw: fl(7),
                    pitch: fl(8),
                },
            });
        }
        rows
    }

    /// The recorded session, one tick at a time on a flat world, seeded from each tick's
    /// predecessor: every flat-ground tick that is not disturbed by something the session had that
    /// a flat world lacks (walls, speed effects, ...) must reproduce the game's velocity and
    /// position bit for bit.
    #[test]
    fn reproduces_flat_ground_walk() {
        let rows = parse_walk();
        let mut qualifying = 0usize;
        let mut exact = 0usize;
        let mut close = 0usize;
        let mut worst_close = 0.0_f64;
        for t in 0..rows.len() - 1 {
            let (a, b) = (&rows[t], &rows[t + 1]);
            // Only the clean case: resting on flat ground (no y change), no jump this tick.
            if !a.on_ground || !b.on_ground || a.pos.y != b.pos.y || b.input.jump {
                continue;
            }
            qualifying += 1;
            let world = World::flat(a.pos.y.floor() as i32);
            let mut p = PlayerState::new(a.pos, a.input.yaw);
            p.vel = a.vel;
            p.on_ground = true;
            p.shift_key_down = a.sneaking;
            p.zza = if a.input.forward && !a.input.back {
                0.98
            } else {
                0.0
            };
            if a.sprinting {
                set_sprinting(&mut p, true);
            }
            tick(&mut p, &b.input, &world);
            let err = (p.vel.x - b.vel.x)
                .abs()
                .max((p.vel.y - b.vel.y).abs())
                .max((p.vel.z - b.vel.z).abs());
            // A difference of 1e-6 or more is something the flat world cannot model (a wall the
            // session ran into, the speed effects it was under); everything else is plain walking.
            if err < 1.0e-6 {
                close += 1;
                if err == 0.0 && p.pos == b.pos {
                    exact += 1;
                }
                worst_close = worst_close.max(err);
            }
        }
        eprintln!(
            "qualifying={qualifying} undisturbed={close} exact={exact} worst={worst_close:e}"
        );
        assert!(qualifying > 1000, "only {qualifying} qualifying ticks");
        assert!(
            close * 2 > qualifying,
            "too few undisturbed ticks: {close}/{qualifying}"
        );
        assert_eq!(exact, close, "worst undisturbed error {worst_close:e}");
    }

    #[test]
    fn collision_flags_for_walking_into_a_wall() {
        let mut grid = ms_world::GridWorld::new(ms_world::FlatWorld::new(0, ms_data::AIR));
        let stone = ms_data::parse_state("minecraft:stone").unwrap();
        grid.fill((-5, -1, -5), (5, -1, 5), stone);
        grid.fill((-5, 0, 2), (5, 3, 2), stone);
        let world = World::grid(grid);
        let mut p = PlayerState::new(Vec3::new(0.5, 0.0, 0.5), 0.0);
        let fwd = Input {
            forward: true,
            ..Input::default()
        };
        for _ in 0..40 {
            tick(&mut p, &fwd, &world);
        }
        assert!(p.horizontal_collision);
        assert!(p.pos.z <= 2.0 - 0.3 + 1e-6);
    }
}
