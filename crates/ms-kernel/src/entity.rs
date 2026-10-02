//! The `Entity` layer of the tick: moving through the world (`Entity.move`), the ground and
//! supporting-block bookkeeping, fall distance, and the block-underfoot lookups that decide
//! friction, speed and jump factors.
//!
//! Method names of the reference are quoted in the comments so each piece can be checked against
//! it. Everything that belongs to another subsystem (block behaviours, fluids, damage) is reached
//! through the hooks in [`crate::blocks`], [`crate::fluids`] and [`crate::damage`].

// The comparisons mirror the reference's `!(a <= b)` forms, which differ from `a > b` for NaN.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::attributes::Attribute;
use crate::collision::{self, aabb, aabb_move, bounding_box, floor, jmin};
use crate::state::PlayerState;
use ms_numerics::Vec3;
use ms_world::aabb::Aabb;
use ms_world::World;

/// `Mth.equal(double, double)`.
#[inline]
pub fn mth_equal(a: f64, b: f64) -> bool {
    (b - a).abs() < f64::from(1.0E-5_f32)
}

#[inline]
pub fn length_sqr(v: Vec3) -> f64 {
    v.x * v.x + v.y * v.y + v.z * v.z
}

/// `Entity.setDeltaMovement(Vec3)`: a non-finite vector is ignored.
#[inline]
pub fn set_vel(p: &mut PlayerState, v: Vec3) {
    if v.x.is_finite() && v.y.is_finite() && v.z.is_finite() {
        p.vel = v;
    }
}

/// `Entity.resetFallDistance`.
#[inline]
pub fn reset_fall_distance(p: &mut PlayerState) {
    p.fall_distance = 0.0;
}

/// `Entity.blockPosition()`: the block containing the position.
#[inline]
pub fn block_position(p: &PlayerState) -> (i32, i32, i32) {
    (floor(p.pos.x), floor(p.pos.y), floor(p.pos.z))
}

/// `LivingEntity.maxUpStep`: the step-height attribute as a float.
#[inline]
pub fn max_up_step(p: &PlayerState) -> f32 {
    p.attributes.value(Attribute::StepHeight) as f32
}

fn block_at(world: &World, pos: (i32, i32, i32)) -> usize {
    world.block(pos.0, pos.1, pos.2)
}

/// `Entity.getOnPos(float)`: the block the entity stands on, taken from the supporting block when
/// there is one (fences, walls and fence gates count as their own top), otherwise from the
/// position `f` below the feet.
pub fn on_pos(p: &PlayerState, world: &World, f: f32) -> (i32, i32, i32) {
    match p.supporting_block {
        Some(support) => {
            if !(f > 1.0E-5_f32) {
                return support;
            }
            let block = block_at(world, support);
            let is_fence = ms_data::block_has_tag(block, "minecraft:fences");
            let is_wall = ms_data::block_has_tag(block, "minecraft:walls");
            let is_gate = ms_data::block_class(block) == "FenceGateBlock";
            if (!(f <= 0.5) || !is_fence) && !is_wall && !is_gate {
                (support.0, floor(p.pos.y - f64::from(f)), support.2)
            } else {
                support
            }
        }
        None => (
            floor(p.pos.x),
            floor(p.pos.y - f64::from(f)),
            floor(p.pos.z),
        ),
    }
}

/// `Entity.getOnPosLegacy`.
#[inline]
pub fn on_pos_legacy(p: &PlayerState, world: &World) -> (i32, i32, i32) {
    on_pos(p, world, 0.2_f32)
}

/// `Entity.getBlockPosBelowThatAffectsMyMovement`.
#[inline]
pub fn block_pos_below_that_affects_my_movement(p: &PlayerState, world: &World) -> (i32, i32, i32) {
    on_pos(p, world, 0.500_001_f32)
}

/// `Block.getFriction` of the block at `pos`.
#[inline]
pub fn block_friction_at(world: &World, pos: (i32, i32, i32)) -> f32 {
    ms_data::block_friction(block_at(world, pos))
}

/// `Entity.getBlockJumpFactor`.
pub fn block_jump_factor(p: &PlayerState, world: &World) -> f32 {
    let f = ms_data::block_jump_factor(block_at(world, block_position(p)));
    let g = ms_data::block_jump_factor(block_at(
        world,
        block_pos_below_that_affects_my_movement(p, world),
    ));
    if f == 1.0 {
        g
    } else {
        f
    }
}

