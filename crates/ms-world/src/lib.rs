//! World state: paletted chunk sections, the block-state registry, and region/NBT loading.
//!
//! Iteration over entities, block entities, and scheduled ticks has to follow the game's
//! order, which comes from its hashed collections — insertion order is not the same thing.

#![forbid(unsafe_code)]

pub mod aabb;
pub mod anvil;
pub mod coords;
