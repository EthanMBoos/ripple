#!/usr/bin/env python3
"""Print a run's delivery results, one column per line.

    python3 crates/delivery/results.py runs/region-1/run000 [runs/region-1/run001 ...]

With several runs, the columns are side by side so two settings can be
compared. Reads run.mcap through reference/run.py.

    python3 crates/delivery/results.py --shrink runs/region-4/run0*

keeps each run's values in summary.json and deletes its run.mcap, since a
sweep of frame recordings fills a disk; the table reads summary.json after.
"""

from __future__ import annotations

import json
import mmap
import sys
from pathlib import Path

REPOSITORY = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY / "reference"))

from run import MAGIC, decode_summary, integer, records, string, zstd  # noqa: E402


def summary_payload(data) -> bytes:
    """The /summary message, decoding chunks from the end of the file only,
    since decoding every frame of a long run takes minutes."""
    chunks, messages, channels = [], [], {}
    offset = len(MAGIC)
    end = len(data) - len(MAGIC)
    while offset < end:
        opcode, body_at = integer(data, offset, 1)
        size, body_at = integer(data, body_at, 8)
        if opcode == 0x04:
            read_channel(data[body_at : body_at + size], channels)
        elif opcode == 0x05:
            messages.append((body_at, size))
        elif opcode == 0x06:
            chunks.append((body_at, size))
        offset = body_at + size

    def summary_in(records_seen):
        found = None
        for opcode, body in records_seen:
            if opcode == 0x04:
                read_channel(body, channels)
            elif opcode == 0x05:
                channel, _ = integer(body, 0, 2)
                if channels.get(channel) == "/summary":
                    found = body[2 + 4 + 8 + 8 :]
        return found

    def chunk_records(body_at, size):
        body = data[body_at : body_at + size]
        _, at = integer(body, 16, 8)
        at += 4
        compression, at = string(body, at)
        nested_len, at = integer(body, at, 8)
        nested = body[at : at + nested_len]
        if compression == "zstd":
            nested = zstd.decompress(nested)
        return records(nested)

    # Channels may be declared inside a chunk; look from the end.
    for body_at, size in reversed(chunks):
        if "/summary" in channels.values():
            break
        summary_in(chunk_records(body_at, size))
    found = summary_in((0x05, data[at : at + size]) for at, size in messages)
    if found is not None:
        return found
    for body_at, size in reversed(chunks):
        found = summary_in(chunk_records(body_at, size))
        if found is not None:
            return found
    raise ValueError("no /summary message in run.mcap")


def read_channel(body, channels) -> None:
    channel, at = integer(body, 0, 2)
    _, at = integer(body, at, 2)
    channels[channel], _ = string(body, at)


def results(folder: Path) -> dict:
    """The DeliveryScore values of a run, from summary.json when the recording
    has been shrunk to it (see shrink), else from run.mcap."""
    summary = folder / "summary.json"
    if summary.exists():
        return json.loads(summary.read_text())
    with open(folder / "run.mcap", "rb") as file, mmap.mmap(file.fileno(), 0, access=mmap.ACCESS_READ) as data:
        metrics, _ = decode_summary(summary_payload(data))
    (teams,) = [teams for name, teams in metrics.items() if "DeliveryScore" in name]
    (team,) = teams.values()
    return team


def shrink(folder: Path) -> None:
    """Keep a run's DeliveryScore values in summary.json and delete the frame
    recording, which is hundreds of megabytes per run. A run still being
    written (no summary yet) is left alone."""
    if (folder / "summary.json").exists() or not (folder / "run.mcap").exists():
        return
    try:
        values = results(folder)
    except Exception:  # noqa: BLE001 - a run still being written reads as anything
        return
    (folder / "summary.json").write_text(json.dumps(values, indent=1, sort_keys=True))
    (folder / "run.mcap").unlink()


def main() -> None:
    args = sys.argv[1:]
    if args and args[0] == "--shrink":
        for folder in args[1:]:
            shrink(Path(folder))
        return
    folders = [Path(arg) for arg in args]
    if not folders:
        sys.exit(__doc__)
    tables = [results(folder) for folder in folders]
    names = sorted({name for table in tables for name in table})
    width = max(len(name) for name in names)
    print(" " * width, *(f"{folder.parent.name}/{folder.name:>12}"[-24:].rjust(24) for folder in folders))
    for name in ["score"] + [n for n in names if n != "score"]:
        cells = []
        for table in tables:
            value = table.get(name)
            if value is None:
                cells.append("".rjust(24))
            elif float(value).is_integer() and abs(value) < 1e12:
                cells.append(f"{int(value)}".rjust(24))
            else:
                cells.append(f"{value:.3f}".rjust(24))
        print(name.ljust(width), *cells)


if __name__ == "__main__":
    main()
