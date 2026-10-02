//! Reading the oracle corpus (`docs/corpus.md`) and replaying it through the kernel.
//!
//! A scenario file is a header (the arena's blocks and the start state) followed by one row per
//! recorded client tick: the input, the player state before the tick (as a diff against the
//! previous row's end state — what the server changed in between), the state after the tick, and
//! any server-side events. [`Scenario::world`] rebuilds the arena; [`apply_state`] writes recorded
//! fields into a [`PlayerState`]; [`compare_state`] lists the fields a simulated state gets wrong.

#![forbid(unsafe_code)]

use ms_kernel::attributes::Attribute;
use ms_kernel::effects::{EffectInstance, Effects};
use ms_kernel::{Input, PlayerState, Pose};
use ms_numerics::Vec3;
use ms_world::{FlatWorld, GridWorld, World};
use serde_json::{Map, Value};
use std::io::Read;
use std::path::{Path, PathBuf};

pub type Fields = Map<String, Value>;

/// One recorded client tick.
#[derive(Clone, Debug)]
pub struct Row {
    pub t: usize,
    pub input: Input,
    /// Start-of-tick state: complete in row 0, otherwise only the fields changed since the previous
    /// row's `post`.
    pub pre: Fields,
    /// End-of-tick state (complete).
    pub post: Fields,
    /// Server-side events that ran before this tick.
    pub srv: Vec<Value>,
}

/// A recorded scenario.
#[derive(Clone, Debug)]
pub struct Scenario {
    pub name: String,
    pub header: Fields,
    pub rows: Vec<Row>,
}

/// The repository's `corpus/` directory.
pub fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

