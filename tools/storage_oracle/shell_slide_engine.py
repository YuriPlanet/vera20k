"""Executed schedule of the shell button slide engine `0x006071E0`.

`ShellDialog__RunSlideAnimation` animates a dialog's right-panel column: every
tile row, the map button (record `+0xD6`) and the top panel (`+0xD5`), with
`DL = 1` for the slide-in and `DL = 0` for the slide-out. This fixture runs the
original rect initializer `0x0072EC70` for the resolution and then the complete
engine body for a dialog record carrying the flags, recording every
`CC_Draw_Shape` (`0x004AED70`) the engine issues per tick and the message it
sends when the loop ends.

The dialog's children are not emulated: the two EnumChildWindows callbacks
that count the visible top buttons (`0x0060A180`, into `[0x00AC1CAC]`) and the
bottom button (`0x0060A250`, into `[0x00AC4894]`) are replaced by the counts of
each family dialog. Draws are recorded, not rasterised.
"""

from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP,
)

from tools.native_oracle import (
    RET_MAGIC, STACK_BASE, STACK_SIZE, OracleError,
    finish_vectors, load_image, provenance, run_checked,
)

HEAP, HEAP_SIZE = 0x40000000, 0x00100000
STUB, STUB_SIZE = 0x50000000, 0x00010000
HWND = 0x00012345

# Shape globals the engine and the rect initializer read, with the retail
# canvas sizes the initializer derives rectangles from.
SHAPES = {
    0xB0FB50: ("MNSCRNS", 472, 448),
    0xB0FA04: ("MNSCRNL", 632, 568),
    0xB0FAF8: ("SDTP", 168, 199),
    0xB0FAC0: ("SDWRNTMP", 168, 177),
    0xB0FA74: ("SDBTNBKGD", 168, 42),
    0xB0FAC4: ("SDBTNANM", 156, 42),
    0xB0F9DC: ("SDMPBTN", 156, 84),
    0xB0FA38: ("SDBTM", 168, 65),
    0xB0FAE8: ("LWSCRNS", 472, 32),
    0xB0FA54: ("LWSCRNL", 632, 32),
    0xB0FB00: ("xSCRT", 472, 448),
    0xB0FB34: ("xSCRBK", 632, 568),
}
PALETTES = (0xB0FBCC, 0xB0FBDC, 0xB0FBA8)

# Family dialogs: (name, top buttons, bottom button, +0xD5, +0xD6).
DIALOGS = (
    ("0xE2", 5, 1, 0, 0),
    ("0x100", 3, 1, 0, 0),
    ("0x129", 1, 1, 0, 0),
    ("0x102", 2, 1, 1, 1),
    ("0x94", 0, 1, 0, 0),
    ("0xB7", 1, 1, 0, 0),
    ("0xD5", 2, 1, 0, 0),
    ("0x10E", 6, 1, 0, 0),
    ("0xA3", 0, 1, 0, 0),
)
RESOLUTIONS = ((640, 480), (800, 600), (1024, 768))


