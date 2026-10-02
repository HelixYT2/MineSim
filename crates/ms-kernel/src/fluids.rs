//! Fluids: the water/lava heights the player's box is submerged to, the flow push, the eye-in-water
//! test, the swimming state, bubble columns, and the in-fluid travel step.
//!
//! Owned by the fluid port. The kernel's tick calls these at the points the game does:
//!
//! * `Entity.baseTick` runs [`update_in_fluid_state_and_push`], then [`update_fluid_on_eyes`], then
//!   [`update_swimming`]; after those, the client clears fire and, `if in_lava`, halves
//!   `fall_distance` (that tail belongs to the base tick, not to this module).
//! * `LivingEntity.checkFallDamage` (called from `Entity.move` after the move) re-checks the water
//!   state at the *new* position when the player was not in water: call
//!   [`update_in_water_state_and_push`] there, `if !p.in_water`. It touches only the water half of
//!   the state, so the lava height from the start of the tick survives (the game does the same).
//! * `LocalPlayer.aiStep` calls [`go_down_in_water`] (sneaking in water); the jump block of
//!   `LivingEntity.aiStep` is [`ai_step_jump`].
//! * `Player.travel` calls [`swimming_travel_adjust`] first (the look-pitch rule), and
//!   `LivingEntity.travel` goes to [`travel_in_fluid`] when [`should_travel_in_fluid`] holds.
//! * `Entity.move` resets the fall distance when a move of at least a block crosses water or a
//!   fall-damage-resetting block: [`reset_fall_distance_on_crossing`], with the collided movement,
//!   before the position is advanced.
//! * Bubble columns are an `entityInside` effect: `apply_effects_from_blocks` calls
//!   [`bubble_column_entity_inside`] for every bubble-column block the box passed through
//!   (with the game's `precise` flag: the box really intersects the block), in the game's visiting
//!   order — the order matters, because the "above" and "inside" effects clamp differently. The
//!   lava/water `Fluid.entityInside` effects are [`lava_ignite`] / [`clear_fire`] /
//!   [`clear_freeze`].
//!
//! `fluidOnEyes` is carried across ticks in [`PlayerState::water_on_eyes`]: the game reads the
//! previous tick's value when it recomputes `wasEyeInWater`, so the recorded `eye_in_water` lags
//! the eye position by one tick (see [`update_fluid_on_eyes`]).

use crate::attributes::Attribute;
use crate::state::PlayerState;
use ms_data::{Fluid, FluidKind};
use ms_numerics::{mth, Vec3};
use ms_world::aabb::Aabb;
use ms_world::World;
use std::sync::OnceLock;

/// The `(float)(Math.PI / 180.0)` constant the rotation code multiplies by.
const DEG_TO_RAD: f32 = (std::f64::consts::PI / 180.0) as f32;

/// `Entity.updateInWaterStateAndDoWaterCurrentPushing`'s push strength.
const WATER_PUSH: f64 = 0.014;
/// Lava's push strength in the overworld (`FAST_LAVA` off); the nether uses 0.007.
const LAVA_PUSH: f64 = 0.0023333333333333335;

// ---------------------------------------------------------------------------------------------
// Java numeric helpers
// ---------------------------------------------------------------------------------------------

/// `Mth.floor(double)`.
fn mth_floor(d: f64) -> i32 {
    let i = d as i32;
    if d < f64::from(i) {
        i.wrapping_sub(1)
    } else {
        i
    }
}

/// `Mth.ceil(double)`.
fn mth_ceil(d: f64) -> i32 {
    let i = d as i32;
    if d > f64::from(i) {
        i.wrapping_add(1)
    } else {
        i
    }
}

/// `Math.max(double, double)`: NaN-propagating, and `max(-0.0, 0.0) = 0.0`.
fn jmax(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 && a.to_bits() == (-0.0_f64).to_bits() {
        return b;
    }
    if a >= b {
        a
    } else {
        b
    }
}

/// `Math.min(double, double)`.
fn jmin(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 && b.to_bits() == (-0.0_f64).to_bits() {
        return b;
    }
    if a <= b {
        a
    } else {
        b
    }
}

fn v_add(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

/// `Vec3.scale(double)`.
fn v_scale(a: Vec3, s: f64) -> Vec3 {
    Vec3::new(a.x * s, a.y * s, a.z * s)
}

/// `Vec3.length()`.
fn v_length(a: Vec3) -> f64 {
    (a.x * a.x + a.y * a.y + a.z * a.z).sqrt()
}

/// `Vec3.lengthSqr()`.
fn v_length_sqr(a: Vec3) -> f64 {
    a.x * a.x + a.y * a.y + a.z * a.z
}

/// `Vec3.normalize()`: zero below `1.0E-5F` (a float widened to double).
fn v_normalize(a: Vec3) -> Vec3 {
    let d = (a.x * a.x + a.y * a.y + a.z * a.z).sqrt();
    if d < f64::from(1.0e-5_f32) {
        Vec3::ZERO
    } else {
        Vec3::new(a.x / d, a.y / d, a.z / d)
    }
}

// ---------------------------------------------------------------------------------------------
// The player's box and block queries
// ---------------------------------------------------------------------------------------------

/// The player's bounding box (`EntityDimensions.makeBoundingBox`): half the width is a float that
/// is widened when it meets the double position.
fn bounding_box(p: &PlayerState) -> Aabb {
    let (w, h) = p.dimensions();
    let half = f64::from(w / 2.0);
    let h = f64::from(h);
    Aabb::new(
        Vec3::new(p.pos.x - half, p.pos.y, p.pos.z - half),
        Vec3::new(p.pos.x + half, p.pos.y + h, p.pos.z + half),
    )
}

/// `AABB.deflate(double)`.
fn deflate(b: Aabb, d: f64) -> Aabb {
    b.inflate(-d, -d, -d)
}

fn fluid_at(world: &World, x: i32, y: i32, z: i32) -> Fluid {
    ms_data::fluid(world.block_state(x, y, z))
}

/// `Fluid.isSame` between the kinds of two non-empty fluids: flowing and source water are the
/// same fluid, as are flowing and source lava.
fn same_fluid(a: Fluid, b: Fluid) -> bool {
    a.kind == b.kind
}

/// `FluidState.getHeight`: the fluid's own height, except that fluid of the same kind above
/// makes the block read as full (`FlowingFluid.hasSameAbove`).
fn fluid_height_at(world: &World, x: i32, y: i32, z: i32, f: Fluid) -> f32 {
    if same_fluid(f, fluid_at(world, x, y + 1, z)) {
        1.0
    } else {
        f.own_height()
    }
}

/// `Entity.isPushedByFluid` (the player's override): not while flying.
pub fn is_pushed_by_fluid(p: &PlayerState) -> bool {
    !p.flying
}

/// `LivingEntity.isAffectedByFluids` (the player's override): not while flying.
pub fn is_affected_by_fluids(p: &PlayerState) -> bool {
    !p.flying
}

/// `Entity.isInShallowWater`: in water, but with the eyes above it.
pub fn is_in_shallow_water(p: &PlayerState) -> bool {
    p.in_water && !p.eye_in_water
}

// ---------------------------------------------------------------------------------------------
// Flow vectors (`FlowingFluid.getFlow`)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dir {
    North,
    East,
    South,
    West,
    Up,
}

/// `Direction.Plane.HORIZONTAL` order with each direction's `(stepX, stepZ)`.
const HORIZONTAL: [(Dir, i32, i32); 4] = [
    (Dir::North, 0, -1),
    (Dir::East, 1, 0),
    (Dir::South, 0, 1),
    (Dir::West, -1, 0),
];

/// Per-block flags derived once from the game's block properties.
const FLAG_FORCE_SOLID_ON: u8 = 1;
const FLAG_FORCE_SOLID_OFF: u8 = 2;
const FLAG_DYNAMIC_SHAPE: u8 = 4;
const FLAG_ICE: u8 = 8;
const FLAG_LEAVES: u8 = 16;

fn block_flags(block: usize) -> u8 {
    static FLAGS: OnceLock<Vec<u8>> = OnceLock::new();
    FLAGS.get_or_init(|| {
        (0..ms_data::BLOCK_COUNT)
            .map(|b| {
                let name = ms_data::block_name(b);
                let short = name.strip_prefix("minecraft:").unwrap_or(name);
                let mut f = 0;
                if FORCE_SOLID_ON.binary_search(&short).is_ok() {
                    f |= FLAG_FORCE_SOLID_ON;
                }
                if FORCE_SOLID_OFF.binary_search(&short).is_ok() {
                    f |= FLAG_FORCE_SOLID_OFF;
                }
                if DYNAMIC_SHAPE.binary_search(&short).is_ok() {
                    f |= FLAG_DYNAMIC_SHAPE;
                }
                // `instanceof IceBlock` (FrostedIceBlock extends it).
                if matches!(ms_data::block_class(b), "IceBlock" | "FrostedIceBlock") {
                    f |= FLAG_ICE;
                }
                if ms_data::block_has_tag(b, "minecraft:leaves") {
                    f |= FLAG_LEAVES;
                }
                f
            })
            .collect()
    })[block]
}

