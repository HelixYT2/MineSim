//! The canonical per-tick serialization of the player state (`docs/contract.md` §2, `contract-v1`)
//! and the rolling hash the golden lock folds it into.
//!
//! The byte layout is the contract: the simulator ([`serialize_player`]) and the recorded corpus
//! (`ms_corpus::canonical`, which reads the same field order from [`LAYOUT`]) must produce
//! identical bytes for identical state, so `H_sim(t) == H_oracle(t)` is a plain integer compare.
//!
//! What is hashed is exactly the state the real client reports for the local player each tick:
//! everything the next tick's physics reads plus the derived bounding-box size. What is not:
//! [`PlayerState::server_vel`] (the server's shadow copy of the velocity, which the client never
//! observes; its effect shows up in `vel` when knockback is delivered) and the attribute modifier
//! lists (the oracle records attribute *values*, which is all that influences anything).
//!
//! `serialize_player` destructures [`PlayerState`] without a rest pattern on purpose: adding a
//! field to the state fails the build here until the field is either added to the layout (a new
//! contract version) or explicitly excluded.

use crate::{StateBuf, HASH_SEED};
use ms_kernel::attributes::Attribute;
use ms_kernel::{PlayerState, Pose};
use xxhash_rust::xxh3::xxh3_64_with_seed;

/// The contract version whose layout this module implements. It is the first byte of every
/// serialized state, so states of different versions can never collide.
pub const CONTRACT_VERSION: u8 = 1;

/// How one entry of the layout is encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `double`: raw IEEE-754 bits, `u64` big-endian.
    F64,
    /// `float`: raw IEEE-754 bits, `u32` big-endian.
    F32,
    /// `boolean`: one byte, `0` or `1`.
    Bool,
    /// `int`: two's-complement `i32` big-endian.
    I32,
    /// `Pose`: the game's enum id as one byte (see [`pose_id_from_name`]).
    Pose,
    /// `mainSupportingBlockPos`: a presence byte, then `BlockPos.asLong` as `i64` (0 if absent).
    Support,
    /// Active effects: `i32` count, then per effect (ascending by id) a `u16`-length-prefixed id,
    /// `i32` amplifier and `i32` duration.
    Effects,
    /// The 14 attribute values in [`Attribute::ALL`] order, `f64` each.
    Attrs,
}

/// The frozen field order of `contract-v1`: the corpus key of each field and its encoding. The
/// serialized state is the version byte followed by these fields in this order.
pub const LAYOUT: &[(&str, Kind)] = &[
    // Entity
    ("x", Kind::F64),
    ("y", Kind::F64),
    ("z", Kind::F64),
    ("dx", Kind::F64),
    ("dy", Kind::F64),
    ("dz", Kind::F64),
    ("yaw", Kind::F32),
    ("pitch", Kind::F32),
    ("ground", Kind::Bool),
    ("hc", Kind::Bool),
    ("mhc", Kind::Bool),
    ("vc", Kind::Bool),
    ("vcb", Kind::Bool),
    ("fall", Kind::F64),
    ("water", Kind::Bool),
    ("eyeWater", Kind::Bool),
    ("lava", Kind::Bool),
    ("waterH", Kind::F64),
    ("lavaH", Kind::F64),
    ("powder", Kind::Bool),
    ("wasPowder", Kind::Bool),
    ("stuckX", Kind::F64),
    ("stuckY", Kind::F64),
    ("stuckZ", Kind::F64),
    ("support", Kind::Support),
    ("noBlocks", Kind::Bool),
    ("pose", Kind::Pose),
    ("w", Kind::F32),
    ("h", Kind::F32),
    ("sprinting", Kind::Bool),
    ("shift", Kind::Bool),
    ("swimming", Kind::Bool),
    ("fire", Kind::I32),
    ("frozen", Kind::I32),
    ("age", Kind::I32),
    ("invul", Kind::I32),
    // LivingEntity
    ("health", Kind::F32),
    ("absorption", Kind::F32),
    ("hurtTime", Kind::I32),
    ("lastHurt", Kind::F32),
    ("deathTime", Kind::I32),
    ("njd", Kind::I32),
    ("jumping", Kind::Bool),
    ("xxa", Kind::F32),
    ("zza", Kind::F32),
    ("speed", Kind::F32),
    ("effects", Kind::Effects),
    ("attrs", Kind::Attrs),
    // Player / LocalPlayer
    ("food", Kind::I32),
    ("saturation", Kind::F32),
    ("exhaustion", Kind::F32),
    ("jumpTrigger", Kind::I32),
    ("sprintTrigger", Kind::I32),
    ("flying", Kind::Bool),
    ("crouching", Kind::Bool),
];

