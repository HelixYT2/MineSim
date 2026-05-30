//! Developer tasks, run via `cargo xtask <task>`.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::process::ExitCode;

const BLOCKS_JSON: &str = "tools/mc/generated/reports/blocks.json";
const MINESIM_BLOCKS: &str = "tools/mc/minesim-blocks.json";
const GENERATED_RS: &str = "crates/ms-data/src/generated.rs";
const GENERATED_SHAPES_RS: &str = "crates/ms-data/src/generated_shapes.rs";
const STATE_SHAPE_BIN: &str = "crates/ms-data/data/state_shape.bin";

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("regen-data") => match regen_data() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("regen-data failed: {e}");
                ExitCode::FAILURE
            }
        },
        Some("oracle") => {
            println!("oracle: not yet implemented");
            ExitCode::SUCCESS
        }
        Some("probe-world") => {
            let a: Vec<String> = std::env::args().skip(2).collect();
            if a.len() != 4 {
                eprintln!("usage: cargo xtask probe-world <region-dir> <x> <y> <z>");
                return ExitCode::from(2);
            }
            let world = ms_world::World::new(&a[0]);
            let (x, y, z) = (
                a[1].parse().unwrap(),
                a[2].parse().unwrap(),
                a[3].parse().unwrap(),
            );
            println!("block at ({x},{y},{z}) = {:?}", world.block_name(x, y, z));
            ExitCode::SUCCESS
        }
        Some("replay-walk") => {
            let a: Vec<String> = std::env::args().skip(2).collect();
            if a.len() != 2 {
                eprintln!("usage: cargo xtask replay-walk <region-dir> <trace-csv>");
                return ExitCode::from(2);
            }
            match replay_walk(&a[0], &a[1]) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("replay-walk failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("freerun") => {
            let a: Vec<String> = std::env::args().skip(2).collect();
            if a.len() != 2 {
                eprintln!("usage: cargo xtask freerun <region-dir> <trace-csv>");
                return ExitCode::from(2);
            }
            match freerun(&a[0], &a[1]) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("freerun failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("bench") => {
            let a: Vec<String> = std::env::args().skip(2).collect();
            let envs = a.first().and_then(|s| s.parse().ok()).unwrap_or(4096usize);
            let ticks = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(1000usize);
            bench(envs, ticks);
            ExitCode::SUCCESS
        }
        other => {
            if let Some(task) = other {
                eprintln!("unknown task: {task}");
            }
            eprintln!("usage: cargo xtask <regen-data|oracle|bench>");
            ExitCode::from(2)
        }
    }
}

type Shape = Vec<[u64; 6]>;

