//! The complete simulated state of the player, field for field what the game keeps across ticks
//! (and what the oracle corpus records), plus the per-tick input.
//!
//! Every field here is either read by the next tick's physics or reported to callers; nothing is
//! derived-and-cached. Field names follow the game's (`deltaMovement` → `vel`, `noJumpDelay` →
//! `no_jump_delay`, ...) so the port can be checked against the reference method by method.

use crate::attributes::Attributes;
use crate::effects::Effects;
use ms_numerics::Vec3;

/// The player's pose, which decides its bounding box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Pose {
    #[default]
    Standing,
    /// Sneaking or forced low by a 1.5-block ceiling: 0.6 × 1.5.
    Crouching,
    /// Swimming, or crawling under a 1-block ceiling: 0.6 × 0.6.
    Swimming,
    /// Elytra flight (not simulated beyond the box).
    FallFlying,
    Dying,
}

impl Pose {
    /// Bounding-box `(width, height)` for this pose at scale 1 (`Player.POSES`).
    pub fn dimensions(self) -> (f32, f32) {
        match self {
            Pose::Standing => (0.6, 1.8),
            Pose::Crouching => (0.6, 1.5),
            Pose::Swimming | Pose::FallFlying => (0.6, 0.6),
            Pose::Dying => (0.2, 0.2),
        }
    }

    /// The game's enum name (as the corpus records it).
    pub fn name(self) -> &'static str {
        match self {
            Pose::Standing => "STANDING",
            Pose::Crouching => "CROUCHING",
            Pose::Swimming => "SWIMMING",
            Pose::FallFlying => "FALL_FLYING",
            Pose::Dying => "DYING",
        }
    }

    pub fn from_name(name: &str) -> Option<Pose> {
        Some(match name {
            "STANDING" => Pose::Standing,
            "CROUCHING" => Pose::Crouching,
            "SWIMMING" => Pose::Swimming,
            "FALL_FLYING" => Pose::FallFlying,
            "DYING" => Pose::Dying,
            _ => return None,
        })
    }
}

/// One tick of player input: the seven movement keys and the absolute look direction. This is
/// exactly what a human at the keyboard controls; sprinting and crouching are derived from it by
/// the game's own rules.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Input {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    /// The sneak key.
    pub shift: bool,
    /// The sprint key.
    pub sprint: bool,
    pub yaw: f32,
    pub pitch: f32,
}

/// The player's full state.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerState {
    // ---- Entity
    pub pos: Vec3,
    /// `deltaMovement`.
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub horizontal_collision: bool,
    pub minor_horizontal_collision: bool,
    pub vertical_collision: bool,
    pub vertical_collision_below: bool,
    pub fall_distance: f64,
    /// `wasTouchingWater` (what `isInWater()` returns).
    pub in_water: bool,
    /// `wasEyeInWater` (`isUnderWater()`).
    pub eye_in_water: bool,
    /// `isInLava()`.
    pub in_lava: bool,
    pub water_height: f64,
    pub lava_height: f64,
    pub in_powder_snow: bool,
    pub was_in_powder_snow: bool,
    pub stuck_speed_multiplier: Vec3,
    /// `mainSupportingBlockPos`, as (x, y, z).
    pub supporting_block: Option<(i32, i32, i32)>,
    pub on_ground_no_blocks: bool,
    pub pose: Pose,
    pub sprinting: bool,
    /// `isShiftKeyDown()`.
    pub shift_key_down: bool,
    pub swimming: bool,
    pub remaining_fire_ticks: i32,
    pub ticks_frozen: i32,
    pub tick_count: i32,
    pub invulnerable_time: i32,

    // ---- LivingEntity
    pub health: f32,
    pub absorption: f32,
    pub hurt_time: i32,
    pub last_hurt: f32,
    pub death_time: i32,
    pub no_jump_delay: i32,
    pub jumping: bool,
    pub xxa: f32,
    pub zza: f32,
    /// The `speed` field (`setSpeed`), refreshed from the movement-speed attribute each tick.
    pub speed: f32,
    pub effects: Effects,
    pub attributes: Attributes,

    // ---- Player / LocalPlayer
    pub food: i32,
    pub saturation: f32,
    pub exhaustion: f32,
    pub jump_trigger_time: i32,
    pub sprint_trigger_time: i32,
    pub flying: bool,
    pub crouching: bool,

    /// The server's copy of the player's `deltaMovement`. In vanilla the server does not run the
    /// player's movement; its copy of the velocity only decays and is what knockback is computed
    /// from before the result is sent to the client (which replaces its own velocity with it).
    pub server_vel: Vec3,
    /// The rest of the server-side player that the damage module needs (the server's `onGround`,
    /// a landing waiting to be turned into fall damage). See `damage::ServerState`.
    pub server: crate::damage::ServerState,
}

impl PlayerState {
    /// A player standing at `pos` looking along `yaw`, at rest, with vanilla defaults (full health
    /// and food, base attributes, no effects).
    pub fn new(pos: Vec3, yaw: f32) -> Self {
        Self {
            pos,
            vel: Vec3::ZERO,
            yaw,
            pitch: 0.0,
            on_ground: false,
            horizontal_collision: false,
            minor_horizontal_collision: false,
            vertical_collision: false,
            vertical_collision_below: false,
            fall_distance: 0.0,
            in_water: false,
            eye_in_water: false,
            in_lava: false,
            water_height: 0.0,
            lava_height: 0.0,
            in_powder_snow: false,
            was_in_powder_snow: false,
            stuck_speed_multiplier: Vec3::ZERO,
            supporting_block: None,
            on_ground_no_blocks: false,
            pose: Pose::Standing,
            sprinting: false,
            shift_key_down: false,
            swimming: false,
            remaining_fire_ticks: 0,
            ticks_frozen: 0,
            tick_count: 0,
            invulnerable_time: 0,
            health: 20.0,
            absorption: 0.0,
            hurt_time: 0,
            last_hurt: 0.0,
            death_time: 0,
            no_jump_delay: 0,
            jumping: false,
            xxa: 0.0,
            zza: 0.0,
            speed: 0.1,
            effects: Effects::default(),
            attributes: Attributes::player(),
            food: 20,
            saturation: 5.0,
            exhaustion: 0.0,
            jump_trigger_time: 0,
            sprint_trigger_time: 0,
            flying: false,
            crouching: false,
            server_vel: Vec3::ZERO,
            server: crate::damage::ServerState::default(),
        }
    }

    /// Bounding-box width and height for the current pose and scale.
    pub fn dimensions(&self) -> (f32, f32) {
        let (w, h) = self.pose.dimensions();
        let scale = self.attributes.value(crate::attributes::Attribute::Scale) as f32;
        if scale == 1.0 {
            (w, h)
        } else {
            (w * scale, h * scale)
        }
    }

    pub fn is_alive(&self) -> bool {
        self.health > 0.0
    }

    /// Eye height for the current pose (`Entity.getEyeHeight`; the player's is 1.62 standing,
    /// 1.27 crouching, 0.4 swimming).
    pub fn eye_height(&self) -> f32 {
        let base: f32 = match self.pose {
            Pose::Standing => 1.62,
            Pose::Crouching => 1.27,
            Pose::Swimming | Pose::FallFlying => 0.4,
            Pose::Dying => 0.2 * 0.85,
        };
        let scale = self.attributes.value(crate::attributes::Attribute::Scale) as f32;
        if scale == 1.0 {
            base
        } else {
            base * scale
        }
    }
}
