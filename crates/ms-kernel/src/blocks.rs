//! Block behaviours that act on the player: climbable blocks, entity-dependent collision shapes
//! (scaffolding, powder snow, and the position-offset shapes of bamboo and pointed dripstone),
//! landing effects (slime and bed bounce, fall-damage multipliers), stepping effects (slime
//! slow-down, magma), and the effects of being inside a block (cobweb, sweet berry bush, powder
//! snow, honey wall slide, bubble columns).
//!
//! Owned by the block-behaviour port. The kernel's tick calls these at the points the game does:
//!
//! * `LivingEntity.onClimbable` is [`on_climbable`].
//! * The collision gather asks [`collision_boxes`] for every block instead of the context-free
//!   table, because the shape of scaffolding and powder snow depends on the entity (its feet
//!   height, whether it sneaks, its fall distance) and bamboo and pointed dripstone are offset by
//!   a hash of their position.
//! * `Entity.move`, after the collision step, calls `checkFallDamage`, which on landing calls
//!   `Block.fallOn` ([`fall_on`], which reaches the damage module), and then, when the vertical
//!   motion was cut short, `Block.updateEntityMovementAfterFallOn` ([`after_fall_on`]). Both act
//!   on the block at [`on_pos`]`(p, world, 0.2)` (`getOnPosLegacy`).
//! * `Entity.move` also records every move that changed the position into a [`MovementLog`]
//!   (`Entity.addMovementThisTick`): `from` is the position before the move, `to` the position
//!   after it, and `original` the motion handed to the collision step (after the stuck
//!   multiplier and the edge back-off). The log is cleared by the consumer below.
//! * At the end of `LivingEntity.aiStep` (after `travel`), `Entity.applyEffectsFromBlocks` runs:
//!   first, when the player is on the ground, [`step_on`] with the block at `getOnPosLegacy`,
//!   then [`apply_effects_from_blocks`] with the log and the position the tick started at.
//!
//! What is deliberately not modelled (documented here so the integrator knows the boundary):
//!
//! * Fire, lava, campfire and cactus contact (`FIRE_IGNITE`, `LAVA_IGNITE`, `lavaHurt`, `inFire`,
//!   `campfire` damage), and the water fluid's `EXTINGUISH`: they need the server's RNG (ignition
//!   draws from `level.random`) and damage sources the damage module does not have yet. On the
//!   client the fire counter is cleared every tick anyway. The step-based collector below has the
//!   machinery (per-step de-duplication, enum-ordered application) and only the effects powder
//!   snow raises are wired up.
//! * Redstone-ish and world-mutating block reactions (pressure plates, buttons, tripwires, big
//!   dripleaf tilting, farmland trampling and turtle eggs on `fallOn`, sculk sensors, redstone ore
//!   on `stepOn`, portals): they change the world, not the player.
//! * Honey's `maybeDoSlideEffects` and powder snow's snowflake particles draw from the client's
//!   `level.random`, which nothing in the player's physics reads.
//! * Walking on powder snow with leather boots: the player has no equipment slots, so
//!   `canEntityWalkOnPowderSnow` is always false ([`can_walk_on_powder_snow`] is the one place to
//!   extend). `freezing via ticksFrozen`: the inside-block effect `FREEZE` raises `ticks_frozen` by
//!   one per step in which it was triggered; the decay (`-2` per tick outside powder snow), the
//!   frost overlay and the freeze damage are `LivingEntity.aiStep`'s, not this module's.

use crate::damage::{self, DamageSource};
use crate::state::{PlayerState, Pose};
use ms_numerics::Vec3;
use ms_world::aabb::Aabb;
use ms_world::World;
use std::sync::OnceLock;

// ---------------------------------------------------------------------------------------------
// Java numerics
// ---------------------------------------------------------------------------------------------

/// `Mth.floor(double)`.
fn jfloor(d: f64) -> i32 {
    let i = d as i32;
    if d < f64::from(i) {
        i.wrapping_sub(1)
    } else {
        i
    }
}

/// `Mth.lfloor(double)`.
fn jlfloor(d: f64) -> i64 {
    let l = d as i64;
    if d < l as f64 {
        l.wrapping_sub(1)
    } else {
        l
    }
}

/// `Mth.frac(double)`.
fn jfrac(d: f64) -> f64 {
    d - jlfloor(d) as f64
}

/// `Mth.sign(double)`.
fn jsign(d: f64) -> i32 {
    if d == 0.0 {
        0
    } else if d > 0.0 {
        1
    } else {
        -1
    }
}

/// `Math.min(double, double)` (NaN-propagating, `-0.0 < 0.0`).
fn jmin(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        a
    } else if a == 0.0 && b == 0.0 && b.is_sign_negative() {
        b
    } else if a <= b {
        a
    } else {
        b
    }
}

/// `Mth.clamp(double, double, double)`.
fn jclamp(d: f64, lo: f64, hi: f64) -> f64 {
    if d < lo {
        lo
    } else {
        jmin(d, hi)
    }
}

fn length_sqr(v: Vec3) -> f64 {
    v.x * v.x + v.y * v.y + v.z * v.z
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// `a.distanceToSqr(b)`: the differences are taken as `b - a`.
fn distance_sqr(a: Vec3, b: Vec3) -> f64 {
    let d = b.x - a.x;
    let e = b.y - a.y;
    let f = b.z - a.z;
    d * d + e * e + f * f
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    X,
    Y,
    Z,
}

fn axis_value(v: Vec3, axis: Axis) -> f64 {
    match axis {
        Axis::X => v.x,
        Axis::Y => v.y,
        Axis::Z => v.z,
    }
}

/// `Direction.axisStepOrder`: Y first, then whichever horizontal axis moves more (X on ties).
fn axis_step_order(v: Vec3) -> [Axis; 3] {
    if v.x.abs() < v.z.abs() {
        [Axis::Y, Axis::Z, Axis::X]
    } else {
        [Axis::Y, Axis::X, Axis::Z]
    }
}

/// `Vec3.relative(direction, d)` for the positive direction of `axis`.
fn relative(v: Vec3, axis: Axis, d: f64) -> Vec3 {
    let (sx, sy, sz) = match axis {
        Axis::X => (1.0, 0.0, 0.0),
        Axis::Y => (0.0, 1.0, 0.0),
        Axis::Z => (0.0, 0.0, 1.0),
    };
    Vec3::new(v.x + d * sx, v.y + d * sy, v.z + d * sz)
}

// ---------------------------------------------------------------------------------------------
// Block classification
// ---------------------------------------------------------------------------------------------

/// The implementing classes this module has behaviour for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Other,
    Web,
    SweetBerryBush,
    PowderSnow,
    Honey,
    BubbleColumn,
    Slime,
    Bed,
    Hay,
    Magma,
    Scaffolding,
    Bamboo,
    PointedDripstone,
    TrapDoor,
    FenceGate,
}

/// Everything the behaviours look up about a block state, resolved once per state.
#[derive(Clone, Copy)]
struct Info {
    kind: Kind,
    /// `BlockTags.CLIMBABLE`.
    climbable: bool,
    /// `BlockTags.CAN_GLIDE_THROUGH`.
    glide_through: bool,
    /// `BlockTags.FENCES`.
    fence: bool,
    /// `BlockTags.WALLS`.
    wall: bool,
}

fn classify(block: usize) -> Info {
    let kind = match ms_data::block_class(block) {
        "WebBlock" => Kind::Web,
        "SweetBerryBushBlock" => Kind::SweetBerryBush,
        "PowderSnowBlock" => Kind::PowderSnow,
        "HoneyBlock" => Kind::Honey,
        "BubbleColumnBlock" => Kind::BubbleColumn,
        "SlimeBlock" => Kind::Slime,
        "BedBlock" => Kind::Bed,
        "HayBlock" => Kind::Hay,
        "MagmaBlock" => Kind::Magma,
        "ScaffoldingBlock" => Kind::Scaffolding,
        "BambooStalkBlock" => Kind::Bamboo,
        "PointedDripstoneBlock" => Kind::PointedDripstone,
        // `instanceof TrapDoorBlock` also holds for the copper variants.
        "TrapDoorBlock" | "WeatheringCopperTrapDoorBlock" => Kind::TrapDoor,
        "FenceGateBlock" => Kind::FenceGate,
        _ => Kind::Other,
    };
    Info {
        kind,
        climbable: ms_data::block_has_tag(block, "minecraft:climbable"),
        glide_through: ms_data::block_has_tag(block, "minecraft:can_glide_through"),
        fence: ms_data::block_has_tag(block, "minecraft:fences"),
        wall: ms_data::block_has_tag(block, "minecraft:walls"),
    }
}

/// The classification of a block state (a flat table, built on first use).
fn info(state: u32) -> Info {
    static TABLE: OnceLock<Vec<Info>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let per_block: Vec<Info> = (0..ms_data::BLOCK_COUNT).map(classify).collect();
        (0..ms_data::BLOCK_STATE_COUNT)
            .map(|s| per_block[ms_data::block_of_state(s)])
            .collect()
    });
    table[state as usize]
}

/// `BlockPos.containing(pos)` / `Entity.blockPosition()`.
fn block_pos(pos: Vec3) -> (i32, i32, i32) {
    (jfloor(pos.x), jfloor(pos.y), jfloor(pos.z))
}

// ---------------------------------------------------------------------------------------------
// onClimbable
// ---------------------------------------------------------------------------------------------

/// `LivingEntity.onClimbable`: the block at the player's block position is in the climbable tag
/// (ladders, vines, scaffolding), or is an open trapdoor sitting on a ladder that faces the same
/// way. Gliding through a block of the can-glide-through tag never counts. (Spectators never
/// climb; the simulated player is never one. `lastClimbablePos` only feeds sounds.)
pub fn on_climbable(p: &PlayerState, world: &World) -> bool {
    let (x, y, z) = block_pos(p.pos);
    let state = world.block_state(x, y, z);
    let here = info(state);
    if p.pose == Pose::FallFlying && here.glide_through {
        return false;
    }
    if here.climbable {
        return true;
    }
    here.kind == Kind::TrapDoor && trapdoor_usable_as_ladder(world, x, y, z, state)
}

