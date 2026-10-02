//! The `LivingEntity` layer of the tick: `baseTick`, `aiStep` (velocity rounding, input, jumping),
//! `travel` through air, and the helpers they use (friction-scaled `moveRelative`, climbing,
//! effective gravity, the flying speed).
//!
//! The player-specific `Player.travel` wrapper (the swimming-pose pitch push) lives here too, since
//! it is a thin layer over `LivingEntity.travel`. Fluid travel itself is the fluid module's
//! ([`crate::fluids::travel_in_fluid`]); it moves the player with [`crate::entity::move_entity`]
//! and pushes with [`move_relative`].

// The comparisons mirror the reference's `!(a <= b)` forms, which differ from `a > b` for NaN.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::attributes::Attribute;
use crate::collision::{floor, jmax};
use crate::entity::{
    block_friction_at, block_jump_factor, block_pos_below_that_affects_my_movement, move_entity,
    on_pos_legacy, reset_fall_distance, set_vel,
};
use crate::input::DEG_TO_RAD;
use crate::state::{Input, PlayerState, Pose};
use ms_numerics::Vec3;
use ms_world::World;

/// `LivingEntity.isImmobile` (`isDeadOrDying`; sleeping is not modelled).
#[inline]
pub fn is_immobile(p: &PlayerState) -> bool {
    p.health <= 0.0
}

/// `Player.isAffectedByFluids`.
#[inline]
pub fn is_affected_by_fluids(p: &PlayerState) -> bool {
    !p.flying
}

/// `Player.isSwimming`: the swimming flag, unless flying.
#[inline]
pub fn is_swimming(p: &PlayerState) -> bool {
    !p.flying && p.swimming
}

/// `LocalPlayer.isUnderWater`: whether the eyes were in water (`Player.wasUnderwater`).
#[inline]
pub fn is_under_water(p: &PlayerState) -> bool {
    p.eye_in_water
}

/// `Entity.isInShallowWater`: in water with the eyes above it.
#[inline]
pub fn is_in_shallow_water(p: &PlayerState) -> bool {
    p.in_water && !is_under_water(p)
}

/// `Entity.isVisuallyCrawling`: in the swimming pose without being in water.
#[inline]
pub fn is_visually_crawling(p: &PlayerState) -> bool {
    p.pose == Pose::Swimming && !p.in_water
}

/// `LocalPlayer.isMovingSlowly`.
#[inline]
pub fn is_moving_slowly(p: &PlayerState) -> bool {
    p.crouching || is_visually_crawling(p)
}

/// `LocalPlayer.applyInput` (the controlled-camera branch): the movement impulse the keys give,
/// and the jump flag.
pub fn apply_input(p: &mut PlayerState, input: &Input, move_vector: (f32, f32)) {
    let sneaking_speed = p.attributes.value(Attribute::SneakingSpeed) as f32;
    let (xxa, zza) = crate::input::modify_input(move_vector, is_moving_slowly(p), sneaking_speed);
    p.xxa = xxa;
    p.zza = zza;
    p.jumping = input.jump;
}

/// `LivingEntity.getJumpPower(1.0F)`: jump strength × block jump factor + jump boost, in float.
pub fn jump_power(p: &PlayerState, world: &World) -> f32 {
    let strength = p.attributes.value(Attribute::JumpStrength) as f32;
    strength * 1.0_f32 * block_jump_factor(p, world) + crate::effects::jump_boost_power(p)
}

/// `LivingEntity.jumpFromGround`.
pub fn jump_from_ground(p: &mut PlayerState, world: &World) {
    let f = jump_power(p, world);
    if !(f <= 1.0E-5_f32) {
        let v = p.vel;
        set_vel(p, Vec3::new(v.x, jmax(f64::from(f), v.y), v.z));
        if p.sprinting {
            let g = p.yaw * DEG_TO_RAD;
            let v = p.vel;
            set_vel(
                p,
                Vec3::new(
                    v.x + (-f64::from(crate::mth::sin(f64::from(g))) * 0.2),
                    v.y + 0.0,
                    v.z + f64::from(crate::mth::cos(f64::from(g))) * 0.2,
                ),
            );
        }
    }
}

