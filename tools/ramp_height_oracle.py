"""Run bounded ground-height setter/placement cases in the original gamemd image.

No downloads, live debugger, native code patches, or behavioral hooks. The fixture
supplies already-initialized map cells and the standard 104/416-lepton constants;
the original 578080 lookup, 47B3A0 slope evaluator, 5F5FA0 setter, 5F5F40 getter,
and Unit/Infantry type placement adjusters execute. IsMarked is false, matching
ordinary same-cell movement's suppressed-mark setter call. Whole locomotor loops,
world-entry callbacks, occupancy, and rendering are not emulated.
"""

from __future__ import annotations

import hashlib
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, __version__ as unicorn_version
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EBX,
    UC_X86_REG_ESP, UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_EDI,
    UC_X86_REG_EFLAGS, UC_X86_REG_FPCW,
)

from tools.native_oracle import (
    NATIVE_FPCW, OracleError, load_image, run_checked,
    finish_vectors, provenance,
)


MEM = 0x21000000
CELL_TABLE = MEM
CELL = MEM + 0x100000
OBJECT = MEM + 0x101000
INPUT = MEM + 0x102000
OUTPUT = MEM + 0x103000
STACK = MEM + 0x1FFFF0
RETURN = 0x30000000
DUMMY = 0xABDC50
REGIONS = {
    "set_height": (0x5F5FA0, 0x5F6052),
    "get_height": (0x5F5F40, 0x5F5FA0),
    "ground_lookup": (0x578080, 0x578100),
    "ground_evaluator": (0x47B3A0, 0x47BB5B),
    "unit_type_adjust": (0x747EB0, 0x747F11),
    "infantry_type_adjust": (0x5247D0, 0x524831),
}


def signed(value: int) -> int:
    return ((value + 0x80000000) & 0xFFFFFFFF) - 0x80000000


class NativeFixture:
    def __init__(self) -> None:
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.uc)
        self.uc.mem_map(MEM, 0x200000)
        self.uc.mem_map(RETURN, 0x1000)
        self.region_hashes = {
            name: hashlib.sha256(bytes(self.uc.mem_read(start, end - start))).hexdigest()
            for name, (start, end) in REGIONS.items()
        }
        self.put32(0x87F924, CELL_TABLE)
        self.put32(0x87F928, 512 * 512)
        self.put32(0x89E7C0, 104)  # initialized Cell ground LevelHeight
        self.put32(0xAC13BC, 416)  # initialized Object SetHeight/GetHeight deck delta
        self.uc.mem_write(0x822D80, struct.pack("<H", NATIVE_FPCW))
        # These must be the actual Unit/Infantry type +0x6C owners, also under -O.
        if self.get32(0x7F6284) != 0x747EB0 or self.get32(0x7EB67C) != 0x5247D0:
            raise OracleError("Unexpected Unit/Infantry placement vtable entries")

    def put32(self, address: int, value: int) -> None:
        self.uc.mem_write(address, struct.pack("<I", value & 0xFFFFFFFF))

    def get32(self, address: int) -> int:
        return struct.unpack("<i", self.uc.mem_read(address, 4))[0]

    def call(self, address: int, *args: int, receiver: int = OBJECT,
             required_addresses=()) -> int:
        self.uc.mem_write(STACK - 0x1000, bytes(0x1000))
        for index, value in enumerate([RETURN, *args]):
            self.put32(STACK + index * 4, value)
        for register in [UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EBX,
                         UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_EDI]:
            self.uc.reg_write(register, 0)
        self.uc.reg_write(UC_X86_REG_ESP, STACK)
        self.uc.reg_write(UC_X86_REG_ECX, receiver)
        self.uc.reg_write(UC_X86_REG_EFLAGS, 2)
        self.uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        run_checked(self.uc, address, RETURN, count=100000,
                    required_addresses=required_addresses)
        return signed(self.uc.reg_read(UC_X86_REG_EAX))

    def execute(self, case: dict) -> dict:
        self.uc.mem_write(CELL_TABLE, bytes(0x100000))
        for address in [CELL, DUMMY]:
            self.uc.mem_write(address, bytes(0x148))
            self.put32(address, 0x7E4EEC)
            self.uc.mem_write(address + 0x24, struct.pack("<hh", *case["stored_cell"]))
            self.uc.mem_write(address + 0x11B,
                              bytes([case["level"] & 255, case["ramp"]]))
        if not case["missing"]:
            self.put32(CELL_TABLE + case["cell_slot"] * 4, CELL)
        self.uc.mem_write(OBJECT, bytes(0x1000))
        self.put32(OBJECT, 0x7F5C70)
        self.uc.mem_write(OBJECT + 0x8C, bytes([case["on_bridge"]]))
        self.uc.mem_write(OBJECT + 0x9C, struct.pack("<iii", *case["coord"]))
        self.uc.mem_write(INPUT, struct.pack("<iii", *case["coord"]))
        ground = self.call(0x578080, INPUT, receiver=0x87F7E8)
        self.call(0x5F5FA0, case["requested_height"],
                  required_addresses=(0x578080, 0x47B3A0))
        raw_z = self.get32(OBJECT + 0xA4)
        above_surface = self.call(0x5F5F40)
        adjusted = {}
        for name, address in [("unit", 0x747EB0), ("infantry", 0x5247D0)]:
            self.call(address, OUTPUT, INPUT)
            adjusted[name] = list(struct.unpack("<iii", self.uc.mem_read(OUTPUT, 12)))
        return {
            "ground_z": ground,
            "set_height_raw_z": raw_z,
            "get_height_after_set": above_surface,
            "placement": adjusted,
            "dummy_coord": list(struct.unpack("<hh", self.uc.mem_read(DUMMY + 0x24, 4))),
        }


