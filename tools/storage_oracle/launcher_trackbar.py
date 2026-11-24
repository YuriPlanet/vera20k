"""Bounded original shell trackbar projection and pointer arithmetic.

Active owner 55FC80 supplies D5/proc 55FDB0 at 55FCB3..55FCC1. That proc
sends ranges 1/2/6/10 (message 406) and positions (405) at 560139..560463.
Original RT_DIALOG D5/language 1033 declares that trackbar class for controls
52B/50F/52A (120x13 DLU) and 52F/532/536 (85x13 DLU).
Common control initialization selects 61D950 for the original ASCII class
msctls_trackbar32 (835848) at 60FC76..60FCB9; the selected handler is retained
at 60FF70 behind subclass dispatcher 610CA0. The first three launcher controls
disable the 50px plaque with 4AC; the three audio controls retain it.
Active in-game BBB owner 4E1FE0 sends 4AC=0 at 4E207F..4E2089 and
4E2128..4E2132 for GameSpeed/ScrollRate, then sets range0..6. Their resource
128x13 DLU gives the additional ordinary 192x21 plain-rail geometry.

The runtime windows are one pixel wider and taller than the resource
conversion (tools/storage_oracle/shell_relayout.py). Skirmish 0x102 (0x529,
0x511, 0x50C) and the D5 audio sliders run 129 wide, Generate Map 0x105's
players slider 0x3EB 226 and the D5 plain sliders 181. Skirmish Credits takes
its range and step from Rules [MultiplayerDialogSettings] (retail 5000..10000,
step 100: 0x4AB at 0x006AED53 with Rules+0x148C read at 0x006AED4A); 0x105
sets the players range 2..8 (0x0059722C..0x00597235).

The setup vectors run an owner's setup on a fresh window, one message per
block: 406 from the fresh position 0, then 405 with the saved value, then the
per-message step check (0x61DB94) and 400. They pin what a value outside the
range leaves and what the owner reads back.

These fixtures execute original interior blocks, not a Windows dialog. Supplied
client RECTs start at (0, 0) and maximum is positive; the range minimum and
step are zero and one except in the runtime Credits and players fixtures. We
bypass HWND lookup, OS pointer retrieval, capture admission, paint,
notifications and audio. Initial/state-set/drag projection is sampled separately;
only the original instructions produce output coordinates. No code is patched.
"""

from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX,
    UC_X86_REG_ESI, UC_X86_REG_ESP,
)

from tools.native_oracle import (
    STACK_BASE, STACK_SIZE, finish_vectors, load_image, provenance, run_checked,
)


