"""Original keyboard registry, metadata operands and assignment mutation.

Registration executes532150 through its call to533D20 (before file I/O).
Assignment executes original modifier checks and table mutation blocks from
5FB320, with an already sorted supplied table and admitted selected command.
No HWND, Windows capture, file persistence, allocation-failure or paint claims.
"""

from pathlib import Path
import hashlib
import struct

from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_ESP, UC_X86_REG_EIP,
)
from tools.native_oracle import (
    SCRATCH, STACK_BASE, STACK_SIZE, configured_gamemd, finish_vectors,
    load_image, provenance, run_checked,
)
from tools.sidebar_oracle.stock import mix, mix_hash


class KeyboardFixture:
    def __init__(self):
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.uc)
        self.uc.mem_map(STACK_BASE, STACK_SIZE)
        self.uc.mem_map(SCRATCH, 0x100000)
        self.stack = STACK_BASE + STACK_SIZE - 0x1000
        self.heap = SCRATCH + 0x1000
        self.uc.hook_add(UC_HOOK_CODE, self.malloc)
        self.put(0x87F65C, SCRATCH)
        self.put(0x87F660, 128)
        self.put(0x87F668, 0)
        self.uc.mem_write(0xA8B8B4, b"\0")
        self.uc.reg_write(UC_X86_REG_ESP, self.stack)
        run_checked(self.uc, 0x532150, 0x533D20, count=100000,
                    required_addresses=[0x532150])
        if self.get(0x87F668) != 87:
            raise RuntimeError("Original ordinary command registry changed")
        self.catalog = self.metadata()
        self.objects = {row["ini_name"]: self.get(SCRATCH + i * 4)
                        for i, row in enumerate(self.catalog)}
        self.names = {obj: name for name, obj in self.objects.items()}
        self.registers = self.uc.context_save()

    def put(self, address, value):
        self.uc.mem_write(address, struct.pack("<I", value))

    def get(self, address):
        return struct.unpack("<I", self.uc.mem_read(address, 4))[0]

    def malloc(self, uc, address, size, data):
        if address != 0x7C8E17:
            return
        sp = uc.reg_read(UC_X86_REG_ESP)
        size = self.get(sp + 4)
        result = self.heap
        self.heap += (size + 15) & ~15
        if self.heap >= SCRATCH + 0x8000:
            raise RuntimeError("Unexpected registration allocation")
        uc.reg_write(UC_X86_REG_EAX, result)
        uc.reg_write(UC_X86_REG_ESP, sp + 4)
        uc.reg_write(UC_X86_REG_EIP, self.get(sp))

    def ascii(self, address):
        return bytes(self.uc.mem_read(address, 200)).split(b"\0")[0].decode("ascii")

    def metadata(self):
        cs = Cs(CS_ARCH_X86, CS_MODE_32)
        result = []
        for index in range(self.get(0x87F668)):
            obj = self.get(SCRATCH + index * 4)
            vtable = self.get(obj)
            values, getters = [], []
            for slot in (4, 8, 12, 16):
                getter = self.get(vtable + slot)
                getters.append(getter)
                ins = list(cs.disasm(bytes(self.uc.mem_read(getter, 60)), getter))
                if slot == 4:
                    if ins[0].op_str.startswith("eax, 0x"):
                        address = int(ins[0].op_str.split(", ")[1], 16)
                    else:
                        address = next(int(i.op_str, 16) for i in ins
                                       if i.mnemonic == "push" and i.op_str.startswith("0x82"))
                else:
                    address = next(int(i.op_str.split(", ")[1], 16) for i in ins
                                   if i.mnemonic == "mov" and i.op_str.startswith("ecx, 0x82"))
                values.append(self.ascii(address))
            name, label, category, description = values
            parameter = self.get(obj + 4) if "%d" in name else None
            result.append(dict(index=index, vtable=vtable, getters=getters,
                               ini_name=name % parameter if parameter else name,
                               name_key=label, category_key=category,
                               description_key=description, parameter=parameter))
        return result

    def assign(self, name, before, command, encoded):
        uc = self.uc
        uc.context_restore(self.registers)
        uc.mem_write(self.stack, bytes(0x200))
        uc.reg_write(UC_X86_REG_ESP, self.stack)
        uc.reg_write(UC_X86_REG_EDI, encoded)
        self.put(self.stack + 0x10, self.objects[command])
        table = SCRATCH + 0x9000
        uc.mem_write(table, bytes(0x1000))
        for i, (key, owner) in enumerate(sorted(before)):
            self.put(table + i * 8, key)
            self.put(table + i * 8 + 4, self.objects[owner])
        self.put(0x87F680, table)
        self.put(0x87F684, len(before))
        self.put(0x87F688, 128)
        uc.mem_write(0x87F68C, b"\1")
        self.put(0x87F690, 0)
        end = run_checked(uc, 0x5FBB38, (0x5FBB47, 0x5FBB6F, 0x5FBBAC),
                          count=1000, required_addresses=[0x48BB40])
        error = {0x5FBB47: "CannotMap", 0x5FBB6F: "CannotRemap"}.get(end)
        if error is None:
            run_checked(uc, 0x5FBBAC, (0x5FBED7, 0x5FBDF1), count=2000,
                        required_addresses=[0x5FBCC7])
        after = sorted((self.get(table + i * 8),
                        self.names[self.get(table + i * 8 + 4)])
                       for i in range(self.get(0x87F684)))
        return dict(name=name, before=[list(row) for row in sorted(before)],
                    command=command, encoded=encoded, error=error,
                    after=[list(row) for row in after])


