"""End-to-end checks for the Python bindings: the raw arena, determinism, checkpointing, and
Gymnasium conformance. Runs under pytest, or standalone (``python tests/test_minesim.py``)."""

import math

import numpy as np

import minesim


def test_import_and_version():
    assert isinstance(minesim.__version__, str)
    assert minesim.__version__


def _settle(arena, max_ticks=5):
    """Step with no input until the player registers contact with the ground."""
    for _ in range(max_ticks):
        arena.step()
        if arena.on_ground():
            return
    raise AssertionError("player did not settle onto the ground")


def test_sprint_jump_moves_forward():
    # yaw 0 faces +Z in Minecraft, so forward motion increases z.
    arena = minesim.Arena(surface_y=0, x=0.5, y=0.0, z=0.5, yaw=0.0)
    _settle(arena)
    assert arena.on_ground()
    start_z = arena.pos()[2]
    for _ in range(40):
        arena.step(forward=True, sprint=True, jump=True)
    end_x, _end_y, end_z = arena.pos()
    assert end_z > start_z + 5.0, f"expected forward travel, moved to z={end_z}"
    assert abs(end_x - 0.5) < 1e-6, "no lateral drift expected on a straight sprint"


def test_jump_leaves_and_returns_to_ground():
    arena = minesim.Arena(surface_y=0)
    _settle(arena)
    assert arena.on_ground()
    arena.step(jump=True)
    assert not arena.on_ground(), "should be airborne the tick after a jump"
    for _ in range(40):
        arena.step()
        if arena.on_ground():
            break
    assert arena.on_ground(), "should land again"
    assert abs(arena.pos()[1]) < 1e-9, "should come to rest exactly on the surface"


def _rollout_hashes(seed_actions):
    arena = minesim.Arena(surface_y=0)
    hashes = []
    for a in seed_actions:
        arena.step(*a)
        hashes.append(arena.state_hash())
    return hashes


def test_determinism_same_inputs_same_hashes():
    actions = [(True, False, False, False, i % 11 == 0, True, False, 0.0) for i in range(200)]
    assert _rollout_hashes(actions) == _rollout_hashes(actions)


def test_checkpoint_restore_reproduces_trajectory():
    arena = minesim.Arena(surface_y=0)
    for _ in range(15):
        arena.step(forward=True, sprint=True, jump=True)
    snapshot = arena.get_state()
    assert isinstance(snapshot, minesim.State)
    tail = [(True, False, False, False, False, True, False, 0.0) for _ in range(25)]
    expected = [(_apply(arena, a), arena.state_hash())[1] for a in tail]

    arena.set_state(snapshot)
    replay = [(_apply(arena, a), arena.state_hash())[1] for a in tail]
    assert replay == expected


def _apply(arena, a):
    arena.step(*a)


def test_gymnasium_env_checker():
    from gymnasium.utils.env_checker import check_env

    env = minesim.MineSimEnv(max_episode_ticks=50)
    check_env(env, skip_render_check=True)


def test_env_reset_is_seed_deterministic():
    env = minesim.MineSimEnv(
        max_episode_ticks=50,
        state_mutator=minesim.components.FixedSpawn(randomize_yaw=True),
    )
    o1, _ = env.reset(seed=7)
    h1 = env.state_hash()
    o2, _ = env.reset(seed=7)
    h2 = env.state_hash()
    assert np.array_equal(o1, o2)
    assert h1 == h2


def test_env_speed_reward_is_nonnegative_and_tracks_motion():
    env = minesim.MineSimEnv(max_episode_ticks=64)
    env.reset(seed=0)
    total = 0.0
    sprinting = np.array([1, 0, 0, 0, 1, 1, 0], dtype=np.int8)  # forward + jump + sprint
    for _ in range(64):
        _obs, reward, terminated, truncated, _info = env.step(sprinting)
        assert reward >= 0.0
        total += reward
        if terminated or truncated:
            break
    assert total > 5.0, f"a sprinting agent should cover ground, got {total}"


def test_vector_env_runs():
    venv = minesim.make_vec_env(num_envs=4, max_episode_ticks=20)
    venv.reset(seed=0)
    for _ in range(20):
        venv.step(venv.action_space.sample())
    venv.close()


def test_native_batch_matches_single_arena():
    n = 6
    yaws = [i * 30.0 for i in range(n)]
    batch = minesim.Batch(num_envs=n, surface_y=0)
    singles = [minesim.Arena(surface_y=0) for _ in range(n)]

    acts = np.zeros((n, 8), dtype=np.float64)
    acts[:, 0] = 1  # forward
    acts[:, 4] = 1  # jump
    acts[:, 5] = 1  # sprint
    acts[:, 7] = yaws

    for _ in range(120):
        batch.step(acts)
        for i, s in enumerate(singles):
            s.step(forward=True, jump=True, sprint=True, yaw=yaws[i])

    expected = np.array([s.state_hash() for s in singles], dtype=np.uint64)
    assert np.array_equal(batch.state_hashes(), expected)