/// `BlockState.blocksMotion`: not cobweb or bamboo sapling, and "solid" in the legacy sense:
/// forced on or off by the block's properties, false for blocks with a dynamic shape, otherwise
/// decided by the bounds of the collision shape (a mean extent of at least 0.7291666666666666, or a
/// full-height one).
fn blocks_motion(state: u32) -> bool {
    let block = ms_data::block_of_state(state);
    let flags = block_flags(block);
    let name = ms_data::block_name(block);
    if name == "minecraft:cobweb" || name == "minecraft:bamboo_sapling" {
        return false;
    }
    if flags & FLAG_FORCE_SOLID_ON != 0 {
        return true;
    }
    if flags & (FLAG_FORCE_SOLID_OFF | FLAG_DYNAMIC_SHAPE) != 0 {
        return false;
    }
    let boxes = ms_data::collision_boxes(state);
    if boxes.is_empty() {
        return false;
    }
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for b in boxes {
        for a in 0..3 {
            lo[a] = lo[a].min(b[a]);
            hi[a] = hi[a].max(b[a + 3]);
        }
    }
    let (dx, dy, dz) = (hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
    (dx + dy + dz) / 3.0 >= 0.7291666666666666 || dy >= 1.0
}

/// The shape `BlockBehaviour.getBlockSupportShape` reports, as boxes: the collision shape except
/// for the few blocks that override it (leaves report none; mud and soul sand a full block; snow
/// layers a column `2 * layers` pixels high).
fn support_boxes(state: u32) -> Vec<[f64; 6]> {
    let block = ms_data::block_of_state(state);
    if block_flags(block) & FLAG_LEAVES != 0 {
        return Vec::new();
    }
    match ms_data::block_class(block) {
        "MudBlock" | "SoulSandBlock" => vec![[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]],
        "SnowLayerBlock" => {
            let layers: f64 = ms_data::property(state, "layers")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1.0);
            vec![[0.0, 0.0, 0.0, 1.0, layers * 2.0 / 16.0, 1.0]]
        }
        _ => ms_data::collision_boxes(state).to_vec(),
    }
}

/// `Block.isFaceFull(shape, direction)` for a union of boxes: the cross-section of the shape at the
/// face, projected onto the face plane, covers the whole unit square (and sticks out of none of it).
fn face_full(boxes: &[[f64; 6]], dir: Dir) -> bool {
    let (axis, positive) = match dir {
        Dir::North => (2, false),
        Dir::South => (2, true),
        Dir::West => (0, false),
        Dir::East => (0, true),
        Dir::Up => (1, true),
    };
    // `VoxelShape.calculateFace` slices at the cell containing 1 - 1.0E-7 (or 1.0E-7).
    let at = if positive { 0.9999999 } else { 1.0e-7 };
    let covering: Vec<&[f64; 6]> = boxes
        .iter()
        .filter(|b| b[axis] <= at && at < b[axis + 3])
        .collect();
    if covering.is_empty() {
        return false;
    }
    let (a1, a2) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    // The slice sticking out of the unit square leaves a region that is in one shape only.
    if covering
        .iter()
        .any(|b| b[a1] < 0.0 || b[a1 + 3] > 1.0 || b[a2] < 0.0 || b[a2 + 3] > 1.0)
    {
        return false;
    }
    let coords = |a: usize| {
        let mut c = vec![0.0, 1.0];
        for b in &covering {
            c.push(b[a]);
            c.push(b[a + 3]);
        }
        c.sort_by(f64::total_cmp);
        c.dedup();
        c
    };
    let (c1, c2) = (coords(a1), coords(a2));
    for w1 in c1.windows(2) {
        for w2 in c2.windows(2) {
            let covered = covering.iter().any(|b| {
                b[a1] <= w1[0] && w1[1] <= b[a1 + 3] && b[a2] <= w2[0] && w2[1] <= b[a2 + 3]
            });
            if !covered {
                return false;
            }
        }
    }
    true
}

/// `FlowingFluid.isSolidFace`: whether the block at `(x, y, z)` shuts a falling fluid's side.
fn is_solid_face(world: &World, kind: FluidKind, x: i32, y: i32, z: i32, dir: Dir) -> bool {
    let state = world.block_state(x, y, z);
    if ms_data::fluid(state).kind == kind {
        return false;
    }
    if dir == Dir::Up {
        return true;
    }
    if block_flags(ms_data::block_of_state(state)) & FLAG_ICE != 0 {
        return false;
    }
    face_full(&support_boxes(state), dir)
}

/// `FlowingFluid.affectsFlow`: empty or the same fluid.
fn affects_flow(f: Fluid, kind: FluidKind) -> bool {
    f.is_empty() || f.kind == kind
}

/// `FlowingFluid.getFlow` for the fluid `f` at `(x, y, z)`: the horizontal pull toward lower
/// neighbours (float height differences, summed in double), pointed straight down if the fluid is
/// falling and a side is walled, and normalised.
fn get_flow(world: &World, x: i32, y: i32, z: i32, f: Fluid) -> Vec3 {
    let mut d = 0.0_f64;
    let mut e = 0.0_f64;
    for (_, dx, dz) in HORIZONTAL {
        let (nx, nz) = (x + dx, z + dz);
        let f2 = fluid_at(world, nx, y, nz);
        if !affects_flow(f2, f.kind) {
            continue;
        }
        let mut h = f2.own_height();
        let mut g = 0.0_f32;
        if h == 0.0 {
            if !blocks_motion(world.block_state(nx, y, nz)) {
                let f3 = fluid_at(world, nx, y - 1, nz);
                if affects_flow(f3, f.kind) {
                    h = f3.own_height();
                    if h > 0.0 {
                        g = f.own_height() - (h - 0.8888889_f32);
                    }
                }
            }
        } else if h > 0.0 {
            g = f.own_height() - h;
        }
        if g != 0.0 {
            // `direction.getStepX() * g` is an int times a float: a float product.
            d += f64::from(dx as f32 * g);
            e += f64::from(dz as f32 * g);
        }
    }
    let mut v = Vec3::new(d, 0.0, e);
    if f.falling {
        for (dir, dx, dz) in HORIZONTAL {
            let (nx, nz) = (x + dx, z + dz);
            if is_solid_face(world, f.kind, nx, y, nz, dir)
                || is_solid_face(world, f.kind, nx, y + 1, nz, dir)
            {
                v = v_add(v_normalize(v), Vec3::new(0.0, -6.0, 0.0));
                break;
            }
        }
    }
    v_normalize(v)
}

// ---------------------------------------------------------------------------------------------
// Entity.updateFluidHeightAndDoFluidPushing and its callers
// ---------------------------------------------------------------------------------------------

/// `Entity.updateFluidHeightAndDoFluidPushing(tag, push)`: returns whether the box touches the
/// fluid and the greatest depth the fluid reaches above the box's floor (the value the game keeps in
/// `fluidHeight`). Adds the current's push to `p.vel`.
///
/// The game's `touchingUnloadedChunk` early-out is not modelled: every chunk is considered loaded.
fn update_fluid_height_and_push(
    p: &mut PlayerState,
    world: &World,
    kind: FluidKind,
    push: f64,
) -> (bool, f64) {
    // A world with no state of this fluid anywhere: every cell below would fail the kind test,
    // leaving `touching = false`, `depth = 0`, a zero flow and so no push. The result and the
    // (absent) side effects are the same without the scan.
    let none_of_this_fluid = match kind {
        FluidKind::Water => !world.may_contain(ms_data::class::WATER),
        FluidKind::Lava => !world.may_contain(ms_data::class::LAVA),
        FluidKind::Empty => false,
    };
    if none_of_this_fluid {
        return (false, 0.0);
    }
    let bb = deflate(bounding_box(p), 0.001);
    let (i, j) = (mth_floor(bb.min.x), mth_ceil(bb.max.x));
    // Cells above the world's highest block are air and hold no fluid.
    let mut l = mth_ceil(bb.max.y);
    if kind != FluidKind::Empty {
        l = l.min(world.max_block_y().saturating_add(1));
    }
    let k = mth_floor(bb.min.y);
    let (m, n) = (mth_floor(bb.min.z), mth_ceil(bb.max.z));
    let mut depth = 0.0_f64;
    let pushed = is_pushed_by_fluid(p);
    let mut touching = false;
    let mut flow_sum = Vec3::ZERO;
    let mut count = 0_i32;
    for x in i..j {
        for y in k..l {
            for z in m..n {
                let f = fluid_at(world, x, y, z);
                if f.kind != kind {
                    continue;
                }
                // `q + fluidState.getHeight(...)` is an int plus a float: a float sum.
                let top = f64::from(y as f32 + fluid_height_at(world, x, y, z, f));
                if top >= bb.min.y {
                    touching = true;
                    depth = jmax(top - bb.min.y, depth);
                    if pushed {
                        let mut flow = get_flow(world, x, y, z, f);
                        if depth < 0.4 {
                            flow = v_scale(flow, depth);
                        }
                        flow_sum = v_add(flow_sum, flow);
                        count += 1;
                    }
                }
            }
        }
    }
    if v_length(flow_sum) > 0.0 {
        if count > 0 {
            flow_sum = v_scale(flow_sum, 1.0 / f64::from(count));
        }
        // Only non-player entities normalise the averaged flow here.
        let vel = p.vel;
        flow_sum = v_scale(flow_sum, push);
        if vel.x.abs() < 0.003 && vel.z.abs() < 0.003 && v_length(flow_sum) < 0.0045000000000000005
        {
            flow_sum = v_scale(v_normalize(flow_sum), 0.0045000000000000005);
        }
        p.vel = v_add(p.vel, flow_sum);
    }
    (touching, depth)
}