/// Position of `field` in [`LAYOUT`], for ordering diffs the way the contract orders fields
/// ("first differing field"). Unknown names sort after every known one.
pub fn field_rank(field: &str) -> usize {
    let name = field.split('.').next().unwrap_or(field);
    LAYOUT
        .iter()
        .position(|(n, _)| *n == name)
        .unwrap_or(LAYOUT.len())
}

/// `net.minecraft.world.entity.Pose` constants in ordinal order (the corpus records the constant
/// name; the hash records the ordinal, `Pose.id()`).
pub const POSE_NAMES: [&str; 18] = [
    "STANDING",
    "FALL_FLYING",
    "SLEEPING",
    "SWIMMING",
    "SPIN_ATTACK",
    "CROUCHING",
    "LONG_JUMPING",
    "DYING",
    "CROAKING",
    "USING_TONGUE",
    "SITTING",
    "ROARING",
    "SNIFFING",
    "EMERGING",
    "DIGGING",
    "SLIDING",
    "SHOOTING",
    "INHALING",
];

/// The game's id for a pose constant name (`"CROUCHING"` → 5).
pub fn pose_id_from_name(name: &str) -> Option<u8> {
    POSE_NAMES.iter().position(|n| *n == name).map(|i| i as u8)
}

/// The game's id for a simulated pose. Exhaustive on purpose: a new [`Pose`] variant must be
/// given its id here.
pub fn pose_id(pose: Pose) -> u8 {
    match pose {
        Pose::Standing => 0,
        Pose::FallFlying => 1,
        Pose::Swimming => 3,
        Pose::Crouching => 5,
        Pose::Dying => 7,
    }
}

/// `BlockPos.asLong`: 26 bits of x, 26 of z, 12 of y.
pub fn pack_block_pos(x: i32, y: i32, z: i32) -> i64 {
    ((i64::from(x) & 0x3ff_ffff) << 38)
        | ((i64::from(z) & 0x3ff_ffff) << 12)
        | (i64::from(y) & 0xfff)
}

/// Write the canonical serialization of `p` into `b` (version byte, then [`LAYOUT`] in order).
pub fn serialize_player(p: &PlayerState, b: &mut StateBuf) {
    let (width, height) = p.dimensions();
    let PlayerState {
        pos,
        vel,
        yaw,
        pitch,
        on_ground,
        horizontal_collision,
        minor_horizontal_collision,
        vertical_collision,
        vertical_collision_below,
        fall_distance,
        in_water,
        eye_in_water,
        in_lava,
        water_height,
        lava_height,
        in_powder_snow,
        was_in_powder_snow,
        stuck_speed_multiplier,
        supporting_block,
        on_ground_no_blocks,
        pose,
        sprinting,
        shift_key_down,
        swimming,
        remaining_fire_ticks,
        ticks_frozen,
        tick_count,
        invulnerable_time,
        health,
        absorption,
        hurt_time,
        last_hurt,
        death_time,
        no_jump_delay,
        jumping,
        xxa,
        zza,
        speed,
        effects,
        attributes,
        food,
        saturation,
        exhaustion,
        jump_trigger_time,
        sprint_trigger_time,
        flying,
        crouching,
        // The server's shadow copy: not client-observable, deliberately not part of the hash.
        server_vel: _,
    } = p;

    b.push_u8(CONTRACT_VERSION);
    // Entity
    b.push_f64(pos.x).push_f64(pos.y).push_f64(pos.z);
    b.push_f64(vel.x).push_f64(vel.y).push_f64(vel.z);
    b.push_f32(*yaw).push_f32(*pitch);
    b.push_bool(*on_ground)
        .push_bool(*horizontal_collision)
        .push_bool(*minor_horizontal_collision)
        .push_bool(*vertical_collision)
        .push_bool(*vertical_collision_below);
    b.push_f64(*fall_distance);
    b.push_bool(*in_water)
        .push_bool(*eye_in_water)
        .push_bool(*in_lava);
    b.push_f64(*water_height).push_f64(*lava_height);
    b.push_bool(*in_powder_snow).push_bool(*was_in_powder_snow);
    b.push_f64(stuck_speed_multiplier.x)
        .push_f64(stuck_speed_multiplier.y)
        .push_f64(stuck_speed_multiplier.z);
    match supporting_block {
        Some((x, y, z)) => b.push_bool(true).push_i64(pack_block_pos(*x, *y, *z)),
        None => b.push_bool(false).push_i64(0),
    };
    b.push_bool(*on_ground_no_blocks);
    b.push_u8(pose_id(*pose));
    b.push_f32(width).push_f32(height);
    b.push_bool(*sprinting)
        .push_bool(*shift_key_down)
        .push_bool(*swimming);
    b.push_i32(*remaining_fire_ticks)
        .push_i32(*ticks_frozen)
        .push_i32(*tick_count)
        .push_i32(*invulnerable_time);
    // LivingEntity
    b.push_f32(*health).push_f32(*absorption);
    b.push_i32(*hurt_time);
    b.push_f32(*last_hurt);
    b.push_i32(*death_time).push_i32(*no_jump_delay);
    b.push_bool(*jumping);
    b.push_f32(*xxa).push_f32(*zza).push_f32(*speed);
    let mut active: Vec<_> = effects.iter().collect();
    active.sort_by(|a, c| a.id.as_bytes().cmp(c.id.as_bytes()));
    b.push_i32(active.len() as i32);
    for e in active {
        b.push_str(&e.id).push_i32(e.amplifier).push_i32(e.duration);
    }
    for a in Attribute::ALL {
        b.push_f64(attributes.value(a));
    }
    // Player / LocalPlayer
    b.push_i32(*food);
    b.push_f32(*saturation).push_f32(*exhaustion);
    b.push_i32(*jump_trigger_time)
        .push_i32(*sprint_trigger_time);
    b.push_bool(*flying).push_bool(*crouching);
}

