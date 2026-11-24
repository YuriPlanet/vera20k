"""Original BombClass / BombListClass (the Crazy Ivan's time bomb).

Executes, case by case:
- BombListClass::Attach 0x00438E70 (the planting gates and the bomb's stores:
  planter, planter's house, carrier, kind, start and end frame, the carrier's
  +0x38 link, the live list and its visibility countdown, the attach sound for
  the planter's local player; IsControlledByCurrentPlayer 0x0050B6F0 runs).
- BombClass::IsTimerExpired 0x00438A70 (the fuse boundary) and
  BombClass::GetClockFrame 0x00438A00 (the clock art frame).
- BombClass::Detonate 0x00438720 (the silent limbo branch, the blast's
  Apply_area_damage arguments, the explosion anim's arguments and the
  bridge-repair-hut 5x5 low/high scan) and BombClass::Defuse 0x004389B0.
- BombListClass::UpdateAll 0x00438BF0 (the spent-record purge, the ticking
  loop's start/follow/stop, the visibility countdown and the BombVisible
  refresh: HouseClass::IsHumanPlayer 0x0050B6F0, Sqrt_Approx 0x004CAC40 and
  ftol 0x007C5F00 run).
- The fuse check in TechnoClass::AI_Update 0x006FA6F5..0x006FA717 (the
  original IsTimerExpired runs), the IvanBomb and BombDisarm arms of
  BulletClass::DetonateAtCoord (0x00469343..0x00469375, 0x004699C4..0x004699FE)
  and TechnoClass::GetFireError's two bomb gates (0x006FCB8D..0x006FCBCD).

Recorded at entry and returned without running: operator new (a fresh
buffer), the AbstractClass constructor, the VocHandle init/stop/release,
VocClass::PlayAt, AnimClass::UpdateLoopingSound 0x00750D40, the planter's,
carrier's and detector's virtuals (WhatAmI +0x2C, owning house +0x3C, GetCoords
+0x48, GetTechnoType +0x84, cell +0x1B8), a spent bomb's deleting destructor
(+0x20), Apply_area_damage, MapClass cell lookup 0x005657A0 (the case's cells),
0x0048ACE0 (the anim's z adjust), SelectDamageAnimation 0x0048A4F0, the AnimClass
constructor, the bridge collapses 0x00574C20 / 0x00574000, and, in the call-site
fragments, Attach, Defuse and Detonate themselves.

Rust consumer: src/sim/bomb_tests.rs.
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EAX, UC_X86_REG_EBX,
                               UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_ESP,
                               UC_X86_REG_EIP, UC_X86_REG_FPCW)

from tools.native_oracle import (load_image, run_checked, SCRATCH, STACK_BASE, STACK_SIZE,
                                 RET_MAGIC, NATIVE_FPCW, finish_vectors, provenance)
from tools.spatial_oracle.map_queries import dwords

REGION = 0x100000
SP = STACK_BASE + STACK_SIZE - 0x1000
RULES_PTR, FRAME, GAME_MODE, PLAYER_PTR = 0x8871E0, 0xA8ED84, 0xA8B238, 0xA83D4C
BRIDGE_ISOTILE = 0xABAD1C
BOMB_LIST = 0x87F5D8
BOMB_ARRAY = 0x89C668          # g_BombClass_Array: +4 items, +8 capacity, +0x10 count
ATTACH, IS_EXPIRED, CLOCK, DETONATE, DEFUSE = 0x438E70, 0x438A70, 0x438A00, 0x438720, 0x4389B0
UPDATE_ALL = 0x438BF0
RULES = SCRATCH + 0x1000
LIST_ITEMS, ARRAY_ITEMS = SCRATCH + 0x3000, SCRATCH + 0x3400
DETECTOR_ITEMS = SCRATCH + 0x3800  # BombList +0x1C
BOMB_VT = SCRATCH + 0x3C00
HOUSES = SCRATCH + 0x4000       # house n at HOUSES + n * 0x400
TECHNOS = SCRATCH + 0x10000     # techno n at TECHNOS + n * 0x1000; its vtable at +0x800, type at +0xC00
BOMBS = SCRATCH + 0x30000       # bomb n at BOMBS + n * 0x100 (preset bombs and new ones)
ANIM_MEM = SCRATCH + 0x40000
CELLS = SCRATCH + 0x50000       # fake cells, 0x100 apart
BULLET = SCRATCH + 0x60000
WARHEAD = SCRATCH + 0x61000
IVAN_WH = SCRATCH + 0x62000
ANIM_TYPE = SCRATCH + 0x63000
STUB = SCRATCH + 0x70000        # one INT3 per stubbed virtual
NATIVES = {  # address -> (name, stack bytes cleaned)
    0x7C8E17: ("new", 0), 0x410170: ("abstract_ctor", 0), 0x405BE0: ("voc_init", 0),
    0x405FD0: ("voc_stop", 0), 0x7509E0: ("play_at", 4), 0x489280: ("area_damage", 0x10),
    0x5657A0: ("cell_at", 4), 0x48ACE0: ("z_adjust", 0xC), 0x48A4F0: ("select_anim", 8),
    0x421EA0: ("anim", 0x1C), 0x574C20: ("bridge_low", 4), 0x574000: ("bridge_high", 4),
    0x405D40: ("voc_release", 0), 0x750D40: ("update_loop", 0),
}
VIRTUALS = {0x20: ("delete", 4), 0x2C: ("what_am_i", 0), 0x3C: ("owner_house", 0),
            0x48: ("coords", 4), 0x84: ("techno_type", 0), 0x1B8: ("cell", 4)}


def house(n):
    return HOUSES + n * 0x400


def techno(n):
    return TECHNOS + n * 0x1000


def bomb(n):
    return BOMBS + n * 0x100


def cell_address(x, y):
    return CELLS + ((x & 0xFF) * 16 + (y & 0xF)) * 0x100


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

    def sword(self, address):
        return struct.unpack("<i", self.u.mem_read(address, 4))[0]

    def byte(self, address):
        return self.u.mem_read(address, 1)[0]

    def ret(self, value, cleaned):
        sp = self.u.reg_read(UC_X86_REG_ESP)
        self.u.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        self.u.reg_write(UC_X86_REG_EIP, self.word(sp))
        self.u.reg_write(UC_X86_REG_ESP, sp + 4 + cleaned)

    def arg(self, n):
        return self.word(self.u.reg_read(UC_X86_REG_ESP) + 4 * (n + 1))

    def coord(self, address):
        return list(struct.unpack("<iii", self.u.mem_read(address, 12)))

    def name_of(self, address):
        if address == 0:
            return None
        for name, candidate in self.names.items():
            if candidate == address:
                return name
        return hex(address)

    def observe(self, u, pc, _size, _data):
        this = u.reg_read(UC_X86_REG_ECX)
        if pc in self.fragment_stubs:
            name, cleaned = self.fragment_stubs[pc]
            args = [self.name_of(self.arg(i)) for i in range(cleaned // 4)]
            self.events.append([name, self.name_of(this)] + args)
            self.ret(0, cleaned)
            return
        if pc in NATIVES:
            name, cleaned = NATIVES[pc]
            self.native(name, this, cleaned)
            return
        if STUB <= pc < STUB + 0x1000:
            offset = pc - STUB
            name, cleaned = VIRTUALS[offset]
            self.virtual(name, this, cleaned)

    def native(self, name, this, cleaned):
        u = self.u
        if name == "new":
            size = self.arg(0)
            if size == 0x5C:
                address = bomb(self.next_bomb)
                self.next_bomb += 1
                self.names[f"bomb{self.next_bomb - 1}"] = address
            else:
                address = ANIM_MEM
            self.events.append(["new", size])
            self.ret(address, cleaned)
        elif name in ("abstract_ctor", "voc_init"):
            self.ret(0, cleaned)
        elif name == "voc_stop":
            self.events.append(["voc_stop", self.name_of(this - 0x3C)])
            self.ret(0, cleaned)
        elif name == "play_at":
            edx = u.reg_read(UC_X86_REG_EDX)
            handle = self.arg(0)
            self.events.append(["play_at", this, self.coord(edx),
                                self.name_of(handle - 0x3C) if handle else 0])
            self.ret(0, cleaned)
        elif name == "voc_release":
            self.events.append(["voc_release", self.name_of(this - 0x3C)])
            self.ret(0, cleaned)
        elif name == "update_loop":
            edx = u.reg_read(UC_X86_REG_EDX)
            self.events.append(["update_loop", self.coord(this), self.name_of(edx - 0x3C)])
            self.ret(0, cleaned)
        elif name == "area_damage":
            edx = u.reg_read(UC_X86_REG_EDX)
            self.events.append(["area_damage", self.coord(this), struct.unpack("<i", struct.pack("<I", edx))[0],
                                self.name_of(self.arg(0)), self.name_of(self.arg(1)),
                                self.arg(2), self.arg(3)])
            self.ret(0, cleaned)
        elif name == "cell_at":
            x, y = struct.unpack("<hh", u.mem_read(self.arg(0), 4))
            self.events.append(["cell_at", x, y])
            self.ret(cell_address(x, y), cleaned)
        elif name == "z_adjust":
            sp = u.reg_read(UC_X86_REG_ESP)
            self.events.append(["z_adjust", list(struct.unpack("<iii", u.mem_read(sp + 4, 12)))])
            self.ret(self.case.get("z_adjust", 0), cleaned)
        elif name == "select_anim":
            edx = u.reg_read(UC_X86_REG_EDX)
            self.events.append(["select_anim", struct.unpack("<i", struct.pack("<I", this))[0],
                                self.name_of(edx), self.arg(0), self.coord(self.arg(1))])
            self.ret(ANIM_TYPE, cleaned)
        elif name == "anim":
            args = [self.arg(i) for i in range(7)]
            self.events.append(["anim", self.name_of(this), self.name_of(args[0]), self.coord(args[1])]
                               + [struct.unpack("<i", struct.pack("<I", a))[0] for a in args[2:]])
            self.ret(this, cleaned)
        elif name in ("bridge_low", "bridge_high"):
            x, y = struct.unpack("<hh", u.mem_read(self.arg(0), 4))
            self.events.append([name, x, y])
            self.ret(0, cleaned)

    def virtual(self, name, this, cleaned):
        obj = self.objects.get(this, {})
        if name == "what_am_i":
            self.ret(obj.get("rtti", 0x0F), cleaned)
        elif name == "owner_house":
            self.ret(house(obj.get("house", 0)), cleaned)
        elif name == "cell":
            out = self.arg(0)
            x, y = obj.get("cell", (10, 10))
            self.u.mem_write(out, struct.pack("<hh", x, y))
            self.ret(out, cleaned)
        elif name == "delete":
            self.events.append(["delete", self.name_of(this)])
            self.ret(this, cleaned)
        elif name == "coords":
            out = self.arg(0)
            self.u.mem_write(out, struct.pack("<iii", *obj.get("location", (2560, 2560, 0))))
            self.ret(out, cleaned)
        elif name == "techno_type":
            self.ret(this + 0xC00, cleaned)

    def reset(self, case):
        u = self.u
        u.mem_write(SCRATCH, bytes(REGION))
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        self.case = case
        self.events, self.names, self.objects = [], {}, {}
        self.fragment_stubs = {}
        self.next_bomb = 8
        for offset in VIRTUALS:
            u.mem_write(STUB + offset, b"\xCC")
        rules = case.get("rules", {})
        u.mem_write(RULES_PTR, dwords(RULES))
        u.mem_write(RULES + 0x20C, dwords(rules.get("ticking_sound", 7)))
        u.mem_write(RULES + 0x210, dwords(rules.get("attach_sound", 9)))
        u.mem_write(RULES + 0xFC8, dwords(IVAN_WH))
        u.mem_write(RULES + 0xFCC, dwords(rules.get("ivan_damage", 450)))
        u.mem_write(RULES + 0xFD0, dwords(rules.get("delay", 450)))
        u.mem_write(RULES + 0xFD8, dwords(rules.get("flicker", 8)))
        self.names["ivan_wh"] = IVAN_WH
        self.names["anim_type"] = ANIM_TYPE
        u.mem_write(FRAME, dwords(case.get("frame", 1000)))
        u.mem_write(GAME_MODE, dwords(case.get("game_mode", 1)))
        u.mem_write(PLAYER_PTR, dwords(house(case.get("local_house", 0))))
        u.mem_write(BRIDGE_ISOTILE, dwords(case.get("bridge_isotile", 500)))
        for n in range(4):
            self.names[f"house{n}"] = house(n)
        # The live list and the all-bombs array with room to append.
        u.mem_write(BOMB_LIST + 0x04, dwords(LIST_ITEMS))
        u.mem_write(BOMB_LIST + 0x08, dwords(64))
        u.mem_write(BOMB_LIST + 0x10, dwords(case.get("list_count", 0)))
        u.mem_write(BOMB_LIST + 0x30, dwords(case.get("countdown", 45)))
        for n in case.get("human_houses", []):
            u.mem_write(house(n) + 0x1EC, bytes([1]))
        for n in case.get("player_control", []):
            u.mem_write(house(n) + 0x1ED, bytes([1]))
        listed = case.get("list")
        if listed is not None:
            u.mem_write(BOMB_LIST + 0x10, dwords(len(listed)))
            for i, n in enumerate(listed):
                u.mem_write(LIST_ITEMS + 4 * i, dwords(0 if n is None else bomb(n)))
        detectors = case.get("detectors", [])
        u.mem_write(BOMB_LIST + 0x1C, dwords(DETECTOR_ITEMS))
        u.mem_write(BOMB_LIST + 0x20, dwords(64))
        u.mem_write(BOMB_LIST + 0x28, dwords(len(detectors)))
        for i, n in enumerate(detectors):
            u.mem_write(DETECTOR_ITEMS + 4 * i, dwords(techno(n)))
        u.mem_write(BOMB_VT + 0x20, dwords(STUB + 0x20))
        u.mem_write(BOMB_ARRAY + 0x04, dwords(ARRAY_ITEMS))
        u.mem_write(BOMB_ARRAY + 0x08, dwords(64))
        u.mem_write(BOMB_ARRAY + 0x10, dwords(0))
        for n, obj in enumerate(case.get("technos", [])):
            t = techno(n)
            self.names[f"techno{n}"] = t
            self.objects[t] = obj
            vt = t + 0x800
            for offset in VIRTUALS:
                u.mem_write(vt + offset, dwords(STUB + offset))
            u.mem_write(t, dwords(vt))
            u.mem_write(t + 0x14, bytes([obj.get("abstract_flags", 3)]))
            u.mem_write(t + 0x21C, dwords(house(obj.get("house", 0))))
            u.mem_write(t + 0x81, bytes([1 if obj.get("in_limbo") else 0]))
            u.mem_write(t + 0x9C, struct.pack("<iii", *obj.get("location", (2560, 2560, 0))))
            u.mem_write(t + 0x520, dwords(t + 0xC00))
            u.mem_write(t + 0xC00 + 0x16B6, bytes([1 if obj.get("bridge_hut") else 0]))
            u.mem_write(t + 0xC00 + 0x5F8, dwords(obj.get("bomb_sight", 0)))
            u.mem_write(t + 0x68, bytes([obj.get("visible", 0)]))
            if "bomb" in obj:
                u.mem_write(t + 0x38, dwords(bomb(obj["bomb"])))
        for n, fields in enumerate(case.get("bombs", [])):
            b = bomb(n)
            self.names[f"bomb{n}"] = b
            planter = fields.get("planter")
            carrier = fields.get("carrier")
            u.mem_write(b + 0x24, dwords(0 if planter is None else techno(planter)))
            u.mem_write(b + 0x28, dwords(house(fields.get("house", 0))))
            u.mem_write(b + 0x2C, dwords(0 if carrier is None else techno(carrier)))
            u.mem_write(b + 0x30, dwords(fields.get("kind", 0)))
            u.mem_write(b + 0x34, dwords(fields.get("start", 0)))
            u.mem_write(b + 0x38, dwords(fields.get("end", 0)))
            u.mem_write(b, dwords(BOMB_VT))
            u.mem_write(b + 0x50, dwords(fields.get("ticking_sound", 7)))
            u.mem_write(b + 0x54, dwords(fields.get("sound_started", 1)))
            u.mem_write(b + 0x58, bytes([fields.get("spent", 0)]))
        for x, y, cell in case.get("cells", []):
            address = cell_address(x, y)
            u.mem_write(address + 0xEC, dwords(cell.get("land", 0)))
            u.mem_write(address + 0x38, dwords(cell.get("isotile", 0)))
            u.mem_write(address + 0x44, dwords(cell.get("overlay", 0xFFFFFFFF)))

    def call(self, entry, *, ecx, args=(), end=RET_MAGIC):
        u = self.u
        sp = SP
        for value in reversed(args):
            sp -= 4
            u.mem_write(sp, dwords(value))
        sp -= 4
        u.mem_write(sp, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, sp)
        u.reg_write(UC_X86_REG_ECX, ecx)
        run_checked(u, entry, end, count=2_000_000)
        return u.reg_read(UC_X86_REG_EAX)

    def fragment(self, begin, end, registers):
        u = self.u
        u.reg_write(UC_X86_REG_ESP, SP)
        for register, value in registers.items():
            u.reg_write(register, value)
        return run_checked(u, begin, end, count=200_000)

    def bomb_state(self, address):
        return dict(planter=self.name_of(self.word(address + 0x24)),
                    house=self.name_of(self.word(address + 0x28)),
                    carrier=self.name_of(self.word(address + 0x2C)),
                    kind=self.word(address + 0x30), start=self.sword(address + 0x34),
                    end=self.sword(address + 0x38), ticking_sound=self.sword(address + 0x50),
                    sound_started=self.word(address + 0x54), spent=self.byte(address + 0x58))

    def techno_state(self, n):
        t = techno(n)
        return dict(bomb=self.name_of(self.word(t + 0x38)), visible=self.byte(t + 0x68),
                    redraw=self.byte(t + 0x80))

    def execute(self, case):
        self.reset(case)
        section = case["section"]
        out = dict(input=case)
        if section == "attach":
            planter = case.get("planter")
            target = case.get("target")
            self.call(ATTACH, ecx=BOMB_LIST,
                      args=[0 if planter is None else techno(planter), 0 if target is None else techno(target)])
            new_bombs = [n for n in range(8, self.next_bomb)]
            out["bombs"] = [self.bomb_state(bomb(n)) for n in new_bombs]
            out["list"] = dict(count=self.word(BOMB_LIST + 0x10), countdown=self.sword(BOMB_LIST + 0x30),
                               items=[self.name_of(self.word(LIST_ITEMS + 4 * i))
                                      for i in range(self.word(BOMB_LIST + 0x10))])
            out["array_count"] = self.word(BOMB_ARRAY + 0x10)
        elif section == "update_all":
            self.call(UPDATE_ALL, ecx=BOMB_LIST)
            out["list"] = dict(count=self.word(BOMB_LIST + 0x10), countdown=self.sword(BOMB_LIST + 0x30),
                               items=[self.name_of(self.word(LIST_ITEMS + 4 * i))
                                      for i in range(self.word(BOMB_LIST + 0x10))])
            out["bombs"] = [self.bomb_state(bomb(n)) for n in range(len(case.get("bombs", [])))]
        elif section == "timer":
            out["expired"] = self.call(IS_EXPIRED, ecx=bomb(0)) & 0xFF
        elif section == "clock":
            out["frame_index"] = self.call(CLOCK, ecx=bomb(0))
        elif section in ("detonate", "defuse"):
            self.call(DETONATE if section == "detonate" else DEFUSE, ecx=bomb(0))
            out["bomb"] = self.bomb_state(bomb(0))
        elif section == "expiry":
            self.fragment_stubs = {DETONATE: ("detonate", 0)}
            self.fragment(0x6FA6F5, 0x6FA717, {UC_X86_REG_ESI: techno(0), UC_X86_REG_EBP: 0})
        elif section in ("ivan_arm", "disarm_arm"):
            self.fragment_stubs = {ATTACH: ("attach", 8), DEFUSE: ("defuse", 0)}
            u = self.u
            u.mem_write(WARHEAD + 0x157, bytes([1 if case.get("ivan_bomb") else 0]))
            u.mem_write(WARHEAD + 0x16E, bytes([1 if case.get("bomb_disarm") else 0]))
            u.mem_write(BULLET + 0x128, dwords(WARHEAD))
            firer, target = case.get("firer"), case.get("target")
            u.mem_write(BULLET + 0xB0, dwords(0 if firer is None else techno(firer)))
            u.mem_write(BULLET + 0x10C, dwords(0 if target is None else techno(target)))
            if section == "ivan_arm":
                pc = self.fragment(0x469343, (0x469375, 0x46937A),
                                   {UC_X86_REG_ESI: BULLET, UC_X86_REG_EAX: WARHEAD})
            else:
                pc = self.fragment(0x4699C4, (0x469AA4, 0x469A03), {UC_X86_REG_ESI: BULLET})
            out["next_arm"] = pc in (0x46937A, 0x469A03)
        elif section == "fire_error":
            u = self.u
            u.mem_write(WARHEAD + 0x157, bytes([1 if case.get("ivan_bomb") else 0]))
            u.mem_write(WARHEAD + 0x16E, bytes([1 if case.get("bomb_disarm") else 0]))
            # The gates' early returns pop four registers and 0x10 of locals, then RET 0xC.
            frame = [0] * 4 + [0] * 4 + [RET_MAGIC] + [0] * 3
            sp = SP - 4 * len(frame)
            u.mem_write(sp, b"".join(dwords(v) for v in frame))
            u.reg_write(UC_X86_REG_ESP, sp)
            u.reg_write(UC_X86_REG_EDI, WARHEAD)
            u.reg_write(UC_X86_REG_EBP, techno(0))
            pc = run_checked(u, 0x6FCB8D, (RET_MAGIC, 0x6FCBCD), count=10_000)
            out["result"] = 5 if pc == RET_MAGIC and u.reg_read(UC_X86_REG_EAX) == 5 else "continue"
        out["events"] = self.events
        out["technos"] = [self.techno_state(n) for n in range(len(case.get("technos", [])))]
        if section not in ("attach", "update_all") and case.get("bombs"):
            out.setdefault("bomb", self.bomb_state(bomb(0)))
        return out


def inputs():
    ivan = dict(rtti=0x0F, house=0, location=(2560, 2688, 0))
    tank = dict(rtti=0x01, house=1, location=(3000, 3100, 0))
    cases = []
    # Attach: gates and stores.
    for name, extra in [
        ("attach_planted", {}),
        ("attach_no_planter", dict(planter=None)),
        ("attach_vehicle_planter", dict(technos=[dict(ivan, rtti=0x01), tank])),
        ("attach_no_target", dict(target=None)),
        ("attach_already_bombed", dict(technos=[ivan, dict(tank, bomb=0)],
                                       bombs=[dict(planter=0, carrier=1, start=900, end=1350)])),
        ("attach_frame_wraps", dict(frame=0x7FFFFF00)),
        ("attach_authored_delay", dict(rules=dict(delay=37))),
        ("attach_other_player_hears_nothing", dict(local_house=2)),
        ("attach_no_attach_sound", dict(rules=dict(attach_sound=-1))),
        ("attach_self", dict(target=0)),
        ("attach_building_target", dict(technos=[ivan, dict(tank, rtti=0x06)])),
        ("attach_planter_of_another_house", dict(technos=[dict(ivan, house=3), tank])),
        ("attach_appends_to_list", dict(list_count=2)),
    ]:
        case = dict(section="attach", name=name, planter=0, target=1, technos=[ivan, tank])
        case.update(extra)
        cases.append(case)
    # IsTimerExpired: the fuse boundary.
    for name, frame, fields in [
        ("timer_before_end", 1449, {}), ("timer_at_end", 1450, {}), ("timer_after_end", 1451, {}),
        ("timer_death_bomb", 1451, dict(kind=1)), ("timer_spent", 1451, dict(spent=1)),
        ("timer_signed_compare", -5, dict(end=0x7FFFFFF0)),
        ("timer_end_wrapped_negative", 0x7FFFFFF0, dict(end=-0x7FFFFF00)),
    ]:
        bomb_fields = dict(planter=0, carrier=1, start=1000, end=1450)
        bomb_fields.update(fields)
        cases.append(dict(section="timer", name=name, frame=frame, technos=[ivan, tank],
                          bombs=[bomb_fields]))
    # GetClockFrame over a fuse, both flicker phases, and the edges.
    for frame in [1000, 1007, 1008, 1015, 1016, 1074, 1075, 1150, 1374, 1375, 1449, 1450, 1451, 2000, 999]:
        cases.append(dict(section="clock", name=f"clock_{frame}", frame=frame, technos=[ivan, tank],
                          bombs=[dict(planter=0, carrier=1, start=1000, end=1450)]))
    cases.append(dict(section="clock", name="clock_death_bomb", frame=1100, technos=[ivan, tank],
                      bombs=[dict(planter=0, carrier=1, start=1000, end=1450, kind=1)]))
    cases.append(dict(section="clock", name="clock_short_delay", frame=1013, rules=dict(delay=7, flicker=1),
                      technos=[ivan, tank], bombs=[dict(planter=0, carrier=1, start=1000, end=1007)]))
    # Detonate.
    armed = dict(planter=0, carrier=1, start=1000, end=1450)
    for name, extra in [
        ("detonate_blast", {}),
        ("detonate_planter_gone", dict(bombs=[dict(armed, planter=None)])),
        ("detonate_in_limbo_is_silent", dict(technos=[ivan, dict(tank, in_limbo=True)])),
        ("detonate_spent", dict(bombs=[dict(armed, spent=1)])),
        ("detonate_no_carrier", dict(bombs=[dict(armed, carrier=None)])),
        ("detonate_authored_damage", dict(rules=dict(ivan_damage=77), z_adjust=-15)),
        ("detonate_building_not_hut", dict(technos=[ivan, dict(tank, rtti=0x06, cell=(11, 12))])),
        ("detonate_hut_high_bridge", dict(technos=[ivan, dict(tank, rtti=0x06, bridge_hut=True, cell=(11, 12))])),
        ("detonate_hut_low_bridge_isotile",
         dict(technos=[ivan, dict(tank, rtti=0x06, bridge_hut=True, cell=(11, 12))],
              cells=[[12, 13, dict(isotile=507)]])),
        ("detonate_hut_low_bridge_overlay",
         dict(technos=[ivan, dict(tank, rtti=0x06, bridge_hut=True, cell=(11, 12))],
              cells=[[9, 10, dict(overlay=0x4A)]])),
        ("detonate_hut_isotile_out_of_range",
         dict(technos=[ivan, dict(tank, rtti=0x06, bridge_hut=True, cell=(11, 12))],
              cells=[[10, 11, dict(isotile=516)], [13, 14, dict(overlay=0x66)]])),
        ("detonate_land_type", dict(cells=[[11, 12, dict(land=6)]])),
    ]:
        case = dict(section="detonate", name=name, technos=[ivan, dict(tank, bomb=0)], bombs=[armed])
        case.update(extra)
        if "technos" in extra:
            case["technos"][1] = dict(case["technos"][1], bomb=0)
        cases.append(case)
    # Defuse.
    cases.append(dict(section="defuse", name="defuse_armed", technos=[ivan, dict(tank, bomb=0)], bombs=[armed]))
    cases.append(dict(section="defuse", name="defuse_no_carrier", technos=[ivan, tank],
                      bombs=[dict(armed, carrier=None)]))
    # The AI_Update fuse check.
    for name, frame, carrier, bomb_fields in [
        ("expiry_goes_off", 1451, dict(tank, bomb=0), armed),
        ("expiry_not_yet", 1450, dict(tank, bomb=0), armed),
        ("expiry_in_limbo_waits", 1451, dict(tank, bomb=0, in_limbo=True), armed),
        ("expiry_no_bomb", 1451, tank, armed),
    ]:
        cases.append(dict(section="expiry", name=name, frame=frame, technos=[carrier, ivan],
                          bombs=[dict(bomb_fields, carrier=0, planter=1)]))
    # DetonateAtCoord's IvanBomb and BombDisarm arms.
    for name, extra in [
        ("ivan_arm_attaches", {}),
        ("ivan_arm_object_target_not_techno", dict(technos=[ivan, dict(tank, abstract_flags=2)])),
        ("ivan_arm_no_target", dict(target=None)),
        ("ivan_arm_no_firer", dict(firer=None)),
        ("ivan_arm_other_warhead", dict(ivan_bomb=False)),
    ]:
        case = dict(section="ivan_arm", name=name, ivan_bomb=True, firer=0, target=1, technos=[ivan, tank])
        case.update(extra)
        cases.append(case)
    for name, extra in [
        ("disarm_arm_defuses", {}),
        ("disarm_arm_unbombed", dict(technos=[ivan, tank])),
        ("disarm_arm_not_object", dict(technos=[ivan, dict(tank, bomb=0, abstract_flags=1)])),
        ("disarm_arm_no_target", dict(target=None)),
        ("disarm_arm_other_warhead", dict(bomb_disarm=False)),
    ]:
        case = dict(section="disarm_arm", name=name, bomb_disarm=True, firer=0, target=1,
                    technos=[ivan, dict(tank, bomb=0)], bombs=[armed])
        case.update(extra)
        cases.append(case)
    # BombListClass::UpdateAll. techno0 plants for house0, techno1 carries.
    carrier = dict(tank, bomb=0)
    live = dict(planter=0, carrier=1, house=0, start=1000, end=1450)

    def detector(dx, dy=0, dz=0, **kw):
        return dict(dict(rtti=0x0F, house=2, bomb_sight=4, location=(3000 + dx, 3100 + dy, dz)), **kw)

    for name, extra in [
        ("update_countdown_ticks", dict(countdown=5, technos=[ivan, dict(carrier, visible=1)])),
        ("update_countdown_one_waits", dict(countdown=1)),
        ("update_countdown_negative_refreshes", dict(countdown=-3)),
        ("update_refresh_planter_player", dict(countdown=0)),
        ("update_refresh_other_player_hides", dict(countdown=0, local_house=2,
                                                   technos=[ivan, dict(carrier, visible=1)])),
        ("update_detector_in_range", dict(countdown=0, local_house=2,
                                          technos=[ivan, carrier, detector(1023)], detectors=[2])),
        ("update_detector_at_range", dict(countdown=0, local_house=2,
                                          technos=[ivan, carrier, detector(1024)], detectors=[2])),
        ("update_detector_diagonal_below_range", dict(countdown=0, local_house=2,
                                                      technos=[ivan, carrier, detector(724, 723)], detectors=[2])),
        ("update_detector_diagonal_past_range", dict(countdown=0, local_house=2,
                                                     technos=[ivan, carrier, detector(724, 725)], detectors=[2])),
        ("update_detector_height_out", dict(countdown=0, local_house=2,
                                            technos=[ivan, carrier, detector(1000, 0, 300)], detectors=[2])),
        ("update_detector_height_in", dict(countdown=0, local_house=2,
                                           technos=[ivan, carrier, detector(1000, 0, 100)], detectors=[2])),
        ("update_detector_sight_three_edge", dict(countdown=0, local_house=2,
                                                  technos=[ivan, carrier, detector(767, 40, bomb_sight=3)],
                                                  detectors=[2])),
        ("update_detector_sight_three_inside", dict(countdown=0, local_house=2,
                                                    technos=[ivan, carrier, detector(767, 39, bomb_sight=3)],
                                                    detectors=[2])),
        ("update_detector_sight_zero", dict(countdown=0, local_house=2,
                                            technos=[ivan, carrier, detector(0, bomb_sight=0)], detectors=[2])),
        ("update_detector_of_another_player", dict(countdown=0, local_house=2,
                                                   technos=[ivan, carrier, detector(100, house=3)],
                                                   detectors=[2])),
        ("update_detector_not_listed", dict(countdown=0, local_house=2,
                                            technos=[ivan, carrier, detector(100)])),
        ("update_detector_second_in_range", dict(countdown=0, local_house=2,
                                                 technos=[ivan, carrier, detector(2000), detector(10)],
                                                 detectors=[2, 3])),
        ("update_campaign_player_control", dict(countdown=0, game_mode=0, local_house=2, player_control=[0])),
        ("update_campaign_human_planter", dict(countdown=0, game_mode=0, local_house=2, human_houses=[0])),
        ("update_campaign_not_controlled", dict(countdown=0, game_mode=0, local_house=0)),
        ("update_sound_starts", dict(countdown=5, bombs=[dict(live, sound_started=0)])),
        ("update_sound_follows", dict(countdown=5)),
        ("update_sound_stops_in_limbo", dict(countdown=5, technos=[ivan, dict(carrier, in_limbo=True)])),
        ("update_no_ticking_sound", dict(countdown=5, bombs=[dict(live, ticking_sound=-1, sound_started=0)])),
        ("update_purges_spent", dict(countdown=5, technos=[ivan, tank], bombs=[dict(live, carrier=None)])),
        ("update_purges_null_entry", dict(countdown=5, list=[None, 0])),
        ("update_two_bombs", dict(countdown=0, local_house=2,
                                  technos=[ivan, carrier, dict(tank, bomb=1, location=(6000, 3100, 0)),
                                           detector(500)],
                                  bombs=[live, dict(live, carrier=2)], detectors=[3])),
    ]:
        case = dict(section="update_all", name=name, technos=[ivan, carrier], bombs=[dict(live, sound_started=1)])
        case.update(extra)
        case.setdefault("list", list(range(len(case["bombs"]))))
        cases.append(case)
    # GetFireError's bomb gates (target = techno0).
    for name, extra in [
        ("fire_error_disarm_unbombed", dict(bomb_disarm=True)),
        ("fire_error_disarm_bombed", dict(bomb_disarm=True, bombed=True)),
        ("fire_error_ivan_bombed", dict(ivan_bomb=True, bombed=True)),
        ("fire_error_ivan_unbombed", dict(ivan_bomb=True)),
        ("fire_error_plain_bombed", dict(bombed=True)),
    ]:
        target = dict(tank, bomb=0) if extra.get("bombed") else tank
        case = dict(section="fire_error", name=name, technos=[target], bombs=[dict(armed, carrier=0)])
        case.update(extra)
        cases.append(case)
    return cases


def generate():
    fixture = Fixture()
    return [fixture.execute(case) for case in inputs()]


def metadata():
    return provenance(
        scope="Original BombListClass::Attach 0x00438E70, BombClass::IsTimerExpired 0x00438A70, GetClockFrame 0x00438A00, Detonate 0x00438720, Defuse 0x004389B0, BombListClass::UpdateAll 0x00438BF0; the fuse check in TechnoClass::AI_Update 0x006FA6F5..0x006FA717, DetonateAtCoord's IvanBomb (0x00469343..0x00469375) and BombDisarm (0x004699C4..0x004699FE) arms, GetFireError's bomb gates 0x006FCB8D..0x006FCBCD",
        assumptions=[
            "Rules [0x8871E0] +20C BombTickingSound, +210 BombAttachSound, +FC8 IvanWarhead, +FCC IvanDamage, +FD0 IvanTimedDelay, +FD8 IvanIconFlickerRate; frame [0xA8ED84]; GameMode [0xA8B238]; PlayerPtr [0xA83D4C]; bridge isotile base [0xABAD1C]",
            "BombList 0x87F5D8 (+4 items, +8 capacity, +0x10 count, +0x1C detector items, +0x28 detector count, +0x30 visibility countdown); g_BombClass_Array 0x89C668; House +1EC IsHuman, +1ED PlayerControl",
            "Bomb +0 vtable, +24 planter, +28 house, +2C carrier, +30 kind, +34 start, +38 end, +3C sound handle, +50 ticking sound, +54 sound started, +58 spent; Techno +14 abstract flags, +38 bomb, +68 visible, +80 redraw, +81 InLimbo, +9C Location, +21C Owner, +520 type (+5F8 BombSight, +16B6 BridgeRepairHut); cell +38 isotile, +44 overlay, +EC land",
            "Warhead +157 IvanBomb, +16E BombDisarm; bullet +B0 owner, +10C target, +128 warhead",
        ],
        substitutions=[
            "Recorded at entry and returned without running: operator new 0x007C8E17 (fresh buffers), AbstractClass ctor 0x00410170, VocHandle init 0x00405BE0 / stop 0x00405FD0 / release 0x00405D40, VocClass::PlayAt 0x007509E0, AnimClass::UpdateLoopingSound 0x00750D40, a spent bomb's deleting destructor (+20), WhatAmI (+2C), owning house (+3C), GetCoords (+48, the case's location), GetTechnoType (+84) and cell (+1B8) virtuals, Apply_area_damage 0x00489280, MapClass cell lookup 0x005657A0 (fake cells), 0x0048ACE0 (the case's z adjust), SelectDamageAnimation 0x0048A4F0, the AnimClass ctor 0x00421EA0, the bridge collapses 0x00574C20 / 0x00574000; in the call-site fragments, Attach, Defuse and Detonate",
        ],
        entry_points={"attach": ATTACH, "is_timer_expired": IS_EXPIRED, "get_clock_frame": CLOCK,
                      "detonate": DETONATE, "defuse": DEFUSE, "update_all": UPDATE_ALL,
                      "expiry_fragment": 0x6FA6F5,
                      "ivan_arm": 0x469343, "disarm_arm": 0x4699C4, "fire_error_gates": 0x6FCB8D})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
