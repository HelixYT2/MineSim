"""A scripted sprint-jump rollout through the default MineSim environment.

    python examples/rollout.py
"""

import numpy as np

import minesim


def main():
    env = minesim.MineSimEnv(max_episode_ticks=200)
    env.reset(seed=0)

    # Forward + jump + sprint, held for the whole episode: the optimal "move fast" policy.
    action = np.zeros(7, dtype=np.int8)
    action[[0, 4, 5]] = 1

    total_reward = 0.0
    ticks = 0
    while True:
        _obs, reward, terminated, truncated, _info = env.step(action)
        total_reward += reward
        ticks += 1
        if terminated or truncated:
            break

    print(f"distance travelled: {total_reward:.2f} blocks over {ticks} ticks")
    print(f"final state hash:   {env.state_hash():#018x}")


if __name__ == "__main__":
    main()