/// Regenerates the committed `ms-data` tables from the datagen `blocks.json` (block/state
/// registry) and the mod-extracted `minesim-blocks.json` (per-state collision shapes and
/// friction). State IDs are contiguous per block, so a block is the half-open range
/// `[FIRST_STATE[i], FIRST_STATE[i + 1])`. Collision shapes are deduplicated since most blocks
/// share a few (full cube, empty, the slab/stair variants); each state stores a shape index.
fn regen_data() -> Result<(), Box<dyn std::error::Error>> {
    let blocks_json: serde_json::Value = serde_json::from_str(&fs::read_to_string(BLOCKS_JSON)?)?;
    let obj = blocks_json
        .as_object()
        .ok_or("blocks.json: expected a top-level object")?;

    let mut blocks: Vec<(u32, u32, String)> = Vec::with_capacity(obj.len());
    let mut state_count = 0u32;
    for (name, def) in obj {
        let states = def["states"]
            .as_array()
            .ok_or("block: missing states array")?;
        let mut first = u32::MAX;
        let mut default = None;
        for state in states {
            let id = u32::try_from(state["id"].as_u64().ok_or("state: missing id")?)?;
            first = first.min(id);
            state_count = state_count.max(id + 1);
            if state.get("default").and_then(serde_json::Value::as_bool) == Some(true) {
                default = Some(id);
            }
        }
        blocks.push((first, default.unwrap_or(first), name.clone()));
    }
    blocks.sort_by_key(|&(first, _, _)| first);

    let n = state_count as usize;
    let mut friction = vec![0u32; n];
    let mut state_shape_raw: Vec<Shape> = vec![Vec::new(); n];
    let coll: serde_json::Value = serde_json::from_str(&fs::read_to_string(MINESIM_BLOCKS)?)?;
    let coll = coll
        .as_object()
        .ok_or("minesim-blocks.json: expected a top-level object")?;
    let mut covered = 0usize;
    for (id_str, v) in coll {
        let id: usize = id_str.parse()?;
        if id >= n {
            continue;
        }
        covered += 1;
        friction[id] = v["friction"].as_i64().ok_or("missing friction")? as i32 as u32;
        let mut shape = Shape::new();
        for b in v["aabbs"].as_array().ok_or("missing aabbs")? {
            let nums = b.as_array().ok_or("aabb: expected array")?;
            let mut coords = [0u64; 6];
            for (k, slot) in coords.iter_mut().enumerate() {
                *slot = nums[k].as_i64().ok_or("aabb: expected number")? as u64;
            }
            shape.push(coords);
        }
        state_shape_raw[id] = shape;
    }
    if covered != n {
        eprintln!("warning: minesim-blocks covered {covered}/{n} states");
    }

    let mut interner: HashMap<Shape, u16> = HashMap::new();
    let mut shapes: Vec<Shape> = Vec::new();
    let mut state_shape = vec![0u16; n];
    for (s, raw) in state_shape_raw.into_iter().enumerate() {
        let next = u16::try_from(shapes.len())?;
        let idx = *interner.entry(raw.clone()).or_insert_with(|| {
            shapes.push(raw);
            next
        });
        state_shape[s] = idx;
    }

    // Which blocks have more than one distinct collision shape across their states (slabs,
    // stairs, fences, walls, doors, ...). For single-shape blocks, name -> default suffices.
    let mut multi_shape = vec![false; blocks.len()];
    for bi in 0..blocks.len() {
        let first = blocks[bi].0;
        let end = if bi + 1 < blocks.len() {
            blocks[bi + 1].0
        } else {
            state_count
        };
        let mut seen_shape: Option<u16> = None;
        for st in first..end {
            let sh = state_shape[st as usize];
            match seen_shape {
                None => seen_shape = Some(sh),
                Some(s) if s != sh => {
                    multi_shape[bi] = true;
                    break;
                }
                _ => {}
            }
        }
    }

    // Property-aware shape lookup keyed by the fastanvil `encoded_description` form
    // ("name|prop=val,...", sorted, dropping waterlogged/powered), only for multi-shape blocks.
    let mut encoded: Vec<(String, u16)> = Vec::new();
    let mut seen_key = HashSet::new();
    for (name, def) in obj {
        for state in def["states"].as_array().ok_or("missing states")? {
            let id = u32::try_from(state["id"].as_u64().ok_or("missing id")?)?;
            let block = blocks.partition_point(|&(f, _, _)| f <= id) - 1;
            if !multi_shape[block] {
                continue;
            }
            let key = encoded_key(name, state.get("properties"));
            if seen_key.insert(key.clone()) {
                encoded.push((key, state_shape[id as usize]));
            }
        }
    }
    encoded.sort_by(|a, b| a.0.cmp(&b.0));

    write_registry(&blocks, state_count)?;
    write_shapes(&blocks, &friction, &shapes, &encoded)?;

    let mut bin = Vec::with_capacity(n * 2);
    for idx in &state_shape {
        bin.extend_from_slice(&idx.to_le_bytes());
    }
    if let Some(parent) = std::path::Path::new(STATE_SHAPE_BIN).parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(STATE_SHAPE_BIN, bin)?;

    println!(
        "{} blocks, {state_count} states, {} unique collision shapes",
        blocks.len(),
        shapes.len()
    );
    Ok(())
}

fn write_registry(blocks: &[(u32, u32, String)], state_count: u32) -> std::io::Result<()> {
    let mut out = String::from(
        "// @generated by `cargo xtask regen-data` from Minecraft 1.21.11 datagen.\n\
         // Do not edit by hand.\n\n",
    );
    let _ = writeln!(out, "pub const BLOCK_STATE_COUNT: u32 = {state_count};");
    let _ = writeln!(out, "pub const BLOCK_COUNT: usize = {};\n", blocks.len());
    out.push_str("pub static BLOCK_NAMES: &[&str] = &[\n");
    for (_, _, name) in blocks {
        let _ = writeln!(out, "    {name:?},");
    }
    out.push_str("];\n\npub static FIRST_STATE: &[u32] = &[\n");
    for &(first, _, _) in blocks {
        let _ = writeln!(out, "    {first},");
    }
    out.push_str("];\n\npub static DEFAULT_STATE: &[u32] = &[\n");
    for &(_, default, _) in blocks {
        let _ = writeln!(out, "    {default},");
    }
    out.push_str("];\n");
    fs::write(GENERATED_RS, out)
}

