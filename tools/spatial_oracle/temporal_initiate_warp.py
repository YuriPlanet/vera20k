"""Original TemporalClass::InitiateWarp 0x0071AF20 (a Chrono shot starting a warp).

Each case builds the firing attacker's TemporalClass and owner, the target and
any other attackers already linked to it, then calls the original InitiateWarp
once. CanWarpTarget 0x0071AE50, LetGo 0x0071ABC0 and RadioClass::
Contact_With_Whom 0x0065AD30 execute; so do the head/insert branches, the
`Strength * 10` store and the `+0x270` write. Every virtual and the other
callees are recorded at entry and return without running: GetTechnoType,
WhatAmI, the centre coordinate, the building pre-check, Mark, Deselect, the
Iron Curtain test, Kill_All_Spawns, CaptureManager FreeAll, the map cell and
building lookups, CreateRadarEvent, the EVA, NotifyUnderAttack, the gattling
stage and the building offline/online calls.

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
INITIATE_WARP = 0x71AF20
PLAYER_PTR = 0xA83D4C
STUBS = SCRATCH + 0x1000
TARGET = SCRATCH + 0x2000
TARGET_VT = SCRATCH + 0x3000
TARGET_TYPE = SCRATCH + 0x4000
TARGET_HOUSE = SCRATCH + 0x6000
LOCAL_HOUSE = SCRATCH + 0x7000
FACTORY, FACTORY_VT, FACTORY_TYPE = SCRATCH + 0x8000, SCRATCH + 0x9000, SCRATCH + 0xA000
OTHER_TARGET, OTHER_VT = SCRATCH + 0xC000, SCRATCH + 0xD000
CONTACTS = SCRATCH + 0xE000
CELL = SCRATCH + 0xE800
SPAWNS, CAPTIVES = SCRATCH + 0xF000, SCRATCH + 0xF400
# Attacker n: its TemporalClass, its owner (+0x100) and the owner's vtable.
ATTACKERS = SCRATCH + 0x10000
ATTACKER_STRIDE = 0x2000
VIRTUALS = {  # vtable offset -> (name, stack bytes cleaned)
    0x84: ("get_type", 0), 0x2C: ("what_am_i", 0), 0x4C: ("centre", 8),
    0x80: ("precheck", 0), 0x124: ("mark", 4), 0x150: ("deselect", 0),
    0x160: ("iron_curtain", 0),
}
NATIVES = {  # address -> (name, stack bytes cleaned)
    0x6B7100: ("kill_spawns", 0), 0x472140: ("free_all", 0),
    0x565730: ("map_cell", 4), 0x47C520: ("cell_building", 0),
    0x65FA70: ("radar_event", 4), 0x752700: ("eva", 4),
    0x4F93E0: ("notify_under_attack", 4), 0x70E000: ("gattling", 4),
    0x4521C0: ("building_offline", 0), 0x452210: ("building_online", 0),
}


def node(n):
    return ATTACKERS + n * ATTACKER_STRIDE


def owner(n):
    return node(n) + 0x100


def owner_vt(n):
    return node(n) + 0x800


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

    def signed(self, address):
        return struct.unpack("<i", self.u.mem_read(address, 4))[0]

    def byte(self, address):
        return self.u.mem_read(address, 1)[0]

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
            name, cleaned = VIRTUALS[pc - STUBS]
            obj = self.objects.get(this, {})
            args = [self.word(sp + 4 * (i + 1)) for i in range(cleaned // 4)]
            value = 0
            if name == "get_type":
                value = obj["type"]
            elif name == "what_am_i":
                value = obj["rtti"]
            elif name == "centre":
                u.mem_write(args[0], struct.pack("<iii", *obj["coords"]))
                value = args[0]
            elif name == "precheck":
                value = obj.get("precheck", 0)
            elif name == "iron_curtain":
                value = obj.get("iron_curtain", 0)
            else:
                self.events.append([name, self.name_of(this)] + args)
            self.ret(value, cleaned)
        elif pc in NATIVES:
            name, cleaned = NATIVES[pc]
            args = [self.word(sp + 4 * (i + 1)) for i in range(cleaned // 4)]
            value = 0
            if name == "map_cell":
                value = CELL
            elif name == "cell_building":
                value = self.cell_building
            elif name == "radar_event":
                cell = self.word(sp + 4)
                self.events.append(["radar_event", this, cell & 0xFFFF, cell >> 16])
                value = self.radar_accepts
            elif name == "eva":
                self.events.append(["eva", self.name_of(this)])
            elif name == "notify_under_attack":
                self.events.append(["notify_under_attack", self.name_of(this), self.name_of(args[0])])
            else:
                self.events.append([name, self.name_of(this)] + args)
            self.ret(value, cleaned)

    def vtable(self, address):
        for offset in VIRTUALS:
            self.u.mem_write(address + offset, dwords(STUBS + offset))

    def object(self, address, vtable, name, **data):
        self.u.mem_write(address, dwords(vtable))
        self.objects[address] = data
        self.names[name] = address

    def call(self, address, ecx, arg):
        u = self.u
        u.mem_write(SP, dwords(RET_MAGIC) + dwords(arg))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, ecx)
        run_checked(u, address, RET_MAGIC, count=200_000)

    def execute(self, case):
        u = self.u
        u.mem_write(SCRATCH, bytes(REGION))
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        self.events, self.objects, self.names = [], {}, {}
        for offset in VIRTUALS:
            u.mem_write(STUBS + offset, b"\xCC")
        self.radar_accepts = case.get("radar_accepts", 1)
        self.cell_building = 0
        target = case.get("target", {})
        # The local player's house.
        u.mem_write(PLAYER_PTR, dwords(LOCAL_HOUSE if case.get("player", True) else 0))
        self.names["local_house"] = LOCAL_HOUSE
        self.names["target_house"] = TARGET_HOUSE
        # The target: a Unit (1), Infantry (2) or Building (6).
        self.vtable(TARGET_VT)
        self.object(TARGET, TARGET_VT, "target", type=TARGET_TYPE, rtti=target.get("rtti", 1),
                    coords=target.get("coords", [4000, 4000, 0]),
                    precheck=target.get("precheck", 0), iron_curtain=target.get("iron_curtain", 0))
        u.mem_write(TARGET + 0x21C, dwords(LOCAL_HOUSE if target.get("local", False) else TARGET_HOUSE))
        u.mem_write(TARGET + 0x6C4, dwords(TARGET_TYPE))
        u.mem_write(TARGET + 0x9C, struct.pack("<iii", *target.get("coords", [4000, 4000, 0])))
        u.mem_write(TARGET_TYPE + 0xA0, struct.pack("<i", target.get("strength", 400)))
        u.mem_write(TARGET_TYPE + 0xD3A, bytes([target.get("warpable", 1)]))
        u.mem_write(TARGET_TYPE + 0x232, bytes([target.get("insignificant", 0)]))
        u.mem_write(TARGET_TYPE + 0xCD5, bytes([target.get("gattling", 0)]))
        u.mem_write(TARGET_TYPE + 0xE0E, bytes([target.get("harvester", 0)]))
        if target.get("spawns"):
            u.mem_write(TARGET + 0x2D0, dwords(SPAWNS))
            self.names["spawns"] = SPAWNS
        if target.get("captives"):
            u.mem_write(TARGET + 0x2BC, dwords(CAPTIVES))
            self.names["captives"] = CAPTIVES
        # The unit's radio contact 0 and the building in its cell.
        u.mem_write(TARGET + 0xE4, dwords(CONTACTS))
        factory = target.get("factory")
        if factory is not None:
            self.vtable(FACTORY_VT)
            self.object(FACTORY, FACTORY_VT, "factory", type=FACTORY_TYPE, rtti=factory.get("rtti", 6))
            u.mem_write(FACTORY + 0x520, dwords(FACTORY_TYPE))
            u.mem_write(FACTORY_TYPE + 0x16BD, bytes([factory.get("weapons_factory", 1)]))
            u.mem_write(CONTACTS + 4 * factory.get("slot", 0), dwords(FACTORY))
            self.cell_building = FACTORY if factory.get("in_cell", True) else 0
        # Attackers: 0 fires; 1.. are already on the target's chain in order.
        attackers = case.get("attackers", [{}])
        for n, attacker in enumerate(attackers):
            t = node(n)
            self.names[f"temporal{n}"] = t
            self.vtable(owner_vt(n))
            self.object(owner(n), owner_vt(n), f"owner{n}", type=TARGET_TYPE, rtti=2)
            u.mem_write(t + 0x24, dwords(owner(n)))
            u.mem_write(owner(n) + 0x274, dwords(t))
            if attacker.get("warped"):
                u.mem_write(owner(n) + 0x278, dwords(OTHER_TARGET + 0x800))
        chain = case.get("chain", [])
        for position, n in enumerate(chain):
            t = node(n)
            u.mem_write(t + 0x28, dwords(TARGET))
            u.mem_write(t + 0x40, dwords(node(chain[position - 1]) if position > 0 else 0))
            u.mem_write(t + 0x44, dwords(node(chain[position + 1]) if position + 1 < len(chain) else 0))
        if chain:
            u.mem_write(TARGET + 0x278, dwords(node(chain[0])))
            u.mem_write(TARGET + 0x270, b"\x01")
            u.mem_write(node(chain[0]) + 0x48, dwords(case.get("head_remaining", 777)))
        # The firer's previous victim, held alone.
        if case.get("previous_victim"):
            self.vtable(OTHER_VT)
            self.object(OTHER_TARGET, OTHER_VT, "previous", type=TARGET_TYPE, rtti=1)
            u.mem_write(node(0) + 0x28, dwords(OTHER_TARGET))
            u.mem_write(node(0) + 0x48, dwords(1234))
            u.mem_write(OTHER_TARGET + 0x278, dwords(node(0)))
            u.mem_write(OTHER_TARGET + 0x270, b"\x01")
        # The target itself warping someone.
        if case.get("victim_warping"):
            victim_node = node(9)
            self.names["victim_temporal"] = victim_node
            self.vtable(OTHER_VT)
            self.object(OTHER_TARGET, OTHER_VT, "victims_victim", type=TARGET_TYPE, rtti=1)
            u.mem_write(TARGET + 0x274, dwords(victim_node))
            u.mem_write(victim_node + 0x24, dwords(TARGET))
            u.mem_write(victim_node + 0x28, dwords(OTHER_TARGET))
            u.mem_write(OTHER_TARGET + 0x278, dwords(victim_node))
            u.mem_write(OTHER_TARGET + 0x270, b"\x01")
        self.call(INITIATE_WARP, node(0), 0 if case.get("null_target") else TARGET)
        nodes = []
        for n in range(len(attackers)):
            t = node(n)
            nodes.append(dict(target=self.name_of(self.word(t + 0x28)),
                              prev=self.name_of(self.word(t + 0x40)),
                              next=self.name_of(self.word(t + 0x44)),
                              warp_remaining=self.signed(t + 0x48)))
        result = dict(input=case, events=self.events, attackers=nodes,
                      target_head=self.name_of(self.word(TARGET + 0x278)),
                      target_warped=self.byte(TARGET + 0x270),
                      house_power_recheck=self.byte(TARGET_HOUSE + 0x5778),
                      house_1fc=self.byte(TARGET_HOUSE + 0x1FC))
        if case.get("previous_victim") or case.get("victim_warping"):
            result["other_head"] = self.name_of(self.word(OTHER_TARGET + 0x278))
            result["other_warped"] = self.byte(OTHER_TARGET + 0x270)
        return result


def inputs():
    unit = lambda **extra: dict(rtti=1, **extra)
    building = lambda **extra: dict(rtti=6, coords=[5000, 5000, 0], **extra)
    cases = [
        dict(name="first_attacker", target=unit()),
        dict(name="strength_zero", target=unit(strength=0)),
        dict(name="strength_negative", target=unit(strength=-1)),
        dict(name="strength_at_wrap", target=unit(strength=0x0CCCCCCC)),
        dict(name="strength_past_wrap", target=unit(strength=0x0CCCCCCD)),
        dict(name="second_attacker", attackers=[{}, {}], chain=[1], target=unit()),
        dict(name="third_attacker", attackers=[{}, {}, {}], chain=[1, 2], target=unit()),
        dict(name="harvester_local", target=unit(harvester=1, local=True)),
        dict(name="harvester_local_radar_deduped", radar_accepts=0,
             target=unit(harvester=1, local=True)),
        dict(name="harvester_other_house", target=unit(harvester=1)),
        dict(name="harvester_no_player", player=False, target=unit(harvester=1)),
        dict(name="infantry_target", target=dict(rtti=2)),
        dict(name="building_first", target=building()),
        dict(name="building_insignificant", target=building(insignificant=1)),
        dict(name="building_precheck", target=building(precheck=1)),
        dict(name="building_second_attacker", attackers=[{}, {}], chain=[1], target=building()),
        dict(name="gattling", target=unit(gattling=1)),
        dict(name="spawns_and_captives", target=unit(spawns=True, captives=True)),
        dict(name="refused_iron_curtain", target=unit(iron_curtain=1, spawns=True)),
        dict(name="refused_unwarpable", target=unit(warpable=0)),
        dict(name="refused_in_factory", target=unit(factory=dict())),
        dict(name="factory_contact_elsewhere", target=unit(factory=dict(in_cell=False))),
        dict(name="factory_contact_not_weapons", target=unit(factory=dict(weapons_factory=0))),
        dict(name="factory_contact_slot1", target=unit(factory=dict(slot=1))),
        dict(name="factory_building_target", target=building(factory=dict())),
        dict(name="attacker_warped", attackers=[dict(warped=True)], target=unit()),
        dict(name="retarget_releases", previous_victim=True, target=unit()),
        dict(name="retarget_refused_still_releases", previous_victim=True,
             target=unit(iron_curtain=1)),
        dict(name="null_target_releases", previous_victim=True, null_target=True),
        dict(name="victim_warping_lets_go", victim_warping=True, target=unit()),
    ]
    return cases


def generate():
    fixture = Fixture()
    return [fixture.execute(case) for case in inputs()]


def metadata():
    return provenance(
        scope="Original 0x0071AF20 TemporalClass::InitiateWarp with CanWarpTarget 0x0071AE50, LetGo 0x0071ABC0 and RadioClass::Contact_With_Whom 0x0065AD30",
        assumptions=[
            "TemporalClass +24 Owner, +28 Target, +40 Prev, +44 Next, +48 WarpRemaining",
            "Techno +21C Owner house, +270 warped, +274 TemporalImUsing, +278 chain head, +2BC CaptureManager, +2D0 SpawnManager, +9C Location, +E4 radio contacts; Unit +6C4 type; Building +520 type",
            "TechnoType +A0 Strength, +D3A Warpable, +232 Insignificant, +CD5 IsGattling; UnitType +E0E Harvester; BuildingType +16BD WeaponsFactory; House +5778, +1FC; [0xA83D4C] PlayerPtr",
        ],
        substitutions=[
            "Recorded at entry and returned without running: GetTechnoType, WhatAmI, the centre coordinate, the building pre-check vt+0x80, Mark, Deselect, the Iron Curtain test vt+0x160, Kill_All_Spawns, CaptureManager FreeAll, MapClass::Get_CellClass_At_Coord, Look_up_building_in_cell, CreateRadarEvent (returns the case's radar_accepts), VoxClass::PlayEVA, HouseClass::NotifyUnderAttack, UpdateGattlingStage, the building offline/online calls",
        ],
        entry_points={"initiate_warp": INITIATE_WARP, "can_warp_target": 0x71AE50,
                      "let_go": 0x71ABC0, "contact_with_whom": 0x65AD30})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
