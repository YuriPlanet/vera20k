"""Execute original RebuildZoneConnectivity's bridge-record consumer region."""

import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_EBP, UC_X86_REG_ESP
from tools.native_oracle import STACK_BASE, STACK_SIZE, finish_vectors, load_image, provenance, run_checked
from tools.spatial_oracle.bridge_records import cases, execute, MAP, RECORDS
from tools.spatial_oracle.map_queries import dwords, packed

BEGIN, END = 0x0056C6CB, 0x0056C7E6
NODES, BUCKETS, ROOT, DATA = 0x00BA0000, 0x00BB0000, 0x00BBF000, 0x00BC0000


def consume(name, size, records):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.reg_write(UC_X86_REG_ESP, STACK_BASE + STACK_SIZE - 0x1000)
    uc.reg_write(UC_X86_REG_EBP, MAP)
    width = size[0] + size[1]
    side = width + 1
    zones = [(x * 17 + y * 7) % 31 for y in range(width) for x in range(width)]
    nodes = bytearray(side * side * 4)
    for y in range(side):
        for x in range(side):
            zone = zones[y * width + x] if x < width and y < width else 0
            struct.pack_into("<H", nodes, (y * side + x) * 4 + 2, zone)
    uc.mem_write(NODES, bytes(nodes))
    uc.mem_write(MAP + 0x68, dwords(NODES, side * side))
    uc.mem_write(MAP + 0xF4, dwords(*size))
    uc.mem_write(MAP + 0x54, dwords(RECORDS))
    uc.mem_write(MAP + 0x60, dwords(len(records)))
    for index, record in enumerate(records):
        uc.mem_write(RECORDS + index * 16, packed(*record["a"]) + packed(*record["b"])
                     + dwords(int(record["active"]), record["kind"]))
    uc.mem_write(MAP + 0x14, dwords(ROOT))
    uc.mem_write(ROOT, dwords(BUCKETS))
    for i in range(256):
        uc.mem_write(BUCKETS + i * 24, dwords(0, DATA + i * 256, 32, 0, 0, 0))
    run_checked(uc, BEGIN, END, count=200000)
    pairs = []
    for i in range(256):
        count = struct.unpack("<I", uc.mem_read(BUCKETS + i * 24 + 16, 4))[0]
        if count > 32:
            raise RuntimeError("bucket overflow")
        for j in range(count):
            a, b = struct.unpack("<II", uc.mem_read(DATA + i * 256 + j * 8, 8))
            if a != b:
                raise RuntimeError("native duplicate edge words disagree")
            pairs.append([a >> 16, a & 0xFFFF])
    return dict(name=name, size=size, width=width, zones=zones, records=records, pairs=pairs)


def generate():
    rows = []
    for case in cases():
        result = execute(case)
        rows.append(consume(case["name"], case["size"], result["records"]))
    record = lambda a, b, active=True: dict(a=a, b=b, active=active, kind=1)
    rows.append(consume("consumer_signed_clamps_and_aliases", (8, 8), [
        record((5, 5), (32767, -32768)), record((5, 5), (32767, 32767)),
        record((6, 5), (-1, 1)), record((7, 5), (17, 0)), record((8, 5), (16, 0)),
        record((0, 0), (5, 5)), record((5, 5), (5, 5)), record((5, 5), (8, 8), False),
    ]))
    # Select supplied base-zone coordinates, not expected consumer outputs.
    coords = {zone: (x, y) for y in range(16) for x in range(16)
              if (zone := (x * 17 + y * 7) % 31) in (1, 2, 17, 18)}
    row = consume("consumer_reverse_duplicates", (8, 8), [
        record(coords[1], coords[2]), record(coords[17], coords[18]),
        record(coords[2], coords[1]),
    ])
    # Distinct pairs 1/2 and 17/18 share bucket 0x12. The later duplicate
    # becomes the first insertion during reverse traversal.
    assert row["pairs"] == [[1, 2], [17, 18]]
    assert len({((a & 15) << 4) | (b & 15) for a, b in row["pairs"]}) == 1
    rows.append(row)
    rows.append(consume("consumer_reverse_collision", (8, 8), [
        record(coords[1], coords[2]), record(coords[17], coords[18]),
    ]))
    assert rows[-1]["pairs"] == [[17, 18], [1, 2]]
    return {"cases": rows}


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope="Original bridge-record consumer56C6CB..56C7E6: signed clamp, reverse order, canonical deduplicated bucket pairs",
        assumptions=["Original producer outputs reused as inputs for83 cases plus three explicit consumer fixtures",
                     "Base-zone words supplied independently of flood fill; padding node words0",
                     "Native node side W+H+1 and count side squared; Rust projection width W+H",
                     "Each native bucket has32 preallocated entries, excluding allocation/failure",
                     "Starts after native flood-fill and stops before adjacency allocation and MovementZone projection"],
        substitutions=[], entry_points={"consumer_begin": BEGIN, "consumer_end": END}))
