//! Player input -> motion for grounded, non-fluid movement.
//!
//! Ports the client input chain (`KeyboardInput.tick` -> `LocalPlayer.modifyInput`) and the
//! flat-ground case of `LivingEntity.aiStep`/`travelInAir`: velocity rounding, friction-scaled
//! input rotated by yaw, then the per-axis drag and gravity. Full collision lives in
//! [`crate::collision`]; here the terrain is assumed flat and unobstructed (the entity rests on
//! the ground, so the vertical movement collapses to the fall-on Y reset).

use ms_numerics::{mth, Vec3};
use ms_world::aabb::Aabb;
use ms_world::World;

const DEG_TO_RAD: f32 = (std::f64::consts::PI / 180.0) as f32;
const HALF_WIDTH: f64 = (0.6_f32 / 2.0_f32) as f64;
const HEIGHT: f64 = 1.8_f32 as f64;
const JUMP_POWER: f64 = 0.42_f32 as f64;
const STEP_HEIGHT: f32 = 0.6;

#[derive(Clone, Copy, Debug)]
pub struct Keys {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
}

fn impulse(a: bool, b: bool) -> f32 {
    if a == b {
        0.0
    } else if a {
        1.0
    } else {
        -1.0
    }
}

fn mth_sqrt(x: f32) -> f32 {
    f64::from(x).sqrt() as f32
}

/// `KeyboardInput.tick` + `LocalPlayer.modifyInput`: keys -> `(xxa, zza)` movement input.
fn movement_input(keys: Keys, sneaking: bool) -> (f32, f32) {
    let forward = impulse(keys.forward, keys.back);
    let strafe = impulse(keys.left, keys.right);
    // Vec2(strafe, forward).normalized()
    let len = mth_sqrt(strafe * strafe + forward * forward);
    if len < 1.0e-4 {
        return (0.0, 0.0);
    }
    let mut x = strafe / len;
    let mut y = forward / len;
    // modifyInput: scale 0.98, sneaking factor, then the square-movement correction.
    x *= 0.98;
    y *= 0.98;
    if sneaking {
        x *= 0.3;
        y *= 0.3;
    }
    square_movement(x, y)
}

/// `LocalPlayer.modifyInputSpeedForSquareMovement`: lets diagonal input reach the same speed as
/// the edge of the unit square.
fn square_movement(x: f32, y: f32) -> (f32, f32) {
    let len = mth_sqrt(x * x + y * y);
    if len <= 0.0 {
        return (x, y);
    }
    // Vec2.scale(1.0F / f): reciprocal first, then multiply — not a direct divide (they differ
    // by a ULP in float, which compounds through the rotation).
    let inv = 1.0 / len;
    let ux = x * inv;
    let uy = y * inv;
    let fa = ux.abs();
    let ga = uy.abs();
    let ratio = if ga > fa { fa / ga } else { ga / fa };
    let dist = mth_sqrt(1.0 + ratio * ratio);
    let h = (len * dist).min(1.0);
    (ux * h, uy * h)
}

/// `Entity.getInputVector`: scale the input by `speed` and rotate it around Y by `yaw`.
fn input_vector(xxa: f64, zza: f64, speed: f32, yaw: f32) -> Vec3 {
    let lensq = xxa * xxa + zza * zza;
    if lensq < 1.0e-7 {
        return Vec3::ZERO;
    }
    let scale = f64::from(speed);
    let (sx, sz) = if lensq > 1.0 {
        let l = lensq.sqrt();
        (xxa / l * scale, zza / l * scale)
    } else {
        (xxa * scale, zza * scale)
    };
    let sin = f64::from(mth::sin(yaw * DEG_TO_RAD));
    let cos = f64::from(mth::cos(yaw * DEG_TO_RAD));
    Vec3::new(sx * cos - sz * sin, 0.0, sz * cos + sx * sin)
}

/// `getSpeed()` = the MOVEMENT_SPEED attribute: base `0.1f` (stored as a float, so
/// `(double)0.1f`), +30% (multiplied total) while sprinting.
fn movement_speed(sprinting: bool) -> f32 {
    let base = f64::from(0.1_f32);
    let factor = if sprinting {
        1.0 + f64::from(0.3_f32)
    } else {
        1.0
    };
    (base * factor) as f32
}