def run_engine(direction, top_buttons, bottom, d5, d6, width, height):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(RET_MAGIC, 0x1000)
    uc.mem_map(HEAP, HEAP_SIZE)
    uc.mem_map(STUB, STUB_SIZE)
    uc.mem_write(STUB, b"\xc3" * STUB_SIZE)
    heap = [HEAP]

    def alloc(size):
        address = heap[0]
        heap[0] += (size + 15) & ~15
        return address

    def w32(address, value):
        uc.mem_write(address, struct.pack("<I", value & 0xFFFFFFFF))

    def r32(address):
        return struct.unpack("<I", bytes(uc.mem_read(address, 4)))[0]

    def rs32(address):
        return struct.unpack("<i", bytes(uc.mem_read(address, 4)))[0]

    shape_names = {}
    for global_address, (name, w, h) in SHAPES.items():
        header = alloc(16)
        uc.mem_write(header, struct.pack("<HHHH", 0, w, h, 1))
        w32(global_address, header)
        shape_names[header] = name
    for global_address in PALETTES:
        w32(global_address, alloc(16))
    w32(0x8A00A4, width)
    w32(0x8A00A8, height)

    events = []
    stubs = {}

    def stub(handler, argument_bytes):
        address = STUB + 16 * len(stubs)
        stubs[address] = handler
        return address

    def arg(index):
        return r32(uc.reg_read(UC_X86_REG_ESP) + 4 + 4 * index)

    def ret(argument_bytes, eax=None):
        sp = uc.reg_read(UC_X86_REG_ESP)
        if eax is not None:
            uc.reg_write(UC_X86_REG_EAX, eax & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_EIP, r32(sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4 + argument_bytes)

    def draw_shape():
        shape, frame, point = arg(0), arg(1), arg(2)
        events.append(("draw", shape_names.get(shape, hex(shape)), frame,
                       rs32(point), rs32(point + 4)))
        ret(0x38, 1)

    def window_rect_helper():  # 0x775690 fastcall(ecx = hwnd, edx = rect*)
        uc.mem_write(uc.reg_read(UC_X86_REG_EDX), struct.pack("<iiii", 0, 0, width, height))
        ret(0, 1)

    internal = {
        0x7C8E17: lambda: ret(0, alloc(arg(0))),  # operator new
        0x7C8B3D: lambda: ret(0),                  # operator delete
        0x4AED70: draw_shape,                      # CC_Draw_Shape
        0x406F70: lambda: ret(0),                  # audio/theme service
        0x750920: lambda: (events.append(("sound",)), ret(8, 0)),
        0x775690: window_rect_helper,
    }

    def enum_children():
        callback = arg(1)
        if callback == 0x60A180:
            w32(0xAC1CAC, top_buttons)
        elif callback == 0x60A250:
            w32(0xAC4894, bottom)
        else:
            raise OracleError(f"unexpected EnumChildWindows callback {callback:#x}")
        ret(12, 1)

    def get_window_rect():
        uc.mem_write(arg(1), struct.pack("<iiii", 0, 0, width, height))
        ret(8, 1)

    def send_message():
        events.append(("message", arg(1)))
        ret(16, 0)

    for iat, handler, argument_bytes in (
        (0x7E11F0, lambda: (events.append(("tick",)), ret(4, 0)), 4),  # Sleep
        (0x7E14E4, enum_children, 12),
        (0x7E13BC, get_window_rect, 8),
        (0x7E14A4, send_message, 16),
    ):
        w32(iat, stub(handler, argument_bytes))

    def surface():
        instance, vtable = alloc(0x40), alloc(0x100)
        w32(instance, vtable)

        def get_rect():
            uc.mem_write(arg(0), struct.pack("<iiii", 0, 0, width, height))
            ret(4, arg(0))

        for offset, handler, argument_bytes in (
            (0x08, lambda: ret(20, 1), 20),        # Blit
            (0x5C, lambda: ret(8, 0x60000000), 8),  # Lock
            (0x60, lambda: ret(0, 1), 0),          # Unlock
            (0x78, get_rect, 4),
        ):
            w32(vtable + offset, stub(handler, argument_bytes))
        return instance

    w32(0x887308, surface())
    w32(0x887310, surface())
    # WWMouseClass: Get_Mouse_State reports a visible cursor.
    mouse, mouse_vtable = alloc(0x10), alloc(0x40)
    w32(mouse, mouse_vtable)
    w32(mouse_vtable + 0x28, stub(lambda: ret(0, 0), 0))
    w32(mouse_vtable + 0x10, stub(lambda: ret(0, 0), 0))
    w32(0x887640, mouse)
    rules = alloc(0x800)
    w32(rules + 0x750, 0xFFFFFFFF)
    w32(0x8871E0, rules)
    # One-bucket dialog-record table holding the dialog's record.
    buckets, node = alloc(16), alloc(0x210)
    w32(buckets, node)
    w32(node, HWND)
    uc.mem_write(node + 4 + 0xD5, bytes([d5, d6, 0]))
    w32(node + 0x204, 0)
    w32(0xAC1B00, buckets)
    w32(0xAC1B04, 1)
    w32(0xAC1B0C, 0)
    w32(0xAC1B18, stub(lambda: ret(0, 0), 0))

    def dispatch(_uc, address, _size, _data):
        handler = internal.get(address) or stubs.get(address)
        if handler:
            handler()

    uc.hook_add(UC_HOOK_CODE, dispatch)

    def invoke(function, ecx, edx):
        sp = STACK_BASE + STACK_SIZE - 0x1000 - 4
        w32(sp, RET_MAGIC)
        uc.reg_write(UC_X86_REG_ESP, sp)
        uc.reg_write(UC_X86_REG_ECX, ecx)
        uc.reg_write(UC_X86_REG_EDX, edx)
        run_checked(uc, function, RET_MAGIC, count=20_000_000, timeout_us=60_000_000)
        if uc.reg_read(UC_X86_REG_ESP) != sp + 4:
            raise OracleError(f"stack imbalance after {function:#x}")

    invoke(0x72EC70, width, height)
    rows = r32(0xB0FA20)
    events.clear()
    invoke(0x6071E0, HWND, direction)

    ticks, current = [], {"buttons": [], "art": []}
    end_message = None
    for event in events:
        if event[0] == "tick":
            ticks.append(current)
            current = {"buttons": [], "art": []}
        elif event[0] == "draw" and event[1] == "SDBTNANM":
            current["buttons"].append([event[3], event[4], event[2]])
        elif event[0] == "draw":
            current["art"].append([event[1], event[2], event[3], event[4]])
        elif event[0] == "message":
            end_message = f"0x{event[1]:X}"
    if current["buttons"] or current["art"]:
        raise OracleError("draws after the last tick")
    return {"rows": rows, "ticks": ticks, "end_message": end_message}


def generate():
    cases = []
    for name, top_buttons, bottom, d5, d6 in DIALOGS:
        for width, height in RESOLUTIONS:
            for direction in (1, 0):
                result = run_engine(direction, top_buttons, bottom, d5, d6, width, height)
                cases.append({
                    "dialog": name, "width": width, "height": height,
                    "direction": "in" if direction else "out",
                    "top_buttons": top_buttons, "bottom_button": bool(bottom),
                    "top_panel": bool(d5), "map_button": bool(d6),
                    **result,
                })
    return {"source": "unicorn/gamemd.exe", "cases": cases}


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=lambda: provenance(
        scope="Complete original slide engine 0x006071E0 (both directions) after the rect initializer 0x0072EC70, for the family dialogs' button counts and flags at 640x480, 800x600 and 1024x768",
        assumptions=[
            "Visible top-button and bottom-button counts per dialog are supplied (0xE2 5+1, 0x100 and 0x101 3+1, 0x129 1+1, 0x102 2+1, 0x94 0+1 with Load 0x40E hidden, 0xB7 1+1 with Load 0x40F counted even when disabled, 0xD5 2+1 Keyboard and Network, 0x10E 6+1 the Westwood Online welcome buttons, 0xA3 0+1 Keyboard Back); the classifier callbacks 0x0060A180/0x0060A250 are not executed",
            "Record flags +0xD5/+0xD6/+0xD7 as set per dialog id at creation: 0x102 has +0xD5 and +0xD6, the family pages none",
            "Draw calls are recorded as (shape, frame, x, y); pixels, palettes and blits are not rasterised",
        ],
        substitutions=[
            "CC_Draw_Shape 0x004AED70 recorded and returns 1",
            "operator new/delete use a bump heap; the audio service 0x00406F70 and VocClass play 0x00750920 return",
            "Sleep, GetWindowRect, SendMessageA and EnumChildWindows (with the fixture counts) are stubbed",
            "Window-rect helper 0x00775690 returns (0,0,W,H); surface vtables Blit/Lock/Unlock/GetRect are stubbed",
            "Shape headers carry the retail canvas sizes only; WWMouse reports a visible cursor",
        ],
        entry_points={"rect_init": 0x72EC70, "slide_engine": 0x6071E0,
                      "top_button_count": 0x60A180, "bottom_button_count": 0x60A250},
    ))
