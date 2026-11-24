"""Read-only original qsort + saved-entry comparator; replace CompareFileTime only."""
import struct
from pathlib import Path
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_EIP
from tools.native_oracle import (
    load_image, run_checked, SCRATCH, STACK_BASE, STACK_SIZE, RET_MAGIC,
    finish_vectors, provenance,
)


def sort_case(times):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, 0x20000)
    uc.mem_map(RET_MAGIC, 0x1000)
    ptrs = [SCRATCH + 0x1000 + i * 0x200 for i in range(len(times))]
    if ptrs:
        uc.mem_write(SCRATCH, struct.pack('<' + 'I' * len(ptrs), *ptrs))
    for pointer, timestamp in zip(ptrs, times):
        uc.mem_write(pointer + 0x1AC, struct.pack('<Q', timestamp))
    esp = STACK_BASE + STACK_SIZE - 0x1000
    uc.mem_write(esp, struct.pack('<IIIII', RET_MAGIC, SCRATCH, len(times), 4, 0x00559D30))
    uc.reg_write(UC_X86_REG_ESP, esp)
    comparisons = []

    def hook(emu, address, size, data):
        if address == 0x00559D49:
            sp = emu.reg_read(UC_X86_REG_ESP)
            left, right = struct.unpack('<II', emu.mem_read(sp, 8))
            a = struct.unpack('<Q', emu.mem_read(left, 8))[0]
            b = struct.unpack('<Q', emu.mem_read(right, 8))[0]
            comparisons.append([
                (left - SCRATCH - 0x1000 - 0x1AC) // 0x200,
                (right - SCRATCH - 0x1000 - 0x1AC) // 0x200,
            ])
            emu.reg_write(UC_X86_REG_EAX, ((a > b) - (a < b)) & 0xFFFFFFFF)
            emu.reg_write(UC_X86_REG_ESP, sp + 8)
            emu.reg_write(UC_X86_REG_EIP, 0x00559D4F)

    uc.hook_add(UC_HOOK_CODE, hook)
    run_checked(uc, 0x007C8B48, RET_MAGIC, count=1000000, timeout_us=1000000)
    result = (
        struct.unpack('<' + 'I' * len(ptrs), uc.mem_read(SCRATCH, 4 * len(ptrs)))
        if ptrs else []
    )
    order = [ptrs.index(pointer) for pointer in result]
    assert uc.reg_read(UC_X86_REG_ESP) == esp + 4
    assert sorted(order) == list(range(len(times)))
    assert [times[i] for i in order] == sorted(times, reverse=True)
    return {'input_times': times, 'output_indices': order, 'comparisons': len(comparisons)}

def generate():
    cases = {f"equal_{n}": sort_case([100] * n) for n in [0, 1, 2, 3, 4, 7, 8, 9, 10, 16, 17, 32]}
    examples = {
        "mixed_8": [20, 10, 20, 10, 30, 30, 20, 10],
        "mixed_9": [20, 10, 20, 10, 30, 30, 20, 10, 30],
        "new_row_future_file": [100, 90, 110],
        "new_row_equal_3": [100, 100, 100],
        "new_row_equal_9": [100] * 9,
        "high_dword": [0, 0xffffffff, 0x100000000, 0x200000000, 0x100000001, 0xffffffffffffffff],
    }
    for n in [9, 10, 16, 32, 64, 127]:
        examples[f"ascending_{n}"] = list(range(n))
        examples[f"descending_{n}"] = list(reversed(range(n)))
        examples[f"ties_{n}"] = [(i * 13 + i // 3) % 7 for i in range(n)]
    cases.update({name: sort_case(times) for name, times in examples.items()})
    return {"source": "unicorn/gamemd.exe", "cases": cases}


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
        provenance=lambda: provenance(
            scope="36 saved-entry qsort fixtures through original qsort/short-sort/comparator; order and comparison count",
            assumptions=[
                "The input pointer array preserves supplied enumeration order; physical filesystem enumeration is outside the fixture",
                "Each record has FILETIME at +0x1AC; all other fields are zero",
                "Fixtures cover both sides of eight-element cutoff, equal keys, future/new-row placement and high DWORD timestamps",
                "The comparator is pure unsigned FILETIME ordering; no other qsort caller is certified",
            ],
            substitutions=["CompareFileTime IAT call at 00559D49 supplies unsigned64 order and stdcall cleanup; original comparator NEG executes"],
            entry_points={"qsort": 0x007C8B48, "short_sort": 0x007C8C9C, "saved_comparator": 0x00559D30},
        ))