/// `LivingEntity.getBlockSpeedFactor` over `Entity.getBlockSpeedFactor`.
pub fn block_speed_factor(p: &PlayerState, world: &World) -> f32 {
    let block = block_at(world, block_position(p));
    let f = ms_data::block_speed_factor(block);
    let name = ms_data::block_name(block);
    let base = if name != "minecraft:water" && name != "minecraft:bubble_column" {
        if f == 1.0 {
            ms_data::block_speed_factor(block_at(
                world,
                block_pos_below_that_affects_my_movement(p, world),
            ))
        } else {
            f
        }
    } else {
        f
    };
    // Mth.lerp((float) movement_efficiency, base, 1.0F)
    let efficiency = p.attributes.value(Attribute::MovementEfficiency) as f32;
    base + efficiency * (1.0_f32 - base)
}

/// `Entity.checkSupportingBlock`: refresh the supporting block and the "on ground without any
/// block under" flag.
pub fn check_supporting_block(
    p: &mut PlayerState,
    world: &World,
    on_ground: bool,
    movement: Option<Vec3>,
) {
    if on_ground {
        let bb = bounding_box(p);
        let probe = aabb(
            bb.min.x,
            bb.min.y - 1.0E-6,
            bb.min.z,
            bb.max.x,
            bb.min.y,
            bb.max.z,
        );
        let mut found = collision::find_supporting_block(world, p, probe);
        if found.is_some() || p.on_ground_no_blocks {
            p.supporting_block = found;
        } else if let Some(m) = movement {
            let back = aabb_move(probe, -m.x, 0.0, -m.z);
            found = collision::find_supporting_block(world, p, back);
            p.supporting_block = found;
        }
        p.on_ground_no_blocks = found.is_none();
    } else {
        p.on_ground_no_blocks = false;
        p.supporting_block = None;
    }
}

/// `Entity.setOnGroundWithMovement(onGround, horizontalCollision, movement)`.
pub fn set_on_ground_with_movement(
    p: &mut PlayerState,
    world: &World,
    on_ground: bool,
    horizontal_collision: bool,
    movement: Vec3,
) {
    p.on_ground = on_ground;
    p.horizontal_collision = horizontal_collision;
    check_supporting_block(p, world, on_ground, Some(movement));
}

// ---------------------------------------------------------------------------------------------
// Player.maybeBackOffFromEdge (sneaking at an edge)
// ---------------------------------------------------------------------------------------------

/// `Player.canFallAtLeast`: whether nothing solid is within `f` below the feet, with the footprint
/// shifted by `(d, e)`.
fn can_fall_at_least(p: &PlayerState, world: &World, d: f64, e: f64, f: f64) -> bool {
    let bb = bounding_box(p);
    let probe = aabb(
        bb.min.x + 1.0E-7 + d,
        bb.min.y - f - 1.0E-7,
        bb.min.z + 1.0E-7 + e,
        bb.max.x - 1.0E-7 + d,
        bb.min.y,
        bb.max.z - 1.0E-7 + e,
    );
    collision::no_collision(world, p, probe)
}

/// `Player.isAboveGround`.
fn is_above_ground(p: &PlayerState, world: &World, f: f32) -> bool {
    let f = f64::from(f);
    p.on_ground
        || (p.fall_distance < f && !can_fall_at_least(p, world, 0.0, 0.0, f - p.fall_distance))
}

/// `Math.signum(double)`.
fn signum(d: f64) -> f64 {
    if d > 0.0 {
        1.0
    } else if d < 0.0 {
        -1.0
    } else {
        d
    }
}

/// `Player.maybeBackOffFromEdge` for `MoverType.SELF`: while sneaking on (or just above) the
/// ground, shrink the horizontal motion in 0.05 steps until the player would not walk off.
pub fn maybe_back_off_from_edge(p: &PlayerState, world: &World, motion: Vec3) -> Vec3 {
    let f = max_up_step(p);
    if !p.flying && !(motion.y > 0.0) && p.shift_key_down && is_above_ground(p, world, f) {
        let mut d = motion.x;
        let mut e = motion.z;
        let h = signum(d) * 0.05;
        let i = signum(e) * 0.05;
        let f = f64::from(f);
        while d != 0.0 && can_fall_at_least(p, world, d, 0.0, f) {
            if d.abs() <= 0.05 {
                d = 0.0;
                break;
            }
            d -= h;
        }
        while e != 0.0 && can_fall_at_least(p, world, 0.0, e, f) {
            if e.abs() <= 0.05 {
                e = 0.0;
                break;
            }
            e -= i;
        }
        while d != 0.0 && e != 0.0 && can_fall_at_least(p, world, d, e, f) {
            if d.abs() <= 0.05 {
                d = 0.0;
            } else {
                d -= h;
            }
            if e.abs() <= 0.05 {
                e = 0.0;
            } else {
                e -= i;
            }
        }
        Vec3::new(d, motion.y, e)
    } else {
        motion
    }
}

