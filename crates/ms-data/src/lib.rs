//! Generated, version-specific game data: the block-state registry, and later the section
//! palette helpers, per-state collision shapes, attributes, and tags. The tables are produced
//! by `cargo xtask regen-data` from the target version's datagen output, so retargeting is
//! mostly a matter of regenerating rather than rewriting.

#![forbid(unsafe_code)]

mod generated;
mod generated_shapes;

use std::collections::HashMap;
use std::sync::OnceLock;

pub use generated::{BLOCK_COUNT, BLOCK_STATE_COUNT};

const _: () = assert!(BLOCK_COUNT > 1000);
const _: () = assert!(BLOCK_STATE_COUNT > 20_000);

/// Namespaced id of a block, e.g. `"minecraft:stone"`.
pub fn block_name(block: usize) -> &'static str {
    generated::BLOCK_NAMES[block]
}

/// The block's default state id — what it takes when placed with no extra information.
pub fn default_state(block: usize) -> u32 {
    generated::DEFAULT_STATE[block]
}

/// The block that owns `state`. State ids are contiguous per block, so this is a binary search
/// over the per-block first-state ids.
pub fn block_of_state(state: u32) -> usize {
    assert!(state < BLOCK_STATE_COUNT, "state id {state} out of range");
    generated::FIRST_STATE.partition_point(|&first| first <= state) - 1
}

static STATE_SHAPE: &[u8] = include_bytes!("../data/state_shape.bin");

/// Collision boxes of a block state as `[minX, minY, minZ, maxX, maxY, maxZ]` in block-local
/// coordinates. Empty for non-colliding states such as air and plants.
pub fn collision_boxes(state: u32) -> &'static [[f64; 6]] {
    assert!(state < BLOCK_STATE_COUNT, "state id {state} out of range");
    let i = state as usize * 2;
    let shape = u16::from_le_bytes([STATE_SHAPE[i], STATE_SHAPE[i + 1]]) as usize;
    generated_shapes::SHAPES[shape]
}

/// The block's friction (slipperiness): 0.6 for most blocks, 0.98 for ice, 0.8 for slime.
pub fn block_friction(block: usize) -> f32 {
    generated_shapes::BLOCK_FRICTION[block]
}

fn index_by_name() -> &'static HashMap<&'static str, usize> {
    static MAP: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    MAP.get_or_init(|| {
        generated::BLOCK_NAMES
            .iter()
            .enumerate()
            .map(|(i, &n)| (n, i))
            .collect()
    })
}

/// Block index for a namespaced id, or `None` if unknown.
pub fn block_index(name: &str) -> Option<usize> {
    index_by_name().get(name).copied()
}

/// Collision boxes for a block looked up by name, using its default state. Exact for full-cube
/// blocks (all states share the shape); approximate for blocks whose placed state differs from
/// the default (slabs, stairs) until property-aware resolution lands.
pub fn collision_boxes_for_name(name: &str) -> &'static [[f64; 6]] {
    match block_index(name) {
        Some(b) => collision_boxes(default_state(b)),
        None => &[],
    }
}

/// Friction for a block looked up by name (0.6 if unknown).
pub fn friction_for_name(name: &str) -> f32 {
    match block_index(name) {
        Some(b) => block_friction(b),
        None => 0.6,
    }
}

/// Collision boxes for a block's `encoded_description` ("name|prop=val,..."). Multi-shape blocks
/// (slabs, stairs, fences, ...) resolve their exact state; everything else falls back to the
/// (shape-identical) default state by name.
pub fn collision_boxes_for_encoded(encoded: &str) -> &'static [[f64; 6]] {
    match generated_shapes::ENCODED.binary_search_by(|&(k, _)| k.cmp(encoded)) {
        Ok(i) => generated_shapes::SHAPES[generated_shapes::ENCODED[i].1 as usize],
        Err(_) => collision_boxes_for_name(encoded.split('|').next().unwrap_or(encoded)),
    }
}

#[cfg(test)]
mod collision_tests {
    use super::*;

    fn block(name: &str) -> usize {
        (0..BLOCK_COUNT).find(|&b| block_name(b) == name).unwrap()
    }

    #[test]
    fn stone_is_a_full_cube() {
        let stone = block("minecraft:stone");
        assert_eq!(block_friction(stone), 0.6_f32);
        assert_eq!(
            collision_boxes(default_state(stone)),
            &[[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]]
        );
    }

    #[test]
    fn ice_is_slippery() {
        assert_eq!(block_friction(block("minecraft:ice")), 0.98_f32);
    }

    #[test]
    fn air_has_no_collision() {
        assert!(collision_boxes(default_state(block("minecraft:air"))).is_empty());
    }

    #[test]
    fn friction_table_matches_block_count() {
        assert_eq!(generated_shapes::BLOCK_FRICTION.len(), BLOCK_COUNT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn air_is_the_first_block() {
        assert_eq!(block_of_state(0), 0);
        assert_eq!(block_name(0), "minecraft:air");
        assert_eq!(default_state(0), 0);
    }

    #[test]
    fn every_state_maps_to_an_owning_block() {
        for state in 0..BLOCK_STATE_COUNT {
            let block = block_of_state(state);
            assert!(block < BLOCK_COUNT);
            assert!(generated::FIRST_STATE[block] <= state);
            if block + 1 < BLOCK_COUNT {
                assert!(state < generated::FIRST_STATE[block + 1]);
            }
        }
    }

    #[test]
    fn counts_are_consistent() {
        assert_eq!(generated::BLOCK_NAMES.len(), BLOCK_COUNT);
        assert_eq!(generated::FIRST_STATE.len(), BLOCK_COUNT);
        assert_eq!(generated::DEFAULT_STATE.len(), BLOCK_COUNT);
    }
}
