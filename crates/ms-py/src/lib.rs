//! Python bindings: a steppable arena exposed as the `minesim._core` extension module. The
//! Gymnasium env and the pluggable observation/action/reward components live in the pure-Python
//! `minesim` package that wraps this. There is no JDK or Minecraft dependency at runtime.

// pyo3 0.22's #[pymethods] trampoline converts an already-`PyErr` result with `Into`, which clippy
// reports as a useless conversion in code we don't author. Allow it for this binding shim only.
#![allow(clippy::useless_conversion)]

use ms_arena::{Action, Arena, BatchArena, Player};
use ms_kernel::player::Keys;
use ms_numerics::Vec3;
use ms_world::World;
use numpy::ndarray::Array2;
use numpy::{IntoPyArray, PyArray1, PyArray2, PyReadonlyArray2};
use pyo3::prelude::*;

/// The full player state, as a flat tuple: position, velocity, yaw, on-ground, jump cooldown.
type State = (f64, f64, f64, f64, f64, f64, f32, bool, i32);

#[pyclass(name = "Arena")]
struct PyArena {
    inner: Arena,
}

#[pymethods]
impl PyArena {
    /// Build an arena. With `region_dir` set, blocks come from that save's Anvil regions;
    /// otherwise the world is a flat floor of `floor_block` (default stone) with its top face at
    /// `surface_y`. The player spawns at `(x, y, z)` looking along `yaw`, at rest.
    #[new]
    #[pyo3(signature = (region_dir=None, surface_y=0, floor_block=None, x=0.5, y=0.0, z=0.5, yaw=0.0))]
    fn new(
        region_dir: Option<String>,
        surface_y: i32,
        floor_block: Option<String>,
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
    ) -> Self {
        let world = match region_dir {
            Some(dir) => World::new(dir),
            None => match floor_block {
                Some(b) => World::flat_of(surface_y, b),
                None => World::flat(surface_y),
            },
        };
        Self {
            inner: Arena::new(world, Vec3::new(x, y, z), yaw),
        }
    }

    /// Reset the player to a position/orientation, at rest. The world is unchanged.
    #[pyo3(signature = (x=0.5, y=0.0, z=0.5, yaw=0.0))]
    fn reset(&mut self, x: f64, y: f64, z: f64, yaw: f32) {
        self.inner.reset(Vec3::new(x, y, z), yaw);
    }

    /// Advance one tick from the given inputs.
    #[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
    #[pyo3(signature = (forward=false, back=false, left=false, right=false, jump=false, sprint=false, sneak=false, yaw=0.0))]
    fn step(
        &mut self,
        forward: bool,
        back: bool,
        left: bool,
        right: bool,
        jump: bool,
        sprint: bool,
        sneak: bool,
        yaw: f32,
    ) {
        self.inner.step(&Action {
            keys: Keys {
                forward,
                back,
                left,
                right,
            },
            jump,
            sprinting: sprint,
            sneaking: sneak,
            yaw,
        });
    }

    fn pos(&self) -> (f64, f64, f64) {
        let p = self.inner.player.pos;
        (p.x, p.y, p.z)
    }

    fn vel(&self) -> (f64, f64, f64) {
        let v = self.inner.player.vel;
        (v.x, v.y, v.z)
    }

    fn yaw(&self) -> f32 {
        self.inner.player.yaw
    }

    fn on_ground(&self) -> bool {
        self.inner.player.on_ground
    }

    /// Ticks remaining on the jump cooldown (0 means the player may jump).
    fn jump_cooldown(&self) -> i32 {
        self.inner.player.no_jump_delay
    }

    /// The canonical per-tick state hash (`docs/contract.md`).
    fn state_hash(&self) -> u64 {
        self.inner.state_hash()
    }

    /// The full player state, for checkpoint/restore.
    fn get_state(&self) -> State {
        let p = &self.inner.player;
        (
            p.pos.x,
            p.pos.y,
            p.pos.z,
            p.vel.x,
            p.vel.y,
            p.vel.z,
            p.yaw,
            p.on_ground,
            p.no_jump_delay,
        )
    }