/// One tick of grounded, unobstructed movement: predicts the next `deltaMovement`. Mirrors the
/// velocity rounding in `LivingEntity.aiStep` followed by `travelInAir`.
pub fn next_velocity(
    vel: Vec3,
    yaw: f32,
    on_ground: bool,
    sprinting: bool,
    sneaking: bool,
    keys: Keys,
) -> Vec3 {
    let mut vx = vel.x;
    let mut vy = vel.y;
    let mut vz = vel.z;
    if vx * vx + vz * vz < 9.0e-6 {
        vx = 0.0;
        vz = 0.0;
    }
    if vy.abs() < 0.003 {
        vy = 0.0;
    }

    let (xxa, zza) = movement_input(keys, sneaking);
    let friction: f32 = if on_ground { 0.6 } else { 1.0 };
    let drag = f64::from(friction * 0.91);
    let speed = if on_ground {
        movement_speed(sprinting) * (0.216_000_02 / (friction * friction * friction))
    } else {
        0.02
    };
    let input = input_vector(f64::from(xxa), f64::from(zza), speed, yaw);

    let mx = vx + input.x;
    let mz = vz + input.z;
    let post_y = if on_ground { 0.0 } else { vy };
    let d = post_y - 0.08;
    Vec3::new(mx * drag, d * f64::from(0.98_f32), mz * drag)
}

/// The player's standing bounding box at a position (`EntityDimensions.makeBoundingBox`).
pub fn player_bb(pos: Vec3) -> Aabb {
    Aabb::new(
        Vec3::new(pos.x - HALF_WIDTH, pos.y, pos.z - HALF_WIDTH),
        Vec3::new(pos.x + HALF_WIDTH, pos.y + HEIGHT, pos.z + HALF_WIDTH),
    )
}

fn mth_equal(a: f64, b: f64) -> bool {
    (b - a).abs() < f64::from(1.0e-5_f32)
}

fn gather_in(world: &World, region: Aabb) -> Vec<Aabb> {
    let x0 = region.min.x.floor() as i32;
    let x1 = region.max.x.floor() as i32;
    let y0 = region.min.y.floor() as i32;
    let y1 = region.max.y.floor() as i32;
    let z0 = region.min.z.floor() as i32;
    let z1 = region.max.z.floor() as i32;
    let mut out = Vec::new();
    for bx in x0..=x1 {
        for by in y0..=y1 {
            for bz in z0..=z1 {
                if let Some(enc) = world.block_encoded(bx, by, bz) {
                    for s in ms_data::collision_boxes_for_encoded(&enc) {
                        out.push(Aabb::new(
                            Vec3::new(
                                f64::from(bx) + s[0],
                                f64::from(by) + s[1],
                                f64::from(bz) + s[2],
                            ),
                            Vec3::new(
                                f64::from(bx) + s[3],
                                f64::from(by) + s[4],
                                f64::from(bz) + s[5],
                            ),
                        ));
                    }
                }
            }
        }
    }
    out
}

/// `Entity.move` for the player: collide the motion (with step-up) against the world, advance the
/// position, and apply the collision rules to the velocity — zero a horizontal component on
/// contact (`Mth.equal` test), zero Y on a vertical hit (default `updateEntityMovementAfterFallOn`).
fn move_entity(pos: Vec3, mut vel: Vec3, on_ground: bool, world: &World) -> (Vec3, Vec3, bool) {
    let bb = player_bb(pos);
    let motion = vel;
    let region =
        bb.expand_towards(motion)
            .expand_towards(Vec3::new(0.0, f64::from(STEP_HEIGHT), 0.0));
    let colliders = gather_in(world, region);
    let moved = crate::collision::collide(motion, bb, on_ground, STEP_HEIGHT, &colliders);
    let new_pos = Vec3::new(pos.x + moved.x, pos.y + moved.y, pos.z + moved.z);
    if !mth_equal(motion.x, moved.x) {
        vel.x = 0.0;
    }
    if !mth_equal(motion.z, moved.z) {
        vel.z = 0.0;
    }
    let vertical_collision = motion.y != moved.y;
    if vertical_collision {
        vel.y = 0.0;
    }
    (new_pos, vel, vertical_collision && motion.y < 0.0)
}

fn block_below_friction(world: &World, pos: Vec3, on_ground: bool) -> f32 {
    if !on_ground {
        return 1.0;
    }
    let bx = pos.x.floor() as i32;
    let by = (pos.y - 0.5).floor() as i32;
    let bz = pos.z.floor() as i32;
    match world.block_name(bx, by, bz) {
        Some(name) => ms_data::friction_for_name(&name),
        None => 0.6,
    }
}

