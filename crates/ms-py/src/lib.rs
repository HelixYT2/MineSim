//! Python bindings: a steppable arena exposed as the `minesim._core` extension module. The
//! Gymnasium env and the pluggable observation/action/reward components live in the pure-Python
//! `minesim` package that wraps this. There is no JDK or Minecraft dependency at runtime.

// pyo3 0.22's #[pymethods] trampoline converts an already-`PyErr` result with `Into`, which clippy
// reports as a useless conversion in code we don't author. Allow it for this binding shim only.
#![allow(clippy::useless_conversion)]

use ms_arena::{Action, Arena, BatchArena, PlayerState};
use ms_numerics::Vec3;
use ms_world::{FlatWorld, GridWorld, World};
use numpy::ndarray::Array2;
use numpy::{IntoPyArray, PyArray1, PyArray2, PyReadonlyArray2};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

/// Columns of [`PyBatch::states`], in order.
const STATE_COLUMNS: [&str; 18] = [
    "x",
    "y",
    "z",
    "vx",
    "vy",
    "vz",
    "yaw",
    "pitch",
    "on_ground",
    "jump_cooldown",
    "health",
    "food",
    "sprinting",
    "crouching",
    "in_water",
    "in_lava",
    "swimming",
    "fall_distance",
];

fn parse_state(text: &str) -> PyResult<u32> {
    ms_data::parse_state(text)
        .ok_or_else(|| PyValueError::new_err(format!("unknown block state '{text}'")))
}

