//! Block behaviours that act on the player: climbable blocks, entity-dependent collision shapes
//! (scaffolding, powder snow), landing effects (slime and bed bounce, fall-damage multipliers),
//! stepping effects (slime slow-down), and the effects of being inside a block (cobweb, sweet
//! berry bush, powder snow, honey wall slide, bubble columns).
//!
//! Owned by the block-behaviour port. The kernel's tick calls these at the points the game does.

use crate::state::PlayerState;
use ms_numerics::Vec3;
use ms_world::aabb::Aabb;
use ms_world::World;

/// `LivingEntity.onClimbable`.
pub fn on_climbable(p: &PlayerState, world: &World) -> bool {
    let _ = (p, world);
    false
}

/// The collision boxes of the block at `(x, y, z)` as the player sees them (scaffolding and powder
/// snow depend on the entity), in block-local coordinates. `bb` is the player's current box.
pub fn collision_boxes(
    p: &PlayerState,
    bb: Aabb,
    world: &World,
    x: i32,
    y: i32,
    z: i32,
) -> Vec<[f64; 6]> {
    let _ = (p, bb);
    ms_data::collision_boxes(world.block_state(x, y, z)).to_vec()
}

/// `Block.updateEntityMovementAfterFallOn` for the block landed on (slime and bed bounce; the
/// default zeroes the vertical velocity).
pub fn after_fall_on(p: &mut PlayerState, world: &World, landed_on: (i32, i32, i32)) {
    let _ = world;
    let _ = landed_on;
    p.vel.y = 0.0;
}

/// `Block.fallOn`: the fall-damage multiplier the landed-on block applies (`None` = no damage at
/// all, e.g. slime without sneaking), before `causeFallDamage`.
pub fn fall_damage_multiplier(
    p: &PlayerState,
    world: &World,
    landed_on: (i32, i32, i32),
) -> Option<f32> {
    let _ = (p, world, landed_on);
    Some(1.0)
}

/// `Block.stepOn` for the supporting block after a move (slime slow-down, magma damage).
pub fn step_on(p: &mut PlayerState, world: &World, on: (i32, i32, i32)) {
    let _ = (p, world, on);
}

/// `Entity.applyEffectsFromBlocks`: `entityInside` of every block the box passed through this tick
/// (from `from` to `to`), e.g. cobweb/berry-bush stuck multipliers, powder snow, honey slide,
/// bubble columns.
pub fn apply_effects_from_blocks(p: &mut PlayerState, world: &World, from: Vec3, to: Vec3) {
    let _ = (p, world, from, to);
}
