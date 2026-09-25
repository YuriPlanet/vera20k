"""Original kind-1 reveal and kind-4 startup parameters of the right-panel statics.

The shell static classifiers and parameter getters take the parent dialog HWND
in ECX and the child HWND in EDX. Each looks the parent up in the dialog-record
table (``[AC1B04]`` count, ``[AC1B18]`` hash, ``[AC1B0C]`` bits, ``[AC1B00]``
buckets; record+0 HWND, +0x204 next, +0x70 dialog id), reads the child's id with
GetDlgCtrlID and walks compare chains. This fixture runs the complete original
bodies for heading ``0x694``, status line ``0x695`` and monitor ``0x71C`` of the
main-menu family and Options/Skirmish/saved-game neighbours, and for the score
dialog ``0x108``'s table statics (Game, Time, the five headers and the eight
rows' name, Kills, Losses, Built and Score cells, template order):

- ``602490`` kind-1 classifier (``60AA60`` sends ``0x4EE`` to every accepted child
  at the shell SHOW completion),
- ``600CA0`` kind-1 timer interval, ``6015E0`` step, ``601D20`` highlight range,
- ``603240`` kind-4 startup timer (``60A9E2``).

Only the record table, GetDlgCtrlID and the no-session predicate are supplied.
"""

from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP,
)

from tools.native_oracle import (
    RET_MAGIC, SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE,
    finish_vectors, load_image, provenance, run_checked,
)

PARENT, CHILD = 0x5678, 0x1234
HASH_STUB = SCRATCH + 0xF000
GET_DLG_CTRL_ID_STUB = SCRATCH + 0xF010
BUCKETS = SCRATCH + 0x100
RECORD = SCRATCH + 0x200
SESSION_ACTIVE = 0x69BBE0

GETTERS = {
    "kind1": 0x602490,
    "interval_ms": 0x600CA0,
    "step": 0x6015E0,
    "range": 0x601D20,
    "kind4_startup_ms": 0x603240,
}
DIALOGS = (0xE2, 0x100, 0x101, 0x129, 0xD5, 0x102, 0xB7)
CONTROLS = (0x694, 0x695, 0x71C)
SCORE_DIALOG = 0x108
# RT_DIALOG 0x108: Game 0x6D2, the headers, the per-row cells (the proc's
# table at 0x0082FD5C: name, Kills, Losses, Built, Score) and Time 0x3EA.
SCORE_TABLE = (
    (0x6D2, 0x69D, 0x69E, 0x69F, 0x6A0, 0x78B)
    + tuple(
        control
        for row in range(8)
        for control in (
            0x411 + row,
            0x419 + row,
            (0x6A3, 0x6A4, 0x6A9, 0x6AC, 0x6AF, 0x6B2, 0x6B5, 0x6B8)[row],
            (0x6A5, 0x6A7, 0x6AA, 0x6AD, 0x6B0, 0x6B3, 0x6B6, 0x6B9)[row],
            (0x6A6, 0x6A8, 0x6AB, 0x6AE, 0x6B1, 0x6B4, 0x6B7, 0x6BA)[row],
        )
    )
    + (0x3EA,)
)


def read_u32(uc, address):
    return struct.unpack("<I", uc.mem_read(address, 4))[0]