class TrackbarFixture:
    def __init__(self):
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.uc)
        self.uc.mem_map(STACK_BASE, STACK_SIZE)
        self.stack = STACK_BASE + STACK_SIZE - 0x1000
        self.registers = self.uc.context_save()

    def put(self, offset, value):
        self.uc.mem_write(self.stack + offset, struct.pack("<i", value))

    def get(self, offset):
        return struct.unpack("<i", self.uc.mem_read(self.stack + offset, 4))[0]

    def prepare(self, width, reserve, maximum, minimum=0, step=1):
        # Restore registers and every stack slot read by the sampled blocks.
        # `maximum` is the native range: maximum minus minimum.
        self.uc.context_restore(self.registers)
        self.uc.mem_write(self.stack, bytes(0x200))
        self.uc.reg_write(UC_X86_REG_ESP, self.stack)
        self.uc.reg_write(UC_X86_REG_EBP, maximum)
        self.uc.reg_write(UC_X86_REG_ESI, minimum)  # Native range minimum.
        self.put(0x1C, step)  # Native step.
        self.put(0x20, reserve)
        self.put(0xA8, width)  # Client right; client left +0xA0 is zero.
        # Execute the original usable-span calculation and minimum clamp.
        run_checked(self.uc, 0x61DA52, 0x61DA7D, count=30,
                    required_addresses=[0x61DA68, 0x61DA6B])

    def paint_bounds(self):
        # Project stored +0x18 pixel offset to the native half-open thumb RECT.
        run_checked(self.uc, 0x61DBB9, 0x61DBD6, count=20,
                    required_addresses=[0x61DBC4, 0x61DBCF])
        return self.get(0x84), self.get(0x28)

    def position(self, width, reserve, maximum, position, path, minimum=0, step=1):
        self.prepare(width, reserve, maximum, minimum, step)
        self.uc.reg_write(UC_X86_REG_EBX, position)
        if path == "set_position":
            # 405 lParam is the value; EAX retains usable span.
            self.put(0x160, minimum + position)
            run_checked(self.uc, 0x61E486, 0x61E4A8, count=30,
                        required_addresses=[0x61E497, 0x61E499, 0x61E49D])
        elif path == "set_range":
            self.put(0x160, maximum << 16)  # 406 MAKELONG(0, maximum).
            run_checked(self.uc, 0x61E59A, 0x61E5C9, count=30,
                        required_addresses=[0x61E5AC, 0x61E5BA, 0x61E5BE])
        elif path == "initial":
            # EBX models the value returned by original TBM_GETPOS;
            # ESI/EBP model the preceding native minimum/range queries.
            self.uc.reg_write(UC_X86_REG_EBX, minimum + position)
            run_checked(self.uc, 0x61DB40, 0x61DB52, count=20,
                        required_addresses=[0x61DB46, 0x61DB4A, 0x61DB4E])
        else:
            raise ValueError(path)
        return self.paint_bounds()

    def pointer(self, width, reserve, maximum, x, path, minimum=0, step=1):
        self.prepare(width, reserve, maximum, minimum, step)
        if path == "drag":
            self.put(0x64, x)  # Client x after GetCursorPos/ScreenToClient.
            run_checked(self.uc, 0x61DC00, 0x61DC6D, count=70,
                        required_addresses=[0x61DC2B, 0x61DC2F,
                                            0x61DC54, 0x61DC58])
        elif path == "rail_click":
            # EAX is client x after LPARAM unpacking and y/thumb admission.
            # Only in-client nonnegative x values take this comparison path.
            self.uc.reg_write(UC_X86_REG_EAX, x)
            run_checked(self.uc, 0x61E545, 0x61E598, count=70,
                        required_addresses=[0x61E56C, 0x61E570,
                                            0x61E58E, 0x61E592])
        else:
            raise ValueError(path)
        value = self.uc.reg_read(UC_X86_REG_EBX)
        return (value, *self.paint_bounds())

    def setup(self, minimum, maximum, step, value, width=129, reserve=50):
        """406 MAKELONG(minimum, maximum) on a fresh window (position 0 of the
        common control's default range 0..100), then 405 `value`, then 400,
        each in its own message: the record reloads range, minimum, position
        and step between them. Returns the position and 400's result."""
        self.prepare(width, reserve, 100, 0, step)
        self.uc.reg_write(UC_X86_REG_EBX, 0)
        self.put(0x160, (maximum << 16) | minimum)
        run_checked(self.uc, 0x61E59A, 0x61E5C9, count=30,
                    required_addresses=[0x61E5AE, 0x61E5B4, 0x61E5BE])
        native_range = self.uc.reg_read(UC_X86_REG_EBP)
        native_minimum = self.uc.reg_read(UC_X86_REG_ESI)
        position = self.uc.reg_read(UC_X86_REG_EBX)

        self.prepare(width, reserve, native_range, native_minimum, step)
        self.uc.reg_write(UC_X86_REG_EBX, position)
        self.put(0x160, value)
        run_checked(self.uc, 0x61E486, 0x61E4A8, count=30,
                    required_addresses=[0x61E48F, 0x61E499])
        position = self.uc.reg_read(UC_X86_REG_EBX)

        self.prepare(width, reserve, native_range, native_minimum, step)
        self.uc.reg_write(UC_X86_REG_EBX, position)
        run_checked(self.uc, 0x61DB94, 0x61DBB1, count=10,
                    required_addresses=[0x61DB98])
        run_checked(self.uc, 0x61E4AD, 0x61E4C4, count=20,
                    required_addresses=[0x61E4B5, 0x61E4BB])
        read_back = struct.unpack("<i", struct.pack(
            "<I", self.uc.reg_read(UC_X86_REG_EAX)))[0]
        return position, read_back


