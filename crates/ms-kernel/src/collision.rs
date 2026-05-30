//! Per-axis swept collision of a moving bounding box against block collision boxes.
//!
//! Ported from `Entity.collideWithShapes` / `Shapes.collide` / `VoxelShape.collide`. The axes
//! are resolved one at a time in the order the game uses (`Direction.axisStepOrder`): Y first,
//! then whichever horizontal axis has the larger motion, then the other. Each axis is clamped
//! against every collider with the same `1e-7` tolerances the game applies, and the box is
//! advanced by the clamped amount before the next axis is tested.

use ms_numerics::Vec3;
use ms_world::aabb::Aabb;

const EPSILON: f64 = 1.0E-7;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    X,
    Y,
    Z,
}

fn component(v: Vec3, axis: Axis) -> f64 {
    match axis {
        Axis::X => v.x,
        Axis::Y => v.y,
        Axis::Z => v.z,
    }
}

fn with_component(v: Vec3, axis: Axis, value: f64) -> Vec3 {
    match axis {
        Axis::X => Vec3::new(value, v.y, v.z),
        Axis::Y => Vec3::new(v.x, value, v.z),
        Axis::Z => Vec3::new(v.x, v.y, value),
    }
}

fn min_on(b: Aabb, axis: Axis) -> f64 {
    component(b.min, axis)
}

fn max_on(b: Aabb, axis: Axis) -> f64 {
    component(b.max, axis)
}

/// `Direction.axisStepOrder`: Y first, then the larger horizontal axis, then the smaller.
fn axis_step_order(motion: Vec3) -> [Axis; 3] {
    if motion.x.abs() < motion.z.abs() {
        [Axis::Y, Axis::Z, Axis::X]
    } else {
        [Axis::Y, Axis::X, Axis::Z]
    }
}

/// One collider clamped along one axis (`VoxelShape.collide` reduced to a single box).
fn collide_one(axis: Axis, collider: Aabb, box_: Aabb, d: f64) -> f64 {
    let (perp_a, perp_b) = match axis {
        Axis::X => (Axis::Y, Axis::Z),
        Axis::Y => (Axis::X, Axis::Z),
        Axis::Z => (Axis::X, Axis::Y),
    };
    // The box must overlap the collider in both perpendicular axes, with the box bounds inset
    // by EPSILON on each side (mirroring the findIndex insets in VoxelShape.collideX).
    let overlaps = max_on(box_, perp_a) - EPSILON > min_on(collider, perp_a)
        && min_on(box_, perp_a) + EPSILON < max_on(collider, perp_a)
        && max_on(box_, perp_b) - EPSILON > min_on(collider, perp_b)
        && min_on(box_, perp_b) + EPSILON < max_on(collider, perp_b);
    if !overlaps {
        return d;
    }
    if d > 0.0 {
        let gap = min_on(collider, axis) - max_on(box_, axis);
        if gap >= -EPSILON {
            return d.min(gap);
        }
    } else if d < 0.0 {
        let gap = max_on(collider, axis) - min_on(box_, axis);
        if gap <= EPSILON {
            return d.max(gap);
        }
    }
    d
}

/// `Shapes.collide`: clamp the motion along one axis against every collider in turn.
fn collide_axis(axis: Axis, box_: Aabb, colliders: &[Aabb], mut d: f64) -> f64 {
    for &collider in colliders {
        if d.abs() < EPSILON {
            return 0.0;
        }
        d = collide_one(axis, collider, box_, d);
    }
    d
}