// ---------------------------------------------------------------------------------------------
// Fall damage
// ---------------------------------------------------------------------------------------------

/// The water half of `Entity.updateInWaterStateAndDoFluidPushing`, which `LivingEntity` re-runs
/// inside `checkFallDamage` when the entity is not in water yet (so landing in water within a move
/// resets the fall distance). The fluid module's single entry point refreshes both fluids, whereas
/// the game refreshes only the water here, so the lava state the `baseTick` update produced is kept.
/// (Lava *pushing* from flowing lava would still be applied twice: the fluid module needs a
/// water-only entry for that to be exact.)
fn update_in_water_state_during_move(p: &mut PlayerState, world: &World) {
    let (in_lava, lava_height) = (p.in_lava, p.lava_height);
    crate::fluids::update_in_fluid_state_and_push(p, world);
    p.in_lava = in_lava;
    p.lava_height = lava_height;
}

/// `LivingEntity.checkFallDamage` + `Entity.checkFallDamage`: accumulate the fall distance, and on
/// landing hand it to the block (`fallOn`) and reset it. `d` is the vertical movement this move.
pub fn check_fall_damage(
    p: &mut PlayerState,
    world: &World,
    d: f64,
    on_ground: bool,
    on_pos: (i32, i32, i32),
) {
    if !p.in_water {
        update_in_water_state_during_move(p, world);
    }
    if !p.in_water && d < 0.0 {
        // fallDistance -= (float) d
        p.fall_distance -= f64::from(d as f32);
    }
    if on_ground {
        if p.fall_distance > 0.0 {
            // Block.fallOn -> Entity.causeFallDamage with the block's distance and multiplier.
            crate::blocks::fall_on(p, world, on_pos);
        }
        reset_fall_distance(p);
    }
}

// ---------------------------------------------------------------------------------------------
// Entity.move
// ---------------------------------------------------------------------------------------------

