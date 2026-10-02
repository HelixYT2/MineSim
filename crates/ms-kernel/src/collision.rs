//! Block collision queries and the entity-vs-world collision resolution.
//!
//! This follows the game's structure rather than a generic AABB sweep, because the exact results
//! (down to the last bit of a position) depend on its details:
//!
//! * `BlockCollisions`: the cells around a query box are visited in a fixed order (x fastest, then
//!   y, then z); the outer ring only contributes blocks whose collision shape is larger than a
//!   cube; a full-cube shape is tested with plain strict box intersection, every other shape with
//!   `Shapes.joinIsNotEmpty`, whose coordinate merging snaps coordinates closer than `1e-7`
//!   together (so a box that overlaps a slab by 1e-14 does not "collide" with it, but would with a
//!   full block).
//! * `VoxelShape.collide`: a shape is a grid of cells (the union of its boxes' coordinates); the
//!   motion is clamped against the first occupied layer found in the direction of travel, looked up
//!   with `findIndex` and the `1e-7` insets, not box by box.
//! * `Entity.collide`: axis order (Y, then the larger horizontal axis), the step-up search with the
//!   candidate heights taken from the shapes' Y coordinates, and the extra `-1.0E-5F` region.
//!
//! Collision boxes come from [`crate::blocks::collision_boxes`] (entity-dependent shapes) in
//! block-local coordinates; this module places them in the world and does the rest.

// The comparisons mirror the reference's `!(a <= b)` forms, which differ from `a > b` for NaN.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::state::PlayerState;
use ms_numerics::Vec3;
use ms_world::aabb::Aabb;
use ms_world::World;

/// `Shapes.EPSILON`.
pub const EPSILON: f64 = 1.0E-7;

// ---------------------------------------------------------------------------------------------
// Java arithmetic helpers
// ---------------------------------------------------------------------------------------------

/// `Math.min(double, double)` (NaN-propagating, `-0.0 < 0.0`).
#[inline]
pub fn jmin(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 && b.to_bits() == (-0.0_f64).to_bits() {
        return b;
    }
    if a <= b {
        a
    } else {
        b
    }
}

/// `Math.max(double, double)` (NaN-propagating, `-0.0 < 0.0`).
#[inline]
pub fn jmax(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 && a.to_bits() == (-0.0_f64).to_bits() {
        return b;
    }
    if a >= b {
        a
    } else {
        b
    }
}

/// `Mth.floor(double)`.
#[inline]
pub fn floor(d: f64) -> i32 {
    let i = d as i32;
    if d < f64::from(i) {
        i.wrapping_sub(1)
    } else {
        i
    }
}

