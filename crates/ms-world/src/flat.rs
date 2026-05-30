//! A synthetic flat world: solid below a surface plane, air above.
//!
//! This is the terrain most reinforcement-learning runs actually want — no save files, trivially
//! cloneable, and cheap to query — so the simulator stays self-contained without a Minecraft
//! installation. The block filling the floor is configurable; the default is stone.

/// Solid `block` for every `y < surface_y`, air at and above. `surface_y` is the level the
/// player's feet rest at when standing on the floor (the block directly below is at `surface_y - 1`).
#[derive(Clone, Debug)]
pub struct FlatWorld {
    surface_y: i32,
    encoded: String,
}

impl FlatWorld {
    /// A floor of `block` (a namespaced id or full `encoded_description`) with its top face at
    /// `surface_y`.
    pub fn new(surface_y: i32, block: impl Into<String>) -> Self {
        Self {
            surface_y,
            encoded: block.into(),
        }
    }

    /// An empty world — air everywhere, nothing to stand on. Useful for free-fall and projectile
    /// tests.
    pub fn void() -> Self {
        Self {
            surface_y: i32::MIN,
            encoded: String::new(),
        }
    }

    pub fn block_encoded(&self, _x: i32, y: i32, _z: i32) -> Option<String> {
        (y < self.surface_y).then(|| self.encoded.clone())
    }
}
