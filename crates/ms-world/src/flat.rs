//! A synthetic flat world: solid below a surface plane, air above.
//!
//! This is the terrain most reinforcement-learning runs actually want — no save files, trivially
//! cloneable, and cheap to query — so the simulator stays self-contained without a Minecraft
//! installation. The block filling the floor is configurable; the default is stone.

/// Solid `state` for every `y < surface_y`, air at and above. `surface_y` is the level the
/// player's feet rest at when standing on the floor (the block directly below is at `surface_y - 1`).
#[derive(Clone, Copy, Debug)]
pub struct FlatWorld {
    surface_y: i32,
    state: u32,
}

impl FlatWorld {
    /// A floor of block state `state` with its top face at `surface_y`.
    pub fn new(surface_y: i32, state: u32) -> Self {
        Self { surface_y, state }
    }

    /// An empty world — air everywhere, nothing to stand on. Useful for free-fall and projectile
    /// tests.
    pub fn void() -> Self {
        Self {
            surface_y: i32::MIN,
            state: ms_data::AIR,
        }
    }

    pub fn surface_y(&self) -> i32 {
        self.surface_y
    }

    pub fn floor_state(&self) -> u32 {
        self.state
    }

    pub fn block_state(&self, _x: i32, y: i32, _z: i32) -> u32 {
        if y < self.surface_y {
            self.state
        } else {
            ms_data::AIR
        }
    }
}
