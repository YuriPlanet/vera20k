"""Complete original 72FC60 geometry with original SHP canvas headers.

72F540's first-paint path calls 72FA10, then passes configured width/height to
72FC60 at 72F55E..72F56A. WM_PAINT_Handler reaches that draw at 621FD5.
72FA10's original pointer table supplies the asset names below: notably
B0FA3C is LSPACER, B0FA90 LENDCAP, B0FAA8 BTTNBKGD, B0FABC RENDCAP.
SIDE2 supplies geometry via B0FAFC; SIDE2B used by the painter is separate.

Archive bytes are explicit stock fixture inputs, not emulation of archive
mounting or palette/rendering. The only execution substitute is successful
16-byte operator-new. Child fixtures additionally execute complete 60B7A0
with supplied Win32 rectangles and an active-scenario predicate. Python maps
original dialog units to input pixels; native instructions calculate placement.
"""

from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX,
    UC_X86_REG_EIP, UC_X86_REG_ESP,
)

from tools.native_oracle import (
    RET_MAGIC, SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE,
    finish_vectors, load_image, provenance, run_checked,
)
from tools.sidebar_oracle.stock import stock_bytes


# Original 72FA10 load destination -> original filename-pointer slot.
ASSET_POINTERS = {
    0xB0FAD4: 0x844D00, 0xB0FAC8: 0x844D04, 0xB0FA50: 0x844D08,
    0xB0FB08: 0x844D0C, 0xB0F9E0: 0x844D10, 0xB0FA68: 0x844D14,
    0xB0FA70: 0x844D18, 0xB0FAFC: 0x844D1C, 0xB0FA8C: 0x844D24,
    0xB0FA48: 0x844D28, 0xB0FA3C: 0x844D2C, 0xB0FA90: 0x844D30,
    0xB0FAA8: 0x844D34, 0xB0FABC: 0x844D38,
}
YURI_POINTERS = {
    0xB0FAD4: 0x844D40, 0xB0FAC8: 0x844D3C,
    0xB0FA50: 0x844D44, 0xB0FA68: 0x844D48,
}


def read_u32(uc, address):
    return struct.unpack("<I", uc.mem_read(address, 4))[0]


def retail_headers():
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    result = {}
    for theme in ("allied", "soviet", "yuri"):
        assets = {}
        pointers = dict(ASSET_POINTERS)
        if theme == "yuri":
            pointers.update(YURI_POINTERS)
        for destination, pointer_slot in pointers.items():
            address = read_u32(uc, pointer_slot)
            name = bytes(uc.mem_read(address, 64)).split(b"\0", 1)[0].decode("ascii")
            if theme == "allied":
                archive = "sidec01.mix"
            elif theme == "yuri" and destination in YURI_POINTERS:
                archive = "sidec02md.mix"
            else:
                archive = "sidec02.mix"
            data = stock_bytes(archive, name)
            marker, width, height, frames = struct.unpack_from("<4H", data)
            if marker != 0 or not width or not height or not frames:
                raise RuntimeError(f"Invalid supplied SHP header: {archive}/{name}")
            assets[f"{destination:08x}"] = {
                "name": name, "archive": archive,
                "header_hex": data[:8].hex(), "width": width, "height": height,
                "frames": frames, "sha256": hashlib.sha256(data).hexdigest(),
            }
        result[theme] = assets
    return result


