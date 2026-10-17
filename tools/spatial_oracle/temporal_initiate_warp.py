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

``--house-notice`` keeps a separate small corpus for the first-head building
notice and ordinary LetGo. It reuses this Fixture, runs original HouseNotify,
Building GetCoords/precheck and animation-empty pause/resume, and records the
radar/audio boundaries. See temporal_house_notice.md for its physical inputs.
"""
import hashlib
import struct
import sys
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EAX,
                               UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_ESP,
                               UC_X86_REG_EIP, UC_X86_REG_FPCW)

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

    def call(self, address, ecx, arg=None):
        u = self.u
        u.mem_write(SP, dwords(RET_MAGIC) + (b"" if arg is None else dwords(arg)))
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


def house_notice_retail():
    """Existing physical lexical/native-reader owners establish selected inputs."""
    from tools.rules_oracle import bridge_child_sound as sound_owner
    from tools.rules_oracle.ini_token_readers import Reader, KEY, READ_BOOL, READ_INT
    from tools.spatial_oracle.building_body_rules import INI, TYPE, SP as READER_SP
    from tools.spatial_oracle.trigger_action_values import (
        ActionFixture, physical_eva, initialize_eva)

    root = sound_owner.ROOT
    reader = Reader()
    fields = dict(strength=0, insignificant=False, resource_gatherer=False,
                  warpable=True, undeploys_into=None)
    keys = dict(strength=("Strength", READ_INT), insignificant=("Insignificant", READ_BOOL),
                resource_gatherer=("ResourceGatherer", READ_BOOL),
                warpable=("Warpable", READ_BOOL))
    layers = []
    for name in ("RULESMD.INI", "LANGRULE.INI", "MPBattleMD.ini", "Hills.map"):
        path = root / name
        if not path.exists():
            assert name == "LANGRULE.INI", path
            layers.append(dict(file=name, absent=True))
            continue
        raw = path.read_bytes()
        sections = sound_owner.sections(raw)
        values = sections.get("YAREFN", {})
        selected = {key: values[key] for key in (*[v[0] for v in keys.values()],
                                                  "UndeploysInto") if key in values}
        for field, (key, native) in keys.items():
            fields[field] = reader.scalar_latin1(values.get(key), int(fields[field]), native)
        text, length = reader.read_string(values.get("UndeploysInto"), "", 0x80)
        if length:
            # This profile's stock SMIN reference exists in the physical catalog.
            # Actual UnitType allocation/full type construction is a stated seam.
            fields["undeploys_into"] = text
        layers.append(dict(file=name, bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(),
                           yarefn=selected,
                           base_under_attack_sound=sections.get("AudioVisual", {}).get(
                               "BaseUnderAttackSound")))

    art_raw = (root / "ARTMD.INI").read_bytes()
    art = sound_owner.sections(art_raw)
    foundation = art["YAREFN"]["Foundation"]
    reader.entry(foundation)
    reader.u.mem_write(READER_SP, dwords(RET_MAGIC, TYPE + 0x1F8, KEY, 0))
    reader.u.reg_write(UC_X86_REG_ESP, READER_SP)
    reader.u.reg_write(UC_X86_REG_ECX, INI)
    run_checked(reader.u, 0x474DA0, RET_MAGIC, count=100_000)
    fields["foundation_id"] = reader.u.reg_read(UC_X86_REG_EAX)
    fields["foundation"] = foundation
    fields["width"] = struct.unpack("<I", reader.u.mem_read(
        0x8192B8 + fields["foundation_id"] * 4, 4))[0]
    fields["height"] = struct.unpack("<I", reader.u.mem_read(
        0x819310 + fields["foundation_id"] * 4, 4))[0]

    # Reuse Sound's registry/INI/allocator transport. Its two historical entries
    # remain resident; the added siren's numeric ID is fixture-relative.
    sound = sound_owner.Sound.__new__(sound_owner.Sound)
    sound.sample_ids = {"gexp06a": 0, "gexpshaa": 1, "ubase": 2}
    sound_owner.Sound.__init__(sound)
    physical = sound.physical
    sound.make_ini({"Defaults": physical["Defaults"],
                    "SoundList": {k: v for k, v in physical["SoundList"].items()
                                  if v == "BaseUnderAttackSiren"},
                    "BaseUnderAttackSiren": physical["BaseUnderAttackSiren"]})
    sound.invoke(0x7510D0, INI)
    siren_id = sound.invoke(0x7514D0, sound.cstring("BaseUnderAttackSiren"))
    sound_ptr = sound.read32(sound.read32(0xB1D37C) + siren_id * 4)
    entry = sound.read32(sound_ptr)
    rules = sound.alloc(0x2000)
    sound.invoke(0x665650, rules)
    ctor_siren = struct.unpack("<i", sound.u.mem_read(rules + 0x184, 4))[0]
    sound_layers = []
    for layer in layers:
        if layer.get("absent"):
            continue
        raw = layer["base_under_attack_sound"]
        sound.make_ini({"AudioVisual": {} if raw is None else {"BaseUnderAttackSound": raw}})
        sound.u.mem_write(READER_SP, dwords(RET_MAGIC))
        for reg, value in ((UC_X86_REG_ESP, READER_SP), (UC_X86_REG_ESI, rules),
                           (UC_X86_REG_EDI, INI), (UC_X86_REG_EAX, 0)):
            sound.u.reg_write(reg, value)
        run_checked(sound.u, 0x669C9A, 0x669CE2, count=100_000,
                    required_addresses=(0x528A10,))
        sound_layers.append(dict(file=layer["file"], raw=raw,
                                 resolved_fixture_id=sound.read32(rules + 0x184)))
    siren = dict(name="BaseUnderAttackSiren", fixture_id=siren_id,
                 constructor_id=ctor_siren, layers=sound_layers,
                 physical=physical["BaseUnderAttackSiren"],
                 sample_names=[name for name, index in sound.sample_ids.items()
                               if index in [sound.read32(entry + 0xB4 + i * 4)
                                            for i in range(sound.read32(entry + 0x134))]],
                 priority=sound.read32(entry + 0x40),
                 volume_fixed16_raw=sound.read32(entry + 0x1C))

    eva_physical, eva_identity = physical_eva()
    eva = ActionFixture()
    initialize_eva(eva, eva_physical)
    wanted = {"EVA_OreMinerUnderAttack", "EVA_OurBaseIsUnderAttack",
              "EVA_OurAllyIsUnderAttack"}
    eva_rows = []
    for index in range(eva.integer(0xB1D4B0)):
        pointer = eva.read32(eva.read32(0xB1D4A4) + index * 4)
        name = eva.string(pointer)
        if name not in wanted:
            continue
        columns = {}
        for side, label in enumerate(("Allied", "Russian", "Yuri")):
            eva.call(0x7534E0, [], ecx=side)
            columns[label] = eva.string(eva.call(0x753380, [], ecx=pointer))
        eva_rows.append(dict(name=name, native_index=index, type=eva.integer(pointer + 0x4C),
                             priority=eva.integer(pointer + 0x48), columns=columns))
    eva.check_code()
    return dict(layers=layers, yarefn=fields, siren=siren,
                art=dict(bytes=len(art_raw), sha256=hashlib.sha256(art_raw).hexdigest(),
                         selected={"YAREFN": art["YAREFN"]}),
                sound=dict(bytes=len(sound.raw), sha256=hashlib.sha256(sound.raw).hexdigest()),
                eva=dict(**eva_identity, native_registry_count=eva.integer(0xB1D4B0),
                         selected=eva_rows))


class HouseNoticeFixture(Fixture):
    """One optional continuation of the existing Temporal VM, no second emulator."""

    def __init__(self):
        super().__init__()
        self.u.hook_add(UC_HOOK_MEM_WRITE, self.observe_timer_write)

    def observe_timer_write(self, u, _access, address, size, value, _data):
        if getattr(self, "phase", "setup") == "setup":
            return
        for name, low, high in self.timer_ranges:
            if address < high and low < address + size:
                self.timer_writes.append(dict(object=name, offset=address - low,
                    pc=f"{u.reg_read(UC_X86_REG_EIP):08X}", bytes=size,
                    value=value & ((1 << (size * 8)) - 1)))

    def observe(self, u, pc, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        this = u.reg_read(UC_X86_REG_ECX)
        if pc == 0x4F93E0:
            self.events.append(["notify_under_attack", self.name_of(this),
                                self.name_of(self.word(sp + 4))])
            return  # continue the original body instead of the inherited sink
        if pc in (0x4521C0, 0x452210):
            self.events.append(["building_offline" if pc == 0x4521C0 else "building_online",
                                self.name_of(this)])
            return  # original +660 writer; all21 animation slots are NULL
        if pc == 0x752700:
            name = bytes(u.mem_read(this, 128)).split(b"\0", 1)[0].decode("ascii")
            self.events.append(["eva", name, struct.unpack("<i", dwords(
                u.reg_read(UC_X86_REG_EDX)))[0], self.signed(sp + 4)])
            self.ret(0, 4)
            return
        if pc == 0x750920:
            self.events.append(["siren", this, u.reg_read(UC_X86_REG_EDX),
                                self.word(sp + 4), self.word(sp + 8)])
            self.ret(0, 8)
            return
        if pc in (0x65C780, 0x65C7E0, 0x65C84B):
            self.events.append(["rng", f"{pc:08X}", self.name_of(this)])
        if pc == 0x6E53A0:
            raise AssertionError("The declared empty HouseTag list reached Tag::Spring")
        super().observe(u, pc, size, data)

    def state(self):
        return dict(target_head=self.name_of(self.word(TARGET + 0x278)),
                    target_warped=self.byte(TARGET + 0x270),
                    target_online=self.byte(TARGET + 0x660),
                    attackers=[dict(target=self.name_of(self.word(node(n) + 0x28)),
                                    prev=self.name_of(self.word(node(n) + 0x40)),
                                    next=self.name_of(self.word(node(n) + 0x44)),
                                    warp_remaining=self.signed(node(n) + 0x48))
                               for n in range(len(self.notice_case.get("attackers", [{}])))],
                    houses={name: dict(power_recheck=self.byte(p + 0x5778),
                                       tech_recheck=self.byte(p + 0x1FC))
                            for name, p in (("local_house", LOCAL_HOUSE),
                                            ("target_house", TARGET_HOUSE))},
                    timers={name: list(struct.unpack("<3i", self.u.mem_read(low, high - low)))
                            for name, low, high in self.timer_ranges},
                    rng={name: bytes(self.u.mem_read(p, 0x3F4)).hex()
                         for name, p in self.rngs.items()})

    def record_phase(self, label):
        self.phases.append(dict(name=label, state=self.state(), events=self.events,
                                timer_writes=self.timer_writes))
        self.events, self.timer_writes = [], []

    def call(self, address, ecx, arg=None):
        if address != INITIATE_WARP:
            return super().call(address, ecx, arg)
        u = self.u
        case, fields = self.notice_case, self.fields
        self.phase = "setup"
        u.mem_write(0xA8B238, dwords(5))
        u.mem_write(0xA8ED84, dwords(1000))
        u.mem_write(TARGET + 0x520, dwords(TARGET_TYPE))
        u.mem_write(TARGET + 0x660, bytes([case.get("online_before", 1)]))
        u.mem_write(TARGET_TYPE + 0x408, dwords(FACTORY_TYPE if fields["undeploys_into"] else 0))
        u.mem_write(TARGET_TYPE + 0x5EC, bytes([int(fields["resource_gatherer"])]))
        u.mem_write(TARGET_TYPE + 0xEF0, dwords(fields["foundation_id"]))
        u.mem_write(TARGET_VT + 0x48, dwords(0x447AC0))
        u.mem_write(TARGET_VT + 0x80, dwords(0x457620))
        u.mem_write(LOCAL_HOUSE + 0x30, dwords(0))
        u.mem_write(TARGET_HOUSE + 0x30, dwords(1))
        house_type, rules, scenario = (SCRATCH + off for off in (0x40000, 0x41000, 0x50000))
        u.mem_write(TARGET_HOUSE + 0x34, dwords(house_type))
        u.mem_write(house_type + 0x1A6, bytes([int(case.get("passive", False))]))
        u.mem_write(TARGET_HOUSE + 0x5788, dwords(int(case.get("allied", False))))
        u.mem_write(0x8871E0, dwords(rules))
        u.mem_write(rules + 0x184, dwords(self.siren_id))
        u.mem_write(0xA8B230, dwords(scenario))
        self.rngs = dict(scenario=scenario + 0x218, main=0x886B88, mapgen=0xABE890)
        for rng in self.rngs.values():
            super().call(0x65C6D0, rng, 3)
        self.timer_ranges = [("target_dispatch", TARGET + 0xC8, TARGET + 0xD4),
                             ("firer_dispatch", owner(0) + 0xC8, owner(0) + 0xD4),
                             ("temporal_unused", node(0) + 0x2C, node(0) + 0x38)]
        for _name, low, _high in self.timer_ranges:
            u.mem_write(low, struct.pack("<3i", 990, 0, 16))
        self.phases, self.timer_writes = [], []
        self.record_phase("before")
        self.phase = "start"
        super().call(address, ecx, arg)
        self.record_phase("start")
        self.phase = "release_firer"
        super().call(0x71ABC0, node(0))
        self.record_phase("release_firer")
        if case.get("chain"):
            self.phase = "release_original_head"
            super().call(0x71ABC0, node(case["chain"][0]))
            self.record_phase("release_original_head")

    def execute_notice(self, case, stock, siren_id):
        self.notice_case = case
        self.fields = dict(stock, **case.get("type_changes", {}))
        self.siren_id = siren_id
        target = dict(rtti=6, coords=[20 * 256 + 128, 20 * 256 + 128, 0],
                      local=case.get("local", True), strength=self.fields["strength"],
                      warpable=int(self.fields["warpable"]),
                      insignificant=int(self.fields["insignificant"]))
        supplied = dict(case, target=target, target_type=self.fields,
                        game_mode=5, frame=1000, rng_seed=3,
                        timer_priors=[990, 0, 16], siren_fixture_id=siren_id,
                        target_online_before=case.get("online_before", 1), house_tags=0)
        super().execute(dict(case, target=target))
        return dict(input=supplied, phases=self.phases)


def house_notice_inputs():
    return [
        dict(name="stock_yarefn_own"),
        dict(name="stock_yarefn_deduped", radar_accepts=0),
        dict(name="ordinary_base", type_changes=dict(undeploys_into=None, resource_gatherer=False)),
        dict(name="undeployer_not_gatherer", type_changes=dict(resource_gatherer=False)),
        dict(name="gatherer_not_undeployer", type_changes=dict(undeploys_into=None)),
        dict(name="allied_yarefn", local=False, allied=True),
        dict(name="remote_yarefn", local=False),
        dict(name="passive_allied_yarefn", local=False, allied=True, passive=True),
        dict(name="second_attacker", chain=[1], attackers=[{}, {}], online_before=0),
        dict(name="insignificant_yarefn", type_changes=dict(insignificant=True)),
        dict(name="one_by_one_undeployer", type_changes=dict(foundation_id=0, foundation="1x1",
                                                             width=1, height=1)),
    ]


def generate_house_notice():
    retail = house_notice_retail()
    fixture = HouseNoticeFixture()
    return dict(schema_version=1, retail=retail,
                cases=[fixture.execute_notice(case, retail["yarefn"], retail["siren"]["fixture_id"])
                       for case in house_notice_inputs()])


def house_notice_metadata():
    return provenance(
        scope="Original first-head Temporal71AF20 -> HouseNotify4F93E0 for stock YAREFN, then original LetGo71ABC0",
        assumptions=[
            "Existing Temporal Fixture supplies target/attacker objects and declared house roles; no full object/scenario constructors or shot producer. Houses have explicit indices0/1, mode5, player pointer and empty Tag list. Other-manager/captive slots and all21 animation pointers are NULL.",
            "Selected physical RULESMD, optional LANGRULE, Battle and Hills use the existing lexical owner and original scalar/name readers. ART Foundation is unlayered and ReadFoundation474DA0 executes. Stock SMIN pointer identity is supplied, excluding UnitType registry allocation/full type reader closure.",
            "Original House branch, Building GetCoords447AC0, foundation dimensions45EC90/45ECA0, precheck457620/465D40 and animation-empty pause/resume4521C0/452210 execute. First-head and prepared second-attacker/777 priors are explicit. Timer priors990/0/16 and frame1000 are component inputs, not game cadence.",
            "Each of Scenario/Main/MapGen is originally seeded3 after fixture transport; full1012-byte states are observed uninterrupted through start/release. Temporal+2C timer has no active reader/writer and remains a documented unrepresented Rust field.",
            "Existing Sound reader retains its two Explosion entries then reads the physical siren, so siren/sample numeric IDs are fixture-relative. Original Rules constructor and selected669C9A..669CE2 sound-reference read execute across actual layers. Whole original EVA753000 reads the fixed470 entries; selected type/priority/side strings are observed through753380/7534E0, excluding playback.",
        ],
        substitutions=[
            "Inherited GetTechnoType/WhatAmI and invulnerability virtuals return declared type/RTTI/flags; Mark and Deselect are observed sinks. No GetCoords or Building precheck result is supplied in this profile.",
            "CreateRadarEvent65FA70 records type/cell and returns the declared radar_accepts boolean; actual dedup/lifetime/outline remains the existing client owner. EVA752700 and Voc750920 record literal/type/priority or resolved fixture sound ID/pan/volume/handle, returning with native RET4/RET8. No cue/device RNG or audible parity is claimed.",
            "Existing native INI/allocator/TLS transport replaces physical INI IO. Sound AddSample4015C0 maps physical gexp06a/gexpshaa/ubase to declared fixture ordinals0/1/2; no sample decode/AudioIndex/device output. Fixed EVA reader reuses its existing paused-stream shell and bounded allocator.",
        ],
        entry_points={"initiate_warp":0x71AF20, "can_warp_target":0x71AE50,
                      "let_go":0x71ABC0, "house_notify":0x4F93E0,
                      "building_coords":0x447AC0, "precheck":0x457620,
                      "read_foundation":0x474DA0, "pause":0x4521C0,
                      "resume":0x452210, "seed_rng":0x65C6D0,
                      "siren_reference":0x669C9A, "eva_read":0x753000})


if __name__ == "__main__":
    argv = sys.argv[1:]
    if "--house-notice" in argv:
        argv.remove("--house-notice")
        finish_vectors(generate_house_notice, Path(__file__).with_name("temporal_house_notice.json"),
                       provenance=house_notice_metadata, argv=argv)
    else:
        finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