fn write_shapes(
    blocks: &[(u32, u32, String)],
    friction: &[u32],
    shapes: &[Shape],
    encoded: &[(String, u16)],
) -> std::io::Result<()> {
    let mut out = String::from(
        "// @generated by `cargo xtask regen-data` from Minecraft 1.21.11.\n\
         // Do not edit by hand. Collision coordinates and friction are raw IEEE-754 bits.\n\n",
    );
    out.push_str("pub static SHAPES: &[&[[f64; 6]]] = &[\n");
    for shape in shapes {
        out.push_str("    &[");
        for (bi, b) in shape.iter().enumerate() {
            if bi > 0 {
                out.push_str(", ");
            }
            out.push('[');
            for (k, &c) in b.iter().enumerate() {
                if k > 0 {
                    out.push_str(", ");
                }
                let _ = write!(out, "f64::from_bits({c:#018x})");
            }
            out.push(']');
        }
        out.push_str("],\n");
    }
    out.push_str("];\n\npub static BLOCK_FRICTION: &[f32] = &[\n");
    for &(first, _, _) in blocks {
        let _ = writeln!(
            out,
            "    f32::from_bits({:#010x}),",
            friction[first as usize]
        );
    }
    out.push_str("];\n\n// Property-aware shape lookup (sorted by key) for multi-shape blocks.\n");
    out.push_str("pub static ENCODED: &[(&str, u16)] = &[\n");
    for (key, idx) in encoded {
        let _ = writeln!(out, "    ({key:?}, {idx}),");
    }
    out.push_str("];\n");
    fs::write(GENERATED_SHAPES_RS, out)
}

/// Free-runs a recorded walk through the `Arena` (no per-tick re-seed), reporting the longest
/// unbroken bit-exact streak. Resyncs to the trace after each divergence to measure subsequent
/// streaks (so unmodeled regimes — flight/swim/entities — don't poison the whole run).
fn freerun(region_dir: &str, csv_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    use ms_arena::{Action, Arena, Player};
    use ms_kernel::player::Keys;
    use ms_numerics::Vec3;
    use ms_world::World;

    struct Row {
        pos: Vec3,
        vel: Vec3,
        yaw: f32,
        on_ground: bool,
        sprinting: bool,
        sneaking: bool,
        keys: Keys,
        jump: bool,
    }

    let text = fs::read_to_string(csv_path)?;
    let mut rows = Vec::new();
    for line in text.lines().skip(1) {
        let c: Vec<&str> = line.split(',').collect();
        if c.len() < 19 {
            continue;
        }
        let d = |i: usize| f64::from_bits(c[i].parse::<i64>().unwrap() as u64);
        let fl = |i: usize| f32::from_bits(c[i].parse::<i32>().unwrap() as u32);
        let b = |i: usize| c[i] == "1";
        rows.push(Row {
            pos: Vec3::new(d(1), d(2), d(3)),
            vel: Vec3::new(d(4), d(5), d(6)),
            yaw: fl(7),
            on_ground: b(9),
            sprinting: b(10),
            sneaking: b(11),
            keys: Keys {
                forward: b(12),
                back: b(13),
                left: b(14),
                right: b(15),
            },
            jump: b(16),
        });
    }
    if rows.is_empty() {
        return Err("empty trace".into());
    }

    let world = World::new(region_dir);
    let r0 = &rows[0];
    let mut arena = Arena::new(world, r0.pos, r0.yaw);
    arena.set_state(Player {
        pos: r0.pos,
        vel: r0.vel,
        yaw: r0.yaw,
        on_ground: r0.on_ground,
        no_jump_delay: 0,
    });

    let (mut streak, mut best, mut best_start, mut cur_start, mut breaks) = (0, 0, 0, 0, 0);
    for (t, n) in rows.iter().enumerate().skip(1) {
        arena.step(&Action {
            keys: n.keys,
            jump: n.jump,
            sprinting: n.sprinting,
            sneaking: n.sneaking,
            yaw: n.yaw,
        });
        let p = arena.player;
        let exact = p.pos.x.to_bits() == n.pos.x.to_bits()
            && p.pos.y.to_bits() == n.pos.y.to_bits()
            && p.pos.z.to_bits() == n.pos.z.to_bits()
            && p.vel.x.to_bits() == n.vel.x.to_bits()
            && p.vel.y.to_bits() == n.vel.y.to_bits()
            && p.vel.z.to_bits() == n.vel.z.to_bits();
        if exact {
            if streak == 0 {
                cur_start = t;
            }
            streak += 1;
            if streak > best {
                best = streak;
                best_start = cur_start;
            }
        } else {
            streak = 0;
            breaks += 1;
            arena.set_state(Player {
                pos: n.pos,
                vel: n.vel,
                yaw: n.yaw,
                on_ground: n.on_ground,
                no_jump_delay: p.no_jump_delay,
            });
        }
    }
    println!(
        "free-run: longest bit-exact streak {best} ticks (from tick {best_start}); {breaks} resyncs over {} ticks",
        rows.len() - 1
    );
    Ok(())
}

