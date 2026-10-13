"""Execute retail right-drag scrolling; no Python copy of the scroll formula.

Run: python -m tools.input_oracle.fast_scroll --check
Explicitly regenerate reviewed evidence with --write. See README.md for bounds.
"""

from __future__ import annotations

from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_HOOK_CODE, UC_MODE_32
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP,
    UC_X86_REG_FPCW, UC_X86_REG_EBX, UC_X86_REG_ESI,
)

from tools import native_oracle
from tools.bridge_click_oracle import STANDARD_Z_MULTIPLIER_BITS
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE,
    finish_vectors, load_image, provenance, run_checked,
)

RIGHT_DRAG = 0x00693440
SCROLL_MAP = 0x004A9840
DISPLAY = 0x0087F7E8
POINT = SCRATCH
VTABLE = SCRATCH + 0x1000
GET_METRICS = SCRATCH + 0x2000
CLIENT_TO_SCREEN = SCRATCH + 0x2010
SET_CURSOR_POS = SCRATCH + 0x2020
SET_CURSOR = SCRATCH + 0x2030
CANCEL_BAND = SCRATCH + 0x2040
SP = STACK_BASE + STACK_SIZE - 0x1000


def i32(machine: Uc, address: int) -> int:
    return struct.unpack("<i", machine.mem_read(address, 4))[0]


def u32(machine: Uc, address: int) -> int:
    return struct.unpack("<I", machine.mem_read(address, 4))[0]


def put32(machine: Uc, address: int, value: int) -> None:
    machine.mem_write(address, struct.pack("<I", value & 0xFFFFFFFF))


def put8(machine: Uc, address: int, value: bool | int) -> None:
    machine.mem_write(address, bytes([int(value)]))


def byte(machine: Uc, address: int) -> bool:
    return bool(machine.mem_read(address, 1)[0])


def return_from_sink(machine: Uc, argument_bytes: int, value: int = 0) -> None:
    stack = machine.reg_read(UC_X86_REG_ESP)
    destination = u32(machine, stack)
    machine.reg_write(UC_X86_REG_EAX, value)
    machine.reg_write(UC_X86_REG_ESP, stack + 4 + argument_bytes)
    machine.reg_write(UC_X86_REG_EIP, destination)


