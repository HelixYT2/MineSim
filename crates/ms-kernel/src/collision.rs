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
//! Collision boxes are in block-local coordinates; this module places them in the world and does
//! the rest. A block whose shape depends on who asks or where it is (scaffolding, powder snow: the
//! `ms_data::class::CONTEXT_SHAPE` class) gets them from [`crate::blocks::collision_boxes`]; every
//! other block's boxes are the table's (`ms_data::collision_boxes`), which is what that function
//! returns for them too, so they are read straight from the table instead of being copied into a
//! fresh `Vec` per block per query (see the test `context_free_blocks_need_no_block_module`, which
//! fails if the block module starts treating another block specially without giving it the class).

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
///
/// Two ordered operands, the overwhelmingly common case, are decided by one comparison; the
/// reference's special cases (NaN, equal values, the zeros) only arise when neither `a < b` nor
/// `a > b` holds, and go through [`jmin_edge`], the reference's own sequence of tests.
#[inline(always)]
pub fn jmin(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else if a > b {
        b
    } else {
        jmin_edge(a, b)
    }
}

#[inline(never)]
fn jmin_edge(a: f64, b: f64) -> f64 {
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

/// `Math.max(double, double)` (NaN-propagating, `-0.0 < 0.0`); see [`jmin`].
#[inline(always)]
pub fn jmax(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else if a < b {
        b
    } else {
        jmax_edge(a, b)
    }
}

#[inline(never)]
fn jmax_edge(a: f64, b: f64) -> f64 {
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

/// [`voxel_collide`] for a shape that is one filled cell (`[min, max]` on each axis), worked out
/// without the binary searches: with two coordinates per axis `findIndex` can only answer -1, 0 or
/// 1, there is a single cell, and the layer scans reduce to one test of whether that cell lies in
/// the window of the box on the two other axes. Same results bit for bit (checked against the
/// general routine in the tests).
fn single_collide(min: &[f64; 3], max: &[f64; 3], axis: usize, bb: Aabb, d: f64) -> f64 {
    match axis {
        0 => single_collide_on::<0, 1, 2>(min, max, bb, d),
        1 => single_collide_on::<1, 2, 0>(min, max, bb, d),
        _ => single_collide_on::<2, 0, 1>(min, max, bb, d),
    }
}

/// Component `A` (0 = x, 1 = y, 2 = z) of a vector, resolved at compile time.
#[inline(always)]
fn component<const A: usize>(v: Vec3) -> f64 {
    match A {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

/// [`single_collide`] for the axis `A`, with `B` and `C` the other two in `voxel_collide`'s order.
#[inline(always)]
fn single_collide_on<const A: usize, const B: usize, const C: usize>(
    min: &[f64; 3],
    max: &[f64; 3],
    bb: Aabb,
    mut d: f64,
) -> f64 {
    if d.abs() < EPSILON {
        return 0.0;
    }
    // `findIndex` over the two coordinates `[lo, hi]`: the index of the last one `<= v`.
    let find = |lo: f64, hi: f64, v: f64| -> i64 {
        if !(v < hi) {
            1
        } else if v < lo {
            -1
        } else {
            0
        }
    };
    // Does the cell lie in the box's window (shrunk by 1e-7) on the two other axes? Only then can
    // it stop the move.
    let (b_lo, b_hi) = (min[B], max[B]);
    let k = find(b_lo, b_hi, component::<B>(bb.min) + EPSILON).max(0);
    let l = 1.min(find(b_lo, b_hi, component::<B>(bb.max) - EPSILON) + 1);
    let (c_lo, c_hi) = (min[C], max[C]);
    let m = find(c_lo, c_hi, component::<C>(bb.min) + EPSILON).max(0);
    let n = 1.min(find(c_lo, c_hi, component::<C>(bb.max) - EPSILON) + 1);
    if !(k < l && m < n) {
        return d;
    }
    let (a_lo, a_hi) = (min[A], max[A]);
    if d > 0.0 {
        // The scan starts at layer `j + 1`, where `j` is the index of the box's far face; the only
        // layer is 0.
        let e = component::<A>(bb.max);
        if find(a_lo, a_hi, e - EPSILON) == -1 {
            let g = a_lo - e;
            if g >= -EPSILON {
                d = jmin(d, g);
            }
        }
    } else if d < 0.0 {
        // The scan starts at layer `i - 1`, where `i` is the index of the box's near face; the only
        // layer is 0.
        let f = component::<A>(bb.min);
        if find(a_lo, a_hi, f + EPSILON) == 1 {
            let g = a_hi - f;
            if g <= EPSILON {
                d = jmax(d, g);
            }
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
        match &self.repr {
            Repr::Single { min, max } => single_collide(min, max, axis, bb, d),
            Repr::Grid { .. } => voxel_collide(self, axis, bb, d),
        }
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

/// Whether a box list is exactly the unit cube (the game's `Shapes.block()`).
fn is_full_cube(boxes: &[[f64; 6]]) -> bool {
    matches!(boxes, [b] if *b == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0])
}

/// A block whose collision shape intersects the query of [`gather`].
struct Hit<'a> {
    pos: (i32, i32, i32),
    /// The block's boxes in block-local coordinates (never empty).
    boxes: &'a [[f64; 6]],
    /// The shape placed in the world, when it had to be built to decide the hit (every shape
    /// but the full cube; boxed to keep a hit small); the sink builds it itself otherwise.
    shape: Option<Box<Shape>>,
}

impl Hit<'_> {
    /// The hit as the collision resolution keeps it: a plain cube is just its cell (every solid
    /// block of a typical floor), anything else its placed shape.
    fn into_piece(self) -> Piece {
        match self.shape {
            None => Piece::Cube([self.pos.0, self.pos.1, self.pos.2]),
            Some(shape) => Piece::Shape(shape),
        }
    }

    fn into_collision(self) -> BlockCollision {
        let (x, y, z) = self.pos;
        let shape = match self.shape {
            Some(shape) => *shape,
            None => Shape::from_boxes(self.boxes, x, y, z).expect("non-empty"),
        };
        BlockCollision {
            pos: self.pos,
            shape,
        }
    }
}

/// Whether every block-collision query whose box starts at height `y` or higher is certain to
/// find nothing: the world holds no block that reaches up to `y` (everything at or above
/// `max_block_y + 1` is air), and none of the blocks the cursor's outer ring would admit (a
/// shape larger than a cube, a moving piston), which could reach up from below the box's own
/// cells. A NaN height is never clear.
///
/// A query box starting at `y` has its lowest cell at `floor(y)` and no cell intersecting it lies
/// lower, so with `y >= max_block_y + 1` the cells it could intersect are all air.
#[inline]
pub fn clear_above(world: &World, y: f64) -> bool {
    y >= f64::from(world.max_block_y()) + 1.0 && !world.may_contain(ms_data::class::RING_RELEVANT)
}

/// [`clear_above`] for the suffocation queries, which only look at suffocating blocks (full cubes,
/// so the ring never matters for them).
#[inline]
pub fn clear_of_suffocating_above(world: &World, y: f64) -> bool {
    y >= f64::from(world.max_block_y()) + 1.0
        && !world.may_contain(ms_data::class::SUFFOCATING_RING_RELEVANT)
}

/// `BlockCollisions`: every block collision shape intersecting `query`, in the game's iteration
/// order. `p`/`ctx_bb` give the entity the shapes are queried for (scaffolding, powder snow).
/// With `only_suffocating`, only suffocating blocks are considered. The sink returns whether to
/// keep going.
///
/// The reference walks a cursor over the cells around `query`; the cells on its outer ring
/// ("kind" 1 or 2: one or two coordinates on the boundary) only matter for blocks with a shape
/// larger than a cube, or a moving piston, and the corners (kind 3) never do. When the world is
/// known to hold no such block (`ms_data::class::RING_RELEVANT` clear in its class union), every
/// ring cell is skipped whatever it holds, and an inner cell that does not intersect the query is
/// skipped too (its shape stays within the cell). The walk then only visits the inner cells that
/// can intersect the query, in the same order; the visited cells that matter are exactly the ones
/// the full walk would have reported.
fn gather(
    world: &World,
    p: &PlayerState,
    ctx_bb: Aabb,
    query: Aabb,
    only_suffocating: bool,
    mut sink: impl FnMut(Hit<'_>) -> bool,
) {
    // Suffocating blocks are full cubes, so for the suffocation queries the ring never matters.
    let ring_mask = if only_suffocating {
        ms_data::class::SUFFOCATING_RING_RELEVANT
    } else {
        ms_data::class::RING_RELEVANT
    };
    let ring_inert = !world.may_contain(ring_mask);
    // Everything above the world's highest block is air: those rows need not be visited, and a
    // query lying wholly above them intersects nothing (see `clear_above`).
    let max_y = world.max_block_y();
    if ring_inert && query.min.y >= f64::from(max_y) + 1.0 {
        return;
    }

    // The block at one cell, decided against the query; false stops the walk.
    let mut visit = |x: i32, y: i32, z: i32, kind: i32| -> bool {
        let state = world.block_state(x, y, z);
        if state == ms_data::AIR {
            return true;
        }
        if only_suffocating && !ms_data::is_suffocating(state) {
            return true;
        }
        let classes = ms_data::state_class(state);
        let large = classes & ms_data::class::LARGE_SHAPE != 0;
        if kind == 1 && !large {
            return true;
        }
        if kind == 2 && classes & ms_data::class::MOVING_PISTON == 0 {
            return true;
        }
        // A shape that stays inside its own cell cannot overlap a query that does not even
        // intersect the cell, so the shape lookup is skipped for those (every solid block of
        // the floor row, typically); only oversized shapes (fences, walls) need it.
        let cell_hit = aabb_intersects(
            query,
            f64::from(x),
            f64::from(y),
            f64::from(z),
            f64::from(x) + 1.0,
            f64::from(y) + 1.0,
            f64::from(z) + 1.0,
        );
        if kind == 0 && !large && !cell_hit {
            return true;
        }
        let entity_boxes;
        let boxes: &[[f64; 6]] = if classes & ms_data::class::CONTEXT_SHAPE == 0 {
            ms_data::collision_boxes(state)
        } else {
            entity_boxes = crate::blocks::collision_boxes(p, ctx_bb, world, x, y, z);
            &entity_boxes
        };
        if boxes.is_empty() {
            return true;
        }
        let (hit, shape) = if is_full_cube(boxes) {
            // `voxelShape == Shapes.block()`: plain strict intersection with the cell.
            (cell_hit, None)
        } else {
            let shape = Shape::from_boxes(boxes, x, y, z).expect("non-empty");
            (shape_overlaps_box(&shape, query), Some(Box::new(shape)))
        };
        !hit || sink(Hit {
            pos: (x, y, z),
            boxes,
            shape,
        })
    };

    // Far from the origin the reference's own cell arithmetic overflows; such queries always take
    // the general walk.
    const SAFE: i32 = 1 << 30;
    let in_range = |v: i32| v > -SAFE && v < SAFE;

    // The cells that can intersect the query: `floor(min) <= x <= floor(max)` on each axis (a
    // cell intersects only if its far face is beyond `min` and its near face before `max`). This
    // range lies inside the walk's inner cells (x0 < x < x1, ...), since
    // `floor(min - EPSILON) <= floor(min)` and `floor(max) <= floor(max + EPSILON)`.
    let (xlo, xhi) = (floor(query.min.x), floor(query.max.x));
    let (ylo, yhi) = (floor(query.min.y), floor(query.max.y));
    let (zlo, zhi) = (floor(query.min.z), floor(query.max.z));
    let narrow_in_range = in_range(xlo)
        && in_range(xhi)
        && in_range(ylo)
        && in_range(yhi)
        && in_range(zlo)
        && in_range(zhi);

    if ring_inert && narrow_in_range {
        // The ring holds nothing anywhere in this world: visit only the cells that can intersect.
        for z in zlo..=zhi {
            for y in ylo..=yhi.min(max_y) {
                for x in xlo..=xhi {
                    if !visit(x, y, z, 0) {
                        return;
                    }
                }
            }
        }
        return;
    }

    // The cells the reference walks: `x0..=x1` and so on, the box around the query's cells plus a
    // one-cell margin whose outer shell is the "ring".
    let fx0 = floor(query.min.x - EPSILON);
    let fx1 = floor(query.max.x + EPSILON);
    let fy0 = floor(query.min.y - EPSILON);
    let fy1 = floor(query.max.y + EPSILON);
    let fz0 = floor(query.min.z - EPSILON);
    let fz1 = floor(query.max.z + EPSILON);
    let (x0, x1) = (fx0.wrapping_sub(1), fx1.wrapping_add(1));
    let (y0, y1) = (fy0.wrapping_sub(1), fy1.wrapping_add(1));
    let (z0, z1) = (fz0.wrapping_sub(1), fz1.wrapping_add(1));
    // A world that does hold blocks the ring would look at may still hold none in this walk's box,
    // which makes the ring just as inert.
    if narrow_in_range
        && in_range(fx0)
        && in_range(fx1)
        && in_range(fy0)
        && in_range(fy1)
        && in_range(fz0)
        && in_range(fz1)
        && !world.ring_relevant_in([x0, y0, z0], [x1, y1, z1])
    {
        for z in zlo..=zhi {
            for y in ylo..=yhi.min(max_y) {
                for x in xlo..=xhi {
                    if !visit(x, y, z, 0) {
                        return;
                    }
                }
            }
        }
        return;
    }

    let y_last = y1.min(max_y);
    for z in z0..z1.wrapping_add(1) {
        for y in y0..y_last.saturating_add(1) {
            for x in x0..x1.wrapping_add(1) {
                // Cursor3D type: how many coordinates are on the outer ring.
                let kind = i32::from(x == x0 || x == x1)
                    + i32::from(y == y0 || y == y1)
                    + i32::from(z == z0 || z == z1);
                if kind == 3 {
                    continue;
                }
                if !visit(x, y, z, kind) {
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
    gather(world, p, ctx_bb, query, false, |h| {
        out.push(h.into_collision());
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

/// One colliding block as `Entity.collide` keeps it while resolving a move: a full cube is just
/// its cell (the shape it stands for is the unit box at that position), any other shape is boxed.
enum Piece {
    Cube([i32; 3]),
    Shape(Box<Shape>),
}

impl Piece {
    /// `VoxelShape.collide(axis, box, d)` of the piece's shape.
    #[inline]
    fn collide(&self, axis: usize, bb: Aabb, d: f64) -> f64 {
        match self {
            Piece::Cube(c) => {
                // The shape `Shape::from_boxes` makes of the unit cube at this cell.
                let min = [f64::from(c[0]), f64::from(c[1]), f64::from(c[2])];
                let max = [min[0] + 1.0, min[1] + 1.0, min[2] + 1.0];
                single_collide(&min, &max, axis, bb, d)
            }
            Piece::Shape(s) => s.collide(axis, bb, d),
        }
    }

    /// The Y coordinates of the piece's shape, ascending (`getCoords(Axis.Y)`), fed to `f` until it
    /// returns false.
    fn for_each_y(&self, mut f: impl FnMut(f64) -> bool) {
        match self {
            Piece::Cube(c) => {
                let y = f64::from(c[1]);
                if f(y) {
                    f(y + 1.0);
                }
            }
            Piece::Shape(s) => {
                for d in s.y_coords() {
                    if !f(d) {
                        break;
                    }
                }
            }
        }
    }
}

/// The blocks found by one query, in order, without touching the heap unless there are more than
/// a handful of them.
struct Pieces {
    inline: [Piece; 8],
    len: usize,
    spill: Vec<Piece>,
}

impl Pieces {
    fn new() -> Self {
        Pieces {
            inline: std::array::from_fn(|_| Piece::Cube([0; 3])),
            len: 0,
            spill: Vec::new(),
        }
    }

    fn push(&mut self, piece: Piece) {
        if self.len < self.inline.len() {
            self.inline[self.len] = piece;
            self.len += 1;
        } else {
            self.spill.push(piece);
        }
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn iter(&self) -> impl Iterator<Item = &Piece> {
        self.inline[..self.len].iter().chain(self.spill.iter())
    }

    /// Everything block `gather` reports for `query`.
    fn gather(world: &World, p: &PlayerState, ctx_bb: Aabb, query: Aabb) -> Pieces {
        let mut pieces = Pieces::new();
        gather(world, p, ctx_bb, query, false, |h| {
            pieces.push(h.into_piece());
            true
        });
        pieces
    }
}

/// A set of shapes that `Entity.collideWithShapes` can resolve a move against.
trait ShapeSet {
    fn is_empty(&self) -> bool;
    /// `Shapes.collide(axis, box, shapes, d)`.
    fn collide_axis(&self, axis: usize, bb: Aabb, d: f64) -> f64;
}

impl ShapeSet for [BlockCollision] {
    fn is_empty(&self) -> bool {
        <[BlockCollision]>::is_empty(self)
    }

    fn collide_axis(&self, axis: usize, bb: Aabb, mut d: f64) -> f64 {
        for s in self {
            if d.abs() < EPSILON {
                return 0.0;
            }
            d = s.shape.collide(axis, bb, d);
        }
        d
    }
}

impl ShapeSet for Pieces {
    fn is_empty(&self) -> bool {
        Pieces::is_empty(self)
    }

    fn collide_axis(&self, axis: usize, bb: Aabb, mut d: f64) -> f64 {
        for s in self.iter() {
            if d.abs() < EPSILON {
                return 0.0;
            }
            d = s.collide(axis, bb, d);
        }
        d
    }
}

/// `Entity.collideWithShapes`.
fn collide_with<S: ShapeSet + ?Sized>(motion: Vec3, bb: Aabb, shapes: &S) -> Vec3 {
    if shapes.is_empty() {
        return motion;
    }
    let mut result = Vec3::ZERO;
    for axis in axis_step_order(motion) {
        let d = get(motion, axis);
        if d != 0.0 {
            let moved = aabb_move(bb, result.x, result.y, result.z);
            let e = shapes.collide_axis(axis, moved, d);
            result = with(result, axis, e);
        }
    }
    result
}

/// `Entity.collideWithShapes`.
pub fn collide_with_shapes_list(motion: Vec3, bb: Aabb, shapes: &[BlockCollision]) -> Vec3 {
    collide_with(motion, bb, shapes)
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
fn candidate_step_up_heights(bb: Aabb, shapes: &Pieces, max_up: f32, current: f32) -> Vec<f32> {
    let mut heights: Vec<f32> = Vec::new();
    for s in shapes.iter() {
        s.for_each_y(|d| {
            let h = (d - bb.min.y) as f32;
            if !(h < 0.0) && h != current {
                if h > max_up {
                    return false;
                }
                if !heights.contains(&h) {
                    heights.push(h);
                }
            }
            true
        });
    }
    heights.sort_by(f32::total_cmp);
    heights
}

/// `Entity.collide`: resolve `motion` against the world, with step-up. `step_height` is
/// `maxUpStep()` (the step-height attribute as a float).
pub fn collide(p: &PlayerState, world: &World, motion: Vec3, step_height: f32) -> Vec3 {
    collide_at(p, world, bounding_box(p), p.on_ground, motion, step_height)
}

/// [`collide`] for the box `bb` with ground flag `on_ground` instead of the player's own (the
/// server's copy of the player moves from its own position and ground state). `p` only supplies
/// the entity context for entity-dependent block shapes.
pub fn collide_at(
    p: &PlayerState,
    world: &World,
    bb: Aabb,
    on_ground: bool,
    motion: Vec3,
    step_height: f32,
) -> Vec3 {
    let length_sqr = motion.x * motion.x + motion.y * motion.y + motion.z * motion.z;
    // The lowest point of the query box (`bb` grown towards `motion`): the world is checked
    // against that height before the box is even built.
    let query_low = if motion.y < 0.0 {
        bb.min.y + motion.y
    } else {
        bb.min.y
    };
    let collided = if length_sqr == 0.0 || clear_above(world, query_low) {
        // Nothing to collide with (no shapes: `collide_with_shapes_list` returns the motion).
        motion
    } else {
        let query = aabb_expand_towards(bb, motion.x, motion.y, motion.z);
        let shapes = Pieces::gather(world, p, bb, query);
        collide_with(motion, bb, &shapes)
    };
    let hit_x = motion.x != collided.x;
    let hit_y = motion.y != collided.y;
    let hit_z = motion.z != collided.z;
    let hit_down = hit_y && motion.y < 0.0;
    if step_height > 0.0 && (hit_down || on_ground) && (hit_x || hit_z) {
        let bb2 = if hit_down {
            aabb_move(bb, 0.0, collided.y, 0.0)
        } else {
            bb
        };
        let mut bb3 = aabb_expand_towards(bb2, motion.x, f64::from(step_height), motion.z);
        if !hit_down {
            bb3 = aabb_expand_towards(bb3, 0.0, f64::from(-1.0E-5_f32), 0.0);
        }
        let shapes = Pieces::gather(world, p, bb, bb3);
        let current = collided.y as f32;
        let heights = candidate_step_up_heights(bb2, &shapes, step_height, current);
        let horizontal_sqr = |v: Vec3| v.x * v.x + v.z * v.z;
        for g in heights {
            let stepped = collide_with(Vec3::new(motion.x, f64::from(g), motion.z), bb2, &shapes);
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

    /// `gather` takes the boxes of every block without the `CONTEXT_SHAPE` class straight from the
    /// table instead of asking the block module. That is only right while the block module returns
    /// the table's boxes for such blocks whoever asks and wherever they are; if this fails, the
    /// block module has started to treat some block specially: give that block the
    /// `ms_data::class::CONTEXT_SHAPE` class (in `ms-data`) so queries keep asking the module.
    #[test]
    fn context_free_blocks_need_no_block_module() {
        use crate::state::Pose;
        use ms_world::{FlatWorld, GridWorld};
        use std::sync::Arc;

        let base = PlayerState::new(Vec3::new(0.5, 0.0, 0.5), 0.0);
        let mut falling = base.clone();
        falling.fall_distance = 6.0;
        falling.vel = Vec3::new(0.0, -1.0, 0.0);
        let mut sneaking = base.clone();
        sneaking.shift_key_down = true;
        sneaking.crouching = true;
        sneaking.pose = Pose::Crouching;
        let mut powder = base.clone();
        powder.in_powder_snow = true;
        powder.was_in_powder_snow = true;
        powder.pos = Vec3::new(12.3, 40.7, -5.1);
        let mut grounded = base.clone();
        grounded.on_ground = true;
        grounded.sprinting = true;
        let contexts = [base, falling, sneaking, powder, grounded];

        let positions = [
            (0, 0, 0),
            (1, 5, 3),
            (-7, 64, -2),
            (100, -30, 250),
            (-1000, 12, 999),
            (13, 200, -77),
            (3, 3, 3),
            (-1, -1, -1),
        ];
        let mut world = World::grid(GridWorld::new(FlatWorld::void()));
        for state in 1..ms_data::BLOCK_STATE_COUNT {
            if ms_data::state_class(state) & ms_data::class::CONTEXT_SHAPE != 0 {
                continue;
            }
            let table = ms_data::collision_boxes(state).to_vec();
            for &(x, y, z) in &positions {
                let World::Grid(grid) = &mut world else {
                    unreachable!()
                };
                Arc::make_mut(grid).set_block(x, y, z, state);
                for p in &contexts {
                    let got = crate::blocks::collision_boxes(p, bounding_box(p), &world, x, y, z);
                    assert_eq!(
                        got,
                        table,
                        "{} at ({x}, {y}, {z}) for a player at {:?}: the block module returns \
                         something other than the table's boxes for a block without the \
                         CONTEXT_SHAPE class",
                        ms_data::state_to_string(state),
                        p.pos
                    );
                }
            }
        }
    }

    /// `BlockCollisions` exactly as the reference walks it: every cell of the box around the
    /// query, ring included, with each cell's block looked at (the walk `gather` was before it
    /// learnt to skip cells that cannot matter). Returns the shapes it reports, in order.
    fn reference_gather(
        world: &World,
        p: &PlayerState,
        ctx_bb: Aabb,
        query: Aabb,
        only_suffocating: bool,
    ) -> Vec<BlockCollision> {
        let large = |state: u32| {
            ms_data::collision_boxes(state)
                .iter()
                .any(|b| (0..3).any(|a| b[a] < 0.0 || b[a + 3] > 1.0))
        };
        let x0 = floor(query.min.x - EPSILON) - 1;
        let x1 = floor(query.max.x + EPSILON) + 1;
        let y0 = floor(query.min.y - EPSILON) - 1;
        let y1 = floor(query.max.y + EPSILON) + 1;
        let z0 = floor(query.min.z - EPSILON) - 1;
        let z1 = floor(query.max.z + EPSILON) + 1;
        let mut out = Vec::new();
        for z in z0..z1 + 1 {
            for y in y0..y1 + 1 {
                for x in x0..x1 + 1 {
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
                    if kind == 1 && !large(state) {
                        continue;
                    }
                    if kind == 2
                        && ms_data::block_name(ms_data::block_of_state(state))
                            != "minecraft:moving_piston"
                    {
                        continue;
                    }
                    let cell_hit = aabb_intersects(
                        query,
                        f64::from(x),
                        f64::from(y),
                        f64::from(z),
                        f64::from(x) + 1.0,
                        f64::from(y) + 1.0,
                        f64::from(z) + 1.0,
                    );
                    if kind == 0 && !large(state) && !cell_hit {
                        continue;
                    }
                    let boxes = crate::blocks::collision_boxes(p, ctx_bb, world, x, y, z);
                    if boxes.is_empty() {
                        continue;
                    }
                    let shape = Shape::from_boxes(&boxes, x, y, z).expect("non-empty");
                    let hit = if is_full_cube(&boxes) {
                        cell_hit
                    } else {
                        shape_overlaps_box(&shape, query)
                    };
                    if hit {
                        out.push(BlockCollision {
                            pos: (x, y, z),
                            shape,
                        });
                    }
                }
            }
        }
        out
    }

    /// The walk `gather` does now (with its shortcuts for worlds that cannot hold anything the ring
    /// would look at, and for queries above every block) reports exactly what the reference walk
    /// does, on random worlds that do and do not hold such blocks and random query boxes.
    #[test]
    fn gather_reports_what_the_full_walk_reports() {
        use ms_world::{FlatWorld, GridWorld};
        let mut rng = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };
        let unit = |r: u64| (r >> 11) as f64 / (1u64 << 53) as f64;
        let all: Vec<u32> = (1..ms_data::BLOCK_STATE_COUNT).collect();
        let ring = ms_data::class::RING_RELEVANT;
        let plain: Vec<u32> = all
            .iter()
            .copied()
            .filter(|&s| ms_data::state_class(s) & ring == 0)
            .collect();
        let ring_states: Vec<u32> = all
            .iter()
            .copied()
            .filter(|&s| ms_data::state_class(s) & ring != 0)
            .collect();
        assert!(!ring_states.is_empty());
        let player = PlayerState::new(Vec3::new(0.5, 0.0, 0.5), 0.0);
        let stone = ms_data::parse_state("minecraft:stone").unwrap();
        let mut compared = 0usize;
        let mut nonempty = 0usize;
        for case in 0..300 {
            let base = match case % 4 {
                0 => FlatWorld::void(),
                1 => FlatWorld::new(0, stone),
                2 => FlatWorld::new(3, stone),
                _ => FlatWorld::new(0, ms_data::AIR),
            };
            let mut grid = GridWorld::new(base);
            // Most worlds hold only blocks the ring ignores; some also a few it would look at,
            // near the queries or far from them.
            let with_ring = case % 3 == 0;
            for _ in 0..(10 + next() % 120) {
                let (x, y, z) = (
                    (next() % 17) as i32 - 8,
                    (next() % 8) as i32 - 3,
                    (next() % 17) as i32 - 8,
                );
                let pool = if with_ring && next() % 6 == 0 {
                    &ring_states
                } else {
                    &plain
                };
                grid.set_block(x, y, z, pool[(next() % pool.len() as u64) as usize]);
            }
            if with_ring && case % 2 == 0 {
                // One far from every query below.
                grid.set_block(
                    200,
                    1,
                    -200,
                    ring_states[(next() % ring_states.len() as u64) as usize],
                );
            }
            let world = if case % 5 == 4 {
                World::Flat(base)
            } else {
                World::grid(grid)
            };
            for _ in 0..30 {
                let c = (
                    unit(next()) * 20.0 - 10.0,
                    unit(next()) * 8.0 - 3.0,
                    unit(next()) * 20.0 - 10.0,
                );
                let size = (unit(next()) * 3.0, unit(next()) * 3.0, unit(next()) * 3.0);
                let mut query = aabb(c.0, c.1, c.2, c.0 + size.0, c.1 + size.1, c.2 + size.2);
                match next() % 12 {
                    // Snap onto cell faces, where the 1e-7 margins matter.
                    0 => query = aabb_inflate(query, 1.0e-7, 0.0, 0.0),
                    1 => {
                        query = aabb(
                            c.0.floor(),
                            c.1.floor(),
                            c.2.floor(),
                            c.0.floor() + 1.0,
                            c.1.floor() + 1.8,
                            c.2.floor() + 1.0,
                        )
                    }
                    2 => query.min.y = f64::NAN,
                    3 => query.max.x = f64::NAN,
                    _ => {}
                }
                for only_suffocating in [false, true] {
                    let want = reference_gather(
                        &world,
                        &player,
                        bounding_box(&player),
                        query,
                        only_suffocating,
                    );
                    let mut got = Vec::new();
                    gather(
                        &world,
                        &player,
                        bounding_box(&player),
                        query,
                        only_suffocating,
                        |h| {
                            got.push(h.into_collision());
                            true
                        },
                    );
                    assert_eq!(
                        format!("{got:?}"),
                        format!("{want:?}"),
                        "case {case}, query {query:?}, suffocating {only_suffocating}"
                    );
                    compared += 1;
                    nonempty += usize::from(!want.is_empty());
                }
            }
        }
        assert!(compared > 10_000);
        assert!(nonempty > 1_000, "only {nonempty} queries hit anything");
    }

    /// The reference's sequence of tests, verbatim.
    fn ref_min(a: f64, b: f64) -> f64 {
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

    fn ref_max(a: f64, b: f64) -> f64 {
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

    #[test]
    fn jmin_jmax_match_the_reference_tests_on_every_pair() {
        let nan2 = f64::from_bits(0x7ff8_0000_0000_1234);
        let vals = [
            0.0,
            -0.0,
            1.0,
            -1.0,
            1.0e-7,
            -1.0e-7,
            0.5,
            f64::MIN_POSITIVE,
            -f64::MIN_POSITIVE,
            5e-324,
            -5e-324,
            1.0e300,
            -1.0e300,
            f64::MAX,
            f64::MIN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
            -f64::NAN,
            nan2,
        ];
        for &a in &vals {
            for &b in &vals {
                assert_eq!(
                    jmin(a, b).to_bits(),
                    ref_min(a, b).to_bits(),
                    "min {a:?} {b:?}"
                );
                assert_eq!(
                    jmax(a, b).to_bits(),
                    ref_max(a, b).to_bits(),
                    "max {a:?} {b:?}"
                );
            }
        }
        // And on random bit patterns (NaNs, zeros and denormals included).
        let mut s = 0x1234_5678_9abc_def1_u64;
        for _ in 0..500_000 {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let a = f64::from_bits(s);
            let b = if s & 0x700 == 0 {
                a
            } else if s & 0x700 == 0x100 {
                f64::from_bits(s.rotate_left(17))
            } else {
                f64::from_bits(s.rotate_left(31) & !(0x7ff << 52) | ((s >> 3) & 0x7ff) << 52)
            };
            assert_eq!(jmin(a, b).to_bits(), ref_min(a, b).to_bits());
            assert_eq!(jmax(a, b).to_bits(), ref_max(a, b).to_bits());
            assert_eq!(jmin(b, a).to_bits(), ref_min(b, a).to_bits());
            assert_eq!(jmax(b, a).to_bits(), ref_max(b, a).to_bits());
        }
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

    /// The closed-form single-cell routine answers exactly what the general one does, including
    /// at the `1e-7` snapping edges, with NaN, infinities and signed zeros.
    #[test]
    fn single_cell_collide_matches_the_general_routine() {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        // Values near the cell's faces, the snapping thresholds and some awkward ones.
        let specials = [
            0.0,
            -0.0,
            1.0,
            0.5,
            1.0e-7,
            -1.0e-7,
            1.0 - 1.0e-7,
            1.0 + 1.0e-7,
            2.0e-7,
            0.3,
            0.7,
            1.3,
            -0.6,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            1.0e-14,
            5.0e-8,
        ];
        let pick = |r: u64| -> f64 {
            let base = specials[(r % specials.len() as u64) as usize];
            match (r >> 8) % 4 {
                0 => base,
                1 => base + ((r >> 16) % 7) as f64 * 1.0e-8,
                2 => base - ((r >> 16) % 7) as f64 * 1.0e-8,
                _ => ((r >> 12) % 4000) as f64 / 1000.0 - 1.0,
            }
        };
        let mut clamped = 0;
        for _ in 0..200_000 {
            let off = [
                (next() % 5) as i32 - 2,
                (next() % 5) as i32 - 2,
                (next() % 5) as i32 - 2,
            ];
            let size = [
                1.0 - (next() % 3) as f64 * 0.25,
                1.0 - (next() % 3) as f64 * 0.25,
                1.0 - (next() % 3) as f64 * 0.25,
            ];
            let boxes = [[0.0, 0.0, 0.0, size[0], size[1], size[2]]];
            let shape = Shape::from_boxes(&boxes, off[0], off[1], off[2]).expect("one box");
            let Repr::Single { min, max } = &shape.repr else {
                panic!("one box must be a single cell");
            };
            let bmin = [pick(next()), pick(next()), pick(next())];
            let bmax = [pick(next()), pick(next()), pick(next())];
            let bb = Aabb {
                min: Vec3::new(bmin[0], bmin[1], bmin[2]),
                max: Vec3::new(bmax[0], bmax[1], bmax[2]),
            };
            let d = pick(next()) * if next() & 1 == 0 { 1.0 } else { -1.0 };
            let axis = (next() % 3) as usize;
            let fast = single_collide(min, max, axis, bb, d);
            let slow = voxel_collide(&shape, axis, bb, d);
            assert_eq!(
                fast.to_bits(),
                slow.to_bits(),
                "axis {axis} d {d:?} bb {bb:?} shape {min:?} {max:?}"
            );
            if fast.to_bits() != d.to_bits() {
                clamped += 1;
            }
        }
        // The random boxes must actually run into the cell often enough for this to mean something.
        assert!(clamped > 5_000, "only {clamped} clamped moves");
    }
}