def native_case(theme, width, height, assets):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    uc.mem_map(RET_MAGIC, 0x1000)
    for index, (destination, asset) in enumerate(assets.items()):
        header_address = SCRATCH + index * 16
        uc.mem_write(header_address, bytes.fromhex(asset["header_hex"]))
        uc.mem_write(int(destination, 16), struct.pack("<I", header_address))
    uc.mem_write(0xA8EB84, struct.pack("<I", width))
    stack = STACK_BASE + STACK_SIZE - 0x1000
    uc.mem_write(stack, struct.pack("<I", RET_MAGIC))
    uc.reg_write(UC_X86_REG_ESP, stack)
    uc.reg_write(UC_X86_REG_ECX, width)
    uc.reg_write(UC_X86_REG_EDX, height)
    allocated = []

    def allocate(_uc, address, _size, _data):
        if address != 0x7C8E17:
            return
        sp = uc.reg_read(UC_X86_REG_ESP)
        requested = read_u32(uc, sp + 4)
        if requested != 16 or len(allocated) >= 16:
            raise RuntimeError(f"Unexpected native allocation: {requested}")
        block = SCRATCH + 0x1000 + len(allocated) * 0x20
        # Poison bytes distinguish original initialization from zero-filled memory.
        uc.mem_write(block, b"\xCD" * 16)
        allocated.append(block)
        uc.reg_write(UC_X86_REG_EAX, block)
        uc.reg_write(UC_X86_REG_EIP, read_u32(uc, sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4)  # Native caller removes size argument.

    hook = uc.hook_add(UC_HOOK_CODE, allocate)
    try:
        run_checked(uc, 0x72FC60, RET_MAGIC, count=2000,
                    required_addresses=[0x72FE77, 0x72FFEB, 0x730058, 0x7300DF])
    finally:
        uc.hook_del(hook)
    if len(allocated) != 16:
        raise RuntimeError(f"Expected 16 native rectangles, got {len(allocated)}")
    rects = {}
    for slot in range(0xB0FC30, 0xB0FC70, 4):
        pointer = read_u32(uc, slot)
        if pointer not in allocated:
            raise RuntimeError(f"Output {slot:#x} is not an allocated rectangle")
        rect = list(struct.unpack("<4i", uc.mem_read(pointer, 16)))
        if -842150451 in rect:  # 0xCDCDCDCD interpreted as signed i32.
            raise RuntimeError(f"Output {slot:#x} retained allocation poison")
        rects[f"{slot:08x}"] = rect
    return {
        "theme": theme, "width": width, "height": height, "rects": rects,
        "side2_count": read_u32(uc, 0xB0FADC),
        "bottom_repeat_count": read_u32(uc, 0xB0F9E4),
        "bottom_left_x": read_u32(uc, 0xB0FAD8),
        "bottom_closed_x": read_u32(uc, 0xB0FAA4),
        "bottom_repeat_span": read_u32(uc, 0xB0FA78),
        "allocation_sizes": [16] * len(allocated),
    }


def generate():
    assets = retail_headers()
    return {"source": "unicorn/gamemd.exe", "assets": assets, "cases": [
        native_case(theme, width, height, headers)
        for theme, headers in assets.items()
        for width, height in ((640, 480), (800, 600), (1024, 768))
    ], "child_layout": [
        native_child_case(control_id, resource_rect, width, height)
        for control_id, resource_rect in bbb_ordinary_rects().items()
        for width, height in ((640, 480), (800, 600), (1024, 768))
    ]}


def bbb_ordinary_rects():
    """Read original RT_DIALOG/0xBBB/1033 DLGTEMPLATE at its pinned PE VA."""
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    data = bytes(uc.mem_read(0xC01B18, 4096))
    style, _exstyle, count, *_bounds = struct.unpack_from("<IIH4h", data)
    if style != 0x40000040 or count != 17:
        raise RuntimeError("Unexpected original BBB dialog header")
    offset = 18

    def variable():
        nonlocal offset
        start = offset
        word = struct.unpack_from("<H", data, offset)[0]
        if word == 0xFFFF:
            offset += 4
            return "ordinal"
        while struct.unpack_from("<H", data, offset)[0]:
            offset += 2
        offset += 2
        return data[start:offset - 2].decode("utf-16-le")

    for _ in range(3):
        variable()  # Menu, window class, title.
    point_size = struct.unpack_from("<H", data, offset)[0]
    offset += 2
    if point_size != 8 or variable() != "MS Sans Serif":
        raise RuntimeError("Unexpected original BBB dialog font")
    result = {}
    for _ in range(count):
        offset = (offset + 3) & ~3
        _style, _exstyle, x, y, width, height, control = struct.unpack_from(
            "<II4hH", data, offset)
        offset += 18
        variable()  # Class.
        variable()  # Caption.
        extra = struct.unpack_from("<H", data, offset)[0]
        offset += 2 + extra
        if control in (0x714, 0x529, 0x601):
            # Supplied ordinary 6x13 dialog base units, Win32 MulDiv rounding.
            result[control] = [(x * 6 + 2) // 4, (y * 13 + 4) // 8,
                               (width * 6 + 2) // 4, (height * 13 + 4) // 8]
    if len(result) != 3:
        raise RuntimeError("Missing representative original BBB controls")
    return result


def native_child_case(control_id, resource_rect, width, height):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    uc.mem_map(RET_MAGIC, 0x1000)
    child, parent = 0x1234, 0x5678
    x, y, w, h = resource_rect
    rectangles = {child: [x, y, x + w, y + h], parent: [0, 0, width, height]}
    # Only imported function-pointer DATA is replaced; body bytes stay original.
    imports = {0x7E14EC: SCRATCH + 0xF000,  # GetParent
               0x7E13BC: SCRATCH + 0xF010,  # GetWindowRect
               0x7E1398: SCRATCH + 0xF020}  # MoveWindow
    for slot, target in imports.items():
        uc.mem_write(slot, struct.pack("<I", target))
    stack = STACK_BASE + STACK_SIZE - 0x1000
    uc.mem_write(stack, struct.pack("<I", RET_MAGIC))
    uc.reg_write(UC_X86_REG_ESP, stack)
    uc.reg_write(UC_X86_REG_ECX, child)
    output = []
    calls = {target: 0 for target in [*imports.values(), 0x69BBE0]}

    def substitute(_uc, address, _size, _data):
        if address not in calls:
            return
        calls[address] += 1
        sp = uc.reg_read(UC_X86_REG_ESP)
        result, arguments = 1, 0
        if address == 0x69BBE0:
            if uc.reg_read(UC_X86_REG_ECX) != 0xA8B238:
                raise RuntimeError("Unexpected active predicate receiver")
        elif address == imports[0x7E14EC]:
            if read_u32(uc, sp + 4) != child:
                raise RuntimeError("Unexpected GetParent receiver")
            result, arguments = parent, 1
        elif address == imports[0x7E13BC]:
            handle, target = struct.unpack("<2I", uc.mem_read(sp + 4, 8))
            uc.mem_write(target, struct.pack("<4i", *rectangles[handle]))
            arguments = 2
        else:
            handle, left, top, out_w, out_h, repaint = struct.unpack(
                "<6i", uc.mem_read(sp + 4, 24))
            if handle != child or repaint:
                raise RuntimeError("Unexpected MoveWindow receiver/repaint")
            output.append([left, top, out_w, out_h])
            arguments = 6
        uc.reg_write(UC_X86_REG_EAX, result)
        uc.reg_write(UC_X86_REG_EIP, read_u32(uc, sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4 * (1 + arguments))

    hook = uc.hook_add(UC_HOOK_CODE, substitute)
    try:
        run_checked(uc, 0x60B7A0, RET_MAGIC, count=300,
                    required_addresses=[0x60B7D9, 0x60B84A, 0x60B86C])
    finally:
        uc.hook_del(hook)
    if list(calls.values()) != [1, 2, 1, 1] or len(output) != 1:
        raise RuntimeError(f"Unexpected child layout calls: {calls}")
    return {"dialog_id": 0xBBB, "control_id": control_id,
            "resource_rect": resource_rect, "width": width, "height": height,
            "rect": output[0]}


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=lambda: provenance(
        scope="Nine complete 72FC60 shell geometry fixtures plus nine complete60B7A0 ordinary BBB child placements at three resolutions",
        assumptions=[
            "Fresh emulator for each geometry; ECX and A8EB84 both contain width, EDX contains height; resolutions640x480/800x600/1024x768",
            "Original72FA10 filename-pointer slots identify the 14 SHP inputs, including side-index2's four Yuri names; raw first8 bytes and whole-file SHA256 are recorded",
            "Explicit nested archive inputs via tools.sidebar_oracle.stock: Allied sidec01, Soviet sidec02, Yuri four Y-specific files sidec02md plus shared files sidec02; native archive mounting is not emulated",
            "All16 B0FC30..B0FC6C rectangle pointers are read after native return; output40 and54 are initialized at the tail; allocation poison guards against unwritten fields",
            "SIDE2 at B0FAFC determines geometry; separate SIDE2B paint selection, palette conversion, clipping and final pixels are outside this fixture",
            "Only ordinary stock positive dimensions and successful allocations are covered; this is not an odd-size or allocation-failure contract",
            "Child input rectangles come from original RT_DIALOG/BBB/1033 atC01B18, font8 MS Sans Serif; supplied6x13 dialog base units convert resource DLU via MulDiv rounding, not emulated Windows font metrics",
            "Three representative ordinary BBB controls529/714/601 execute full60B7A0 with parentorigin0 and parentextent equal resolution; original7F5BE4/7F5BF0 base800/600 globals remain untouched; dispatcher and title/back helpers are outside child fixture scope",
        ],
        substitutions=["7C8E17 operator-new returns distinct writable 16-byte blocks; original caller cleanup executes, no native instructions are patched",
                       "Child fixtures replace GetParent/GetWindowRect/MoveWindow IAT pointers with supplied window-state hooks, capture native MoveWindow arguments, and returntrue from69BBE0 active predicate; no placement arithmetic is substituted"],
        entry_points={"layout": 0x72FC60, "loader": 0x72FA10,
                      "first_paint_layout_call": 0x72F56A,
                      "side2_count_division": 0x72FE77,
                      "bottom_repeat_division": 0x72FFEB,
                      "ordinary_child_placement": 0x60B7A0},
    ))