/// `LivingEntity.jumpInLiquid`: swim up.
pub fn jump_in_liquid(p: &mut PlayerState) {
    let v = p.vel;
    set_vel(
        p,
        Vec3::new(v.x + 0.0, v.y + f64::from(0.04_f32), v.z + 0.0),
    );
}

/// `LivingEntity.goDownInWater`: sink.
pub fn go_down_in_water(p: &mut PlayerState) {
    let v = p.vel;
    set_vel(
        p,
        Vec3::new(v.x + 0.0, v.y + f64::from(-0.04_f32), v.z + 0.0),
    );
}

/// `Entity.getGravity` for a living entity: the gravity attribute.
#[inline]
pub fn gravity(p: &PlayerState) -> f64 {
    p.attributes.value(Attribute::Gravity)
}

/// `LivingEntity.getEffectiveGravity`: slow falling caps gravity at 0.01 while not rising.
pub fn effective_gravity(p: &PlayerState) -> f64 {
    let falling = p.vel.y <= 0.0;
    if falling && p.effects.has("minecraft:slow_falling") {
        crate::collision::jmin(gravity(p), 0.01)
    } else {
        gravity(p)
    }
}

/// `Player.getFlyingSpeed`: the air-control speed (the abilities' flying speed is the vanilla
/// default 0.05 when flying).
pub fn flying_speed(p: &PlayerState) -> f32 {
    if p.flying {
        if p.sprinting {
            0.05_f32 * 2.0_f32
        } else {
            0.05_f32
        }
    } else if p.sprinting {
        0.025_999_999_f32
    } else {
        0.02_f32
    }
}

/// `LivingEntity.getSpeed` for the player: the movement-speed attribute as a float.
#[inline]
pub fn speed(p: &PlayerState) -> f32 {
    p.attributes.value(Attribute::MovementSpeed) as f32
}

/// `Entity.getInputVector`: `input` scaled to `speed` (normalised first when longer than 1) and
/// rotated by yaw.
pub fn input_vector(input: Vec3, speed: f32, yaw: f32) -> Vec3 {
    let d = input.x * input.x + input.y * input.y + input.z * input.z;
    if d < 1.0E-7 {
        return Vec3::ZERO;
    }
    let s = f64::from(speed);
    let scaled = if d > 1.0 {
        // Vec3.normalize
        let len = (input.x * input.x + input.y * input.y + input.z * input.z).sqrt();
        let n = if len < f64::from(1.0E-5_f32) {
            Vec3::ZERO
        } else {
            Vec3::new(input.x / len, input.y / len, input.z / len)
        };
        Vec3::new(n.x * s, n.y * s, n.z * s)
    } else {
        Vec3::new(input.x * s, input.y * s, input.z * s)
    };
    let h = f64::from(crate::mth::sin(f64::from(yaw * DEG_TO_RAD)));
    let i = f64::from(crate::mth::cos(f64::from(yaw * DEG_TO_RAD)));
    Vec3::new(
        scaled.x * i - scaled.z * h,
        scaled.y,
        scaled.z * i + scaled.x * h,
    )
}

/// `Entity.moveRelative`: add the rotated input to the velocity.
pub fn move_relative(p: &mut PlayerState, speed: f32, input: Vec3) {
    let v = input_vector(input, speed, p.yaw);
    let cur = p.vel;
    set_vel(p, Vec3::new(cur.x + v.x, cur.y + v.y, cur.z + v.z));
}

/// `LivingEntity.getFrictionInfluencedSpeed`.
fn friction_influenced_speed(p: &PlayerState, f: f32) -> f32 {
    if p.on_ground {
        speed(p) * (0.216_000_02_f32 / (f * f * f))
    } else {
        flying_speed(p)
    }
}

/// `Mth.clamp(double, double, double)`.
#[inline]
fn clamp(d: f64, lo: f64, hi: f64) -> f64 {
    if d < lo {
        lo
    } else {
        crate::collision::jmin(d, hi)
    }
}