def stock_csf():
    data = mix((configured_gamemd().parent / "langmd.mix").read_bytes())[mix_hash("ra2md.csf")]
    offset, entries = 24, {}
    while data[offset:offset + 4] == b" LBL":
        count, size = struct.unpack_from("<II", data, offset + 4)
        offset += 12
        key = data[offset:offset + size].decode("ascii").upper()
        offset += size
        for _ in range(count):
            tag = data[offset:offset + 4]
            size = struct.unpack_from("<I", data, offset + 4)[0]
            offset += 8
            value = bytes(x ^ 255 for x in data[offset:offset + size * 2]).decode("utf-16-le")
            offset += size * 2
            if tag == b"WRTS":
                size = struct.unpack_from("<I", data, offset)[0]
                offset += 4 + size
            entries[key] = value
    return entries, hashlib.sha256(data).hexdigest()


def generate():
    fixture = KeyboardFixture()
    csf, csf_hash = stock_csf()
    for row in fixture.catalog:
        for field in ("name", "category", "description"):
            value = csf[row[field + "_key"].upper()]
            row[field + "_format"] = value
    cases = [
        ("empty", [], "StopObject", 83),
        ("free", [(83, "StopObject")], "StopObject", 81),
        ("collision", [(68, "DeployObject"), (83, "StopObject")], "StopObject", 68),
        ("same", [(83, "StopObject")], "StopObject", 83),
        ("unassign", [(83, "StopObject")], "StopObject", 0),
        ("delete_two_unassign", [(46, "Delete"), (110, "Delete")], "Delete", 0),
        ("delete_two_reassign", [(46, "Delete"), (110, "Delete")], "Delete", 88),
        ("replace_second_delete", [(46, "Delete"), (83, "StopObject"), (110, "Delete")], "StopObject", 110),
        ("modified_collision", [(595, "StopObject"), (68, "DeployObject")], "DeployObject", 595),
    ]
    # Every ordinary modifier combination for normal, Shift-sensitive and
    # any-modifier commands, plus conflict checks against those base owners.
    for command, key in (("StopObject", 83), ("TypeSelect", 84),
                         ("HealthNav", 72), ("PlanningMode", 90)):
        for modifiers in range(8):
            cases.append((f"selected_{command}_{modifiers}", [(key, command)],
                          command, key | (modifiers << 8)))
            cases.append((f"base_{command}_{modifiers}",
                          [(key, command), (68, "DeployObject")],
                          "DeployObject", key | (modifiers << 8)))
    return dict(source="unicorn/gamemd.exe", catalog=fixture.catalog,
                csf=dict(archive="langmd.mix", name="ra2md.csf", sha256=csf_hash),
                assignments=[fixture.assign(*case) for case in cases])


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=lambda: provenance(
        scope="87 ordinary registered command objects and their original metadata operands;73 admitted keyboard assignment cases",
        assumptions=[
            "Debug registration flag A8B8B4 is zero; registry capacity128 and writable storage supplied",
            "Assignment selected command is supplied at original stack+10; EDI is the admitted encoded key",
            "Supplied key table is sorted, unique by encoded key, has capacity128 and ordinary u16 key values",
            "Catalog metadata reads original getter operands; CSF parsing is input preparation, not emulated string loading",
            "No GUI event admission/capture, full dialog, disk INI write/reset, non-English collation or allocation-failure claims",
        ],
        substitutions=[
            "Malloc7C8E17 returns bounded zero-filled scratch storage during original registration",
            "Registration stops before533D20 file I/O; assignment starts after HWND/list/capture lookup",
            "Modifier rejection stops before GUI error-label calls; successful mutation executes original lookup/removal/insertion",
        ],
        entry_points={"registry": 0x532150, "registry_end": 0x533D20,
                      "assignment_checks": 0x5FBB38, "assignment_mutation": 0x5FBBAC,
                      "selected_modifier_check": 0x48BB40, "base_modifier_check": 0x48BB60}))
