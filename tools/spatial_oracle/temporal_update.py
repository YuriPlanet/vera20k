"""Original TemporalClass::Update 0x0071A760 (the Chrono Legionnaire's warp tick).

Each case builds a head TemporalClass, its chained attackers and their owners,
and the warped target, then calls the original Update once. The chain damage
sum (SumChainDamage 0x0071AB10 with its depth cap), the WarpRemaining step and
its `<= 0` erase test, the open-topped distance release (the original
Sqrt_Approx 0x004CAC40, ftol 0x007C5F00 and LetGo 0x0071ABC0 run) and the
corrupt-head release (ClearLinkedList 0x0071ADE0) execute. Every virtual and the
erase callees are recorded at entry and return without running: GetCoords,
SelectWeapon, GetWeapon, GetTechnoType, the type cost, WhatAmI, the occupant
count, Enter_Idle_Mode, the unit-lost notice, Record_The_Kill, UnInit, Mark,
operator new, the AnimClass constructor, VeterancyStruct::Add, the occupant
kill, the bunker and slave releases and the building back-online call.

Rust consumer: src/sim/temporal_tests.rs.
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
UPDATE = 0x71A760
RULES = SCRATCH + 0x1000
TARGET, TARGET_VT, TARGET_TYPE, TARGET_HOUSE = (SCRATCH + 0x2000, SCRATCH + 0x3000,
                                               SCRATCH + 0x4000, SCRATCH + 0x5000)
STUBS = SCRATCH + 0x6000
NEWBUF = SCRATCH + 0x8000
# Attacker n: its TemporalClass, its owner, the owner's vtable, type, weapon.
ATTACKERS = SCRATCH + 0x10000
ATTACKER_STRIDE = 0x2000
WARP_AWAY, OVERLOAD_MARKER = 0x5A5A0340, 0
VIRTUALS = {  # vtable offset -> (name, stack bytes cleaned)
    0x48: ("get_coords", 4), 0x2E4: ("select_weapon", 4), 0x3F8: ("get_weapon", 4),
    0x84: ("get_type", 0), 0x2C: ("what_am_i", 0), 0x408: ("occupants", 0),
    0x484: ("idle", 8), 0x3B8: ("unit_lost", 4), 0xE0: ("record_kill", 4),
    0xF8: ("uninit", 0), 0x124: ("mark", 4),
}
NATIVES = {  # address -> (name, stack bytes cleaned, cdecl)
    0x7C8E17: ("new", 0, True), 0x421EA0: ("anim", 0x1C, False),
    0x74FF50: ("veterancy_add", 8, False), 0x4585C0: ("kill_occupants", 4, False),
    0x4593A0: ("bunker_release", 0, False), 0x459470: ("leave_bunker", 0, False),
    0x6B0AE0: ("free_slaves", 8, False), 0x452210: ("building_online", 0, False),
    0x6CB4D0: ("super_reset", 4, False),
}
# A TechnoType vtable whose +0x84 (the cost for a house) is a stub.
TYPE_VT, TYPE_COST = SCRATCH + 0x7000, SCRATCH + 0x7800


def node(n):
    return ATTACKERS + n * ATTACKER_STRIDE


def owner(n):
    return node(n) + 0x100


def owner_vt(n):
    return node(n) + 0x800


def owner_type(n):
    return node(n) + 0x1000


def weapon_struct(n):
    return node(n) + 0x1800


class Fixture:
    def __init__(self):
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.u)
        self.u.mem_map(SCRATCH, REGION)
        self.u.mem_map(STACK_BASE, STACK_SIZE)
        self.u.mem_map(RET_MAGIC, 0x1000)
        self.events = []
        self.objects = {}
        self.u.hook_add(UC_HOOK_CODE, self.observe)

    def word(self, address):
        return struct.unpack("<I", self.u.mem_read(address, 4))[0]

    def signed(self, address):
        return struct.unpack("<i", self.u.mem_read(address, 4))[0]

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
        if STUBS <= pc < STUBS + 0x1000:
            offset = pc - STUBS
            name, cleaned = VIRTUALS[offset]
            obj = self.objects.get(this, {})
            args = [self.word(sp + 4 * (i + 1)) for i in range(cleaned // 4)]
            value = 0
            if name == "get_coords":
                u.mem_write(args[0], struct.pack("<iii", *obj["coords"]))
                value = args[0]
            elif name == "select_weapon":
                value = 0
            elif name == "get_weapon":
                value = obj["weapon_struct"]
            elif name == "get_type":
                value = obj["type"]
            elif name == "what_am_i":
                value = obj["rtti"]
            elif name == "occupants":
                value = obj.get("occupants", 0)
            else:
                self.events.append([name, self.name_of(this)] + [self.name_of(a) for a in args])
            self.ret(value, cleaned)
        elif pc == TYPE_COST:
            self.ret(self.objects[this]["cost"], 4)
        elif pc in NATIVES:
            name, cleaned, cdecl = NATIVES[pc]
            count = 7 if name == "anim" else cleaned // 4
            args = [self.word(sp + 4 * (i + 1)) for i in range(count)]
            if name == "new":
                self.ret(NEWBUF, 0)
                return
            if name == "anim":
                coord = list(struct.unpack("<iii", u.mem_read(args[1], 12)))
                self.events.append(["anim", self.name_of(args[0]), coord] + args[2:])
            elif name == "veterancy_add":
                self.events.append(["veterancy_add", self.name_of(this - 0x150),
                                    self.signed(sp + 4), self.signed(sp + 8)])
            else:
                self.events.append([name, self.name_of(this)] + [self.name_of(a) for a in args])
            self.ret(0, cleaned)

    def techno(self, address, vtable, name, **data):
        self.u.mem_write(address, dwords(vtable))
        self.objects[address] = data
        self.names[name] = address

    def vtable(self, address):
        for offset in VIRTUALS:
            self.u.mem_write(address + offset, dwords(STUBS + offset))

    def call(self, address, ecx):
        u = self.u
        u.mem_write(SP, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, ecx)
        run_checked(u, address, RET_MAGIC, count=2_000_000)

    def execute(self, case):
        u = self.u
        u.mem_write(SCRATCH, bytes(REGION))
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        self.events, self.objects, self.names = [], {}, {}
        for offset in VIRTUALS:
            u.mem_write(STUBS + offset, b"\xCC")
        u.mem_write(TYPE_COST, b"\xCC")
        u.mem_write(TYPE_VT + 0x84, dwords(TYPE_COST))
        u.mem_write(0x8871E0, dwords(RULES))
        u.mem_write(RULES + 0xF60, dwords(case.get("open_topped_distance", 7)))
        u.mem_write(RULES + 0x340, dwords(WARP_AWAY))
        self.names["warp_away"] = WARP_AWAY
        # The target: a Unit (1) or Building (6) at Location +9C.
        self.vtable(TARGET_VT)
        target = case["target"]
        self.techno(TARGET, TARGET_VT, "target", coords=target["coords"], type=TARGET_TYPE,
                    rtti=target["rtti"], occupants=target.get("occupants", 0))
        u.mem_write(TARGET + 0x9C, struct.pack("<iii", *target["coords"]))
        u.mem_write(TARGET + 0x21C, dwords(TARGET_HOUSE))
        u.mem_write(TARGET + 0x270, b"\x01")
        u.mem_write(TARGET_TYPE, dwords(TYPE_VT))
        self.objects[TARGET_TYPE] = dict(cost=target.get("cost", 900))
        self.names["target_house"] = TARGET_HOUSE
        # Attackers in chain order; attacker 0 is the head.
        attackers = case["attackers"]
        for n, attacker in enumerate(attackers):
            self.vtable(owner_vt(n))
            self.techno(owner(n), owner_vt(n), f"owner{n}", coords=attacker.get("coords", [0, 0, 0]),
                        type=owner_type(n), weapon_struct=weapon_struct(n))
            u.mem_write(owner(n) + 0x82, bytes([attacker.get("open_topped", 0)]))
            u.mem_write(owner(n) + 0x21C, dwords(SCRATCH + 0x5800))
            u.mem_write(owner_type(n), dwords(TYPE_VT))
            u.mem_write(owner_type(n) + 0xC8E, bytes([attacker.get("trainable", 1)]))
            self.objects[owner_type(n)] = dict(cost=attacker.get("cost", 1500))
            u.mem_write(weapon_struct(n), dwords(weapon_struct(n) + 0x100))
            u.mem_write(weapon_struct(n) + 0x100 + 0xA4, dwords(attacker["damage"]))
            t = node(n)
            self.names[f"temporal{n}"] = t
            u.mem_write(t + 0x24, dwords(owner(n)))
            u.mem_write(t + 0x28, dwords(TARGET))
            u.mem_write(t + 0x40, dwords(node(n - 1) if n > 0 else 0))
            u.mem_write(t + 0x44, dwords(node(n + 1) if n + 1 < len(attackers) else 0))
            u.mem_write(owner(n) + 0x274, dwords(t))
        u.mem_write(node(0) + 0x48, dwords(case["warp_remaining"]))
        if case.get("corrupt_head"):
            # The last attacker becomes a predecessor of the head: head.Prev =
            # last, last.Next = head, detached from the tail it was on.
            last = len(attackers) - 1
            u.mem_write(node(0) + 0x40, dwords(node(last)))
            u.mem_write(node(last) + 0x44, dwords(node(0)))
            u.mem_write(node(last) + 0x40, dwords(0))
            u.mem_write(node(last - 1) + 0x44, dwords(0))
        u.mem_write(TARGET + 0x278, dwords(node(0)))
        if case.get("detached"):
            # The head lost its Target (a non-removal expiry) while the victim
            # still names it.
            u.mem_write(node(0) + 0x28, dwords(0))
        self.call(UPDATE, node(0))
        after = []
        for n in range(len(attackers)):
            t = node(n)
            after.append(dict(target=self.name_of(self.word(t + 0x28)),
                              prev=self.name_of(self.word(t + 0x40)),
                              next=self.name_of(self.word(t + 0x44)),
                              warp_remaining=self.signed(t + 0x48),
                              warp_per_step=self.signed(t + 0x4C)))
        return dict(input=case, events=self.events, attackers=after,
                    target_head=self.name_of(self.word(TARGET + 0x278)),
                    target_warped=u.mem_read(TARGET + 0x270, 1)[0])


def unit_at(x, y, z):
    return dict(rtti=1, coords=[x, y, z])


def inputs():
    cases = [
        # One Chrono Legionnaire on a Rhino (Strength 400).
        dict(name="one_step", attackers=[dict(damage=8)], warp_remaining=4000,
             target=unit_at(4000, 4000, 0)),
        dict(name="erase_at_zero", attackers=[dict(damage=8)], warp_remaining=8,
             target=unit_at(4000, 4000, 0)),
        dict(name="one_left", attackers=[dict(damage=8)], warp_remaining=9,
             target=unit_at(4000, 4000, 0)),
        dict(name="already_spent", attackers=[dict(damage=8)], warp_remaining=0,
             target=unit_at(4000, 4000, 0)),
        dict(name="elite_step", attackers=[dict(damage=16)], warp_remaining=1250,
             target=unit_at(4000, 4000, 0)),
        dict(name="untrainable_erase", attackers=[dict(damage=5, trainable=0)], warp_remaining=3,
             target=unit_at(4000, 4000, 0)),
        dict(name="chain_of_three", attackers=[dict(damage=8), dict(damage=16), dict(damage=5)],
             warp_remaining=100, target=unit_at(4000, 4000, 0)),
        dict(name="chain_erase", attackers=[dict(damage=8), dict(damage=16)], warp_remaining=24,
             target=unit_at(4000, 4000, 0)),
        dict(name="depth_cap", attackers=[dict(damage=1) for _ in range(60)], warp_remaining=1000,
             target=unit_at(4000, 4000, 0)),
        dict(name="corrupt_head", attackers=[dict(damage=8), dict(damage=8), dict(damage=16)], warp_remaining=100,
             corrupt_head=True, target=unit_at(4000, 4000, 0)),
        dict(name="building_erase", attackers=[dict(damage=8)], warp_remaining=4,
             target=dict(rtti=6, coords=[5000, 5000, 0], occupants=2)),
        dict(name="building_step", attackers=[dict(damage=8)], warp_remaining=4000,
             target=dict(rtti=6, coords=[5000, 5000, 0])),
        dict(name="detached_erase", attackers=[dict(damage=8)], warp_remaining=8,
             detached=True, target=unit_at(4000, 4000, 0)),
    ]
    # The open-topped release: owner at the origin, target at a distance.
    for name, coords in (("straight_1792", [1792, 0, 0]), ("straight_1793", [1793, 0, 0]),
                         ("straight_1794", [1794, 0, 0]), ("straight_1795", [1795, 0, 0]),
                         ("straight_1796", [1796, 0, 0]), ("straight_1800", [1800, 0, 0]),
                         ("negative_1795", [-1795, 0, 0]), ("diagonal_1270", [1270, 1270, 0]),
                         ("diagonal_1267", [1267, 1267, 0]), ("diagonal_1268", [1268, 1267, 0]),
                         ("cube_1034", [1034, 1034, 1034]), ("cube_1035", [1035, 1035, 1035]),
                         ("near_1791", [1791, 45, 12]), ("near_1792", [1792, 1, 1]),
                         ("vertical_1793", [0, 0, 1793])):
        cases.append(dict(name=f"open_topped_{name}",
                          attackers=[dict(damage=5, open_topped=1, coords=[0, 0, 0])],
                          warp_remaining=4000, target=unit_at(*coords)))
    cases.append(dict(name="open_topped_chained_release",
                      attackers=[dict(damage=5, open_topped=1, coords=[0, 0, 0]), dict(damage=8)],
                      warp_remaining=4000, target=unit_at(1900, 0, 0)))
    return cases


def generate():
    fixture = Fixture()
    return [fixture.execute(case) for case in inputs()]


def metadata():
    return provenance(
        scope="Original 0x0071A760 TemporalClass::Update with SumChainDamage, LetGo, ClearLinkedList, Sqrt_Approx and ftol",
        assumptions=[
            "TemporalClass +24 Owner, +28 Target, +40 Prev, +44 Next, +48 WarpRemaining, +4C WarpPerStep",
            "Owner +82 InOpenTransport, +150 VeterancyStruct, +274 TemporalImUsing; target +270 warped, +278 chain head, +9C Location",
            "Rules +F60 OpenToppedWarpDistance, +340 WarpAway",
        ],
        substitutions=[
            "Recorded at entry and returned without running: GetCoords, SelectWeapon (0), GetWeapon, GetTechnoType, the type cost, WhatAmI, the occupant count, Enter_Idle_Mode, the unit-lost notice, Record_The_Kill, UnInit, Mark, operator new, AnimClass constructor, VeterancyStruct::Add, occupant kill, bunker and slave releases, building back-online",
        ],
        entry_points={"update": UPDATE, "sum_chain": 0x71AB10, "let_go": 0x71ABC0,
                      "clear_linked_list": 0x71ADE0})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
