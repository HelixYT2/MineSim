//! The block source the kernel queries during collision and friction lookups. It is either a real
//! save read through Anvil regions or a synthetic flat/empty world; both answer the same two
//! questions ("what block is at this coordinate") so the kernel does not care which it is.

use crate::anvil::AnvilWorld;
use crate::flat::FlatWorld;
use std::path::Path;

pub enum World {
    Anvil(AnvilWorld),
    Flat(FlatWorld),
}

impl World {
    /// A world backed by the Anvil region files in `region_dir`.
    pub fn new(region_dir: impl AsRef<Path>) -> Self {
        World::Anvil(AnvilWorld::new(region_dir))
    }

    /// A flat world with a stone floor whose top face is at `surface_y` (so a player spawned with
    /// feet at `surface_y` is standing on the ground).
    pub fn flat(surface_y: i32) -> Self {
        World::Flat(FlatWorld::new(surface_y, "minecraft:stone"))
    }

    /// A flat world floored with `block` (a namespaced id such as `"minecraft:ice"`, or a full
    /// encoded description).
    pub fn flat_of(surface_y: i32, block: impl Into<String>) -> Self {
        World::Flat(FlatWorld::new(surface_y, block))
    }

    /// An empty world — air everywhere, nothing to stand on.
    pub fn void() -> Self {
        World::Flat(FlatWorld::void())
    }

    /// The block's `encoded_description` at a coordinate, or `None` for air / out of range.
    pub fn block_encoded(&self, x: i32, y: i32, z: i32) -> Option<String> {
        match self {
            World::Anvil(w) => w.block_encoded(x, y, z),
            World::Flat(w) => w.block_encoded(x, y, z),
        }
    }

    /// The namespaced block id at a coordinate (the name part of the encoded description).
    pub fn block_name(&self, x: i32, y: i32, z: i32) -> Option<String> {
        self.block_encoded(x, y, z)
            .map(|e| e.split('|').next().unwrap_or("").to_string())
    }
}
