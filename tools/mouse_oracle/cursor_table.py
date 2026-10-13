"""Extract the compiled mouse-cursor tables from a gamemd executable.

Two structures, both compiled into the executable and not INI-driven:

* The cursor descriptor table at ``0x0082D028``: records of 7 dwords.
  ``Set_Mouse_Cursor_Shape`` (``0x005BDC80``) indexes it as ``base + 28 * row``:

  ===== ============================================================
  d0    first mouse.sha frame
  d1    frame count
  d2    animation rate in clock units of 16 ms (0 = no advance)
  d3    alternate ("mini") first frame, or -1 when the row has none
  d4    alternate frame count
  d5    hotspot x mode, d6 hotspot y mode:
        0 = near edge, 12345 = centre, 54321 = far edge
        (``0x005BDD27``/``0x005BDD44`` turn those sentinels into the offset)
  ===== ============================================================

  The table carries no length, so the extent is derived: records are read until
  one stops satisfying the layout above, and the first rejected record is
  reported. On the stock layout this yields 86 rows, after which the bytes are
  unrelated string data.

* The action-to-row switch in ``DisplayClass::SetCursorFromAction``
  (``0x004AAE90``): the block at ``0x004AB449`` dispatches on ``action - 1``
  through the jump table at ``0x004AB884``. The entry count is read from that
  block's own ``cmp eax, <bound>`` (``ja`` is strictly above, so the bound is
  inclusive and the table holds ``bound + 1`` entries; 0x47 gives 72 actions).
  Each target pushes a cursor row and calls the display's ``+0x48`` virtual; the
  default target pushes row 0. Two targets derive the row from a call instead of
  an immediate.

The executable identity is recorded in the sidecar. The anchor checks below must
pass before anything is written, so a binary whose code does not match the
layout this tool assumes is rejected rather than mis-read.

    py -3 -m tools.mouse_oracle.cursor_table --exe <gamemd.exe> --write
    py -3 -m tools.mouse_oracle.cursor_table --check
"""

from __future__ import annotations

import argparse
import json
import re
import struct
from pathlib import Path

from capstone import CS_ARCH_X86, CS_MODE_32, Cs

from tools import native_oracle as native

REPO = Path(__file__).resolve().parents[2]
FIXTURE = Path("tools/mouse_oracle/cursor_table.json")
SIDECAR = Path("tools/mouse_oracle/cursor_table.meta.json")

TABLE_BASE = 0x0082D028
RECORD_BYTES = 28
SETTER = 0x005BDC80
ACTION_SWITCH = 0x004AAE90
ACTION_DISPATCH = 0x004AB449
ACTION_TABLE = 0x004AB884
SET_CURSOR_VIRTUAL = 0x48

# MOUSE.SHA carries 517 frames, so no record may start past it.
MOUSE_SHA_FRAMES = 517
HOTSPOT_MODES = (0, 12345, 54321)
# A record is table data only if every field satisfies the layout the setter
# itself assumes. Rates above 8 never appear in the stock table.
MAX_RATE = 8
# The scan stops at the first record that fails, so this only bounds the search.
MAX_ROW_SCAN = 256

# Stock retail gamemd.exe. A launcher-patched copy is only accepted with
# --accept-patched, and its identity is recorded in the sidecar.
STOCK_SHA256 = "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"

# Addresses this extraction assumes, each recorded elsewhere in this repository
# from stock analysis. Every one must decode as stated before the tables are
# trusted: they are the cross-check that lets a patched binary be read at all.
ANCHORS = {
    0x005BDC95: "8d348d28d08200",  # lea esi,[ecx*4 + 0x82d028]
    0x004AB453: "ff248584b84a00",  # jmp [eax*4 + 0x4ab884]
    0x004AB43C: "ff5248",  # call [edx + 0x48]
    0x007E3EBC + 0x4E4: "80da4300",  # Building vtable +0x4E4 -> 0x0043DA80
    0x007E3EBC + 0x444: "40667000",  # Building vtable +0x444 -> 0x00706640
}


class Image:
    """Static file bytes, addressed through the shared oracle's range owner."""

    def __init__(self, data: bytes) -> None:
        self.data = data

    def bytes_at(self, va: int, length: int) -> bytes:
        return native.file_span(self.data, va, length)[1]

    def dword(self, va: int) -> int:
        return struct.unpack_from("<I", self.bytes_at(va, 4))[0]

    def disasm(self, va: int, length: int) -> list:
        decoder = Cs(CS_ARCH_X86, CS_MODE_32)
        return list(decoder.disasm(self.bytes_at(va, length), va))


