//! Axis-aligned bounding boxes. The per-axis collision sweep that uses these against block
//! shapes lives in the kernel; this is just the box and the handful of operations it needs.

use ms_numerics::Vec3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    pub fn from_corners(x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64) -> Self {
        Self {
            min: Vec3::new(x0.min(x1), y0.min(y1), z0.min(z1)),
            max: Vec3::new(x0.max(x1), y0.max(y1), z0.max(z1)),
        }
    }

    pub fn move_by(self, d: Vec3) -> Self {
        Self {
            min: Vec3::new(self.min.x + d.x, self.min.y + d.y, self.min.z + d.z),
            max: Vec3::new(self.max.x + d.x, self.max.y + d.y, self.max.z + d.z),
        }
    }

    pub fn inflate(self, x: f64, y: f64, z: f64) -> Self {
        Self {
            min: Vec3::new(self.min.x - x, self.min.y - y, self.min.z - z),
            max: Vec3::new(self.max.x + x, self.max.y + y, self.max.z + z),
        }
    }

    /// `AABB.expandTowards`: grow the box in the direction of `d` (the min side moves for a
    /// negative component, the max side for a positive one).
    pub fn expand_towards(self, d: Vec3) -> Self {
        let mut b = self;
        if d.x < 0.0 {
            b.min.x += d.x;
        } else if d.x > 0.0 {
            b.max.x += d.x;
        }
        if d.y < 0.0 {
            b.min.y += d.y;
        } else if d.y > 0.0 {
            b.max.y += d.y;
        }
        if d.z < 0.0 {
            b.min.z += d.z;
        } else if d.z > 0.0 {
            b.max.z += d.z;
        }
        b
    }

    pub fn intersects(self, other: Aabb) -> bool {
        self.min.x < other.max.x
            && self.max.x > other.min.x
            && self.min.y < other.max.y
            && self.max.y > other.min.y
            && self.min.z < other.max.z
            && self.max.z > other.min.z
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_and_move() {
        let a = Aabb::from_corners(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        let b = Aabb::from_corners(0.5, 0.5, 0.5, 1.5, 1.5, 1.5);
        assert!(a.intersects(b));

        let shifted = a.move_by(Vec3::new(2.0, 0.0, 0.0));
        assert!(!a.intersects(shifted));
    }

    #[test]
    fn inflate_grows_both_sides() {
        let a = Aabb::from_corners(0.0, 0.0, 0.0, 1.0, 1.0, 1.0).inflate(0.5, 0.0, 0.0);
        assert_eq!(a.min.x, -0.5);
        assert_eq!(a.max.x, 1.5);
    }
}
