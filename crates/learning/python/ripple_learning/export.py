"""Save a Stable-Baselines3 PPO policy as the ONNX network a Ripple policy
plugin runs."""

import onnx
import torch


class _Mean(torch.nn.Module):
    """The policy's mean action: what it does with exploration noise off.
    The Rust plugin clamps it to [-1, 1], as SB3's `predict` does."""

    def __init__(self, policy):
        super().__init__()
        self.policy = policy

    def forward(self, observation):
        features = self.policy.extract_features(observation)
        latent = self.policy.mlp_extractor.forward_actor(features)
        return self.policy.action_net(latent)


def export_onnx(model, path):
    policy = model.policy.to("cpu").eval()
    size = model.observation_space.shape[0]
    torch.onnx.export(
        _Mean(policy).eval(),
        (torch.zeros(1, size),),
        str(path),
        input_names=["observation"],
        output_names=["action"],
        external_data=False,
    )
    # The exporter records a Python stack trace on every node, which
    # embeds the exporting machine's home directory in the shipped file.
    model = onnx.load(str(path))
    for node in model.graph.node:
        kept = [p for p in node.metadata_props if "stack_trace" not in p.key]
        del node.metadata_props[:]
        node.metadata_props.extend(kept)
        node.doc_string = ""
    onnx.save(model, str(path))
