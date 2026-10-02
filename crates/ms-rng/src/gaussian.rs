//! The Marsaglia polar method shared by every `nextGaussian` in the game and the JDK.

/// State of `MarsagliaPolarGaussian` (and of `java.util.Random`'s cached gaussian): the polar
/// method yields two normal deviates per round and keeps the second for the next call.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MarsagliaPolarGaussian {
    next_next: f64,
    have_next_next: bool,
}

impl MarsagliaPolarGaussian {
    pub const fn new() -> Self {
        Self {
            next_next: 0.0,
            have_next_next: false,
        }
    }

    /// Drops a pending cached value; `setSeed` does this.
    pub fn reset(&mut self) {
        self.have_next_next = false;
    }

    /// Whether the next call returns the cached second deviate without drawing.
    pub fn has_cached(&self) -> bool {
        self.have_next_next
    }

    /// One deviate, drawing uniform doubles from `next_double` and using `log` for the natural
    /// logarithm. The square root is `Math.sqrt` (exact in IEEE arithmetic).
    ///
    /// The game's `MarsagliaPolarGaussian` calls `Math.log` (see [`ms_numerics::hotspot`]);
    /// `java.util.Random` calls `StrictMath.log` (see [`ms_numerics::fdlibm`]).
    pub fn next_gaussian(
        &mut self,
        mut next_double: impl FnMut() -> f64,
        log: fn(f64) -> f64,
    ) -> f64 {
        if self.have_next_next {
            self.have_next_next = false;
            return self.next_next;
        }
        loop {
            let d = 2.0 * next_double() - 1.0;
            let e = 2.0 * next_double() - 1.0;
            let f = d * d + e * e;
            if f >= 1.0 || f == 0.0 {
                continue;
            }
            let g = (-2.0 * log(f) / f).sqrt();
            self.next_next = e * g;
            self.have_next_next = true;
            return d * g;
        }
    }
}
