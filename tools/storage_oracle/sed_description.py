"""Execute the original SED description reader with a supplied INI lookup fixture.

The fresh-file MapSeed callers construct and parse an INI before their first
Description lookup (597A30 and 597D60). The fixture provides the resulting lookup
indexes and empty section-pointer cache; the file parser itself is not emulated.
Reader 528F00, CRC, copy, trim, strtok and sscanf execute original instructions.
Only the CRT thread-storage accessor is substituted, for strtok state.

The output allocation deliberately has room for 128 units plus a terminator.
Native MapSeed callers do not: at 128 decoded units they corrupt adjacent memory.
This probe demonstrates the reader output, not that corrupt caller execution.
"""

import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP,
)
from tools.native_oracle import (
    load_image, run_checked, call, SCRATCH, STACK_BASE, STACK_SIZE, RET_MAGIC,
    finish_vectors, provenance,
)

READER = 0x00528F00
CRC = 0x004A1DE0
TLS_ACCESSOR = 0x007D140B
SECTION_NAME = 0x0082BB24
KEY_NAME = 0x0081B1A4


def native_crc(value):
    return call(
        CRC, ecx=SCRATCH, stack_args=[SCRATCH + 0x100, len(value)],
        writes={SCRATCH: bytes(16), SCRATCH + 0x100: value},
        timeout_instr=3000,
    )["eax"]


def reader(value, cached=False, count=128, presence="entry"):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, 0x20000)
    uc.mem_map(RET_MAGIC, 0x1000)
    ini, section, entry = SCRATCH, SCRATCH + 0x100, SCRATCH + 0x200
    sections, entries = SCRATCH + 0x300, SCRATCH + 0x400
    raw, out = SCRATCH + 0x1000, SCRATCH + 0x10000
    default, tls = SCRATCH + 0x11000, SCRATCH + 0x12000

    def dword(address, number):
        uc.mem_write(address, struct.pack("<I", number))

    dword(ini + 4, SECTION_NAME if cached else 0)
    dword(ini + 8, section if cached else 0)
    dword(ini + 0x28, sections)
    dword(ini + 0x2C, int(presence != "none"))
    dword(ini + 0x34, 1)
    dword(ini + 0x38, sections)
    dword(sections, native_crc(b"RandomMap"))
    dword(sections + 4, section)
    dword(section + 0x2C, entries)
    dword(section + 0x30, int(presence == "entry"))
    dword(section + 0x38, 1)
    dword(section + 0x3C, entries)
    dword(entries, native_crc(b"Description"))
    dword(entries + 4, entry)
    dword(entry + 0x10, raw)
    uc.mem_write(raw, value + b"\0")
    uc.mem_write(out - 2, b"\xa5" * 514)
    uc.mem_write(default, "DEFAULT\0".encode("utf-16le"))
    sp = STACK_BASE + STACK_SIZE - 0x1000
    uc.mem_write(sp - 0x6000, b"\x5a" * 0x6000)
    for argument in reversed([SECTION_NAME, KEY_NAME, default, out, count]):
        sp -= 4
        dword(sp, argument)
    sp -= 4
    dword(sp, RET_MAGIC)
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_ECX, ini)

    def thread_storage_hook(emulator, address, size, data):
        if address == TLS_ACCESSOR:
            # Supply only CRT per-thread storage; original strtok/scanner run.
            esp = emulator.reg_read(UC_X86_REG_ESP)
            ret = struct.unpack("<I", emulator.mem_read(esp, 4))[0]
            emulator.reg_write(UC_X86_REG_EAX, tls)
            emulator.reg_write(UC_X86_REG_ESP, esp + 4)
            emulator.reg_write(UC_X86_REG_EIP, ret)

    uc.hook_add(UC_HOOK_CODE, thread_storage_hook)
    run_checked(uc, READER, RET_MAGIC, count=3_000_000, timeout_us=10_000_000)
    units = list(struct.unpack("<132H", uc.mem_read(out, 264)))
    if uc.mem_read(out - 2, 2) != b"\xa5\xa5" or units[129:] != [0xA5A5] * 3:
        raise RuntimeError("Reader wrote outside supplied capacity plus terminator")
    if uc.reg_read(UC_X86_REG_ESP) != sp + 24:
        raise RuntimeError("Reader stack cleanup differs from the five-argument call")
    length = uc.reg_read(UC_X86_REG_EAX)
    if length != units.index(0):
        raise RuntimeError("Native returned length differs from first wide NUL")
    return {
        "raw": value.decode("ascii") if presence == "entry" else None,
        "presence": presence,
        "cached_section_pointer": cached,
        "count": count,
        "returned_length": length,
        "visible_units": units[:length],
    }


CASES = (
    ("ordinary", b"52,61,6e,64,6f,6d,20,4d,61,70,"),
    ("first_failed", b"z"), ("later_failed", b"41,z,42"),
    ("only_commas", b",,,"), ("empty", b""), ("blank", b" \t\r\n"),
    ("empty_tokens", b"41,,42,"), ("nul", b"0,41"),
    ("narrowed_nul", b"10000,41"), ("prefix", b"0x41,+42,-1"),
    ("invalid_prefix", b"0xg,41,0x"), ("trailing_junk", b"41z,42stuff"),
    ("overflow", b"FFFFFFFFF,100000000,123456789"),
    ("scanner_space", b"41, \t\v\f\r\n42"),
    ("non_scanner_space", b"41,\x1f42"),
    ("source_nul", b"41,\x0042"),
    ("surrogate_pair", b"d83d,de00"), ("unpaired_surrogate", b"d83d,41"),
    ("saved_limit", b"41,"*79), ("below_capacity", b"41,"*127),
    ("capacity", b"41,"*128), ("over_capacity", b"41,"*129),
    ("all_failed", b"z,z,z"), ("zero_prefix", b"00x41,0x0"),
    ("sign_failure", b"+,-,+0x,-0x"),
)


def generate():
    cases=[]
    for name, raw in CASES:
        cases.append({"name":name, **reader(raw)})
    cases.append({"name":"source_cutoff", **reader(b"41" + b","*(0x4fff-2) + b"42")})
    cases.append({"name":"missing_section", **reader(b"", presence="none")})
    cases.append({"name":"missing_key", **reader(b"", presence="section_only")})
    # Diagnostic only: this proves why the first-failure rule is caller-scoped.
    cases.append({"name":"cached_section_diagnostic", **reader(b"z", True)})
    return {"source":"unicorn/gamemd.exe", "cases":cases}


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
        provenance=lambda: provenance(
            scope="28 cold-cache description fixtures and one cached-section diagnostic; original reader with supplied INI indexes",
            assumptions=[
                "INI lookup fixture has at most one RandomMap section and one Description entry; file parsing is not executed",
                "Cold section-pointer cache models fresh INI construction/loading at MapSeed 597A30 and 597D60",
                "Cached diagnostic supplies prior stack pattern 0x5A and is excluded from Rust comparison",
                "Default is DEFAULT; output includes spare allocation beyond 128-unit count to observe terminator safely",
                "At 128 units native MapSeed caller storage corrupts adjacent memory; this harness does not model that execution",
                "Raw UTF16 output is compared separately from Rust String lossy display conversion",
            ],
            substitutions=["CRT accessor 007D140B returns a supplied per-case thread-storage pointer for original strtok"],
            entry_points={"ReadCommaHexUTF16":0x00528F00,"CRC":0x004A1DE0},
        ))