/// One full grounded tick over a real world: velocity rounding, input, jump, then `travelInAir`
/// with real block collision. `no_jump_delay` carries the 10-tick jump cooldown across ticks.
#[allow(clippy::too_many_arguments)]
pub fn step(
    pos: Vec3,
    vel: Vec3,
    yaw: f32,
    on_ground: bool,
    sprinting: bool,
    sneaking: bool,
    keys: Keys,
    jump: bool,
    no_jump_delay: &mut i32,
    world: &World,
) -> (Vec3, Vec3, bool) {
    if *no_jump_delay > 0 {
        *no_jump_delay -= 1;
    }

    let mut v = vel;
    if v.x * v.x + v.z * v.z < 9.0e-6 {
        v.x = 0.0;
        v.z = 0.0;
    }
    if v.y.abs() < 0.003 {
        v.y = 0.0;
    }

    let (xxa, zza) = movement_input(keys, sneaking);

    if jump {
        if on_ground && *no_jump_delay == 0 {
            if v.y < JUMP_POWER {
                v.y = JUMP_POWER;
            }
            if sprinting {
                let g = yaw * DEG_TO_RAD;
                v.x += -f64::from(mth::sin(g)) * 0.2;
                v.z += f64::from(mth::cos(g)) * 0.2;
            }
            *no_jump_delay = 10;
        }
    } else {
        *no_jump_delay = 0;
    }

    let friction = block_below_friction(world, pos, on_ground);
    let drag = f64::from(friction * 0.91);
    let speed = if on_ground {
        movement_speed(sprinting) * (0.216_000_02 / (friction * friction * friction))
    } else if sprinting {
        0.025_999_999
    } else {
        0.02
    };
    let input = input_vector(f64::from(xxa), f64::from(zza), speed, yaw);
    v.x += input.x;
    v.z += input.z;

    let (new_pos, post, new_on_ground) = move_entity(pos, v, on_ground, world);
    let d = post.y - 0.08;
    let new_vel = Vec3::new(post.x * drag, d * f64::from(0.98_f32), post.z * drag);
    (new_pos, new_vel, new_on_ground)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Row {
        y: u64,
        dx: f64,
        dy: f64,
        dz: f64,
        yaw: f32,
        on_ground: bool,
        sprinting: bool,
        sneaking: bool,
        keys: Keys,
        jump: bool,
    }

    fn parse() -> Vec<Row> {
        let csv = include_str!("../testdata/walk.csv");
        let mut rows = Vec::new();
        for line in csv.lines().skip(1) {
            let c: Vec<&str> = line.split(',').collect();
            if c.len() < 19 {
                continue;
            }
            let d = |i: usize| f64::from_bits(c[i].parse::<i64>().unwrap() as u64);
            let fl = |i: usize| f32::from_bits(c[i].parse::<i32>().unwrap() as u32);
            let b = |i: usize| c[i] == "1";
            rows.push(Row {
                y: c[2].parse::<i64>().unwrap() as u64,
                dx: d(4),
                dy: d(5),
                dz: d(6),
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
        rows
    }

    #[test]
    fn reproduces_flat_ground_walk() {
        let rows = parse();
        let mut qualifying = 0usize;
        let mut exact = 0usize;
        let mut close = 0usize;
        let mut obstructed = 0usize;
        let mut worst_close = 0.0_f64;
        let mut worst_info = String::new();
        for t in 0..rows.len() - 1 {
            let a = &rows[t];
            let b = &rows[t + 1];
            // Only the clean case: resting on flat ground (no y change), no jump this tick.
            if !a.on_ground || !b.on_ground || a.y != b.y || b.jump {
                continue;
            }
            qualifying += 1;
            let vel = Vec3::new(a.dx, a.dy, a.dz);
            let pred = next_velocity(vel, b.yaw, a.on_ground, b.sprinting, b.sneaking, b.keys);
            let err = (pred.x - b.dx)
                .abs()
                .max((pred.y - b.dy).abs())
                .max((pred.z - b.dz).abs());
            // Errors >= 1e-6 are horizontal collisions (the game zeroed a component): not
            // modelled by the flat-ground path here. Everything else is unobstructed movement.
            if err >= 1.0e-6 {
                obstructed += 1;
            } else {
                close += 1;
                if err == 0.0 {
                    exact += 1;
                }
                if err > worst_close {
                    worst_close = err;
                    worst_info = format!(
                        "tick {} fwd={} l={} r={} sprint={} yaw={} vin=({},{}) dx:{}/{} dz:{}/{}",
                        t + 1,
                        b.keys.forward,
                        b.keys.left,
                        b.keys.right,
                        b.sprinting,
                        b.yaw,
                        a.dx,
                        a.dz,
                        pred.x,
                        b.dx,
                        pred.z,
                        b.dz
                    );
                }
            }
        }
        eprintln!(
            "qualifying={qualifying} unobstructed={close} (exact={exact}, worst={worst_close:e}) obstructed={obstructed}\nworst: {worst_info}"
        );
        assert!(qualifying > 1000, "only {qualifying} qualifying ticks");
        assert!(
            close * 2 > qualifying,
            "too few unobstructed ticks: {close}/{qualifying}"
        );
        // Every unobstructed grounded tick must reproduce vanilla bit-for-bit.
        assert!(
            exact == close,
            "{} of {close} unobstructed ticks not bit-exact; worst {worst_close:e}\n{worst_info}",
            close - exact
        );
    }
}
