//! Stepping many independent arenas at once.
//!
//! Each arena owns its world and player and shares nothing with the others, so a batch advances
//! in parallel with no synchronisation and no cross-env RNG — the parallel result is identical,
//! env for env, to stepping them serially. This is the throughput path for reinforcement-learning
//! rollout collection.

use crate::{Action, Arena};
use rayon::prelude::*;

pub struct BatchArena {
    arenas: Vec<Arena>,
}

impl BatchArena {
    /// Build `n` arenas, the `i`th from `make(i)`.
    pub fn from_fn(n: usize, make: impl Fn(usize) -> Arena) -> Self {
        Self {
            arenas: (0..n).map(make).collect(),
        }
    }

    /// Take ownership of a set of pre-built arenas.
    pub fn new(arenas: Vec<Arena>) -> Self {
        Self { arenas }
    }

    /// Reset every arena's player to the same spawn, at rest.
    pub fn reset_all(&mut self, pos: ms_numerics::Vec3, yaw: f32) {
        for arena in &mut self.arenas {
            arena.reset(pos, yaw);
        }
    }

    pub fn len(&self) -> usize {
        self.arenas.len()
    }

    pub fn is_empty(&self) -> bool {
        self.arenas.is_empty()
    }

    pub fn arenas(&self) -> &[Arena] {
        &self.arenas
    }

    pub fn arena(&self, i: usize) -> &Arena {
        &self.arenas[i]
    }

    /// Advance every arena by one tick from its matching action, across the rayon thread pool.
    pub fn step(&mut self, actions: &[Action]) {
        assert_eq!(
            actions.len(),
            self.arenas.len(),
            "one action per arena required"
        );
        self.arenas
            .par_iter_mut()
            .zip(actions.par_iter())
            .for_each(|(arena, action)| arena.step(action));
    }

    /// Advance every arena by one tick on the current thread. The reference path that [`step`]
    /// must match hash-for-hash.
    ///
    /// [`step`]: Self::step
    pub fn step_serial(&mut self, actions: &[Action]) {
        assert_eq!(
            actions.len(),
            self.arenas.len(),
            "one action per arena required"
        );
        for (arena, action) in self.arenas.iter_mut().zip(actions) {
            arena.step(action);
        }
    }

    /// The per-arena canonical state hashes, in order.
    pub fn state_hashes(&self) -> Vec<u64> {
        self.arenas.iter().map(Arena::state_hash).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ms_kernel::player::Keys;
    use ms_numerics::Vec3;
    use ms_world::World;

    fn sprint_forward(yaw: f32) -> Action {
        Action {
            keys: Keys {
                forward: true,
                back: false,
                left: false,
                right: false,
            },
            jump: true,
            sprinting: true,
            sneaking: false,
            yaw,
        }
    }

    fn spawn(i: usize) -> Arena {
        Arena::new(World::flat(0), Vec3::new(0.5, 0.0, 0.5), i as f32 * 10.0)
    }

    #[test]
    fn parallel_matches_serial() {
        let n = 64;
        let actions: Vec<Action> = (0..n).map(|i| sprint_forward(i as f32 * 10.0)).collect();

        let mut par = BatchArena::from_fn(n, spawn);
        let mut ser = BatchArena::from_fn(n, spawn);
        for _ in 0..300 {
            par.step(&actions);
            ser.step_serial(&actions);
            assert_eq!(par.state_hashes(), ser.state_hashes());
        }
    }

    #[test]
    fn distinct_headings_stay_distinct() {
        let mut batch = BatchArena::from_fn(2, |i| {
            Arena::new(
                World::flat(0),
                Vec3::new(0.5, 0.0, 0.5),
                if i == 0 { 0.0 } else { 90.0 },
            )
        });
        let actions = vec![sprint_forward(0.0), sprint_forward(90.0)];
        for _ in 0..50 {
            batch.step(&actions);
        }
        let h = batch.state_hashes();
        assert_ne!(h[0], h[1], "envs with different yaw should diverge");
    }
}
