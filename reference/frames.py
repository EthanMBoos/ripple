"""Read SCRIMMAGE's length-delimited proto3 Frame logs using the standard library.

Field numbers follow src/proto/scrimmage/proto/{Frame,Contact,ID,State,
Vector3d,Quaternion}.proto on Ubuntu-24.04. Only that schema's wire subset is
needed: varints, fixed64, length-delimited fields, and unknown fixed32 fields.
Validate decoded state here so invalid data cannot masquerade as a match.
"""

from dataclasses import dataclass
import math
from pathlib import Path
import struct


@dataclass
class Entity:
    id: int
    team_id: int
    group_index: int
    kind: int
    active: bool
    position: tuple
    velocity: tuple
    orientation: tuple
    angular_velocity: tuple


@dataclass
class Frame:
    time_s: float
    entities: dict


def read_varint(data, offset):
    value = 0
    for index in range(10):
        if offset >= len(data):
            raise ValueError("truncated protobuf varint")
        byte = data[offset]
        offset += 1
        if index == 9 and byte > 1:
            raise ValueError("protobuf varint exceeds 64 bits")
        value |= (byte & 0x7f) << (7 * index)
        if byte < 0x80:
            return value, offset
    raise ValueError("invalid protobuf varint")


def read_payload(data, offset, size):
    end = offset + size
    if end > len(data):
        raise ValueError("truncated protobuf payload")
    return data[offset:end], end


def fields(data):
    result = {}
    offset = 0
    while offset < len(data):
        key, offset = read_varint(data, offset)
        number, wire_type = key >> 3, key & 7
        if not 0 < number < (1 << 29):
            raise ValueError("invalid protobuf field number")
        if wire_type == 0:
            value, offset = read_varint(data, offset)
        elif wire_type in (1, 5):
            value, offset = read_payload(data, offset, 8 if wire_type == 1 else 4)
        elif wire_type == 2:
            size, offset = read_varint(data, offset)
            value, offset = read_payload(data, offset, size)
        else:
            raise ValueError(f"unsupported protobuf wire type {wire_type}")
        result.setdefault(number, []).append((wire_type, value))
    return result


def values(message, number, wire_type):
    result = []
    for actual_type, value in message.get(number, []):
        if actual_type != wire_type:
            raise ValueError(f"wrong wire type for field {number}")
        result.append(value)
    return result


def number(message, field, *, integer=False):
    items = values(message, field, 0 if integer else 1)
    if integer:
        # Protobuf int32 may be sign-extended to a ten-byte varint.
        value = items[-1] & 0xffffffff if items else 0
        return value - (1 << 32) if value >= (1 << 31) else value
    value = struct.unpack("<d", items[-1])[0] if items else 0.0
    if not math.isfinite(value):
        raise ValueError(f"nonfinite value in field {field}")
    return value


def message(message_fields, field, name):
    items = values(message_fields, field, 2)
    if not items:
        raise ValueError(f"missing {name}")
    # Repeated occurrences of a singular message merge in protobuf.
    return fields(b"".join(items))


def vector(state, field, name, dimensions=3):
    components = message(state, field, name)
    return tuple(number(components, index) for index in range(1, dimensions + 1))


def decode_frame(payload):
    frame = fields(payload)
    time_s = number(frame, 1)
    entities = {}
    for payload in values(frame, 2, 2):
        contact = fields(payload)
        identity = message(contact, 1, "contact ID")
        state = message(contact, 2, "contact state")
        entity_id = number(identity, 1, integer=True)
        if entity_id in entities:
            raise ValueError(f"duplicate entity ID {entity_id}")
        orientation = vector(state, 2, "orientation", 4)
        norm_squared = sum(value * value for value in orientation)
        if abs(norm_squared - 1.0) >= 1e-8:
            raise ValueError("orientation is not a unit quaternion")
        kind = number(contact, 3, integer=True)
        if kind not in range(5):
            raise ValueError(f"invalid contact type {kind}")
        active = values(contact, 4, 0)
        entities[entity_id] = Entity(
            id=entity_id,
            team_id=number(identity, 3, integer=True),
            group_index=number(identity, 2, integer=True),
            kind=kind,
            active=bool(active[-1]) if active else False,
            position=vector(state, 1, "position"),
            velocity=vector(state, 3, "linear velocity"),
            orientation=orientation,
            angular_velocity=vector(state, 4, "angular velocity"),
        )
    return Frame(time_s, entities)


def read_frames(path):
    path = Path(path)
    if path.suffix == ".mcap" or path.is_dir():
        from run import read_run
        return read_run(path).frames
    data = path.read_bytes()
    result = []
    offset = 0
    try:
        while offset < len(data):
            size, offset = read_varint(data, offset)
            payload, offset = read_payload(data, offset, size)
            result.append(decode_frame(payload))
    except ValueError as error:
        raise ValueError(f"{path}: frame {len(result)}: {error}") from error
    return result