/// Names of the recorded client scenarios, sorted.
pub fn client_scenarios() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(corpus_dir().join("client"))
        .map(|d| {
            d.filter_map(Result::ok)
                .filter_map(|e| {
                    let n = e.file_name().to_string_lossy().to_string();
                    n.strip_suffix(".jsonl.gz")
                        .or_else(|| n.strip_suffix(".jsonl"))
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names.dedup();
    names
}

impl Scenario {
    /// Load `corpus/client/<name>.jsonl.gz` (or the uncompressed `.jsonl`).
    pub fn load(name: &str) -> Result<Self, String> {
        let dir = corpus_dir().join("client");
        let gz = dir.join(format!("{name}.jsonl.gz"));
        let text = if gz.exists() {
            let mut s = String::new();
            flate2::read::GzDecoder::new(std::fs::File::open(&gz).map_err(|e| e.to_string())?)
                .read_to_string(&mut s)
                .map_err(|e| e.to_string())?;
            s
        } else {
            std::fs::read_to_string(dir.join(format!("{name}.jsonl")))
                .map_err(|e| format!("{name}: {e}"))?
        };
        Self::parse(name, &text)
    }

    pub fn parse(name: &str, text: &str) -> Result<Self, String> {
        let mut lines = text.lines();
        let header: Fields = serde_json::from_str(lines.next().ok_or("empty file")?)
            .map_err(|e| format!("{name} header: {e}"))?;
        let mut rows = Vec::new();
        for (i, line) in lines.enumerate() {
            let v: Fields =
                serde_json::from_str(line).map_err(|e| format!("{name} row {i}: {e}"))?;
            rows.push(Row {
                t: v["t"].as_u64().unwrap_or(i as u64) as usize,
                input: parse_input(&v["in"]),
                pre: v["pre"].as_object().cloned().unwrap_or_default(),
                post: v["post"].as_object().cloned().unwrap_or_default(),
                srv: v
                    .get("srv")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
            });
        }
        Ok(Self {
            name: name.to_string(),
            header,
            rows,
        })
    }

    /// The arena as a [`World`]: the stone floor everywhere plus the recorded blocks.
    pub fn world(&self) -> World {
        let arena = &self.header["arena"];
        let floor_y = arena["floorY"].as_i64().unwrap_or(-64) as i32;
        let floor = ms_data::parse_state(arena["floorBlock"].as_str().unwrap_or("minecraft:stone"))
            .expect("known floor block");
        let mut grid = GridWorld::new(FlatWorld::new(floor_y + 1, floor));
        for b in self.header["blocks"].as_array().into_iter().flatten() {
            let a = b.as_array().expect("block entry is an array");
            let (x, y, z) = (
                a[0].as_i64().unwrap() as i32,
                a[1].as_i64().unwrap() as i32,
                a[2].as_i64().unwrap() as i32,
            );
            let text = a[3].as_str().unwrap();
            let state = ms_data::parse_state(text)
                .unwrap_or_else(|| panic!("{}: unknown block state {text}", self.name));
            grid.set_block(x, y, z, state);
        }
        // Below the floor is outside the world: no blocks.
        World::grid(grid)
    }

    /// The complete state at the start of the recording, with effects and attributes brought in
    /// the way the client receives them (so attribute modifiers match the effects).
    pub fn initial_state(&self) -> PlayerState {
        let mut p = PlayerState::new(Vec3::ZERO, 0.0);
        apply_pre(&mut p, &self.rows[0].pre, None);
        p
    }
}

fn bit(v: &Value) -> bool {
    v.as_i64() == Some(1)
}

fn f32_bits(v: &Value) -> f32 {
    f32::from_bits(v.as_i64().expect("f32 bits") as i32 as u32)
}

fn f64_bits(v: &Value) -> f64 {
    f64::from_bits(v.as_i64().expect("f64 bits") as u64)
}

fn parse_input(v: &Value) -> Input {
    Input {
        forward: bit(&v["f"]),
        back: bit(&v["b"]),
        left: bit(&v["l"]),
        right: bit(&v["r"]),
        jump: bit(&v["j"]),
        shift: bit(&v["s"]),
        sprint: bit(&v["sp"]),
        yaw: f32_bits(&v["yaw"]),
        pitch: f32_bits(&v["pitch"]),
    }
}

/// Unpack `BlockPos.asLong`.
pub fn unpack_block_pos(packed: i64) -> (i32, i32, i32) {
    let p = ms_world::coords::BlockPos::from_long(packed);
    (p.x, p.y, p.z)
}

/// Write every recorded field present in `f` into `p`. Unknown fields are ignored; the
/// `attrs` values are applied as attribute base values only when no modifiers explain them, so
/// prefer replaying effects through the kernel.
pub fn apply_state(p: &mut PlayerState, f: &Fields) {
    for (k, v) in f {
        match k.as_str() {
            "x" => p.pos.x = f64_bits(v),
            "y" => p.pos.y = f64_bits(v),
            "z" => p.pos.z = f64_bits(v),
            "dx" => p.vel.x = f64_bits(v),
            "dy" => p.vel.y = f64_bits(v),
            "dz" => p.vel.z = f64_bits(v),
            "yaw" => p.yaw = f32_bits(v),
            "pitch" => p.pitch = f32_bits(v),
            "ground" => p.on_ground = bit(v),
            "hc" => p.horizontal_collision = bit(v),
            "mhc" => p.minor_horizontal_collision = bit(v),
            "vc" => p.vertical_collision = bit(v),
            "vcb" => p.vertical_collision_below = bit(v),
            "fall" => p.fall_distance = f64_bits(v),
            "water" => p.in_water = bit(v),
            "eyeWater" => p.eye_in_water = bit(v),
            "lava" => p.in_lava = bit(v),
            "waterH" => p.water_height = f64_bits(v),
            "lavaH" => p.lava_height = f64_bits(v),
            "powder" => p.in_powder_snow = bit(v),
            "wasPowder" => p.was_in_powder_snow = bit(v),
            "stuckX" => p.stuck_speed_multiplier.x = f64_bits(v),
            "stuckY" => p.stuck_speed_multiplier.y = f64_bits(v),
            "stuckZ" => p.stuck_speed_multiplier.z = f64_bits(v),
            "support" => {
                p.supporting_block = v.as_i64().map(unpack_block_pos);
            }
            "noBlocks" => p.on_ground_no_blocks = bit(v),
            "pose" => {
                if let Some(pose) = v.as_str().and_then(Pose::from_name) {
                    p.pose = pose;
                }
            }
            "sprinting" => p.sprinting = bit(v),
            "shift" => p.shift_key_down = bit(v),
            "swimming" => p.swimming = bit(v),
            "fire" => p.remaining_fire_ticks = v.as_i64().unwrap_or(0) as i32,
            "frozen" => p.ticks_frozen = v.as_i64().unwrap_or(0) as i32,
            "age" => p.tick_count = v.as_i64().unwrap_or(0) as i32,
            "invul" => p.invulnerable_time = v.as_i64().unwrap_or(0) as i32,
            "health" => p.health = f32_bits(v),
            "absorption" => p.absorption = f32_bits(v),
            "hurtTime" => p.hurt_time = v.as_i64().unwrap_or(0) as i32,
            "lastHurt" => p.last_hurt = f32_bits(v),
            "deathTime" => p.death_time = v.as_i64().unwrap_or(0) as i32,
            "njd" => p.no_jump_delay = v.as_i64().unwrap_or(0) as i32,
            "jumping" => p.jumping = bit(v),
            "xxa" => p.xxa = f32_bits(v),
            "zza" => p.zza = f32_bits(v),
            "speed" => p.speed = f32_bits(v),
            "effects" => {
                let mut e = Effects::default();
                for x in v.as_array().into_iter().flatten() {
                    e.insert(EffectInstance {
                        id: x["id"].as_str().unwrap_or_default().to_string(),
                        amplifier: x["amp"].as_i64().unwrap_or(0) as i32,
                        duration: x["dur"].as_i64().unwrap_or(0) as i32,
                    });
                }
                p.effects = e;
            }
            "food" => p.food = v.as_i64().unwrap_or(20) as i32,
            "saturation" => p.saturation = f32_bits(v),
            "exhaustion" => p.exhaustion = f32_bits(v),
            "jumpTrigger" => p.jump_trigger_time = v.as_i64().unwrap_or(0) as i32,
            "sprintTrigger" => p.sprint_trigger_time = v.as_i64().unwrap_or(0) as i32,
            "flying" => p.flying = bit(v),
            "crouching" => p.crouching = bit(v),
            _ => {}
        }
    }
}

/// Apply a row's `pre` (what the server changed since `prev_post`, the previous row's end state;
/// `None` for the first row): every plain field is written directly, while effects, the sprint
/// flag and attribute updates go through the kernel's client-side effect handling, so attribute
/// modifiers stay consistent with the effects. This is what a replay should use; [`apply_state`]
/// is the raw field writer.
pub fn apply_pre(p: &mut PlayerState, pre: &Fields, prev_post: Option<&Fields>) {
    let mut plain = pre.clone();
    let effects = plain.remove("effects").map(|v| parse_effects(&v));
    let sprinting = plain.remove("sprinting").map(|v| bit(&v));
    let attrs = plain.remove("attrs");
    apply_state(p, &plain);
    let updated: Vec<Attribute> = match &attrs {
        Some(Value::Object(now)) => Attribute::ALL
            .into_iter()
            .filter(|a| {
                let before = prev_post
                    .and_then(|f| f.get("attrs"))
                    .and_then(|v| v.get(a.name()));
                prev_post.is_none() || before != now.get(a.name())
            })
            .collect(),
        _ => Vec::new(),
    };
    ms_kernel::effects::client_apply_server_changes(p, effects.as_deref(), sprinting, &updated);
}

fn parse_effects(v: &Value) -> Vec<EffectInstance> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|x| EffectInstance {
            id: x["id"].as_str().unwrap_or_default().to_string(),
            amplifier: x["amp"].as_i64().unwrap_or(0) as i32,
            duration: x["dur"].as_i64().unwrap_or(0) as i32,
        })
        .collect()
}

/// One field that differs between a simulated state and the recording.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldDiff {
    pub field: String,
    pub expected: String,
    pub actual: String,
}