/// Whether the segment `from -> to` passes through a block that resets fall distance: a
/// `fall_damage_resetting` block (treated as a full cube, as the game's clip does) or water
/// (up to the fluid surface). This is the `ClipContext.Block.FALLDAMAGE_RESETTING` /
/// `Fluid.WATER` raycast of `Entity.move`, reduced to the question of whether it hits anything.
fn fall_reset_clip_hits(world: &World, from: Vec3, to: Vec3) -> bool {
    let (x0, x1) = (floor(from.x.min(to.x)), floor(from.x.max(to.x)));
    let (y0, y1) = (floor(from.y.min(to.y)), floor(from.y.max(to.y)));
    let (z0, z1) = (floor(from.z.min(to.z)), floor(from.z.max(to.z)));
    let dir = Vec3::new(to.x - from.x, to.y - from.y, to.z - from.z);
    for x in x0..=x1 {
        for y in y0..=y1 {
            for z in z0..=z1 {
                let state = world.block_state(x, y, z);
                if state == ms_data::AIR {
                    continue;
                }
                let block = ms_data::block_of_state(state);
                let mut top = None;
                if ms_data::block_has_tag(block, "minecraft:fall_damage_resetting") {
                    top = Some(1.0);
                }
                let fluid = ms_data::fluid(state);
                if fluid.kind == ms_data::FluidKind::Water {
                    let above = ms_data::fluid(world.block_state(x, y + 1, z));
                    let h = if above.kind == ms_data::FluidKind::Water {
                        1.0
                    } else {
                        f64::from(fluid.own_height())
                    };
                    top = Some(top.map_or(h, |t: f64| t.max(h)));
                }
                if let Some(h) = top {
                    let cube = aabb(
                        f64::from(x),
                        f64::from(y),
                        f64::from(z),
                        f64::from(x) + 1.0,
                        f64::from(y) + h,
                        f64::from(z) + 1.0,
                    );
                    if segment_hits_box(from, dir, cube) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Slab test: does `from + t * dir` for `t` in `[0, 1]` touch `b`?
fn segment_hits_box(from: Vec3, dir: Vec3, b: Aabb) -> bool {
    let mut t0 = 0.0_f64;
    let mut t1 = 1.0_f64;
    for (o, d, lo, hi) in [
        (from.x, dir.x, b.min.x, b.max.x),
        (from.y, dir.y, b.min.y, b.max.y),
        (from.z, dir.z, b.min.z, b.max.z),
    ] {
        if d == 0.0 {
            if o < lo || o > hi {
                return false;
            }
        } else {
            let (mut a, mut c) = ((lo - o) / d, (hi - o) / d);
            if a > c {
                std::mem::swap(&mut a, &mut c);
            }
            t0 = t0.max(a);
            t1 = t1.min(c);
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

/// `Entity.move(MoverType.SELF, motion)` for the (physics-enabled) local player: consume the stuck
/// multiplier, back off edges when sneaking, collide, advance the position, update the collision
/// flags, ground and supporting block, check fall damage, zero velocity components that hit a wall,
/// let the landed-on block react, and apply the block speed factor. A horizontal hit is classified
/// as minor (nearly head-on) with [`crate::input::is_horizontal_collision_minor`], which decides
/// whether sprinting survives it.
pub fn move_entity(p: &mut PlayerState, world: &World, motion: Vec3) {
    let mut vec3 = motion;
    if length_sqr(p.stuck_speed_multiplier) > 1.0E-7 {
        let s = p.stuck_speed_multiplier;
        vec3 = Vec3::new(vec3.x * s.x, vec3.y * s.y, vec3.z * s.z);
        p.stuck_speed_multiplier = Vec3::ZERO;
        set_vel(p, Vec3::ZERO);
    }
    vec3 = maybe_back_off_from_edge(p, world, vec3);
    let vec32 = collision::collide(p, world, vec3, max_up_step(p));
    let d = length_sqr(vec32);
    if d > 1.0E-7 || length_sqr(vec3) - d < 1.0E-7 {
        if p.fall_distance != 0.0 && d >= 1.0 {
            let e = jmin(d.sqrt(), 8.0);
            // vec32.normalize().scale(e), added to the position
            let len = d.sqrt();
            let unit = if len < f64::from(1.0E-5_f32) {
                Vec3::ZERO
            } else {
                Vec3::new(vec32.x / len, vec32.y / len, vec32.z / len)
            };
            let target = Vec3::new(
                p.pos.x + unit.x * e,
                p.pos.y + unit.y * e,
                p.pos.z + unit.z * e,
            );
            if fall_reset_clip_hits(world, p.pos, target) {
                reset_fall_distance(p);
            }
        }
        let from = p.pos;
        let to = Vec3::new(p.pos.x + vec32.x, p.pos.y + vec32.y, p.pos.z + vec32.z);
        // Entity.addMovementThisTick: the inside-block effects later walk this move.
        p.movements
            .record(crate::blocks::Movement::new(from, to, Some(vec3)));
        p.pos = to;
    }
    let bl = !mth_equal(vec3.x, vec32.x);
    let bl2 = !mth_equal(vec3.z, vec32.z);
    p.horizontal_collision = bl || bl2;
    // Math.abs(vec3.y) > 0.0 || isLocalInstanceAuthoritative(): always for the local player.
    p.vertical_collision = vec3.y != vec32.y;
    p.vertical_collision_below = p.vertical_collision && vec3.y < 0.0;
    let below = p.vertical_collision_below;
    let hc = p.horizontal_collision;
    set_on_ground_with_movement(p, world, below, hc, vec32);
    p.minor_horizontal_collision = if p.horizontal_collision {
        crate::input::is_horizontal_collision_minor(p.yaw, p.xxa, p.zza, vec32.x, vec32.z)
    } else {
        false
    };
    let on_pos = on_pos_legacy(p, world);
    let on_ground = p.on_ground;
    check_fall_damage(p, world, vec32.y, on_ground, on_pos);
    if p.horizontal_collision {
        let v = p.vel;
        set_vel(
            p,
            Vec3::new(if bl { 0.0 } else { v.x }, v.y, if bl2 { 0.0 } else { v.z }),
        );
    }
    if vec3.y != vec32.y {
        crate::blocks::after_fall_on(p, world, on_pos);
    }
    let f = f64::from(block_speed_factor(p, world));
    let v = p.vel;
    set_vel(p, Vec3::new(v.x * f, v.y * 1.0, v.z * f));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signum_keeps_zero_sign() {
        assert_eq!(signum(2.5), 1.0);
        assert_eq!(signum(-0.1), -1.0);
        assert_eq!(signum(-0.0).to_bits(), (-0.0_f64).to_bits());
    }

    fn floor_world(blocks: &[(&str, (i32, i32, i32))]) -> World {
        let mut grid = ms_world::GridWorld::new(ms_world::FlatWorld::new(0, ms_data::AIR));
        let stone = ms_data::parse_state("minecraft:stone").unwrap();
        grid.fill((-4, -1, -4), (4, -1, 4), stone);
        for (name, (x, y, z)) in blocks {
            grid.set_block(*x, *y, *z, ms_data::parse_state(name).unwrap());
        }
        World::grid(grid)
    }

    fn standing(x: f64, z: f64, world: &World) -> PlayerState {
        let mut p = PlayerState::new(Vec3::new(x, 0.0, z), 0.0);
        p.on_ground = true;
        check_supporting_block(&mut p, world, true, None);
        p
    }

    #[test]
    fn supporting_block_is_the_nearest_to_the_feet() {
        // Straddling the seam between two blocks: the one whose centre is closer wins.
        let world = floor_world(&[]);
        let p = standing(0.9, 0.5, &world);
        assert_eq!(p.supporting_block, Some((0, -1, 0)));
        let p = standing(1.1, 0.5, &world);
        assert_eq!(p.supporting_block, Some((1, -1, 0)));
        // Exactly on the seam: equal distances resolve towards the greater block position.
        let p = standing(1.0, 0.5, &world);
        assert_eq!(p.supporting_block, Some((1, -1, 0)));
        // In mid-air there is no supporting block.
        let mut q = PlayerState::new(Vec3::new(0.5, 3.0, 0.5), 0.0);
        check_supporting_block(&mut q, &world, false, None);
        assert_eq!(q.supporting_block, None);
    }

    #[test]
    fn fences_are_their_own_top_for_the_legacy_position() {
        // Standing on top of a fence (1.5 high): the supporting block is the fence itself, and for
        // the legacy 0.2 lookup the position 0.2 below the feet (block y = 1) is NOT used.
        let world = floor_world(&[("minecraft:oak_fence", (0, 0, 0))]);
        let mut p = PlayerState::new(Vec3::new(0.5, 1.5, 0.5), 0.0);
        p.on_ground = true;
        check_supporting_block(&mut p, &world, true, None);
        assert_eq!(p.supporting_block, Some((0, 0, 0)));
        assert_eq!(on_pos(&p, &world, 0.2), (0, 0, 0));
        // The 0.500001 lookup (used for friction) takes the block below the feet position instead.
        assert_eq!(on_pos(&p, &world, 0.500_001), (0, 0, 0));
        // Without a supporting block the position decides.
        p.supporting_block = None;
        assert_eq!(on_pos(&p, &world, 0.2), (0, 1, 0));
        let stone_world = floor_world(&[]);
        let q = standing(0.5, 0.5, &stone_world);
        assert_eq!(on_pos(&q, &stone_world, 0.2), (0, -1, 0));
        assert_eq!(on_pos(&q, &stone_world, 1.0E-5), (0, -1, 0));
    }

    #[test]
    fn stuck_multiplier_scales_one_move_and_is_consumed() {
        let world = World::void();
        let mut p = PlayerState::new(Vec3::new(0.5, 10.0, 0.5), 0.0);
        p.stuck_speed_multiplier = Vec3::new(0.25, 0.05, 0.25);
        p.vel = Vec3::new(0.2, -0.1, 0.2);
        move_entity(&mut p, &world, Vec3::new(0.2, -0.1, 0.2));
        assert_eq!(p.pos.x, 0.5 + 0.2 * 0.25);
        assert_eq!(p.pos.y, 10.0 + -0.1 * 0.05);
        assert_eq!(p.stuck_speed_multiplier, Vec3::ZERO);
        assert_eq!(
            p.vel,
            Vec3::ZERO,
            "the velocity is cancelled along with the multiplier"
        );
    }

    #[test]
    fn falling_accumulates_distance_and_landing_resets_it() {
        let world = floor_world(&[]);
        let mut p = PlayerState::new(Vec3::new(0.5, 5.0, 0.5), 0.0);
        for _ in 0..40 {
            let v = p.vel;
            move_entity(&mut p, &world, v);
            p.vel.y = (p.vel.y - 0.08) * f64::from(0.98_f32);
            if p.on_ground {
                break;
            }
        }
        assert!(p.on_ground);
        assert_eq!(p.fall_distance, 0.0);
        assert_eq!(p.pos.y, 0.0);
    }

    #[test]
    fn segment_box_test() {
        let b = aabb(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        assert!(segment_hits_box(
            Vec3::new(0.5, 3.0, 0.5),
            Vec3::new(0.0, -2.5, 0.0),
            b
        ));
        assert!(!segment_hits_box(
            Vec3::new(0.5, 3.0, 0.5),
            Vec3::new(0.0, -1.5, 0.0),
            b
        ));
        assert!(!segment_hits_box(
            Vec3::new(2.5, 3.0, 0.5),
            Vec3::new(0.0, -5.0, 0.0),
            b
        ));
    }
}