/// The canonical bytes of `p`.
pub fn player_bytes(p: &PlayerState) -> Vec<u8> {
    let mut b = StateBuf::new();
    serialize_player(p, &mut b);
    b.into_bytes()
}

/// `H(t)` for the player state `p`: `xxh3_64(seed = HASH_SEED, serialize(p))`.
pub fn player_hash(p: &PlayerState) -> u64 {
    let mut b = StateBuf::new();
    serialize_player(p, &mut b);
    b.hash()
}

/// A running fingerprint of a whole tick sequence: `R(-1) = 0`,
/// `R(t) = xxh3_64(seed = HASH_SEED, be64(R(t-1)) ++ be64(H(t)))`. Equal sequences give equal
/// values; a difference at any tick changes every later value, so one number locks a scenario.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RollingHash {
    value: u64,
    count: u64,
}

impl RollingHash {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold in the next tick's hash `H(t)`; returns the new rolling value `R(t)`.
    pub fn push(&mut self, tick_hash: u64) -> u64 {
        let mut buf = [0u8; 16];
        buf[..8].copy_from_slice(&self.value.to_be_bytes());
        buf[8..].copy_from_slice(&tick_hash.to_be_bytes());
        self.value = xxh3_64_with_seed(&buf, HASH_SEED);
        self.count += 1;
        self.value
    }

    /// `R(t)` after the last pushed tick (0 before any).
    pub fn value(&self) -> u64 {
        self.value
    }