/// The state of `p` serialized exactly like the corpus records it (field name → JSON value), for
/// comparison and for writing traces of simulator runs.
pub fn state_fields(p: &PlayerState) -> Fields {
    let d = |x: f64| Value::from(x.to_bits() as i64);
    let f = |x: f32| Value::from(x.to_bits() as i32);
    let b = |x: bool| Value::from(i64::from(x));
    let mut m = Fields::new();
    m.insert("x".into(), d(p.pos.x));
    m.insert("y".into(), d(p.pos.y));
    m.insert("z".into(), d(p.pos.z));
    m.insert("dx".into(), d(p.vel.x));
    m.insert("dy".into(), d(p.vel.y));
    m.insert("dz".into(), d(p.vel.z));
    m.insert("yaw".into(), f(p.yaw));
    m.insert("pitch".into(), f(p.pitch));
    m.insert("ground".into(), b(p.on_ground));
    m.insert("hc".into(), b(p.horizontal_collision));
    m.insert("mhc".into(), b(p.minor_horizontal_collision));
    m.insert("vc".into(), b(p.vertical_collision));
    m.insert("vcb".into(), b(p.vertical_collision_below));
    m.insert("fall".into(), d(p.fall_distance));
    m.insert("water".into(), b(p.in_water));
    m.insert("eyeWater".into(), b(p.eye_in_water));
    m.insert("lava".into(), b(p.in_lava));
    m.insert("waterH".into(), d(p.water_height));
    m.insert("lavaH".into(), d(p.lava_height));
    m.insert("powder".into(), b(p.in_powder_snow));
    m.insert("wasPowder".into(), b(p.was_in_powder_snow));
    m.insert("stuckX".into(), d(p.stuck_speed_multiplier.x));
    m.insert("stuckY".into(), d(p.stuck_speed_multiplier.y));
    m.insert("stuckZ".into(), d(p.stuck_speed_multiplier.z));
    if let Some((x, y, z)) = p.supporting_block {
        m.insert(
            "support".into(),
            Value::from(ms_world::coords::BlockPos::new(x, y, z).as_long()),
        );
    }
    m.insert("noBlocks".into(), b(p.on_ground_no_blocks));
    m.insert("pose".into(), Value::from(p.pose.name()));
    let (w, h) = p.dimensions();
    m.insert("w".into(), f(w));
    m.insert("h".into(), f(h));
    m.insert("sprinting".into(), b(p.sprinting));
    m.insert("shift".into(), b(p.shift_key_down));
    m.insert("swimming".into(), b(p.swimming));
    m.insert("fire".into(), Value::from(p.remaining_fire_ticks));
    m.insert("frozen".into(), Value::from(p.ticks_frozen));
    m.insert("invul".into(), Value::from(p.invulnerable_time));
    m.insert("health".into(), f(p.health));
    m.insert("absorption".into(), f(p.absorption));
    m.insert("hurtTime".into(), Value::from(p.hurt_time));
    m.insert("lastHurt".into(), f(p.last_hurt));
    m.insert("deathTime".into(), Value::from(p.death_time));
    m.insert("njd".into(), Value::from(p.no_jump_delay));
    m.insert("jumping".into(), b(p.jumping));
    m.insert("xxa".into(), f(p.xxa));
    m.insert("zza".into(), f(p.zza));
    m.insert("speed".into(), f(p.speed));
    let effects: Vec<Value> = p
        .effects
        .iter()
        .map(|e| serde_json::json!({"id": e.id, "amp": e.amplifier, "dur": e.duration}))
        .collect();
    m.insert("effects".into(), Value::from(effects));
    let mut attrs = Fields::new();
    for a in Attribute::ALL {
        attrs.insert(a.name().into(), d(p.attributes.value(a)));
    }
    m.insert("attrs".into(), Value::Object(attrs));
    m.insert("food".into(), Value::from(p.food));
    m.insert("saturation".into(), f(p.saturation));
    m.insert("exhaustion".into(), f(p.exhaustion));
    m.insert("jumpTrigger".into(), Value::from(p.jump_trigger_time));
    m.insert("sprintTrigger".into(), Value::from(p.sprint_trigger_time));
    m.insert("flying".into(), b(p.flying));
    m.insert("crouching".into(), b(p.crouching));
    m
}

