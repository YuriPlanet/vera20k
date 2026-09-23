"""Original HouseClass::Blowup_All 0x004FC6D0 (a defeated house's objects die).

Each case builds a TechnoClass::Array of Technos with owners, mind-control
links, trigger ownership and Temporal heads, then calls the original
Blowup_All once for one house. TechnoClass::GetOriginalOwner 0x0070F820,
CaptureManagerClass::GetOriginalOwner 0x004722F0 and SetOriginalOwnerToCivilian
0x00472330 execute. ReceiveDamage (vtable +0x16C) is recorded at entry with its
seven arguments and returns without running (optionally removing the object
from the array, to show the sweep's slot re-read); the side lookup 0x006A46D0
returns the case's Civilian side index; the Temporal chain release 0x0071AD40
is recorded.

Rust consumer: src/sim/world/house_defeat_tests.rs.
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_FPCW

from tools.native_oracle import (load_image, run_checked, SCRATCH, STACK_BASE, STACK_SIZE,
                                 RET_MAGIC, NATIVE_FPCW, finish_vectors, provenance)
from tools.spatial_oracle.map_queries import dwords

REGION = 0x100000
SP = STACK_BASE + STACK_SIZE - 0x1000
BLOWUP_ALL = 0x4FC6D0
TECHNO_ITEMS, TECHNO_COUNT = 0xA8EC7C, 0xA8EC88
HOUSE_ITEMS, HOUSE_COUNT = 0xA8022C, 0xA80238
RULES_PTR = 0x8871E0
SIDE_LOOKUP, TEMPORAL_RELEASE = 0x6A46D0, 0x71AD40
RULES = SCRATCH + 0x1000
C4 = SCRATCH + 0x1800
VT = SCRATCH + 0x2000
RECEIVE_STUB = SCRATCH + 0x2800
ARRAY = SCRATCH + 0x3000
HOUSES = SCRATCH + 0x4000           # house n at HOUSES + n * 0x400; its type at +0x200
HOUSE_ARRAY = SCRATCH + 0x7000
OBJECTS = SCRATCH + 0x10000         # techno n at OBJECTS + n * 0x800
MANAGERS = SCRATCH + 0x40000        # manager n at MANAGERS + n * 0x200, nodes after it
RECEIVE_DAMAGE = 0x16C


def house(n):
    return HOUSES + n * 0x400


def techno(n):
    return OBJECTS + n * 0x800


def manager(n):
    return MANAGERS + n * 0x200


class Fixture:
    def __init__(self):
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.u)
        self.u.mem_map(SCRATCH, REGION)
        self.u.mem_map(STACK_BASE, STACK_SIZE)
        self.u.mem_map(RET_MAGIC, 0x1000)
        self.u.hook_add(UC_HOOK_CODE, self.observe)

    def word(self, address):
        return struct.unpack("<I", self.u.mem_read(address, 4))[0]

    def ret(self, value, cleaned):
        sp = self.u.reg_read(UC_X86_REG_ESP)
        self.u.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        self.u.reg_write(UC_X86_REG_EIP, self.word(sp))
        self.u.reg_write(UC_X86_REG_ESP, sp + 4 + cleaned)

    def name_of(self, address):
        for name, candidate in self.names.items():
            if candidate == address:
                return name
        return hex(address)

    def observe(self, u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        this = u.reg_read(UC_X86_REG_ECX)
        if pc == RECEIVE_STUB:
            args = [self.word(sp + 4 * (i + 1)) for i in range(7)]
            damage = struct.unpack("<i", u.mem_read(args[0], 4))[0]
            self.events.append(["receive_damage", self.name_of(this), damage, args[1],
                                self.name_of(args[2]), args[3], args[4], args[5], args[6]])
            if self.remove_on_damage.get(self.name_of(this)):
                count = self.word(TECHNO_COUNT)
                items = [self.word(ARRAY + 4 * i) for i in range(count)]
                items.remove(this)
                for i, item in enumerate(items):
                    u.mem_write(ARRAY + 4 * i, dwords(item))
                u.mem_write(TECHNO_COUNT, dwords(count - 1))
            self.ret(0, 0x1C)
        elif pc == SIDE_LOOKUP:
            self.ret(self.civilian_side, 0)
        elif pc == TEMPORAL_RELEASE:
            self.events.append(["temporal_release", self.name_of(this)])
            self.ret(0, 0)

    def execute(self, case):
        u = self.u
        u.mem_write(SCRATCH, bytes(REGION))
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        self.events, self.names = [], {}
        self.remove_on_damage = {}
        self.civilian_side = case.get("civilian_side", 99)
        u.mem_write(RECEIVE_STUB, b"\xCC")
        u.mem_write(VT + RECEIVE_DAMAGE, dwords(RECEIVE_STUB))
        u.mem_write(RULES_PTR, dwords(RULES))
        u.mem_write(RULES + 0xFA8, dwords(C4))
        self.names["c4"] = C4
        # Houses: HouseClass::Array with each house's HouseType side index (+0xBC).
        houses = case["houses"]
        for n, side in enumerate(houses):
            self.names[f"house{n}"] = house(n)
            u.mem_write(house(n) + 0x34, dwords(house(n) + 0x200))
            u.mem_write(house(n) + 0x200 + 0xBC, dwords(side))
            u.mem_write(HOUSE_ARRAY + 4 * n, dwords(house(n)))
        u.mem_write(HOUSE_ITEMS, dwords(HOUSE_ARRAY))
        u.mem_write(HOUSE_COUNT, dwords(len(houses)))
        # Technos in array order.
        objects = case["objects"]
        for n, obj in enumerate(objects):
            t = techno(n)
            self.names[f"obj{n}"] = t
            u.mem_write(t, dwords(VT))
            u.mem_write(t + 0x21C, dwords(house(obj["owner"])))
            u.mem_write(t + 0x6C, struct.pack("<i", obj.get("health", 100)))
            if obj.get("warped"):
                u.mem_write(t + 0x278, dwords(t + 0x700))
                self.names[f"head{n}"] = t + 0x700
            if "trigger_owner" in obj:
                u.mem_write(t + 0x2CC, dwords(1))
                u.mem_write(t + 0x2E0, dwords(house(obj["trigger_owner"])))
            if obj.get("remove"):
                self.remove_on_damage[f"obj{n}"] = True
            u.mem_write(ARRAY + 4 * n, dwords(t))
        # Mind control: controller index -> list of (victim index, original owner house).
        for c, (controller, nodes) in enumerate(case.get("controllers", {}).items()):
            m = manager(c)
            controller = int(controller)
            u.mem_write(techno(controller) + 0x2BC, dwords(m))
            node_items = m + 0x100
            u.mem_write(m + 0x28, dwords(node_items))
            u.mem_write(m + 0x34, dwords(len(nodes)))
            for k, (victim, original) in enumerate(nodes):
                node_address = m + 0x180 + 8 * k
                u.mem_write(node_items + 4 * k, dwords(node_address))
                u.mem_write(node_address, dwords(techno(victim)) + dwords(house(original)))
                u.mem_write(techno(victim) + 0x2C0, dwords(techno(controller)))
        u.mem_write(TECHNO_ITEMS, dwords(ARRAY))
        u.mem_write(TECHNO_COUNT, dwords(len(objects)))
        u.mem_write(SP, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, house(case["house"]))
        run_checked(u, BLOWUP_ALL, RET_MAGIC, count=500_000)
        nodes = {}
        for c, (controller, entries) in enumerate(case.get("controllers", {}).items()):
            m = manager(c)
            nodes[controller] = [self.name_of(self.word(m + 0x180 + 8 * k + 4))
                                 for k in range(len(entries))]
        return dict(input=case, events=self.events, original_owners=nodes)


def inputs():
    obj = lambda owner, **extra: dict(owner=owner, **extra)
    return [
        dict(name="own_objects_in_array_order", house=0, houses=[0, 1],
             objects=[obj(0, health=100), obj(1), obj(0, health=250), obj(0, health=0)]),
        dict(name="nothing_owned", house=0, houses=[0, 1], objects=[obj(1), obj(1)]),
        dict(name="empty_array", house=0, houses=[0], objects=[]),
        dict(name="captured_by_other_to_civilian", house=0, houses=[0, 1, 2], civilian_side=2,
             objects=[obj(1), obj(1, health=90), obj(0)],
             controllers={"0": [[1, 0]]}),
        dict(name="captured_by_other_no_civilian", house=0, houses=[0, 1], civilian_side=5,
             objects=[obj(1), obj(1, health=90), obj(0)],
             controllers={"0": [[1, 0]]}),
        dict(name="our_captive_survives", house=0, houses=[0, 1],
             objects=[obj(0), obj(0, health=70)],
             controllers={"0": [[1, 1]]}),
        dict(name="trigger_owned_elsewhere_dies", house=0, houses=[0, 1],
             objects=[obj(1, trigger_owner=0, health=60), obj(1, trigger_owner=1)]),
        dict(name="warped_object_released_first", house=0, houses=[0, 1],
             objects=[obj(0, warped=True, health=400)]),
        dict(name="removal_rereads_the_slot", house=0, houses=[0, 1],
             objects=[obj(0, remove=True, health=10), obj(0, health=20), obj(1)]),
        dict(name="two_captives_one_controller", house=0, houses=[0, 1, 2], civilian_side=2,
             objects=[obj(1), obj(1, health=11), obj(1, health=22)],
             controllers={"0": [[1, 0], [2, 0]]}),
    ]


def generate():
    fixture = Fixture()
    return [fixture.execute(case) for case in inputs()]


def metadata():
    return provenance(
        scope="Original 0x004FC6D0 HouseClass::Blowup_All with GetOriginalOwner 0x0070F820, CaptureManagerClass::GetOriginalOwner 0x004722F0 and SetOriginalOwnerToCivilian 0x00472330",
        assumptions=[
            "TechnoClass::Array items [0xA8EC7C], count [0xA8EC88]; HouseClass::Array items [0xA8022C], count [0xA80238]",
            "Techno +21C Owner, +6C Health, +278 Temporal head, +2BC CaptureManager, +2C0 MindControlledBy, +2CC/+2E0 trigger original owner; House +34 HouseType, HouseType +BC side; CaptureManager +28 node items, +34 count; node {victim, original owner}; Rules +FA8 C4Warhead",
        ],
        substitutions=[
            "Recorded at entry and returned without running: ReceiveDamage (vtable +0x16C; optionally removes the object from the array), the SideClass lookup 0x006A46D0 (returns the case's Civilian side), the Temporal chain release 0x0071AD40",
        ],
        entry_points={"blowup_all": BLOWUP_ALL, "get_original_owner": 0x70F820,
                      "capture_original_owner": 0x4722F0, "set_original_owner_to_civilian": 0x472330})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
