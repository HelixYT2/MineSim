//! The block source the kernel queries during collision, friction, fluid and climbing lookups. It
//! is a real save read through Anvil regions, a synthetic flat/empty world, or a flat world with
//! blocks placed on it; all answer the same question ("which block state is at this coordinate")
//! so the kernel does not care which it is.

use crate::anvil::AnvilWorld;
use crate::flat::FlatWorld;
use crate::grid::GridWorld;
use std::path::Path;
use std::sync::Arc;

#[derive(Clone)]
pub enum World {
    Anvil(Arc<AnvilWorld>),
    Flat(FlatWorld),
    /// Shared so that a batch of environments can use one built arena without copying it.
    Grid(Arc<GridWorld>),
}

impl World {
    /// A world backed by the Anvil region files in `region_dir`.
    pub fn new(region_dir: impl AsRef<Path>) -> Self {
        World::Anvil(Arc::new(AnvilWorld::new(region_dir)))
    }

    /// A flat world with a stone floor whose top face is at `surface_y` (so a player spawned with
    /// feet at `surface_y` is standing on the ground).
    pub fn flat(surface_y: i32) -> Self {
        World::Flat(FlatWorld::new(surface_y, stone()))
    }

    /// A flat world floored with `block` (a block id such as `"minecraft:ice"`, or a full state
    /// such as `"minecraft:snow[layers=4]"`). Unknown blocks are an error.
    pub fn flat_of(surface_y: i32, block: &str) -> Result<Self, String> {
        let state =
            ms_data::parse_state(block).ok_or_else(|| format!("unknown block state '{block}'"))?;
        Ok(World::Flat(FlatWorld::new(surface_y, state)))
    }

    /// An empty world — air everywhere, nothing to stand on.
    pub fn void() -> Self {
        World::Flat(FlatWorld::void())
    }

    /// A built world: `grid` (a flat base plus placed blocks).
    pub fn grid(grid: GridWorld) -> Self {
        World::Grid(Arc::new(grid))
    }

    /// The block state id at a coordinate (`ms_data::AIR` for air or out of range).
    #[inline]
    pub fn block_state(&self, x: i32, y: i32, z: i32) -> u32 {
        match self {
            World::Anvil(w) => w.block_state(x, y, z),
            World::Flat(w) => w.block_state(x, y, z),
            World::Grid(w) => w.block_state(x, y, z),
        }
    }

    /// The union of the `ms_data::class` bits of every block state this world can hold: if a bit is
    /// clear, no block of that class exists anywhere in it, so a scan for such blocks can be
    /// skipped (it would have found nothing and changed nothing). Tracked as blocks are placed for
    /// flat and built worlds; a world read from region files could hold anything and reports every
    /// bit.
    #[inline]
    pub fn classes(&self) -> u32 {
        match self {
            World::Anvil(_) => ms_data::class::ALL,
            World::Flat(w) => w.classes(),
            World::Grid(w) => w.classes(),
        }
    }

    /// The highest `y` at which the world can hold a non-air block: every cell above it is air.
    /// Exact for a flat floor, a conservative bound for a built world (it never shrinks when a
    /// block is removed), and unbounded for a world read from region files.
    #[inline]
    pub fn max_block_y(&self) -> i32 {
        match self {
            World::Anvil(_) => i32::MAX,
            World::Flat(w) => w.max_block_y(),
            World::Grid(w) => w.max_block_y(),
        }
    }

    /// Whether a block whose collision shape the outer ring of the `BlockCollisions` cursor
    /// would look at (`ms_data::class::RING_RELEVANT`) may lie in the box with inclusive corners
    /// `lo` and `hi`. Tracked as blocks are placed in built worlds; a world read from region files
    /// could hold one anywhere.
    #[inline]
    pub fn ring_relevant_in(&self, lo: [i32; 3], hi: [i32; 3]) -> bool {
        match self {
            World::Anvil(_) => true,
            World::Flat(w) => w.classes() & ms_data::class::RING_RELEVANT != 0,
            World::Grid(w) => w.ring_relevant_in(lo, hi),
        }
    }

    /// Whether the world may hold a block state with any of the `ms_data::class` bits in `mask`.
    #[inline]
    pub fn may_contain(&self, mask: u32) -> bool {
        self.classes() & mask != 0
    }

    /// The block (registry index) at a coordinate.
    #[inline]
    pub fn block(&self, x: i32, y: i32, z: i32) -> usize {
        ms_data::block_of_state(self.block_state(x, y, z))
    }

    /// The namespaced block id at a coordinate.
    pub fn block_name(&self, x: i32, y: i32, z: i32) -> &'static str {
        ms_data::block_name(self.block(x, y, z))
    }
}

fn stone() -> u32 {
    ms_data::parse_state("minecraft:stone").expect("stone is in every version")
}
