"""Train a PPO agent on the default MineSim task with Stable-Baselines3.

Requires the optional RL dependencies:

    pip install "minesim[gym]" stable-baselines3

The default task rewards horizontal distance covered each tick, so a trained agent converges on
sprint-jumping in a straight line.
"""

from stable_baselines3 import PPO
from stable_baselines3.common.env_util import make_vec_env

import minesim


def main():
    venv = make_vec_env(lambda: minesim.MineSimEnv(max_episode_ticks=200), n_envs=8)
    model = PPO("MlpPolicy", venv, n_steps=256, verbose=1)
    model.learn(total_timesteps=100_000)
    model.save("minesim_ppo")


if __name__ == "__main__":
    main()
