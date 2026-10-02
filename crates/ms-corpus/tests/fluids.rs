//! The fluid port checked against the oracle corpus.
//!
//! Isolated checks (every tick of every fluid scenario, from the recorded start-of-tick state):
//! `update_in_fluid_state_and_push` must reproduce the recorded `water`, `lava`, `waterH`, `lavaH`
//! (and the fall-distance reset on touching water), `update_fluid_on_eyes` the `eyeWater` flag and
//! `update_swimming` the `swimming` flag, bit for bit.
//!
//! Where the recording shows end-of-tick state, the game's order matters: the water state recorded
//! after a tick that *began* out of water comes from `LivingEntity.checkFallDamage`, which re-checks
//! the water at the position the move ended at (see the module docs of `ms_kernel::fluids`). The
//! test mirrors that: it re-runs the water half at the recorded end position, with the box of the
//! start-of-tick pose.
//!
//! The full-tick check drives `travel_in_fluid` with a minimal `Entity.move` (per-axis collision
//! with step-up, no edge back-off) and replays whole ticks that start in fluid.

use ms_corpus::{apply_state, compare_state, FieldDiff, Fields, Scenario};
use ms_kernel::attributes::{Attribute, Modifier, Operation};
use ms_kernel::{collision, fluids};
use ms_kernel::{Input, PlayerState};
use ms_numerics::{mth, Vec3};
use ms_world::aabb::Aabb;
use ms_world::World;

const FLUID_SCENARIOS: &[&str] = &[
    "water_pool",
    "water_swim",
    "water_flow",
    "lava_pool",
    "bubble_columns",
    "fall_into_water",
];

fn subset(post: &Fields, keys: &[&str]) -> Fields {
    keys.iter()
        .filter_map(|k| post.get(*k).map(|v| ((*k).to_string(), v.clone())))
        .collect()
}

/// Compare the named fields of `p` with the recorded `post` row.
fn diff_fields(p: &PlayerState, post: &Fields, keys: &[&str]) -> Vec<FieldDiff> {
    let mut q = p.clone();
    // `compare_state` flags an unrecorded supporting block; this check only looks at `keys`.
    q.supporting_block = None;
    compare_state(&q, &subset(post, keys))
}

#[derive(Default, Debug)]
struct Stats {
    ticks: usize,
    in_water_ticks: usize,
    in_lava_ticks: usize,
    rechecked_ticks: usize,
    eye_ticks: usize,
    swimming_ticks: usize,
    mismatches: Vec<String>,
}

impl Stats {
    fn note(&mut self, name: &str, tick: usize, what: &str, diffs: Vec<FieldDiff>) {
        for d in diffs {
            self.mismatches.push(format!(
                "{name} t={tick} [{what}] {}: expected {} got {}",
                d.field, d.expected, d.actual
            ));
        }
    }
}

/// `fluidOnEyes` as it stood before the first recorded tick: computed for the start position
/// (the player is placed and settled there before recording starts).
fn initial_water_on_eyes(p: &PlayerState, world: &World) -> bool {
    let mut q = p.clone();
    fluids::update_fluid_on_eyes(&mut q, world);
    q.water_on_eyes
}

fn replay_fluid_state(name: &str) -> Stats {
    let s = Scenario::load(name).unwrap();
    let world = s.world();
    let mut p = s.initial_state();
    p.water_on_eyes = initial_water_on_eyes(&p, &world);
    let mut st = Stats::default();
    for (i, row) in s.rows.iter().enumerate() {
        if i > 0 {
            apply_state(&mut p, &row.pre);
        }
        st.ticks += 1;
        let start_pos = p.pos;
        // Entity.baseTick: water and lava, then the eyes, then swimming.
        fluids::update_in_fluid_state_and_push(&mut p, &world);
        let was_in_water = p.in_water;
        fluids::update_fluid_on_eyes(&mut p, &world);
        fluids::update_swimming(&mut p, &world);

        // The recorded end-of-tick water state is the start-of-tick one if the player was in water,
        // else the re-check at the end position (checkFallDamage).
        let mut end = p.clone();
        if !was_in_water {
            st.rechecked_ticks += 1;
            apply_state(&mut end, &subset(&row.post, &["x", "y", "z"]));
            fluids::update_in_water_state_and_push(&mut end, &world);
        }
        st.note(
            name,
            i,
            "water/lava state",
            diff_fields(&end, &row.post, &["water", "waterH", "lava", "lavaH"]),
        );
        if row.post["water"].as_i64() == Some(1) {
            st.in_water_ticks += 1;
            // Touching water at the end of the tick leaves no fall distance behind.
            st.note(name, i, "fall", diff_fields(&end, &row.post, &["fall"]));
        }
        if row.post["lava"].as_i64() == Some(1) {
            st.in_lava_ticks += 1;
        }
        st.eye_ticks += 1;
        st.note(name, i, "eyes", diff_fields(&p, &row.post, &["eyeWater"]));
        if row.post["swimming"].as_i64() == Some(1) || p.swimming {
            st.swimming_ticks += 1;
        }
        st.note(
            name,
            i,
            "swimming",
            diff_fields(&p, &row.post, &["swimming"]),
        );
        let _ = start_pos;
        // Continue from the recorded end-of-tick state.
        apply_state(&mut p, &row.post);
    }
    st
}

