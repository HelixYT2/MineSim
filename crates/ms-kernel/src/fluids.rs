//! Fluids: the water/lava heights the player's box is submerged to, the flow push, the eye-in-water
//! test, the swimming state, bubble columns, and the in-fluid travel step.
//!
//! Owned by the fluid port. The kernel's tick calls these at the points the game does.

use crate::state::PlayerState;
use ms_numerics::Vec3;
use ms_world::World;

/// `Entity.updateInWaterStateAndDoFluidPushing` (with `updateFluidHeightAndDoFluidPushing` for
/// water then lava): refresh `in_water`, `in_lava`, `water_height`, `lava_height`, apply the
/// current's push to `vel`, and the fall-distance/fire side effects of entering water.
pub fn update_in_fluid_state_and_push(p: &mut PlayerState, world: &World) {
    let _ = (p, world);
}

/// `Entity.updateFluidOnEyes`: whether the eyes are in water (`eye_in_water` as `isUnderWater`).
pub fn update_fluid_on_eyes(p: &mut PlayerState, world: &World) {
    let _ = (p, world);
}

/// `Entity.updateSwimming` (the player's override included).
pub fn update_swimming(p: &mut PlayerState, world: &World) {
    let _ = (p, world);
}

/// `Entity.getFluidJumpThreshold`.
pub fn fluid_jump_threshold(p: &PlayerState) -> f64 {
    if p.eye_height() < 0.4 {
        0.0
    } else {
        0.4
    }
}

/// `LivingEntity.travelInFluid` (water and lava branches, including `jumpOutOfFluid` and the
/// falling adjustment). `input` is `(xxa, yya, zza)`.
pub fn travel_in_fluid(p: &mut PlayerState, world: &World, input: Vec3) {
    let _ = (p, world, input);
}

/// The fluid state at the player's block position decides whether `travel` uses the fluid
/// branch (`LivingEntity.shouldTravelInFluid`).
pub fn should_travel_in_fluid(p: &PlayerState, world: &World) -> bool {
    let _ = world;
    (p.in_water || p.in_lava) && !p.flying
}
