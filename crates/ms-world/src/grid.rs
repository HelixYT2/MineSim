//! A buildable world: a flat base with arbitrary blocks placed on top. This is how custom arenas
//! are made (obstacle courses, pools, ladders) and how the oracle corpus scenarios are rebuilt for
//! replay. Blocks are stored sparsely per 16³ section, so a few thousand placed blocks cost a few
//! hash lookups per query regardless of where they are.

use crate::flat::FlatWorld;
use std::collections::HashMap;

const SECTION: i32 = 16;

#[derive(Clone, Debug)]
pub struct GridWorld {
    base: FlatWorld,
    sections: HashMap<(i32, i32, i32), Box<[u32; 4096]>>,
}

/// Marks "not overridden — use the base world" inside a section.
const UNSET: u32 = u32::MAX;

impl GridWorld {
    /// A world that is `base` everywhere until blocks are placed.
    pub fn new(base: FlatWorld) -> Self {
        Self {
            base,
            sections: HashMap::new(),
        }
    }

    pub fn base(&self) -> &FlatWorld {
        &self.base
    }

    fn split(x: i32, y: i32, z: i32) -> ((i32, i32, i32), usize) {
        let key = (
            x.div_euclid(SECTION),
            y.div_euclid(SECTION),
            z.div_euclid(SECTION),
        );
        let (lx, ly, lz) = (
            x.rem_euclid(SECTION) as usize,
            y.rem_euclid(SECTION) as usize,
            z.rem_euclid(SECTION) as usize,
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
}
