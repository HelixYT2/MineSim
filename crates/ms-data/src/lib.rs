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

/// The friction of a block without a special one: the `0.6F` that `BlockBehaviour.Properties`
/// defaults to. Air has it too.
pub const DEFAULT_FRICTION: f32 = 0.6;

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
///
/// This is a string search; physics code that asks per tick should use the precomputed
/// [`class`] bits ([`state_class`]) instead.
pub fn block_has_tag(block: usize, tag: &str) -> bool {
    match generated::TAG_NAMES.binary_search(&tag) {
        Ok(t) => generated::BLOCK_TAGS[block]
            .binary_search(&(t as u16))
            .is_ok(),
        Err(_) => false,
    }
}

/// Classes of block states, as bit flags, precomputed once per state so the hot paths never touch
/// the string tables (names, classes, tags).
///
/// A world tracks the union of the classes of every state it can hold (see
/// `ms_world::World::classes`), which lets a query skip work that provably cannot find anything: a
/// world whose union lacks [`FLUID`](class::FLUID) has no water or lava to scan for, and so on.
/// The sets are conservative: a bit may be set for a state that does not need it, never the other
/// way round. A module that gives a *new* block a behaviour that the existing bits do not cover
/// must add a bit for it here, so that worlds containing it stop taking the fast paths.
pub mod class {
    /// The state carries a fluid: water or lava (source or flowing), waterlogged blocks, bubble
    /// columns, kelp, seagrass.
    pub const FLUID: u32 = 1 << 0;
    /// The context-free collision shape extends outside the block's own unit cube
    /// (`hasLargeCollisionShape`: fences, walls, ...).
    pub const LARGE_SHAPE: u32 = 1 << 1;
    /// `minecraft:moving_piston`, the one block whose collision the outer ring of the
    /// `BlockCollisions` cursor looks at even though its shape is not large.
    pub const MOVING_PISTON: u32 = 1 << 2;
    /// The block is in the `minecraft:climbable` tag.
    pub const CLIMBABLE: u32 = 1 << 3;
    /// The block is in the `minecraft:fall_damage_resetting` tag.
    pub const FALL_DAMAGE_RESETTING: u32 = 1 << 4;
    /// The collision shape is not the table's `collision_boxes` for every asker and position: it
    /// depends on the entity (scaffolding, powder snow) or on where the block is. Queries ask the
    /// kernel's block module for these blocks and read the table for all the others, so a block
    /// that module treats specially must be given this class here.
    pub const CONTEXT_SHAPE: u32 = 1 << 5;
    /// The block is in the `minecraft:fences` tag.
    pub const FENCE: u32 = 1 << 6;
    /// The block is in the `minecraft:walls` tag.
    pub const WALL: u32 = 1 << 7;
    /// The block's class is `FenceGateBlock`.
    pub const FENCE_GATE: u32 = 1 << 8;
    /// `minecraft:water` or `minecraft:bubble_column` (the blocks whose own speed factor is read
    /// without looking underneath).
    pub const WATER_OR_BUBBLE_COLUMN: u32 = 1 << 9;

    /// A suffocating block that the outer ring of the `BlockCollisions` cursor would look at
    /// (`RING_RELEVANT` and suffocating). Suffocating blocks are full cubes, so the data has none,
    /// which lets the suffocation queries skip the ring in every world.
    pub const SUFFOCATING_RING_RELEVANT: u32 = 1 << 10;
    /// The block's speed factor is not 1.0 (soul sand, honey).
    pub const SPEED_FACTOR: u32 = 1 << 11;
    /// The block's jump factor is not 1.0 (honey).
    pub const JUMP_FACTOR: u32 = 1 << 12;
    /// The block's friction is not [`DEFAULT_FRICTION`](super::DEFAULT_FRICTION) (ice, slime).
    pub const FRICTION: u32 = 1 << 13;
    /// The state carries water (a subset of [`FLUID`]).
    pub const WATER: u32 = 1 << 14;
    /// The state carries lava (a subset of [`FLUID`]).
    pub const LAVA: u32 = 1 << 15;

    /// A cell on the outer ring of the `BlockCollisions` cursor can only matter if its block has
    /// one of these.
    pub const RING_RELEVANT: u32 = LARGE_SHAPE | MOVING_PISTON;
    /// Every class bit: what a world of unknown contents reports.
    pub const ALL: u32 = u32::MAX;
}