/// The water half of `Entity.updateInWaterStateAndDoFluidPushing`
/// (`updateInWaterStateAndDoWaterCurrentPushing`): refresh `in_water` and `water_height`, push by
/// the current, and reset the fall distance on touching water.
///
/// `LivingEntity.checkFallDamage` calls exactly this, after the move and only when the player was
/// not in water, to catch the water entered during the move. The lava state is left alone.
pub fn update_in_water_state_and_push(p: &mut PlayerState, world: &World) {
    let (touching, depth) = update_fluid_height_and_push(p, world, FluidKind::Water, WATER_PUSH);
    p.water_height = depth;
    if touching {
        // (`doWaterSplashEffect` only plays sounds and particles.)
        p.fall_distance = 0.0;
        p.in_water = true;
    } else {
        p.in_water = false;
    }
}

/// `Entity.updateInWaterStateAndDoFluidPushing` (with `updateFluidHeightAndDoFluidPushing` for
/// water then lava): refresh `in_water`, `in_lava`, `water_height`, `lava_height`, apply the
/// current's push to `vel`, and reset the fall distance when touching water.
///
/// The fire effects of the fluids are not here: in this version water extinguishes and lava
/// ignites through `Fluid.entityInside` during `applyEffectsFromBlocks` (see [`lava_ignite`] and
/// [`clear_fire`]). The base tick's `fall_distance *= 0.5` while `in_lava` follows this call.
pub fn update_in_fluid_state_and_push(p: &mut PlayerState, world: &World) {
    // `fluidHeight.clear()`
    p.water_height = 0.0;
    p.lava_height = 0.0;
    // Without any fluid in the world both scans below find nothing: not touching, depth 0.
    if !world.may_contain(ms_data::class::FLUID) {
        p.in_water = false;
        p.in_lava = false;
        return;
    }
    update_in_water_state_and_push(p, world);
    let (_touching, depth) = update_fluid_height_and_push(p, world, FluidKind::Lava, LAVA_PUSH);
    p.lava_height = depth;
    // `isInLava`: `!firstTick && fluidHeight(LAVA) > 0`.
    p.in_lava = depth > 0.0;
}

/// `Entity.updateFluidOnEyes` plus the one-tick lag the local player shows: `eye_in_water`
/// (`LocalPlayer.isUnderWater`, `wasUnderwater`) takes the previous tick's "water on the eyes"
/// result, and the new result — the fluid at the eye block reaching above the eye — is stored in
/// `water_on_eyes` for the next tick. The eye is `pos.y + eyeHeight` (no further offset in this
/// version).
pub fn update_fluid_on_eyes(p: &mut PlayerState, world: &World) {
    p.eye_in_water = p.water_on_eyes;
    p.water_on_eyes = false;
    // Without any water in the world the eye is never in it and nothing more happens.
    if !world.may_contain(ms_data::class::WATER) {
        return;
    }
    let d = p.pos.y + f64::from(p.eye_height());
    let (bx, by, bz) = (mth_floor(p.pos.x), mth_floor(d), mth_floor(p.pos.z));
    let f = fluid_at(world, bx, by, bz);
    if !f.is_empty() {
        // `blockPos.getY() + fluidState.getHeight(...)`: int plus float, a float sum.
        let e = f64::from(by as f32 + fluid_height_at(world, bx, by, bz, f));
        if e > d && f.kind == FluidKind::Water {
            p.water_on_eyes = true;
        }
    }
}

/// `Entity.updateSwimming` (the player's override included): flying stops swimming; a swimmer keeps
/// it while sprinting in water; otherwise sprinting with the eyes under water starts it, provided
/// the block the feet are in holds water.
pub fn update_swimming(p: &mut PlayerState, world: &World) {
    if p.flying {
        p.swimming = false;
    } else if p.swimming {
        p.swimming = p.sprinting && p.in_water;
    } else {
        p.swimming = p.sprinting
            && p.eye_in_water
            && fluid_at(
                world,
                mth_floor(p.pos.x),
                mth_floor(p.pos.y),
                mth_floor(p.pos.z),
            )
            .kind
                == FluidKind::Water;
    }
}

// ---------------------------------------------------------------------------------------------
// Jumping and sinking
// ---------------------------------------------------------------------------------------------

/// `Entity.getFluidJumpThreshold`.
pub fn fluid_jump_threshold(p: &PlayerState) -> f64 {
    if f64::from(p.eye_height()) < 0.4 {
        0.0
    } else {
        0.4
    }
}

/// `LivingEntity.jumpInLiquid`: an upward nudge of `0.04F`.
pub fn jump_in_liquid(p: &mut PlayerState) {
    p.vel = v_add(p.vel, Vec3::new(0.0, f64::from(0.04_f32), 0.0));
}

/// `LivingEntity.goDownInWater`: a downward nudge of `0.04F` (sneaking in water).
pub fn go_down_in_water(p: &mut PlayerState) {
    p.vel = v_add(p.vel, Vec3::new(0.0, f64::from(-0.04_f32), 0.0));
}

/// The jump block of `LivingEntity.aiStep`: with the jump key held and fluids acting on the
/// player, either swim up (`jumpInLiquid`) or jump from the ground (`jump_from_ground`, supplied by
/// the caller, which then also gets the 10-tick delay); with the key released the delay resets.
// The comparisons keep the reference's `!(g > h)` form, which differs from `g <= h` for NaN.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn ai_step_jump(p: &mut PlayerState, jump_from_ground: &mut dyn FnMut(&mut PlayerState)) {
    if p.jumping && is_affected_by_fluids(p) {
        let g = if p.in_lava {
            p.lava_height
        } else {
            p.water_height
        };
        let in_water_here = p.in_water && g > 0.0;
        let h = fluid_jump_threshold(p);
        if !in_water_here || p.on_ground && !(g > h) {
            if !p.in_lava || p.on_ground && !(g > h) {
                if (p.on_ground || in_water_here && g <= h) && p.no_jump_delay == 0 {
                    jump_from_ground(p);
                    p.no_jump_delay = 10;
                }
            } else {
                jump_in_liquid(p);
            }
        } else {
            jump_in_liquid(p);
        }
    } else {
        p.no_jump_delay = 0;
    }
}

// ---------------------------------------------------------------------------------------------
// Travel
// ---------------------------------------------------------------------------------------------

/// The fluid state at the player's block position decides whether `travel` uses the fluid
/// branch (`LivingEntity.shouldTravelInFluid`; the player cannot stand on fluids).
pub fn should_travel_in_fluid(p: &PlayerState, world: &World) -> bool {
    let _ = world;
    (p.in_water || p.in_lava) && is_affected_by_fluids(p)
}

/// `Entity.getLookAngle().y` for the player's pitch: the float `-sin(pitch)` widened.
fn look_angle_y(p: &PlayerState) -> f64 {
    f64::from(-mth::sin(p.pitch * DEG_TO_RAD))
}

