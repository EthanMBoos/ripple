"""Reinforcement learning on Ripple: a task is one Python autonomy class.

A task (`hover.Hover`, `spread.Spread`) is a `Task`: it declares its ports,
observation and action sizes, and shipped policy file, and defines
`observe(ctx)`, what the policy sees, and `act(action)`, the port values an
action becomes. Its `step` is `act(policy(observe(ctx)))`, where the policy
is the trained network from the ONNX file, or, while training, the trainer's
next action. `ripple run` flies the task through the `Python` autonomy; the
training `Simulation` steps the same class with a `Trainer`, so the trained
and the flown code are one method. The Gymnasium and PettingZoo environments
(`hover_env`, `spread_env`) wrap `Simulation`; they need the `learning`
extra, and the training scripts in `crates/python/learning/` the `train`
extra.
"""

from ripple._native import Network, Simulation


class Task:
    """A learned autonomy. Subclasses set `ports`, `observation_size`,
    `action_size`, and `policy_file` (the shipped network) and define
    `observe(ctx)` and `act(action)`.

    `observe` must depend on `ctx` alone, not on anything kept on `self`
    between calls: while training, the `Simulation` calls it on a second
    instance of the task, built from the mission's parameters and never
    initialized, to give the environment its observations between steps,
    while the instance flying the vehicle calls it inside `step`. State kept
    in one would not be in the other."""

    def __init__(self, policy=None):
        # The network to fly; relative paths start where `ripple` runs. `None`
        # is the task's shipped policy. Unused while training.
        self.policy_path = policy

    def initialize(self, ctx):
        trainer = getattr(ctx, "trainer", None)
        if trainer is not None:
            self.policy = trainer.policy(ctx.entity.id)
        else:
            path = self.policy_file if self.policy_path is None else self.policy_path
            self.policy = Network(str(path), self.observation_size).act

    def step(self, ctx):
        return self.act(self.policy(self.observe(ctx)))


class Trainer:
    """The actions a trainer chooses, one per entity, set before each step
    and taken by that entity's autonomy during it. `Simulation` fills
    `actions`; each task's `initialize` takes its policy from `policy`."""

    def __init__(self):
        self.actions = {}

    def policy(self, entity_id):
        def act(observation):
            # Taken, not read, so a step without a new action is an error
            # rather than a silent repeat.
            try:
                action = self.actions.pop(entity_id)
            except KeyError:
                raise RuntimeError(f"the trainer set no action for entity {entity_id}") from None
            return [min(max(float(value), -1.0), 1.0) for value in action]

        return act


__all__ = ["Network", "Simulation", "Task", "Trainer"]
