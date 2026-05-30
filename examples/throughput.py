"""Measure native batched-stepping throughput from Python.

    python examples/throughput.py

The physics step runs across the rayon thread pool with the GIL released, so a single Python
process drives many thousands of independent environments.
"""

import time

import numpy as np

import minesim


def main(num_envs=8192, ticks=500):
    batch = minesim.Batch(num_envs=num_envs, surface_y=0)

    actions = np.zeros((num_envs, 8))
    actions[:, 0] = 1  # forward
    actions[:, 4] = 1  # jump
    actions[:, 5] = 1  # sprint

    for _ in range(10):  # warm up the thread pool
        batch.step(actions)

    start = time.perf_counter()
    for _ in range(ticks):
        batch.step(actions)
    elapsed = time.perf_counter() - start

    rate = num_envs * ticks / elapsed
    print(f"{num_envs} envs x {ticks} ticks in {elapsed:.3f}s")
    print(f"{rate / 1e6:.1f} M env-ticks/s  ({rate / 20 / 1e6:.1f}M x real-time aggregate)")


if __name__ == "__main__":
    main()
