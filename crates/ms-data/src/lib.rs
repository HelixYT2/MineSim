//! Generated, version-specific game data: the block-state registry with every block's
//! properties, per-state collision shapes, fluids and suffocation, and per-block friction,
//! speed/jump factors, implementing class and tags. The tables are produced by
//! `cargo xtask regen-data` from the target version's datagen output, so retargeting is mostly a
//! matter of regenerating rather than rewriting.
//!
//! Everything is keyed by the game's numeric block-state id (`u32`, air = 0). Text forms —
//! `"minecraft:oak_stairs[facing=east,half=bottom]"` (command syntax) and
//! `"minecraft:oak_stairs|facing=east,half=bottom"` (the Anvil reader's form) — are parsed with
//! [`parse_state`].

#![forbid(unsafe_code)]

mod generated;
mod generated_shapes;

use std::collections::HashMap;
use std::sync::OnceLock;

pub use generated::{BLOCK_COUNT, BLOCK_STATE_COUNT};

const _: () = assert!(BLOCK_COUNT > 1000);
const _: () = assert!(BLOCK_STATE_COUNT > 20_000);

/// The air block state.
pub const AIR: u32 = 0;

/// Namespaced id of a block, e.g. `"minecraft:stone"`.
pub fn block_name(block: usize) -> &'static str {
    generated::BLOCK_NAMES[block]
}

/// The block's default state id — what it takes when placed with no extra information.
pub fn default_state(block: usize) -> u32 {
    generated::DEFAULT_STATE[block]
}

/// The block that owns `state` (a table lookup; the physics asks this for every block it touches).
#[inline]
pub fn block_of_state(state: u32) -> usize {
    state_blocks()[state as usize] as usize
}

/// Per state, its block index. State ids are contiguous per block, so this is built once from the
/// per-block first-state ids.
fn state_blocks() -> &'static [u16] {
    static TABLE: OnceLock<Vec<u16>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = vec![0u16; BLOCK_STATE_COUNT as usize];
        for (block, &first) in generated::FIRST_STATE.iter().enumerate() {
            let end = generated::FIRST_STATE
                .get(block + 1)
                .copied()
                .unwrap_or(BLOCK_STATE_COUNT);
            for s in first..end {
                t[s as usize] = block as u16;
            }
        }
        t
    })
}

static STATE_SHAPE: &[u8] = include_bytes!("../data/state_shape.bin");
static STATE_FLAGS: &[u8] = include_bytes!("../data/state_flags.bin");

const _: () = assert!(STATE_SHAPE.len() == BLOCK_STATE_COUNT as usize * 2);
const _: () = assert!(STATE_FLAGS.len() == BLOCK_STATE_COUNT as usize * 2);

/// Collision boxes of a block state as `[minX, minY, minZ, maxX, maxY, maxZ]` in block-local
/// coordinates, for a context-free query (no entity). Empty for non-colliding states such as air
/// and plants. A few blocks (scaffolding, powder snow) collide differently depending on the
/// entity; the kernel handles those.
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

/// The block's speed factor: 1.0 for most blocks, 0.4 for soul sand and honey.
pub fn block_speed_factor(block: usize) -> f32 {
    generated::BLOCK_SPEED_FACTOR[block]
}

/// The block's jump factor: 1.0 for most blocks, 0.5 for honey.
pub fn block_jump_factor(block: usize) -> f32 {
    generated::BLOCK_JUMP_FACTOR[block]
}

/// Simple name of the game class implementing the block (`"SlimeBlock"`, `"LadderBlock"`, ...).
/// Behaviour that the game attaches to a block class rather than a tag keys off this.
pub fn block_class(block: usize) -> &'static str {
    generated::CLASS_NAMES[generated::BLOCK_CLASS[block] as usize]
}

/// Whether the block is in the block tag `tag` (e.g. `"minecraft:climbable"`).
pub fn block_has_tag(block: usize, tag: &str) -> bool {
    match generated::TAG_NAMES.binary_search(&tag) {
        Ok(t) => generated::BLOCK_TAGS[block]
            .binary_search(&(t as u16))
            .is_ok(),
        Err(_) => false,
    }
}

/// The kind of fluid a block state contains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FluidKind {
    Empty,
    Water,
    Lava,
}

/// The fluid a block state carries: water/lava source and flowing blocks, waterlogged blocks,
/// bubble columns, kelp, and so on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fluid {
    pub kind: FluidKind,
    /// 1..=8 (8 for sources and falling fluid), 0 when empty.
    pub amount: u8,
    pub source: bool,
    pub falling: bool,
}