/// The swimming rule at the top of `Player.travel` (when not riding): while swimming, the vertical
/// velocity is pulled toward the look direction's Y component, harder when looking steeply down —
/// unless looking up out of the water. Call before `LivingEntity.travel`.
pub fn swimming_travel_adjust(p: &mut PlayerState, world: &World) {
    // `Player.isSwimming`: the flag, and not while flying.
    if p.flying || !p.swimming {
        return;
    }
    let d = look_angle_y(p);
    let e = if d < -0.2 { 0.085 } else { 0.06 };
    let above = fluid_at(
        world,
        mth_floor(p.pos.x),
        mth_floor(p.pos.y + 1.0 - 0.1),
        mth_floor(p.pos.z),
    );
    if d <= 0.0 || p.jumping || !above.is_empty() {
        let v = p.vel;
        p.vel = v_add(v, Vec3::new(0.0, (d - v.y) * e, 0.0));
    }
}

/// `Entity.moveRelative(speed, input)`: add the input, scaled by `speed` and rotated by the yaw, to
/// the velocity (`Entity.getInputVector`).
fn move_relative(p: &mut PlayerState, speed: f32, input: Vec3) {
    let lensq = v_length_sqr(input);
    if lensq < 1.0e-7 {
        return;
    }
    let v = v_scale(
        if lensq > 1.0 {
            v_normalize(input)
        } else {
            input
        },
        f64::from(speed),
    );
    let sin = mth::sin(p.yaw * DEG_TO_RAD);
    let cos = mth::cos(p.yaw * DEG_TO_RAD);
    let delta = Vec3::new(
        v.x * f64::from(cos) - v.z * f64::from(sin),
        v.y,
        v.z * f64::from(cos) + v.x * f64::from(sin),
    );
    p.vel = v_add(p.vel, delta);
}

/// `LivingEntity.getEffectiveGravity` (the gravity attribute; slow falling caps it at 0.01 while
/// not rising).
fn effective_gravity(p: &PlayerState) -> f64 {
    let gravity = p.attributes.value(Attribute::Gravity);
    let falling = p.vel.y <= 0.0;
    if falling && p.effects.has("minecraft:slow_falling") {
        jmin(gravity, 0.01)
    } else {
        gravity
    }
}

/// `LivingEntity.getFluidFallingAdjustedMovement`: ease the sinking speed toward `-gravity / 16`
/// (and snap to `-0.003` near the resting speed), unless sprinting or weightless.
fn fluid_falling_adjusted_movement(p: &PlayerState, gravity: f64, falling: bool, v: Vec3) -> Vec3 {
    if gravity != 0.0 && !p.sprinting {
        let e = if falling && (v.y - 0.005).abs() >= 0.003 && (v.y - gravity / 16.0).abs() < 0.003 {
            -0.003
        } else {
            v.y - gravity / 16.0
        };
        Vec3::new(v.x, e, v.z)
    } else {
        v
    }
}

/// `LivingEntity.jumpOutOfFluid`: when pushing against a wall and the spot a bit above the water
/// line is free, pop up out of the fluid (`0.3F`). `y_before` is the Y before this tick's move.
fn jump_out_of_fluid(p: &mut PlayerState, world: &World, y_before: f64) {
    let v = p.vel;
    if p.horizontal_collision
        && is_free(
            p,
            world,
            v.x,
            v.y + f64::from(0.6_f32) - p.pos.y + y_before,
            v.z,
        )
    {
        p.vel = Vec3::new(v.x, f64::from(0.3_f32), v.z);
    }
}

/// `Entity.isFree(dx, dy, dz)`: the box moved by `(dx, dy, dz)` hits no block collision shape and
/// contains no fluid. (Block shapes come from [`crate::blocks::collision_boxes`]; overlap is the
/// strict box test, without the shape merger's 1e-7 tolerance.)
fn is_free(p: &PlayerState, world: &World, dx: f64, dy: f64, dz: f64) -> bool {
    let bb = bounding_box(p);
    let moved = Aabb::new(
        Vec3::new(bb.min.x + dx, bb.min.y + dy, bb.min.z + dz),
        Vec3::new(bb.max.x + dx, bb.max.y + dy, bb.max.z + dz),
    );
    no_block_collision(p, world, moved) && !contains_any_liquid(world, moved)
}