def fixtures() -> list[dict]:
    cases: list[dict] = []

    def add(name: str, *, xy=(640, 640), input_z=0, level=0, ramp=0,
            on_bridge=False, requested_height=0, missing=False,
            stored_cell=(2, 2), cell_slot=1026) -> None:
        cases.append(dict(name=name, coord=[*xy, input_z], level=level, ramp=ramp,
                          on_bridge=on_bridge, requested_height=requested_height,
                          missing=missing, stored_cell=list(stored_cell), cell_slot=cell_slot))

    for ramp in range(21):
        for x, y in [(0, 0), (1, 255), (64, 192), (128, 128), (255, 1), (255, 255)]:
            add(f"ramp_{ramp}_sub_{x}_{y}", xy=(512+x, 512+y), ramp=ramp)
    for level in [-128, -4, -1, 0, 5, 127]:
        for bridge in [False, True]:
            add(f"signed_level_{level}_bridge_{int(bridge)}", level=level, ramp=1,
                on_bridge=bridge)
    for height in [-417, -1, 0, 1, 37, 2147483647, -2147483648]:
        add(f"requested_{height}", level=5, ramp=9, on_bridge=True, requested_height=height)
    for input_z in [-2000, -1, 0, 51, 52, 53, 468, 900, 2147483647]:
        add(f"placement_input_{input_z}", ramp=1, input_z=input_z)
    add("missing_positive", level=-4, ramp=3, missing=True)
    add("negative_xy_divides_toward_zero", xy=(-1, 640), level=5, ramp=2,
        stored_cell=(0, 2), cell_slot=1024)
    add("negative_xy_dummy", xy=(-257, 640), level=5, ramp=2, missing=True)
    add("fixed_slot_alias", xy=(-256, 256), level=3, ramp=6,
        stored_cell=(511, 0), cell_slot=511)
    return cases


def result() -> dict:
    fixture = NativeFixture()
    return {
        "schema": 1,
        "unicorn_version": unicorn_version,
        "native_fpcw": NATIVE_FPCW,
        "region_sha256": fixture.region_hashes,
        "fixture_constants": {"ground_level_leptons": 104, "bridge_deck_leptons": 416},
        "coverage": "Unmarked setter/getter and type placement leaves; not whole locomotor or rendering parity.",
        "cases": [case | {"native": fixture.execute(case)} for case in fixtures()],
    }


def metadata() -> dict:
    return provenance(
        scope='Unmarked ground-height setter/getter and type placement leaves; not whole locomotor or rendering parity.',
        assumptions=[
            'Supplied initialized map cells and fixed 512-wide cell slots; shared dummy state follows original calls.',
            'Ground LevelHeight=104 and bridge deck delta=416 leptons are supplied startup constants.',
            'Live and cached x87 control words=0x0E7F; IsMarked=false suppresses world-entry/occupation callbacks.',
        ],
        substitutions=[],
        entry_points={name: bounds[0] for name, bounds in REGIONS.items()},
    )


def main(argv=None) -> None:
    finish_vectors(result, Path(__file__).with_name('ramp_height_vectors.json'),
                   provenance=metadata, argv=argv)


if __name__ == "__main__":
    main()
