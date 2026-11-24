"""Executed status-line help lookup of the shell dialogs' controls.

The common handler `0x00622B50` answers a hit test by sending the help of the
child under the pointer to the status line `0x695` (`0x00622CCB..0x00622E83`):
the child's own `0x4E8` answer, else the dialog's `0x4E9` answer, else the
table lookup `0x006040B0`, which keys the CSF help label by the dialog id
(record `+0x6C`) and the control id (GetDlgCtrlID). This fixture runs that
lookup for every control of the offline shell dialogs' RT_DIALOG templates and
records the returned label (`null` when it returns none). The `0x4E8` and
`0x4E9` answers are per-proc and established separately.
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
from tools.storage_oracle.shell_relayout import Image, parse_template

LOOKUP = 0x6040B0
PARENT, CHILD = 0x5678, 0x1234
HASH_STUB = SCRATCH + 0xF000
GET_DLG_CTRL_ID_STUB = SCRATCH + 0xF010
BUCKETS = SCRATCH + 0x100
RECORD = SCRATCH + 0x200
# The offline shell: main menu family, Skirmish and its map dialogs, score,
# campaign, saved games (and the seed browser's save/delete forms), Options
# and its Keyboard page, and the WOL welcome.
DIALOGS = (0xE2, 0x100, 0x101, 0x129, 0x102, 0x6B, 0x105, 0x108, 0x94, 0xB7,
           0x2B4, 0x2B5, 0xD5, 0xA3, 0x10E)


def read_u32(uc, address):
    return struct.unpack("<I", uc.mem_read(address, 4))[0]


def help_label(dialog_id, control_id):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    uc.mem_map(RET_MAGIC, 0x1000)
    # One-bucket dialog-record table holding the parent's record.
    for address, value in (
        (0xAC1B04, 1), (0xAC1B18, HASH_STUB), (0xAC1B0C, 0), (0xAC1B00, BUCKETS),
        (BUCKETS, RECORD), (RECORD, PARENT), (RECORD + 0x204, 0), (RECORD + 0x70, dialog_id),
        # Only imported function-pointer data is replaced; body bytes stay original.
        (0x7E13B4, GET_DLG_CTRL_ID_STUB),
    ):
        uc.mem_write(address, struct.pack("<I", value))
    stack = STACK_BASE + STACK_SIZE - 0x1000
    uc.mem_write(stack, struct.pack("<I", RET_MAGIC))
    uc.reg_write(UC_X86_REG_ESP, stack)
    uc.reg_write(UC_X86_REG_ECX, PARENT)
    uc.reg_write(UC_X86_REG_EDX, CHILD)
    calls = {HASH_STUB: 0, GET_DLG_CTRL_ID_STUB: 0}

    def substitute(_uc, address, _size, _data):
        if address not in calls:
            return
        calls[address] += 1
        sp = uc.reg_read(UC_X86_REG_ESP)
        if address == HASH_STUB:
            result, arguments = 0, 0
        else:
            if read_u32(uc, sp + 4) != CHILD:
                raise RuntimeError("GetDlgCtrlID asked for an unexpected HWND")
            result, arguments = control_id, 1
        uc.reg_write(UC_X86_REG_EAX, result)
        uc.reg_write(UC_X86_REG_EIP, read_u32(uc, sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4 * (1 + arguments))

    hook = uc.hook_add(UC_HOOK_CODE, substitute)
    try:
        run_checked(uc, LOOKUP, RET_MAGIC, count=200_000)
    finally:
        uc.hook_del(hook)
    if calls != {HASH_STUB: 1, GET_DLG_CTRL_ID_STUB: 1}:
        raise RuntimeError(f"Unexpected lookup calls: {calls}")
    pointer = uc.reg_read(UC_X86_REG_EAX)
    if not pointer:
        return None
    label = bytearray()
    while byte := uc.mem_read(pointer + len(label), 1)[0]:
        label.append(byte)
    return label.decode("ascii")


def generate():
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    image = Image(uc)
    cases = []
    for dialog_id in DIALOGS:
        for control in parse_template(image.dialog_template(dialog_id)):
            # GetDlgCtrlID returns the id's low word as a signed int; -1
            # (IDC_STATIC) takes the lookup's early return.
            control_id = control["id"] & 0xFFFF
            if control_id == 0xFFFF:
                continue
            cases.append({"dialog_id": dialog_id, "control_id": control_id,
                          "class": control["class"], "help": help_label(dialog_id, control_id)})
    return {"source": "unicorn/gamemd.exe", "cases": cases}


if __name__ == "__main__":
    finish_vectors(
        generate, Path(__file__).with_suffix(".json"),
        provenance=lambda: provenance(
            scope="The status-line help table lookup 0x006040B0 for every control of 15 offline shell dialogs' RT_DIALOG templates",
            assumptions=[
                "The parent dialog's record is found by the original lookup loop in a supplied one-bucket table; record fields other than HWND, next and dialog id stay zero",
                "Only the table fallback is executed: a child's own 0x4E8 answer and the dialog proc's 0x4E9 answer, which the common handler asks first, are established separately",
            ],
            substitutions=[
                "Record-table hash function pointer [AC1B18] returns bucket 0",
                "GetDlgCtrlID IAT pointer returns the template control id",
            ],
            entry_points={"help_lookup": LOOKUP},
        ),
    )
