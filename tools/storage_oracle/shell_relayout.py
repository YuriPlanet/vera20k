"""Executed WM_INITDIALOG relayout of right-panel shell dialogs.

`ShellDialog__CommonMessageHandler` `0x00622B50` lays a family dialog out on
WM_INITDIALOG (`0x00622F8C..0x00622FBE`), after the rect initializer
`0x0072EC70` has run for the resolution:

- `EnumChildWindows(0x0060AAB0)`: each child's record setup, including the
  right-panel inset override `+0xDC` (`0x0060AC99..0x0060AD16`);
- `0x0060C540(dialog)`: the slide include set (record `+0xB0`);
- `0x0060C4A0(dialog, &{640, 480})`: `MoveWindow(dialog, 0, 0, W, H)` and
  `EnumChildWindows(0x0060C0C0)`, whose per-child placement dispatches to the
  right-panel static `0x0060B1D0`, the status line `0x0060B550`, the button
  rows `0x0060B000`/`0x0060B350` and the fix-up pass `0x0060B950`.

This fixture parses the original RT_DIALOG templates, creates each child at
the Windows conversion of its template (6x13 dialog base units, MulDiv
rounding) one pixel wider and taller, runs those bodies and records every
child's final window rect and the placement helpers it passed.
"""

from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP,
)

from tools.native_oracle import (
    IMAGE_BASE, RET_MAGIC, STACK_BASE, STACK_SIZE, OracleError,
    finish_vectors, load_image, provenance, run_checked,
)

HEAP, HEAP_SIZE = 0x40000000, 0x00200000
STUB, STUB_SIZE = 0x50000000, 0x00010000
PARENT = 0x1000
# The runtime child window is one pixel wider and taller than the template
# conversion (measured on the retail heading, monitor and list windows).
WINDOW_GROWTH = 1
SIZES = ((640, 480), (800, 600), (1024, 768))
# Skirmish, Choose Map, the random-map dialog and the score screen.
DIALOGS = (0x102, 0x6B, 0x105, 0x108)

# Shape globals the rect initializer reads, with the retail canvas sizes
# (the same fixture as shell_slide_engine.py).
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

HELPERS = {
    0x60AF50: "0x0060AF50", 0x60B000: "0x0060B000", 0x60B1D0: "0x0060B1D0",
    0x60B350: "0x0060B350", 0x60B420: "0x0060B420", 0x60B550: "0x0060B550",
    0x60B610: "0x0060B610", 0x60B7A0: "0x0060B7A0", 0x60B950: "0x0060B950",
}
CLASSES = {0x80: "Button", 0x81: "Edit", 0x82: "Static", 0x83: "ListBox",
           0x84: "ScrollBar", 0x85: "ComboBox"}


def muldiv(value, numerator, denominator):
    product = value * numerator
    half = denominator // 2
    return (product + half) // denominator if product >= 0 else -((-product + half) // denominator)


class Image:
    """Reads the mapped original image: resource and import directories."""

    def __init__(self, uc):
        self.uc = uc
        pe = self.u32(IMAGE_BASE + 0x3C)
        self.directories = IMAGE_BASE + pe + 24 + 96

    def u16(self, address):
        return struct.unpack("<H", bytes(self.uc.mem_read(address, 2)))[0]

    def u32(self, address):
        return struct.unpack("<I", bytes(self.uc.mem_read(address, 4)))[0]

    def directory(self, index):
        return IMAGE_BASE + self.u32(self.directories + 8 * index)

    def imports(self):
        """IAT address -> imported function name."""
        names = {}
        descriptor = self.directory(1)
        while self.u32(descriptor + 12):
            lookup, iat = self.u32(descriptor), self.u32(descriptor + 16)
            index = 0
            while entry := self.u32(IMAGE_BASE + (lookup or iat) + 4 * index):
                if not entry & 0x80000000:
                    names[IMAGE_BASE + iat + 4 * index] = self.cstr(IMAGE_BASE + entry + 2)
                index += 1
            descriptor += 20
        return names

    def cstr(self, address):
        out = bytearray()
        while byte := self.uc.mem_read(address + len(out), 1)[0]:
            out.append(byte)
        return out.decode("ascii")

    def entries(self, root, offset):
        count = self.u16(root + offset + 12) + self.u16(root + offset + 14)
        base = root + offset + 16
        return [(self.u32(base + 8 * i), self.u32(base + 8 * i + 4)) for i in range(count)]

    def dialog_template(self, dialog_id):
        root = self.directory(2)
        for kind, types in self.entries(root, 0):
            if kind != 5:
                continue
            for name, languages in self.entries(root, types & 0x7FFFFFFF):
                if name != dialog_id:
                    continue
                for _language, leaf in self.entries(root, languages & 0x7FFFFFFF):
                    rva, size = self.u32(root + leaf), self.u32(root + leaf + 4)
                    return bytes(self.uc.mem_read(IMAGE_BASE + rva, size))
        raise OracleError(f"RT_DIALOG {dialog_id:#x} missing")