def test_native_batch_states_shape_and_motion():
    n = 4
    batch = minesim.Batch(num_envs=n, surface_y=0)
    acts = np.zeros((n, 8))
    acts[:, 0] = 1  # forward
    acts[:, 4] = 1  # jump
    acts[:, 5] = 1  # sprint
    z0 = batch.states()[:, 2].copy()
    for _ in range(40):
        batch.step(acts)
    st = batch.states()
    assert st.shape == (n, len(minesim.Batch.STATE_COLUMNS))
    assert np.all(st[:, 2] > z0 + 5.0), "every env should advance in +z"
    assert len(batch) == n


def test_effects_change_movement():
    plain = minesim.Arena(surface_y=0)
    fast = minesim.Arena(surface_y=0)
    assert fast.add_effect("speed", 1, 1000)
    assert fast.effects() == [("minecraft:speed", 1, 1000)]
    for _ in range(40):
        plain.step(forward=True)
        fast.step(forward=True)
    assert fast.pos()[2] > plain.pos()[2] * 1.3
    jump = minesim.Arena(surface_y=0)
    jump.add_effect("minecraft:jump_boost", 2, 1000)
    _settle(jump)
    peak = 0.0
    for _ in range(30):
        jump.step(jump=True)
        peak = max(peak, jump.pos()[1])
    assert peak > 1.8, f"jump boost III should clear 1.8 blocks, peaked at {peak}"


def test_hits_hurt_and_knock_back():
    arena = minesim.Arena(surface_y=0)
    _settle(arena)
    assert arena.hurt(2.0, from_x=0.5, from_z=3.5)
    assert arena.health() == 18.0
    arena.step()
    assert arena.vel()[2] < -0.3, "pushed away from the attacker"
    assert not arena.hurt(1.0, from_x=0.5, from_z=3.5), "weaker hit inside the invulnerability window"


def test_fall_damage_and_death():
    blocks = [(0, y, 0, "minecraft:stone") for y in range(30)]
    arena = minesim.Arena(surface_y=0, blocks=blocks, x=0.5, y=30.0, z=0.5)
    _settle(arena)
    for _ in range(10):
        arena.step(forward=True)
    for _ in range(80):
        arena.step()
    assert arena.is_dead(), f"a 30-block fall is lethal, health {arena.health()}"


def test_blocks_and_climbing():
    arena = minesim.Arena(surface_y=0, x=0.5, y=0.0, z=0.5)
    for y in range(6):
        arena.set_block(0, y, 2, "minecraft:stone")
        arena.set_block(0, y, 1, "minecraft:ladder[facing=north]")
    assert arena.get_block(0, 3, 1).startswith("minecraft:ladder[facing=north")
    _settle(arena)
    for _ in range(60):
        arena.step(forward=True)
    assert arena.pos()[1] > 3.0, f"walking into a ladder climbs it, y={arena.pos()[1]}"


def test_water_slows_and_floats():
    blocks = [(x, 0, z, "minecraft:water") for x in range(-3, 4) for z in range(-3, 4)]
    arena = minesim.Arena(surface_y=0, blocks=blocks, x=0.5, y=0.0, z=0.5)
    for _ in range(20):
        arena.step()
    assert arena.in_water()
    for _ in range(20):
        arena.step(jump=True)
    assert arena.vel()[1] > 0.0 or arena.pos()[1] > 0.0


def test_arrow_hits_player():
    arena = minesim.Arena(surface_y=0)
    _settle(arena)
    arena.spawn_projectile("arrow", 0.5, 1.2, -5.5, 0.0, 0.05, 2.0)
    assert len(arena.projectiles()) == 1
    for _ in range(10):
        arena.step()
    assert arena.projectiles() == []
    assert arena.health() == 16.0


def test_native_batch_rejects_bad_action_shape():
    batch = minesim.Batch(num_envs=3)
    try:
        batch.step(np.zeros((3, 7)))
    except ValueError:
        pass
    else:
        raise AssertionError("expected a ValueError for the wrong action shape")


def _run_all():
    tests = [v for k, v in sorted(globals().items()) if k.startswith("test_") and callable(v)]
    failures = 0
    for t in tests:
        try:
            t()
            print(f"PASS  {t.__name__}")
        except Exception as exc:  # noqa: BLE001 - report-and-continue for the standalone runner
            failures += 1
            print(f"FAIL  {t.__name__}: {type(exc).__name__}: {exc}")
    print(f"\n{len(tests) - failures}/{len(tests)} passed")
    return failures


if __name__ == "__main__":
    import sys

    sys.exit(1 if _run_all() else 0)