/// `LivingEntity.handleOnClimbable`: on a ladder or vine, cap the velocity, cancel the fall, and
/// hold on while sneaking.
fn handle_on_climbable(p: &mut PlayerState, world: &World, v: Vec3) -> Vec3 {
    if crate::blocks::on_climbable(p, world) {
        reset_fall_distance(p);
        let lim = f64::from(0.15_f32);
        let d = clamp(v.x, -lim, lim);
        let e = clamp(v.z, -lim, lim);
        let mut g = jmax(v.y, -lim);
        if g < 0.0 && !in_block_is_scaffolding(p, world) && is_suppressing_sliding_down_ladder(p) {
            g = 0.0;
        }
        Vec3::new(d, g, e)
    } else {
        v
    }
}

/// `getInBlockState().is(Blocks.SCAFFOLDING)`.
fn in_block_is_scaffolding(p: &PlayerState, world: &World) -> bool {
    let (x, y, z) = crate::entity::block_position(p);
    ms_data::block_name(world.block(x, y, z)) == "minecraft:scaffolding"
}

/// `LocalPlayer.isSuppressingSlidingDownLadder`: sneaking while not flying.
#[inline]
fn is_suppressing_sliding_down_ladder(p: &PlayerState) -> bool {
    !p.flying && p.shift_key_down
}

/// `LivingEntity.handleRelativeFrictionAndCalculateMovement`.
fn handle_relative_friction_and_calculate_movement(
    p: &mut PlayerState,
    world: &World,
    input: Vec3,
    friction: f32,
) -> Vec3 {
    let s = friction_influenced_speed(p, friction);
    move_relative(p, s, input);
    let v = handle_on_climbable(p, world, p.vel);
    set_vel(p, v);
    let motion = p.vel;
    move_entity(p, world, motion);
    let mut out = p.vel;
    // (this.horizontalCollision || this.jumping) && (this.onClimbable() || wasInPowderSnow &&
    // PowderSnowBlock.canEntityWalkOnPowderSnow(this)): the player has no leather boots here.
    if (p.horizontal_collision || p.jumping) && crate::blocks::on_climbable(p, world) {
        out = Vec3::new(out.x, 0.2, out.z);
    }
    out
}

/// `LivingEntity.travelInAir`.
fn travel_in_air(p: &mut PlayerState, world: &World, input: Vec3) {
    let below = block_pos_below_that_affects_my_movement(p, world);
    let f: f32 = if p.on_ground {
        block_friction_at(world, below)
    } else {
        1.0
    };
    let g = f * 0.91_f32;
    let vec32 = handle_relative_friction_and_calculate_movement(p, world, input, f);
    let mut d = vec32.y;
    if let Some(levitation) = p.effects.get("minecraft:levitation") {
        d += (0.05 * f64::from(levitation.amplifier + 1) - vec32.y) * 0.2;
    } else {
        // The client has every chunk of the arena loaded, so this is the gravity branch.
        d -= effective_gravity(p);
    }
    // shouldDiscardFriction() is false for the player.
    let h = 0.98_f32;
    set_vel(
        p,
        Vec3::new(
            vec32.x * f64::from(g),
            d * f64::from(h),
            vec32.z * f64::from(g),
        ),
    );
}

/// `LivingEntity.travel`.
fn living_travel(p: &mut PlayerState, world: &World, input: Vec3) {
    if crate::fluids::should_travel_in_fluid(p, world) {
        crate::fluids::travel_in_fluid(p, world, input);
    } else {
        // (elytra flight is not simulated)
        travel_in_air(p, world, input);
    }
}