def run_getter(entry, dialog_id, control_id):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    uc.mem_map(RET_MAGIC, 0x1000)
    # One-bucket dialog-record table holding the parent's record.
    uc.mem_write(0xAC1B04, struct.pack("<I", 1))
    uc.mem_write(0xAC1B18, struct.pack("<I", HASH_STUB))
    uc.mem_write(0xAC1B0C, struct.pack("<I", 0))
    uc.mem_write(0xAC1B00, struct.pack("<I", BUCKETS))
    uc.mem_write(BUCKETS, struct.pack("<I", RECORD))
    uc.mem_write(RECORD, struct.pack("<I", PARENT))
    uc.mem_write(RECORD + 0x204, struct.pack("<I", 0))
    uc.mem_write(RECORD + 0x70, struct.pack("<I", dialog_id))
    # Only imported function-pointer DATA is replaced; body bytes stay original.
    uc.mem_write(0x7E13B4, struct.pack("<I", GET_DLG_CTRL_ID_STUB))
    stack = STACK_BASE + STACK_SIZE - 0x1000
    uc.mem_write(stack, struct.pack("<I", RET_MAGIC))
    uc.reg_write(UC_X86_REG_ESP, stack)
    uc.reg_write(UC_X86_REG_ECX, PARENT)
    uc.reg_write(UC_X86_REG_EDX, CHILD)
    calls = {HASH_STUB: 0, GET_DLG_CTRL_ID_STUB: 0, SESSION_ACTIVE: 0}

    def substitute(_uc, address, _size, _data):
        if address not in calls:
            return
        calls[address] += 1
        sp = uc.reg_read(UC_X86_REG_ESP)
        arguments = 0
        if address == HASH_STUB:
            if read_u32(uc, uc.reg_read(UC_X86_REG_ECX)) != PARENT:
                raise RuntimeError("Record lookup keyed by an unexpected HWND")
            result = 0
        elif address == GET_DLG_CTRL_ID_STUB:
            if read_u32(uc, sp + 4) != CHILD:
                raise RuntimeError("GetDlgCtrlID asked for an unexpected HWND")
            result, arguments = control_id, 1
        else:
            if uc.reg_read(UC_X86_REG_ECX) != 0xA8B238:
                raise RuntimeError("Unexpected session predicate receiver")
            result = 0  # Pre-match shell: no network session.
        uc.reg_write(UC_X86_REG_EAX, result)
        uc.reg_write(UC_X86_REG_EIP, read_u32(uc, sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4 * (1 + arguments))

    hook = uc.hook_add(UC_HOOK_CODE, substitute)
    try:
        run_checked(uc, entry, RET_MAGIC, count=20_000)
    finally:
        uc.hook_del(hook)
    if calls[HASH_STUB] != 1 or calls[GET_DLG_CTRL_ID_STUB] != 1:
        raise RuntimeError(f"Unexpected lookup calls for {entry:#x}: {calls}")
    value = uc.reg_read(UC_X86_REG_EAX)
    return value & 0xFF if entry == GETTERS["kind1"] else value


def generate():
    pairs = [(dialog_id, control_id) for dialog_id in DIALOGS for control_id in CONTROLS]
    pairs += [(SCORE_DIALOG, control_id) for control_id in CONTROLS + SCORE_TABLE]
    cases = []
    for dialog_id, control_id in pairs:
        case = {"dialog_id": dialog_id, "control_id": control_id}
        for name, entry in GETTERS.items():
            case[name] = run_getter(entry, dialog_id, control_id)
        cases.append(case)
    return {"source": "unicorn/gamemd.exe", "cases": cases}


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=lambda: provenance(
        scope="Complete original kind-1 classifier/interval/step/range getters and the kind-4 startup timer for 0x694/0x695/0x71C in seven shell dialogs and the score dialog 0x108, plus 0x108's 47 table statics",
        assumptions=[
            "Pre-match shell: the network-session predicate 69BBE0 returns false",
            "The parent dialog's record is found by the original lookup loop in a supplied one-bucket table; record fields other than HWND, next and dialog id stay zero",
            "Values are the getters' returns; which statics a dialog creates, their painting and message order are established separately by instruction reading",
        ],
        substitutions=[
            "Record-table hash function pointer [AC1B18] returns bucket 0",
            "GetDlgCtrlID IAT pointer returns the fixture control id",
            "69BBE0 returns false (no network session)",
        ],
        entry_points={**GETTERS, "show_completion_enum": 0x60AA60,
                      "kind4_init": 0x60A9E2},
    ))