    /// How many ticks have been folded in.
    pub fn count(&self) -> u64 {
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ms_kernel::effects::EffectInstance;
    use ms_numerics::Vec3;

    fn base() -> PlayerState {
        PlayerState::new(Vec3::new(0.5, -63.0, 0.5), 90.0)
    }

    /// Bytes for a state with the given effect list (insertion order as given).
    fn with_effects(ids: &[(&str, i32, i32)]) -> Vec<u8> {
        let mut p = base();
        for (id, amp, dur) in ids {
            p.effects.insert(EffectInstance {
                id: (*id).to_string(),
                amplifier: *amp,
                duration: *dur,
            });
        }
        player_bytes(&p)
    }

    #[test]
    fn layout_names_are_unique() {
        for (i, (a, _)) in LAYOUT.iter().enumerate() {
            assert!(
                LAYOUT[i + 1..].iter().all(|(b, _)| a != b),
                "duplicate layout field {a}"
            );
        }
    }

    #[test]
    fn bytes_are_stable_and_big_endian() {
        let p = base();
        assert_eq!(player_bytes(&p), player_bytes(&p.clone()));
        let bytes = player_bytes(&p);
        assert_eq!(bytes[0], CONTRACT_VERSION);
        assert_eq!(&bytes[1..9], &0.5_f64.to_bits().to_be_bytes());
        assert_eq!(&bytes[9..17], &(-63.0_f64).to_bits().to_be_bytes());
        assert_eq!(&bytes[17..25], &0.5_f64.to_bits().to_be_bytes());
        // Velocity (3 x f64), then yaw as raw f32 bits.
        assert_eq!(&bytes[49..53], &90.0_f32.to_bits().to_be_bytes());
    }

    #[test]
    fn length_matches_the_layout() {
        let mut want = 1usize;
        for (_, kind) in LAYOUT {
            want += match kind {
                Kind::F64 => 8,
                Kind::F32 | Kind::I32 => 4,
                Kind::Bool | Kind::Pose => 1,
                Kind::Support => 9,
                Kind::Effects => 4,
                Kind::Attrs => 8 * Attribute::ALL.len(),
            };
        }
        assert_eq!(player_bytes(&base()).len(), want);
        // Each effect adds u16 length + id + two i32.
        let one = with_effects(&[("minecraft:speed", 1, 600)]);
        assert_eq!(one.len(), want + 2 + "minecraft:speed".len() + 8);
    }

    /// A state with every hashed field set explicitly (nothing inherited from the defaults of
    /// `PlayerState::new`, so changing those defaults cannot disturb the frozen value).
    fn frozen_state() -> PlayerState {
        let mut p = PlayerState::new(Vec3::ZERO, 0.0);
        p.pos = Vec3::new(4.5, -63.0, -2.5);
        p.vel = Vec3::new(0.0, -0.0784000015258789, 0.1);
        p.yaw = 135.0;
        p.pitch = -22.5;
        p.on_ground = true;
        p.horizontal_collision = false;
        p.minor_horizontal_collision = true;
        p.vertical_collision = true;
        p.vertical_collision_below = true;
        p.fall_distance = 2.5;
        p.in_water = false;
        p.eye_in_water = true;
        p.in_lava = false;
        p.water_height = 0.8888888955116272;
        p.lava_height = 0.0;
        p.in_powder_snow = false;
        p.was_in_powder_snow = true;
        p.stuck_speed_multiplier = Vec3::new(0.25, 0.05, 0.25);
        p.supporting_block = Some((4, -64, -3));
        p.on_ground_no_blocks = false;
        p.pose = Pose::Crouching;
        p.sprinting = false;
        p.shift_key_down = true;
        p.swimming = false;
        p.remaining_fire_ticks = -20;
        p.ticks_frozen = 7;
        p.tick_count = 1234;
        p.invulnerable_time = 12;
        p.health = 17.5;
        p.absorption = 2.0;
        p.hurt_time = 4;
        p.last_hurt = 3.0;
        p.death_time = 0;
        p.no_jump_delay = 6;
        p.jumping = true;
        p.xxa = f32::from_bits(0x3e96_872c); // 0.2940000295639038
        p.zza = -f32::from_bits(0x3e96_872c);
        p.speed = 0.1_f32;
        p.effects = Default::default();
        for (id, amplifier, duration) in
            [("minecraft:speed", 1, 600), ("minecraft:jump_boost", 0, -1)]
        {
            p.effects.insert(EffectInstance {
                id: id.into(),
                amplifier,
                duration,
            });
        }
        p.attributes = ms_kernel::attributes::Attributes::player();
        for (i, a) in Attribute::ALL.into_iter().enumerate() {
            // Inside every attribute's allowed range, so `value()` returns the base unchanged.
            let base = match a {
                Attribute::MaxHealth => 20.0,
                Attribute::Scale => 1.0,
                _ => 0.5 + i as f64 / 64.0,
            };
            p.attributes.set_base(a, base);
        }
        p.food = 17;
        p.saturation = 3.5;
        p.exhaustion = 1.25;
        p.jump_trigger_time = 3;
        p.sprint_trigger_time = 0;
        p.flying = false;
        p.crouching = true;
        p.server_vel = Vec3::new(9.0, 9.0, 9.0);
        p
    }

    /// `contract-v1` is frozen: this value changes only if the byte layout, the hash seed or the
    /// hash function changes, each of which requires a new contract version (and re-blessing every
    /// golden hash).
    #[test]
    fn contract_v1_hash_is_frozen() {
        let p = frozen_state();
        assert_eq!(
            player_hash(&p),
            FROZEN_HASH,
            "contract-v1 serialization changed; this needs a new contract version"
        );
        assert_eq!(player_bytes(&p).len(), 383);
    }

    const FROZEN_HASH: u64 = 13_107_009_037_501_832_953;

    #[test]
    fn nan_is_hashed_by_bit_pattern() {
        let mut a = base();
        let mut b = base();
        a.pos.x = f64::from_bits(0x7ff8_0000_0000_0000);
        b.pos.x = f64::from_bits(0x7ff8_0000_0000_0001);
        assert_eq!(player_hash(&a), player_hash(&a.clone()));
        assert_ne!(
            player_bytes(&a),
            player_bytes(&b),
            "NaN payloads must differ"
        );
        assert_ne!(player_hash(&a), player_hash(&b));
        let mut f = base();
        let mut g = base();
        f.yaw = f32::from_bits(0x7fc0_0000);
        g.yaw = f32::from_bits(0xffc0_0000);
        assert_ne!(player_hash(&f), player_hash(&g), "NaN sign must differ");
    }

    #[test]
    fn negative_zero_is_distinct_everywhere() {
        let zero = PlayerState::new(Vec3::ZERO, 0.0);
        type Edit = fn(&mut PlayerState);
        let edits: &[(&str, Edit)] = &[
            ("pos.x", |p| p.pos.x = -0.0),
            ("vel.y", |p| p.vel.y = -0.0),
            ("yaw", |p| p.yaw = -0.0),
            ("stuck.z", |p| p.stuck_speed_multiplier.z = -0.0),
            ("saturation", |p| p.exhaustion = -0.0),
            ("fall", |p| p.fall_distance = -0.0),
        ];
        for (name, edit) in edits {
            let mut p = zero.clone();
            edit(&mut p);
            assert_ne!(player_bytes(&p), player_bytes(&zero), "-0.0 in {name}");
        }
    }

    #[test]
    fn effects_serialize_in_id_order_regardless_of_insertion() {
        let a = with_effects(&[("minecraft:speed", 1, 600), ("minecraft:jump_boost", 0, 20)]);
        let b = with_effects(&[("minecraft:jump_boost", 0, 20), ("minecraft:speed", 1, 600)]);
        assert_eq!(a, b);
        let c = with_effects(&[("minecraft:speed", 2, 600), ("minecraft:jump_boost", 0, 20)]);
        assert_ne!(a, c, "amplifier is part of the state");
    }

    #[test]
    fn support_presence_is_distinct_from_the_origin() {
        let none = base();
        let mut origin = base();
        origin.supporting_block = Some((0, 0, 0));
        assert_ne!(player_bytes(&none), player_bytes(&origin));
        assert_eq!(pack_block_pos(0, 0, 0), 0);
    }

    #[test]
    fn block_pos_packing_matches_the_game() {
        // BlockPos(-1, -64, -1).asLong(): x and z occupy 26 bits each, y 12.
        let packed = pack_block_pos(-1, -64, -1);
        assert_eq!(
            packed,
            ((0x3ff_ffff_i64) << 38) | (0x3ff_ffff_i64 << 12) | 0xfc0
        );
        assert_eq!(pack_block_pos(4, -63, 0), (4_i64 << 38) | 0xfc1);
    }

    #[test]
    fn pose_ids_follow_the_game_enum() {
        for pose in [
            Pose::Standing,
            Pose::Crouching,
            Pose::Swimming,
            Pose::FallFlying,
            Pose::Dying,
        ] {
            assert_eq!(
                Some(pose_id(pose)),
                pose_id_from_name(pose.name()),
                "{pose:?}"
            );
        }
        assert_eq!(pose_id_from_name("CROUCHING"), Some(5));
        assert_eq!(pose_id_from_name("INHALING"), Some(17));
        assert_eq!(pose_id_from_name("nope"), None);
    }

    /// Every field of the layout must influence the bytes (otherwise it is not locked), and the
    /// server-side shadow velocity must not.
    #[test]
    fn every_state_field_changes_the_bytes() {
        type Edit = fn(&mut PlayerState);
        let edits: &[(&str, Edit)] = &[
            ("x", |p| p.pos.x = 1.0),
            ("y", |p| p.pos.y = 1.0),
            ("z", |p| p.pos.z = 1.0),
            ("dx", |p| p.vel.x = 1.0),
            ("dy", |p| p.vel.y = 1.0),
            ("dz", |p| p.vel.z = 1.0),
            ("yaw", |p| p.yaw = 1.0),
            ("pitch", |p| p.pitch = 1.0),
            ("ground", |p| p.on_ground = true),
            ("hc", |p| p.horizontal_collision = true),
            ("mhc", |p| p.minor_horizontal_collision = true),
            ("vc", |p| p.vertical_collision = true),
            ("vcb", |p| p.vertical_collision_below = true),
            ("fall", |p| p.fall_distance = 1.0),
            ("water", |p| p.in_water = true),
            ("eyeWater", |p| p.eye_in_water = true),
            ("lava", |p| p.in_lava = true),
            ("waterH", |p| p.water_height = 1.0),
            ("lavaH", |p| p.lava_height = 1.0),
            ("powder", |p| p.in_powder_snow = true),
            ("wasPowder", |p| p.was_in_powder_snow = true),
            ("stuckX", |p| p.stuck_speed_multiplier.x = 1.0),
            ("stuckY", |p| p.stuck_speed_multiplier.y = 1.0),
            ("stuckZ", |p| p.stuck_speed_multiplier.z = 1.0),
            ("support", |p| p.supporting_block = Some((1, 2, 3))),
            ("noBlocks", |p| p.on_ground_no_blocks = true),
            ("pose", |p| p.pose = Pose::Crouching),
            ("sprinting", |p| p.sprinting = true),
            ("shift", |p| p.shift_key_down = true),
            ("swimming", |p| p.swimming = true),
            ("fire", |p| p.remaining_fire_ticks = 1),
            ("frozen", |p| p.ticks_frozen = 1),
            ("age", |p| p.tick_count = 1),
            ("invul", |p| p.invulnerable_time = 1),
            ("health", |p| p.health = 1.0),
            ("absorption", |p| p.absorption = 1.0),
            ("hurtTime", |p| p.hurt_time = 1),
            ("lastHurt", |p| p.last_hurt = 1.0),
            ("deathTime", |p| p.death_time = 1),
            ("njd", |p| p.no_jump_delay = 1),
            ("jumping", |p| p.jumping = true),
            ("xxa", |p| p.xxa = 1.0),
            ("zza", |p| p.zza = 1.0),
            ("speed", |p| p.speed = 1.0),
            ("effects", |p| {
                p.effects.insert(EffectInstance {
                    id: "minecraft:speed".into(),
                    amplifier: 0,
                    duration: 1,
                })
            }),
            ("attrs", |p| p.attributes.set_base(Attribute::Gravity, 0.1)),
            ("food", |p| p.food = 1),
            ("saturation", |p| p.saturation = 1.0),
            ("exhaustion", |p| p.exhaustion = 1.0),
            ("jumpTrigger", |p| p.jump_trigger_time = 1),
            ("sprintTrigger", |p| p.sprint_trigger_time = 1),
            ("flying", |p| p.flying = true),
            ("crouching", |p| p.crouching = true),
        ];
        let reference = PlayerState::new(Vec3::ZERO, 0.0);
        let want = player_bytes(&reference);
        for (name, edit) in edits {
            let mut p = reference.clone();
            edit(&mut p);
            assert_ne!(player_bytes(&p), want, "{name} does not reach the bytes");
        }
        // The edit table covers the whole layout except the derived box size.
        for (name, _) in LAYOUT {
            if *name == "w" || *name == "h" {
                continue;
            }
            assert!(edits.iter().any(|(n, _)| n == name), "no edit for {name}");
        }
        let mut p = reference.clone();
        p.server_vel = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(player_bytes(&p), want, "server_vel is not part of the hash");
        // The box size is derived: a scale change moves it even with the pose fixed.
        let mut p = reference.clone();
        p.attributes.set_base(Attribute::Scale, 2.0);
        assert_ne!(player_bytes(&p), want);
    }

    #[test]
    fn rolling_hash_is_order_and_length_sensitive() {
        let run = |hs: &[u64]| {
            let mut r = RollingHash::new();
            for &h in hs {
                r.push(h);
            }
            r
        };
        assert_eq!(run(&[]).value(), 0);
        assert_eq!(run(&[1, 2, 3]), run(&[1, 2, 3]));
        assert_ne!(run(&[1, 2, 3]).value(), run(&[1, 3, 2]).value());
        assert_ne!(run(&[1, 2, 3]).value(), run(&[1, 2]).value());
        assert_ne!(run(&[0]).value(), run(&[0, 0]).value());
        assert_eq!(run(&[1, 2, 3]).count(), 3);
        // A change at tick 0 reaches the final value.
        assert_ne!(run(&[9, 2, 3]).value(), run(&[1, 2, 3]).value());
    }
}
