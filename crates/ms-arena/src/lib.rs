//! The embeddable simulation: an [`Arena`] owns a world and a player, advances it one game tick at a
//! time from an [`Action`] (the keys and look direction of that tick), and exposes the state for
//! snapshotting and hashing. The physics is the kernel's [`ms_kernel::player::tick`]: sprinting,
//! crouching and the rest of the player rules are derived from the keys exactly as the game does,
//! so an action is what a human at the keyboard controls and nothing more.
//!
//! A tick runs in vanilla's order: the client's tick of the player, then the server's handling of
//! the move packet it sent (landings turn into fall damage here), the motion sync (a hit's
//! knockback replaces the player's velocity here), and the server's own tick of its copy of the
//! player. Damage and effects applied between ticks act on the server side and reach the player
//! with the same one-tick delay they have in the game.

use ms_kernel::damage::{self, DamageSource, TickStart};
use ms_kernel::{effects, player};
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

    /// Replace the world (the player is left where it is).
    pub fn set_world(&mut self, world: World) {
        self.world = world;
    }

    /// Place a block state in this arena's world. A flat world becomes a built one on the first
    /// edit; a world shared with other arenas is copied on write, so the others are unaffected.
    /// Saves loaded from Anvil regions are read-only.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) -> Result<(), String> {
        if let World::Flat(flat) = &self.world {
            self.world = World::grid(ms_world::GridWorld::new(*flat));
        }
        match &mut self.world {
            World::Grid(grid) => {
                std::sync::Arc::make_mut(grid).set_block(x, y, z, state);
                Ok(())
            }
            _ => Err("blocks cannot be placed in a world loaded from region files".into()),
        }
    }

    /// Advance one game tick.
    pub fn step(&mut self, action: &Action) {
        let p = &mut self.player;
        let start = TickStart::of(p);
        player::tick(p, action, &self.world);
        damage::server_move_packet(p, &start, &self.world);
        damage::sync_motion(p);
        damage::server_do_tick(p, &self.world);
    }

    /// Whether the player is dead (health at zero). The physics keeps running; an environment
    /// usually ends the episode here.
    pub fn is_dead(&self) -> bool {
        !self.player.is_alive()
    }

    /// Give the player a status effect (`"minecraft:speed"`, ...), as `/effect give` would.
    /// Returns whether anything changed (a weaker or shorter effect does not replace a stronger
    /// one).
    pub fn add_effect(&mut self, id: &str, amplifier: i32, duration: i32) -> bool {
        effects::add_effect(&mut self.player, id, amplifier, duration)
    }

    pub fn remove_effect(&mut self, id: &str) -> bool {
        effects::remove_effect(&mut self.player, id)
    }

    pub fn clear_effects(&mut self) -> bool {
        effects::clear_effects(&mut self.player)
    }

    /// Hurt the player as a mob attack from the point `(x, z)` would: damage with the
    /// invulnerability window, and the standard knockback away from that point (delivered at the
    /// end of the next tick, as in the game). Returns whether the hit landed.
    pub fn hurt_from(&mut self, amount: f32, x: f64, z: f64) -> bool {
        damage::hurt(&mut self.player, DamageSource::Point { x, z }, amount)
    }

    /// Damage without a source position (no knockback).
    pub fn hurt(&mut self, amount: f32) -> bool {
        damage::hurt(&mut self.player, DamageSource::Generic, amount)
    }

    /// A bare knockback of `strength` along `(dx, dz)` reversed (`LivingEntity.knockback`), as an
    /// attack's knockback enchantment or an explosion-free shove would apply it.
    pub fn knockback(&mut self, strength: f64, dx: f64, dz: f64) {
        damage::knockback(&mut self.player, strength, dx, dz);
    }

    /// Snapshot the player state (for checkpoint/restore).
    pub fn get_state(&self) -> Player {
        self.player.clone()
    }

    /// Restore a previously snapshotted player state.
    pub fn set_state(&mut self, player: Player) {
        self.player = player;
    }

    /// Place the player at `pos` with velocity `vel` (a teleport): the server's copy of the player
    /// is synchronised to it.
    pub fn teleport(&mut self, pos: Vec3, vel: Vec3, yaw: f32, pitch: f32) {
        let p = &mut self.player;
        p.pos = pos;
        p.vel = vel;
        p.yaw = yaw;
        p.pitch = pitch;
        p.fall_distance = 0.0;
        damage::reset_server_copy(p);
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

#[cfg(test)]
mod tests {
    use super::*;
    use ms_world::{FlatWorld, GridWorld};

    fn idle() -> Action {
        Action::default()
    }

    fn settle(a: &mut Arena) {
        for _ in 0..5 {
            a.step(&idle());
        }
    }

    #[test]
    fn falling_off_a_tower_hurts_after_landing() {
        // A 10-block tower: stepping off it falls 10 blocks, 10 - 3 = 7 damage.
        let stone = ms_data::parse_state("minecraft:stone").unwrap();
        let mut grid = GridWorld::new(FlatWorld::new(0, stone));
        grid.fill((0, 0, 0), (0, 9, 0), stone);
        let mut a = Arena::new(World::grid(grid), Vec3::new(0.5, 10.0, 0.5), 0.0);
        settle(&mut a);
        assert_eq!(a.player.health, 20.0);
        let walk = Action {
            forward: true,
            ..idle()
        };
        for _ in 0..10 {
            a.step(&walk);
        }
        for _ in 0..60 {
            a.step(&idle());
        }
        assert!(a.player.on_ground && a.player.pos.y == 0.0);
        assert_eq!(a.player.health, 13.0);
    }

    #[test]
    fn a_hit_knocks_back_on_the_next_tick() {
        let mut a = Arena::new(World::flat(0), Vec3::new(0.5, 0.0, 0.5), 0.0);
        settle(&mut a);
        assert!(a.hurt_from(2.0, 0.5, 3.5));
        assert_eq!(a.player.health, 18.0);
        // The hit lands between ticks; its knockback reaches the player at the end of the next one.
        a.step(&idle());
        assert!(
            a.player.vel.z < -0.3,
            "pushed away from +z: {:?}",
            a.player.vel
        );
        assert!(a.player.vel.y > 0.3);
        // Inside the invulnerability window a weaker hit does nothing.
        assert!(!a.hurt_from(1.0, 0.5, 3.5));
        assert_eq!(a.player.health, 18.0);
    }

    #[test]
    fn speed_effect_makes_walking_faster() {
        let walk = Action {
            forward: true,
            ..idle()
        };
        let mut plain = Arena::new(World::flat(0), Vec3::new(0.5, 0.0, 0.5), 0.0);
        let mut fast = Arena::new(World::flat(0), Vec3::new(0.5, 0.0, 0.5), 0.0);
        assert!(fast.add_effect("minecraft:speed", 1, 1000));
        for _ in 0..40 {
            plain.step(&walk);
            fast.step(&walk);
        }
        assert!(fast.player.pos.z > plain.player.pos.z * 1.3);
    }
}