/// The fastanvil `encoded_description` form: `"name|prop=val,..."`, properties sorted, dropping
/// `waterlogged`/`powered` (which never change collision shape).
fn encoded_key(name: &str, properties: Option<&serde_json::Value>) -> String {
    let mut key = format!("{name}|");
    if let Some(map) = properties.and_then(serde_json::Value::as_object) {
        let mut kv: Vec<(&str, &str)> = map
            .iter()
            .filter(|(k, _)| k.as_str() != "waterlogged" && k.as_str() != "powered")
            .map(|(k, v)| (k.as_str(), v.as_str().unwrap_or("")))
            .collect();
        kv.sort_unstable();
        let mut sep = "";
        for (k, v) in kv {
            key.push_str(sep);
            key.push_str(k);
            key.push('=');
            key.push_str(v);
            sep = ",";
        }
    }
    key
}

/// Replays a recorded walk through the full world-collision step, comparing predicted velocity
/// and position to the trace tick-by-tick (re-seeding state from the trace each tick, so this is
/// a per-step check). Reports how many ticks reproduce bit-for-bit.
fn replay_walk(region_dir: &str, csv_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    use ms_kernel::player::{step, Keys};
    use ms_numerics::Vec3;
    use ms_world::World;

    struct Row {
        pos: Vec3,
        vel: Vec3,
        yaw: f32,
        on_ground: bool,
        sprinting: bool,
        sneaking: bool,
        keys: Keys,
        jump: bool,
    }

    let text = fs::read_to_string(csv_path)?;
    let mut rows = Vec::new();
    for line in text.lines().skip(1) {
        let c: Vec<&str> = line.split(',').collect();
        if c.len() < 19 {
            continue;
        }
        let d = |i: usize| f64::from_bits(c[i].parse::<i64>().unwrap() as u64);
        let fl = |i: usize| f32::from_bits(c[i].parse::<i32>().unwrap() as u32);
        let b = |i: usize| c[i] == "1";
        rows.push(Row {
            pos: Vec3::new(d(1), d(2), d(3)),
            vel: Vec3::new(d(4), d(5), d(6)),
            yaw: fl(7),
            on_ground: b(9),
            sprinting: b(10),
            sneaking: b(11),
            keys: Keys {
                forward: b(12),
                back: b(13),
                left: b(14),
                right: b(15),
            },
            jump: b(16),
        });
    }

    let world = World::new(region_dir);
    let mut no_jump_delay = 0i32;
    let (mut total, mut vel_exact, mut pos_exact, mut diverged) = (0usize, 0usize, 0usize, 0usize);
    // Grounded-walking subset: where collision against terrain actually matters.
    let (mut walk_total, mut walk_exact, mut walk_diverged) = (0usize, 0usize, 0usize);
    let mut worst = 0.0_f64;
    let mut worst_info = String::new();
    for t in 0..rows.len() - 1 {
        let a = &rows[t];
        let n = &rows[t + 1];
        let walking = a.on_ground && n.on_ground;
        let (pp, pv, _) = step(
            a.pos,
            a.vel,
            n.yaw,
            a.on_ground,
            n.sprinting,
            n.sneaking,
            n.keys,
            n.jump,
            &mut no_jump_delay,
            &world,
        );
        total += 1;
        let ve = pv.x.to_bits() == n.vel.x.to_bits()
            && pv.y.to_bits() == n.vel.y.to_bits()
            && pv.z.to_bits() == n.vel.z.to_bits();
        if ve {
            vel_exact += 1;
        }
        if pp.x.to_bits() == n.pos.x.to_bits()
            && pp.y.to_bits() == n.pos.y.to_bits()
            && pp.z.to_bits() == n.pos.z.to_bits()
        {
            pos_exact += 1;
        }
        let err = (pv.x - n.vel.x)
            .abs()
            .max((pv.y - n.vel.y).abs())
            .max((pv.z - n.vel.z).abs());
        if err >= 1.0e-6 {
            diverged += 1;
        }
        if walking {
            walk_total += 1;
            if ve {
                walk_exact += 1;
            } else if err >= 1.0e-6 {
                walk_diverged += 1;
                if err > worst {
                    worst = err;
                    worst_info = format!(
                        "tick {}: pred_vel=({},{},{}) actual=({},{},{}) jump={} fwd={} sprint={} yaw={}",
                        t + 1,
                        pv.x,
                        pv.y,
                        pv.z,
                        n.vel.x,
                        n.vel.y,
                        n.vel.z,
                        n.jump,
                        n.keys.forward,
                        n.sprinting,
                        n.yaw
                    );
                }
            }
        }
    }
    println!("all ticks:        total={total} vel_exact={vel_exact} pos_exact={pos_exact} diverged={diverged}");
    println!("grounded-walking: total={walk_total} vel_exact={walk_exact} diverged={walk_diverged} worst={worst:e}");
    if !worst_info.is_empty() {
        println!("worst: {worst_info}");
    }
    Ok(())
}

