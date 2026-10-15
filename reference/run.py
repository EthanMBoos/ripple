"""Read Ripple run.mcap without the MCAP Python package.

The writer compresses MCAP chunks with zstd: `compression.zstd` is in the
standard library from Python 3.14; older versions need `pip install
backports.zstd`, which provides the same module. This reader handles the record
types Ripple writes and decodes Ripple's protobuf messages into the same Frame and
Entity objects used for SCRIMMAGE comparisons, and the summary into each metric
plugin's per-team results. It keeps each channel's payload bytes so determinism
and equivalence can compare them directly.
"""

from dataclasses import dataclass
import json
import math
from pathlib import Path
import struct

try:
    from compression import zstd
except ImportError:  # Python before 3.14
    from backports import zstd

from frames import Entity, Frame, fields, message, number, read_varint, values, vector

MAGIC = b"\x89MCAP0\r\n"
KIND = {"aircraft": 0, "quadrotor": 1, "sphere": 2}
TRACE_TOPICS = ("/beliefs", "/readings", "/deliveries", "/outputs")


@dataclass
class Run:
    frames: list
    events: list
    # {plugin: {team_id: {"score": ..., column: value}}}, as in a sweep's
    # results.jsonl; a column the plugin set no value for is absent.
    metrics: dict
    # {plugin: [column, ...]}: each plugin's declared columns, in order,
    # including any no team has a value for.
    metric_columns: dict
    scenario: dict
    traces: list
    metadata: dict
    channels: dict


def integer(data, offset, size):
    end = offset + size
    if end > len(data):
        raise ValueError("truncated MCAP field")
    return int.from_bytes(data[offset:end], "little"), end


def blob(data, offset, width=4):
    size, offset = integer(data, offset, width)
    end = offset + size
    if end > len(data):
        raise ValueError("truncated MCAP payload")
    return data[offset:end], end


def string(data, offset):
    value, offset = blob(data, offset)
    return value.decode("utf-8"), offset


def string_map(data, offset):
    body, offset = blob(data, offset)
    entries = {}
    cursor = 0
    while cursor < len(body):
        key, cursor = string(body, cursor)
        value, cursor = string(body, cursor)
        if key in entries:
            raise ValueError(f"duplicate MCAP metadata key {key}")
        entries[key] = value
    return entries, offset


def records(data):
    offset = 0
    while offset < len(data):
        opcode, offset = integer(data, offset, 1)
        body, offset = blob(data, offset, 8)
        yield opcode, body


def decode_file(data):
    if not data.startswith(MAGIC) or not data.endswith(MAGIC):
        raise ValueError("MCAP magic or footer missing")
    channels, metadata, payloads, trace_stream = {}, {}, {}, []
    footer_seen = False

    def read_record(opcode, body):
        nonlocal footer_seen
        if opcode == 0x02:
            footer_seen = True
        if opcode == 0x04:  # Channel
            channel, offset = integer(body, 0, 2)
            _, offset = integer(body, offset, 2)  # schema ID
            topic, offset = string(body, offset)
            channels[channel] = topic
        elif opcode == 0x05:  # Message
            channel, offset = integer(body, 0, 2)
            _, offset = integer(body, offset, 4)  # sequence
            _, offset = integer(body, offset, 8)  # log time
            _, offset = integer(body, offset, 8)  # publish time
            if channel not in channels:
                raise ValueError(f"message on unknown MCAP channel {channel}")
            topic = channels[channel]
            payload = body[offset:]
            payloads.setdefault(topic, []).append(payload)
            if topic in TRACE_TOPICS:
                trace_stream.append(payload)
        elif opcode == 0x06:  # Chunk
            size, offset = integer(body, 16, 8)  # after the start and end times
            offset += 4  # CRC
            compression, offset = string(body, offset)
            nested, offset = blob(body, offset, 8)
            if offset != len(body):
                raise ValueError("trailing bytes in MCAP chunk")
            if compression == "zstd":
                nested = zstd.decompress(nested)
            elif compression:
                raise ValueError(f"unsupported MCAP compression {compression}")
            if len(nested) != size:
                raise ValueError("MCAP chunk size differs from its header")
            for inner_opcode, inner_body in records(nested):
                read_record(inner_opcode, inner_body)
        elif opcode == 0x0C:  # Metadata
            name, offset = string(body, 0)
            entries, offset = string_map(body, offset)
            metadata[name] = entries

    for opcode, body in records(data[len(MAGIC):-len(MAGIC)]):
        read_record(opcode, body)
    if not footer_seen:
        raise ValueError("MCAP footer missing")
    return metadata, payloads, trace_stream


