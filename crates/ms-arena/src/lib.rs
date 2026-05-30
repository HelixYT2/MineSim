//! The embeddable simulation: an [`Arena`] owns a world and a player, advances it one tick at a
//! time from an [`Action`], and exposes state for snapshotting and hashing. Scope today is
//! grounded movement on inert terrain (the validated kernel); fluids, entities, and the batched
//! parallel API build on top of this.

use ms_kernel::player::{self, Keys};
use ms_numerics::Vec3;
use ms_oracle::StateBuf;
use ms_world::anvil::World;

/// The player's simulated state.
#[derive(Clone, Copy, Debug)]
pub struct Player {
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub on_ground: bool,
    pub no_jump_delay: i32,
}

/// One tick of agent input. `sprinting`/`sneaking` are exposed as direct boolean actions rather
/// than derived from a key + conditions, which suits a reinforcement-learning action space.
#[derive(Clone, Copy, Debug)]
pub struct Action {
    pub keys: Keys,
    pub jump: bool,
    pub sprinting: bool,
    pub sneaking: bool,
    pub yaw: f32,
}

pub struct Arena {
    world: World,
    pub player: Player,
}

impl Arena {
    /// Create an arena over `world` with the player at `pos` looking along `yaw`, at rest.
    pub fn new(world: World, pos: Vec3, yaw: f32) -> Self {
        Self {
            world,
            player: Player {
                pos,
                vel: Vec3::ZERO,
                yaw,
                on_ground: false,
                no_jump_delay: 0,
            },
        }
    }

    /// Reset the player to a position/orientation, at rest.
    pub fn reset(&mut self, pos: Vec3, yaw: f32) {
        self.player = Player {
            pos,
            vel: Vec3::ZERO,
            yaw,
            on_ground: false,
            no_jump_delay: 0,
        };
    }

    /// Advance one tick.
    pub fn step(&mut self, action: &Action) {
        self.player.yaw = action.yaw;
        let (pos, vel, on_ground) = player::step(
            self.player.pos,
            self.player.vel,
            action.yaw,
            self.player.on_ground,
            action.sprinting,
            action.sneaking,
            action.keys,
            action.jump,
            &mut self.player.no_jump_delay,
            &self.world,
        );
        self.player.pos = pos;
        self.player.vel = vel;
        self.player.on_ground = on_ground;
    }

    /// Snapshot the player state (for checkpoint/restore).
    pub fn get_state(&self) -> Player {
        self.player
    }

    /// Restore a previously snapshotted player state.
    pub fn set_state(&mut self, player: Player) {
        self.player = player;
    }

    /// The canonical per-tick state hash (`docs/contract.md`).
    pub fn state_hash(&self) -> u64 {
        let p = &self.player;
        let mut buf = StateBuf::new();
        buf.push_f64(p.pos.x)
            .push_f64(p.pos.y)
            .push_f64(p.pos.z)
            .push_f64(p.vel.x)
            .push_f64(p.vel.y)
            .push_f64(p.vel.z)
            .push_f32(p.yaw)
            .push_bool(p.on_ground);
        buf.hash()
    }
}
