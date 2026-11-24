"""Execute the score dialog's row sort.

ScoreDialog__WndProc calls MSVC qsort 0x007C8B48 over the entry-pointer array
0x00ABF958 with comparator 0x005C9AE0 (entry +0x60 = score) at
0x005C9D6D..0x005C9D7A, only when there is more than one row. The array starts
in the order ScoreDialog__FillEntries admitted the houses (0x005C9D51..0x005C9D66).
Each case runs the original qsort and comparator on supplied scores and records
the resulting row order as indices into that input order.
"""

from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_ESP

from tools.native_oracle import (
    RET_MAGIC, SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE,
    finish_vectors, load_image, provenance, run_checked,
)

QSORT = 0x007C8B48
COMPARATOR = 0x005C9AE0
POINTERS = 0x00ABF958
ENTRY_SIZE = 0x70
SCORE_OFFSET = 0x60
ENTRIES = SCRATCH + 0x100

CASES = (
    (10, 20), (20, 10), (5, 5), (0, 0, 0), (0, 0, 0, 0), (7,) * 8,
    (100, 50, 50, 10), (50, 100, 50, 10), (3, 1, 2, 1, 3, 2, 1, 3),
    (-5, 0, 5), (0, 900, 0), (1200, 0, 0, 0),
)


def native_order(scores):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    uc.mem_map(RET_MAGIC, 0x1000)
    pointers = []
    for index, score in enumerate(scores):
        entry = ENTRIES + index * ENTRY_SIZE
        blob = bytearray(ENTRY_SIZE)
        struct.pack_into("<i", blob, SCORE_OFFSET, score)
        uc.mem_write(entry, bytes(blob))
        pointers.append(entry)
    count = len(scores)
    uc.mem_write(POINTERS, struct.pack(f"<{count}I", *pointers))
    sp = STACK_BASE + STACK_SIZE - 0x1000
    # cdecl qsort(base, num, width, compare), then the return address.
    for value in (COMPARATOR, 4, count, POINTERS):
        sp -= 4
        uc.mem_write(sp, struct.pack("<I", value))
    sp -= 4
    uc.mem_write(sp, struct.pack("<I", RET_MAGIC))
    uc.reg_write(UC_X86_REG_ESP, sp)
    run_checked(uc, QSORT, RET_MAGIC, count=1_000_000,
                required_addresses=[COMPARATOR])
    order = struct.unpack(f"<{count}I", bytes(uc.mem_read(POINTERS, 4 * count)))
    return [(pointer - ENTRIES) // ENTRY_SIZE for pointer in order]


def generate():
    return {
        "source": "unicorn/gamemd.exe",
        "cases": [
            {"scores": list(scores), "native_order": native_order(scores)}
            for scores in CASES
        ],
    }


if __name__ == "__main__":
    finish_vectors(
        generate, Path(__file__).with_suffix(".json"),
        provenance=lambda: provenance(
            scope="The original qsort 0x007C8B48 with the score comparator 0x005C9AE0 on twelve supplied score lists of two to eight rows",
            assumptions=[
                "Entries are 0x70 bytes with the score at +0x60 and nothing else read by the comparator",
                "The pointer array starts in input order, as ScoreDialog__FillEntries leaves it",
                "Which houses get a row and their scores are established separately",
            ],
            substitutions=["Entries and the pointer array are written into scratch and 0x00ABF958 directly"],
            entry_points={"qsort": QSORT, "comparator": COMPARATOR},
        ),
    )