def check_anchors(image: Image) -> list[str]:
    problems = []
    for va, want in ANCHORS.items():
        got = image.bytes_at(va, len(want) // 2).hex()
        if got != want:
            problems.append("%X: want %s, got %s" % (va, want, got))
    return problems


def _record(fields: tuple[int, ...]) -> dict:
    def value(v: int) -> int | None:
        return None if v == 0xFFFFFFFF else v

    return {
        "frame": fields[0],
        "count": fields[1],
        "rate": fields[2],
        "mini_frame": value(fields[3]),
        "mini_count": value(fields[4]),
        "hotspot_x": fields[5],
        "hotspot_y": fields[6],
    }


def _is_record(fields: tuple[int, ...]) -> bool:
    """Whether a 28-byte block can be a descriptor record at all."""
    frame, count, rate, mini, mini_count, hx, hy = fields
    if not (0 < count <= MOUSE_SHA_FRAMES and frame < MOUSE_SHA_FRAMES - count + 1):
        return False
    if rate > MAX_RATE:
        return False
    if hx not in HOTSPOT_MODES or hy not in HOTSPOT_MODES:
        return False
    if mini is None:
        return mini_count is None
    return mini == 0xFFFFFFFF or mini < MOUSE_SHA_FRAMES


def read_table(image: Image) -> tuple[list[dict], dict]:
    """The descriptor records, plus how the scan decided where the table ends."""
    rows: list[dict] = []
    rejected: dict | None = None
    for row in range(MAX_ROW_SCAN):
        fields = struct.unpack(
            "<7I", image.bytes_at(TABLE_BASE + row * RECORD_BYTES, RECORD_BYTES)
        )
        if not _is_record(fields):
            rejected = {
                "first_rejected_row": row,
                "dwords": ["0x%08X" % v for v in fields],
                "as_text": "".join(
                    chr(c) if 32 <= c < 127 else "." for c in struct.pack("<7I", *fields)
                ),
            }
            break
        rows.append(dict(row=row, **_record(fields)))
    if rejected is None or len(rows) < 80:
        raise SystemExit(
            "descriptor table scan stopped after %d records; the layout this tool "
            "assumes does not hold for this executable" % len(rows)
        )
    return rows, rejected


def _row_of_block(image: Image, target: int) -> tuple[int | None, str]:
    """The cursor row a dispatch target pushes, or how it is computed instead.

    Arguments go right to left, so the row is the last immediate pushed before
    the ``call [x+0x48]``; the register push ahead of it carries the object the
    display virtual receives.
    """
    row: int | None = None
    note = ""
    for ins in image.disasm(target, 96):
        if ins.mnemonic == "push":
            text = ins.op_str
            if re.fullmatch(r"0x[0-9a-f]+", text):
                row = int(text, 16)
            elif not note:
                note = "second argument %s" % text
        elif ins.mnemonic == "call":
            if "+ 0x48]" in ins.op_str:
                return row, note
            if ins.op_str.startswith("0x"):
                return None, "row from call %s" % ins.op_str
        elif ins.mnemonic in ("ret", "jmp"):
            break
    return None, "no row"


def read_dispatch(image: Image) -> tuple[int, int]:
    """The jump table address and its entry count, read from the dispatch block.

    ``dec eax`` (``action - 1``), ``cmp eax, <bound>``, ``ja default``, then
    ``jmp [eax*4 + <table>]``. ``ja`` is strictly above, so indices ``0..bound``
    are all reachable and the table holds ``bound + 1`` entries.
    """
    insns = image.disasm(ACTION_DISPATCH, 24)
    bound = None
    table = None
    for index, ins in enumerate(insns):
        if ins.mnemonic == "cmp" and bound is None:
            digits = re.findall(r"0x([0-9a-f]+)", ins.op_str)
            if digits:
                bound = int(digits[-1], 16)
        if ins.mnemonic == "jmp" and "*4" in ins.op_str:
            digits = re.findall(r"0x([0-9a-f]+)", ins.op_str)
            if digits:
                table = int(digits[-1], 16)
            break
    if bound is None or table is None:
        raise SystemExit(
            "could not read the action switch bound and jump table at %X; this "
            "executable does not match the expected layout" % ACTION_DISPATCH
        )
    return table, bound + 1


def read_action_rows(image: Image) -> tuple[list[dict], int]:
    table, count = read_dispatch(image)
    if table != ACTION_TABLE:
        raise SystemExit(
            "the action dispatch jumps through %X, not the %X this tool documents"
            % (table, ACTION_TABLE)
        )
    out = []
    for index in range(count):
        target = image.dword(table + 4 * index)
        row, note = _row_of_block(image, target)
        out.append(
            {"action": index + 1, "row": row, "block": "0x%08X" % target, "note": note}
        )
    return out, count


def extract(exe: Path, accept_patched: bool) -> tuple[dict, dict]:
    # `file_span` and the sidecar both require the caller to have established
    # the input's identity, so that happens before any read.
    data = exe.read_bytes()
    sha = native.image_sha256(data)
    if sha != STOCK_SHA256 and not accept_patched:
        raise SystemExit(
            "%s has SHA-256 %s, not the stock %s.\n"
            "Re-run with --accept-patched to record this binary's identity in "
            "the sidecar." % (exe, sha, STOCK_SHA256)
        )
    image = Image(data)
    problems = check_anchors(image)
    if problems:
        raise SystemExit("anchor checks failed:\n  " + "\n  ".join(problems))
    rows, rejected = read_table(image)
    actions, entries = read_action_rows(image)
    payload = {
        "descriptor_table_base": "0x%08X" % TABLE_BASE,
        "record_bytes": RECORD_BYTES,
        "rows": rows,
        "table_extent": {
            "how": (
                "records read until one fails the layout (frame and count inside "
                "MOUSE.SHA's 517 frames, rate <= %d, both hotspot fields in "
                "{0, 12345, 54321})" % MAX_RATE
            ),
            **rejected,
        },
        "action_switch": {
            "function": "0x%08X" % ACTION_SWITCH,
            "dispatch": "0x%08X" % ACTION_DISPATCH,
            "jump_table": "0x%08X" % ACTION_TABLE,
            "entry_count": entries,
            "count_how": "the dispatch's own `cmp eax, <bound>`; `ja` leaves the "
            "bound inclusive, so the table holds bound + 1 entries",
            "entries": actions,
        },
    }
    sidecar = {
        "schema_version": 1,
        "native_sha256": sha,
        "stock_sha256": STOCK_SHA256,
        "binary_is_stock": sha == STOCK_SHA256,
        "binary_size": len(data),
        "command": "py -3 -m tools.mouse_oracle.cursor_table --exe <path> --write",
        "scope": (
            "Compiled mouse-cursor descriptor table and the "
            "DisplayClass::SetCursorFromAction action-to-row switch. Static "
            "reads of the executable; no native execution and no rendered "
            "pixels."
        ),
        "anchor_checks": ["0x%X" % va for va in sorted(ANCHORS)],
        "limits": [
            "Row semantics (which game state selects which action) are not "
            "established here; only action -> row and row -> frames.",
            "A row with count > 1 and rate 0 is recorded as stored; how the "
            "original advances such a row is not established.",
            "Two switch entries derive their row from a call (0x00731CC0, "
            "0x00731CB0); the callee's result is not recorded.",
            "The descriptor table has no stored length: the row count comes from "
            "the layout check above, and the action count from the dispatch's own "
            "bound. Both are recorded with the derived value, not assumed.",
        ],
    }
    return payload, sidecar


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--exe", type=Path, help="gamemd executable to read")
    ap.add_argument("--write", action="store_true", help="write the fixture and sidecar")
    ap.add_argument("--check", action="store_true", help="compare the committed fixture")
    ap.add_argument(
        "--accept-patched",
        action="store_true",
        help="allow a binary whose hash is not the stock one (recorded in the sidecar)",
    )
    args = ap.parse_args()

    if args.exe is None or args.write == args.check:
        raise SystemExit("give --exe with exactly one of --write or --check")

    payload, sidecar = extract(args.exe, args.accept_patched)
    if args.check:
        committed = json.loads((REPO / FIXTURE).read_text(encoding="utf-8"))
        if committed == payload:
            print("cursor_table.json matches the executable's tables")
            return 0
        raise SystemExit("cursor_table.json differs from the executable's tables")

    out = REPO / FIXTURE
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(payload, indent=1) + "\n", encoding="utf-8")
    (REPO / SIDECAR).write_text(json.dumps(sidecar, indent=1) + "\n", encoding="utf-8")
    print(
        "wrote %s (%d rows, %d action entries)"
        % (
            out,
            len(payload["rows"]),
            payload["action_switch"]["entry_count"],
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