/// `LivingEntity.trapdoorUsableAsLadder`: open, with a ladder directly below facing the same way.
fn trapdoor_usable_as_ladder(world: &World, x: i32, y: i32, z: i32, state: u32) -> bool {
    if ms_data::property(state, "open") != Some("true") {
        return false;
    }
    let below = world.block_state(x, y - 1, z);
    ms_data::block_name(ms_data::block_of_state(below)) == "minecraft:ladder"
        && ms_data::property(below, "facing") == ms_data::property(state, "facing")
}

// ---------------------------------------------------------------------------------------------
// Entity-dependent collision shapes
// ---------------------------------------------------------------------------------------------

const FULL_BLOCK: [f64; 6] = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];

/// `EntityCollisionContext.isAbove(shape, pos, ..)` for a shape whose top is `shape_top` (block
/// local): the entity's feet are above the shape's top face (less the `1.0E-5F` tolerance).
fn is_above(p: &PlayerState, block_y: i32, shape_top: f64) -> bool {
    p.pos.y > f64::from(block_y) + shape_top - f64::from(1.0E-5_f32)
}

/// `PowderSnowBlock.canEntityWalkOnPowderSnow`: the player must wear leather boots. The simulated
/// player has no equipment, so never.
pub fn can_walk_on_powder_snow(_p: &PlayerState) -> bool {
    false
}

/// `PowderSnowBlock.getCollisionShape` for the player: a falling player (`fallDistance > 2.5`)
/// meets the low shape (0.9 high), a sneaking-free walker on boots a full block when above it,
/// everyone else nothing.
fn powder_snow_boxes(p: &PlayerState, block_y: i32) -> Vec<[f64; 6]> {
    if p.fall_distance > 2.5 {
        // Shapes.box(0, 0, 0, 1, 0.9F, 1)
        return vec![[0.0, 0.0, 0.0, 1.0, f64::from(0.9_f32), 1.0]];
    }
    if can_walk_on_powder_snow(p) && is_above(p, block_y, 1.0) && !p.shift_key_down {
        return vec![FULL_BLOCK];
    }
    Vec::new()
}

/// `ScaffoldingBlock.getCollisionShape`: the stable shape when the player's feet are above the
/// block and it is not descending (sneaking); the thin bottom plate for an unstable scaffold when
/// the feet are above the block below it; otherwise nothing (the player falls through).
fn scaffolding_boxes(p: &PlayerState, state: u32, block_y: i32) -> Vec<[f64; 6]> {
    if is_above(p, block_y, 1.0) && !p.shift_key_down {
        // The context-free table holds the stable shape (an empty context counts as above and
        // not descending).
        return ms_data::collision_boxes(state).to_vec();
    }
    let distance_nonzero = ms_data::property(state, "distance") != Some("0");
    let bottom = ms_data::property(state, "bottom") == Some("true");
    // SHAPE_BELOW_BLOCK is the unit cube moved down by one: its top face is at y = 0.
    if distance_nonzero && bottom && is_above(p, block_y, 0.0) {
        // SHAPE_UNSTABLE_BOTTOM = column(16, 0, 2)
        return vec![[0.0, 0.0, 0.0, 1.0, 0.125, 1.0]];
    }
    Vec::new()
}

/// `Mth.getSeed(x, 0, z)`.
fn position_seed(x: i32, z: i32) -> i64 {
    // `x * 3129871 ^ z * 116129781L ^ y` with y = 0: the first product wraps as an int.
    let l = (x.wrapping_mul(3129871) as i64) ^ (z as i64).wrapping_mul(116129781);
    l.wrapping_mul(l)
        .wrapping_mul(42317861)
        .wrapping_add(l.wrapping_mul(11))
        >> 16
}

/// The horizontal offset of an `OffsetType.XZ` block at `(x, z)` (`BlockStateBase.getOffset`),
/// clamped to `max_offset` (`getMaxHorizontalOffset`).
fn xz_offset(x: i32, z: i32, max_offset: f32) -> (f64, f64) {
    let l = position_seed(x, z);
    let f = f64::from(max_offset);
    // `((float)(l & 15) / 15.0F - 0.5) * 0.5`: the quotient is a float, the rest double.
    let axis = |nibble: i64| (f64::from((nibble as f32) / 15.0_f32) - 0.5) * 0.5;
    let d = jclamp(axis(l & 15), f64::from(-max_offset), f);
    let e = jclamp(axis(l >> 8 & 15), f64::from(-max_offset), f);
    (d, e)
}

/// Bamboo and pointed dripstone collide with their shape moved by a hash of their position. The
/// context-free table was dumped at the origin, where that offset is the clamp's lower bound
/// (`-max_offset` on both axes); undo that and apply the offset of the real position.
fn offset_shape_boxes(state: u32, x: i32, z: i32, max_offset: f32) -> Vec<[f64; 6]> {
    let at_origin = -f64::from(max_offset);
    let (ox, oz) = xz_offset(x, z, max_offset);
    ms_data::collision_boxes(state)
        .iter()
        .map(|b| {
            [
                (b[0] - at_origin) + ox,
                b[1],
                (b[2] - at_origin) + oz,
                (b[3] - at_origin) + ox,
                b[4],
                (b[5] - at_origin) + oz,
            ]
        })
        .collect()
}

/// The collision boxes of the block at `(x, y, z)` as the player sees them, in block-local
/// coordinates. Scaffolding and powder snow depend on the entity (`EntityCollisionContext`: the
/// feet height `entityBottom`, `isDescending` = sneaking, the fall distance), bamboo and pointed
/// dripstone on the block position; everything else is the context-free table.
///
/// `bb` (the player's current box) is accepted for the caller's convenience; the game's context
/// reads the entity's `getY()` at the time the collision query starts, which is `p.pos.y`.
pub fn collision_boxes(
    p: &PlayerState,
    bb: Aabb,
    world: &World,
    x: i32,
    y: i32,
    z: i32,
) -> Vec<[f64; 6]> {
    let _ = bb;
    let state = world.block_state(x, y, z);
    match info(state).kind {
        Kind::Scaffolding => scaffolding_boxes(p, state, y),
        Kind::PowderSnow => powder_snow_boxes(p, y),
        Kind::Bamboo => offset_shape_boxes(state, x, z, 0.25),
        // MAX_HORIZONTAL_OFFSET = SHAPE_BASE.min(X) = (8 - 6) / 16
        Kind::PointedDripstone => offset_shape_boxes(state, x, z, 0.125),
        _ => ms_data::collision_boxes(state).to_vec(),
    }
}

// ---------------------------------------------------------------------------------------------
// Block under the feet
// ---------------------------------------------------------------------------------------------

/// `Entity.getOnPos(f)`: the block the entity stands on. With a main supporting block, that one
/// (moved to the block row `f` below the feet unless it is a fence, wall or fence gate, which
/// reach higher than their row); without one, the block `f` below the feet. `f` is 0.2 for
/// `getOnPosLegacy` (landing and `stepOn`), 1.0E-5 for `getOnPos`, 0.500001 for the block whose
/// speed and jump factor apply.
pub fn on_pos(p: &PlayerState, world: &World, f: f32) -> (i32, i32, i32) {
    match p.supporting_block {
        Some((sx, sy, sz)) => {
            if f <= 1.0E-5_f32 {
                return (sx, sy, sz);
            }
            let support = info(world.block_state(sx, sy, sz));
            let is_fence = f <= 0.5 && support.fence;
            if is_fence || support.wall || support.kind == Kind::FenceGate {
                (sx, sy, sz)
            } else {
                (sx, jfloor(p.pos.y - f64::from(f)), sz)
            }
        }
        None => (
            jfloor(p.pos.x),
            jfloor(p.pos.y - f64::from(f)),
            jfloor(p.pos.z),
        ),
    }
}

// ---------------------------------------------------------------------------------------------
// Landing: fallOn and updateEntityMovementAfterFallOn
// ---------------------------------------------------------------------------------------------

/// What `Block.fallOn` does about fall damage for the landed-on block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FallOn {
    /// `causeFallDamage` is not called at all (powder snow; slime while sneaking).
    NoDamageCall,
    /// `entity.causeFallDamage(distance, multiplier, ..)`: `distance` is the fall distance the
    /// block passes on (beds halve it, stalagmites add 2.5), `multiplier` scales the damage
    /// (hay and honey 0.2, slime 0.0, others 1.0).
    Damage { distance: f64, multiplier: f32 },
}

/// `Block.fallOn` for the block at `landed_on`, as data: which fall-damage call it makes.
pub fn fall_on_effect(
    p: &PlayerState,
    world: &World,
    landed_on: (i32, i32, i32),
    fall_distance: f64,
) -> FallOn {
    let (x, y, z) = landed_on;
    let state = world.block_state(x, y, z);
    let d = fall_distance;
    match info(state).kind {
        // SlimeBlock: sneaking suppresses the bounce and (in 1.21.11) the damage call with it;
        // otherwise the call has a multiplier of 0.
        Kind::Slime => {
            if p.shift_key_down {
                FallOn::NoDamageCall
            } else {
                FallOn::Damage {
                    distance: d,
                    multiplier: 0.0,
                }
            }
        }
        Kind::Hay | Kind::Honey => FallOn::Damage {
            distance: d,
            multiplier: 0.2,
        },
        // BedBlock: the default call with half the distance.
        Kind::Bed => FallOn::Damage {
            distance: d * 0.5,
            multiplier: 1.0,
        },
        Kind::PowderSnow => FallOn::NoDamageCall,
        Kind::PointedDripstone => {
            if ms_data::property(state, "vertical_direction") == Some("up")
                && ms_data::property(state, "thickness") == Some("tip")
            {
                FallOn::Damage {
                    distance: d + 2.5,
                    multiplier: 2.0,
                }
            } else {
                FallOn::Damage {
                    distance: d,
                    multiplier: 1.0,
                }
            }
        }
        _ => FallOn::Damage {
            distance: d,
            multiplier: 1.0,
        },
    }
}

