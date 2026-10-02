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
    /// `ms_data::state_class(state)`, computed once.
    classes: u32,
}

impl FlatWorld {
    /// A floor of block state `state` with its top face at `surface_y`.
    pub fn new(surface_y: i32, state: u32) -> Self {
        Self {
            surface_y,
            state,
            classes: ms_data::state_class(state),
        }
    }

    /// An empty world — air everywhere, nothing to stand on. Useful for free-fall and projectile
    /// tests.
    pub fn void() -> Self {
        Self {
            surface_y: i32::MIN,
            state: ms_data::AIR,
            classes: 0,
        }
    }

    /// The union of the `ms_data::class` bits of every state this world can hold (the floor's;
    /// air has none).
    #[inline]
    pub fn classes(&self) -> u32 {
        self.classes
    }

    pub fn surface_y(&self) -> i32 {
        self.surface_y
    }

    /// The highest `y` at which this world can hold a non-air block (everything above is air).
    #[inline]
    pub fn max_block_y(&self) -> i32 {
        self.surface_y.saturating_sub(1)
    }

    pub fn floor_state(&self) -> u32 {
        self.state
    }

    #[inline]
    pub fn block_state(&self, _x: i32, y: i32, _z: i32) -> u32 {
        if y < self.surface_y {
            self.state
        } else {
            ms_data::AIR
        }
    }
}