def parse_template(data):
    """DLGTEMPLATE or DLGTEMPLATEEX: the controls in template order."""
    extended = struct.unpack_from("<HH", data, 0) == (1, 0xFFFF)
    if extended:
        _help, _exstyle, style, count = struct.unpack_from("<IIIH", data, 4)
        offset = 26
    else:
        style, _exstyle, count = struct.unpack_from("<IIH", data, 0)
        offset = 18

    def variable():
        nonlocal offset
        start = offset
        if struct.unpack_from("<H", data, offset)[0] == 0xFFFF:
            offset += 4
            return struct.unpack_from("<H", data, start + 2)[0]
        while struct.unpack_from("<H", data, offset)[0]:
            offset += 2
        offset += 2
        return data[start:offset - 2].decode("utf-16-le")

    variable(), variable(), variable()  # Menu, class, title.
    if style & 0x40:  # DS_SETFONT: point size (EX: weight, italic, charset), face.
        offset += 6 if extended else 2
        variable()
    controls = []
    for _ in range(count):
        offset = (offset + 3) & ~3
        if extended:
            _help, _exstyle, control_style, x, y, w, h, control = struct.unpack_from(
                "<III4hI", data, offset)
            offset += 24
        else:
            control_style, _exstyle, x, y, w, h, control = struct.unpack_from(
                "<II4hH", data, offset)
            offset += 18
        kind, caption = variable(), variable()
        offset += 2 + struct.unpack_from("<H", data, offset)[0]
        controls.append({"id": control, "class": CLASSES.get(kind, kind),
                         "style": control_style, "template": [x, y, w, h],
                         "caption": caption})
    return controls