/// The world described by the constructor arguments: a save, or a flat floor plus placed blocks.
fn build_world(
    region_dir: Option<String>,
    surface_y: i32,
    floor_block: Option<&str>,
    blocks: Option<Vec<(i32, i32, i32, String)>>,
) -> PyResult<World> {
    if let Some(dir) = region_dir {
        if blocks.is_some() {
            return Err(PyValueError::new_err(
                "blocks cannot be combined with region_dir",
            ));
        }
        return Ok(World::new(dir));
    }
    let floor = parse_state(floor_block.unwrap_or("minecraft:stone"))?;
    let base = FlatWorld::new(surface_y, floor);
    match blocks {
        None => Ok(World::Flat(base)),
        Some(list) => {
            let mut grid = GridWorld::new(base);
            for (x, y, z, state) in list {
                grid.set_block(x, y, z, parse_state(&state)?);
            }
            Ok(World::grid(grid))
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
fn action(
    forward: bool,
    back: bool,
    left: bool,
    right: bool,
    jump: bool,
    sprint: bool,
    sneak: bool,
    yaw: f32,
    pitch: f32,
) -> Action {
    Action {
        forward,
        back,
        left,
        right,
        jump,
        shift: sneak,
        sprint,
        yaw,
        pitch,
    }
}

fn state_row(p: &PlayerState) -> [f64; 18] {
    let b = |v: bool| f64::from(u8::from(v));
    [
        p.pos.x,
        p.pos.y,
        p.pos.z,
        p.vel.x,
        p.vel.y,
        p.vel.z,
        f64::from(p.yaw),
        f64::from(p.pitch),
        b(p.on_ground),
        f64::from(p.no_jump_delay),
        f64::from(p.health),
        f64::from(p.food),
        b(p.sprinting),
        b(p.crouching),
        b(p.in_water),
        b(p.in_lava),
        b(p.swimming),
        p.fall_distance,
    ]
}

/// An opaque snapshot of the complete simulated state, for checkpoint/restore.
#[pyclass(name = "State", module = "minesim._core")]
#[derive(Clone)]
struct PyState {
    inner: PlayerState,
}

#[pymethods]
impl PyState {
    fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }

    fn __copy__(&self) -> Self {
        self.clone()
    }
}

#[pyclass(name = "Arena", module = "minesim._core")]
struct PyArena {
    inner: Arena,
}

#[pymethods]
impl PyArena {
    /// Build an arena. With `region_dir` set, blocks come from that save's Anvil regions;
    /// otherwise the world is a flat floor of `floor_block` (default stone) with its top face at
    /// `surface_y`, plus any `blocks` given as `(x, y, z, "minecraft:block[prop=value]")` tuples.
    /// The player spawns at `(x, y, z)` looking along `yaw`, at rest.
    #[new]
    #[pyo3(signature = (region_dir=None, surface_y=0, floor_block=None, blocks=None, x=0.5, y=0.0, z=0.5, yaw=0.0))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        region_dir: Option<String>,
        surface_y: i32,
        floor_block: Option<String>,
        blocks: Option<Vec<(i32, i32, i32, String)>>,
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
    ) -> PyResult<Self> {
        let world = build_world(region_dir, surface_y, floor_block.as_deref(), blocks)?;
        Ok(Self {
            inner: Arena::new(world, Vec3::new(x, y, z), yaw),
        })
    }

    /// Reset the player to a position/orientation, at rest, with full health and food and no
    /// effects. The world is unchanged.
    #[pyo3(signature = (x=0.5, y=0.0, z=0.5, yaw=0.0, pitch=0.0))]
    fn reset(&mut self, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) {
        self.inner.reset(Vec3::new(x, y, z), yaw);
        self.inner.player.pitch = pitch;
    }

    /// Advance one game tick. The arguments are the keys held this tick and the absolute look
    /// direction; `sprint` and `sneak` are the sprint and sneak keys, so sprinting and crouching
    /// follow the game's own rules (e.g. sprinting needs forward and more than 6 food). Omitting
    /// `pitch` keeps the current one.
    #[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
    #[pyo3(signature = (forward=false, back=false, left=false, right=false, jump=false, sprint=false, sneak=false, yaw=0.0, pitch=None))]
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
        pitch: Option<f32>,
    ) {
        let pitch = pitch.unwrap_or(self.inner.player.pitch);
        self.inner.step(&action(
            forward, back, left, right, jump, sprint, sneak, yaw, pitch,
        ));
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

    fn pitch(&self) -> f32 {
        self.inner.player.pitch
    }

    fn on_ground(&self) -> bool {
        self.inner.player.on_ground
    }

    /// Ticks remaining on the jump cooldown (0 means the player may jump).
    fn jump_cooldown(&self) -> i32 {
        self.inner.player.no_jump_delay
    }

    fn health(&self) -> f32 {
        self.inner.player.health
    }

    fn food(&self) -> i32 {
        self.inner.player.food
    }

    fn is_dead(&self) -> bool {
        self.inner.is_dead()
    }

    fn sprinting(&self) -> bool {
        self.inner.player.sprinting
    }

    fn crouching(&self) -> bool {
        self.inner.player.crouching
    }

    fn in_water(&self) -> bool {
        self.inner.player.in_water
    }

    fn in_lava(&self) -> bool {
        self.inner.player.in_lava
    }

    fn swimming(&self) -> bool {
        self.inner.player.swimming
    }

    fn fall_distance(&self) -> f64 {
        self.inner.player.fall_distance
    }

    fn horizontal_collision(&self) -> bool {
        self.inner.player.horizontal_collision
    }

    /// The pose: "STANDING", "CROUCHING" or "SWIMMING".
    fn pose(&self) -> &'static str {
        self.inner.player.pose.name()
    }

    /// Active effects as `(id, amplifier, remaining_ticks)` tuples.
    fn effects(&self) -> Vec<(String, i32, i32)> {
        self.inner
            .player
            .effects
            .iter()
            .map(|e| (e.id.clone(), e.amplifier, e.duration))
            .collect()
    }

    /// Everything above as one dict (handy for observations and debugging).
    fn state_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let d = PyDict::new_bound(py);
        let row = state_row(&self.inner.player);
        for (k, v) in STATE_COLUMNS.iter().zip(row) {
            d.set_item(*k, v)?;
        }
        d.set_item("pose", self.pose())?;
        d.set_item("effects", self.effects())?;
        Ok(d)
    }

    /// Give the player a status effect, e.g. `add_effect("minecraft:speed", 1, 600)` (amplifier
    /// 1 = Speed II, 600 ticks). Returns whether it changed anything (vanilla's replacement rules).
    #[pyo3(signature = (effect, amplifier=0, duration=600))]
    fn add_effect(&mut self, effect: &str, amplifier: i32, duration: i32) -> bool {
        self.inner.add_effect(&qualify(effect), amplifier, duration)
    }

    fn remove_effect(&mut self, effect: &str) -> bool {
        self.inner.remove_effect(&qualify(effect))
    }

    fn clear_effects(&mut self) -> bool {
        self.inner.clear_effects()
    }

    /// Damage the player. With a source point `(from_x, from_z)` it is a mob attack from there,
    /// with the game's knockback away from it; without one there is no knockback. Returns whether
    /// the hit landed (the invulnerability window blocks weaker repeat hits).
    #[pyo3(signature = (amount, from_x=None, from_z=None))]
    fn hurt(&mut self, amount: f32, from_x: Option<f64>, from_z: Option<f64>) -> PyResult<bool> {
        match (from_x, from_z) {
            (Some(x), Some(z)) => Ok(self.inner.hurt_from(amount, x, z)),
            (None, None) => Ok(self.inner.hurt(amount)),
            _ => Err(PyValueError::new_err(
                "give both from_x and from_z, or neither",
            )),
        }
    }

    /// `LivingEntity.knockback(strength, dx, dz)`: a push of `strength` opposite to `(dx, dz)`.
    fn knockback(&mut self, strength: f64, dx: f64, dz: f64) {
        self.inner.knockback(strength, dx, dz);
    }

    /// Move the player (velocity reset unless given).
    #[pyo3(signature = (x, y, z, yaw=None, pitch=None, vel=(0.0, 0.0, 0.0)))]
    #[allow(clippy::too_many_arguments)]
    fn teleport(
        &mut self,
        x: f64,
        y: f64,
        z: f64,
        yaw: Option<f32>,
        pitch: Option<f32>,
        vel: (f64, f64, f64),
    ) {
        let yaw = yaw.unwrap_or(self.inner.player.yaw);
        let pitch = pitch.unwrap_or(self.inner.player.pitch);
        self.inner.teleport(
            Vec3::new(x, y, z),
            Vec3::new(vel.0, vel.1, vel.2),
            yaw,
            pitch,
        );
    }

    /// Place a block, e.g. `set_block(3, 0, 5, "minecraft:ladder[facing=north]")`.
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) -> PyResult<()> {
        let s = parse_state(state)?;
        self.inner
            .set_block(x, y, z, s)
            .map_err(PyValueError::new_err)
    }

    /// The block state at a coordinate, e.g. `"minecraft:oak_stairs[facing=east,...]"`.
    fn get_block(&self, x: i32, y: i32, z: i32) -> String {
        ms_data::state_to_string(self.inner.world().block_state(x, y, z))
    }

    /// The canonical per-tick state hash (`docs/contract.md`).
    fn state_hash(&self) -> u64 {
        self.inner.state_hash()
    }

    /// A snapshot of the complete state, for checkpoint/restore.
    fn get_state(&self) -> PyState {
        PyState {
            inner: self.inner.get_state(),
        }
    }

    /// Restore a snapshot taken with [`get_state`].
    fn set_state(&mut self, state: &PyState) {
        self.inner.set_state(state.inner.clone());
    }
}

