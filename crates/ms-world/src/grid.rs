//! A buildable world: a flat base with arbitrary blocks placed on top. This is how custom arenas
//! are made (obstacle courses, pools, ladders) and how the oracle corpus scenarios are rebuilt for
//! replay. Blocks are stored sparsely per 16³ section, so a few thousand placed blocks cost a few
//! hash lookups per query regardless of where they are.

use crate::flat::FlatWorld;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

const SECTION_SHIFT: u32 = 4;
const SECTION_MASK: i32 = (1 << SECTION_SHIFT) - 1;

/// A multiplicative hasher for the small integer section keys. The default SipHash costs more
/// than the rest of a block lookup; the map is only ever probed and filled, never iterated, so the
/// hash function cannot influence any result.
#[derive(Default, Clone, Copy)]
struct SectionHasher(u64);

impl Hasher for SectionHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0 ^ (self.0 >> 32)
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(u64::from(b));
        }
    }

    #[inline]
    fn write_i32(&mut self, i: i32) {
        self.write_u64(i as u32 as u64);
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

type SectionMap = HashMap<(i32, i32, i32), Box<[u32; 4096]>, BuildHasherDefault<SectionHasher>>;

#[derive(Clone, Debug)]
pub struct GridWorld {
    base: FlatWorld,
    sections: SectionMap,
    /// The union of the `ms_data::class` bits of the base and of every state ever placed. Placing
    /// a block can only add bits (a block that is later overwritten keeps its bits), so the set
    /// stays a conservative superset of what the world holds.
    classes: u32,
    /// The highest `y` of the base's floor or of any non-air block ever placed: every cell above
    /// it is air. Like `classes`, it only grows.
    max_y: i32,
    /// The smallest box (inclusive corners) holding every block with a `RING_RELEVANT` class that
    /// was ever placed; inverted (`min > max`) while there is none. Only grows.
    ring_min: [i32; 3],
    ring_max: [i32; 3],
}

/// Marks "not overridden — use the base world" inside a section.
const UNSET: u32 = u32::MAX;

impl GridWorld {
    /// A world that is `base` everywhere until blocks are placed.
    pub fn new(base: FlatWorld) -> Self {
        Self {
            base,
            sections: HashMap::default(),
            classes: base.classes(),
            max_y: base.max_block_y(),
            ring_min: [i32::MAX; 3],
            ring_max: [i32::MIN; 3],
        }
    }

    pub fn base(&self) -> &FlatWorld {
        &self.base
    }

    /// The union of the `ms_data::class` bits of every state this world can hold.
    #[inline]
    pub fn classes(&self) -> u32 {
        self.classes
    }

    /// The highest `y` at which this world can hold a non-air block (everything above is air).
    #[inline]
    pub fn max_block_y(&self) -> i32 {
        self.max_y
    }

    /// Whether a block with a `ms_data::class::RING_RELEVANT` class (a shape larger than its cube,
    /// a moving piston) may lie in the box with inclusive corners `lo` and `hi`.
    #[inline]
    pub fn ring_relevant_in(&self, lo: [i32; 3], hi: [i32; 3]) -> bool {
        if self.base.classes() & ms_data::class::RING_RELEVANT != 0 {
            return true;
        }
        (0..3).all(|i| self.ring_min[i] <= hi[i] && self.ring_max[i] >= lo[i])
    }

    #[inline]
    fn split(x: i32, y: i32, z: i32) -> ((i32, i32, i32), usize) {
        let key = (x >> SECTION_SHIFT, y >> SECTION_SHIFT, z >> SECTION_SHIFT);
        let (lx, ly, lz) = (
            (x & SECTION_MASK) as usize,
            (y & SECTION_MASK) as usize,
            (z & SECTION_MASK) as usize,
        );
        (key, (ly << 8) | (lz << 4) | lx)
    }

    /// Place block state `state` (air included) at a coordinate.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) {
        let (key, i) = Self::split(x, y, z);
        let section = self
            .sections
            .entry(key)
            .or_insert_with(|| Box::new([UNSET; 4096]));
        section[i] = state;
        self.classes |= ms_data::state_class(state);
        if state != ms_data::AIR {
            self.max_y = self.max_y.max(y);
        }
        if ms_data::state_class(state) & ms_data::class::RING_RELEVANT != 0 {
            for (i, c) in [x, y, z].into_iter().enumerate() {
                self.ring_min[i] = self.ring_min[i].min(c);
                self.ring_max[i] = self.ring_max[i].max(c);
            }
        }
    }

    /// Fill the box between two corners (inclusive) with `state`.
    pub fn fill(&mut self, a: (i32, i32, i32), b: (i32, i32, i32), state: u32) {
        for x in a.0.min(b.0)..=a.0.max(b.0) {
            for y in a.1.min(b.1)..=a.1.max(b.1) {
                for z in a.2.min(b.2)..=a.2.max(b.2) {
                    self.set_block(x, y, z, state);
                }
            }
        }
    }

    #[inline]
    pub fn block_state(&self, x: i32, y: i32, z: i32) -> u32 {
        let (key, i) = Self::split(x, y, z);
        match self.sections.get(&key) {
            Some(s) if s[i] != UNSET => s[i],
            _ => self.base.block_state(x, y, z),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placed_blocks_override_the_base() {
        let stone = ms_data::parse_state("minecraft:stone").unwrap();
        let ice = ms_data::parse_state("minecraft:ice").unwrap();
        let mut w = GridWorld::new(FlatWorld::new(0, stone));
        assert_eq!(w.block_state(5, -1, 5), stone);
        assert_eq!(w.block_state(5, 0, 5), ms_data::AIR);
        w.set_block(5, -1, 5, ice);
        w.set_block(-17, 40, -33, stone);
        w.set_block(3, -1, 3, ms_data::AIR);
        assert_eq!(w.block_state(5, -1, 5), ice);
        assert_eq!(w.block_state(-17, 40, -33), stone);
        assert_eq!(w.block_state(3, -1, 3), ms_data::AIR);
        assert_eq!(w.block_state(4, -1, 4), stone);
        w.fill((0, 0, 0), (1, 1, 1), ice);
        assert_eq!(w.block_state(1, 1, 0), ice);
    }

    #[test]
    fn section_split_matches_euclidean_division() {
        for x in [
            -33,
            -17,
            -16,
            -15,
            -1,
            0,
            1,
            15,
            16,
            17,
            40,
            i32::MIN,
            i32::MAX,
        ] {
            let (key, i) = GridWorld::split(x, x, x);
            assert_eq!(key.0, x.div_euclid(16));
            assert_eq!(i & 15, x.rem_euclid(16) as usize);
            assert_eq!((i >> 4) & 15, x.rem_euclid(16) as usize);
            assert_eq!(i >> 8, x.rem_euclid(16) as usize);
        }
    }

    #[test]
    fn max_block_y_tracks_the_floor_and_placed_blocks() {
        let stone = ms_data::parse_state("minecraft:stone").unwrap();
        let mut w = GridWorld::new(FlatWorld::new(0, stone));
        assert_eq!(w.max_block_y(), -1);
        w.set_block(3, -5, 3, stone);
        assert_eq!(w.max_block_y(), -1);
        w.set_block(3, 7, 3, ms_data::AIR);
        assert_eq!(w.max_block_y(), -1);
        w.set_block(3, 7, 3, stone);
        assert_eq!(w.max_block_y(), 7);
        w.set_block(3, 7, 3, ms_data::AIR);
        assert_eq!(
            w.max_block_y(),
            7,
            "a removed block keeps the bound conservative"
        );
        let void = GridWorld::new(FlatWorld::void());
        assert_eq!(void.max_block_y(), i32::MIN);
        // Everything above the bound really is air.
        for y in (w.max_block_y() + 1)..(w.max_block_y() + 40) {
            assert_eq!(w.block_state(3, y, 3), ms_data::AIR);
        }
    }

    #[test]
    fn ring_bounds_follow_the_placed_large_shapes() {
        let stone = ms_data::parse_state("minecraft:stone").unwrap();
        let fence = ms_data::parse_state("minecraft:oak_fence").unwrap();
        let mut w = GridWorld::new(FlatWorld::new(0, stone));
        assert!(!w.ring_relevant_in([-100, -100, -100], [100, 100, 100]));
        w.set_block(5, 0, 5, stone);
        assert!(!w.ring_relevant_in([-100, -100, -100], [100, 100, 100]));
        w.set_block(30, 2, -4, fence);
        assert!(w.ring_relevant_in([30, 2, -4], [30, 2, -4]));
        assert!(w.ring_relevant_in([20, 0, -10], [31, 5, 0]));
        assert!(!w.ring_relevant_in([-4, -4, -4], [4, 4, 4]));
        assert!(!w.ring_relevant_in([31, 0, -10], [40, 5, 0]));
        assert!(!w.ring_relevant_in([20, 3, -10], [31, 5, 0]));
        w.set_block(-8, 0, 9, fence);
        assert!(w.ring_relevant_in([-9, 0, 9], [-8, 0, 10]));
        // A floor of fences is relevant everywhere.
        let floor = GridWorld::new(FlatWorld::new(0, fence));
        assert!(floor.ring_relevant_in([0, 0, 0], [0, 0, 0]));
    }

    #[test]
    fn classes_only_ever_grow() {
        let stone = ms_data::parse_state("minecraft:stone").unwrap();
        let water = ms_data::parse_state("minecraft:water").unwrap();
        let fence = ms_data::parse_state("minecraft:oak_fence").unwrap();
        let mut w = GridWorld::new(FlatWorld::new(0, stone));
        assert_eq!(w.classes(), 0);
        w.set_block(0, 3, 0, ms_data::AIR);
        w.set_block(0, 3, 0, stone);
        assert_eq!(w.classes(), 0);
        w.set_block(1, 3, 0, water);
        assert_ne!(w.classes() & ms_data::class::FLUID, 0);
        assert_eq!(w.classes() & ms_data::class::LARGE_SHAPE, 0);
        // Overwriting the water keeps the (conservative) bit.
        w.set_block(1, 3, 0, ms_data::AIR);
        assert_ne!(w.classes() & ms_data::class::FLUID, 0);
        w.fill((2, 0, 0), (3, 1, 1), fence);
        assert_ne!(w.classes() & ms_data::class::LARGE_SHAPE, 0);
        // A floor of water starts with the bit set.
        assert_ne!(
            GridWorld::new(FlatWorld::new(0, water)).classes() & ms_data::class::FLUID,
            0
        );
    }
}