class Machine:
    """One dialog and its children on a stubbed window manager."""

    def __init__(self, dialog_id, width, height):
        self.width, self.height = width, height
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(uc)
        uc.mem_map(STACK_BASE, STACK_SIZE)
        uc.mem_map(RET_MAGIC, 0x1000)
        uc.mem_map(HEAP, HEAP_SIZE)
        uc.mem_map(STUB, STUB_SIZE)
        uc.mem_write(STUB, b"\xc3" * STUB_SIZE)
        self.heap = HEAP
        self.stubs = {}
        self.pending_enum = None
        image = Image(uc)
        self.controls = parse_template(image.dialog_template(dialog_id))
        for address, (_name, w, h) in SHAPES.items():
            header = self.alloc(16)
            uc.mem_write(header, struct.pack("<HHHH", 0, w, h, 1))
            self.w32(address, header)
        for address in PALETTES:
            self.w32(address, self.alloc(16))
        self.w32(0x8A00A4, width)
        self.w32(0x8A00A8, height)
        self.windows = {PARENT: {"id": 0, "class": "#32770", "style": 0x40000040,
                                 "rect": [0, 0, width, height]}}
        self.children = []
        for index, control in enumerate(self.controls):
            hwnd = 0x2000 + index
            x, y, w, h = control["template"]
            left, top = muldiv(x, 6, 4), muldiv(y, 13, 8)
            self.windows[hwnd] = {
                "id": control["id"], "class": control["class"], "style": control["style"],
                "rect": [left, top, left + muldiv(w, 6, 4) + WINDOW_GROWTH,
                         top + muldiv(h, 13, 8) + WINDOW_GROWTH],
            }
            self.children.append(hwnd)
        # Dialog-record table: one bucket chaining a 0x208-byte node per
        # window; [node] = HWND, record = node + 4.
        self.nodes = {}
        buckets = self.alloc(16)
        self.w32(0xAC1B00, buckets)
        self.w32(0xAC1B0C, 0)
        self.w32(0xAC1B18, self.stub(lambda: self.ret(0, 0)))
        previous = 0
        for hwnd in [PARENT] + self.children:
            node = self.alloc(0x210)
            self.w32(node, hwnd)
            self.w32(node + 0x204, previous)
            previous = node
            self.nodes[hwnd] = node
        self.w32(buckets, previous)
        self.w32(0xAC1B04, len(self.nodes))
        self.w32(self.record(PARENT) + 0x6C, dialog_id)
        for hwnd in self.children:
            code = {"Button": 0, "Static": 2}.get(self.windows[hwnd]["class"], 0xB)
            self.w32(self.record(hwnd) + 0x68, code)
        self.w32(0xAC48A8, PARENT)
        self.install(image.imports())

    def alloc(self, size):
        address = self.heap
        self.heap += (size + 15) & ~15
        return address

    def w32(self, address, value):
        self.uc.mem_write(address, struct.pack("<I", value & 0xFFFFFFFF))

    def r32(self, address):
        return struct.unpack("<I", bytes(self.uc.mem_read(address, 4)))[0]

    def record(self, hwnd):
        return self.nodes[hwnd] + 4

    def arg(self, index):
        return self.r32(self.uc.reg_read(UC_X86_REG_ESP) + 4 + 4 * index)

    def signed_args(self, count):
        return [struct.unpack("<i", struct.pack("<I", self.arg(i)))[0] for i in range(count)]

    def ret(self, argument_bytes, eax=None):
        sp = self.uc.reg_read(UC_X86_REG_ESP)
        if eax is not None:
            self.uc.reg_write(UC_X86_REG_EAX, eax & 0xFFFFFFFF)
        self.uc.reg_write(UC_X86_REG_EIP, self.r32(sp))
        self.uc.reg_write(UC_X86_REG_ESP, sp + 4 + argument_bytes)

    def stub(self, handler):
        address = STUB + 16 * len(self.stubs)
        self.stubs[address] = handler
        return address

    def install(self, imports):
        uc, windows = self.uc, self.windows

        def get_window_rect():
            uc.mem_write(self.arg(1), struct.pack("<iiii", *windows[self.arg(0)]["rect"]))
            self.ret(8, 1)

        def get_client_rect():
            left, top, right, bottom = windows[self.arg(0)]["rect"]
            uc.mem_write(self.arg(1), struct.pack("<iiii", 0, 0, right - left, bottom - top))
            self.ret(8, 1)

        def move_window():
            hwnd = self.arg(0)
            _, x, y, w, h = self.signed_args(5)
            windows[hwnd]["rect"] = [x, y, x + w, y + h]
            self.ret(24, 1)

        def set_window_pos():
            hwnd, flags = self.arg(0), self.arg(6)
            _, _, x, y, w, h = self.signed_args(6)
            left, top, right, bottom = windows[hwnd]["rect"]
            if flags & 2:  # SWP_NOMOVE
                x, y = left, top
            if flags & 1:  # SWP_NOSIZE
                w, h = right - left, bottom - top
            windows[hwnd]["rect"] = [x, y, x + w, y + h]
            self.ret(28, 1)

        def get_window_long():
            index = self.signed_args(2)[1]
            window = windows[self.arg(0)]
            self.ret(8, {-16: window["style"], -12: window["id"]}.get(index, 0))

        def get_dlg_item():
            wanted = self.arg(1)
            self.ret(8, next((h for h in self.children if windows[h]["id"] == wanted), 0))

        def get_class_name():
            name = windows[self.arg(0)]["class"].encode() + b"\0"
            uc.mem_write(self.arg(1), name)
            self.ret(12, len(name) - 1)

        def enum_child_windows():
            self.pending_enum = (self.arg(1), self.arg(2))
            self.ret(12, 1)

        handlers = {
            "GetParent": (lambda: self.ret(4, PARENT if self.arg(0) != PARENT else 0)),
            "GetWindowRect": get_window_rect,
            "GetClientRect": get_client_rect,
            "MoveWindow": move_window,
            "SetWindowPos": set_window_pos,
            "GetWindowLongA": get_window_long,
            "GetDlgCtrlID": (lambda: self.ret(4, windows[self.arg(0)]["id"])),
            "GetDlgItem": get_dlg_item,
            "SendMessageA": (lambda: self.ret(16, 0)),
            "IsWindowVisible": (lambda: self.ret(
                4, 1 if windows.get(self.arg(0), {}).get("style", 0) & 0x10000000 else 0)),
            "GetClassNameA": get_class_name,
            "ClientToScreen": (lambda: self.ret(8, 1)),
            "EnumChildWindows": enum_child_windows,
            "Sleep": (lambda: self.ret(4, 0)),
        }
        for iat, name in imports.items():
            handler = handlers.get(name)
            if handler is None:
                def handler(name=name):
                    raise OracleError(f"unexpected import call {name}")
            self.w32(iat, self.stub(handler))

        def window_rect():  # 0x00775690 fastcall(ECX = hwnd, EDX = rect out)
            rect = windows.get(uc.reg_read(UC_X86_REG_ECX), {"rect": [0, 0, self.width, self.height]})["rect"]
            uc.mem_write(uc.reg_read(UC_X86_REG_EDX), struct.pack("<iiii", *rect))
            self.ret(0, 1)

        internal = {
            0x7C8E17: lambda: self.ret(0, self.alloc(self.arg(0))),  # operator new
            0x7C8B3D: lambda: self.ret(0),  # operator delete
            0x775690: window_rect,
        }

        def dispatch(_uc, address, _size, _data):
            handler = internal.get(address) or self.stubs.get(address)
            if handler:
                handler()

        uc.hook_add(UC_HOOK_CODE, dispatch)

    def call(self, function, ecx=0, edx=0, stack_args=(), count=5_000_000):
        sp = STACK_BASE + STACK_SIZE - 0x2000 - 4 * len(stack_args)
        for index, value in enumerate(stack_args):
            self.w32(sp + 4 * index, value)
        sp -= 4
        self.w32(sp, RET_MAGIC)
        self.uc.reg_write(UC_X86_REG_ESP, sp)
        self.uc.reg_write(UC_X86_REG_ECX, ecx)
        self.uc.reg_write(UC_X86_REG_EDX, edx)
        run_checked(self.uc, function, RET_MAGIC, count=count, timeout_us=60_000_000)