/// `CollisionGetter.noBlockCollision`: no collision box of any block in the (one-block padded)
/// range overlaps `bb`.
fn no_block_collision(p: &PlayerState, world: &World, bb: Aabb) -> bool {
    let (x0, x1) = (
        mth_floor(bb.min.x - 1.0e-7) - 1,
        mth_floor(bb.max.x + 1.0e-7) + 1,
    );
    let (y0, y1) = (
        mth_floor(bb.min.y - 1.0e-7) - 1,
        mth_floor(bb.max.y + 1.0e-7) + 1,
    );
    let (z0, z1) = (
        mth_floor(bb.min.z - 1.0e-7) - 1,
        mth_floor(bb.max.z + 1.0e-7) + 1,
    );
    for x in x0..=x1 {
        for y in y0..=y1 {
            for z in z0..=z1 {
                for s in crate::blocks::collision_boxes(p, bb, world, x, y, z) {
                    let b = Aabb::new(
                        Vec3::new(
                            f64::from(x) + s[0],
                            f64::from(y) + s[1],
                            f64::from(z) + s[2],
                        ),
                        Vec3::new(
                            f64::from(x) + s[3],
                            f64::from(y) + s[4],
                            f64::from(z) + s[5],
                        ),
                    );
                    if b.intersects(bb) {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// `LevelReader.containsAnyLiquid`.
fn contains_any_liquid(world: &World, bb: Aabb) -> bool {
    let (i, j) = (mth_floor(bb.min.x), mth_ceil(bb.max.x));
    let (k, l) = (mth_floor(bb.min.y), mth_ceil(bb.max.y));
    let (m, n) = (mth_floor(bb.min.z), mth_ceil(bb.max.z));
    for x in i..j {
        for y in k..l {
            for z in m..n {
                if !fluid_at(world, x, y, z).is_empty() {
                    return true;
                }
            }
        }
    }
    false
}

/// `LivingEntity.travelInFluid` (water and lava branches, including `jumpOutOfFluid` and the
/// falling adjustment). `input` is `(xxa, yya, zza)`; the caller has checked
/// [`should_travel_in_fluid`].
///
/// `move_fn` is `Entity.move(MoverType.SELF, delta)` from the core tick: it must advance the
/// position, set the collision flags, and apply the post-move steps (fall damage bookkeeping,
/// velocity zeroing on contact). This function reads `horizontal_collision` and `pos` after it.
pub fn travel_in_fluid(
    p: &mut PlayerState,
    world: &World,
    input: Vec3,
    move_fn: &mut dyn FnMut(&mut PlayerState, Vec3),
) {
    let falling = p.vel.y <= 0.0;
    let y_before = p.pos.y;
    let gravity = effective_gravity(p);
    if p.in_water {
        travel_in_water(p, world, input, gravity, falling, y_before, move_fn);
    } else {
        travel_in_lava(p, world, input, gravity, falling, y_before, move_fn);
    }
}

/// `LivingEntity.travelInWater`.
fn travel_in_water(
    p: &mut PlayerState,
    world: &World,
    input: Vec3,
    gravity: f64,
    falling: bool,
    y_before: f64,
    move_fn: &mut dyn FnMut(&mut PlayerState, Vec3),
) {
    // `getWaterSlowDown()` is 0.8F.
    let mut f: f32 = if p.sprinting { 0.9 } else { 0.8 };
    let mut g: f32 = 0.02;
    let mut h = p.attributes.value(Attribute::WaterMovementEfficiency) as f32;
    if !p.on_ground {
        h *= 0.5;
    }
    if h > 0.0 {
        f += (0.546_000_06_f32 - f) * h;
        // `Player.getSpeed()` is the movement-speed attribute as a float.
        let speed = p.attributes.value(Attribute::MovementSpeed) as f32;
        g += (speed - g) * h;
    }
    if p.effects.has("minecraft:dolphins_grace") {
        f = 0.96;
    }
    move_relative(p, g, input);
    let motion = p.vel;
    move_fn(p, motion);
    let mut v = p.vel;
    if p.horizontal_collision && crate::blocks::on_climbable(p, world) {
        v = Vec3::new(v.x, 0.2, v.z);
    }
    v = Vec3::new(
        v.x * f64::from(f),
        v.y * f64::from(0.8_f32),
        v.z * f64::from(f),
    );
    p.vel = fluid_falling_adjusted_movement(p, gravity, falling, v);
    jump_out_of_fluid(p, world, y_before);
}

/// `LivingEntity.travelInLava`.
fn travel_in_lava(
    p: &mut PlayerState,
    world: &World,
    input: Vec3,
    gravity: f64,
    falling: bool,
    y_before: f64,
    move_fn: &mut dyn FnMut(&mut PlayerState, Vec3),
) {
    move_relative(p, 0.02, input);
    let motion = p.vel;
    move_fn(p, motion);
    if p.lava_height <= fluid_jump_threshold(p) {
        let v = p.vel;
        p.vel = Vec3::new(v.x * 0.5, v.y * f64::from(0.8_f32), v.z * 0.5);
        p.vel = fluid_falling_adjusted_movement(p, gravity, falling, p.vel);
    } else {
        p.vel = v_scale(p.vel, 0.5);
    }
    if gravity != 0.0 {
        p.vel = v_add(p.vel, Vec3::new(0.0, -gravity / 4.0, 0.0));
    }
    jump_out_of_fluid(p, world, y_before);
}

// ---------------------------------------------------------------------------------------------
// The fall-distance reset on crossing water (the clip in `Entity.move`)
// ---------------------------------------------------------------------------------------------

/// `Mth.lfloor(double)`.
fn mth_lfloor(d: f64) -> i64 {
    let l = d as i64;
    if d < l as f64 {
        l - 1
    } else {
        l
    }
}

/// `Mth.frac(double)`.
fn mth_frac(d: f64) -> f64 {
    d - mth_lfloor(d) as f64
}

/// `Mth.sign(double)`.
fn mth_sign(d: f64) -> i32 {
    if d == 0.0 {
        0
    } else if d > 0.0 {
        1
    } else {
        -1
    }
}

/// `AABB.clipPoint` for one face of one box; updates the nearest hit parameter `ds` and reports
/// whether this face is the new nearest hit.
#[allow(clippy::too_many_arguments)]
fn clip_point(
    ds: &mut f64,
    d: f64,
    e: f64,
    f: f64,
    g: f64,
    h: f64,
    i: f64,
    j: f64,
    k: f64,
    l: f64,
    m: f64,
    n: f64,
) -> bool {
    let o = (g - l) / d;
    let p = m + o * e;
    let q = n + o * f;
    if 0.0 < o && o < *ds && h - 1.0e-7 < p && p < i + 1.0e-7 && j - 1.0e-7 < q && q < k + 1.0e-7 {
        *ds = o;
        true
    } else {
        false
    }
}

/// `VoxelShape.clip` for a single box `[min, max]` (already in world coordinates) against the
/// segment `from -> to`: whether the segment hits it. A segment that starts inside the box (its
/// point 0.1% along lies in the half-open box) counts as a hit.
fn clip_box_hits(min: [f64; 3], max: [f64; 3], from: Vec3, to: Vec3) -> bool {
    let delta = Vec3::new(to.x - from.x, to.y - from.y, to.z - from.z);
    if v_length_sqr(delta) < 1.0e-7 {
        return false;
    }
    let near = v_add(from, v_scale(delta, 0.001));
    let inside = |v: f64, a: usize| min[a] <= v && v < max[a];
    if inside(near.x, 0) && inside(near.y, 1) && inside(near.z, 2) {
        return true;
    }
    // `AABB.getDirection`: the nearest face the segment enters.
    let (dx, dy, dz) = (delta.x, delta.y, delta.z);
    let mut ds = 1.0_f64;
    let mut hit = false;
    if dx > 1.0e-7 {
        hit |= clip_point(
            &mut ds, dx, dy, dz, min[0], min[1], max[1], min[2], max[2], from.x, from.y, from.z,
        );
    } else if dx < -1.0e-7 {
        hit |= clip_point(
            &mut ds, dx, dy, dz, max[0], min[1], max[1], min[2], max[2], from.x, from.y, from.z,
        );
    }
    if dy > 1.0e-7 {
        hit |= clip_point(
            &mut ds, dy, dz, dx, min[1], min[2], max[2], min[0], max[0], from.y, from.z, from.x,
        );
    } else if dy < -1.0e-7 {
        hit |= clip_point(
            &mut ds, dy, dz, dx, max[1], min[2], max[2], min[0], max[0], from.y, from.z, from.x,
        );
    }
    if dz > 1.0e-7 {
        hit |= clip_point(
            &mut ds, dz, dx, dy, min[2], min[0], max[0], min[1], max[1], from.z, from.x, from.y,
        );
    } else if dz < -1.0e-7 {
        hit |= clip_point(
            &mut ds, dz, dx, dy, max[2], min[0], max[0], min[1], max[1], from.z, from.x, from.y,
        );
    }
    hit
}

/// `BlockGetter.traverseBlocks`: visit the blocks the segment passes through, in order, until the
/// visitor reports a hit.
fn traverse_blocks(from: Vec3, to: Vec3, visit: &mut dyn FnMut(i32, i32, i32) -> bool) -> bool {
    if from == to {
        return false;
    }
    let lerp = |a: f64, b: f64, c: f64| b + a * (c - b);
    let (d, e, f) = (
        lerp(-1.0e-7, to.x, from.x),
        lerp(-1.0e-7, to.y, from.y),
        lerp(-1.0e-7, to.z, from.z),
    );
    let (g, h, i) = (
        lerp(-1.0e-7, from.x, to.x),
        lerp(-1.0e-7, from.y, to.y),
        lerp(-1.0e-7, from.z, to.z),
    );
    let (mut j, mut k, mut l) = (mth_floor(g), mth_floor(h), mth_floor(i));
    if visit(j, k, l) {
        return true;
    }
    let (m, n, o) = (d - g, e - h, f - i);
    let (p, q, r) = (mth_sign(m), mth_sign(n), mth_sign(o));
    let s = if p == 0 { f64::MAX } else { f64::from(p) / m };
    let t = if q == 0 { f64::MAX } else { f64::from(q) / n };
    let u = if r == 0 { f64::MAX } else { f64::from(r) / o };
    let mut v = s * if p > 0 {
        1.0 - mth_frac(g)
    } else {
        mth_frac(g)
    };
    let mut w = t * if q > 0 {
        1.0 - mth_frac(h)
    } else {
        mth_frac(h)
    };
    let mut x = u * if r > 0 {
        1.0 - mth_frac(i)
    } else {
        mth_frac(i)
    };
    while v <= 1.0 || w <= 1.0 || x <= 1.0 {
        if v < w {
            if v < x {
                j += p;
                v += s;
            } else {
                l += r;
                x += u;
            }
        } else if w < x {
            k += q;
            w += t;
        } else {
            l += r;
            x += u;
        }
        if visit(j, k, l) {
            return true;
        }
    }
    false
}

/// `Level.clip` with `ClipContext.Block.FALLDAMAGE_RESETTING` and `ClipContext.Fluid.WATER` for
/// the player: whether the segment `from -> to` hits a block tagged `fall_damage_resetting` (cobweb,
/// vines, ladders, ... as full blocks; also end portals/gateways), or any water (its shape is the
/// block's column up to the fluid height).
pub fn fall_damage_resetting_clip_hits(world: &World, from: Vec3, to: Vec3) -> bool {
    traverse_blocks(from, to, &mut |x, y, z| {
        let state = world.block_state(x, y, z);
        let block = ms_data::block_of_state(state);
        let name = ms_data::block_name(block);
        let (fx, fy, fz) = (f64::from(x), f64::from(y), f64::from(z));
        if (ms_data::block_has_tag(block, "minecraft:fall_damage_resetting")
            || name == "minecraft:end_gateway"
            || name == "minecraft:end_portal")
            && clip_box_hits([fx, fy, fz], [fx + 1.0, fy + 1.0, fz + 1.0], from, to)
        {
            return true;
        }
        let f = ms_data::fluid(state);
        if f.kind == FluidKind::Water {
            let h = f64::from(fluid_height_at(world, x, y, z, f));
            return clip_box_hits([fx, fy, fz], [fx + 1.0, fy + h, fz + 1.0], from, to);
        }
        false
    })
}

/// The block in `Entity.move` that resets the fall distance when a long move (at least one block)
/// crosses water or a fall-damage-resetting block: `moved` is the collided movement, applied from
/// the current position. Call before the position is advanced.
pub fn reset_fall_distance_on_crossing(p: &mut PlayerState, world: &World, moved: Vec3) {
    let d = v_length_sqr(moved);
    if p.fall_distance != 0.0 && d >= 1.0 {
        let e = jmin(v_length(moved), 8.0);
        let end = v_add(p.pos, v_scale(v_normalize(moved), e));
        if fall_damage_resetting_clip_hits(world, p.pos, end) {
            p.fall_distance = 0.0;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Bubble columns and the fluids' entityInside effects
// ---------------------------------------------------------------------------------------------

/// `Entity.handleOnAboveBubbleColumn` through `Player.onAboveBubbleColumn` (which ignores the
/// column while flying): at the top of a column the velocity is driven toward `-0.9` (drag down)
/// or `1.8` (upward column).
pub fn on_above_bubble_column(p: &mut PlayerState, drag_down: bool) {
    if p.flying {
        return;
    }
    let v = p.vel;
    let y = if drag_down {
        jmax(-0.9, v.y - 0.03)
    } else {
        jmin(1.8, v.y + 0.1)
    };
    p.vel = Vec3::new(v.x, y, v.z);
}

/// `Entity.handleOnInsideBubbleColumn` through `Player.onInsideBubbleColumn`: inside a column the
/// velocity is driven toward `-0.3` (drag down) or `0.7`, and the fall distance resets.
pub fn on_inside_bubble_column(p: &mut PlayerState, drag_down: bool) {
    if p.flying {
        return;
    }
    let v = p.vel;
    let y = if drag_down {
        jmax(-0.3, v.y - 0.03)
    } else {
        jmin(0.7, v.y + 0.06)
    };
    p.vel = Vec3::new(v.x, y, v.z);
    p.fall_distance = 0.0;
}

/// `BubbleColumnBlock.entityInside` for the bubble column at `(x, y, z)` (a no-op unless `precise`,
/// the game's flag for "the box really intersects the block"): at the top of the column — the block
/// above has neither a collision shape nor fluid — it is the "above" effect, otherwise the
/// "inside" one. The drag direction is the block's `drag` property.
pub fn bubble_column_entity_inside(
    p: &mut PlayerState,
    world: &World,
    x: i32,
    y: i32,
    z: i32,
    precise: bool,
) {
    if !precise {
        return;
    }
    let state = world.block_state(x, y, z);
    let drag_down = ms_data::property(state, "drag") == Some("true");
    let above = world.block_state(x, y + 1, z);
    let open_above = ms_data::collision_boxes(above).is_empty() && ms_data::fluid(above).is_empty();
    if open_above {
        on_above_bubble_column(p, drag_down);
    } else {
        on_inside_bubble_column(p, drag_down);
    }
}

/// `Entity.lavaIgnite` (the `LAVA_IGNITE` inside-block effect of lava): set on fire for 15
/// seconds if that is more than the remaining time, and clear the freeze counter. (`lavaHurt`'s
/// damage is server-side.)
pub fn lava_ignite(p: &mut PlayerState) {
    if p.remaining_fire_ticks < 300 {
        p.remaining_fire_ticks = 300;
    }
    clear_freeze(p);
}

/// `Entity.clearFire` (the `EXTINGUISH` effect of water): `min(0, remaining)`.
pub fn clear_fire(p: &mut PlayerState) {
    p.remaining_fire_ticks = p.remaining_fire_ticks.min(0);
}

/// `Entity.clearFreeze` (the `CLEAR_FREEZE` effect of lava).
pub fn clear_freeze(p: &mut PlayerState) {
    p.ticks_frozen = 0;
}

// ---------------------------------------------------------------------------------------------
// Block property tables (from the reference's block registrations)
// ---------------------------------------------------------------------------------------------

/// Blocks registered with `forceSolidOn()` (directly or through a copy of such a block's
/// properties), sorted. `BlockState.isSolid` is true for these regardless of their shape.
#[rustfmt::skip]
const FORCE_SOLID_ON: &[&str] = &[
    "acacia_fence_gate", "acacia_hanging_sign", "acacia_pressure_plate", "acacia_sign",
    "acacia_wall_hanging_sign", "acacia_wall_sign", "amethyst_cluster", "andesite_wall",
    "bamboo", "bamboo_fence_gate", "bamboo_hanging_sign", "bamboo_pressure_plate",
    "bamboo_sapling", "bamboo_sign", "bamboo_wall_hanging_sign", "bamboo_wall_sign", "bell",
    "birch_fence_gate", "birch_hanging_sign", "birch_pressure_plate", "birch_sign",
    "birch_wall_hanging_sign", "birch_wall_sign", "black_banner", "black_candle_cake",
    "black_wall_banner", "blackstone_wall", "blue_banner", "blue_candle_cake",
    "blue_wall_banner", "brick_wall", "brown_banner", "brown_candle_cake", "brown_wall_banner",
    "cake", "candle_cake", "cherry_fence_gate", "cherry_hanging_sign", "cherry_pressure_plate",
    "cherry_sign", "cherry_wall_hanging_sign", "cherry_wall_sign", "cobbled_deepslate_wall",
    "cobblestone_wall", "cobweb", "conduit", "copper_chain", "copper_lantern",
    "crimson_fence_gate", "crimson_hanging_sign", "crimson_pressure_plate", "crimson_sign",
    "crimson_wall_hanging_sign", "crimson_wall_sign", "cyan_banner", "cyan_candle_cake",
    "cyan_wall_banner", "dark_oak_fence_gate", "dark_oak_hanging_sign",
    "dark_oak_pressure_plate", "dark_oak_sign", "dark_oak_wall_hanging_sign",
    "dark_oak_wall_sign", "dead_brain_coral", "dead_brain_coral_block", "dead_brain_coral_fan",
    "dead_brain_coral_wall_fan", "dead_bubble_coral", "dead_bubble_coral_block",
    "dead_bubble_coral_fan", "dead_bubble_coral_wall_fan", "dead_fire_coral",
    "dead_fire_coral_block", "dead_fire_coral_fan", "dead_fire_coral_wall_fan",
    "dead_horn_coral", "dead_horn_coral_block", "dead_horn_coral_fan",
    "dead_horn_coral_wall_fan", "dead_tube_coral", "dead_tube_coral_block",
    "dead_tube_coral_fan", "dead_tube_coral_wall_fan", "deepslate_brick_wall",
    "deepslate_tile_wall", "diorite_wall", "dried_ghast", "end_stone_brick_wall",
    "exposed_copper_chain", "exposed_copper_lantern", "exposed_lightning_rod", "firefly_bush",
    "granite_wall", "gray_banner", "gray_candle_cake", "gray_wall_banner", "green_banner",
    "green_candle_cake", "green_wall_banner", "heavy_weighted_pressure_plate", "iron_chain",
    "jungle_fence_gate", "jungle_hanging_sign", "jungle_pressure_plate", "jungle_sign",
    "jungle_wall_hanging_sign", "jungle_wall_sign", "lantern", "large_amethyst_bud",
    "light_blue_banner", "light_blue_candle_cake", "light_blue_wall_banner",
    "light_gray_banner", "light_gray_candle_cake", "light_gray_wall_banner",
    "light_weighted_pressure_plate", "lightning_rod", "lime_banner", "lime_candle_cake",
    "lime_wall_banner", "magenta_banner", "magenta_candle_cake", "magenta_wall_banner",
    "mangrove_fence_gate", "mangrove_hanging_sign", "mangrove_pressure_plate", "mangrove_sign",
    "mangrove_wall_hanging_sign", "mangrove_wall_sign", "medium_amethyst_bud",
    "mossy_cobblestone_wall", "mossy_stone_brick_wall", "moving_piston", "mud_brick_wall",
    "nether_brick_wall", "oak_fence", "oak_fence_gate", "oak_hanging_sign",
    "oak_pressure_plate", "oak_sign", "oak_wall_hanging_sign", "oak_wall_sign", "orange_banner",
    "orange_candle_cake", "orange_wall_banner", "oxidized_copper_chain",
    "oxidized_copper_lantern", "oxidized_lightning_rod", "pale_oak_fence_gate",
    "pale_oak_hanging_sign", "pale_oak_pressure_plate", "pale_oak_sign",
    "pale_oak_wall_hanging_sign", "pale_oak_wall_sign", "pink_banner", "pink_candle_cake",
    "pink_wall_banner", "pointed_dripstone", "polished_blackstone_brick_wall",
    "polished_blackstone_pressure_plate", "polished_blackstone_wall", "polished_deepslate_wall",
    "polished_tuff_wall", "prismarine_wall", "purple_banner", "purple_candle_cake",
    "purple_wall_banner", "red_banner", "red_candle_cake", "red_nether_brick_wall",
    "red_sandstone_wall", "red_wall_banner", "sandstone_wall", "sculk_vein",
    "small_amethyst_bud", "soul_lantern", "spruce_fence_gate", "spruce_hanging_sign",
    "spruce_pressure_plate", "spruce_sign", "spruce_wall_hanging_sign", "spruce_wall_sign",
    "stone_brick_wall", "stone_pressure_plate", "tuff_brick_wall", "tuff_wall", "turtle_egg",
    "warped_fence_gate", "warped_hanging_sign", "warped_pressure_plate", "warped_sign",
    "warped_wall_hanging_sign", "warped_wall_sign", "waxed_copper_chain",
    "waxed_copper_lantern", "waxed_exposed_copper_chain", "waxed_exposed_copper_lantern",
    "waxed_exposed_lightning_rod", "waxed_lightning_rod", "waxed_oxidized_copper_chain",
    "waxed_oxidized_copper_lantern", "waxed_oxidized_lightning_rod",
    "waxed_weathered_copper_chain", "waxed_weathered_copper_lantern",
    "waxed_weathered_lightning_rod", "weathered_copper_chain", "weathered_copper_lantern",
    "weathered_lightning_rod", "white_banner", "white_candle_cake", "white_wall_banner",
    "yellow_banner", "yellow_candle_cake", "yellow_wall_banner",
];

/// Blocks registered with `forceSolidOff()`, sorted.
#[rustfmt::skip]
const FORCE_SOLID_OFF: &[&str] = &[
    "azalea", "big_dripleaf", "chorus_flower", "chorus_plant", "end_rod", "flowering_azalea",
    "ladder", "snow",
];

/// Blocks registered with `dynamicShape()`, sorted: they keep no cached shape, so they are not
/// "solid" unless forced.
#[rustfmt::skip]
const DYNAMIC_SHAPE: &[&str] = &[
    "bamboo", "firefly_bush", "moving_piston", "pointed_dripstone", "powder_snow",
    "scaffolding",
];

#[cfg(test)]
mod tests {
    use super::*;
    use ms_world::{FlatWorld, GridWorld};

    fn state(s: &str) -> u32 {
        ms_data::parse_state(s).unwrap_or_else(|| panic!("unknown state {s}"))
    }

    /// A void world with a stone floor top at y = 0 and `blocks` placed on it.
    fn world(blocks: &[((i32, i32, i32), &str)]) -> World {
        let mut g = GridWorld::new(FlatWorld::new(0, state("minecraft:stone")));
        for &((x, y, z), s) in blocks {
            g.set_block(x, y, z, state(s));
        }
        World::grid(g)
    }

    fn player_at(x: f64, y: f64, z: f64) -> PlayerState {
        PlayerState::new(Vec3::new(x, y, z), 0.0)
    }

    #[test]
    fn property_tables_are_sorted_and_name_real_blocks() {
        for table in [FORCE_SOLID_ON, FORCE_SOLID_OFF, DYNAMIC_SHAPE] {
            assert!(
                table.windows(2).all(|w| w[0] < w[1]),
                "table must be sorted"
            );
            for name in table {
                assert!(
                    ms_data::block_index(&format!("minecraft:{name}")).is_some(),
                    "unknown block {name}"
                );
            }
        }
    }

    #[test]
    fn blocks_motion_follows_shape_and_overrides() {
        let yes = |s: &str| blocks_motion(state(s));
        assert!(yes("minecraft:stone"));
        assert!(yes("minecraft:stone_slab[type=bottom]"));
        assert!(yes("minecraft:oak_fence"));
        // Shape-based: a carpet is too flat.
        assert!(!yes("minecraft:white_carpet"));
        assert!(!yes("minecraft:air"));
        assert!(!yes("minecraft:water"));
        // Forced on although there is no collision shape; forced off although there is one.
        assert!(yes("minecraft:stone_pressure_plate"));
        assert!(yes("minecraft:oak_sign"));
        assert!(!yes("minecraft:ladder"));
        // Never: cobweb and bamboo sapling, and unshaped dynamic blocks.
        assert!(!yes("minecraft:cobweb"));
        assert!(!yes("minecraft:bamboo_sapling"));
        assert!(!yes("minecraft:powder_snow"));
    }

    #[test]
    fn face_full_covers_the_unit_square() {
        let full = [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]];
        for d in [Dir::North, Dir::East, Dir::South, Dir::West, Dir::Up] {
            assert!(face_full(&full, d));
        }
        // A bottom slab: its sides are half-height, its top stops short of the cell's top.
        let slab = [[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]];
        assert!(!face_full(&slab, Dir::North));
        assert!(!face_full(&slab, Dir::Up));
        // Two boxes that together cover a side.
        let halves = [
            [0.0, 0.0, 0.0, 1.0, 0.5, 1.0],
            [0.0, 0.5, 0.0, 1.0, 1.0, 1.0],
        ];
        assert!(face_full(&halves, Dir::West));
        // A fence post covers only the middle.
        let post = [[0.375, 0.0, 0.375, 0.625, 1.5, 0.625]];
        assert!(!face_full(&post, Dir::East));
        assert!(!face_full(&[], Dir::East));
    }

    #[test]
    fn fluid_heights_use_the_same_fluid_above_rule() {
        let w = world(&[
            ((0, 1, 0), "minecraft:water[level=0]"),
            ((0, 2, 0), "minecraft:water[level=0]"),
            ((5, 1, 0), "minecraft:water[level=3]"),
            ((7, 1, 0), "minecraft:lava[level=0]"),
            ((7, 2, 0), "minecraft:water[level=0]"),
        ]);
        let f = |x, y| {
            let fl = fluid_at(&w, x, y, 0);
            fluid_height_at(&w, x, y, 0, fl)
        };
        assert_eq!(f(0, 1), 1.0); // water under water
        assert_eq!(f(0, 2), 8.0 / 9.0); // top of a source
        assert_eq!(f(5, 1), 5.0 / 9.0); // flowing level 3 has amount 5
        assert_eq!(f(7, 1), 8.0 / 9.0); // lava under water is not "the same fluid"
    }

    #[test]
    fn flow_points_downhill_and_is_zero_on_flat_water() {
        // A source at (0,1,0) with a lower flowing block to its east at (1,1,0).
        let w = world(&[
            ((0, 1, 0), "minecraft:water[level=0]"),
            ((1, 1, 0), "minecraft:water[level=4]"),
        ]);
        let f = fluid_at(&w, 0, 1, 0);
        let v = get_flow(&w, 0, 1, 0, f);
        // The neighbour is lower: the flow from the source heads east, as a unit vector.
        assert_eq!(v, Vec3::new(1.0, 0.0, 0.0));
        // Equal-height neighbours cancel.
        let w = world(&[
            ((0, 1, 0), "minecraft:water[level=0]"),
            ((1, 1, 0), "minecraft:water[level=0]"),
            ((-1, 1, 0), "minecraft:water[level=0]"),
        ]);
        let f = fluid_at(&w, 0, 1, 0);
        assert_eq!(get_flow(&w, 0, 1, 0, f), Vec3::ZERO);
    }

    #[test]
    fn falling_water_beside_a_wall_pulls_straight_down() {
        let w = world(&[
            ((0, 1, 0), "minecraft:water[level=8]"),
            ((1, 1, 0), "minecraft:stone"),
        ]);
        let f = fluid_at(&w, 0, 1, 0);
        assert!(f.falling);
        assert_eq!(get_flow(&w, 0, 1, 0, f), Vec3::new(0.0, -1.0, 0.0));
        // Without the wall there is no solid side to pin it.
        let w = world(&[((0, 1, 0), "minecraft:water[level=8]")]);
        let f = fluid_at(&w, 0, 1, 0);
        assert_eq!(get_flow(&w, 0, 1, 0, f), Vec3::ZERO);
    }

    #[test]
    fn standing_in_still_water_sets_state_and_resets_the_fall() {
        // A 3x3 pond, 2 deep, on the floor.
        let mut blocks = Vec::new();
        for x in -1..=1 {
            for z in -1..=1 {
                blocks.push(((x, 0, z), "minecraft:water[level=0]"));
                blocks.push(((x, 1, z), "minecraft:water[level=0]"));
            }
        }
        let w = world(&blocks);
        let mut p = player_at(0.5, 0.0, 0.5);
        p.fall_distance = 5.0;
        update_in_fluid_state_and_push(&mut p, &w);
        assert!(p.in_water && !p.in_lava);
        assert_eq!(p.fall_distance, 0.0);
        // Surface at 1 + 8/9 (top water block), box bottom at 0.001 inside.
        assert_eq!(p.water_height, f64::from(1.0_f32 + 8.0 / 9.0) - 0.001);
        assert_eq!(p.lava_height, 0.0);
        assert_eq!(p.vel, Vec3::ZERO, "still water does not push");
    }

    #[test]
    fn eye_flag_lags_one_tick_and_swimming_needs_sprint_and_eyes() {
        let w = world(&[
            ((0, 0, 0), "minecraft:water[level=0]"),
            ((0, 1, 0), "minecraft:water[level=0]"),
            ((0, 2, 0), "minecraft:water[level=0]"),
        ]);
        let mut p = player_at(0.5, 0.0, 0.5);
        // First tick: the eyes (y = 1.62) are in water, but the flag reads the previous result.
        update_fluid_on_eyes(&mut p, &w);
        assert!(!p.eye_in_water && p.water_on_eyes);
        update_fluid_on_eyes(&mut p, &w);
        assert!(p.eye_in_water);
        // Swimming starts only while sprinting under water in a water block.
        p.in_water = true;
        update_swimming(&mut p, &w);
        assert!(!p.swimming);
        p.sprinting = true;
        update_swimming(&mut p, &w);
        assert!(p.swimming);
        // A swimmer keeps swimming while sprinting in water, even with the eyes out.
        p.eye_in_water = false;
        update_swimming(&mut p, &w);
        assert!(p.swimming);
        p.in_water = false;
        update_swimming(&mut p, &w);
        assert!(!p.swimming);
        // Flying cancels it.
        p.in_water = true;
        p.eye_in_water = true;
        update_swimming(&mut p, &w);
        assert!(p.swimming);
        p.flying = true;
        update_swimming(&mut p, &w);
        assert!(!p.swimming);
    }

    #[test]
    fn jump_threshold_depends_on_eye_height() {
        let mut p = player_at(0.0, 0.0, 0.0);
        assert_eq!(fluid_jump_threshold(&p), 0.4);
        p.pose = crate::state::Pose::Swimming;
        // The swimming eye height is 0.4F, which as a double exceeds 0.4: still the full threshold.
        assert_eq!(fluid_jump_threshold(&p), 0.4);
        p.pose = crate::state::Pose::Dying;
        assert_eq!(fluid_jump_threshold(&p), 0.0);
    }

    #[test]
    fn sinking_and_swimming_nudges_are_float_constants() {
        let mut p = player_at(0.0, 0.0, 0.0);
        go_down_in_water(&mut p);
        assert_eq!(p.vel.y, f64::from(-0.04_f32));
        jump_in_liquid(&mut p);
        assert_eq!(p.vel.y, f64::from(-0.04_f32) + f64::from(0.04_f32));
    }

    #[test]
    fn bubble_columns_clamp_the_vertical_speed() {
        let mut p = player_at(0.0, 0.0, 0.0);
        p.fall_distance = 3.0;
        // Upward column, top of the column: toward 1.8.
        on_above_bubble_column(&mut p, false);
        assert_eq!(p.vel.y, 0.1);
        p.vel.y = 1.75;
        on_above_bubble_column(&mut p, false);
        assert_eq!(p.vel.y, 1.8);
        // Inside: toward 0.7, and the fall distance resets.
        p.vel.y = 0.68;
        on_inside_bubble_column(&mut p, false);
        assert_eq!(p.vel.y, 0.7);
        assert_eq!(p.fall_distance, 0.0);
        // Downward column.
        p.vel.y = -0.29;
        on_inside_bubble_column(&mut p, true);
        assert_eq!(p.vel.y, -0.3);
        p.vel.y = -0.88;
        on_above_bubble_column(&mut p, true);
        assert_eq!(p.vel.y, -0.9);
        // Flying players are not moved.
        p.flying = true;
        p.vel.y = 0.0;
        on_above_bubble_column(&mut p, false);
        assert_eq!(p.vel.y, 0.0);
    }

    #[test]
    fn bubble_column_block_picks_above_or_inside() {
        let w = world(&[
            ((0, 1, 0), "minecraft:bubble_column[drag=false]"),
            ((0, 2, 0), "minecraft:bubble_column[drag=false]"),
            ((0, 3, 0), "minecraft:water[level=0]"),
        ]);
        let mut p = player_at(0.5, 1.0, 0.5);
        // Open water above the lower block: "inside" (+0.06).
        bubble_column_entity_inside(&mut p, &w, 0, 1, 0, true);
        assert_eq!(p.vel.y, 0.06);
        // Imprecise contact does nothing.
        let mut q = player_at(0.5, 1.0, 0.5);
        bubble_column_entity_inside(&mut q, &w, 0, 1, 0, false);
        assert_eq!(q.vel.y, 0.0);
        // Nothing above the top block of a column: "above" (+0.1).
        let w = world(&[((0, 1, 0), "minecraft:bubble_column[drag=false]")]);
        let mut p = player_at(0.5, 1.0, 0.5);
        bubble_column_entity_inside(&mut p, &w, 0, 1, 0, true);
        assert_eq!(p.vel.y, 0.1);
    }

    #[test]
    fn travel_in_still_water_applies_drag_and_the_sinking_adjustment() {
        // A deep pool around the player; `move_fn` that moves nothing.
        let mut blocks = Vec::new();
        for x in -2..=2 {
            for y in 1..=4 {
                for z in -2..=2 {
                    blocks.push(((x, y, z), "minecraft:water[level=0]"));
                }
            }
        }
        let w = world(&blocks);
        let mut p = player_at(0.5, 2.0, 0.5);
        update_in_fluid_state_and_push(&mut p, &w);
        assert!(p.in_water);
        p.vel = Vec3::new(0.0, 0.0, 0.0);
        travel_in_fluid(&mut p, &w, Vec3::ZERO, &mut |_, _| {});
        // y: 0 * 0.8F, then eased toward -gravity/16: -0.005.
        assert_eq!(p.vel, Vec3::new(0.0, -0.005, 0.0));
        // Sprinting skips the adjustment and drags less horizontally.
        let mut p = player_at(0.5, 2.0, 0.5);
        p.in_water = true;
        p.sprinting = true;
        p.vel = Vec3::new(1.0, 1.0, 1.0);
        travel_in_fluid(&mut p, &w, Vec3::ZERO, &mut |_, _| {});
        assert_eq!(
            p.vel,
            Vec3::new(f64::from(0.9_f32), f64::from(0.8_f32), f64::from(0.9_f32))
        );
    }

    #[test]
    fn lava_travel_halves_and_sinks() {
        let mut p = player_at(0.5, 2.0, 0.5);
        p.in_lava = true;
        p.lava_height = 1.0; // above the 0.4 jump threshold: the deep-lava branch
        p.vel = Vec3::new(1.0, 1.0, 1.0);
        travel_in_fluid(&mut p, &world(&[]), Vec3::ZERO, &mut |_, _| {});
        assert_eq!(p.vel, Vec3::new(0.5, 0.5 + -0.08 / 4.0, 0.5));
    }

    #[test]
    fn clip_finds_water_surfaces_and_ignores_air() {
        let w = world(&[
            ((0, 1, 0), "minecraft:water[level=0]"),
            ((0, 2, 0), "minecraft:water[level=0]"),
            ((4, 1, 0), "minecraft:cobweb"),
        ]);
        // Falling from y=6 through the pool.
        assert!(fall_damage_resetting_clip_hits(
            &w,
            Vec3::new(0.5, 6.0, 0.5),
            Vec3::new(0.5, 0.5, 0.5)
        ));
        // Falling beside it.
        assert!(!fall_damage_resetting_clip_hits(
            &w,
            Vec3::new(2.5, 6.0, 0.5),
            Vec3::new(2.5, 1.5, 0.5)
        ));
        // Stopping above the surface (the top block holds 8/9 of a block).
        assert!(!fall_damage_resetting_clip_hits(
            &w,
            Vec3::new(0.5, 6.0, 0.5),
            Vec3::new(0.5, 3.5, 0.5)
        ));
        // Cobweb counts as a full block.
        assert!(fall_damage_resetting_clip_hits(
            &w,
            Vec3::new(4.5, 4.0, 0.5),
            Vec3::new(4.5, 1.5, 0.5)
        ));
    }

    #[test]
    fn push_reads_the_flow_and_applies_the_minimum_rule() {
        // Water that flows east: a source and a lower neighbour; the player in the source block.
        let w = world(&[
            ((0, 1, 0), "minecraft:water[level=0]"),
            ((1, 1, 0), "minecraft:water[level=5]"),
        ]);
        let mut p = player_at(0.5, 1.0, 0.5);
        update_in_fluid_state_and_push(&mut p, &w);
        assert!(p.in_water);
        // Deep in the block: the unit flow, scaled by 0.014.
        assert_eq!(p.vel, Vec3::new(0.014, 0.0, 0.0));
        // Only the top of the fluid touches the feet: the flow is scaled by the depth (0.2879..)
        // and, being under 0.0045 for a player at rest, raised to exactly that length.
        let mut p = player_at(0.5, 1.6, 0.5);
        update_in_fluid_state_and_push(&mut p, &w);
        assert!(p.in_water);
        assert_eq!(p.vel, Vec3::new(0.0045000000000000005, 0.0, 0.0));
        // Already moving horizontally: the plain scaled flow.
        let mut p = player_at(0.5, 1.6, 0.5);
        p.vel = Vec3::new(0.0, 0.0, 0.01);
        update_in_fluid_state_and_push(&mut p, &w);
        assert!(p.vel.x < 0.0045 && p.vel.x > 0.004 && p.vel.z == 0.01);
        // A flying player is not pushed.
        let mut p = player_at(0.5, 1.0, 0.5);
        p.flying = true;
        update_in_fluid_state_and_push(&mut p, &w);
        assert!(p.in_water);
        assert_eq!(p.vel, Vec3::ZERO);
    }
}