/// Fields the comparison skips: counters the simulator does not model (`age` is the entity's
/// lifetime tick count; `climbable`/`fallFlying` are derived queries, checked through their
/// effects).
pub const UNCOMPARED: &[&str] = &["age", "climbable", "fallFlying"];

/// Every field of `expected` (a recorded `post`) that `p` does not reproduce bit-for-bit. Effects
/// compare as sets; attributes compare per attribute.
pub fn compare_state(p: &PlayerState, expected: &Fields) -> Vec<FieldDiff> {
    let actual = state_fields(p);
    let mut out = Vec::new();
    for (k, want) in expected {
        if UNCOMPARED.contains(&k.as_str()) {
            continue;
        }
        let got = actual.get(k).cloned().unwrap_or(Value::Null);
        let same = match (k.as_str(), want, &got) {
            ("effects", Value::Array(a), Value::Array(b)) => {
                let key = |v: &Value| v.to_string();
                let mut a: Vec<String> = a.iter().map(key).collect();
                let mut b: Vec<String> = b.iter().map(key).collect();
                a.sort();
                b.sort();
                a == b
            }
            ("attrs", Value::Object(a), Value::Object(b)) => {
                a.iter().all(|(n, v)| b.get(n) == Some(v))
            }
            _ => *want == got,
        };
        if !same {
            out.push(FieldDiff {
                field: k.clone(),
                expected: describe(k, want),
                actual: describe(k, &got),
            });
        }
    }
    if !expected.contains_key("support") && p.supporting_block.is_some() {
        out.push(FieldDiff {
            field: "support".into(),
            expected: "none".into(),
            actual: format!("{:?}", p.supporting_block),
        });
    }
    out
}

/// Human-readable form of a recorded value (decoding the raw bits of float fields).
pub fn describe(field: &str, v: &Value) -> String {
    const F64: &[&str] = &[
        "x", "y", "z", "dx", "dy", "dz", "fall", "waterH", "lavaH", "stuckX", "stuckY", "stuckZ",
    ];
    const F32: &[&str] = &[
        "yaw",
        "pitch",
        "w",
        "h",
        "health",
        "absorption",
        "lastHurt",
        "xxa",
        "zza",
        "speed",
        "saturation",
        "exhaustion",
    ];
    match v.as_i64() {
        Some(bits) if F64.contains(&field) => format!("{:e}", f64::from_bits(bits as u64)),
        Some(bits) if F32.contains(&field) => format!("{:e}", f32::from_bits(bits as i32 as u32)),
        Some(packed) if field == "support" => format!("{:?}", unpack_block_pos(packed)),
        _ => v.to_string(),
    }
}