def relayout(dialog_id, width, height):
    machine = Machine(dialog_id, width, height)
    machine.call(0x72EC70, ecx=width, edx=height, count=20_000_000)
    for hwnd in machine.children:
        machine.call(0x60AAB0, stack_args=(hwnd, 0))
    machine.call(0x60C540, ecx=PARENT)
    minimum = machine.alloc(16)
    machine.w32(minimum, 640)
    machine.w32(minimum + 4, 480)
    machine.call(0x60C4A0, ecx=PARENT, edx=minimum)
    callback, parameter = machine.pending_enum or (None, None)
    if callback != 0x60C0C0:
        raise OracleError(f"0x0060C4A0 enumerated with {callback!r}, expected 0x0060C0C0")
    helpers = []

    def observe(_uc, address, _size, _data):
        if address in HELPERS:
            helpers.append(HELPERS[address])

    hook = machine.uc.hook_add(UC_HOOK_CODE, observe)
    children = []
    try:
        for hwnd, control in zip(machine.children, machine.controls):
            helpers.clear()
            machine.call(0x60C0C0, stack_args=(hwnd, parameter))
            left, top, right, bottom = machine.windows[hwnd]["rect"]
            children.append({
                "id": control["id"], "class": control["class"], "template": control["template"],
                "rect": [left, top, right - left, bottom - top], "placement": list(helpers),
                "inset_override": machine.r32(machine.record(hwnd) + 0xDC),
            })
    finally:
        machine.uc.hook_del(hook)
    return children


def generate():
    return {
        "source": "unicorn/gamemd.exe",
        "cases": [
            {"dialog_id": dialog_id, "width": width, "height": height,
             "children": relayout(dialog_id, width, height)}
            for dialog_id in DIALOGS
            for width, height in SIZES
        ],
    }


if __name__ == "__main__":
    finish_vectors(
        generate, Path(__file__).with_suffix(".json"),
        provenance=lambda: provenance(
            scope="The WM_INITDIALOG child relayout (0x0060AAB0, 0x0060C540, 0x0060C4A0 -> 0x0060C0C0 and its placement helpers) of 0x102, 0x6B, 0x105 and 0x108 at 640x480, 800x600 and 1024x768, after 0x0072EC70",
            assumptions=[
                "Children start at the 6x13 dialog-base-unit conversion of their RT_DIALOG template (MulDiv rounding) one pixel wider and taller, the runtime window size measured in retail captures; the conversion itself is Windows code and not executed",
                "The dialog is at (0, 0); no game is suspended behind the shell and the session globals are zero (no network session)",
                "Record +0x68 (the class code the subclass installer stores) is Button 0, Static 2, anything else 0xB",
            ],
            substitutions=[
                "user32 window calls are served by a window model of the dialog and its children; any other import faults",
                "Right-panel shape globals hold headers with the retail canvas sizes",
                "operator new/delete and the window-rect helper 0x00775690 are replaced",
                "Dialog-record table hash [0xAC1B18] returns bucket 0",
            ],
            entry_points={"rect_init": 0x72EC70, "child_setup": 0x60AAB0,
                          "include_set": 0x60C540, "relayout": 0x60C4A0,
                          "child_relayout": 0x60C0C0},
        ),
    )
