"""Original WOL custom-match timing, layout, help and slide-record controls.

- Timing: the "Start Game Now" `0x794BA0` block `0x794F6F..0x79523E` runs
  natively for controlled game speed `[0xA8B268]`, the eight ping dwords at
  `0xB7790C`, the WOL channel user list (owner flag bit0 at record +0,
  nickname at +0x24, compared with the local nickname by `_stricmp
  0x7C8D20`) and the command-line debug byte `[0xA8E2C8]`. Only IPX
  `SetTiming 0x540C60` is replaced by a recorder.
- Layouts: full-screen pages run the shared `shell_relayout.relayout`; compact
  popups run `shell_relayout.popup_layout` (775BC0 resize, 60C540/60C7D0
  classification and 777060 centring).
- Help: `shell_help_keys.help_label` (table lookup `0x6040B0`).
- Slide records: the record-flag block `0x6228BD..0x6229D4` and the
  top/bottom classifier callbacks `0x60A180`/`0x60A250`, shared with
  `shell_slide_engine.py`.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP

from tools.native_oracle import OracleError, STACK_BASE, STACK_SIZE, finish_vectors, provenance, run_checked
from tools.storage_oracle.shell_help_keys import help_label
from tools.storage_oracle.shell_relayout import Machine, popup_layout, relayout
from tools.storage_oracle.shell_slide_engine import executed_slide_record

TIMING_BEGIN, TIMING_END = 0x794F6F, 0x79523E
SET_TIMING = 0x540C60
SPEED, PINGS, DEBUG = 0xA8B268, 0xB7790C, 0xA8E2C8
USER_COUNT, USERS, LOCAL_NAME = 0xB77900, 0xB77908, 0xB73E40
OUTPUTS = dict(max_ahead=0xA8B550, frame_send_rate=0xA8B554, precalc_max_ahead=0xA8B56C,
               precalc_frame_rate=0xA8B570, max_max_ahead=0xA8B568, latency_fudge=0xA8DB9C,
               current_speed=0xA8EB60)
USER_LEN = 0x6C
SIZES = ((640, 480), (800, 600), (1024, 768))
PAGES = (0x10F, 0x113, 0xC2, 0xC9, 0x2BC, 0xD6, 0x112, 0xFE, 0x114, 0x116, 0x122)
POPUPS = (0xC3, 0xE1, 0xC4, 0xCF, 0x11E, 0x115, 0x130)


def signed(value):
    return struct.unpack("<i", struct.pack("<I", value & 0xFFFFFFFF))[0]


def timing_case(name, speed, pings, users, local="Tester", debug=False):
    """users: [(flags, nickname)] in channel order; owner = first flags&1 row."""
    m = Machine(0x113, 800, 600)
    m.w32(SPEED, speed)
    for index, ping in enumerate(pings):
        m.w32(PINGS + 4 * index, ping)
    m.uc.mem_write(DEBUG, bytes([int(debug)]))
    records = m.alloc(max(1, len(users)) * USER_LEN)
    for index, (flags, nick) in enumerate(users):
        row = records + index * USER_LEN
        m.uc.mem_write(row, bytes(USER_LEN))
        m.w32(row, flags)
        m.uc.mem_write(row + 0x24, nick.encode("ascii") + b"\0")
    m.w32(USERS, records)
    m.w32(USER_COUNT, len(users))
    name_pointer = m.alloc(32)
    m.uc.mem_write(name_pointer, local.encode("ascii") + b"\0")
    m.w32(LOCAL_NAME, name_pointer)
    for address in OUTPUTS.values():
        m.w32(address, 0xDEADBEEF)
    m.uc.mem_write(0xA8B574, b"\xA5" * (4 * 0xD0))
    calls = []
    # 0x795094 clears PreCalcFrameRate (no owner row, or its nickname differs
    # from the local one); 0x79536C is the owner-only PreCalcMaxAhead path
    # after SetTiming; 0x7953BB the ping 0/1000 fixed PreCalcMaxAhead path.
    marks = {0x795094: "frame_rate_cleared", 0x79536C: "owner_precalc", 0x7953BB: "fixed_precalc"}
    visits = dict.fromkeys(marks.values(), 0)

    def observe(uc, address, _size, _data):
        if address == SET_TIMING:
            if uc.reg_read(UC_X86_REG_ECX) != 0xA8E9C0:
                raise OracleError("SetTiming receiver is not the IPX manager")
            calls.append([signed(m.arg(i)) for i in range(4)])
            m.ret(16)
        elif address in marks:
            visits[marks[address]] += 1

    m.uc.hook_add(UC_HOOK_CODE, observe)
    sp = STACK_BASE + STACK_SIZE - 0x2000
    m.uc.reg_write(UC_X86_REG_ESP, sp)
    m.uc.reg_write(UC_X86_REG_ESI, 0)  # Caller 0x794F3A: xor esi, esi.
    run_checked(m.uc, TIMING_BEGIN, TIMING_END, count=200_000)
    if m.uc.reg_read(UC_X86_REG_ESP) != sp:
        raise OracleError(f"{name}: timing block stack imbalance")
    if bytes(m.uc.mem_read(0xA8B574, 4 * 0xD0)) != bytes(4 * 0xD0):
        raise OracleError(f"{name}: 0xA8B574[0xD0] not cleared")
    outputs = {key: signed(m.r32(address)) for key, address in OUTPUTS.items()}
    if any(value == signed(0xDEADBEEF) for value in outputs.values()):
        raise OracleError(f"{name}: timing output not written {outputs}")
    if len(calls) > 1:
        raise OracleError(f"{name}: SetTiming called {len(calls)} times")
    return dict(name=name, game_speed=speed, pings=pings, debug=debug, local=local,
                users=[dict(flags=flags, nick=nick) for flags, nick in users],
                channel_owner=visits["frame_rate_cleared"] == 0,
                **outputs, set_timing=calls[0] if calls else None,
                owner_precalc=visits["owner_precalc"] == 1, fixed_precalc=visits["fixed_precalc"] == 1)


OWNER = [(0x8001, "Tester"), (0, "Guest")]
GUEST = [(1, "Host"), (0x8000, "Tester")]


def timing_cases():
    pings = {
        "all_0": [0] * 8,
        "all_1000": [1000] * 8,
        "mixed_negative_250": [-1, 250, -1, 0, 120, -1, 0, 0],
        "max_599": [599, 10, -1, 0, 0, 0, 0, 0],
        "max_600": [0, 600, 599, 0, 0, 0, 0, -1],
        "max_1000_with_lower": [1000, 999, 0, 0, 0, 0, 0, 0],
        "max_1001": [1001, 1000, 0, 0, 0, 0, 0, 0],
    }
    cases = []
    for speed in (0, 1, 2, 3, 4, 5, 6, -1):
        for ping_name, values in pings.items():
            for role, users in (("owner", OWNER), ("guest", GUEST)):
                cases.append(timing_case(f"speed{speed}_{ping_name}_{role}", speed, values, users))
    for name, speed, values, users, local, debug in (
        ("all_negative_owner", 0, [-1] * 8, OWNER, "Tester", False),
        ("ping_1_owner", 0, [1] + [0] * 7, OWNER, "Tester", False),
        ("ping_last_slot_owner", 4, [0] * 7 + [480], OWNER, "Tester", False),
        ("large_1000000_owner", 2, [1_000_000] + [0] * 7, OWNER, "Tester", False),
        ("wrap_35791394_owner", 2, [35_791_394] + [0] * 7, OWNER, "Tester", False),
        ("wrap_35791395_owner", 2, [35_791_395] + [0] * 7, OWNER, "Tester", False),
        ("wrap_40000000_guest", 2, [40_000_000] + [0] * 7, GUEST, "Tester", False),
        ("signed_max_owner", 5, [0x7FFFFFFF] + [0] * 7, OWNER, "Tester", False),
        ("speed_signed_min_owner", -0x80000000, [300] + [0] * 7, OWNER, "Tester", False),
        ("speed_7_owner", 7, [300] + [0] * 7, OWNER, "Tester", False),
        ("no_users_ping_300", 2, [300] + [0] * 7, [], "Tester", False),
        ("no_users_ping_0", 2, [0] * 8, [], "Tester", False),
        ("no_owner_flag", 2, [300] + [0] * 7, [(0, "Tester"), (0, "Guest")], "Tester", False),
        ("owner_flag_second_row", 2, [300] + [0] * 7, [(0, "Guest"), (1, "Tester")], "Tester", False),
        ("owner_name_case_insensitive", 2, [300] + [0] * 7, [(1, "TESTER")], "tester", False),
        ("first_flagged_row_decides", 2, [300] + [0] * 7, [(1, "Host"), (1, "Tester")], "Tester", False),
        ("debug_owner_ping_400", 2, [400] + [0] * 7, OWNER, "Tester", True),
        ("debug_guest_ping_500", 0, [500] + [0] * 7, GUEST, "Tester", True),
        ("debug_owner_ping_0", 3, [0] * 8, OWNER, "Tester", True),
    ):
        cases.append(timing_case(name, speed, values, users, local, debug))
    return cases


def hex_id(control_id):
    return f"0x{control_id & 0xFFFF:X}"


def keyed(controls):
    """Control rows keyed by id; repeated ids (IDC_STATIC) take '#n' suffixes."""
    rows, seen = {}, {}
    for row in controls:
        key = hex_id(row["id"])
        count = seen.get(key, 0)
        seen[key] = count + 1
        rows[f"{key}#{count}" if count else key] = {k: v for k, v in row.items() if k != "id"}
    return rows


def page_layout(dialog_id):
    controls = Machine(dialog_id, 800, 600).controls
    sizes = {}
    for width, height in SIZES:
        rows = [dict(child, style=source["style"], caption=source["caption"])
                for child, source in zip(relayout(dialog_id, width, height), controls)]
        sizes[f"{width}x{height}"] = dict(dialog=[0, 0, width, height], controls=keyed(rows))
    return dict(kind="page", sizes=sizes)


def popup(dialog_id):
    controls = Machine(dialog_id, 800, 600).controls
    sizes, background = {}, set()
    for width, height in SIZES:
        _machine, result = popup_layout(dialog_id, width, height)
        if result["fullscreen"] or not result["mode2"]:
            raise OracleError(f"{dialog_id:#x}: not a 0x60C7D0 compact popup")
        background.add(result["background_mode"])
        x, y = result["dialog_rect"][:2]
        rows = []
        for child, source in zip(result["children"], controls):
            sx, sy, w, h = child["rect"]
            rows.append({"id": child["id"], "class": source["class"], "style": source["style"],
                         "caption": source["caption"], "template": source["template"],
                         "rect": [sx - x, sy - y, w, h], "screen_rect": child["rect"]})
        sizes[f"{width}x{height}"] = dict(dialog=result["dialog_rect"], controls=keyed(rows))
    if len(background) != 1:
        raise OracleError(f"{dialog_id:#x}: background mode differs by resolution")
    return dict(kind="popup", background_mode=background.pop(), sizes=sizes)


def help_keys(dialog_id):
    return {hex_id(c["id"]): help_label(dialog_id, c["id"] & 0xFFFF)
            for c in Machine(dialog_id, 800, 600).controls if c["id"] & 0xFFFF != 0xFFFF}


def slide_records():
    rows = {}
    for dialog_id in PAGES:
        record = executed_slide_record(dialog_id)
        rows[hex_id(dialog_id)] = dict(record, top=[hex_id(i) for i in record["top"]],
                                       bottom=[hex_id(i) for i in record["bottom"]])
    return rows


def generate():
    layouts = {hex_id(d): page_layout(d) for d in PAGES}
    layouts.update({hex_id(d): popup(d) for d in POPUPS})
    return dict(source="unicorn/gamemd.exe", timing=timing_cases(), layouts=layouts,
                help={hex_id(d): help_keys(d) for d in PAGES + POPUPS}, slides=slide_records())


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope="WOL Start Game Now timing block 0x794F6F..0x79523E (inside 0x794BA0); relayout/popup geometry and status-line help of WOL dialogs 0x10F, 0x113, 0xC2, 0xC9, 0x2BC and its Sound Options page 0xD6, the auto-login query 0x112, the locale page 0xFE, My Information 0x114, the Buddy List 0x116 and the add-buddy query 0x122 (pages) and 0xC3, 0xE1, 0xC4, the page dialog 0xCF, the page-received dialog 0x11E, Add Buddy 0x115 and the connection verify 0x130 (popups) at 640x480, 800x600 and 1024x768; slide record flags 0x6228BD..0x6229D4 and top/bottom classifiers 0x60A180/0x60A250 for the eleven pages",
        assumptions=[
            "Timing enters 0x794F6F with caller-established ESI=0 (0x794F3A) and stops at 0x79523E, before the debug-only 0x664610 call and the WDT/clan name block. Game speed, the eight raw ping dwords, the debug byte 0xA8E2C8, the channel user list (count 0xB77900, 0x6C-byte rows at [0xB77908]: +0 flags, +0x24 nickname) and the local nickname string at [0xB73E40] are controlled inputs; every output starts at 0xDEADBEEF and 0xA8B574[0xD0] at 0xA5 bytes, so each observed value is a native write",
            "channel_owner is observed, not supplied: true when 0x795094 (PreCalcFrameRate=0) is not executed, i.e. the first user row with flags bit0 has a nickname _stricmp-equal (0x7C8D20, C locale handle 0) to the local one",
            "Debug byte 0xA8E2C8 is the command-line flag read at 0x52F998; it is dormant in retail play and only three controls set it",
            "Page layouts run shell_relayout.relayout, which fails closed unless 0x60C540 accepts the dialog (0x622F9C); popups run shell_relayout.popup_layout: 6x13 DLU MulDiv start, 0x775BC0 child-then-parent resize with cached benchmark client 300x163, 0x60C540/0x60C7D0 classification and 0x777060/0x777080 centring on the main window [0xB73550] whose client is the screen. Window non-client borders and font metrics are not emulated",
            "Popup control rect is client-relative (screen_rect minus the dialog origin); page control rect is the screen rect of a dialog at (0,0,W,H) before NewEdit 0x497's +1/-2 text inset",
            "Help is the table lookup 0x6040B0 only; per-control 0x4E8 and per-proc 0x4E9 answers (0x113 lists, 0xC2/0xC9 ping icons) are not executed",
            "Slide classifiers count template children with WS_VISIBLE; dialog-proc ShowWindow/EnableWindow changes after creation are not executed (the decompiled 0x78AC10, 0x78F7F0 and 0x799EA0 bodies hide no counted button; 0xC9's proc 0x791B10 is not checked; 0x114's proc 0x799080 only enables or disables 0x6EB/0x687 at init, and the template-disabled Forget 0x6EB is counted)",
        ],
        substitutions=[
            "IPX SetTiming 0x540C60 (thiscall on 0xA8E9C0) records its four stack arguments and returns; no other call in the timing block is substituted (0x4068E0 is the original bare ret, 0x7B5440 and 0x7C8D20 run unchanged)",
            "Existing shell_relayout.Machine window/record transport, operator new/delete and fail-closed imports; GetWindowLongA(GWL_HWNDPARENT) answers the main window for the popup and the popup for its children",
            "Help lookup reuses shell_help_keys.help_label: record-table hash returns bucket 0 and GetDlgCtrlID returns the template id",
        ],
        entry_points={"start_game_now": 0x794BA0, "timing_begin": TIMING_BEGIN, "timing_end": TIMING_END,
                      "set_timing": SET_TIMING, "stricmp": 0x7C8D20, "local_name": 0x7B5440,
                      "relayout": 0x60C4A0, "fullscreen_class": 0x60C540, "popup_class": 0x60C7D0,
                      "resize": 0x775BC0, "center": 0x777060, "help": 0x6040B0,
                      "record_flags_begin": 0x6228BD, "record_flags_end": 0x6229D4,
                      "top_button_count": 0x60A180, "bottom_button_count": 0x60A250}))