class DragFixture:
    """Original function with explicit OS and downstream camera/cursor boundaries."""

    def __init__(self, case: dict):
        self.case = case
        self.machine = machine = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(machine)
        machine.mem_map(STACK_BASE, STACK_SIZE)
        machine.mem_map(SCRATCH, SCRATCH_SIZE)
        machine.mem_map(RET_MAGIC, 0x1000)

        # Execute the actual WinMain controlfp/cache fragment instead of assuming
        # that the shared runner's legacy default is appropriate for this path.
        # Windows/MSVC initial PC53/nearest is supplied; both original calls run.
        machine.reg_write(UC_X86_REG_ESP, SP)
        machine.reg_write(UC_X86_REG_FPCW, 0x027F)
        run_checked(machine, 0x006BBFB7, 0x006BBFCE, count=1000,
                    required_addresses=[0x007CBF49, 0x007C5EE4])
        if (machine.reg_read(UC_X86_REG_FPCW) != 0x0E3F
                or u32(machine, 0x00822D80) != 0x0E3F):
            raise RuntimeError("Original WinMain did not establish PC53/chop")

        put32(machine, DISPLAY, VTABLE)
        put32(machine, VTABLE + 0x48, SET_CURSOR)
        put32(machine, VTABLE + 0xC4, CANCEL_BAND)
        put32(machine, 0x007E13EC, GET_METRICS)
        put32(machine, 0x007E14B8, CLIENT_TO_SCREEN)
        put32(machine, 0x007E144C, SET_CURSOR_POS)
        put8(machine, 0x00A8E9A0, case["game_active"])
        put8(machine, 0x00A8E378, case["tactical_active"])
        put8(machine, 0x00A8ED6B, case["map_editor"])
        put32(machine, 0x008A00A4, case["viewport"][0])
        put32(machine, 0x008A00A8, case["viewport"][1])
        put32(machine, 0x00A8EB6C, case["scroll_method"])
        put32(machine, 0x00A8EB70, case["scroll_rate"])
        # A nonzero supplied window origin makes method1/2 warp composition
        # observable. These methods are characterized, not implemented here.
        put32(machine, 0x00886FA0, 11)
        put32(machine, 0x00886FA4, 23)
        put32(machine, 0x00B73550, 0x1234)
        put32(machine, DISPLAY + 0x5550, case["anchor"][0])
        put32(machine, DISPLAY + 0x5554, case["anchor"][1])
        put8(machine, DISPLAY + 0x5558, case["crossed_before"])
        put8(machine, DISPLAY + 0x554C, case["engaged_before"])
        put8(machine, DISPLAY + 0x11CF, case["band_box"])
        self.clear_observations()
        machine.hook_add(UC_HOOK_CODE, self.sink)

    def clear_observations(self) -> None:
        self.scroll_calls: list[dict] = []
        self.cursor_id: int | None = None
        self.warp: list[int] | None = None
        self.band_box_cancel_points: list[list[int]] = []

    def sink(self, machine: Uc, address: int, _size: int, _data: object) -> None:
        stack = machine.reg_read(UC_X86_REG_ESP)
        if address == GET_METRICS:
            metric = u32(machine, stack + 4)
            if metric not in (0x44, 0x45):
                raise RuntimeError(f"Unexpected metric {metric}")
            return_from_sink(machine, 4, self.case["drag_metrics"][metric - 0x44])
        elif address == CLIENT_TO_SCREEN:
            point = u32(machine, stack + 8)
            put32(machine, point, i32(machine, point) + 101)
            put32(machine, point + 4, i32(machine, point + 4) + 202)
            return_from_sink(machine, 8, 1)
        elif address == SET_CURSOR_POS:
            self.warp = [i32(machine, stack + 4), i32(machine, stack + 8)]
            return_from_sink(machine, 8, 1)
        elif address == SET_CURSOR:
            self.cursor_id = i32(machine, stack + 4)
            return_from_sink(machine, 8)
        elif address == CANCEL_BAND:
            point = u32(machine, stack + 4)
            self.band_box_cancel_points.append([i32(machine, point), i32(machine, point + 4)])
            # This call is an observed sink: it does not invent an engagement
            # write. Native checks the original engaged byte after returning.
            return_from_sink(machine, 4)
        elif address == SCROLL_MAP:
            direction = i32(machine, stack + 4)
            distance = i32(machine, u32(machine, stack + 8))
            apply = bool(u32(machine, stack + 12))
            if direction not in (0, 2, 4, 6):
                raise RuntimeError(f"Unexpected scroll direction {direction}")
            self.scroll_calls.append({"direction": direction, "distance": distance, "apply": apply})
            if self.case.get("execute_scroll_map", False):
                # Camera sequences retain the real downstream request/probe
                # bodies. Ordinary drag rows above stop at the declared sink.
                return
            allowed = not self.case["blocked_mask"] & (1 << (direction // 2))
            return_from_sink(machine, 12, int(allowed))

    def run(self, cursor: list[int]) -> dict:
        machine = self.machine
        self.clear_observations()
        put32(machine, POINT, cursor[0])
        put32(machine, POINT + 4, cursor[1])
        put32(machine, SP, RET_MAGIC)
        put32(machine, SP + 4, POINT)
        machine.reg_write(UC_X86_REG_ESP, SP)
        machine.reg_write(UC_X86_REG_ECX, DISPLAY)
        run_checked(machine, RIGHT_DRAG, RET_MAGIC, count=5000)
        if machine.reg_read(UC_X86_REG_ESP) != SP + 8:
            raise RuntimeError("Right-drag RET 4 did not balance its argument")
        return self.snapshot()

    def snapshot(self) -> dict:
        machine = self.machine
        # Decode the native Scroll_Map command arguments for the Rust consumer;
        # no formula or expected speed is computed in this Python fixture.
        motion = [0, 0]
        for command in self.scroll_calls:
            if command["apply"]:
                axis, sign = {0: (1, -1), 2: (0, 1), 4: (1, 1), 6: (0, -1)}[command["direction"]]
                motion[axis] += sign * command["distance"]
        return {
            "crossed": byte(machine, DISPLAY + 0x5558),
            "engaged": byte(machine, DISPLAY + 0x554C),
            "scroll_calls": self.scroll_calls,
            "cursor_id": self.cursor_id,
            "warp": self.warp,
            "band_box_cancel_points": self.band_box_cancel_points,
            "motion": motion,
        }


def native_update(case: dict) -> dict:
    """Original current-pointer poll, including the call into original drag."""
    fixture = DragFixture(case)
    machine = fixture.machine
    put8(machine, DISPLAY + 0x555A, case["captured"])
    put8(machine, DISPLAY + 0x5559, case["edge_enabled"])
    put8(machine, 0x00A8ED9C, case["input_locked"])
    put32(machine, 0x00887640, DISPLAY)
    put32(machine, VTABLE + 0x34, SCRATCH + 0x2100)
    calls = []
    tested_buttons = []

    def sink(uc: Uc, address: int, _size: int, _data: object) -> None:
        stack = uc.reg_read(UC_X86_REG_ESP)
        if address == SCRATCH + 0x2100:
            result = u32(uc, stack + 4)
            put32(uc, result, case["cursor"][0] + 11)
            put32(uc, result + 4, case["cursor"][1] + 23)
            return_from_sink(uc, 4, result)
        elif address == 0x0063AB60:
            return_from_sink(uc, 8, int(case["planning_consumed"]))
        elif address == 0x0054F5C0:
            button = u32(uc, stack + 4)
            tested_buttons.append(button)
            if button not in (1, 2):
                raise RuntimeError(f"Unexpected logical mouse button {button}")
            return_from_sink(uc, 4, int(case["left_held"] if button == 1 else case["right_held"]))
        elif address == 0x004AC380:
            calls.append("band_box_move")
            return_from_sink(uc, 4)
        elif address == 0x00692B60:
            calls.append("edge_scroll")
            return_from_sink(uc, 4)
        elif address == 0x00692300:
            calls.append("process_click")
            return_from_sink(uc, 24, 0)
        elif address == RIGHT_DRAG:
            calls.append("right_drag")

    machine.hook_add(UC_HOOK_CODE, sink)
    put32(machine, SP, RET_MAGIC)
    machine.reg_write(UC_X86_REG_ESP, SP)
    machine.reg_write(UC_X86_REG_ECX, DISPLAY)
    run_checked(machine, 0x00692F30, RET_MAGIC, count=5000)
    if machine.reg_read(UC_X86_REG_ESP) != SP + 4:
        raise RuntimeError("Original update did not balance its stack")
    return {"calls": calls, "tested_buttons": tested_buttons, "drag": fixture.snapshot()}


def update_inputs() -> list[dict]:
    base = inputs("update", cursor=[419, 289], captured=True, edge_enabled=True,
                  input_locked=False, planning_consumed=False, left_held=False, right_held=True)
    cases = []
    for name, changes in (
        ("held_right", {}), ("held_both", {"left_held": True}),
        ("held_left", {"left_held": True, "right_held": False}),
        ("no_live_buttons", {"right_held": False}),
        ("uncaptured_right", {"captured": False}),
        ("planning_consumed_right", {"planning_consumed": True}),
        ("planning_consumed_uncaptured", {"planning_consumed": True, "captured": False}),
        ("locked_right", {"input_locked": True}),
        ("uncaptured_edges_disabled", {"captured": False, "edge_enabled": False}),
        ("uncaptured_editor", {"captured": False, "map_editor": True}),
        ("held_game_inactive", {"game_active": False}),
    ):
        cases.append({**base, **changes, "name": name})
    return cases


def native_message(case: dict) -> dict:
    """Original tactical receiver with native capture/latch and cleanup writes."""
    fixture = DragFixture(case)
    machine = fixture.machine
    put8(machine, DISPLAY + 0x555A, case["captured"])
    put8(machine, DISPLAY + 0x11D0, True)
    put8(machine, 0x00A8ED5C, case["scenario_started"])
    put8(machine, 0x00A8ED9C, case["input_locked"])
    put8(machine, DISPLAY + 0x555C, True)
    put32(machine, 0x00887640, DISPLAY)
    put32(machine, 0x00887324, SCRATCH + 0x5000)
    put32(machine, VTABLE + 0x10, SCRATCH + 0x2200)
    put32(machine, VTABLE + 0x8C, 0x004AEAD0)
    put32(machine, 0x007E13FC, SCRATCH + 0x2210)
    put32(machine, 0x007E13F8, SCRATCH + 0x2220)
    calls = []

    def sink(uc: Uc, address: int, _size: int, _data: object) -> None:
        if address == 0x00692300:
            calls.append("process_click")
            return_from_sink(uc, 24, int(case["click_admitted"]))
        elif address in (0x0063AAC0, 0x0063AB00):
            calls.append("planning_down" if address == 0x0063AAC0 else "planning_up")
            return_from_sink(uc, 8)
        elif address == 0x004AAD30:
            calls.append("cancel_selection_or_mode")
            return_from_sink(uc, 4)
        elif address == 0x004AEAD0:
            calls.append("native_cleanup")
        elif address == 0x006DA160:
            calls.append("native_clear_band_rect")
        elif address == SCRATCH + 0x2200:
            calls.append("display_flush")
            return_from_sink(uc, 0)
        elif address == SCRATCH + 0x2210:
            calls.append("set_capture")
            return_from_sink(uc, 4)
        elif address == SCRATCH + 0x2220:
            calls.append("release_capture")
            return_from_sink(uc, 0)

    machine.hook_add(UC_HOOK_CODE, sink)
    put32(machine, POINT, case["message"])
    packed = case["capture_destination"] if case["message"] == 0x215 else (
        (case["event_cursor"][0] & 0xFFFF) | ((case["event_cursor"][1] & 0xFFFF) << 16))
    put32(machine, POINT + 4, packed)
    for index, value in enumerate([RET_MAGIC, 0, POINT, 0, POINT + 4]):
        put32(machine, SP + index * 4, value)
    machine.reg_write(UC_X86_REG_ESP, SP)
    machine.reg_write(UC_X86_REG_ECX, DISPLAY)
    run_checked(machine, 0x006930A0, RET_MAGIC, count=5000)
    if machine.reg_read(UC_X86_REG_ESP) != SP + 20:
        raise RuntimeError("Original mouse receiver did not balance RET16")
    return {
        "calls": calls, "captured": byte(machine, DISPLAY + 0x555A),
        "crossed": byte(machine, DISPLAY + 0x5558),
        "engaged": byte(machine, DISPLAY + 0x554C),
        "anchor": [i32(machine, DISPLAY + 0x5550), i32(machine, DISPLAY + 0x5554)],
        "band_box": byte(machine, DISPLAY + 0x11CF),
        "band_armed": byte(machine, DISPLAY + 0x11D0),
        "cursor_id": fixture.cursor_id,
    }


def message_inputs() -> list[dict]:
    base = inputs("message", captured=False, scenario_started=True, input_locked=False,
                  click_admitted=True, message=0x204, event_cursor=[431, 363],
                  capture_destination=0x4567)
    cases = []
    for name, changes in (
        ("right_press", {}),
        ("right_press_resets_threshold", {"crossed_before": True, "engaged_before": True}),
        ("right_press_already_captured", {"captured": True}),
        ("right_press_invalid_point", {"click_admitted": False}),
        ("right_press_locked", {"input_locked": True}),
        ("right_press_game_inactive", {"game_active": False}),
        ("right_press_tactical_inactive", {"tactical_active": False}),
        ("right_press_before_scenario", {"scenario_started": False}),
        ("right_press_editor_before_scenario", {"scenario_started": False, "map_editor": True}),
        ("right_release_no_capture", {"message": 0x205}),
        ("right_click_release", {"message": 0x205, "captured": True}),
        ("right_drag_release", {"message": 0x205, "captured": True,
                                "crossed_before": True, "engaged_before": True}),
        ("right_release_band_box", {"message": 0x205, "captured": True, "band_box": True}),
        ("capture_changed", {"message": 0x215, "captured": True, "band_box": True}),
        ("capture_changed_locked", {"message": 0x215, "captured": True,
                                    "band_box": True, "input_locked": True}),
        ("capture_same_window", {"message": 0x215, "captured": True, "capture_destination": 0x1234}),
        ("middle_press", {"message": 0x207}),
        ("middle_release", {"message": 0x208, "captured": True}),
    ):
        cases.append({**base, **changes, "name": name})
    return cases


def inputs(name: str, **changes) -> dict:
    case = {
        "name": name, "viewport": [800, 600], "anchor": [400, 300],
        "cursor": [409, 300], "scroll_rate": 3, "scroll_method": 0,
        "drag_metrics": [4, 4], "crossed_before": False,
        "engaged_before": False, "band_box": False, "game_active": True,
        "tactical_active": True, "map_editor": False, "blocked_mask": 0,
    }
    case.update(changes)
    return case


def input_cases() -> list[dict]:
    cases = []
    for rate in range(7):
        for delta in (-49, -22, -9, -8, -7, -1, 0, 1, 7, 8, 9, 22, 49):
            for axis in range(2):
                cursor = [400, 300]
                cursor[axis] += delta
                cases.append(inputs(f"rate{rate}_axis{axis}_delta{delta}",
                                    scroll_rate=rate, cursor=cursor))
        for dx, dy in ((9, 9), (-9, 9), (9, -9), (-9, -9), (1, 9), (-1, -9)):
            cases.append(inputs(f"rate{rate}_diagonal_{dx}_{dy}", scroll_rate=rate,
                                cursor=[400 + dx, 300 + dy]))
        for edge, anchor, delta in (
            ("left", [0, 300], [-1, 0]), ("right", [799, 300], [1, 0]),
            ("top", [400, 0], [0, -1]), ("bottom", [400, 599], [0, 1]),
        ):
            # Already crossed: only the original per-frame edge boost is under
            # examination here, independently of OS drag metrics.
            for scale in (0, 1, 4, 5, 6, 9):
                cursor = [anchor[i] + scale * delta[i] for i in range(2)]
                cases.append(inputs(f"rate{rate}_{edge}_{scale}", scroll_rate=rate,
                                    anchor=anchor, cursor=cursor, crossed_before=True))

    # Native reciprocal-multiply/truncate boundaries. Crossed input isolates
    # arithmetic for tiny displacements and both signs. Large synthetic pointer
    # displacements characterize arithmetic, not actual desktop geometry.
    for divisor in range(1, 8):
        for quotient in (1, 2, 3, 7, 49, 100, 1000):
            for offset in (-1, 0, 1):
                for sign in (-1, 1):
                    delta = sign * (divisor * quotient + offset)
                    cases.append(inputs(f"reciprocal_d{divisor}_q{quotient}_o{offset}_s{sign}",
                                        scroll_rate=divisor - 1, cursor=[400 + delta, 300],
                                        crossed_before=True))

    for name, anchor, cursor in (
        ("left_near9_out", [9, 300], [8, 300]),
        ("left_near10_out", [10, 300], [9, 300]),
        ("left_near9_in", [9, 300], [10, 300]),
        ("right_near790_out", [790, 300], [791, 300]),
        ("right_near791_out", [791, 300], [792, 300]),
        ("right_near791_in", [791, 300], [790, 300]),
        ("top_near9_out", [400, 9], [400, 8]),
        ("top_near10_out", [400, 10], [400, 9]),
        ("top_near9_in", [400, 9], [400, 10]),
        ("bottom_near590_out", [400, 590], [400, 591]),
        ("bottom_near591_out", [400, 591], [400, 592]),
        ("bottom_near591_in", [400, 591], [400, 590]),
        ("bottom_at_height_stationary", [400, 600], [400, 600]),
        ("top_left_corner_stationary", [0, 0], [0, 0]),
        ("bottom_right_corner_stationary", [799, 599], [799, 599]),
        ("left_anchor_cursor_far_inside", [0, 300], [40, 300]),
        ("inside_anchor_cursor_left_edge", [40, 300], [0, 300]),
        ("inside_anchor_cursor_right_edge", [760, 300], [799, 300]),
    ):
        cases.append(inputs(name, anchor=anchor, cursor=cursor, crossed_before=True))

    for delta in (5, 6, 7, 11, 12, 13):
        for axis in range(2):
            cursor = [400, 300]
            cursor[axis] += delta
            cases.append(inputs(f"custom_metrics_axis{axis}_{delta}",
                                drag_metrics=[3, 6], cursor=cursor))
    for name, changes in (
        ("game_inactive", {"game_active": False}),
        ("tactical_inactive", {"tactical_active": False}),
        ("editor_without_tactical", {"tactical_active": False, "map_editor": True}),
        ("inactive_even_with_editor", {"game_active": False, "map_editor": True}),
        ("band_box_threshold", {"band_box": True}),
        ("band_box_under_threshold", {"band_box": True, "cursor": [408, 300]}),
        ("band_box_already_engaged", {"band_box": True, "engaged_before": True}),
        ("already_engaged_under_threshold", {"engaged_before": True, "cursor": [401, 300]}),
        ("already_crossed_tiny_motion", {"crossed_before": True, "cursor": [401, 300]}),
        ("already_crossed_stationary", {"crossed_before": True, "cursor": [400, 300]}),
    ):
        cases.append(inputs(name, **changes))
    for blocked_mask in range(16):
        cases.append(inputs(f"blocked_mask{blocked_mask}", blocked_mask=blocked_mask,
                            cursor=[409, 291]))
    for method in (1, 2):
        for rate in range(7):
            cases.append(inputs(f"method{method}_rate{rate}", scroll_method=method,
                                scroll_rate=rate, cursor=[409, 291]))
        cases.append(inputs(f"method{method}_edge", scroll_method=method,
                            anchor=[0, 0], cursor=[0, 0], crossed_before=True))
        cases.append(inputs(f"method{method}_stationary", scroll_method=method,
                            cursor=[400, 300], crossed_before=True))
    return cases


def native_throttle(case: dict) -> dict:
    """Original throttle through 0x55E33B; FPS bookkeeping after it is excluded."""
    machine = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(machine)
    machine.mem_map(STACK_BASE, STACK_SIZE)
    machine.mem_map(SCRATCH, SCRATCH_SIZE)
    machine.reg_write(UC_X86_REG_ESP, SP)
    put32(machine, 0x00A8B238, case["session_mode"])
    put32(machine, 0x00887348, 0)
    put32(machine, 0x00887350, case["duration"])
    put32(machine, 0x00887328, 0)
    put32(machine, 0x00887330, case["duration"])
    put32(machine, 0x00A8E314, 0)
    put32(machine, 0x00A8EDA0, case["game_state"])
    put8(machine, 0x00A8ED80, case["app_active"])
    put32(machine, 0x007E11F0, SCRATCH + 0x3000)
    put32(machine, 0x00887324, SCRATCH)
    put32(machine, SCRATCH, VTABLE)
    put32(machine, VTABLE + 0x5C, SCRATCH + 0x3010)
    clocks = {0x006C8C40: iter(case["frame_clock"]),
              0x005D5890: iter(case["millisecond_clock"])}
    observed = {"input_calls": 0, "network_service_calls": 0,
                "offline_service_calls": 0, "sleep_calls": [],
                "command_calls": 0, "tactical_calls": 0, "render_calls": 0}

    def sink(uc: Uc, address: int, _size: int, _data: object) -> None:
        if address in clocks:
            try:
                value = next(clocks[address])
            except StopIteration as error:
                raise RuntimeError(f"Unexpected extra clock read at {address:#x}") from error
            return_from_sink(uc, 0, value)
        elif address == SCRATCH + 0x3000:
            observed["sleep_calls"].append(u32(uc, uc.reg_read(UC_X86_REG_ESP) + 4))
            return_from_sink(uc, 4)
        else:
            boundaries = {
                0x004F4320: ("input_calls", 12),
                0x0048D080: ("network_service_calls", 0),
                0x004A4830: ("offline_service_calls", 0),
                0x0055DEE0: ("command_calls", 0),
                SCRATCH + 0x3010: ("tactical_calls", 0),
                0x004F4480: ("render_calls", 0),
            }
            if address in boundaries:
                key, argument_bytes = boundaries[address]
                observed[key] += 1
                return_from_sink(uc, argument_bytes)

    machine.hook_add(UC_HOOK_CODE, sink)
    run_checked(machine, 0x0055E160, 0x0055E33B, count=2000,
                required_addresses=[0x0055E197])
    for address, values in clocks.items():
        if list(values):
            raise RuntimeError(f"Supplied clock values unused at {address:#x}")
    observed["accumulated_wait"] = i32(machine, 0x00A8E314)
    return observed


def throttle_inputs() -> list[dict]:
    cases = []
    for mode in range(7):
        for duration in (0, 6, 10, 11, 20):
            offline = mode in (0, 5)
            cases.append({
                "name": f"session{mode}_wait{duration}", "session_mode": mode,
                "duration": duration, "game_state": 0, "app_active": True,
                "frame_clock": ([0, 0] if duration == 0 else [0, 0, 0, 0, duration])
                               if offline else [0],
                "millisecond_clock": [] if offline else ([0] if duration == 0 else [0, 0, duration]),
            })
    for game_state, app_active in ((1, True), (0, False)):
        cases.append({
            "name": f"session3_gate_state{game_state}_active{app_active}",
            "session_mode": 3, "duration": 20, "game_state": game_state,
            "app_active": app_active, "frame_clock": [0],
            "millisecond_clock": [0, 20],
        })
    return cases


class CameraFixture(DragFixture):
    """Retain native requests across calls, then execute native AI's commit."""

    tactical = SCRATCH + 0x4000

    def __init__(self, case: dict):
        super().__init__(inputs(case["name"], viewport=case["viewport"],
                                anchor=case["drag_anchor"],
                                scroll_rate=case["scroll_rate"],
                                drag_metrics=case["drag_metrics"],
                                execute_scroll_map=True))
        machine = self.machine
        self.refresh_calls = 0
        self.exit_registrations = 0
        self.native_case = case
        self.stream_payload = bytearray()
        self.stream_position = 0
        self.stream_calls: list[dict] = []
        self.swizzle_tokens: list[int] = []
        # The same-frame condition skips unrelated focus animation/timer work
        # while retaining the entire original pending-view commit and clamp.
        for address, value in (
            (0x0087F8DC, case["map_rect_width"]),
            (0x0087F8E4, case["local_size"][0]),
            (0x0087F8E8, case["local_size"][1]),
            (0x0087F8EC, case["local_size"][2]),
            (0x0087F8F0, case["local_size"][3]),
            (0x00886FA8, case["viewport"][0]),
            (0x00886FAC, case["viewport"][1]),
            (0x00887324, self.tactical),
            (0x00A8ED84, 0), (0x00A8D5F8, 0),
            (self.tactical + 0xA8, 0),
        ):
            put32(machine, address, value)
        # Existing native projection fixture owns this startup value. These
        # rows use Z=0 and do not broaden its established height coverage.
        machine.mem_write(0x00B0CD48, struct.pack("<Q", STANDARD_Z_MULTIPLIER_BITS))
        if case.get("focus_sequence"):
            put8(machine, 0x00A8ED5C, case.get("scenario_started", True))
            # Keep the unrelated wall-clock waypoint animation stopped with
            # nonzero remaining time. Focus still executes the original full
            # TacticalAI body, interpolation, clamp and view publication.
            put32(machine, self.tactical + 0xDA4, -1)
            put32(machine, self.tactical + 0xDAC, 1)
        for axis in range(2):
            put32(machine, POINT + axis * 4, case["initial_center_seed"][axis])
        self.invoke(0x006D8640, [POINT])
        self.initial_center = [i32(machine, POINT), i32(machine, POINT + 4)]
        for offset in (0xD64, 0xD74):
            for axis in range(2):
                put32(machine, self.tactical + offset + axis * 4, self.initial_center[axis])

    def sink(self, machine: Uc, address: int, size: int, data: object) -> None:
        super().sink(machine, address, size, data)
        if address == 0x007C978A:
            self.exit_registrations += 1
            return_from_sink(machine, 0)
        elif address == 0x006D8B30:
            self.refresh_calls += 1
            return_from_sink(machine, 0)
        elif address == 0x006C8C40:
            # An explicit wall-clock bucket. The waypoint draw timer is kept
            # stopped during measured AI visits; it is not the focus clock.
            return_from_sink(machine, 0, 0)
        elif address == 0x006CF2C0:
            # Abstract Load's Here_I_Am boundary. These Tactical scenes have
            # no VisibleCell pointers to swizzle; record the saved-this token.
            stack = machine.reg_read(UC_X86_REG_ESP)
            self.swizzle_tokens.append(u32(machine, stack + 8))
            return_from_sink(machine, 12)
        elif address in (SCRATCH + 0x9100, SCRATCH + 0x9110):
            stack = machine.reg_read(UC_X86_REG_ESP)
            receiver, pointer, length, actual = struct.unpack(
                "<4I", machine.mem_read(stack + 4, 16))
            if receiver != SCRATCH + 0x9000:
                raise RuntimeError("Unexpected camera IStream receiver")
            if address == SCRATCH + 0x9110:
                self.stream_payload.extend(machine.mem_read(pointer, length))
                operation = "write"
            else:
                block = bytes(self.stream_payload[self.stream_position:self.stream_position + length])
                if len(block) != length:
                    raise RuntimeError("Original camera Load over-read its saved bytes")
                machine.mem_write(pointer, block)
                self.stream_position += length
                operation = "read"
            if actual:
                put32(machine, actual, length)
            self.stream_calls.append({"operation": operation, "length": length})
            return_from_sink(machine, 16)

    def invoke(self, address: int, arguments: list[int], *, receiver: int | None = None) -> None:
        machine = self.machine
        for index, value in enumerate([RET_MAGIC, *arguments]):
            put32(machine, SP + index * 4, value)
        machine.reg_write(UC_X86_REG_ESP, SP)
        machine.reg_write(UC_X86_REG_ECX, self.tactical if receiver is None else receiver)
        run_checked(machine, address, RET_MAGIC, count=100000)
        if machine.reg_read(UC_X86_REG_ESP) != SP + 4 * (len(arguments) + 1):
            raise RuntimeError(f"Camera entry {address:#x} did not balance arguments")

    def advance(self, step: dict) -> dict:
        machine = self.machine
        self.clear_observations()
        self.refresh_calls = 0
        operation = step["operation"]
        drag = None
        if operation == "constructor":
            self.invoke(0x006D1C20, [])
            # Keep the unrelated waypoint-display wall timer stopped, as in
            # the preexisting focus controls, after observing the real ctor.
            put32(machine, self.tactical + 0xDA4, -1)
            put32(machine, self.tactical + 0xDAC, 1)
        elif operation == "save_restore":
            stream, table = SCRATCH + 0x9000, SCRATCH + 0x9020
            put32(machine, stream, table)
            put32(machine, table + 0xC, SCRATCH + 0x9100)
            put32(machine, table + 0x10, SCRATCH + 0x9110)
            self.stream_payload.clear()
            self.stream_position = 0
            self.stream_calls.clear()
            self.swizzle_tokens.clear()
            self.invoke(0x006DBE00, [self.tactical, stream, 0])
            if machine.reg_read(UC_X86_REG_EAX) != 0:
                raise RuntimeError("Original Tactical Save failed")
            # Destroy only the receiving object's measured fields, so success
            # demonstrates actual stream retention rather than an unread copy.
            for offset in (0xA8, 0xC8, 0xCC, 0xD0, 0xD4, 0xD8, 0xDC, 0xD64, 0xD68, 0xD74, 0xD78):
                put32(machine, self.tactical + offset, 0xA5A5A5A5)
            if "local_size" in step:
                for axis, value in enumerate(step["local_size"]):
                    put32(machine, 0x0087F8E4 + axis * 4, value)
            self.invoke(0x006DBD20, [self.tactical, stream])
            if machine.reg_read(UC_X86_REG_EAX) != 0:
                raise RuntimeError("Original Tactical Load failed")
            if self.stream_position != len(self.stream_payload):
                raise RuntimeError("Original camera Load left saved bytes unread")
            # No-init reconstructs the separate waypoint-display wall timer.
            # Keep it stopped for the following focus-only AI measurements.
            put32(machine, self.tactical + 0xDA4, -1)
            put32(machine, self.tactical + 0xDAC, 1)
        elif operation == "scroll":
            put32(machine, POINT + 0x20, step["distance"])
            self.invoke(SCROLL_MAP, [step["direction"], POINT + 0x20, 1], receiver=DISPLAY)
        elif operation == "drag":
            drag = self.run(step["cursor"])
        elif operation == "commit":
            if "frame" in step:
                put32(machine, 0x00A8ED84, step["frame"])
            self.invoke(0x006D2540, [])
        elif operation == "focus":
            for axis, value in enumerate(step["coord"]):
                put32(machine, POINT + axis * 4, value)
            self.invoke(0x006D2420, [POINT, step["velocity"]])
        elif operation == "scenario_started":
            put8(machine, 0x00A8ED5C, step["value"])
        elif operation == "replay_flags":
            put32(machine, 0x00A8D5F8, step["value"])
        elif operation == "set_view":
            if step["coord"][2] != 0:
                raise ValueError("Camera lifecycle rows deliberately bound SetView to Z=0")
            for axis, value in enumerate(step["coord"]):
                put32(machine, POINT + axis * 4, value)
            self.invoke(0x006D6070, [POINT])
        else:
            raise ValueError(f"Unknown camera operation {operation}")
        output = {
            "committed_center": [i32(machine, self.tactical + 0xD64),
                                 i32(machine, self.tactical + 0xD68)],
            "requested_center": [i32(machine, self.tactical + 0xD74),
                                 i32(machine, self.tactical + 0xD78)],
            "refresh_calls": self.refresh_calls,
        }
        if drag is not None:
            output["drag"] = drag
        if operation == "save_restore":
            output["stream"] = {"calls": self.stream_calls.copy(),
                                "bytes": len(self.stream_payload),
                                "consumed": self.stream_position,
                                "swizzle_saved_tokens": self.swizzle_tokens.copy()}
        if self.native_case.get("focus_sequence"):
            output["focus"] = {
                "start": [i32(machine, self.tactical + 0xC8),
                          i32(machine, self.tactical + 0xCC)],
                "target": [i32(machine, self.tactical + 0xD0),
                           i32(machine, self.tactical + 0xD4)],
                "rate_bits": u32(machine, self.tactical + 0xD8),
                "progress_bits": u32(machine, self.tactical + 0xDC),
                "last_ai_frame": u32(machine, self.tactical + 0xA8),
                "frame": u32(machine, 0x00A8ED84),
            }
        return output


def camera_inputs() -> list[dict]:
    def scroll(direction: int, distance: int, label: str = "supplied_request") -> dict:
        return {"operation": "scroll", "direction": direction,
                "distance": distance, "label": label}

    commit = {"operation": "commit"}
    drag_left = {"operation": "drag", "cursor": [220, 240]}
    drag_right = {"operation": "drag", "cursor": [420, 240]}
    set_view = {"operation": "set_view", "coord": [25600, 25600, 0]}
    cases = (
        ("west_prior_keyboard_then_mouse_single_commit", [-999999, 2000],
         [scroll(2, 21, "prior_frame_keyboard_request"), drag_left, commit]),
        ("west_same_requests_separate_commit_control", [-999999, 2000],
         [drag_left, commit, scroll(2, 21), commit]),
        ("east_prior_keyboard_then_mouse_single_commit", [999999, 2000],
         [scroll(6, 21, "prior_frame_keyboard_request"), drag_right, commit]),
        ("interior_opposing_requests_cancel", [0, 2500],
         [scroll(2, 25), scroll(6, 25), commit]),
        ("interior_axes_accumulate_then_commit", [0, 2500],
         [scroll(2, 21), scroll(0, 25), scroll(6, 9), scroll(4, 7), commit]),
        ("keyboard_after_commit_retained_until_next_mouse", [-999999, 2000],
         [commit, scroll(2, 21, "post_AI_keyboard_request"), drag_left, commit]),
        ("absolute_view_overwrites_uncommitted_request", [0, 2500],
         [scroll(2, 21), set_view, commit]),
        ("absolute_view_after_commit_replaces_camera", [-999999, 2000],
         [drag_left, commit, set_view, commit]),
        ("absolute_view_then_fresh_request", [0, 2500],
         [scroll(2, 21), set_view, scroll(6, 9), commit]),
        ("absolute_view_same_center_still_discards_request", [0, 3000],
         [scroll(2, 21), set_view, commit]),
    )
    return [{"name": name, "viewport": [640, 480], "map_rect_width": 100,
             "local_size": [0, 0, 100, 100], "initial_center_seed": seed,
             "drag_anchor": [320, 240], "scroll_rate": 3, "drag_metrics": [4, 4],
             "steps": steps} for name, seed, steps in cases]


def native_camera_sequence(case: dict) -> dict:
    fixture = CameraFixture(case)
    return {**case, "initial_center": fixture.initial_center,
            "steps": [{**step, "expected": fixture.advance(step)} for step in case["steps"]]}


def focus_inputs() -> list[dict]:
    def focus(velocity: int, coord: list[int] | None = None) -> dict:
        return {"operation": "focus", "velocity": velocity,
                "coord": coord or [25600, 25600, 0]}

    def commit(frame: int) -> dict:
        return {"operation": "commit", "frame": frame}

    # Timelines are inputs; no second interpolation algorithm computes the
    # outputs or completion frame. Capture original float stores on each visit.
    cases = [(f"focus_velocity_{velocity}",
              [focus(velocity), commit(0), commit(1), commit(1)]
              + [commit(frame) for frame in range(2, horizon)])
             for velocity, horizon in enumerate((710, 370, 170, 70, 50))]
    cases += [
        ("focus_retarget_keeps_same_frame_gate",
         [focus(4), commit(1), commit(2), focus(3, [12800, 25600, 0]),
          commit(2), commit(3), commit(4), commit(40)]),
        ("focus_during_unstarted_scenario",
         [{"operation": "scenario_started", "value": False}, focus(4),
          commit(1), commit(2), {"operation": "scenario_started", "value": True},
          commit(2), commit(3)]),
        ("focus_replay_gate_retains_state",
         [focus(4), {"operation": "replay_flags", "value": 2}, commit(1),
          {"operation": "replay_flags", "value": 0}, commit(1), commit(1)]),
        ("focus_clamps_outside_target",
         [focus(4, [256000, 256000, 0])] + [commit(frame) for frame in range(1, 20)]),
        ("focus_clamps_north",
         [focus(4, [-256000, -256000, 0])] + [commit(frame) for frame in range(1, 21)]),
        ("focus_clamps_east",
         [focus(4, [256000, -256000, 0])] + [commit(frame) for frame in range(1, 21)]),
        ("focus_clamps_west",
         [focus(4, [-256000, 256000, 0])] + [commit(frame) for frame in range(1, 21)]),
        ("focus_overwrites_pending_scroll",
         [{"operation": "scroll", "direction": 2, "distance": 21},
          focus(4), commit(1), {"operation": "scroll", "direction": 6, "distance": 21},
          commit(2)]),
        ("absolute_view_during_focus_keeps_original_trajectory",
         [focus(4), commit(1),
          {"operation": "set_view", "coord": [12800, 25600, 0]},
          commit(1), commit(2)]),
        ("focus_zero_target_is_inactive",
         [focus(4, [0, 0, 0]), commit(1), commit(2)]),
        ("focus_wrapping_frame_comparison",
         [focus(4), commit(0xFFFFFFFF), commit(0), commit(0), commit(1)]),
    ]
    for height in (-104, 727, 728, 1500):
        cases.append((f"focus_subcell_height_{height}",
                      [focus(4, [25601, 24317, height])]
                      + [commit(frame) for frame in range(1, 21)]))
    rows = [{"name": name, "focus_sequence": True, "viewport": [640, 480],
             "map_rect_width": 100, "local_size": [0, 0, 100, 100],
             "initial_center_seed": [0, 2500], "drag_anchor": [320, 240],
             "scroll_rate": 3, "drag_metrics": [4, 4], "steps": steps}
            for name, steps in cases]
    for name, map_width, local_size in (
        ("focus_nonzero_local_rect", 58, [4, 29, 52, 60]),
        ("focus_view_larger_than_local_rect", 10, [0, 0, 4, 4]),
    ):
        rows.append({**rows[0], "name": name, "map_rect_width": map_width,
                     "local_size": local_size,
                     "steps": [focus(4)] + [commit(frame) for frame in range(1, 21)]})
    return rows


def camera_lifecycle_inputs() -> list[dict]:
    base = {**focus_inputs()[0], "focus_sequence": True}
    constructor = {"operation": "constructor"}
    focus = {"operation": "focus", "coord": [25600, 25600, 0], "velocity": 4}
    commit = lambda frame: {"operation": "commit", "frame": frame}
    scroll = {"operation": "scroll", "direction": 2, "distance": 21}
    restore = {"operation": "save_restore"}
    cases = (
        ("real_ctor_frame_zero_is_admitted", [constructor, commit(0), commit(0), focus, commit(1)]),
        ("real_save_load_mid_focus_discards_pending_request", [constructor, focus, commit(1), scroll, restore, commit(1), commit(2), commit(3)]),
        ("real_save_load_completed_focus_stays_clean", [constructor, focus, *[commit(n) for n in range(1, 18)], restore, commit(18)]),
        ("real_save_load_reclamps_view_retains_focus", [constructor, focus, commit(1), {**restore, "local_size": [4, 29, 52, 60]}, commit(2)]),
    )
    return [{**base, "name": name, "steps": steps} for name, steps in cases]


def code_guard(u):
    return [(native_oracle.IMAGE_BASE + rva, bytes(u.mem_read(native_oracle.IMAGE_BASE + rva, size)))
            for rva, _raw, size, _virtual, flags in native_oracle._sections(native_oracle.image_bytes())
            if flags & 0x20000000 and size]


def check_code(u, guard):
    assert all(bytes(u.mem_read(at, len(raw))) == raw for at, raw in guard)


class SavedViewFixture(CameraFixture):
    def sink(self, u, pc, size, data):
        if pc == 0x6D8B30:
            # Deliberately remove the existing camera fixture's refresh sink.
            # The original updater and projection arithmetic now run.
            self.refresh_calls += 1
            return
        if pc == 0x6C8C40:
            return_from_sink(u, 0, 123)
            return
        if pc in (0x6CF240, 0x6CF2C0):
            self.registrations.append(hex(pc))
            return_from_sink(u, 8 if pc == 0x6CF240 else 12, 0)
            return
        if pc in (SCRATCH + 0x20200, SCRATCH + 0x20210):
            sp = u.reg_read(UC_X86_REG_ESP)
            receiver, pointer, length, out = struct.unpack('<4I', u.mem_read(sp + 4, 16))
            assert receiver == self.stream
            self.io.append(dict(operation='read' if pc == SCRATCH + 0x20200 else 'write', bytes=length))
            if pc == SCRATCH + 0x20210:
                self.payload.extend(u.mem_read(pointer, length))
            else:
                assert self.cursor + length <= len(self.payload)
                u.mem_write(pointer, bytes(self.payload[self.cursor:self.cursor + length]))
                self.cursor += length
            if out:
                put32(u, out, length)
            return_from_sink(u, 16, 0)
            return
        super().sink(u, pc, size, data)


def native_saved_view(spec):
    setup = dict(camera_inputs()[0])
    setup['name'] = spec['name']
    setup['initial_center_seed'] = [0, 2500]
    setup['viewport'] = spec['viewport']
    setup['map_rect_width'] = spec['map_rect_width']
    setup['local_size'] = spec['local_size']
    f = SavedViewFixture(setup)
    u, obj = f.machine, f.tactical
    u.mem_map(SCRATCH + 0x20000, 0x1000)
    f.registrations, f.payload, f.io, f.cursor = [], bytearray(), [], 0
    f.stream = SCRATCH + 0x20000
    put32(u, f.stream, SCRATCH + 0x20100)
    put32(u, SCRATCH + 0x2010C, SCRATCH + 0x20200)
    put32(u, SCRATCH + 0x20110, SCRATCH + 0x20210)
    guard = code_guard(u)
    u.reg_write(UC_X86_REG_ESI, obj)
    u.reg_write(UC_X86_REG_EBX, 0)
    native_oracle.run_checked(u, 0x6D1DC5, 0x6D1E24)
    put32(u, obj, 0x7F4348)
    put32(u, obj + 0xE0, 0)  # Empty unidentified saved pointer region.
    put32(u, 0xB0CE30, spec['viewport'][0])
    put32(u, 0xB0CE34, spec['viewport'][1])
    u.mem_write(0xA8ED6B, bytes([spec['map_editor']]))
    u.mem_write(obj + 0xD64, struct.pack('<ii', *spec['saved_center']))
    u.mem_write(obj + 0xD74, struct.pack('<ii', *spec['saved_requested']))
    u.mem_write(obj + 0xB0, struct.pack('<ii', 123456, -123456))
    u.mem_write(obj + 0xD80, b'\x5A' * 16)
    f.invoke(0x6DBE00, [obj, f.stream, 0])
    saved_size = len(f.payload)
    # Distinct outgoing view, overwritten by actual native IStream Load.
    u.mem_write(obj + 0xD64, struct.pack('<ii', 6789, 8765))
    u.mem_write(obj + 0xD74, struct.pack('<ii', -6789, -8765))
    f.refresh_calls = 0
    f.invoke(0x6DBD20, [obj, f.stream])

    def state():
        return dict(committed=[i32(u, obj + 0xD64), i32(u, obj + 0xD68)],
                    requested=[i32(u, obj + 0xD74), i32(u, obj + 0xD78)],
                    top_left=[i32(u, obj + 0xB0), i32(u, obj + 0xB4)],
                    visible_cells=list(struct.unpack('<4i', u.mem_read(obj + 0xD80, 16))),
                    redraw=u.mem_read(obj + 0xD7D, 1)[0],
                    refresh_calls=f.refresh_calls)

    loaded = state()
    resized = None
    if spec.get('resize'):
        width, height = spec['resize']
        put32(u, 0x886FA8, width)
        put32(u, 0x886FAC, height)
        rect = SCRATCH + 0x20300
        u.mem_write(rect, struct.pack('<4i', 0, 0, width, height))
        f.invoke(0x6D5F60, [rect])
        resized = state()
    check_code(u, guard)
    assert saved_size == 0xE1C and f.cursor == saved_size
    return dict(input=spec, loaded=loaded, resized=resized, stream_bytes=saved_size,
                io=f.io, pointer_registration=f.registrations,
                code_unchanged=True)


def saved_view_inputs():
    cases = []
    for name, center, editor, width, local, viewport, resize in [
        ('interior_pending_discarded', [0, 2500], 0, 100, [0,0,100,100], [640,480], None),
        ('west_new_map_clamp', [-9999,2500], 0, 100, [0,0,100,100], [640,480], None),
        ('north_new_map_clamp', [0,-9999], 0, 100, [0,0,100,100], [640,480], None),
        ('south_east_new_map_clamp', [9999,9999], 0, 100, [0,0,100,100], [640,480], None),
        ('offset_local_rectangle', [-9000,9999], 0, 90, [5,7,60,70], [800,600], None),
        ('map_editor_retains_outside', [-9999,9999], 1, 100, [0,0,100,100], [640,480], None),
        ('viewport_resize_reclamps_saved_center', [-9999,2500], 0, 100, [0,0,100,100], [640,480], [1024,768]),
        ('odd_viewport_halves', [0,2500], 0, 100, [0,0,100,100], [641,481], None),
        ('odd_viewport_west_clamp', [-9999,2500], 0, 100, [0,0,100,100], [641,481], None),
        ('odd_viewport_east_clamp', [9999,2500], 0, 100, [0,0,100,100], [641,481], None),
        ('odd_viewport_south_clamp', [0,9999], 0, 100, [0,0,100,100], [641,481], None),
        ('normalized_fields_north_clamp', [0,-9999], 0, 100, [2,5,96,89], [640,480], None),
        ('normalized_fields_south_clamp', [0,9999], 0, 100, [2,5,96,89], [640,480], None),
    ]:
        cases.append(dict(name=name, saved_center=center,
                          saved_requested=[678,901], map_editor=editor,
                          map_rect_width=width, local_size=local,
                          viewport=viewport, resize=resize))
    return cases



def generate() -> dict:
    cases = []
    for case in input_cases():
        cases.append({**case, "expected": DragFixture(case).run(case["cursor"])})
    sequence_input = inputs("cross_then_stationary_then_return_to_anchor", scroll_rate=3)
    fixture = DragFixture(sequence_input)
    cursors = [[408, 300], [409, 300], [409, 300], [409, 300], [400, 300], [399, 300]]
    sequence = {
        "input": sequence_input,
        "frames": [{"cursor": cursor, "expected": fixture.run(cursor)} for cursor in cursors],
    }
    return {"source": "unicorn/gamemd.exe", "function": hex(RIGHT_DRAG),
            "cases": cases, "sequences": [sequence],
            "update_cases": [{**case, "expected": native_update(case)} for case in update_inputs()],
            "message_cases": [{**case, "expected": native_message(case)} for case in message_inputs()],
            "camera_sequences": [native_camera_sequence(case) for case in camera_inputs()],
            "focus_sequences": [native_camera_sequence(case) for case in focus_inputs()],
            "camera_lifecycle_sequences": [native_camera_sequence(case) for case in camera_lifecycle_inputs()],
            "throttle_cases": [{**case, "expected": native_throttle(case)}
                               for case in throttle_inputs()],
            "camera_save_load": [native_saved_view(case) for case in saved_view_inputs()]}


if __name__ == "__main__":
    finish_vectors(
        generate, Path(__file__).with_suffix(".json"),
        provenance=lambda: provenance(
            scope="Full original Tactical_RightDrag_Pan: supplied cursor/viewport/OS metrics and scroll settings; original gate, threshold, edge boost, arithmetic, call order and cursor table; thirteen original Tactical Save/Load/resize controls with derived viewport projection",
            assumptions=[
                "Fixture Display singleton fields model explicit anchor/crossed/engaged/band-box inputs; constructors and Windows delivery are outside these vectors",
                "Windows initial x87 PC53/nearest 0x027F is supplied; original WinMain 0x6BBFB7..0x6BBFCE executes controlfp and cache, establishing live/cached 0x0E3F (reserved bit6 clear, PC53/chop)",
                "Original Options getter, direction/trigonometric helpers, static tables and ftol execute unchanged",
                "ScrollRate 0..6 and ScrollMethod 0 are primary; method 1/2 rows characterize original factor 12 and cursor warp only",
                "Ordinary drag-row motion observes native requested Scroll_Map distances; those rows do not execute downstream clipping/projection",
                "Camera sequences execute original Scroll_Map/request, same-frame TacticalAI commit/clamp and Z=0 instant SetView; supplied MapWidth100/LocalSize0,0,100,100 and viewport640x480 are geometry fixtures, not retail-map provenance",
                "Camera sequence Tactical+A8 and global frame are both0, bypassing unrelated animation/timer work while retaining full original request commit; original clamp determines initial centers from supplied seeds",
                "Focus sequences execute original FocusOn6D2420 and full TacticalAI6D2540, float stores/interpolation75F5C0/clamp/cleanup for velocities0..4, all four clipping edges, nonzero/small LocalSize, repeated/wrapping frame gates, retarget, zero target, absolute SetView, ScenarioStarted/replay gates and pending scrolls. Original waypoint animation is kept stopped with remaining time1; geometry, subcell coordinates and Z=-104/0/727/728/1500 are explicit supplied inputs, not a stock mission timing or terrain ground-height comparison",
                "Original throttle entry 0x55E160 through 0x55E33B observes supplied clocks and session modes 0..6; later FPS bookkeeping is outside scope",
                "Saved view rows supply restored map/local rectangle and current viewport; input centers/requests are explicit pre-save state, not camera-command outputs or retail-map provenance",
                "Saved view rows execute original constructor matrix stores6D1DC5..6D1E24, TacticalSave/AbstractSave and TacticalLoad/AbstractLoad/no-init/clamp/6D8B30 viewport update; optional resize executes6D5F60",
                "Saved Tactical pointer region+E0 is empty and isolated identity registration succeeds; no world swizzle, Ole/COM loading, ordinary selection or Follow execution is claimed",
                "No original executable instructions are replaced; fresh original image per independent row, retained state only within the explicitly listed sequence",
            ],
            substitutions=[
                "Mouse receiver/update: supplied click-admission, planning-handler consumption, logical-button and cursor observations; planning/cancel-selection/edge-scroll/band-move/window capture/display flush are sinks; original cleanup and Tactical_ClearBandRect execute",
                "Throttle frame/millisecond clock leaves return supplied timelines; Sleep, offline service, network service, Input, command processing, tactical callback and rendering are observed sinks",
                "GetSystemMetrics returns supplied SM_CXDRAG/SM_CYDRAG, normally fixture 4/4 and custom 3/6",
                "Scroll_Map is a recorded sink in ordinary drag rows; camera_sequences execute it and its directional request/probe callees unchanged",
                "Camera sequences replace CRT exit registration and derived viewport refresh0x6D8B30 with observed void sinks; committed/requested centers and native clamp/SetView writes remain original",
                "Camera sequence keyboard distances are supplied producer outputs; the21 distance is the original constant at0x82A030, while drag25 is produced by original method0 for100 pixels at rate3; keyboard producer/follow-target selection is not emulated",
                "Camera SetView Z=0 rows supply the standard original projection multiplier from existing bridge_click_oracle; no new height or terrain coverage is claimed",
                "Camera lifecycle rows execute the full original Tactical constructor, Abstract/Tactical Save and Load, no-init constructor, size leaf, native clamp and subsequent AI. IStream retains exactly original requested bytes; Here_I_Am records the identity token without reconstructing the global Swizzle manager. VisibleCellCount is zero; pointer swizzling, complete scenario reload and serialization wire equivalence to Rust are excluded. The unrelated waypoint-display clock is supplied bucket0 and stopped during AI visits.",
                "Saved view rows replace the ordinary camera fixture's6D8B30 sink with the original updater and projection; exact in-memory IStream transfer,6C8C40 clock123 and recorded successful isolated identity registration are the only new seams",
                "Display vtable+0x48 records cursor, +0xC4 records band-box cancel argument without engagement mutation",
                "ClientToScreen adds fixture origin 101/202, SetCursorPos records requested OS coordinates; tactical offsets are 11/23",
            ],
            entry_points={"Tactical_RightDrag_Pan": RIGHT_DRAG,
                          "WinMain_controlfp_start": 0x006BBFB7,
                          "WinMain_controlfp_end": 0x006BBFCE,
                          "Math_ftol": 0x007C5F00,
                          "Mouse_receiver": 0x006930A0, "Mouse_update": 0x00692F30,
                          "Throttle_start": 0x0055E160, "Throttle_end": 0x0055E33B,
                          "Scroll_Map": SCROLL_MAP, "RequestDirectionalScroll": 0x006D8530,
                          "TacticalAI": 0x006D2540, "ClampViewCenter": 0x006D8640,
                          "SetViewToCoordInstant": 0x006D6070,
                          "FocusOn": 0x006D2420, "InterpolatePoint": 0x0075F5C0,
                          "TacticalConstructor": 0x006D1C20, "TacticalNoInit": 0x006D1E30,
                          "TacticalSave": 0x006DBE00, "TacticalLoad": 0x006DBD20,
                          "AbstractSave": 0x00410320, "AbstractLoad": 0x00410380,
                          "TacticalResize": 0x006D5F60, "UpdateViewport": 0x006D8B30,
                          "ConstructorMatrixStart": 0x006D1DC5,
                          "ConstructorMatrixEnd": 0x006D1E24},
        ),
        source_paths={"producer": Path(__file__), "shared_runner": Path(native_oracle.__file__),
                      "projection_fixture": Path(native_oracle.__file__).with_name("bridge_click_oracle.py")},
    )