#[inline]
fn get(v: Vec3, axis: usize) -> f64 {
    match axis {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

#[inline]
fn with(v: Vec3, axis: usize, value: f64) -> Vec3 {
    match axis {
        0 => Vec3::new(value, v.y, v.z),
        1 => Vec3::new(v.x, value, v.z),
        _ => Vec3::new(v.x, v.y, value),
    }
}

// ---------------------------------------------------------------------------------------------
// Bounding boxes
// ---------------------------------------------------------------------------------------------

/// `new AABB(x0, y0, z0, x1, y1, z1)`: the corners are sorted per axis.
#[inline]
pub fn aabb(x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64) -> Aabb {
    Aabb {
        min: Vec3::new(jmin(x0, x1), jmin(y0, y1), jmin(z0, z1)),
        max: Vec3::new(jmax(x0, x1), jmax(y0, y1), jmax(z0, z1)),
    }
}

/// `AABB.move(double, double, double)`.
#[inline]
pub fn aabb_move(b: Aabb, x: f64, y: f64, z: f64) -> Aabb {
    aabb(
        b.min.x + x,
        b.min.y + y,
        b.min.z + z,
        b.max.x + x,
        b.max.y + y,
        b.max.z + z,
    )
}

/// `AABB.inflate(double, double, double)`.
#[inline]
pub fn aabb_inflate(b: Aabb, x: f64, y: f64, z: f64) -> Aabb {
    aabb(
        b.min.x - x,
        b.min.y - y,
        b.min.z - z,
        b.max.x + x,
        b.max.y + y,
        b.max.z + z,
    )
}

/// `AABB.deflate(double)`.
#[inline]
pub fn aabb_deflate(b: Aabb, d: f64) -> Aabb {
    aabb_inflate(b, -d, -d, -d)
}

/// `AABB.expandTowards(double, double, double)`.
#[inline]
pub fn aabb_expand_towards(b: Aabb, x: f64, y: f64, z: f64) -> Aabb {
    let (mut g, mut h, mut i) = (b.min.x, b.min.y, b.min.z);
    let (mut j, mut k, mut l) = (b.max.x, b.max.y, b.max.z);
    if x < 0.0 {
        g += x;
    } else if x > 0.0 {
        j += x;
    }
    if y < 0.0 {
        h += y;
    } else if y > 0.0 {
        k += y;
    }
    if z < 0.0 {
        i += z;
    } else if z > 0.0 {
        l += z;
    }
    aabb(g, h, i, j, k, l)
}

/// `AABB.intersects(double...)`: strict overlap on every axis.
#[inline]
pub fn aabb_intersects(b: Aabb, x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64) -> bool {
    b.min.x < x1 && b.max.x > x0 && b.min.y < y1 && b.max.y > y0 && b.min.z < z1 && b.max.z > z0
}

/// The player's bounding box at `pos` for pose dimensions `(width, height)`
/// (`EntityDimensions.makeBoundingBox`: the half-width is a float, promoted when added).
#[inline]
pub fn bounding_box_at(pos: Vec3, dims: (f32, f32)) -> Aabb {
    let g = f64::from(dims.0 / 2.0_f32);
    let h = f64::from(dims.1);
    aabb(pos.x - g, pos.y, pos.z - g, pos.x + g, pos.y + h, pos.z + g)
}

/// The player's current bounding box.
#[inline]
pub fn bounding_box(p: &PlayerState) -> Aabb {
    bounding_box_at(p.pos, p.dimensions())
}

// ---------------------------------------------------------------------------------------------
// Voxel shapes
// ---------------------------------------------------------------------------------------------

/// What `VoxelShape` operations need from a shape: its coordinate lists and which cells of the
/// grid are filled.
trait Vox {
    /// Number of coordinates on `axis` (cells + 1).
    fn count(&self, axis: usize) -> usize;
    fn coord(&self, axis: usize, i: usize) -> f64;
    /// Whether the cell is filled; indices are in range.
    fn full(&self, x: usize, y: usize, z: usize) -> bool;
}

#[inline]
fn size_of(v: &impl Vox, axis: usize) -> i64 {
    v.count(axis) as i64 - 1
}

/// `DiscreteVoxelShape.isFullWide`: out-of-range cells are empty.
#[inline]
fn is_full_wide(v: &impl Vox, x: i64, y: i64, z: i64) -> bool {
    x >= 0
        && y >= 0
        && z >= 0
        && x < size_of(v, 0)
        && y < size_of(v, 1)
        && z < size_of(v, 2)
        && v.full(x as usize, y as usize, z as usize)
}

/// `VoxelShape.findIndex`: the index of the last coordinate `<= d` (-1 if none).
#[inline]
fn find_index(v: &impl Vox, axis: usize, d: f64) -> i64 {
    // Mth.binarySearch(0, size + 1, i -> d < coord(i)) - 1
    let mut lo = 0_i64;
    let mut k = v.count(axis) as i64;
    while k > 0 {
        let l = k / 2;
        let m = lo + l;
        if d < v.coord(axis, m as usize) {
            k = l;
        } else {
            lo = m + 1;
            k -= l + 1;
        }
    }
    lo - 1
}

/// `VoxelShape.collide(axis, aabb, d)` (`collideX` with the axis cycle resolved).
fn voxel_collide(v: &impl Vox, axis: usize, bb: Aabb, mut d: f64) -> f64 {
    if v.count(0) < 2 {
        return d; // empty
    }
    if d.abs() < EPSILON {
        return 0.0;
    }
    // The two perpendicular axes; their order does not affect the result (the search only asks
    // whether any cell in the window is filled).
    let (b, c) = match axis {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    };
    let e = get(bb.max, axis);
    let f = get(bb.min, axis);
    let i = find_index(v, axis, f + EPSILON);
    let j = find_index(v, axis, e - EPSILON);
    let k = find_index(v, b, get(bb.min, b) + EPSILON).max(0);
    let l = size_of(v, b).min(find_index(v, b, get(bb.max, b) - EPSILON) + 1);
    let m = find_index(v, c, get(bb.min, c) + EPSILON).max(0);
    let n = size_of(v, c).min(find_index(v, c, get(bb.max, c) - EPSILON) + 1);
    let o = size_of(v, axis);
    let full_at = |p: i64, q: i64, r: i64| -> bool {
        let mut idx = [0_i64; 3];
        idx[axis] = p;
        idx[b] = q;
        idx[c] = r;
        is_full_wide(v, idx[0], idx[1], idx[2])
    };
    if d > 0.0 {
        let mut p = j + 1;
        while p < o {
            for q in k..l {
                for r in m..n {
                    if full_at(p, q, r) {
                        let g = v.coord(axis, p as usize) - e;
                        if g >= -EPSILON {
                            d = jmin(d, g);
                        }
                        return d;
                    }
                }
            }
            p += 1;
        }
    } else if d < 0.0 {
        let mut p = i - 1;
        while p >= 0 {
            for q in k..l {
                for r in m..n {
                    if full_at(p, q, r) {
                        let g = v.coord(axis, (p + 1) as usize) - f;
                        if g <= EPSILON {
                            d = jmax(d, g);
                        }
                        return d;
                    }
                }
            }
            p -= 1;
        }
    }
    d
}

/// A collision shape placed in the world: one filled box, or a grid built from several.
#[derive(Clone, Debug)]
pub struct Shape {
    repr: Repr,
}

#[derive(Clone, Debug)]
enum Repr {
    /// A single filled cell with the given world bounds.
    Single { min: [f64; 3], max: [f64; 3] },
    /// A grid: per-axis world coordinates and the filled cells (`x * ny * nz + y * nz + z`).
    Grid {
        coords: [Vec<f64>; 3],
        cells: Vec<bool>,
    },
}

impl Vox for Shape {
    #[inline]
    fn count(&self, axis: usize) -> usize {
        match &self.repr {
            Repr::Single { .. } => 2,
            Repr::Grid { coords, .. } => coords[axis].len(),
        }
    }

    #[inline]
    fn coord(&self, axis: usize, i: usize) -> f64 {
        match &self.repr {
            Repr::Single { min, max } => {
                if i == 0 {
                    min[axis]
                } else {
                    max[axis]
                }
            }
            Repr::Grid { coords, .. } => coords[axis][i],
        }
    }

    #[inline]
    fn full(&self, x: usize, y: usize, z: usize) -> bool {
        match &self.repr {
            Repr::Single { .. } => true,
            Repr::Grid { coords, cells } => {
                let ny = coords[1].len() - 1;
                let nz = coords[2].len() - 1;
                cells[(x * ny + y) * nz + z]
            }
        }
    }
}

impl Shape {
    /// Place block-local `boxes` at block `(x, y, z)`. The shape is the union of the boxes on the
    /// grid of all their coordinates (what the game's `optimize()`d shapes are). Returns `None` for
    /// an empty list.
    pub fn from_boxes(boxes: &[[f64; 6]], x: i32, y: i32, z: i32) -> Option<Shape> {
        let off = [f64::from(x), f64::from(y), f64::from(z)];
        match boxes {
            [] => None,
            [b] => Some(Shape {
                repr: Repr::Single {
                    min: [b[0] + off[0], b[1] + off[1], b[2] + off[2]],
                    max: [b[3] + off[0], b[4] + off[1], b[5] + off[2]],
                },
            }),
            _ => {
                let mut coords: [Vec<f64>; 3] = [Vec::new(), Vec::new(), Vec::new()];
                for axis in 0..3 {
                    let mut c: Vec<f64> = Vec::with_capacity(boxes.len() * 2);
                    for b in boxes {
                        c.push(b[axis]);
                        c.push(b[axis + 3]);
                    }
                    c.sort_by(f64::total_cmp);
                    c.dedup();
                    coords[axis] = c;
                }
                let (nx, ny, nz) = (
                    coords[0].len() - 1,
                    coords[1].len() - 1,
                    coords[2].len() - 1,
                );
                let mut cells = vec![false; nx * ny * nz];
                for b in boxes {
                    for ix in 0..nx {
                        if !(b[0] <= coords[0][ix] && coords[0][ix + 1] <= b[3]) {
                            continue;
                        }
                        for iy in 0..ny {
                            if !(b[1] <= coords[1][iy] && coords[1][iy + 1] <= b[4]) {
                                continue;
                            }
                            for iz in 0..nz {
                                if b[2] <= coords[2][iz] && coords[2][iz + 1] <= b[5] {
                                    cells[(ix * ny + iy) * nz + iz] = true;
                                }
                            }
                        }
                    }
                }
                for (axis, list) in coords.iter_mut().enumerate() {
                    for c in list.iter_mut() {
                        *c += off[axis];
                    }
                }
                Some(Shape {
                    repr: Repr::Grid { coords, cells },
                })
            }
        }
    }

    /// A shape consisting of one box given in world coordinates.
    pub fn from_world_box(b: Aabb) -> Shape {
        Shape {
            repr: Repr::Single {
                min: [b.min.x, b.min.y, b.min.z],
                max: [b.max.x, b.max.y, b.max.z],
            },
        }
    }

    /// `VoxelShape.collide(axis, box, d)` for axis `0..3`.
    pub fn collide(&self, axis: usize, bb: Aabb, d: f64) -> f64 {
        voxel_collide(self, axis, bb, d)
    }

    /// The Y coordinates of the shape's grid, ascending (`getCoords(Axis.Y)`).
    pub fn y_coords(&self) -> Vec<f64> {
        let n = self.count(1);
        (0..n).map(|i| self.coord(1, i)).collect()
    }

    /// The lowest/highest coordinate on an axis (`VoxelShape.min/max`).
    pub fn bounds(&self, axis: usize) -> (f64, f64) {
        (self.coord(axis, 0), self.coord(axis, self.count(axis) - 1))
    }
}

/// `IndirectMerger` for the `AND` operation: merges two ascending coordinate lists, snapping
/// coordinates that differ by less than `1e-7`, dropping the parts outside the second list's range.
/// Returns, per merged cell, the cell index in each list (`-1` = before the list).
fn indirect_merge_and(a: &impl Vox, axis: usize, b: [f64; 2]) -> Vec<(i64, i64)> {
    let n_a = a.count(axis);
    let n_b = 2_usize;
    let cap = n_a + n_b;
    let mut first = vec![0_i64; cap];
    let mut second = vec![0_i64; cap];
    let mut d = f64::NAN;
    let (mut l, mut m, mut n) = (0_usize, 0_usize, 0_usize);
    let result_length;
    loop {
        let a_done = m >= n_a;
        let b_done = n >= n_b;
        if a_done && b_done {
            result_length = l.max(1);
            break;
        }
        let take_a = !a_done && (b_done || a.coord(axis, m) < b[n] + EPSILON);
        if take_a {
            m += 1;
            if n == 0 || b_done {
                continue;
            }
        } else {
            n += 1;
            if m == 0 || a_done {
                continue;
            }
        }
        let o = m as i64 - 1;
        let p = n as i64 - 1;
        let e = if take_a {
            a.coord(axis, o as usize)
        } else {
            b[p as usize]
        };
        if !(d >= e - EPSILON) {
            first[l] = o;
            second[l] = p;
            l += 1;
            d = e;
        } else {
            first[l - 1] = o;
            second[l - 1] = p;
        }
    }
    (0..result_length.saturating_sub(1))
        .map(|j| (first[j], second[j]))
        .collect()
}

/// `Shapes.joinIsNotEmpty(shape, Shapes.create(box), BooleanOp.AND)`: do the shape and the box
/// overlap (with the game's `1e-7` coordinate snapping)?
fn shape_overlaps_box(shape: &Shape, b: Aabb) -> bool {
    // Shapes.create(AABB) is the empty shape for a box thinner than 1e-7 on any axis.
    if b.max.x - b.min.x < EPSILON || b.max.y - b.min.y < EPSILON || b.max.z - b.min.z < EPSILON {
        return false;
    }
    for axis in 0..3 {
        let (smin, smax) = shape.bounds(axis);
        if smax < get(b.min, axis) - EPSILON {
            return false;
        }
        if get(b.max, axis) < smin - EPSILON {
            return false;
        }
    }
    let mx = indirect_merge_and(shape, 0, [b.min.x, b.max.x]);
    let my = indirect_merge_and(shape, 1, [b.min.y, b.max.y]);
    let mz = indirect_merge_and(shape, 2, [b.min.z, b.max.z]);
    for &(i, j) in &mx {
        if j != 0 {
            continue;
        }
        for &(k, l) in &my {
            if l != 0 {
                continue;
            }
            for &(mm, n) in &mz {
                if n != 0 {
                    continue;
                }
                if is_full_wide(shape, i, k, mm) {
                    return true;
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------------------------
// BlockCollisions
// ---------------------------------------------------------------------------------------------

/// A block collision found by [`block_collisions`].
#[derive(Clone, Debug)]
pub struct BlockCollision {
    pub pos: (i32, i32, i32),
    pub shape: Shape,
}

/// Whether the context-free collision shape extends outside the unit cube (`hasLargeCollisionShape`),
/// tabulated once for every block state.
fn has_large_collision_shape(state: u32) -> bool {
    static LARGE: std::sync::OnceLock<Vec<bool>> = std::sync::OnceLock::new();
    let table = LARGE.get_or_init(|| {
        (0..ms_data::BLOCK_STATE_COUNT)
            .map(|s| {
                ms_data::collision_boxes(s)
                    .iter()
                    .any(|b| (0..3).any(|a| b[a] < 0.0 || b[a + 3] > 1.0))
            })
            .collect()
    });
    table[state as usize]
}

/// Whether a box list is exactly the unit cube (the game's `Shapes.block()`).
fn is_full_cube(boxes: &[[f64; 6]]) -> bool {
    matches!(boxes, [b] if *b == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0])
}

/// `BlockCollisions`: every block collision shape intersecting `query`, in the game's iteration
/// order. `p`/`ctx_bb` give the entity the shapes are queried for (scaffolding, powder snow).
/// With `only_suffocating`, only suffocating blocks are considered.
fn gather(
    world: &World,
    p: &PlayerState,
    ctx_bb: Aabb,
    query: Aabb,
    only_suffocating: bool,
    mut sink: impl FnMut(BlockCollision) -> bool,
) {
    let x0 = floor(query.min.x - EPSILON) - 1;
    let x1 = floor(query.max.x + EPSILON) + 1;
    let y0 = floor(query.min.y - EPSILON) - 1;
    let y1 = floor(query.max.y + EPSILON) + 1;
    let z0 = floor(query.min.z - EPSILON) - 1;
    let z1 = floor(query.max.z + EPSILON) + 1;
    for z in z0..=z1 {
        for y in y0..=y1 {
            for x in x0..=x1 {
                // Cursor3D type: how many coordinates are on the outer ring.
                let kind = i32::from(x == x0 || x == x1)
                    + i32::from(y == y0 || y == y1)
                    + i32::from(z == z0 || z == z1);
                if kind == 3 {
                    continue;
                }
                let state = world.block_state(x, y, z);
                if state == ms_data::AIR {
                    continue;
                }
                if only_suffocating && !ms_data::is_suffocating(state) {
                    continue;
                }
                if kind == 1 && !has_large_collision_shape(state) {
                    continue;
                }
                if kind == 2
                    && ms_data::block_name(ms_data::block_of_state(state))
                        != "minecraft:moving_piston"
                {
                    continue;
                }
                // A shape that stays inside its own cell cannot overlap a query that does not even
                // intersect the cell, so the shape lookup is skipped for those (every solid block of
                // the floor row, typically); only oversized shapes (fences, walls) need it.
                if kind == 0
                    && !has_large_collision_shape(state)
                    && !aabb_intersects(
                        query,
                        f64::from(x),
                        f64::from(y),
                        f64::from(z),
                        f64::from(x) + 1.0,
                        f64::from(y) + 1.0,
                        f64::from(z) + 1.0,
                    )
                {
                    continue;
                }
                let boxes = crate::blocks::collision_boxes(p, ctx_bb, world, x, y, z);
                if boxes.is_empty() {
                    continue;
                }
                let shape = Shape::from_boxes(&boxes, x, y, z).expect("non-empty");
                let hit = if is_full_cube(&boxes) {
                    // `voxelShape == Shapes.block()`: plain strict intersection with the cell.
                    aabb_intersects(
                        query,
                        f64::from(x),
                        f64::from(y),
                        f64::from(z),
                        f64::from(x) + 1.0,
                        f64::from(y) + 1.0,
                        f64::from(z) + 1.0,
                    )
                } else {
                    shape_overlaps_box(&shape, query)
                };
                if hit
                    && !sink(BlockCollision {
                        pos: (x, y, z),
                        shape,
                    })
                {
                    return;
                }
            }
        }
    }
}

/// `Level.getBlockCollisions(entity, box)`: the shapes, in the game's order.
pub fn block_collisions(
    world: &World,
    p: &PlayerState,
    ctx_bb: Aabb,
    query: Aabb,
) -> Vec<BlockCollision> {
    let mut out = Vec::new();
    gather(world, p, ctx_bb, query, false, |c| {
        out.push(c);
        true
    });
    out
}

/// `CollisionGetter.noBlockCollision`: no block collision shape intersects `query`.
pub fn no_block_collision(world: &World, p: &PlayerState, ctx_bb: Aabb, query: Aabb) -> bool {
    let mut any = false;
    gather(world, p, ctx_bb, query, false, |_| {
        any = true;
        false
    });
    !any
}

/// `Level.noCollision(entity, box)` for the player: blocks only (there are no other entities or a
/// near world border in the simulation).
pub fn no_collision(world: &World, p: &PlayerState, query: Aabb) -> bool {
    no_block_collision(world, p, bounding_box(p), query)
}

/// `CollisionGetter.collidesWithSuffocatingBlock`.
pub fn collides_with_suffocating_block(world: &World, p: &PlayerState, query: Aabb) -> bool {
    let mut any = false;
    gather(world, p, bounding_box(p), query, true, |_| {
        any = true;
        false
    });
    any
}

/// `CollisionGetter.findSupportingBlock`: the colliding block whose centre is nearest the player's
/// position (ties broken towards the greatest `BlockPos.compareTo`).
pub fn find_supporting_block(
    world: &World,
    p: &PlayerState,
    query: Aabb,
) -> Option<(i32, i32, i32)> {
    let mut best: Option<(i32, i32, i32)> = None;
    let mut best_d = f64::MAX;
    gather(world, p, bounding_box(p), query, false, |c| {
        let (x, y, z) = c.pos;
        let dx = f64::from(x) + 0.5 - p.pos.x;
        let dy = f64::from(y) + 0.5 - p.pos.y;
        let dz = f64::from(z) + 0.5 - p.pos.z;
        let e = dx * dx + dy * dy + dz * dz;
        let better = e < best_d
            || (e == best_d
                && match best {
                    None => true,
                    Some(b) => block_pos_compare(b, c.pos) < 0,
                });
        if better {
            best = Some(c.pos);
            best_d = e;
        }
        true
    });
    best
}

/// `Vec3i.compareTo`: by y, then z, then x.
fn block_pos_compare(a: (i32, i32, i32), b: (i32, i32, i32)) -> i32 {
    if a.1 == b.1 {
        if a.2 == b.2 {
            a.0.wrapping_sub(b.0)
        } else {
            a.2.wrapping_sub(b.2)
        }
    } else {
        a.1.wrapping_sub(b.1)
    }
}

// ---------------------------------------------------------------------------------------------
// Entity.collide
// ---------------------------------------------------------------------------------------------

/// `Direction.axisStepOrder`.
fn axis_step_order(motion: Vec3) -> [usize; 3] {
    if motion.x.abs() < motion.z.abs() {
        [1, 2, 0]
    } else {
        [1, 0, 2]
    }
}

/// `Shapes.collide(axis, box, shapes, d)`.
fn shapes_collide(axis: usize, bb: Aabb, shapes: &[BlockCollision], mut d: f64) -> f64 {
    for s in shapes {
        if d.abs() < EPSILON {
            return 0.0;
        }
        d = s.shape.collide(axis, bb, d);
    }
    d
}

/// `Entity.collideWithShapes`.
pub fn collide_with_shapes_list(motion: Vec3, bb: Aabb, shapes: &[BlockCollision]) -> Vec3 {
    if shapes.is_empty() {
        return motion;
    }
    let mut result = Vec3::ZERO;
    for axis in axis_step_order(motion) {
        let d = get(motion, axis);
        if d != 0.0 {
            let moved = aabb_move(bb, result.x, result.y, result.z);
            let e = shapes_collide(axis, moved, shapes, d);
            result = with(result, axis, e);
        }
    }
    result
}

/// `Entity.collideWithShapes` over plain boxes (each a one-box shape).
pub fn collide_with_shapes(motion: Vec3, bb: Aabb, colliders: &[Aabb]) -> Vec3 {
    let shapes: Vec<BlockCollision> = colliders
        .iter()
        .map(|&b| BlockCollision {
            pos: (0, 0, 0),
            shape: Shape::from_world_box(b),
        })
        .collect();
    collide_with_shapes_list(motion, bb, &shapes)
}

/// `Entity.collectCandidateStepUpHeights`: the distinct heights (floats, relative to the box
/// bottom) of the shapes' Y coordinates within reach, sorted ascending.
fn candidate_step_up_heights(
    bb: Aabb,
    shapes: &[BlockCollision],
    max_up: f32,
    current: f32,
) -> Vec<f32> {
    let mut heights: Vec<f32> = Vec::new();
    for s in shapes {
        for d in s.shape.y_coords() {
            let h = (d - bb.min.y) as f32;
            if !(h < 0.0) && h != current {
                if h > max_up {
                    break;
                }
                if !heights.contains(&h) {
                    heights.push(h);
                }
            }
        }
    }
    heights.sort_by(f32::total_cmp);
    heights
}

/// `Entity.collide`: resolve `motion` against the world, with step-up. `step_height` is
/// `maxUpStep()` (the step-height attribute as a float).
pub fn collide(p: &PlayerState, world: &World, motion: Vec3, step_height: f32) -> Vec3 {
    let bb = bounding_box(p);
    let length_sqr = motion.x * motion.x + motion.y * motion.y + motion.z * motion.z;
    let collided = if length_sqr == 0.0 {
        motion
    } else {
        let query = aabb_expand_towards(bb, motion.x, motion.y, motion.z);
        let shapes = block_collisions(world, p, bb, query);
        collide_with_shapes_list(motion, bb, &shapes)
    };
    let hit_x = motion.x != collided.x;
    let hit_y = motion.y != collided.y;
    let hit_z = motion.z != collided.z;
    let hit_down = hit_y && motion.y < 0.0;
    if step_height > 0.0 && (hit_down || p.on_ground) && (hit_x || hit_z) {
        let bb2 = if hit_down {
            aabb_move(bb, 0.0, collided.y, 0.0)
        } else {
            bb
        };
        let mut bb3 = aabb_expand_towards(bb2, motion.x, f64::from(step_height), motion.z);
        if !hit_down {
            bb3 = aabb_expand_towards(bb3, 0.0, f64::from(-1.0E-5_f32), 0.0);
        }
        let shapes = block_collisions(world, p, bb, bb3);
        let current = collided.y as f32;
        let heights = candidate_step_up_heights(bb2, &shapes, step_height, current);
        let horizontal_sqr = |v: Vec3| v.x * v.x + v.z * v.z;
        for g in heights {
            let stepped =
                collide_with_shapes_list(Vec3::new(motion.x, f64::from(g), motion.z), bb2, &shapes);
            if horizontal_sqr(stepped) > horizontal_sqr(collided) {
                let d = bb.min.y - bb2.min.y;
                return Vec3::new(stepped.x + -0.0, stepped.y + -d, stepped.z + -0.0);
            }
        }
    }
    collided
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(feet_y: f64) -> Aabb {
        aabb(0.0, feet_y, 0.0, 0.6, feet_y + 1.8, 0.6)
    }

    fn unit_block() -> Aabb {
        aabb(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)
    }

    #[test]
    fn empty_world_does_not_clamp() {
        let m = collide_with_shapes(Vec3::new(0.3, -0.2, 0.1), player(1.0), &[]);
        assert_eq!(m, Vec3::new(0.3, -0.2, 0.1));
    }

    #[test]
    fn falls_to_block_surface() {
        let m = collide_with_shapes(Vec3::new(0.0, -0.9, 0.0), player(1.5), &[unit_block()]);
        assert_eq!(m.y, -0.5);
        assert_eq!(m.x, 0.0);
        assert_eq!(m.z, 0.0);
    }

    #[test]
    fn resting_stays_put() {
        let m = collide_with_shapes(Vec3::new(0.0, -0.0784, 0.0), player(1.0), &[unit_block()]);
        assert_eq!(m.y, 0.0);
    }

    #[test]
    fn stops_at_wall() {
        let wall = aabb(1.0, 0.0, 0.0, 2.0, 3.0, 1.0);
        let m = collide_with_shapes(Vec3::new(0.5, 0.0, 0.0), player(1.0), &[wall]);
        assert_eq!(m.x, 1.0 - 0.6);
    }

    #[test]
    fn unobstructed_horizontal_passes() {
        // A block to the side the player does not overlap vertically: no clamp.
        let m = collide_with_shapes(Vec3::new(0.5, 0.0, 0.0), player(5.0), &[unit_block()]);
        assert_eq!(m.x, 0.5);
    }

    #[test]
    fn jmin_jmax_follow_java_for_signed_zero() {
        assert_eq!(jmin(0.0, -0.0).to_bits(), (-0.0_f64).to_bits());
        assert_eq!(jmin(-0.0, 0.0).to_bits(), (-0.0_f64).to_bits());
        assert_eq!(jmax(-0.0, 0.0).to_bits(), 0.0_f64.to_bits());
        assert_eq!(jmax(0.0, -0.0).to_bits(), 0.0_f64.to_bits());
        assert!(jmin(f64::NAN, 1.0).is_nan());
        assert!(jmax(1.0, f64::NAN).is_nan());
    }

    #[test]
    fn grid_shape_collides_like_its_boxes() {
        // A bottom slab plus a post: standing next to the post, moving +x stops at its face.
        let boxes = [
            [0.0, 0.0, 0.0, 1.0, 0.5, 1.0],
            [0.375, 0.5, 0.375, 0.625, 1.0, 0.625],
        ];
        let s = Shape::from_boxes(&boxes, 5, 0, 0).unwrap();
        // The box hovers over the slab's top, beside the post.
        let b = aabb(4.0, 0.5, 5.0 - 0.0, 4.2, 2.3, 5.0);
        let _ = b;
        let bb = aabb(4.5, 0.5, 0.4, 5.1, 2.3, 0.6);
        let d = s.collide(0, bb, 1.0);
        // Post face at x = 5.375: the box's max x is 5.1.
        assert!((d - (5.375 - 5.1)).abs() < 1e-12, "{d}");
    }

    #[test]
    fn partial_shapes_use_snapped_overlap() {
        let slab = Shape::from_boxes(&[[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]], 0, 0, 0).unwrap();
        // Overlapping the slab top by 1e-14 does not count as a collision for a partial shape...
        let barely = aabb(0.2, 0.5 - 1.0e-14, 0.2, 0.8, 2.3, 0.8);
        assert!(!shape_overlaps_box(&slab, barely));
        // ...but a real overlap does.
        let real = aabb(0.2, 0.4, 0.2, 0.8, 2.2, 0.8);
        assert!(shape_overlaps_box(&slab, real));
    }

    #[test]
    fn find_index_matches_binary_search_semantics() {
        let s = Shape::from_boxes(&[[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]], 0, 0, 0).unwrap();
        assert_eq!(find_index(&s, 1, -1.0), -1);
        assert_eq!(find_index(&s, 1, 0.0), 0);
        assert_eq!(find_index(&s, 1, 0.25), 0);
        assert_eq!(find_index(&s, 1, 0.5), 1);
        assert_eq!(find_index(&s, 1, 3.0), 1);
    }
}