impl Fluid {
    pub const EMPTY: Fluid = Fluid {
        kind: FluidKind::Empty,
        amount: 0,
        source: false,
        falling: false,
    };

    pub fn is_empty(self) -> bool {
        self.kind == FluidKind::Empty
    }

    /// `FluidState.getOwnHeight`: the fluid surface within its block, `amount / 9`.
    pub fn own_height(self) -> f32 {
        f32::from(self.amount) / 9.0
    }
}

/// The fluid in `state`.
pub fn fluid(state: u32) -> Fluid {
    let b = STATE_FLAGS[state as usize * 2];
    let kind = match b & 3 {
        1 => FluidKind::Water,
        2 => FluidKind::Lava,
        _ => return Fluid::EMPTY,
    };
    Fluid {
        kind,
        amount: (b >> 2) & 0xf,
        source: b & 0x40 != 0,
        falling: b & 0x80 != 0,
    }
}

/// `BlockState.isSuffocating` for a context-free query.
pub fn is_suffocating(state: u32) -> bool {
    STATE_FLAGS[state as usize * 2 + 1] & 1 != 0
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

/// Block index for a namespaced id (`"minecraft:"` may be omitted), or `None` if unknown.
pub fn block_index(name: &str) -> Option<usize> {
    let map = index_by_name();
    map.get(name).copied().or_else(|| {
        if name.contains(':') {
            None
        } else {
            map.get(format!("minecraft:{name}").as_str()).copied()
        }
    })
}

/// The properties of a block, sorted by name, each with its possible values in state order.
pub fn block_properties(block: usize) -> &'static [(&'static str, &'static [&'static str])] {
    generated::BLOCK_PROPERTIES[block]
}

/// The value of property `name` in `state`, or `None` if the block has no such property.
pub fn property(state: u32, name: &str) -> Option<&'static str> {
    let block = block_of_state(state);
    let props = block_properties(block);
    let mut rest = state - generated::FIRST_STATE[block];
    let mut found = None;
    for (k, vals) in props.iter().rev() {
        let n = vals.len() as u32;
        if *k == name {
            found = Some(vals[(rest % n) as usize]);
        }
        rest /= n;
    }
    found
}

/// `state` with property `name` set to `value`, or `None` if the block lacks that property or
/// value.
pub fn with_property(state: u32, name: &str, value: &str) -> Option<u32> {
    let block = block_of_state(state);
    let props = block_properties(block);
    let first = generated::FIRST_STATE[block];
    let mut digits = decode(state - first, props);
    let i = props.iter().position(|(k, _)| *k == name)?;
    digits[i] = props[i].1.iter().position(|v| *v == value)? as u32;
    Some(first + encode(&digits, props))
}

fn decode(mut index: u32, props: &[(&str, &[&str])]) -> Vec<u32> {
    let mut digits = vec![0; props.len()];
    for (i, (_, vals)) in props.iter().enumerate().rev() {
        let n = vals.len() as u32;
        digits[i] = index % n;
        index /= n;
    }
    digits
}

fn encode(digits: &[u32], props: &[(&str, &[&str])]) -> u32 {
    digits
        .iter()
        .zip(props)
        .fold(0, |acc, (&d, (_, vals))| acc * vals.len() as u32 + d)
}

/// Parse a block state from its text form: `"minecraft:ladder[facing=north]"` (command
/// syntax), `"minecraft:ladder|facing=north"` (the Anvil reader's form), or a bare block id for
/// its default state. Properties that are not given keep their default-state values.
pub fn parse_state(text: &str) -> Option<u32> {
    let text = text.trim();
    let (name, props) = match text.find(['[', '|']) {
        Some(i) => (&text[..i], text[i + 1..].trim_end_matches(']').trim()),
        None => (text, ""),
    };
    let block = block_index(name.trim())?;
    let mut state = default_state(block);
    for kv in props.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (k, v) = kv.split_once('=')?;
        state = with_property(state, k.trim(), v.trim())?;
    }
    Some(state)
}