/// Measures stepping throughput on flat terrain with a sprint-jump workload: one env, the whole
/// batch on a single thread, and the whole batch across the rayon pool. Build with `--release`;
/// debug numbers are not representative.
fn bench(envs: usize, ticks: usize) {
    use ms_arena::{Action, Arena, BatchArena};
    use ms_kernel::player::Keys;
    use ms_numerics::Vec3;
    use ms_world::World;
    use std::time::Instant;

    let make = |i: usize| Arena::new(World::flat(0), Vec3::new(0.5, 0.0, 0.5), (i % 360) as f32);
    let actions: Vec<Action> = (0..envs)
        .map(|i| Action {
            keys: Keys {
                forward: true,
                back: false,
                left: false,
                right: false,
            },
            jump: true,
            sprinting: true,
            sneaking: false,
            yaw: (i % 360) as f32,
        })
        .collect();

    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    println!("MineSim throughput  (flat world, sprint+jump)");
    println!("  envs={envs}  ticks={ticks}  hardware-threads={threads}\n");

    {
        let mut one = BatchArena::from_fn(1, make);
        let act = actions[..1].to_vec();
        for _ in 0..100 {
            one.step_serial(&act);
        }
        let t = Instant::now();
        for _ in 0..ticks {
            one.step_serial(&act);
        }
        report(
            "single arena  (1 env, 1 thread)",
            ticks,
            t.elapsed().as_secs_f64(),
        );
    }

    let serial_rate = {
        let mut b = BatchArena::from_fn(envs, make);
        for _ in 0..10 {
            b.step_serial(&actions);
        }
        let t = Instant::now();
        for _ in 0..ticks {
            b.step_serial(&actions);
        }
        let secs = t.elapsed().as_secs_f64();
        report("batch serial  (1 thread)       ", envs * ticks, secs);
        (envs * ticks) as f64 / secs
    };

    {
        let mut b = BatchArena::from_fn(envs, make);
        for _ in 0..10 {
            b.step(&actions);
        }
        let t = Instant::now();
        for _ in 0..ticks {
            b.step(&actions);
        }
        let secs = t.elapsed().as_secs_f64();
        let rate = (envs * ticks) as f64 / secs;
        report("batch parallel (rayon)         ", envs * ticks, secs);
        println!("  parallel speedup over serial: {:.1}x", rate / serial_rate);
    }
}

fn report(label: &str, env_ticks: usize, secs: f64) {
    let rate = env_ticks as f64 / secs;
    println!(
        "  {label}: {:>7.2} M env-ticks/s  ({:.1} ns/env-tick)",
        rate / 1.0e6,
        1.0e9 / rate
    );
}