/// `Player.travel`: while swimming, steer vertically towards the look direction; flying scales the
/// vertical velocity afterwards.
pub fn travel(p: &mut PlayerState, world: &World, input: Vec3) {
    if is_swimming(p) {
        // getLookAngle().y = -sin(xRot in radians)
        let d = f64::from(-crate::mth::sin(f64::from(p.pitch * DEG_TO_RAD)));
        let e = if d < -0.2 { 0.085 } else { 0.06 };
        let probe = (floor(p.pos.x), floor(p.pos.y + 1.0 - 0.1), floor(p.pos.z));
        let fluid = ms_data::fluid(world.block_state(probe.0, probe.1, probe.2));
        if d <= 0.0 || p.jumping || !fluid.is_empty() {
            let v = p.vel;
            set_vel(p, Vec3::new(v.x + 0.0, v.y + (d - v.y) * e, v.z + 0.0));
        }
    }
    if p.flying {
        let d = p.vel.y;
        living_travel(p, world, input);
        let v = p.vel;
        set_vel(p, Vec3::new(v.x, d * 0.6, v.z));
    } else {
        living_travel(p, world, input);
    }
}

/// `LivingEntity.aiStep` for the local player (called through `Player.aiStep`): velocity
/// rounding, the input, jumping, travel, and the effects of the blocks passed through.
///
/// `old_pos` is the position at the start of the tick (`Entity.oldPosition`), the start of the
/// block-effects movement.
pub fn ai_step(
    p: &mut PlayerState,
    world: &World,
    input: &Input,
    move_vector: (f32, f32),
    old_pos: Vec3,
) {
    if p.no_jump_delay > 0 {
        p.no_jump_delay -= 1;
    }

    // Players round a tiny horizontal speed to zero as a whole, and a tiny vertical one alone.
    let v = p.vel;
    let (mut d, mut e, mut f) = (v.x, v.y, v.z);
    if v.x * v.x + v.z * v.z < 9.0E-6 {
        d = 0.0;
        f = 0.0;
    }
    if v.y.abs() < 0.003 {
        e = 0.0;
    }
    set_vel(p, Vec3::new(d, e, f));

    apply_input(p, input, move_vector);
    if is_immobile(p) {
        p.jumping = false;
        p.xxa = 0.0;
        p.zza = 0.0;
    }

    // jump
    if p.jumping && is_affected_by_fluids(p) {
        let g = if p.in_lava {
            p.lava_height
        } else {
            p.water_height
        };
        let in_water_and_wet = p.in_water && g > 0.0;
        let h = crate::fluids::fluid_jump_threshold(p);
        if !in_water_and_wet || (p.on_ground && !(g > h)) {
            if !p.in_lava || (p.on_ground && !(g > h)) {
                if (p.on_ground || (in_water_and_wet && g <= h)) && p.no_jump_delay == 0 {
                    jump_from_ground(p, world);
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

    // travel
    let input_vec = Vec3::new(f64::from(p.xxa), 0.0, f64::from(p.zza));
    if p.effects.has("minecraft:slow_falling") || p.effects.has("minecraft:levitation") {
        reset_fall_distance(p);
    }
    travel(p, world, input_vec);

    // Entity.applyEffectsFromBlocks: the landed-on block's stepOn, then the blocks passed through.
    if p.on_ground {
        let on = on_pos_legacy(p, world);
        crate::blocks::step_on(p, world, on);
    }
    let to = p.pos;
    crate::blocks::apply_effects_from_blocks(p, world, old_pos, to);
}

/// `Entity.baseTick` + `LivingEntity.baseTick` as the client runs them for the local player.
pub fn base_tick(p: &mut PlayerState, world: &World) {
    p.was_in_powder_snow = p.in_powder_snow;
    p.in_powder_snow = false;
    crate::fluids::update_in_fluid_state_and_push(p, world);
    crate::fluids::update_fluid_on_eyes(p, world);
    crate::fluids::update_swimming(p, world);
    // On the client the fire counter never goes positive (clearFire).
    p.remaining_fire_ticks = p.remaining_fire_ticks.min(0);
    if p.in_lava {
        p.fall_distance *= 0.5;
    }
    // LivingEntity.baseTick: the hurt and invulnerability countdowns, then the effects.
    crate::damage::tick_timers(p);
    crate::effects::tick_effects(p);
}