/// Accept `"speed"` for `"minecraft:speed"`.
fn qualify(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

/// A batch of independent arenas stepped together. The physics step runs across the rayon thread
/// pool with the GIL released, so this is the throughput path for collecting reinforcement-learning
/// rollouts from Python. Inputs and outputs are numpy arrays. All environments share one world
/// (built once); an environment that edits its world gets its own copy.
#[pyclass(name = "Batch", module = "minesim._core")]
struct PyBatch {
    inner: BatchArena,
}

#[pymethods]
impl PyBatch {
    #[new]
    #[pyo3(signature = (num_envs, surface_y=0, floor_block=None, blocks=None, x=0.5, y=0.0, z=0.5, yaw=0.0))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        num_envs: usize,
        surface_y: i32,
        floor_block: Option<String>,
        blocks: Option<Vec<(i32, i32, i32, String)>>,
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
    ) -> PyResult<Self> {
        let spawn = Vec3::new(x, y, z);
        let world = build_world(None, surface_y, floor_block.as_deref(), blocks)?;
        Ok(Self {
            inner: BatchArena::from_fn(num_envs, |_| Arena::new(world.clone(), spawn, yaw)),
        })
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

    /// Reset the listed environments to a spawn, at rest.
    #[pyo3(signature = (indices, x=0.5, y=0.0, z=0.5, yaw=0.0))]
    fn reset(&mut self, indices: Vec<usize>, x: f64, y: f64, z: f64, yaw: f32) -> PyResult<()> {
        let n = self.inner.len();
        for i in indices {
            if i >= n {
                return Err(PyValueError::new_err(format!(
                    "env index {i} out of range ({n})"
                )));
            }
            self.inner.arena_mut(i).reset(Vec3::new(x, y, z), yaw);
        }
        Ok(())
    }

    /// Advance every environment one tick. `actions` is an `(n, 8)` or `(n, 9)` array whose
    /// columns are forward, back, left, right, jump, sprint, sneak (nonzero = held), the absolute
    /// yaw, and optionally the absolute pitch.
    fn step(&mut self, py: Python<'_>, actions: PyReadonlyArray2<'_, f64>) -> PyResult<()> {
        let a = actions.as_array();
        let n = self.inner.len();
        let cols = a.shape()[1];
        if a.shape()[0] != n || !(cols == 8 || cols == 9) {
            return Err(PyValueError::new_err(format!(
                "actions must have shape ({n}, 8) or ({n}, 9), got {:?}",
                a.shape()
            )));
        }
        let acts: Vec<Action> = (0..n)
            .map(|i| {
                let pitch = if cols == 9 {
                    a[[i, 8]] as f32
                } else {
                    self.inner.arena(i).player.pitch
                };
                action(
                    a[[i, 0]] != 0.0,
                    a[[i, 1]] != 0.0,
                    a[[i, 2]] != 0.0,
                    a[[i, 3]] != 0.0,
                    a[[i, 4]] != 0.0,
                    a[[i, 5]] != 0.0,
                    a[[i, 6]] != 0.0,
                    a[[i, 7]] as f32,
                    pitch,
                )
            })
            .collect();
        py.allow_threads(|| self.inner.step(&acts));
        Ok(())
    }

    /// The state of every environment as an `(n, 18)` array; the column names are
    /// `Batch.STATE_COLUMNS`.
    fn states<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray2<f64>> {
        let n = self.inner.len();
        let mut data = Vec::with_capacity(n * STATE_COLUMNS.len());
        for i in 0..n {
            data.extend_from_slice(&state_row(&self.inner.arena(i).player));
        }
        Array2::from_shape_vec((n, STATE_COLUMNS.len()), data)
            .expect("state buffer is exactly n*18 elements")
            .into_pyarray_bound(py)
    }

    #[classattr]
    #[allow(non_snake_case)]
    fn STATE_COLUMNS() -> Vec<&'static str> {
        STATE_COLUMNS.to_vec()
    }

    /// Give environment `i` a status effect.
    #[pyo3(signature = (i, effect, amplifier=0, duration=600))]
    fn add_effect(
        &mut self,
        i: usize,
        effect: &str,
        amplifier: i32,
        duration: i32,
    ) -> PyResult<bool> {
        self.check(i)?;
        Ok(self
            .inner
            .arena_mut(i)
            .add_effect(&qualify(effect), amplifier, duration))
    }

    /// Damage environment `i`'s player (from a point, with knockback, if given).
    #[pyo3(signature = (i, amount, from_x=None, from_z=None))]
    fn hurt(
        &mut self,
        i: usize,
        amount: f32,
        from_x: Option<f64>,
        from_z: Option<f64>,
    ) -> PyResult<bool> {
        self.check(i)?;
        let a = self.inner.arena_mut(i);
        match (from_x, from_z) {
            (Some(x), Some(z)) => Ok(a.hurt_from(amount, x, z)),
            (None, None) => Ok(a.hurt(amount)),
            _ => Err(PyValueError::new_err(
                "give both from_x and from_z, or neither",
            )),
        }
    }

    /// The per-environment canonical state hashes as an `(n,)` array.
    fn state_hashes<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<u64>> {
        self.inner.state_hashes().into_pyarray_bound(py)
    }
}

impl PyBatch {
    fn check(&self, i: usize) -> PyResult<()> {
        if i < self.inner.len() {
            Ok(())
        } else {
            Err(PyValueError::new_err(format!(
                "env index {i} out of range ({})",
                self.inner.len()
            )))
        }
    }
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyArena>()?;
    m.add_class::<PyBatch>()?;
    m.add_class::<PyState>()?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