/// `Entity.collide`: the per-axis sweep, plus auto step-up onto ledges up to `max_up_step` high
/// (so walking into a slab/stair/single-block edge climbs it instead of stopping).
pub fn collide(
    motion: Vec3,
    box_: Aabb,
    on_ground: bool,
    max_up_step: f32,
    colliders: &[Aabb],
) -> Vec3 {
    let collided = collide_with_shapes(motion, box_, colliders);
    let hit_x = motion.x != collided.x;
    let hit_z = motion.z != collided.z;
    let hit_down = motion.y != collided.y && motion.y < 0.0;
    if max_up_step > 0.0 && (hit_down || on_ground) && (hit_x || hit_z) {
        let base = if hit_down {
            box_.move_by(Vec3::new(0.0, collided.y, 0.0))
        } else {
            box_
        };
        let current = collided.y as f32;
        let target = collided.x * collided.x + collided.z * collided.z;
        for height in candidate_step_heights(base, colliders, max_up_step, current) {
            let stepped = collide_with_shapes(
                Vec3::new(motion.x, f64::from(height), motion.z),
                base,
                colliders,
            );
            if stepped.x * stepped.x + stepped.z * stepped.z > target {
                let drop = box_.min.y - base.min.y;
                return Vec3::new(stepped.x, stepped.y - drop, stepped.z);
            }
        }
    }
    collided
}

/// `Entity.collectCandidateStepUpHeights`: the distinct collider top/bottom planes (relative to
/// the box bottom) that are within reach of a step up, sorted ascending.
fn candidate_step_heights(box_: Aabb, colliders: &[Aabb], max_up: f32, current: f32) -> Vec<f32> {
    let mut heights: Vec<f32> = Vec::new();
    for c in colliders {
        for coord in [c.min.y, c.max.y] {
            let h = (coord - box_.min.y) as f32;
            if h >= 0.0 && h != current {
                if h > max_up {
                    break;
                }
                if !heights.contains(&h) {
                    heights.push(h);
                }
            }
        }
    }
    heights.sort_unstable_by(f32::total_cmp);
    heights
}

/// Clamp `motion` so `box_` does not pass through any collider, resolving axes in game order.
pub fn collide_with_shapes(motion: Vec3, box_: Aabb, colliders: &[Aabb]) -> Vec3 {
    if colliders.is_empty() {
        return motion;
    }
    let mut result = Vec3::ZERO;
    for axis in axis_step_order(motion) {
        let d = component(motion, axis);
        if d != 0.0 {
            let clamped = collide_axis(axis, box_.move_by(result), colliders, d);
            result = with_component(result, axis, clamped);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(feet_y: f64) -> Aabb {
        Aabb::from_corners(0.0, feet_y, 0.0, 0.6, feet_y + 1.8, 0.6)
    }

    const BLOCK: Aabb = Aabb {
        min: Vec3::new(0.0, 0.0, 0.0),
        max: Vec3::new(1.0, 1.0, 1.0),
    };

    #[test]
    fn empty_world_does_not_clamp() {
        let m = collide_with_shapes(Vec3::new(0.3, -0.2, 0.1), player(1.0), &[]);
        assert_eq!(m, Vec3::new(0.3, -0.2, 0.1));
    }

    #[test]
    fn falls_to_block_surface() {
        let m = collide_with_shapes(Vec3::new(0.0, -0.9, 0.0), player(1.5), &[BLOCK]);
        assert_eq!(m.y, -0.5);
        assert_eq!(m.x, 0.0);
        assert_eq!(m.z, 0.0);
    }

    #[test]
    fn resting_stays_put() {
        let m = collide_with_shapes(Vec3::new(0.0, -0.0784, 0.0), player(1.0), &[BLOCK]);
        assert_eq!(m.y, 0.0);
    }

    #[test]
    fn stops_at_wall() {
        let wall = Aabb::from_corners(1.0, 0.0, 0.0, 2.0, 3.0, 1.0);
        let m = collide_with_shapes(Vec3::new(0.5, 0.0, 0.0), player(1.0), &[wall]);
        assert_eq!(m.x, 1.0 - 0.6);
    }

    #[test]
    fn unobstructed_horizontal_passes() {
        // A block to the side the player does not overlap vertically: no clamp.
        let m = collide_with_shapes(Vec3::new(0.5, 0.0, 0.0), player(5.0), &[BLOCK]);
        assert_eq!(m.x, 0.5);
    }
}
