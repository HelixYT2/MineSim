//! The embeddable simulation: an [`Arena`] owns a world and a player, advances it one client tick at
//! a time from an [`Action`] (the keys and look direction of that tick), and exposes the state for
//! snapshotting and hashing. The physics is the kernel's [`ms_kernel::player::tick`]: sprinting,
//! crouching and the rest of the player rules are derived from the keys exactly as the game does,
//! so an action is what a human at the keyboard controls and nothing more.

use ms_kernel::player;
use ms_numerics::Vec3;
use ms_oracle::StateBuf;
use ms_world::World;

mod batch;
pub use batch::BatchArena;

pub use ms_kernel::{Input, PlayerState, Pose};

/// One tick of agent input: the seven movement keys and the absolute look direction.
pub type Action = Input;

/// The player's simulated state (everything the kernel keeps across ticks).
pub type Player = PlayerState;

pub struct Arena {
    world: World,
    pub player: Player,
}

impl Arena {
    /// Create an arena over `world` with the player at `pos` looking along `yaw`, at rest.
    pub fn new(world: World, pos: Vec3, yaw: f32) -> Self {
        Self {
            world,
            player: PlayerState::new(pos, yaw),
        }
    }

    /// Reset the player to a position/orientation, at rest (full health and food, no effects).
    pub fn reset(&mut self, pos: Vec3, yaw: f32) {
        self.player = PlayerState::new(pos, yaw);
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    /// Advance one tick.
    pub fn step(&mut self, action: &Action) {
        player::tick(&mut self.player, action, &self.world);
    }

    /// Snapshot the player state (for checkpoint/restore).
    pub fn get_state(&self) -> Player {
        self.player.clone()
    }

    /// Restore a previously snapshotted player state.
    pub fn set_state(&mut self, player: Player) {
        self.player = player;
    }

    /// The canonical per-tick state hash (`docs/contract.md`). The fields, in serialization
    /// order: position (3 x f64), velocity (3 x f64), fall distance (f64); yaw, pitch, `xxa`,
    /// `yya`, `zza` (f32); the flags on ground, horizontal collision, vertical collision, sprinting,
    /// swimming, in water, in lava and no-physics (one byte each); the integers no-jump delay, tick
    /// count and food level (i32), and the pose ordinal (u32).
    pub fn state_hash(&self) -> u64 {
        let p = &self.player;
        let pose = match p.pose {
            Pose::Standing => 0_u32,
            Pose::Crouching => 1,
            Pose::Swimming => 2,
            Pose::FallFlying => 3,
            Pose::Dying => 4,
        };
        let mut buf = StateBuf::new();
        buf.push_f64(p.pos.x)
            .push_f64(p.pos.y)
            .push_f64(p.pos.z)
            .push_f64(p.vel.x)
            .push_f64(p.vel.y)
            .push_f64(p.vel.z)
            .push_f64(p.fall_distance)
            .push_f32(p.yaw)
            .push_f32(p.pitch)
            .push_f32(p.xxa)
            .push_f32(0.0) // yya: always zero for the player
            .push_f32(p.zza)
            .push_bool(p.on_ground)
            .push_bool(p.horizontal_collision)
            .push_bool(p.vertical_collision)
            .push_bool(p.sprinting)
            .push_bool(p.swimming)
            .push_bool(p.in_water)
            .push_bool(p.in_lava)
            .push_bool(false) // noPhysics
            .push_i32(p.no_jump_delay)
            .push_i32(p.tick_count)
            .push_i32(p.food)
            .push_u32(pose);
        buf.hash()
    }
}