#[test]
fn fluid_state_matches_the_corpus_bit_for_bit() {
    let mut total = 0;
    let mut all = Vec::new();
    for name in FLUID_SCENARIOS {
        let st = replay_fluid_state(name);
        eprintln!(
            "{name}: {} ticks, {} in water, {} in lava, {} re-checked after the move, {} swimming; {} mismatches",
            st.ticks,
            st.in_water_ticks,
            st.in_lava_ticks,
            st.rechecked_ticks,
            st.swimming_ticks,
            st.mismatches.len()
        );
        total += st.ticks;
        all.extend(st.mismatches);
    }
    eprintln!("{total} ticks checked");
    assert!(
        all.is_empty(),
        "{} mismatches, first few:\n{}",
        all.len(),
        all.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// The same isolated check over every committed scenario (courses with pools, ladders, slime,
/// cobwebs, projectile arenas, ...), most of which never touch fluid: the port must not invent any.
#[test]
fn fluid_state_matches_in_every_committed_scenario() {
    let mut total = 0;
    let mut wet = 0;
    let mut all = Vec::new();
    for name in ms_corpus::client_scenarios() {
        let st = replay_fluid_state(&name);
        total += st.ticks;
        wet += st.in_water_ticks + st.in_lava_ticks;
        all.extend(st.mismatches);
    }
    eprintln!("{total} ticks over every scenario, {wet} in fluid");
    assert!(
        all.is_empty(),
        "{} mismatches, first few:\n{}",
        all.len(),
        all.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
}

// ---------------------------------------------------------------------------------------------
// Whole in-fluid ticks
// ---------------------------------------------------------------------------------------------
//
// The pieces of `LocalPlayer`/`LivingEntity`/`Entity` around the fluid code, reduced to what a tick
// in water or lava needs: base tick, sink/jump input, velocity rounding, `Player.travel`'s
// swimming rule, `travelInFluid` with a plain `Entity.move` (per-axis collision with step-up, the
// collision flags and velocity zeroing, `checkFallDamage` including its water re-check), and the
// bubble columns of `applyEffectsFromBlocks`. What the recording supplies instead of simulating it:
// the movement input (`xxa`, `zza`), the jump flag and the sprint flag as the tick used them
// (the keyboard/sprint-start rules belong to the core tick).

const DEG_TO_RAD: f32 = (std::f64::consts::PI / 180.0) as f32;

fn player_box(p: &PlayerState) -> Aabb {
    let (w, h) = p.dimensions();
    let half = f64::from(w / 2.0);
    Aabb::new(
        Vec3::new(p.pos.x - half, p.pos.y, p.pos.z - half),
        Vec3::new(p.pos.x + half, p.pos.y + f64::from(h), p.pos.z + half),
    )
}

/// `Mth.equal(double, double)`.
fn mth_equal(a: f64, b: f64) -> bool {
    (b - a).abs() < f64::from(1.0e-5_f32)
}

/// `Entity.move(MoverType.SELF, motion)` for a player with no edge back-off, no stuck-speed
/// multiplier and no fall-damage-resetting clip (none of which these scenarios reach in fluid).
fn entity_move(p: &mut PlayerState, world: &World, motion: Vec3) {
    let bb = player_box(p);
    let step = p.attributes.value(Attribute::StepHeight) as f32;
    let moved = collision::collide_at(p, world, bb, p.on_ground, motion, step);
    let d = moved.x * moved.x + moved.y * moved.y + moved.z * moved.z;
    let want = motion.x * motion.x + motion.y * motion.y + motion.z * motion.z;
    if d > 1.0e-7 || want - d < 1.0e-7 {
        p.pos = Vec3::new(p.pos.x + moved.x, p.pos.y + moved.y, p.pos.z + moved.z);
    }
    let hx = !mth_equal(motion.x, moved.x);
    let hz = !mth_equal(motion.z, moved.z);
    p.horizontal_collision = hx || hz;
    p.vertical_collision = motion.y != moved.y;
    p.vertical_collision_below = p.vertical_collision && motion.y < 0.0;
    p.on_ground = p.vertical_collision_below;
    // LivingEntity.checkFallDamage: catch the water entered during the move, then
    // Entity.checkFallDamage.
    if !p.in_water {
        fluids::update_in_water_state_and_push(p, world);
    }
    if !p.in_water && moved.y < 0.0 {
        p.fall_distance -= f64::from(moved.y as f32);
    }
    if p.on_ground {
        p.fall_distance = 0.0;
    }
    if p.horizontal_collision {
        p.vel = Vec3::new(
            if hx { 0.0 } else { p.vel.x },
            p.vel.y,
            if hz { 0.0 } else { p.vel.z },
        );
    }
    if motion.y != moved.y {
        // Block.updateEntityMovementAfterFallOn (the default: `multiply(1.0, 0.0, 1.0)`, which
        // leaves a negative zero behind a downward velocity).
        p.vel = Vec3::new(p.vel.x * 1.0, p.vel.y * 0.0, p.vel.z * 1.0);
    }
}

/// `LivingEntity.jumpFromGround` (block jump factor 1 in these arenas).
fn jump_from_ground(p: &mut PlayerState) {
    let power = p.attributes.value(Attribute::JumpStrength) as f32 * 1.0 * 1.0
        + ms_kernel::effects::jump_boost_power(p);
    if power > 1.0e-5 {
        p.vel = Vec3::new(p.vel.x, f64::from(power).max(p.vel.y), p.vel.z);
        if p.sprinting {
            let g = p.yaw * DEG_TO_RAD;
            p.vel = Vec3::new(
                p.vel.x + f64::from(-mth::sin(g)) * 0.2,
                p.vel.y,
                p.vel.z + f64::from(mth::cos(g)) * 0.2,
            );
        }
    }
}

fn set_sprint_modifier(p: &mut PlayerState, sprinting: bool) {
    p.attributes
        .remove_modifier(Attribute::MovementSpeed, "minecraft:sprinting");
    if sprinting {
        p.attributes.add_modifier(
            Attribute::MovementSpeed,
            Modifier {
                id: "minecraft:sprinting".into(),
                amount: f64::from(0.3_f32),
                operation: Operation::AddMultipliedTotal,
            },
        );
    }
}

/// `BlockPos.betweenCornersInDirection(aabb, dir)`: the blocks of the box, starting at the corner
/// behind the direction of travel; Y is the slowest axis, then the larger horizontal one, then the
/// smaller (`Direction.axisStepOrder`), each stepping along its direction of travel.
fn blocks_of(bb: Aabb, dir: Vec3) -> Vec<(i32, i32, i32)> {
    let lo = [
        bb.min.x.floor() as i32,
        bb.min.y.floor() as i32,
        bb.min.z.floor() as i32,
    ];
    let hi = [
        bb.max.x.floor() as i32,
        bb.max.y.floor() as i32,
        bb.max.z.floor() as i32,
    ];
    let d = [dir.x, dir.y, dir.z];
    let start: Vec<i32> = (0..3)
        .map(|a| if d[a] >= 0.0 { lo[a] } else { hi[a] })
        .collect();
    let order: [usize; 3] = if dir.x.abs() < dir.z.abs() {
        [1, 2, 0]
    } else {
        [1, 0, 2]
    };
    let step = |a: usize| if d[a] >= 0.0 { 1 } else { -1 };
    let len = |a: usize| hi[a] - lo[a];
    let mut v = Vec::new();
    for i in 0..=len(order[0]) {
        for j in 0..=len(order[1]) {
            for k in 0..=len(order[2]) {
                let mut c = start.clone();
                c[order[0]] += step(order[0]) * i;
                c[order[1]] += step(order[1]) * j;
                c[order[2]] += step(order[2]) * k;
                v.push((c[0], c[1], c[2]));
            }
        }
    }
    v
}

fn box_at(p: &PlayerState, pos: Vec3) -> Aabb {
    let mut q = p.clone();
    q.pos = pos;
    let e = f64::from(1.0e-5_f32);
    player_box(&q).inflate(-e, -e, -e)
}

/// The bubble-column part of `Entity.applyEffectsFromBlocks` for one move from `from` to the
/// current position: the blocks the box touched at the start, then those it touches at the end, each
/// with the game's `precise` flag (a long move, or the end box really overlaps the block).
fn apply_bubble_columns(p: &mut PlayerState, world: &World, from: Vec3) {
    let to = p.pos;
    let end_box = box_at(p, to);
    let moved = Vec3::new(to.x - from.x, to.y - from.y, to.z - from.z);
    let start_box = end_box.move_by(Vec3::new(-moved.x, -moved.y, -moved.z));
    let dist2 = moved.x * moved.x + moved.y * moved.y + moved.z * moved.z;
    let long_move = dist2 > 0.9999900000002526_f64 * 0.9999900000002526_f64;
    let mut visited: Vec<(i32, i32, i32)> = Vec::new();
    let order: Vec<(i32, i32, i32)> = blocks_of(start_box, moved)
        .into_iter()
        .chain(blocks_of(end_box, moved))
        .collect();
    for (x, y, z) in order {
        if visited.contains(&(x, y, z)) {
            continue;
        }
        visited.push((x, y, z));
        let state = world.block_state(x, y, z);
        if ms_data::block_name(ms_data::block_of_state(state)) != "minecraft:bubble_column" {
            continue;
        }
        let cell = Aabb::new(
            Vec3::new(f64::from(x), f64::from(y), f64::from(z)),
            Vec3::new(f64::from(x + 1), f64::from(y + 1), f64::from(z + 1)),
        );
        let precise = long_move || end_box.intersects(cell);
        fluids::bubble_column_entity_inside(p, world, x, y, z, precise);
    }
}

/// One tick of a player that starts in fluid, from the recorded start-of-tick state in `p` (with
/// the recorded post-tick flags the keyboard rules would produce in `post`). Returns false, doing
/// nothing past the base tick, when the tick does not take the in-fluid travel branch.
fn fluid_tick(p: &mut PlayerState, world: &World, input: &Input, post: &Fields) -> bool {
    let from = p.pos;
    // The look direction of this tick (set by the client before the tick runs).
    p.yaw = input.yaw;
    p.pitch = input.pitch;
    // Entity.baseTick
    fluids::update_in_fluid_state_and_push(p, world);
    fluids::update_fluid_on_eyes(p, world);
    fluids::update_swimming(p, world);
    if p.in_lava {
        p.fall_distance *= 0.5;
    }
    if !fluids::should_travel_in_fluid(p, world) {
        return false;
    }
    // LocalPlayer.aiStep: the movement input as the tick used it.
    let f32_of = |k: &str| f32::from_bits(post[k].as_i64().unwrap() as i32 as u32);
    p.sprinting = post["sprinting"].as_i64() == Some(1);
    set_sprint_modifier(p, p.sprinting);
    p.xxa = f32_of("xxa");
    p.zza = f32_of("zza");
    p.jumping = post["jumping"].as_i64() == Some(1);
    if p.in_water && input.shift && fluids::is_affected_by_fluids(p) {
        fluids::go_down_in_water(p);
    }
    // LivingEntity.aiStep
    if p.no_jump_delay > 0 {
        p.no_jump_delay -= 1;
    }
    let v = p.vel;
    let (mut vx, mut vz) = (v.x, v.z);
    if vx * vx + vz * vz < 9.0e-6 {
        vx = 0.0;
        vz = 0.0;
    }
    let vy = if v.y.abs() < 0.003 { 0.0 } else { v.y };
    p.vel = Vec3::new(vx, vy, vz);
    fluids::ai_step_jump(p, &mut jump_from_ground);
    // Player.travel -> LivingEntity.travel
    fluids::swimming_travel_adjust(p, world);
    let input_vec = Vec3::new(f64::from(p.xxa), 0.0, f64::from(p.zza));
    fluids::travel_in_fluid(p, world, input_vec, &mut |p, m| entity_move(p, world, m));
    apply_bubble_columns(p, world, from);
    true
}

/// The fall-distance reset in `Entity.move`: a move of a block or more that crosses water (or a
/// fall-damage-resetting block such as a cobweb) clears the fall distance before the move's own
/// contribution is added. Checked on every recorded tick, in every scenario, that starts out of
/// fluid with a fall in progress and moves at least a block: if it ends airborne out of fluid, the
/// clip must miss and the recorded fall distance must be the old one plus this move's drop; if it
/// ends in water, the clip must hit (the feet crossed the surface).
#[test]
fn long_moves_reset_the_fall_distance_exactly_when_the_clip_hits() {
    let mut misses = 0;
    let mut into_water = 0;
    let mut bad = Vec::new();
    for name in ms_corpus::client_scenarios() {
        let s = Scenario::load(&name).unwrap();
        let world = s.world();
        let mut p = s.initial_state();
        for (i, row) in s.rows.iter().enumerate() {
            if i > 0 {
                apply_state(&mut p, &row.pre);
            }
            let start = p.clone();
            let mut end = start.clone();
            apply_state(&mut end, &subset(&row.post, &["x", "y", "z"]));
            let moved = Vec3::new(
                end.pos.x - start.pos.x,
                end.pos.y - start.pos.y,
                end.pos.z - start.pos.z,
            );
            let long_fall = start.fall_distance != 0.0
                && !start.in_water
                && !start.in_lava
                && start.effects.is_empty()
                && moved.x * moved.x + moved.y * moved.y + moved.z * moved.z >= 1.0;
            if long_fall {
                let hit = fluids::fall_damage_resetting_clip_hits(&world, start.pos, {
                    let len = (moved.x * moved.x + moved.y * moved.y + moved.z * moved.z).sqrt();
                    let e = len.min(8.0);
                    Vec3::new(
                        start.pos.x + moved.x / len * e,
                        start.pos.y + moved.y / len * e,
                        start.pos.z + moved.z / len * e,
                    )
                });
                let post = |k: &str| row.post[k].as_i64() == Some(1);
                if post("water") {
                    into_water += 1;
                    if !hit {
                        bad.push(format!("{name} t={i}: ended in water but the clip missed"));
                    }
                } else if !post("ground") && !post("lava") {
                    misses += 1;
                    let mut q = start.clone();
                    fluids::reset_fall_distance_on_crossing(&mut q, &world, moved);
                    // Entity.checkFallDamage adds this move's drop afterwards.
                    let fall = q.fall_distance - f64::from(moved.y as f32);
                    let want = f64::from_bits(row.post["fall"].as_i64().unwrap() as u64);
                    if hit || fall != want {
                        bad.push(format!(
                            "{name} t={i}: clip hit={hit}, predicted fall {fall}, recorded {want}"
                        ));
                    }
                }
            }
            apply_state(&mut p, &row.post);
        }
    }
    eprintln!("{misses} long airborne falls (clip missed, fall added up), {into_water} long falls into water (clip hit)");
    assert!(
        bad.is_empty(),
        "{} mismatches:\n{}",
        bad.len(),
        bad.join("\n")
    );
    assert!(misses >= 3 && into_water >= 1);
}

#[test]
fn whole_ticks_in_fluid_match_the_corpus() {
    const FIELDS: &[&str] = &[
        "x", "y", "z", "dx", "dy", "dz", "hc", "vc", "vcb", "ground", "fall", "njd", "water",
        "waterH", "lava", "lavaH", "swimming", "eyeWater",
    ];
    let mut total_checked = 0;
    let mut total_exact = 0;
    let mut report = Vec::new();
    for name in FLUID_SCENARIOS {
        let s = Scenario::load(name).unwrap();
        let world = s.world();
        let mut p = s.initial_state();
        p.water_on_eyes = initial_water_on_eyes(&p, &world);
        let (mut checked, mut exact) = (0, 0);
        let mut first_bad: Vec<String> = Vec::new();
        for (i, row) in s.rows.iter().enumerate() {
            if i > 0 {
                apply_state(&mut p, &row.pre);
            }
            let before = p.clone();
            let ran = fluid_tick(&mut p, &world, &row.input, &row.post);
            if ran {
                checked += 1;
                let diffs = diff_fields(&p, &row.post, FIELDS);
                if diffs.is_empty() {
                    exact += 1;
                } else if first_bad.len() < 6 {
                    first_bad.push(format!(
                        "t={i}: {}",
                        diffs
                            .iter()
                            .map(|d| format!("{} want {} got {}", d.field, d.expected, d.actual))
                            .collect::<Vec<_>>()
                            .join("; ")
                    ));
                }
            } else {
                // Not an in-fluid tick: carry on from the recording; only the hidden
                // fluid-on-eyes state advances by the game's own rule.
                p = before;
                let mut q = p.clone();
                fluids::update_fluid_on_eyes(&mut q, &world);
                p.water_on_eyes = q.water_on_eyes;
            }
            let keep = p.water_on_eyes;
            apply_state(&mut p, &row.post);
            p.water_on_eyes = keep;
        }
        report.push(format!(
            "{name}: {exact}/{checked} in-fluid ticks bit-exact (of {} ticks)",
            s.rows.len()
        ));
        for b in first_bad {
            report.push(format!("    {b}"));
        }
        total_checked += checked;
        total_exact += exact;
    }
    eprintln!("{}", report.join("\n"));
    eprintln!("total: {total_exact}/{total_checked}");
    assert!(
        total_checked > 400,
        "too few in-fluid ticks ({total_checked})"
    );
    assert_eq!(
        total_exact,
        total_checked,
        "in-fluid ticks that are not bit-exact:\n{}",
        report.join("\n")
    );
}
