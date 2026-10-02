//! The simulation kernel: the player's per-tick physics (input, sprinting and crouching rules,
//! jumping, travel through air and fluids, climbing, per-axis collision with step-up), status
//! effects and attributes, damage and knockback, block behaviours, and projectiles.

#![forbid(unsafe_code)]

pub mod attributes;
pub mod blocks;
pub mod collision;
pub mod damage;
pub mod effects;
pub mod entity;
pub mod fluids;
pub mod input;
pub mod living;
pub mod mth;
pub mod player;
pub mod projectile;
pub mod state;

pub use state::{Input, PlayerState, Pose};