/// `Block.fallOn`: the landed-on block reacts to a landing after `p.fall_distance` blocks by
/// calling `causeFallDamage` with its own distance and multiplier. Called by
/// `Entity.checkFallDamage` when the player lands with a positive fall distance; the caller
/// resets the fall distance afterwards.
pub fn fall_on(p: &mut PlayerState, world: &World, landed_on: (i32, i32, i32)) {
    let distance = p.fall_distance;
    if let FallOn::Damage {
        distance,
        multiplier,
    } = fall_on_effect(p, world, landed_on, distance)
    {
        damage::cause_fall_damage(p, distance, multiplier);
    }
}

/// The vertical velocity of a bounce: `-vy * factor` (beds scale it by `0.66F` first). Only a
/// downward velocity bounces.
fn bounce_up(p: &mut PlayerState, scale: Option<f32>) {
    let v = p.vel;
    if v.y < 0.0 {
        // `entity instanceof LivingEntity ? 1.0 : 0.8`
        let living = 1.0;
        let y = match scale {
            Some(s) => -v.y * f64::from(s) * living,
            None => -v.y * living,
        };
        p.vel = Vec3::new(v.x, y, v.z);
    }
}

/// `Block.updateEntityMovementAfterFallOn` for the block landed on: the default cancels the
/// vertical velocity (`multiply(1, 0, 1)`, which leaves a negative zero for a downward velocity);
/// slime and beds bounce unless the player sneaks.
pub fn after_fall_on(p: &mut PlayerState, world: &World, landed_on: (i32, i32, i32)) {
    let (x, y, z) = landed_on;
    let kind = info(world.block_state(x, y, z)).kind;
    match kind {
        Kind::Slime if !p.shift_key_down => bounce_up(p, None),
        Kind::Bed if !p.shift_key_down => bounce_up(p, Some(0.66_f32)),
        _ => {
            let v = p.vel;
            p.vel = Vec3::new(v.x * 1.0, v.y * 0.0, v.z * 1.0);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Stepping: stepOn
// ---------------------------------------------------------------------------------------------

/// `Block.stepOn` for the block under the feet (`getOnPosLegacy`), called when the player is on
/// the ground at the end of the tick: slime slows a slow-moving player down unless it sneaks, and
/// magma burns one that does not. (The magma damage is the server's; it reaches the damage module
/// as a generic one-point hit.)
pub fn step_on(p: &mut PlayerState, world: &World, on: (i32, i32, i32)) {
    let (x, y, z) = on;
    match info(world.block_state(x, y, z)).kind {
        Kind::Slime => {
            let d = p.vel.y.abs();
            if d < 0.1 && !p.shift_key_down {
                let e = 0.4 + d * 0.2;
                let v = p.vel;
                p.vel = Vec3::new(v.x * e, v.y * 1.0, v.z * e);
            }
        }
        Kind::Magma if !p.shift_key_down => {
            damage::hurt(p, DamageSource::Generic, 1.0);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------
// Inside blocks: Entity.applyEffectsFromBlocks
// ---------------------------------------------------------------------------------------------

/// One recorded move (`Entity.Movement`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Movement {
    pub from: Vec3,
    pub to: Vec3,
    /// The motion handed to the collision step, when known (`axisDependentOriginalMovement`):
    /// the inside-block check then walks the move one axis at a time in the collision's axis
    /// order instead of along the straight line from `from` to `to`.
    pub original: Option<Vec3>,
}

impl Movement {
    pub fn new(from: Vec3, to: Vec3, original: Option<Vec3>) -> Self {
        Self { from, to, original }
    }
}

/// The moves of the current tick (`Entity.movementThisTick`), oldest first. `Entity.move` records
/// every move that changed the position; [`apply_effects_from_blocks`] consumes the log.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MovementLog {
    entries: Vec<Movement>,
}

impl MovementLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// `Entity.addMovementThisTick`: with 100 entries already recorded, the two oldest merge into
    /// one segment (without axis information) to make room.
    pub fn record(&mut self, m: Movement) {
        if self.entries.len() >= 100 {
            let first = self.entries.remove(0);
            let second = self.entries.remove(0);
            self.entries
                .insert(0, Movement::new(first.from, second.to, None));
        }
        self.entries.push(m);
    }

    /// `Entity.removeLatestMovementRecording`.
    pub fn remove_latest(&mut self) {
        self.entries.pop();
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// `Entity.makeStuckInBlock` for the player: reset the fall distance and set the stuck-speed
/// multiplier the next `move` applies (a flying player ignores it).
fn make_stuck_in_block(p: &mut PlayerState, multiplier: Vec3) {
    if !p.flying {
        p.fall_distance = 0.0;
        p.stuck_speed_multiplier = multiplier;
    }
}

/// The inside-block effects that are deferred until the traversal is over and then applied in
/// enum order within each step (`InsideBlockEffectType`; the ones that nothing wired up yet —
/// clear-freeze, the two ignitions — are left out).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum InsideEffect {
    /// Powder snow: the player counts as being in powder snow, and freezes one tick further.
    Freeze,
    /// Powder snow and water: put out the fire.
    Extinguish,
}

impl InsideEffect {
    /// `InsideBlockEffectType.values()` order.
    const ORDER: [InsideEffect; 2] = [InsideEffect::Freeze, InsideEffect::Extinguish];

    fn bit(self) -> u8 {
        match self {
            InsideEffect::Freeze => 1,
            InsideEffect::Extinguish => 2,
        }
    }

    /// `FREEZE`: `setIsInPowderSnow(true)` and, when the player can freeze,
    /// `ticksFrozen = min(ticksRequiredToFreeze, ticksFrozen + 1)` (140 ticks to freeze).
    /// `EXTINGUISH`: `clearFire`.
    fn apply(self, p: &mut PlayerState) {
        match self {
            InsideEffect::Freeze => {
                p.in_powder_snow = true;
                p.ticks_frozen = p.ticks_frozen.saturating_add(1).min(140);
            }
            InsideEffect::Extinguish => {
                p.remaining_fire_ticks = p.remaining_fire_ticks.min(0);
            }
        }
    }
}

/// `InsideBlockEffectApplier.StepBasedCollector`: effects raised by blocks are collected per step
/// of the traversal (each type at most once per step), flushed in enum order when the step
/// advances, and applied together at the end.
#[derive(Default)]
struct Collector {
    in_step: u8,
    queued: Vec<InsideEffect>,
    /// `-1` before the first block.
    last_step: Option<i32>,
}

impl Collector {
    fn advance_step(&mut self, step: i32) {
        if self.last_step != Some(step) {
            self.last_step = Some(step);
            self.flush_step();
        }
    }

    fn flush_step(&mut self) {
        for effect in InsideEffect::ORDER {
            if self.in_step & effect.bit() != 0 {
                self.in_step &= !effect.bit();
                self.queued.push(effect);
            }
        }
    }

    fn apply(&mut self, effect: InsideEffect) {
        self.in_step |= effect.bit();
    }

    fn apply_and_clear(&mut self, p: &mut PlayerState) {
        self.flush_step();
        for effect in std::mem::take(&mut self.queued) {
            if !p.is_alive() {
                break;
            }
            effect.apply(p);
        }
        self.last_step = None;
    }
}

/// `Entity.applyEffectsFromBlocks()`: the effects of every block the player's box passed through
/// this tick, in the game's visiting order. `log` holds the moves `Entity.move` recorded; it is
/// emptied. `old_pos` is the position the tick started at (`oldPosition`): it stands in for the
/// move when none was recorded, and is the base of the movement the sweet berry bush judges.
/// Call [`step_on`] first when the player is on the ground, as the game does.
///
/// The visit applies, per block: cobweb, sweet berry bush and powder snow set the stuck-speed
/// multiplier (and reset the fall distance), powder snow raises the freeze effect, the honey
/// block makes a player sliding down its side crawl, and bubble columns push (see the module
/// documentation for what is left out).
pub fn apply_effects_from_blocks(
    p: &mut PlayerState,
    world: &World,
    log: &mut MovementLog,
    old_pos: Vec3,
) {
    let mut moves = std::mem::take(&mut log.entries);
    match moves.last() {
        None => moves.push(Movement::new(old_pos, p.pos, None)),
        Some(last) => {
            // 9.9999994E-11F
            if distance_sqr(last.to, p.pos) > f64::from(9.999_999_4e-11_f32) {
                let from = last.to;
                moves.push(Movement::new(from, p.pos, None));
            }
        }
    }
    check_inside_blocks(p, world, &moves, old_pos);
    // Reuse the log's allocation for the next tick.
    moves.clear();
    log.entries = moves;
}

/// `Entity.applyEffectsFromBlocks(from, to)`: the effects of one straight move (no axis
/// information), as used for moves the game does not record itself. `from` doubles as the old
/// position.
pub fn apply_effects_from_segment(p: &mut PlayerState, world: &World, from: Vec3, to: Vec3) {
    check_inside_blocks(p, world, &[Movement::new(from, to, None)], from);
}

/// `Entity.checkInsideBlocks(list, collector)` followed by the collector's `applyAndClear`.
fn check_inside_blocks(p: &mut PlayerState, world: &World, moves: &[Movement], old_pos: Vec3) {
    let mut walk = Walk {
        p,
        world,
        old_pos,
        collector: Collector::default(),
        visited: Vec::new(),
    };
    for m in moves {
        let mut from = m.from;
        let delta = sub(m.to, m.from);
        let mut budget = 16;
        match m.original {
            Some(original) if length_sqr(delta) > 0.0 => {
                for axis in axis_step_order(original) {
                    let d = axis_value(delta, axis);
                    if d != 0.0 {
                        let next = relative(from, axis, d);
                        budget -= walk.check_segment(from, next, budget);
                        from = next;
                    }
                }
            }
            _ => budget -= walk.check_segment(m.from, m.to, 16),
        }
        if budget <= 0 {
            walk.check_segment(m.to, m.to, 1);
        }
    }
    let Walk {
        p, mut collector, ..
    } = walk;
    collector.apply_and_clear(p);
}

/// The state of one `checkInsideBlocks` run.
struct Walk<'a> {
    p: &'a mut PlayerState,
    world: &'a World,
    old_pos: Vec3,
    collector: Collector,
    /// Blocks whose `entityInside` already ran this tick (`visitedBlocks`).
    visited: Vec<(i32, i32, i32)>,
}

/// The player's bounding box with its feet at `pos` (`Entity.makeBoundingBox(Vec3)`).
fn bounding_box_at(p: &PlayerState, pos: Vec3) -> Aabb {
    let (w, h) = p.dimensions();
    let half = f64::from(w / 2.0_f32);
    Aabb::new(
        Vec3::new(pos.x - half, pos.y, pos.z - half),
        Vec3::new(pos.x + half, pos.y + f64::from(h), pos.z + half),
    )
}

impl Walk<'_> {
    /// The inner `Entity.checkInsideBlocks(from, to, ..)`: visit the blocks the box overlaps on
    /// its way from `from` to `to`, at most `budget` steps deep. Returns the number of steps it
    /// consumed.
    fn check_segment(&mut self, from: Vec3, to: Vec3, budget: i32) -> i32 {
        let bb = bounding_box_at(self.p, to).inflate(
            -f64::from(1.0E-5_f32),
            -f64::from(1.0E-5_f32),
            -f64::from(1.0E-5_f32),
        );
        // A move of nearly a block or more counts every overlapped block as precisely touched.
        let long_move =
            distance_sqr(from, to) > 0.999_990_000_000_252_6_f64 * 0.999_990_000_000_252_6;
        let mut last_step = 0;
        for_each_block_intersected_between(from, to, bb, &mut |pos, step| {
            if !self.p.is_alive() || step >= budget {
                return false;
            }
            last_step = step;
            self.visit_block(pos, step, from, to, bb, long_move);
            true
        });
        last_step + 1
    }

    /// One block of the traversal: skip air, decide whether the player really is inside it, and
    /// run its `entityInside` once per tick.
    fn visit_block(
        &mut self,
        pos: (i32, i32, i32),
        step: i32,
        from: Vec3,
        to: Vec3,
        bb: Aabb,
        long_move: bool,
    ) {
        let (x, y, z) = pos;
        let state = self.world.block_state(x, y, z);
        if state == ms_data::AIR {
            return;
        }
        let kind = info(state).kind;
        let inside = match kind {
            // PowderSnowBlock.getEntityInsideCollisionShape: the collision shape for this entity
            // when it has one (the falling shape), else the whole block.
            Kind::PowderSnow => {
                let boxes = powder_snow_boxes(self.p, y);
                if boxes.is_empty() || boxes == [FULL_BLOCK] {
                    true
                } else {
                    let moved: Vec<Aabb> = boxes
                        .iter()
                        .map(|b| {
                            Aabb::new(
                                Vec3::new(
                                    b[0] + f64::from(x),
                                    b[1] + f64::from(y),
                                    b[2] + f64::from(z),
                                ),
                                Vec3::new(
                                    b[3] + f64::from(x),
                                    b[4] + f64::from(y),
                                    b[5] + f64::from(z),
                                ),
                            )
                        })
                        .collect();
                    collided_along_vector(bounding_box_at(self.p, from), sub(to, from), &moved)
                }
            }
            // Every other block uses Shapes.block(): inside as soon as the traversal reaches it.
            _ => true,
        };
        if !inside || self.visited.contains(&pos) {
            return;
        }
        self.visited.push(pos);
        self.collector.advance_step(step);
        let precise = long_move
            || bb.intersects(Aabb::from_corners(
                f64::from(x),
                f64::from(y),
                f64::from(z),
                f64::from(x) + 1.0,
                f64::from(y) + 1.0,
                f64::from(z) + 1.0,
            ));
        self.entity_inside(kind, state, pos, precise);
    }

    /// `BlockState.entityInside` for the blocks that act on the player.
    fn entity_inside(&mut self, kind: Kind, state: u32, pos: (i32, i32, i32), precise: bool) {
        match kind {
            Kind::Web => {
                // WebBlock: (0.25, 0.05F, 0.25), or a quarter of the slowdown with the weaving
                // effect.
                let v = if self.p.effects.has("minecraft:weaving") {
                    Vec3::new(0.5, 0.25, 0.5)
                } else {
                    Vec3::new(0.25, f64::from(0.05_f32), 0.25)
                };
                make_stuck_in_block(self.p, v);
            }
            Kind::SweetBerryBush => {
                make_stuck_in_block(
                    self.p,
                    Vec3::new(f64::from(0.8_f32), 0.75, f64::from(0.8_f32)),
                );
                // The server's half: a grown bush (age > 0) hurts a player that moved
                // horizontally by at least 0.003F along an axis this tick.
                if ms_data::property(state, "age") != Some("0") {
                    let moved = sub(self.p.pos, self.old_pos);
                    if moved.x * moved.x + moved.z * moved.z > 0.0 {
                        let threshold = f64::from(0.003_f32);
                        if moved.x.abs() >= threshold || moved.z.abs() >= threshold {
                            damage::hurt(self.p, DamageSource::Generic, 1.0);
                        }
                    }
                }
            }
            Kind::PowderSnow => {
                // Only when the player's own block is powder snow (feet inside it).
                let (bx, by, bz) = block_pos(self.p.pos);
                let own = self.world.block_state(bx, by, bz);
                if info(own).kind == Kind::PowderSnow {
                    make_stuck_in_block(
                        self.p,
                        Vec3::new(f64::from(0.9_f32), 1.5, f64::from(0.9_f32)),
                    );
                }
                self.collector.apply(InsideEffect::Freeze);
                self.collector.apply(InsideEffect::Extinguish);
            }
            Kind::Honey => {
                if is_sliding_down_honey(self.p, pos) {
                    honey_slide_movement(self.p);
                }
            }
            Kind::BubbleColumn => bubble_column_inside(self.p, self.world, pos, precise),
            _ => {}
        }
    }
}

/// `HoneyBlock.isSlidingDown`: in the air, at or below the block's top (less a tolerance), falling
/// faster than the slide's start threshold, and pressed against one of the block's sides.
fn is_sliding_down_honey(p: &PlayerState, pos: (i32, i32, i32)) -> bool {
    if p.on_ground {
        return false;
    }
    if p.pos.y > f64::from(pos.1) + 0.9375 - 1.0E-7 {
        return false;
    }
    if honey_old_delta_y(p.vel.y) >= -0.08 {
        return false;
    }
    let d = (f64::from(pos.0) + 0.5 - p.pos.x).abs();
    let e = (f64::from(pos.2) + 0.5 - p.pos.z).abs();
    let (width, _) = p.dimensions();
    let f = 0.4375 + f64::from(width / 2.0_f32);
    d + 1.0E-7 > f || e + 1.0E-7 > f
}

/// `HoneyBlock.getOldDeltaY`: undo one tick of gravity and drag on a vertical velocity.
fn honey_old_delta_y(d: f64) -> f64 {
    d / f64::from(0.98_f32) + 0.08
}

/// `HoneyBlock.getNewDeltaY`: one tick of gravity and drag.
fn honey_new_delta_y(d: f64) -> f64 {
    (d - 0.08) * f64::from(0.98_f32)
}

/// `HoneyBlock.doSlideMovement`: cap the fall speed at the slide speed (scaling the horizontal
/// velocity with it when the player was falling faster than 0.13 per tick), and reset the fall
/// distance.
fn honey_slide_movement(p: &mut PlayerState) {
    let v = p.vel;
    let old = honey_old_delta_y(v.y);
    p.vel = if old < -0.13 {
        let d = -0.05 / old;
        Vec3::new(v.x * d, honey_new_delta_y(-0.05), v.z * d)
    } else {
        Vec3::new(v.x, honey_new_delta_y(-0.05), v.z)
    };
    p.fall_distance = 0.0;
}

/// `BubbleColumnBlock.entityInside` (a no-op unless `precise`: the box really intersects the
/// block): at the top of a column — the block above has neither a collision shape nor fluid — the
/// player is pushed along `Entity.handleOnAboveBubbleColumn`, inside it along
/// `handleOnInsideBubbleColumn`; the column's `drag` property picks the direction. A flying player
/// ignores both. (The fluid module owns these velocity rules; this is the same arithmetic, kept
/// private so the module compiles on its own — the integrator can route to the fluid module's.)
fn bubble_column_inside(p: &mut PlayerState, world: &World, pos: (i32, i32, i32), precise: bool) {
    if !precise || p.flying {
        return;
    }
    let (x, y, z) = pos;
    let state = world.block_state(x, y, z);
    let drag_down = ms_data::property(state, "drag") == Some("true");
    let above = world.block_state(x, y + 1, z);
    let open_above = ms_data::collision_boxes(above).is_empty() && ms_data::fluid(above).is_empty();
    let v = p.vel;
    if open_above {
        let y = if drag_down {
            jmax(-0.9, v.y - 0.03)
        } else {
            jmin(1.8, v.y + 0.1)
        };
        p.vel = Vec3::new(v.x, y, v.z);
    } else {
        let y = if drag_down {
            jmax(-0.3, v.y - 0.03)
        } else {
            jmin(0.7, v.y + 0.06)
        };
        p.vel = Vec3::new(v.x, y, v.z);
        p.fall_distance = 0.0;
    }
}

/// `Math.max(double, double)`.
fn jmax(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        a
    } else if a == 0.0 && b == 0.0 && a.is_sign_negative() {
        b
    } else if a >= b {
        a
    } else {
        b
    }
}

// ---------------------------------------------------------------------------------------------
// The block traversal: BlockGetter.forEachBlockIntersectedBetween
// ---------------------------------------------------------------------------------------------

/// `BlockPos.betweenClosed(AABB)` corners: the blocks containing the box's min and max points.
fn block_range(bb: Aabb) -> ((i32, i32, i32), (i32, i32, i32)) {
    (
        (jfloor(bb.min.x), jfloor(bb.min.y), jfloor(bb.min.z)),
        (jfloor(bb.max.x), jfloor(bb.max.y), jfloor(bb.max.z)),
    )
}

/// `BlockPos.betweenCornersInDirection(c0, c1, direction)`: the blocks of the box spanned by the
/// two corners, starting at the corner the direction moves away from and sweeping Y slowest, then
/// the axis the direction moves less along, then the one it moves more along (`axisStepOrder`).
fn between_corners_in_direction(
    c0: (i32, i32, i32),
    c1: (i32, i32, i32),
    dir: Vec3,
) -> Vec<(i32, i32, i32)> {
    let lo = (c0.0.min(c1.0), c0.1.min(c1.1), c0.2.min(c1.2));
    let hi = (c0.0.max(c1.0), c0.1.max(c1.1), c0.2.max(c1.2));
    let size = (hi.0 - lo.0, hi.1 - lo.1, hi.2 - lo.2);
    let start = (
        if dir.x >= 0.0 { lo.0 } else { hi.0 },
        if dir.y >= 0.0 { lo.1 } else { hi.1 },
        if dir.z >= 0.0 { lo.2 } else { hi.2 },
    );
    let order = axis_step_order(dir);
    let unit = |axis: Axis| -> (i32, i32, i32) {
        let s = if axis_value(dir, axis) >= 0.0 { 1 } else { -1 };
        match axis {
            Axis::X => (s, 0, 0),
            Axis::Y => (0, s, 0),
            Axis::Z => (0, 0, s),
        }
    };
    let extent = |axis: Axis| match axis {
        Axis::X => size.0,
        Axis::Y => size.1,
        Axis::Z => size.2,
    };
    let (u0, u1, u2) = (unit(order[0]), unit(order[1]), unit(order[2]));
    let (n0, n1, n2) = (extent(order[0]), extent(order[1]), extent(order[2]));
    let mut out = Vec::with_capacity(((n0 + 1) * (n1 + 1) * (n2 + 1)).max(0) as usize);
    for i in 0..=n0 {
        for j in 0..=n1 {
            for k in 0..=n2 {
                out.push((
                    start.0 + u0.0 * i + u1.0 * j + u2.0 * k,
                    start.1 + u0.1 * i + u1.1 * j + u2.1 * k,
                    start.2 + u0.2 * i + u1.2 * j + u2.2 * k,
                ));
            }
        }
    }
    out
}

/// `BlockGetter.getFurthestCorner`: which corner of the box (as signs along each axis) the
/// travel direction leaves last, chosen by the axis the direction moves least along.
fn furthest_corner(dir: Vec3) -> (i32, i32, i32) {
    let d = dir.x.abs();
    let e = dir.y.abs();
    let f = dir.z.abs();
    let i = if dir.x >= 0.0 { 1 } else { -1 };
    let j = if dir.y >= 0.0 { 1 } else { -1 };
    let k = if dir.z >= 0.0 { 1 } else { -1 };
    if d <= e && d <= f {
        (-i, -k, j)
    } else if e <= f {
        (k, -j, -i)
    } else {
        (-j, i, -k)
    }
}

/// One plane test of `AABB.clip`: where the segment crosses the plane `plane` of the main axis
/// (`main` is its direction component, `start` its start), if that is within `ds` and inside the
/// face rectangle (with the 1.0E-7 tolerance). Updates `ds` and reports a hit.
#[allow(clippy::too_many_arguments)]
fn clip_plane(
    ds: &mut f64,
    main: f64,
    other_a: f64,
    other_b: f64,
    plane: f64,
    range_a: (f64, f64),
    range_b: (f64, f64),
    start: (f64, f64, f64),
) -> bool {
    let t = (plane - start.0) / main;
    let a = start.1 + t * other_a;
    let b = start.2 + t * other_b;
    if 0.0 < t
        && t < *ds
        && range_a.0 - 1.0E-7 < a
        && a < range_a.1 + 1.0E-7
        && range_b.0 - 1.0E-7 < b
        && b < range_b.1 + 1.0E-7
    {
        *ds = t;
        true
    } else {
        false
    }
}

/// `AABB.clip(min..max, from, to)`: the first point where the segment enters the box, if any.
fn clip_box(min: Vec3, max: Vec3, from: Vec3, to: Vec3) -> Option<Vec3> {
    let mut ds = 1.0_f64;
    let (dx, dy, dz) = (to.x - from.x, to.y - from.y, to.z - from.z);
    let mut hit = false;
    let x_range = (min.x, max.x);
    let y_range = (min.y, max.y);
    let z_range = (min.z, max.z);
    if dx > 1.0E-7 {
        hit |= clip_plane(
            &mut ds,
            dx,
            dy,
            dz,
            min.x,
            y_range,
            z_range,
            (from.x, from.y, from.z),
        );
    } else if dx < -1.0E-7 {
        hit |= clip_plane(
            &mut ds,
            dx,
            dy,
            dz,
            max.x,
            y_range,
            z_range,
            (from.x, from.y, from.z),
        );
    }
    if dy > 1.0E-7 {
        hit |= clip_plane(
            &mut ds,
            dy,
            dz,
            dx,
            min.y,
            z_range,
            x_range,
            (from.y, from.z, from.x),
        );
    } else if dy < -1.0E-7 {
        hit |= clip_plane(
            &mut ds,
            dy,
            dz,
            dx,
            max.y,
            z_range,
            x_range,
            (from.y, from.z, from.x),
        );
    }
    if dz > 1.0E-7 {
        hit |= clip_plane(
            &mut ds,
            dz,
            dx,
            dy,
            min.z,
            x_range,
            y_range,
            (from.z, from.x, from.y),
        );
    } else if dz < -1.0E-7 {
        hit |= clip_plane(
            &mut ds,
            dz,
            dx,
            dy,
            max.z,
            x_range,
            y_range,
            (from.z, from.x, from.y),
        );
    }
    if hit {
        Some(Vec3::new(
            from.x + ds * dx,
            from.y + ds * dy,
            from.z + ds * dz,
        ))
    } else {
        None
    }
}

/// `AABB.clip(i, j, k, i+1, j+1, k+1, from, to)` for the unit block `(i, j, k)`.
fn clip_block(i: i32, j: i32, k: i32, from: Vec3, to: Vec3) -> Option<Vec3> {
    clip_box(
        Vec3::new(f64::from(i), f64::from(j), f64::from(k)),
        Vec3::new(
            f64::from(i.wrapping_add(1)),
            f64::from(j.wrapping_add(1)),
            f64::from(k.wrapping_add(1)),
        ),
        from,
        to,
    )
}

/// `AABB.contains(Vec3)`: half-open on the max side.
fn box_contains(b: Aabb, v: Vec3) -> bool {
    v.x >= b.min.x
        && v.x < b.max.x
        && v.y >= b.min.y
        && v.y < b.max.y
        && v.z >= b.min.z
        && v.z < b.max.z
}

/// `AABB.getCenter`.
fn box_center(b: Aabb) -> Vec3 {
    Vec3::new(
        b.min.x + 0.5 * (b.max.x - b.min.x),
        b.min.y + 0.5 * (b.max.y - b.min.y),
        b.min.z + 0.5 * (b.max.z - b.min.z),
    )
}

/// `AABB.collidedAlongVector`: whether the box, moved by `motion`, overlaps any of `boxes` (each
/// grown by the box's half size, so the test is on its centre).
fn collided_along_vector(bb: Aabb, motion: Vec3, boxes: &[Aabb]) -> bool {
    let center = box_center(bb);
    let end = Vec3::new(
        center.x + motion.x,
        center.y + motion.y,
        center.z + motion.z,
    );
    for b in boxes {
        let grown = b.inflate(
            (bb.max.x - bb.min.x) * 0.5 - 1.0E-7,
            (bb.max.y - bb.min.y) * 0.5 - 1.0E-7,
            (bb.max.z - bb.min.z) * 0.5 - 1.0E-7,
        );
        if box_contains(grown, end) || box_contains(grown, center) {
            return true;
        }
        if clip_box(grown.min, grown.max, center, end).is_some() {
            return true;
        }
    }
    false
}

/// A set of block positions with insertion reporting (`LongOpenHashSet.add`). The sets here hold a
/// few dozen entries, so a vector beats hashing.
#[derive(Default)]
struct BlockSet {
    items: Vec<(i32, i32, i32)>,
}

impl BlockSet {
    fn add(&mut self, pos: (i32, i32, i32)) -> bool {
        if self.items.contains(&pos) {
            false
        } else {
            self.items.push(pos);
            true
        }
    }
}

/// `BlockGetter.forEachBlockIntersectedBetween(from, to, box, visitor)`: call `visit(pos, step)`
/// for every block the `bb` (the entity's box at the end of the move) overlaps, or overlapped
/// while travelling from `from` to `to`. `step` numbers the stages of the sweep: 0 for the blocks
/// around the start of the move, then 1, 2, ... for the blocks entered along the travel line, and
/// last for the blocks around the end. Stops as soon as `visit` returns false; returns whether the
/// whole traversal ran.
fn for_each_block_intersected_between(
    from: Vec3,
    to: Vec3,
    bb: Aabb,
    visit: &mut dyn FnMut((i32, i32, i32), i32) -> bool,
) -> bool {
    let delta = sub(to, from);
    // Mth.square(1.0E-5F) is a float product.
    let tiny = 1.0E-5_f32;
    if length_sqr(delta) < f64::from(tiny * tiny) {
        let (lo, hi) = block_range(bb);
        // BlockPos.betweenClosed: x fastest, then y, then z.
        for z in lo.2..=hi.2 {
            for y in lo.1..=hi.1 {
                for x in lo.0..=hi.0 {
                    if !visit((x, y, z), 0) {
                        return false;
                    }
                }
            }
        }
        return true;
    }
    let mut seen = BlockSet::default();
    // `delta.scale(-1.0)`: multiplying by -1 is exact negation.
    let start_box = bb.move_by(Vec3::new(-delta.x, -delta.y, -delta.z));
    let (lo, hi) = block_range(start_box);
    for pos in between_corners_in_direction(lo, hi, delta) {
        if !visit(pos, 0) {
            return false;
        }
        seen.add(pos);
    }
    let Some(steps) = add_collisions_along_travel(&mut seen, delta, bb, visit) else {
        return false;
    };
    let (lo, hi) = block_range(bb);
    for pos in between_corners_in_direction(lo, hi, delta) {
        if seen.add(pos) && !visit(pos, steps + 1) {
            return false;
        }
    }
    true
}

/// `BlockGetter.addCollisionsAlongTravel`: walk a line through the grid along the box's furthest
/// corner (relative to the travel direction), and for every block the line enters add the blocks
/// the box sweeps over there. Returns how many blocks the line entered (the number of steps), or
/// `None` when the visitor asked to stop.
fn add_collisions_along_travel(
    seen: &mut BlockSet,
    delta: Vec3,
    bb: Aabb,
    visit: &mut dyn FnMut((i32, i32, i32), i32) -> bool,
) -> Option<i32> {
    let size = (
        bb.max.x - bb.min.x,
        bb.max.y - bb.min.y,
        bb.max.z - bb.min.z,
    );
    let corner = furthest_corner(delta);
    let center = box_center(bb);
    let far = Vec3::new(
        center.x + size.0 * 0.5 * f64::from(corner.0),
        center.y + size.1 * 0.5 * f64::from(corner.1),
        center.z + size.2 * 0.5 * f64::from(corner.2),
    );
    let rel = sub(far, delta);
    let (mut i, mut j, mut k) = (jfloor(rel.x), jfloor(rel.y), jfloor(rel.z));
    let (sx, sy, sz) = (jsign(delta.x), jsign(delta.y), jsign(delta.z));
    let inv = |s: i32, d: f64| if s == 0 { f64::MAX } else { f64::from(s) / d };
    let (gx, gy, gz) = (inv(sx, delta.x), inv(sy, delta.y), inv(sz, delta.z));
    let (mut px, mut py, mut pz) = (
        gx * if sx > 0 {
            1.0 - jfrac(rel.x)
        } else {
            jfrac(rel.x)
        },
        gy * if sy > 0 {
            1.0 - jfrac(rel.y)
        } else {
            jfrac(rel.y)
        },
        gz * if sz > 0 {
            1.0 - jfrac(rel.z)
        } else {
            jfrac(rel.z)
        },
    );
    let mut steps = 0;
    while px <= 1.0 || py <= 1.0 || pz <= 1.0 {
        if px < py {
            if px < pz {
                i = i.wrapping_add(sx);
                px += gx;
            } else {
                k = k.wrapping_add(sz);
                pz += gz;
            }
        } else if py < pz {
            j = j.wrapping_add(sy);
            py += gy;
        } else {
            k = k.wrapping_add(sz);
            pz += gz;
        }
        if let Some(hit) = clip_block(i, j, k, rel, far) {
            steps += 1;
            // Mth.clamp(x, i + 1.0E-5F, i + 1.0 - 1.0E-5F): the lower bound is a float sum.
            let eps = 1.0E-5_f32;
            let lower = |c: i32| f64::from(c as f32 + eps);
            let upper = |c: i32| f64::from(c) + 1.0 - f64::from(eps);
            let tx = jclamp(hit.x, lower(i), upper(i));
            let ty = jclamp(hit.y, lower(j), upper(j));
            let tz = jclamp(hit.z, lower(k), upper(k));
            let end = (
                jfloor(tx - size.0 * f64::from(corner.0)),
                jfloor(ty - size.1 * f64::from(corner.1)),
                jfloor(tz - size.2 * f64::from(corner.2)),
            );
            for pos in between_corners_in_direction((i, j, k), end, delta) {
                if seen.add(pos) && !visit(pos, steps) {
                    return None;
                }
            }
        }
    }
    Some(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world_with(blocks: &[((i32, i32, i32), &str)]) -> World {
        let mut grid = ms_world::GridWorld::new(ms_world::FlatWorld::new(0, ms_data::AIR));
        for ((x, y, z), name) in blocks {
            grid.set_block(
                *x,
                *y,
                *z,
                ms_data::parse_state(name).expect("known block state"),
            );
        }
        World::grid(grid)
    }

    fn player_at(x: f64, y: f64, z: f64) -> PlayerState {
        PlayerState::new(Vec3::new(x, y, z), 0.0)
    }

    #[test]
    fn climbable_blocks_and_trapdoors() {
        let w = world_with(&[
            ((0, 0, 0), "minecraft:ladder[facing=north]"),
            ((1, 0, 0), "minecraft:vine[south=true]"),
            ((2, 0, 0), "minecraft:scaffolding"),
            ((3, 0, 0), "minecraft:stone"),
            // an open trapdoor above a ladder facing the same way climbs, other ways do not
            ((4, 0, 0), "minecraft:ladder[facing=east]"),
            ((4, 1, 0), "minecraft:oak_trapdoor[open=true,facing=east]"),
            ((5, 0, 0), "minecraft:ladder[facing=east]"),
            ((5, 1, 0), "minecraft:oak_trapdoor[open=true,facing=west]"),
            ((6, 0, 0), "minecraft:ladder[facing=east]"),
            ((6, 1, 0), "minecraft:oak_trapdoor[open=false,facing=east]"),
            ((7, 0, 0), "minecraft:ladder[facing=east]"),
            (
                (7, 1, 0),
                "minecraft:waxed_copper_trapdoor[open=true,facing=east]",
            ),
        ]);
        let at = |x: f64, y: f64| on_climbable(&player_at(x + 0.5, y, 0.5), &w);
        assert!(at(0.0, 0.0));
        assert!(at(1.0, 0.0));
        assert!(at(2.0, 0.0));
        assert!(!at(3.0, 0.0));
        assert!(!at(9.0, 0.0));
        assert!(at(4.0, 1.0));
        assert!(!at(5.0, 1.0));
        assert!(!at(6.0, 1.0));
        // the copper variants are trapdoors too
        assert!(at(7.0, 1.0));
    }

    #[test]
    fn gliding_through_a_glide_through_block_is_not_climbing() {
        // Nothing in the tag overlaps the climbable tag today; the tag lookup must at least work.
        let w = world_with(&[((0, 0, 0), "minecraft:ladder[facing=north]")]);
        let mut p = player_at(0.5, 0.0, 0.5);
        p.pose = Pose::FallFlying;
        assert!(on_climbable(&p, &w));
    }

    #[test]
    fn scaffolding_shape_depends_on_height_and_sneaking() {
        let w = world_with(&[
            ((0, 0, 0), "minecraft:scaffolding[bottom=true,distance=3]"),
            ((2, 0, 0), "minecraft:scaffolding[bottom=false,distance=0]"),
            ((4, 0, 0), "minecraft:scaffolding[bottom=true,distance=0]"),
        ]);
        let bb = Aabb::from_corners(0.0, 0.0, 0.0, 0.6, 1.8, 0.6);
        let stable = collision_boxes(&player_at(0.5, 1.0, 0.5), bb, &w, 0, 0, 0);
        assert_eq!(stable.len(), 7);
        // sneaking above it: falls through the stable shape, but an unstable (bottom) scaffold
        // keeps its thin base plate for a player above the block below.
        let mut sneaking = player_at(0.5, 1.0, 0.5);
        sneaking.shift_key_down = true;
        assert_eq!(
            collision_boxes(&sneaking, bb, &w, 0, 0, 0),
            vec![[0.0, 0.0, 0.0, 1.0, 0.125, 1.0]]
        );
        // a stable scaffold (distance 0, not bottom) has nothing for a sneaking player
        assert!(collision_boxes(&sneaking, bb, &w, 2, 0, 0).is_empty());
        // distance 0 never has the base plate
        assert!(collision_boxes(&sneaking, bb, &w, 4, 0, 0).is_empty());
        // inside a stable block (feet below the top): nothing; the unstable one keeps its plate
        let inside = player_at(0.5, 0.5, 0.5);
        assert!(collision_boxes(&inside, bb, &w, 2, 0, 0).is_empty());
        assert_eq!(collision_boxes(&inside, bb, &w, 0, 0, 0).len(), 1);
        // the tolerance: feet exactly at the top face minus 1e-5 are not above
        let edge = player_at(0.5, 1.0 - f64::from(1.0E-5_f32), 0.5);
        assert!(collision_boxes(&edge, bb, &w, 2, 0, 0).is_empty());
        let above_edge = player_at(0.5, 1.0 - f64::from(1.0E-5_f32) + 1.0E-9, 0.5);
        assert_eq!(collision_boxes(&above_edge, bb, &w, 2, 0, 0).len(), 7);
    }

    #[test]
    fn powder_snow_collides_only_for_a_falling_player() {
        let w = world_with(&[((0, 0, 0), "minecraft:powder_snow")]);
        let bb = Aabb::from_corners(0.0, 0.0, 0.0, 0.6, 1.8, 0.6);
        let mut p = player_at(0.5, 1.0, 0.5);
        assert!(collision_boxes(&p, bb, &w, 0, 0, 0).is_empty());
        p.fall_distance = 2.5;
        assert!(collision_boxes(&p, bb, &w, 0, 0, 0).is_empty());
        p.fall_distance = 2.5000001;
        assert_eq!(
            collision_boxes(&p, bb, &w, 0, 0, 0),
            vec![[0.0, 0.0, 0.0, 1.0, f64::from(0.9_f32), 1.0]]
        );
    }

    #[test]
    fn bamboo_collision_follows_the_position_hash() {
        let w = world_with(&[
            ((0, 0, 0), "minecraft:bamboo"),
            ((7, 0, -3), "minecraft:bamboo"),
        ]);
        let bb = Aabb::from_corners(0.0, 0.0, 0.0, 0.6, 1.8, 0.6);
        let p = player_at(0.5, 1.0, 0.5);
        // At the origin the hash is 0 and the offset is the lower clamp, (-0.25, -0.25): the
        // 3-pixel stalk sits at its corner.
        let at_origin = collision_boxes(&p, bb, &w, 0, 0, 0);
        assert_eq!(
            at_origin,
            vec![[0.15625, 0.0, 0.15625, 0.34375, 1.0, 0.34375]]
        );
        // Anywhere else the box is the 3-pixel column (width 0.1875) at a hashed offset within
        // +-0.25 of the block centre.
        let b = collision_boxes(&p, bb, &w, 7, 0, -3)[0];
        assert_eq!(b[1], 0.0);
        assert_eq!(b[4], 1.0);
        assert!((b[3] - b[0] - 0.1875).abs() < 1.0E-12);
        assert!((b[5] - b[2] - 0.1875).abs() < 1.0E-12);
        assert!(b[0] >= 0.40625 - 0.25 - 1.0E-12 && b[0] <= 0.40625 + 0.25 + 1.0E-12);
        // the offset of the real position, applied the way the game does
        let (ox, oz) = xz_offset(7, -3, 0.25);
        assert_eq!(b[0], 0.40625 + ox);
        assert_eq!(b[2], 0.40625 + oz);
    }

    #[test]
    fn position_seed_matches_known_values() {
        // Mth.getSeed(0, 0, 0) = 0, and the XZ offset of the origin is the lower clamp.
        assert_eq!(position_seed(0, 0), 0);
        assert_eq!(xz_offset(0, 0, 0.25), (-0.25, -0.25));
        assert_eq!(xz_offset(0, 0, 0.125), (-0.125, -0.125));
    }

    #[test]
    fn default_landing_zeroes_vertical_velocity_with_a_signed_zero() {
        let w = world_with(&[((0, 0, 0), "minecraft:stone")]);
        let mut p = player_at(0.5, 1.0, 0.5);
        p.vel = Vec3::new(0.1, -0.5, -0.2);
        after_fall_on(&mut p, &w, (0, 0, 0));
        assert_eq!(p.vel.x.to_bits(), 0.1_f64.to_bits());
        assert_eq!(p.vel.z.to_bits(), (-0.2_f64).to_bits());
        assert_eq!(p.vel.y.to_bits(), (-0.0_f64).to_bits());
        p.vel.y = 0.5;
        after_fall_on(&mut p, &w, (0, 0, 0));
        assert_eq!(p.vel.y.to_bits(), 0.0_f64.to_bits());
    }

    #[test]
    fn slime_and_bed_bounce_unless_sneaking() {
        let w = world_with(&[
            ((0, 0, 0), "minecraft:slime_block"),
            ((1, 0, 0), "minecraft:red_bed[facing=south,part=foot]"),
        ]);
        let mut p = player_at(0.5, 1.0, 0.5);
        p.vel = Vec3::new(0.1, -0.9, 0.0);
        after_fall_on(&mut p, &w, (0, 0, 0));
        assert_eq!(p.vel.y, 0.9);
        assert_eq!(p.vel.x, 0.1);
        // a bounce only reverses a downward velocity
        p.vel.y = 0.3;
        after_fall_on(&mut p, &w, (0, 0, 0));
        assert_eq!(p.vel.y, 0.3);
        // bed: -vy * 0.66F * 1.0, with the float promoted to double
        p.vel = Vec3::new(0.0, -0.9, 0.0);
        after_fall_on(&mut p, &w, (1, 0, 0));
        assert_eq!(p.vel.y, 0.9 * f64::from(0.66_f32));
        // sneaking lands like on any other block
        p.shift_key_down = true;
        p.vel = Vec3::new(0.0, -0.9, 0.0);
        after_fall_on(&mut p, &w, (0, 0, 0));
        assert_eq!(p.vel.y, 0.0);
        p.vel = Vec3::new(0.0, -0.9, 0.0);
        after_fall_on(&mut p, &w, (1, 0, 0));
        assert_eq!(p.vel.y, 0.0);
    }

    #[test]
    fn fall_damage_calls_per_block() {
        let w = world_with(&[
            ((0, 0, 0), "minecraft:stone"),
            ((1, 0, 0), "minecraft:hay_block"),
            ((2, 0, 0), "minecraft:slime_block"),
            ((3, 0, 0), "minecraft:white_bed[part=head]"),
            ((4, 0, 0), "minecraft:honey_block"),
            ((5, 0, 0), "minecraft:powder_snow"),
            (
                (6, 0, 0),
                "minecraft:pointed_dripstone[thickness=tip,vertical_direction=up]",
            ),
            (
                (7, 0, 0),
                "minecraft:pointed_dripstone[thickness=tip,vertical_direction=down]",
            ),
        ]);
        let mut p = player_at(0.5, 1.0, 0.5);
        let at = |p: &PlayerState, x| fall_on_effect(p, &w, (x, 0, 0), 10.0);
        let dmg = |distance, multiplier| FallOn::Damage {
            distance,
            multiplier,
        };
        assert_eq!(at(&p, 0), dmg(10.0, 1.0));
        assert_eq!(at(&p, 1), dmg(10.0, 0.2));
        assert_eq!(at(&p, 2), dmg(10.0, 0.0));
        assert_eq!(at(&p, 3), dmg(5.0, 1.0));
        assert_eq!(at(&p, 4), dmg(10.0, 0.2));
        assert_eq!(at(&p, 5), FallOn::NoDamageCall);
        assert_eq!(at(&p, 6), dmg(12.5, 2.0));
        assert_eq!(at(&p, 7), dmg(10.0, 1.0));
        p.shift_key_down = true;
        assert_eq!(at(&p, 2), FallOn::NoDamageCall);
        assert_eq!(at(&p, 0), dmg(10.0, 1.0));
    }

    #[test]
    fn slime_step_slows_a_slow_player_unless_sneaking() {
        let w = world_with(&[((0, 0, 0), "minecraft:slime_block")]);
        let mut p = player_at(0.5, 1.0, 0.5);
        p.vel = Vec3::new(0.2, -0.0784, -0.1);
        step_on(&mut p, &w, (0, 0, 0));
        let e = 0.4 + 0.0784_f64.abs() * 0.2;
        assert_eq!(p.vel, Vec3::new(0.2 * e, -0.0784, -0.1 * e));
        // too fast vertically: untouched
        let mut q = player_at(0.5, 1.0, 0.5);
        q.vel = Vec3::new(0.2, -0.1, 0.0);
        step_on(&mut q, &w, (0, 0, 0));
        assert_eq!(q.vel, Vec3::new(0.2, -0.1, 0.0));
        // sneaking: untouched
        let mut r = player_at(0.5, 1.0, 0.5);
        r.shift_key_down = true;
        r.vel = Vec3::new(0.2, -0.0784, 0.0);
        step_on(&mut r, &w, (0, 0, 0));
        assert_eq!(r.vel, Vec3::new(0.2, -0.0784, 0.0));
    }

    #[test]
    fn on_pos_uses_the_supporting_block_and_special_fence_rows() {
        let w = world_with(&[
            ((0, 0, 0), "minecraft:stone"),
            ((3, 0, 0), "minecraft:oak_fence"),
            ((6, 0, 0), "minecraft:cobblestone_wall"),
        ]);
        let mut p = player_at(0.5, 1.0, 0.5);
        // no support: the block 0.2 below the feet
        assert_eq!(on_pos(&p, &w, 0.2), (0, 0, 0));
        p.pos = Vec3::new(0.5, 1.1, 0.5);
        assert_eq!(on_pos(&p, &w, 0.2), (0, 0, 0));
        // with support: the supporting column, row of the feet minus f
        p.supporting_block = Some((0, 0, 0));
        p.pos = Vec3::new(0.9, 1.0, 0.5);
        assert_eq!(on_pos(&p, &w, 0.2), (0, 0, 0));
        assert_eq!(on_pos(&p, &w, 1.0E-5), (0, 0, 0));
        // fences keep their own row for small f (their collision reaches 1.5 high)
        p.supporting_block = Some((3, 0, 0));
        p.pos = Vec3::new(3.5, 1.6, 0.5);
        assert_eq!(on_pos(&p, &w, 0.2), (3, 0, 0));
        assert_eq!(on_pos(&p, &w, 0.500001), (3, 1, 0));
        // walls always do
        p.supporting_block = Some((6, 0, 0));
        p.pos = Vec3::new(6.5, 1.5, 0.5);
        assert_eq!(on_pos(&p, &w, 0.500001), (6, 0, 0));
    }

    #[test]
    fn honey_slide_formulas() {
        // getOldDeltaY undoes one tick of gravity and drag; getNewDeltaY applies it
        let v = -0.4;
        let old = honey_old_delta_y(v);
        assert_eq!(old, v / f64::from(0.98_f32) + 0.08);
        let mut p = player_at(0.5, 0.5, 0.5);
        p.vel = Vec3::new(0.2, v, -0.1);
        p.fall_distance = 3.0;
        honey_slide_movement(&mut p);
        let d = -0.05 / old;
        assert_eq!(p.vel.x, 0.2 * d);
        assert_eq!(p.vel.z, -0.1 * d);
        assert_eq!(p.vel.y, (-0.05 - 0.08) * f64::from(0.98_f32));
        assert_eq!(p.fall_distance, 0.0);
        // a slow fall keeps the horizontal velocity
        let mut q = player_at(0.5, 0.5, 0.5);
        q.vel = Vec3::new(0.2, -0.1, 0.3);
        honey_slide_movement(&mut q);
        assert_eq!((q.vel.x, q.vel.z), (0.2, 0.3));
    }

    #[test]
    fn honey_only_slides_down_a_side_in_the_air() {
        let mut p = player_at(0.5, 0.5, 0.5);
        // pressed against the side: |dx| = 0.5 - 0.3 + ... use a position near the edge
        p.pos = Vec3::new(0.5 + 0.74, 0.5, 0.5);
        p.vel = Vec3::new(0.0, -0.3, 0.0);
        assert!(is_sliding_down_honey(&p, (0, 0, 0)));
        p.on_ground = true;
        assert!(!is_sliding_down_honey(&p, (0, 0, 0)));
        p.on_ground = false;
        // too high (above the top of the block)
        p.pos.y = 0.9375;
        assert!(!is_sliding_down_honey(&p, (0, 0, 0)));
        p.pos.y = 0.5;
        // not falling fast enough
        p.vel.y = -0.05;
        assert!(!is_sliding_down_honey(&p, (0, 0, 0)));
        p.vel.y = -0.3;
        // centred over the block: not against a side
        p.pos.x = 0.5;
        assert!(!is_sliding_down_honey(&p, (0, 0, 0)));
    }

    #[test]
    fn cobweb_sets_the_stuck_multiplier_and_resets_the_fall() {
        let w = world_with(&[((0, 0, 0), "minecraft:cobweb")]);
        let mut p = player_at(0.5, 0.0, 0.5);
        p.fall_distance = 4.0;
        apply_effects_from_segment(
            &mut p,
            &w,
            Vec3::new(0.5, 1.0, 0.5),
            Vec3::new(0.5, 0.0, 0.5),
        );
        assert_eq!(
            p.stuck_speed_multiplier,
            Vec3::new(0.25, f64::from(0.05_f32), 0.25)
        );
        assert_eq!(p.fall_distance, 0.0);
        // not overlapping: nothing
        let mut q = player_at(5.5, 0.0, 0.5);
        apply_effects_from_segment(
            &mut q,
            &w,
            Vec3::new(5.5, 1.0, 0.5),
            Vec3::new(5.5, 0.0, 0.5),
        );
        assert_eq!(q.stuck_speed_multiplier, Vec3::ZERO);
        // with the weaving effect the slowdown is milder
        let mut r = player_at(0.5, 0.0, 0.5);
        crate::effects::add_effect(&mut r, "minecraft:weaving", 0, 100);
        apply_effects_from_segment(
            &mut r,
            &w,
            Vec3::new(0.5, 0.0, 0.5),
            Vec3::new(0.5, 0.0, 0.5),
        );
        assert_eq!(r.stuck_speed_multiplier, Vec3::new(0.5, 0.25, 0.5));
        // a flying player ignores it
        let mut s = player_at(0.5, 0.0, 0.5);
        s.flying = true;
        apply_effects_from_segment(
            &mut s,
            &w,
            Vec3::new(0.5, 0.0, 0.5),
            Vec3::new(0.5, 0.0, 0.5),
        );
        assert_eq!(s.stuck_speed_multiplier, Vec3::ZERO);
    }

    #[test]
    fn a_move_through_a_web_is_noticed_even_when_it_ends_beyond() {
        // a fast fall passes through a web block and ends below it: still stuck
        let w = world_with(&[((0, 3, 0), "minecraft:cobweb")]);
        let mut p = player_at(0.5, 1.0, 0.5);
        let mut log = MovementLog::new();
        log.record(Movement::new(
            Vec3::new(0.5, 6.0, 0.5),
            Vec3::new(0.5, 1.0, 0.5),
            Some(Vec3::new(0.0, -5.0, 0.0)),
        ));
        apply_effects_from_blocks(&mut p, &w, &mut log, Vec3::new(0.5, 6.0, 0.5));
        assert_eq!(p.stuck_speed_multiplier.x, 0.25);
        assert!(log.is_empty());
    }

    #[test]
    fn powder_snow_freezes_and_sticks_only_inside_it() {
        let w = world_with(&[((0, 0, 0), "minecraft:powder_snow")]);
        // feet inside the block: stuck and freezing
        let mut p = player_at(0.5, 0.2, 0.5);
        let pos = p.pos;
        apply_effects_from_segment(&mut p, &w, pos, pos);
        assert_eq!(
            p.stuck_speed_multiplier,
            Vec3::new(f64::from(0.9_f32), 1.5, f64::from(0.9_f32))
        );
        assert!(p.in_powder_snow);
        assert_eq!(p.ticks_frozen, 1);
        // box overlapping the block from beside it, feet in air next to it: freezing but not stuck
        let mut q = player_at(1.2, 0.2, 0.5);
        let pos = q.pos;
        apply_effects_from_segment(&mut q, &w, pos, pos);
        assert_eq!(q.stuck_speed_multiplier, Vec3::ZERO);
        assert!(q.in_powder_snow);
        // the freeze counter saturates at 140
        let mut r = player_at(0.5, 0.2, 0.5);
        r.ticks_frozen = 140;
        let pos = r.pos;
        apply_effects_from_segment(&mut r, &w, pos, pos);
        assert_eq!(r.ticks_frozen, 140);
        // fire is put out
        let mut s = player_at(0.5, 0.2, 0.5);
        s.remaining_fire_ticks = 100;
        let pos = s.pos;
        apply_effects_from_segment(&mut s, &w, pos, pos);
        assert_eq!(s.remaining_fire_ticks, 0);
    }

    #[test]
    fn powder_snow_effects_are_counted_per_step() {
        // Two powder snow blocks reached at different steps of one move freeze twice; two in the
        // same step freeze once.
        let w = world_with(&[
            ((0, 0, 0), "minecraft:powder_snow"),
            ((0, 0, 1), "minecraft:powder_snow"),
        ]);
        let mut p = player_at(0.5, 0.0, 1.5);
        let mut log = MovementLog::new();
        log.record(Movement::new(
            Vec3::new(0.5, 0.0, 3.5),
            Vec3::new(0.5, 0.0, 1.5),
            Some(Vec3::new(0.0, 0.0, -2.0)),
        ));
        apply_effects_from_blocks(&mut p, &w, &mut log, Vec3::new(0.5, 0.0, 3.5));
        assert!(p.in_powder_snow);
        assert!(p.ticks_frozen >= 1);
        // a stationary box over both blocks: one step, one freeze
        let mut q = player_at(0.5, 0.0, 1.0);
        let pos = q.pos;
        apply_effects_from_segment(&mut q, &w, pos, pos);
        assert_eq!(q.ticks_frozen, 1);
    }

    #[test]
    fn bubble_columns_push_the_player() {
        let w = world_with(&[
            ((0, 0, 0), "minecraft:bubble_column[drag=false]"),
            ((0, 1, 0), "minecraft:bubble_column[drag=false]"),
            ((3, 0, 0), "minecraft:bubble_column[drag=true]"),
        ]);
        // inside a column (water above): +0.06, fall distance reset
        let mut p = player_at(0.5, 0.2, 0.5);
        p.fall_distance = 2.0;
        let pos = p.pos;
        apply_effects_from_segment(&mut p, &w, pos, pos);
        assert!(p.vel.y > 0.0);
        assert_eq!(p.fall_distance, 0.0);
        // at the top of a drag-down column: toward -0.9
        let mut q = player_at(3.5, 0.2, 0.5);
        q.vel = Vec3::new(0.0, -0.89, 0.0);
        let pos = q.pos;
        apply_effects_from_segment(&mut q, &w, pos, pos);
        assert_eq!(q.vel.y, -0.9);
    }

    #[test]
    fn traversal_visits_each_block_once_in_game_order() {
        // A stationary box visits the blocks it overlaps, x fastest then y then z, all at step 0.
        let bb = Aabb::from_corners(0.2, 0.2, 0.2, 1.8, 0.8, 0.8);
        let mut seen = Vec::new();
        let ran =
            for_each_block_intersected_between(Vec3::ZERO, Vec3::ZERO, bb, &mut |pos, step| {
                seen.push((pos, step));
                true
            });
        assert!(ran);
        assert_eq!(seen, vec![((0, 0, 0), 0), ((1, 0, 0), 0)]);
    }

    #[test]
    fn traversal_covers_the_swept_volume() {
        // A 0.6-wide box moving 3 blocks along +x at the end position: every block row crossed is
        // visited exactly once, and stopping early works.
        let bb = Aabb::from_corners(2.7, 0.1, 0.1, 3.3, 1.7, 0.7);
        let from = Vec3::new(0.0, 0.0, 0.0);
        let to = Vec3::new(3.0, 0.0, 0.0);
        let mut seen = Vec::new();
        for_each_block_intersected_between(from, to, bb, &mut |pos, _| {
            seen.push(pos);
            true
        });
        for x in 0..=3 {
            for y in 0..=1 {
                assert_eq!(
                    seen.iter().filter(|p| **p == (x, y, 0)).count(),
                    1,
                    "block ({x}, {y}, 0)"
                );
            }
        }
        let mut count = 0;
        let ran = for_each_block_intersected_between(from, to, bb, &mut |_, _| {
            count += 1;
            count < 3
        });
        assert!(!ran);
        assert_eq!(count, 3);
    }

    #[test]
    fn movement_log_merges_the_oldest_two_at_capacity() {
        let mut log = MovementLog::new();
        for i in 0..100 {
            let f = f64::from(i);
            log.record(Movement::new(
                Vec3::new(f, 0.0, 0.0),
                Vec3::new(f + 1.0, 0.0, 0.0),
                None,
            ));
        }
        assert_eq!(log.len(), 100);
        log.record(Movement::new(
            Vec3::new(100.0, 0.0, 0.0),
            Vec3::new(101.0, 0.0, 0.0),
            None,
        ));
        assert_eq!(log.len(), 100);
        assert_eq!(log.entries[0].from.x, 0.0);
        assert_eq!(log.entries[0].to.x, 2.0);
        log.remove_latest();
        assert_eq!(log.len(), 99);
        log.clear();
        assert!(log.is_empty());
    }

    #[test]
    fn clip_matches_simple_geometry() {
        // a segment through the middle of a unit block enters at its west face
        let hit = clip_block(1, 0, 0, Vec3::new(0.0, 0.5, 0.5), Vec3::new(3.0, 0.5, 0.5));
        assert_eq!(hit, Some(Vec3::new(1.0, 0.5, 0.5)));
        // a segment that stops short misses
        assert_eq!(
            clip_block(1, 0, 0, Vec3::new(0.0, 0.5, 0.5), Vec3::new(0.9, 0.5, 0.5)),
            None
        );
        // and one passing beside it
        assert_eq!(
            clip_block(1, 0, 0, Vec3::new(0.0, 1.5, 0.5), Vec3::new(3.0, 1.5, 0.5)),
            None
        );
    }
}