    /// Restore a state previously returned by [`get_state`].
    fn set_state(&mut self, state: State) {
        self.inner.set_state(Player {
            pos: Vec3::new(state.0, state.1, state.2),
            vel: Vec3::new(state.3, state.4, state.5),
            yaw: state.6,
            on_ground: state.7,
            no_jump_delay: state.8,
        });
    }
}

/// A batch of independent flat-world arenas stepped together. The physics step runs across the
/// rayon thread pool with the GIL released, so this is the throughput path for collecting
/// reinforcement-learning rollouts from Python. Inputs and outputs are numpy arrays.
#[pyclass(name = "Batch")]
struct PyBatch {
    inner: BatchArena,
}

#[pymethods]
impl PyBatch {
    #[new]
    #[pyo3(signature = (num_envs, surface_y=0, floor_block=None, x=0.5, y=0.0, z=0.5, yaw=0.0))]
    fn new(
        num_envs: usize,
        surface_y: i32,
        floor_block: Option<String>,
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
    ) -> Self {
        let spawn = Vec3::new(x, y, z);
        let make = |_i: usize| {
            let world = match &floor_block {
                Some(b) => World::flat_of(surface_y, b.clone()),
                None => World::flat(surface_y),
            };
            Arena::new(world, spawn, yaw)
        };
        Self {
            inner: BatchArena::from_fn(num_envs, make),
        }
    }

    fn __len__(&self) -> usize {
        self.inner.len()
    }

    /// Number of environments in the batch.
    fn len(&self) -> usize {
        self.inner.len()
    }

    /// Reset every environment to the same spawn, at rest.
    #[pyo3(signature = (x=0.5, y=0.0, z=0.5, yaw=0.0))]
    fn reset_all(&mut self, x: f64, y: f64, z: f64, yaw: f32) {
        self.inner.reset_all(Vec3::new(x, y, z), yaw);
    }

    /// Advance every environment one tick. `actions` is an `(n, 8)` array whose columns are
    /// forward, back, left, right, jump, sprint, sneak (nonzero = pressed) and an absolute yaw.
    fn step(&mut self, py: Python<'_>, actions: PyReadonlyArray2<'_, f64>) -> PyResult<()> {
        let a = actions.as_array();
        let n = self.inner.len();
        if a.shape() != [n, 8] {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "actions must have shape ({n}, 8), got {:?}",
                a.shape()
            )));
        }
        let acts: Vec<Action> = (0..n)
            .map(|i| Action {
                keys: Keys {
                    forward: a[[i, 0]] != 0.0,
                    back: a[[i, 1]] != 0.0,
                    left: a[[i, 2]] != 0.0,
                    right: a[[i, 3]] != 0.0,
                },
                jump: a[[i, 4]] != 0.0,
                sprinting: a[[i, 5]] != 0.0,
                sneaking: a[[i, 6]] != 0.0,
                yaw: a[[i, 7]] as f32,
            })
            .collect();
        py.allow_threads(|| self.inner.step(&acts));
        Ok(())
    }

    /// The full state of every environment as an `(n, 9)` array: position (x, y, z), velocity
    /// (x, y, z), yaw, on-ground (0 or 1), and jump cooldown.
    fn states<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray2<f64>> {
        let n = self.inner.len();
        let mut data = Vec::with_capacity(n * 9);
        for i in 0..n {
            let p = &self.inner.arena(i).player;
            data.extend_from_slice(&[
                p.pos.x,
                p.pos.y,
                p.pos.z,
                p.vel.x,
                p.vel.y,
                p.vel.z,
                f64::from(p.yaw),
                f64::from(u8::from(p.on_ground)),
                f64::from(p.no_jump_delay),
            ]);
        }
        Array2::from_shape_vec((n, 9), data)
            .expect("state buffer is exactly n*9 elements")
            .into_pyarray_bound(py)
    }

    /// The per-environment canonical state hashes as an `(n,)` array.
    fn state_hashes<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<u64>> {
        self.inner.state_hashes().into_pyarray_bound(py)
    }
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyArena>()?;
    m.add_class::<PyBatch>()?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
