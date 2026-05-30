//! The simulation kernel: the per-tick phase order and scheduling, entity movement and
//! per-axis collision against block shapes, the player input/physics model, knockback and
//! effects, and projectiles.

#![forbid(unsafe_code)]

pub mod collision;
pub mod player;
