//! Block coordinates and the long-packed form the game stores them in.

pub const MIN_Y: i32 = -64;
pub const WORLD_HEIGHT: i32 = 384;
pub const SECTION_COUNT: i32 = WORLD_HEIGHT / 16;

const X_BITS: u32 = 26;
const Z_BITS: u32 = 26;
const Y_BITS: u32 = 64 - X_BITS - Z_BITS;
const XZ_MASK: i64 = (1 << X_BITS) - 1;
const Y_MASK: i64 = (1 << Y_BITS) - 1;
const Z_OFFSET: u32 = Y_BITS;
const X_OFFSET: u32 = Y_BITS + Z_BITS;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub fn as_long(self) -> i64 {
        let x = (self.x as i64 & XZ_MASK) << X_OFFSET;
        let z = (self.z as i64 & XZ_MASK) << Z_OFFSET;
        let y = self.y as i64 & Y_MASK;
        x | z | y
    }

    pub fn from_long(packed: i64) -> Self {
        let x = packed >> X_OFFSET;
        let y = (packed << (64 - Y_BITS)) >> (64 - Y_BITS);
        let z = (packed << (64 - Z_OFFSET - Z_BITS)) >> (64 - Z_BITS);
        Self {
            x: x as i32,
            y: y as i32,
            z: z as i32,
        }
    }
}

/// Index of the chunk section containing `block_y`, counting up from the bottom of the world.
pub fn section_index(block_y: i32) -> i32 {
    (block_y >> 4) - (MIN_Y >> 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_roundtrip() {
        let cases = [
            BlockPos::new(0, 0, 0),
            BlockPos::new(1, 2, 3),
            BlockPos::new(-1, -1, -1),
            BlockPos::new(1000, -50, -2000),
            BlockPos::new((1 << 25) - 1, (1 << 11) - 1, -(1 << 25)),
            BlockPos::new(-(1 << 25), -(1 << 11), (1 << 25) - 1),
        ];
        for p in cases {
            assert_eq!(BlockPos::from_long(p.as_long()), p, "{p:?}");
        }
    }

    #[test]
    fn known_packings() {
        assert_eq!(BlockPos::new(0, 0, 0).as_long(), 0);
        assert_eq!(BlockPos::new(1, 0, 0).as_long(), 1i64 << 38);
        assert_eq!(BlockPos::new(0, 0, 1).as_long(), 1i64 << 12);
        assert_eq!(BlockPos::new(0, 1, 0).as_long(), 1);
    }

    #[test]
    fn section_indices() {
        assert_eq!(section_index(-64), 0);
        assert_eq!(section_index(-49), 0);
        assert_eq!(section_index(-48), 1);
        assert_eq!(section_index(0), 4);
        assert_eq!(section_index(319), 23);
        assert_eq!(SECTION_COUNT, 24);
    }
}