/// The command-syntax text of a state, e.g. `"minecraft:oak_stairs[facing=east,half=bottom,...]"`.
pub fn state_to_string(state: u32) -> String {
    let block = block_of_state(state);
    let props = block_properties(block);
    let mut out = block_name(block).to_string();
    if !props.is_empty() {
        let digits = decode(state - generated::FIRST_STATE[block], props);
        out.push('[');
        for (i, ((k, vals), d)) in props.iter().zip(&digits).enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(k);
            out.push('=');
            out.push_str(vals[*d as usize]);
        }
        out.push(']');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(name: &str) -> usize {
        block_index(name).unwrap()
    }

    #[test]
    fn air_is_the_first_block() {
        assert_eq!(block_of_state(0), 0);
        assert_eq!(block_name(0), "minecraft:air");
        assert_eq!(default_state(0), 0);
    }

    #[test]
    fn block_of_state_matches_a_search() {
        for state in 0..BLOCK_STATE_COUNT {
            let searched = generated::FIRST_STATE.partition_point(|&first| first <= state) - 1;
            assert_eq!(block_of_state(state), searched);
        }
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
        assert_eq!(generated::BLOCK_PROPERTIES.len(), BLOCK_COUNT);
        assert_eq!(generated::BLOCK_CLASS.len(), BLOCK_COUNT);
        assert_eq!(generated::BLOCK_TAGS.len(), BLOCK_COUNT);
        assert_eq!(generated::BLOCK_SPEED_FACTOR.len(), BLOCK_COUNT);
        assert_eq!(generated::BLOCK_JUMP_FACTOR.len(), BLOCK_COUNT);
        assert_eq!(generated_shapes::BLOCK_FRICTION.len(), BLOCK_COUNT);
    }

    #[test]
    fn every_state_round_trips_through_text() {
        for state in 0..BLOCK_STATE_COUNT {
            let text = state_to_string(state);
            assert_eq!(parse_state(&text), Some(state), "{text}");
        }
    }

    #[test]
    fn parses_partial_and_anvil_forms() {
        let ladder = parse_state("minecraft:ladder[facing=east]").unwrap();
        assert_eq!(property(ladder, "facing"), Some("east"));
        assert_eq!(property(ladder, "waterlogged"), Some("false"));
        assert_eq!(
            parse_state("ladder|facing=east,waterlogged=false"),
            Some(ladder)
        );
        assert_eq!(
            parse_state("minecraft:stone"),
            Some(default_state(block("minecraft:stone")))
        );
        assert_eq!(parse_state("minecraft:nope"), None);
        assert_eq!(parse_state("minecraft:ladder[facing=up]"), None);
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
    fn block_factors() {
        assert_eq!(block_friction(block("minecraft:ice")), 0.98_f32);
        assert_eq!(block_friction(block("minecraft:slime_block")), 0.8_f32);
        assert_eq!(block_speed_factor(block("minecraft:soul_sand")), 0.4_f32);
        assert_eq!(block_speed_factor(block("minecraft:honey_block")), 0.4_f32);
        assert_eq!(block_jump_factor(block("minecraft:honey_block")), 0.5_f32);
        assert_eq!(block_speed_factor(block("minecraft:stone")), 1.0_f32);
    }

    #[test]
    fn classes_and_tags() {
        assert_eq!(block_class(block("minecraft:slime_block")), "SlimeBlock");
        assert!(block_has_tag(
            block("minecraft:ladder"),
            "minecraft:climbable"
        ));
        assert!(!block_has_tag(
            block("minecraft:stone"),
            "minecraft:climbable"
        ));
        assert!(block_has_tag(
            block("minecraft:oak_fence"),
            "minecraft:fences"
        ));
    }

    #[test]
    fn fluids() {
        let src = parse_state("minecraft:water[level=0]").unwrap();
        let f = fluid(src);
        assert_eq!(f.kind, FluidKind::Water);
        assert!(f.source);
        assert_eq!(f.amount, 8);
        let flowing = fluid(parse_state("minecraft:water[level=3]").unwrap());
        assert_eq!(flowing.amount, 5);
        assert!(!flowing.source && !flowing.falling);
        let falling = fluid(parse_state("minecraft:water[level=8]").unwrap());
        assert!(falling.falling);
        assert!(fluid(AIR).is_empty());
        let logged = parse_state("minecraft:oak_stairs[waterlogged=true]").unwrap();
        assert_eq!(fluid(logged).kind, FluidKind::Water);
        assert_eq!(
            fluid(parse_state("minecraft:lava").unwrap()).kind,
            FluidKind::Lava
        );
    }

    #[test]
    fn air_has_no_collision() {
        assert!(collision_boxes(AIR).is_empty());
        assert!(!is_suffocating(AIR));
        assert!(is_suffocating(default_state(block("minecraft:stone"))));
    }
}