fn state_classes() -> &'static [u32] {
    static TABLE: OnceLock<Vec<u32>> = OnceLock::new();
    TABLE.get_or_init(|| {
        // Block-level bits first (string work, once per block), then the state-level ones.
        let per_block: Vec<u32> = (0..BLOCK_COUNT)
            .map(|b| {
                let mut f = 0;
                let name = block_name(b);
                let class_name = block_class(b);
                for (tag, bit) in [
                    ("minecraft:climbable", class::CLIMBABLE),
                    (
                        "minecraft:fall_damage_resetting",
                        class::FALL_DAMAGE_RESETTING,
                    ),
                    ("minecraft:fences", class::FENCE),
                    ("minecraft:walls", class::WALL),
                ] {
                    if block_has_tag(b, tag) {
                        f |= bit;
                    }
                }
                if class_name == "FenceGateBlock" {
                    f |= class::FENCE_GATE;
                }
                // Shapes that depend on the entity (scaffolding, powder snow) or on the block's
                // position (bamboo and pointed dripstone are offset by a position hash).
                if matches!(
                    class_name,
                    "ScaffoldingBlock"
                        | "PowderSnowBlock"
                        | "BambooStalkBlock"
                        | "PointedDripstoneBlock"
                ) {
                    f |= class::CONTEXT_SHAPE;
                }
                if name == "minecraft:moving_piston" {
                    f |= class::MOVING_PISTON;
                }
                if name == "minecraft:water" || name == "minecraft:bubble_column" {
                    f |= class::WATER_OR_BUBBLE_COLUMN;
                }
                if block_speed_factor(b) != 1.0 {
                    f |= class::SPEED_FACTOR;
                }
                if block_jump_factor(b) != 1.0 {
                    f |= class::JUMP_FACTOR;
                }
                if block_friction(b) != DEFAULT_FRICTION {
                    f |= class::FRICTION;
                }
                f
            })
            .collect();
        (0..BLOCK_STATE_COUNT)
            .map(|s| {
                let mut f = per_block[block_of_state(s)];
                match fluid(s).kind {
                    FluidKind::Empty => {}
                    FluidKind::Water => f |= class::FLUID | class::WATER,
                    FluidKind::Lava => f |= class::FLUID | class::LAVA,
                }
                if collision_boxes(s)
                    .iter()
                    .any(|b| (0..3).any(|a| b[a] < 0.0 || b[a + 3] > 1.0))
                {
                    f |= class::LARGE_SHAPE;
                }
                if f & class::RING_RELEVANT != 0 && is_suffocating(s) {
                    f |= class::SUFFOCATING_RING_RELEVANT;
                }
                f
            })
            .collect()
    })
}

/// The [`class`] bits of a block state (a table lookup; air has none). A state id outside the
/// registry reports every bit, the conservative answer.
#[inline]
pub fn state_class(state: u32) -> u32 {
    state_classes()
        .get(state as usize)
        .copied()
        .unwrap_or(class::ALL)
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
    fn state_classes_match_the_string_tables() {
        assert_eq!(state_class(AIR), 0);
        assert_eq!(state_class(BLOCK_STATE_COUNT), class::ALL);
        for state in 0..BLOCK_STATE_COUNT {
            let b = block_of_state(state);
            let c = state_class(state);
            let has = |bit: u32| c & bit != 0;
            assert_eq!(has(class::FLUID), !fluid(state).is_empty(), "{state}");
            assert_eq!(has(class::WATER), fluid(state).kind == FluidKind::Water);
            assert_eq!(has(class::LAVA), fluid(state).kind == FluidKind::Lava);
            assert_eq!(
                has(class::LARGE_SHAPE),
                collision_boxes(state)
                    .iter()
                    .any(|b| (0..3).any(|a| b[a] < 0.0 || b[a + 3] > 1.0)),
                "{state}"
            );
            assert_eq!(
                has(class::MOVING_PISTON),
                block_name(b) == "minecraft:moving_piston"
            );
            assert_eq!(
                has(class::CLIMBABLE),
                block_has_tag(b, "minecraft:climbable")
            );
            assert_eq!(
                has(class::FALL_DAMAGE_RESETTING),
                block_has_tag(b, "minecraft:fall_damage_resetting")
            );
            assert_eq!(
                has(class::SUFFOCATING_RING_RELEVANT),
                is_suffocating(state) && has(class::RING_RELEVANT)
            );
            assert_eq!(has(class::SPEED_FACTOR), block_speed_factor(b) != 1.0);
            assert_eq!(has(class::JUMP_FACTOR), block_jump_factor(b) != 1.0);
            assert_eq!(has(class::FRICTION), block_friction(b) != DEFAULT_FRICTION);
            assert_eq!(has(class::FENCE), block_has_tag(b, "minecraft:fences"));
            assert_eq!(has(class::WALL), block_has_tag(b, "minecraft:walls"));
            assert_eq!(has(class::FENCE_GATE), block_class(b) == "FenceGateBlock");
            assert_eq!(
                has(class::CONTEXT_SHAPE),
                matches!(
                    block_class(b),
                    "ScaffoldingBlock"
                        | "PowderSnowBlock"
                        | "BambooStalkBlock"
                        | "PointedDripstoneBlock"
                )
            );
            assert_eq!(
                has(class::WATER_OR_BUBBLE_COLUMN),
                matches!(block_name(b), "minecraft:water" | "minecraft:bubble_column")
            );
        }
        // Spot checks that the interesting blocks are classified at all.
        let st = |n: &str| parse_state(n).unwrap();
        assert!(state_class(st("minecraft:ladder")) & class::CLIMBABLE != 0);
        assert!(state_class(st("minecraft:scaffolding")) & class::CONTEXT_SHAPE != 0);
        assert!(state_class(st("minecraft:oak_fence")) & class::LARGE_SHAPE != 0);
        assert_eq!(state_class(st("minecraft:stone")), 0);
        assert!(state_class(st("minecraft:ice")) & class::FRICTION != 0);
        assert!(state_class(st("minecraft:soul_sand")) & class::SPEED_FACTOR != 0);
        assert!(state_class(st("minecraft:honey_block")) & class::JUMP_FACTOR != 0);
    }

    #[test]
    fn air_has_no_collision() {
        assert!(collision_boxes(AIR).is_empty());
        assert!(!is_suffocating(AIR));
        assert!(is_suffocating(default_state(block("minecraft:stone"))));
    }
}