def text_field(item, field):
    raw = values(item, field, 2)
    return raw[-1].decode("utf-8") if raw else ""


def double_field(item, field):
    raw = values(item, field, 1)
    if not raw:
        return 0.0
    value = struct.unpack("<d", raw[-1])[0]
    if not math.isfinite(value):
        raise ValueError(f"nonfinite time in field {field}")
    return value


def entity_descriptions(payloads, scenario):
    groups = scenario["entities"]
    descriptions = {}
    for payload in payloads:
        item = fields(payload)
        entity_id = number(item, 1, integer=True)
        group = number(item, 3, integer=True)
        if (group < 0 or group >= len(groups) or entity_id in descriptions
                or text_field(item, 4) != groups[group]["label"]):
            raise ValueError(f"invalid description for entity {entity_id}")
        model = text_field(item, 5).lower()
        descriptions[entity_id] = (number(item, 2, integer=True), group,
                                   KIND.get(model, 3))
    return descriptions


def decode_frames(payloads, descriptions):
    frames = []
    for payload in payloads:
        item = fields(payload)
        entities = {}
        for raw in values(item, 2, 2):
            row = fields(raw)
            entity_id = number(row, 1, integer=True)
            if entity_id in entities or entity_id not in descriptions:
                raise ValueError(f"frame has duplicate or unknown entity {entity_id}")
            state = message(row, 2, "truth")
            team, group, kind = descriptions[entity_id]
            active = values(row, 3, 0)
            orientation = vector(state, 2, "orientation", 4)
            if abs(sum(part * part for part in orientation) - 1.0) >= 1e-8:
                raise ValueError("orientation is not a unit quaternion")
            entities[entity_id] = Entity(
                id=entity_id, team_id=team, group_index=group,
                kind=kind, active=bool(active[-1]) if active else False,
                position=vector(state, 1, "position"),
                orientation=orientation,
                velocity=vector(state, 3, "velocity"),
                angular_velocity=vector(state, 4, "angular velocity"))
        frames.append(Frame(double_field(item, 1), entities))
    return frames


def decode_events(payloads):
    result = []
    for payload in payloads:
        item = fields(payload)
        ids = []
        for raw in values(item, 3, 2):
            cursor = 0
            while cursor < len(raw):
                value, cursor = read_varint(raw, cursor)
                value &= 0xffffffff
                ids.append(value - (1 << 32) if value >= (1 << 31) else value)
        result.append({"time_s": double_field(item, 1), "kind": text_field(item, 2),
                       "entity_ids": ids})
    return result


def metric_number(item, field):
    # Metrics may be NaN or infinite (a zero flight window), unlike frames.
    raw = values(item, field, 1)
    return struct.unpack("<d", raw[-1])[0] if raw else 0.0


def decode_summary(payload):
    metrics, columns = {}, {}
    for raw in values(fields(payload), 1, 2):
        report = fields(raw)
        plugin, teams = text_field(report, 1), {}
        names = [name.decode("utf-8") for name in values(report, 2, 2)]
        for team_raw in values(report, 3, 2):
            team = fields(team_raw)
            team_id = number(team, 1, integer=True)
            if team_id in teams:
                raise ValueError(f"{plugin}: duplicate summary team {team_id}")
            row = {"score": metric_number(team, 2)}
            for value_raw in values(team, 3, 2):
                value = fields(value_raw)
                name = text_field(value, 1)
                if name not in names or name in row:
                    raise ValueError(f"{plugin}: undeclared or repeated column {name}")
                row[name] = metric_number(value, 2)
            teams[team_id] = row
        if plugin in metrics:
            raise ValueError(f"duplicate summary plugin {plugin}")
        metrics[plugin], columns[plugin] = teams, names
    return metrics, columns


def decode_trace(payload):
    return json.loads(payload)


def read_run(path):
    path = Path(path)
    if path.is_dir():
        path /= "run.mcap"
    metadata, channels, trace_stream = decode_file(path.read_bytes())
    run = metadata.get("ripple.run", {})
    if "scenario" not in run:
        raise ValueError(f"{path}: missing scenario metadata")
    scenario = json.loads(run["scenario"])
    descriptions = entity_descriptions(channels.get("/entities", []), scenario)
    summary = channels.get("/summary", [])
    if len(summary) != 1:
        raise ValueError(f"{path}: expected one summary")
    traces = []
    for payload in trace_stream:
        traces.append(decode_trace(payload))
    return Run(decode_frames(channels.get("/frames", []), descriptions),
               decode_events(channels.get("/events", [])),
               *decode_summary(summary[0]), scenario, traces,
               metadata.get("ripple.result", {}), channels)