# The runtime shell windows: (width, reserve, range, minimum, step).
RUNTIME = (
    (129, 50, 6, 0, 1),         # Skirmish Game Speed 0x529
    (129, 50, 5000, 5000, 100),  # Skirmish Credits 0x511, retail Rules
    (129, 50, 10, 0, 1),        # Skirmish Unit Count 0x50C, D5 audio
    (226, 50, 6, 2, 1),         # Generate Map players 0x3EB
    (181, 0, 1, 0, 1),          # D5 Detail
    (181, 0, 2, 0, 1),          # D5 Difficulty
    (181, 0, 6, 0, 1),          # D5 Scroll
)


# Owner setups: (minimum, maximum, step). Credits with retail Rules, the
# constructor defaults, a zero step and a minimum off the step; Unit Count
# retail and default; Game Speed; Generate Map's players.
SETUPS = (
    (5000, 10000, 100),
    (2500, 10000, 100),
    (5000, 10000, 0),
    (2550, 10000, 100),
    (0, 10, 1),
    (1, 20, 1),
    (0, 6, 1),
    (2, 8, 1),
)


def setup_values(minimum, maximum):
    # Both ends and their neighbours, a value off the step, and values far
    # outside any range.
    values = {minimum - 1, minimum, minimum + 1, maximum - 1, maximum,
              maximum + 1, (minimum + maximum) // 2 + 49, -50, 0, 20000}
    return sorted(values)


def sampled_positions(maximum, step):
    if step == 1:
        return range(maximum + 1)
    # Every stepped position, and the positions either side of a few steps.
    stepped = set(range(0, maximum + 1, step))
    stepped.update((1, step - 1, step + 1, maximum // 2 + 1, maximum - 1))
    return sorted(stepped)


def generate():
    fixture = TrackbarFixture()
    geometries = []
    # Preserve the original sixteen launcher arithmetic fixtures verbatim and
    # append ordinary BBB plain192/range6 and B8 numeric263/range10 controls,
    # then the runtime shell windows.
    inputs = [(width, reserve, maximum, 0, 1)
              for width in (128, 180) for reserve in (0, 50)
              for maximum in (1, 2, 6, 10)]
    inputs.extend(((192, 0, 6, 0, 1), (263, 50, 10, 0, 1)))
    inputs.extend(RUNTIME)
    for width, reserve, maximum, minimum, step in inputs:
        positions = []
        for position in sampled_positions(maximum, step):
            bounds = fixture.position(width, reserve, maximum, position,
                                      "set_position", minimum, step)
            # 406 clamps the old position against the new minimum, so it
            # projects like 405 only when the minimum is zero.
            paths = ("set_range", "initial") if minimum == 0 else ("initial",)
            for path in paths:
                other = fixture.position(width, reserve, maximum,
                                         position, path, minimum, step)
                if other != bounds:
                    raise RuntimeError(f"Native {path} disagrees: "
                                       f"{width, reserve, maximum, position}")
            positions.append({"position": position,
                              "thumb_left": bounds[0],
                              "thumb_right": bounds[1]})
        pointers = []
        for x in range(-8, width + 9):
            value, left, right = fixture.pointer(width, reserve, maximum,
                                                x, "drag", minimum, step)
            if 0 <= x < width:
                click = fixture.pointer(width, reserve, maximum,
                                        x, "rail_click", minimum, step)
                if click != (value, left, right):
                    raise RuntimeError(f"Native click/drag disagree: "
                                       f"{width, reserve, maximum, x}")
            pointers.append({"x": x, "position": value,
                             "thumb_left": left, "thumb_right": right})
        geometry = {"width": width, "reserve": reserve, "maximum": maximum,
                    "positions": positions, "pointers": pointers}
        if (minimum, step) != (0, 1):
            geometry.update(minimum=minimum, step=step)
        geometries.append(geometry)
    setups = []
    for minimum, maximum, step in SETUPS:
        for value in setup_values(minimum, maximum):
            position, read_back = fixture.setup(minimum, maximum, step, value)
            setups.append({"minimum": minimum, "maximum": maximum,
                           "step": step, "value": value,
                           "position": position, "read_back": read_back})
    return {"source": "unicorn/gamemd.exe", "geometries": geometries,
            "setups": setups}


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=lambda: provenance(
        scope=("25 supplied geometries (18 arithmetic, 7 runtime shell "
               "windows); 203 positions compared across initialization/405 "
               "(and 406 with minimum zero); 4500 drag pointer samples, "
               "including 4075 in-client samples compared with the admitted "
               "rail-click block; 77 owner-setup vectors over 8 "
               "ranges (406 from position 0, 405, the step check, 400)"),
        assumptions=[
            "Client RECT left/top zero; widths 128 and 180, plaque reserve 0 or 50; these cross combinations are arithmetic fixtures, not observed layouts",
            "Additional BBB fixture is width192/reserve0/maximum6, from 128x13 DLU and the active 4E1FE0 configuration; height21 does not enter these arithmetic blocks",
            "B8 adds width263/reserve50/maximum10 from175x13 DLU;6B6382..647F disables cue but retains the default numeric plaque",
            "Runtime fixtures use the windows retail paints, one pixel wider than the resource conversion: Skirmish 0x102 width129/reserve50 with ranges 6, 5000 (minimum 5000, step 100 from retail Rules) and 10; 0x105 players width226/reserve50/range6/minimum2; D5 plain width181/reserve0 with ranges 1/2/6 (the D5 audio range10 shares the 129 fixture)",
            "Range minimum zero and step one except the runtime Credits and players fixtures; every valid position except Credits, which samples every step and positions beside a few steps; 406 is compared only with minimum zero, because 406 clamps the old position against the new minimum; no invalid setter or zero-range claims",
            "Client x is every integer from -8 through width+8; negative/overshoot samples model capture drag, not admitted rail clicks",
            "The admitted rail-click block is compared for x in [0,width); y/old-thumb admission, HWND state lookup, Windows pointer retrieval and notifications are outside the fixture",
            "Registers and stack are reset before each block sequence; native span arithmetic supplies EAX and stack+0x10; original signed integer division executes",
            "Thumb bounds are read after native stored-offset projection; pixel appearance, repaint timing and whole-dialog equivalence are not established",
            "Setup vectors start from position 0 (the common control's default) and run 406, 405 and 400 as separate messages at width129/reserve50; the record's range, minimum and position carry between them as the tail writes them back; no zero-range or 16-bit-overflowing ranges",
        ],
        substitutions=[],
        entry_points={
            "span": 0x61DA52, "span_end": 0x61DA7D,
            "initial": 0x61DB40, "initial_end": 0x61DB52,
            "set_position": 0x61E486, "set_position_end": 0x61E4A8,
            "set_range": 0x61E59A, "set_range_end": 0x61E5C9,
            "drag": 0x61DC00, "drag_end": 0x61DC6D,
            "rail_click": 0x61E545, "rail_click_end": 0x61E598,
            "thumb_bounds": 0x61DBB9, "thumb_bounds_end": 0x61DBD6,
            "step_check": 0x61DB94, "step_check_end": 0x61DBB1,
            "get_position": 0x61E4AD, "get_position_end": 0x61E4C4,
        },
    ))
