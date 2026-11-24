"""Compare retail CRT rand arithmetic under explicit thread-context seeds.

This does not establish the live seed at any shell checkpoint. Native startup
initializes TLS context+0x14 to 1 (0x007D13F8), but other consumers share it.
The original rand body starts at 0x007CB4AA with a TLS accessor call; this probe
supplies its returned pointer and executes 0x007CB4AF through the RET boundary.
"""

from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_EAX

from tools.native_oracle import SCRATCH, SCRATCH_SIZE, load_image, run_checked
from tools.native_oracle import finish_vectors, provenance

BEGIN = 0x007CB4AF
END = 0x007CB4CB
SEEDS = (0, 1, 0x7FFF, 0x8000, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF)


def generate():
    cases = []
    for seed in SEEDS:
        uc = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(uc)
        uc.mem_map(SCRATCH, SCRATCH_SIZE)
        guard = bytes([0xA5]) * 0x14 + struct.pack("<I", seed) + bytes([0x5A]) * 8
        uc.mem_write(SCRATCH, guard)
        draws = []
        for _ in range(32):
            uc.reg_write(UC_X86_REG_EAX, SCRATCH)
            run_checked(uc, BEGIN, END, count=100, required_addresses=[0x007CB4BE])
            after = bytes(uc.mem_read(SCRATCH, len(guard)))
            if after[:0x14] != guard[:0x14] or after[0x18:] != guard[0x18:]:
                raise RuntimeError("CRT rand wrote outside context seed")
            draws.append({"value": uc.reg_read(UC_X86_REG_EAX),
                          "state": struct.unpack_from("<I", after, 0x14)[0]})
        cases.append({"seed": seed, "draws": draws})
    return {"source": "unicorn/gamemd.exe", "cases": cases}


if __name__ == "__main__":
    finish_vectors(
        generate, Path(__file__).with_suffix(".json"),
        provenance=lambda: provenance(
            scope="Seven supplied seeds, 32 sequential CRT draws each; arithmetic and seed write extent only",
            assumptions=[
                "EAX is the valid context pointer returned by native TLS accessor 0x007D140B",
                "Each case has a fresh context; state is retained across its 32 draws",
                "No startup route, TLS allocation, consumer scheduling or live Save seed is certified",
            ],
            substitutions=["TLS accessor call bypassed by supplying its returned context pointer"],
            entry_points={"rand_arithmetic": BEGIN, "ret_boundary": END},
        ),
    )
