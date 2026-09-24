"""Original UnitClass per-frame firing update 0x00736DF0 (Ghidra label UnitClass__Fire_At_Target;
called every frame by UnitClass::AI at 0x007365E1), with the REAL gattling bodies.

Each row builds a Unit with a clone of the original Unit vtable 0x007F5C70 in which only the slots
below are replaced, calls 0x00736DF0 once (thiscall, no arguments, RET) and records which gattling
call ran, from which site and with which ticks, the state afterwards and the ordered call log.

    0x0073708A  IncreaseGattlingStage(1)   IsGattling and the code is 0, 2, 3 or 4
    0x007370A9  UpdateGattlingStage(1)     IsGattling and any other code (-1 and 12 included)
    0x00737116  UpdateGattlingStage(1)     no target (+0x2B4 = 0) or GetWeapon(0)->WeaponType = 0

Substituted (each returns the row's value and is logged with its arguments):
    weapon_index       vt+0x2E4 (target) -> idx                           RET 4
    fire_error         vt+0x3C0 (target, idx, check_range) -> code        RET 0xC
    deploy_fire        vt+0x4E4 () -> bool (Unit 0x00736D50)              RET 0
    queue_mission      vt+0x1E8 (mission, commence_now)                   RET 8
    fire               vt+0x3CC (target, idx)                             RET 8
    set_target         vt+0x3C8 (target), record only                     RET 4
    start_uncloaking   vt+0x45C (arg) (label lead)                        RET 4
    damage             0x006F3970 (idx) -> int                            RET 4
    can_fire_at        0x006F77B0 (target, idx) -> bool (label lead)      RET 8
    direction          0x005F3DB0 (out, target): writes the row's DirStruct word to out  RET 8
    spawn_clear        0x006B7BB0 (ECX = +0x2D0 SpawnManager) (label lead) RET 0
    locomotor_moving   ILocomotion +0x10 through +0x674 (stdcall this)    RET 4
Runs natively: the whole body 0x00736DF0..0x00737146 including the jump table 0x00737148; GetTechnoType
vt+0x84 -> vt+0x88 0x00741490; GetWeapon vt+0x3F8 0x0070E140 and IsElite 0x00750010;
IncreaseGattlingStage 0x0070DE70, UpdateGattlingStage 0x0070E000, GetValue 0x0070DDF0; the audio
helpers, PlayAt (early exit with [0x008464AC] = 0) and RandomClass::Next on g_MainRng as in
gattling_stage.py; FacingClass::Set_Desired 0x004C9220 and Desired 0x004C9470 on the body facing
+0x388 and turret facing +0x3A0 (ROT 0); HealthRatio 0x005F5C60; the target's WhatAmI vt+0x2C and
type getter vt+0x88 (original Unit 0x007F5C70 / Infantry 0x007EB058 vtables).

`calls` is the ordered log of substituted calls plus the gattling call itself
("gattling_increase" / "gattling_update" / "gattling_update_no_target" with its ticks), so the
shot (fire) is seen to precede the charge. `arm` is the jump-table target taken (null: code 1, 3,
4, 7, 10, the vt+0x4E4 early return, or > 11 unsigned, which all skip the arms).

Row schema (sparse over `defaults`; dict groups merge per key):
    code            fire_error verdict (any int; > 11 unsigned skips the table)
    weapon_index    weapon_index verdict          deploy_fire  vt+0x4E4 verdict
    damage          0x006F3970 verdict            can_fire_at  0x006F77B0 verdict
    direction       DirStruct word from 0x005F3DB0            locomotor_moving  ILocomotion +0x10
    type            is_gattling +0xCD5, turret +0xCA1 (+0xE11 = !turret, the reader's post-pass
                    0x00747753..0x00747759); +0xE10 and the visceroid bytes +0xE18/+0xE19 stay 0
                    (+0xE10's only writer is the constructor, 0x0074711B). Stage tables, rates and
                    weapons are retail YTNK (gattling_stage.py TYPES).
    slot0           type weapon slot 0 present (false = NULL WeaponType)
    unit            value +0x144, stage +0x140, latch +0x4B8, turret_anim +0x148, veterancy +0x150
                    (float), firing_flag +0x68D, navcom +0x5A4 (bool), spawn_manager +0x2D0 (bool),
                    has_target +0x2B4 (bool)
    target          kind unit | infantry (flags +0x14 = 7); health +0x6C, strength type +0xA0
    expect          increase | update | update_no_target | none: the gattling site this row must
                    reach (checked; the site is a required address). Not an input to native code.
Fixed: Rules [0x008871E0]+0x16F8 = 1.0 (supplied); Frame [0x00A8ED84] = 1000; g_MainRng seeded by
0x0065C6D0(0x2A61); the unit's audio controllers zeroed. Writes allowed: stack, unit +0x140, +0x144,
+0x148, +0x4B8, +0x4D4, +0x68D, controller +8 words (+0x4AC, +0x4C8, value 0), the two FacingClass
blocks (+0x388..+0x39F, +0x3A0..+0x3B7), g_MainRng inside Next. Everything else fails the row.
Rust consumer: src/sim/combat/gattling_tests.rs (`original_unit_fire_update_rows`).
"""

import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
                               UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
                               UC_X86_REG_ESP, UC_X86_REG_FPCW, UC_X86_REG_FPSW, UC_X86_REG_FPTAG)

from tools.native_oracle import (NATIVE_FPCW, RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE,
                                 OracleError, finish_vectors, load_image, provenance, run_checked)

# ---- native identities ------------------------------------------------------------------------
ENTRY, BODY_END = 0x736DF0, 0x737147
UNIT_VT, INFANTRY_VT = 0x7F5C70, 0x7EB058
SITES = {0x73708A: "increase", 0x7370A9: "update", 0x737116: "update_no_target"}
ARMS = {0x736EE9: "0 ok", 0x736F78: "2 facing", 0x736E7E: "5 illegal", 0x737054: "6 cant",
        0x73704B: "8/11 range", 0x737023: "9 cloaked"}
TABLE_JUMP = 0x736E77
STOP, RELEASE, PLAY_AT, PLAY_AT_CONTINUES = 0x405D40, 0x406060, 0x7509E0, 0x7509F2
NEXT, NEXT_RETURNS, RANDOM_RANGED, SEED = 0x65C780, (0x65C787, 0x65C7D0), 0x65C7E0, 0x65C6D0
MAIN_RNG, RNG_SIZE, RNG_SEED = 0x886B88, 0x3F4, 0x2A61
AUDIO_MANAGER, SOUND_ENABLED, FRAME, RULES_PTR = 0x87E2A0, 0x8464AC, 0xA8ED84, 0x8871E0
COM_ERROR = 0x7DC720

# ---- scratch layout ---------------------------------------------------------------------------
REGION = 0x60000
SP = STACK_BASE + STACK_SIZE - 0x1000
UNIT, TARGET, DUMMY = SCRATCH, SCRATCH + 0x1000, SCRATCH + 0x2000
UNIT_TYPE, TARGET_TYPE = SCRATCH + 0x4000, SCRATCH + 0x6000
WEAPONS, REPORTS = SCRATCH + 0x8000, SCRATCH + 0xC000
RULES = SCRATCH + 0x10000
CLONE_VT = SCRATCH + 0x20000
LOCO, LOCO_VT = SCRATCH + 0x21000, SCRATCH + 0x21100
STUBS = SCRATCH + 0x22000
LOCATION = (0x1480, 0x1280, 0x68)

STUB_POPS = {"weapon_index": 4, "fire_error": 0xC, "deploy_fire": 0, "queue_mission": 8,
             "fire": 8, "set_target": 4, "start_uncloaking": 4, "locomotor_moving": 4}
STUB_AT = {name: STUBS + 0x10 * n for n, name in enumerate(STUB_POPS)}
STUB_NAME = {address: name for name, address in STUB_AT.items()}
VT_SLOTS = {0x2E4: "weapon_index", 0x3C0: "fire_error", 0x4E4: "deploy_fire", 0x1E8: "queue_mission",
            0x3CC: "fire", 0x3C8: "set_target", 0x45C: "start_uncloaking"}
NATIVE_STUBS = {0x6F3970: ("damage", 4), 0x6F77B0: ("can_fire_at", 8), 0x5F3DB0: ("direction", 8),
                0x6B7BB0: ("spawn_clear", 0)}
ARGS = {"weapon_index": 1, "fire_error": 3, "deploy_fire": 0, "queue_mission": 2, "fire": 2,
        "set_target": 1, "start_uncloaking": 1, "locomotor_moving": 0, "damage": 1,
        "can_fire_at": 2, "direction": 2, "spawn_clear": 0}
UNIT_WRITES = {0x140: 4, 0x144: 4, 0x148: 4, 0x4B8: 1, 0x4D4: 1, 0x68D: 1, 0x4AC: 4, 0x4C8: 4}
FACINGS = {"body": 0x388, "turret": 0x3A0}

DEFAULTS = {
    "code": 0, "weapon_index": 0, "deploy_fire": False, "damage": 25, "can_fire_at": False,
    "direction": 0x2000, "locomotor_moving": False,
    "type": {"is_gattling": 1, "turret": 1}, "slot0": True,
    "unit": {"value": 400, "stage": 1, "latch": 1, "turret_anim": 7, "veterancy": 0.0,
             "firing_flag": 1, "navcom": False, "spawn_manager": False, "has_target": True},
    "target": {"kind": "unit", "health": 100, "strength": 100},
}
YTNK = {"stage": [200, 400, 600, 0, 0, 0], "elite_stage": [100, 200, 300, 0, 0, 0],
        "rate_up": 1, "rate_down": 50}


def u32(value):
    return struct.pack("<I", int(value) & 0xFFFFFFFF)


def i32(raw):
    return struct.unpack("<i", u32(raw))[0]


def resolve(row):
    extra = set(row) - set(DEFAULTS) - {"name", "expect"}
    if extra:
        raise ValueError(f"{row.get('name')}: unknown inputs {sorted(extra)}")
    full = {}
    for key, default in DEFAULTS.items():
        given = row.get(key)
        if isinstance(default, dict):
            unknown = set(given or {}) - set(default)
            if unknown:
                raise ValueError(f"{row.get('name')}: unknown {key} fields {sorted(unknown)}")
            full[key] = {**default, **(given or {})}
        else:
            full[key] = default if given is None else given
    if row["expect"] not in ("increase", "update", "update_no_target", "none"):
        raise ValueError(f"{row['name']}: bad expect")
    return full


class Fixture:
    def __init__(self):
        u = self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(SCRATCH, REGION)
        u.mem_map(RET_MAGIC, 0x1000)
        if bytes(u.mem_read(AUDIO_MANAGER, 4)) != bytes(4) or u.mem_read(SOUND_ENABLED, 1)[0] != 1:
            raise OracleError("unexpected image audio globals")
        u.mem_write(SOUND_ENABLED, b"\0")
        u.hook_add(UC_HOOK_CODE, self.on_stub, begin=STUBS, end=STUBS + 0xFFF)
        for address in NATIVE_STUBS:
            u.hook_add(UC_HOOK_CODE, self.on_native, begin=address, end=address)
        for address in SITES:
            u.hook_add(UC_HOOK_CODE, self.on_site, begin=address, end=address)
        for address in ARMS:
            u.hook_add(UC_HOOK_CODE, self.on_arm, begin=address, end=address)
        u.hook_add(UC_HOOK_CODE, self.on_audio, begin=STOP, end=STOP)
        u.hook_add(UC_HOOK_CODE, self.on_audio, begin=RELEASE, end=RELEASE)
        u.hook_add(UC_HOOK_CODE, self.on_play, begin=PLAY_AT, end=PLAY_AT)
        u.hook_add(UC_HOOK_CODE, self.on_next, begin=NEXT, end=NEXT)
        for address in NEXT_RETURNS:
            u.hook_add(UC_HOOK_CODE, self.on_next_return, begin=address, end=address)
        for address in (PLAY_AT_CONTINUES, RANDOM_RANGED, COM_ERROR):
            u.hook_add(UC_HOOK_CODE, self.on_forbidden, begin=address, end=address)
        u.hook_add(UC_HOOK_MEM_WRITE, self.on_write)
        self.seeding, self.row = False, None

    def word(self, address):
        return struct.unpack("<I", self.u.mem_read(address, 4))[0]

    def fail(self, message):
        self.violations.append(message)
        self.u.emu_stop()

    def arg(self, n):
        return self.word(self.u.reg_read(UC_X86_REG_ESP) + 4 * (n + 1))

    def returns(self, value, pops):
        u = self.u
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, int(value) & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_EIP, self.word(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + pops)

    def name_of(self, value):
        return {0: None, UNIT: "unit", TARGET: "target"}.get(value, i32(value))

    def log(self, name, count, this=None):
        args = [self.name_of(self.arg(n)) if name in ("fire_error", "fire", "set_target",
                                                       "can_fire_at", "weapon_index") and n == 0
                else i32(self.arg(n)) for n in range(count)]
        if name == "direction":
            args = [self.name_of(self.arg(1))]
        entry = {"call": name, "args": args}
        if this is not None:
            entry["this"] = this
        self.calls.append(entry)

    # -- hooks
    def on_stub(self, u, address, _size, _data):
        name = STUB_NAME.get(address)
        if name is None:
            return self.fail(f"unknown stub 0x{address:08X}")
        this = u.reg_read(UC_X86_REG_ECX)
        if name == "locomotor_moving":
            if self.arg(0) != LOCO:
                return self.fail("locomotor call on another object")
            self.calls.append({"call": name, "args": []})
            return self.returns(int(self.row["locomotor_moving"]), STUB_POPS[name])
        if this != UNIT:
            return self.fail(f"{name} called on 0x{this:08X}")
        self.log(name, ARGS[name])
        value = {"weapon_index": self.row["weapon_index"], "fire_error": self.row["code"],
                 "deploy_fire": int(self.row["deploy_fire"])}.get(name, 0)
        return self.returns(value, STUB_POPS[name])

    def on_native(self, u, address, _size, _data):
        name, pops = NATIVE_STUBS[address]
        this = u.reg_read(UC_X86_REG_ECX)
        expected = DUMMY if name == "spawn_clear" else UNIT
        if this != expected:
            return self.fail(f"{name} called on 0x{this:08X}")
        self.log(name, ARGS[name])
        if name == "direction":
            u.mem_write(self.arg(0), u32(self.row["direction"] & 0xFFFF))
            return self.returns(self.arg(0), pops)
        value = {"damage": self.row["damage"], "can_fire_at": int(self.row["can_fire_at"])}.get(name, 0)
        return self.returns(value, pops)

    def on_site(self, u, address, _size, _data):
        ticks = i32(self.word(u.reg_read(UC_X86_REG_ESP)))
        self.sites.append({"site": f"0x{address:08X}", "call": SITES[address], "ticks": ticks})
        self.calls.append({"call": f"gattling_{SITES[address]}", "args": [ticks]})

    def on_arm(self, _u, address, _size, _data):
        self.arms.append(ARMS[address])

    def on_audio(self, u, address, _size, _data):
        offset = u.reg_read(UC_X86_REG_ECX) - UNIT
        name = {0x4A4: "a", 0x4C0: "b"}.get(offset)
        if name is None:
            return self.fail(f"audio helper on +0x{offset:X}")
        self.audio.append(("stop " if address == STOP else "release ") + name)

    def on_play(self, u, _address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if self.word(sp + 4) != UNIT + 0x4A4:
            return self.fail("PlayAt with another controller")
        self.plays.append({"sound": i32(u.reg_read(UC_X86_REG_ECX)),
                           "coord": list(struct.unpack("<3i", u.mem_read(u.reg_read(UC_X86_REG_EDX), 12)))})
        self.audio.append("play")

    def on_next(self, u, _address, _size, _data):
        if u.reg_read(UC_X86_REG_ECX) != MAIN_RNG:
            return self.fail("Random::Next on another generator")
        self.pending_next = True

    def on_next_return(self, u, _address, _size, _data):
        self.draws.append(u.reg_read(UC_X86_REG_EAX))
        self.pending_next = False

    def on_forbidden(self, u, address, _size, _data):
        self.fail(f"reached 0x{address:08X}")

    def on_write(self, u, _access, address, size, value, _data):
        if STACK_BASE <= address and address + size <= STACK_BASE + STACK_SIZE:
            return
        if MAIN_RNG <= address and address + size <= MAIN_RNG + RNG_SIZE and (
                self.seeding or self.pending_next):
            return
        offset = address - UNIT
        if UNIT_WRITES.get(offset) == size:
            if offset in (0x4AC, 0x4C8) and value != 0:
                return self.fail("controller +8 store not 0")
            self.writes.add(offset)
            return
        for base in FACINGS.values():
            if base <= offset and offset + size <= base + 0x18:
                self.writes.add(base)
                return
        self.fail(f"write of {size} at 0x{address:08X} from 0x{u.reg_read(UC_X86_REG_EIP):08X}")

    # -- fixture
    def build(self, row):
        u = self.u
        u.mem_write(SCRATCH, bytes(REGION))
        u.mem_write(SP - 0x3000, bytes(0x3100))
        u.mem_write(STUBS, b"\xCC" * 0x1000)
        u.mem_write(FRAME, u32(1000))
        u.mem_write(RULES_PTR, u32(RULES))
        u.mem_write(RULES + 0x16F8, struct.pack("<d", 1.0))
        # Type: retail YTNK tables; weapons slot n reports sound n+1 (elite 101+n).
        kind, unit, target = row["type"], row["unit"], row["target"]
        u.mem_write(UNIT_TYPE + 0xCA1, bytes([kind["turret"]]))
        u.mem_write(UNIT_TYPE + 0xCD5, bytes([kind["is_gattling"]]))
        u.mem_write(UNIT_TYPE + 0xE11, bytes([int(not kind["turret"])]))
        u.mem_write(UNIT_TYPE + 0xCD8, u32(3))
        u.mem_write(UNIT_TYPE + 0xCDC, b"".join(u32(v) for v in YTNK["stage"]))
        u.mem_write(UNIT_TYPE + 0xCF4, b"".join(u32(v) for v in YTNK["elite_stage"]))
        u.mem_write(UNIT_TYPE + 0xD0C, u32(YTNK["rate_up"]) + u32(YTNK["rate_down"]))
        for n in range(12):
            address, items = WEAPONS + 0x100 * n, REPORTS + 0x10 * n
            u.mem_write(items, u32(n + 1 if n < 6 else 101 + n - 6))
            u.mem_write(address + 0xC0, u32(items))
            u.mem_write(address + 0xCC, u32(1))
            if n == 0 and not row["slot0"]:
                continue
            u.mem_write(UNIT_TYPE + (0x898 + 0x1C * n if n < 6 else 0xA94 + 0x1C * (n - 6)),
                        u32(address))
        # The unit, with its cloned vtable.
        u.mem_write(CLONE_VT, bytes(u.mem_read(UNIT_VT, 0x600)))
        for offset, name in VT_SLOTS.items():
            u.mem_write(CLONE_VT + offset, u32(STUB_AT[name]))
        u.mem_write(UNIT, u32(CLONE_VT))
        u.mem_write(UNIT + 0x14, bytes([7]))
        u.mem_write(UNIT + 0x9C, b"".join(u32(v) for v in LOCATION))
        u.mem_write(UNIT + 0x6C4, u32(UNIT_TYPE))
        u.mem_write(UNIT + 0x140, u32(unit["stage"]))
        u.mem_write(UNIT + 0x144, u32(unit["value"]))
        u.mem_write(UNIT + 0x148, u32(unit["turret_anim"]))
        u.mem_write(UNIT + 0x150, struct.pack("<f", unit["veterancy"]))
        u.mem_write(UNIT + 0x4B8, bytes([unit["latch"]]))
        u.mem_write(UNIT + 0x68D, bytes([unit["firing_flag"]]))
        u.mem_write(UNIT + 0x5A4, u32(DUMMY if unit["navcom"] else 0))
        u.mem_write(UNIT + 0x2D0, u32(DUMMY if unit["spawn_manager"] else 0))
        u.mem_write(UNIT + 0x2B4, u32(TARGET if unit["has_target"] else 0))
        u.mem_write(UNIT + 0x674, u32(LOCO))
        u.mem_write(LOCO, u32(LOCO_VT))
        u.mem_write(LOCO_VT + 0x10, u32(STUB_AT["locomotor_moving"]))
        for base in FACINGS.values():                 # desired 0x4000, previous 0x4000, ROT 0
            u.mem_write(UNIT + base, u32(0x4000) * 2 + u32(-1) + u32(0) * 2 + b"\0\0")
        # The target.
        u.mem_write(TARGET, u32({"unit": UNIT_VT, "infantry": INFANTRY_VT}[target["kind"]]))
        u.mem_write(TARGET + 0x14, bytes([7]))
        u.mem_write(TARGET + 0x6C, u32(target["health"]))
        u.mem_write(TARGET + (0x6C4 if target["kind"] == "unit" else 0x6C0), u32(TARGET_TYPE))
        u.mem_write(TARGET_TYPE + 0xA0, u32(target["strength"]))
        self.seeding = True
        self.violations, self.pending_next = [], False
        u.mem_write(SP, u32(RET_MAGIC) + u32(RNG_SEED))
        u.reg_write(UC_X86_REG_ECX, MAIN_RNG)
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, SEED, RET_MAGIC, count=50_000)
        self.seeding = False

    def execute(self, sparse):
        row = self.row = resolve(sparse)
        self.build(row)
        u = self.u
        self.calls, self.sites, self.arms, self.audio, self.plays, self.draws = [], [], [], [], [], []
        self.writes, self.violations, self.pending_next = set(), [], False
        u.mem_write(SP, u32(RET_MAGIC))
        for register in (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EDX, UC_X86_REG_ESI,
                         UC_X86_REG_EDI, UC_X86_REG_EBP):
            u.reg_write(register, 0)
        u.reg_write(UC_X86_REG_ECX, UNIT)
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        u.reg_write(UC_X86_REG_FPSW, 0)
        u.reg_write(UC_X86_REG_FPTAG, 0xFFFF)
        expect = sparse["expect"]
        required = [a for a, n in SITES.items() if n == expect]
        try:
            run_checked(u, ENTRY, RET_MAGIC, count=200_000, required_addresses=required)
        except OracleError as error:
            raise OracleError(f"{sparse['name']}: {self.violations or error}") from error
        if self.violations:
            raise OracleError(f"{sparse['name']}: {self.violations}")
        if u.reg_read(UC_X86_REG_ESP) != SP + 4:
            raise OracleError(f"{sparse['name']}: unbalanced stack")
        observed = self.sites[0]["call"] if self.sites else "none"
        if len(self.sites) > 1 or observed != expect:
            raise OracleError(f"{sparse['name']}: expected {expect}, observed {self.sites}")
        state = {"value": i32(self.word(UNIT + 0x144)), "stage": i32(self.word(UNIT + 0x140)),
                 "latch": u.mem_read(UNIT + 0x4B8, 1)[0],
                 "turret_anim": i32(self.word(UNIT + 0x148)),
                 "firing_flag": u.mem_read(UNIT + 0x68D, 1)[0],
                 "facings": {name: self.word(UNIT + base) & 0xFFFF for name, base in FACINGS.items()}}
        return {"input": sparse, "gattling": self.sites[0] if self.sites else None,
                "arm": self.arms[0] if self.arms else None, **state, "calls": self.calls,
                "draws": self.draws, "plays": self.plays, "audio": ", ".join(self.audio),
                "writes": " ".join(f"0x{o:03X}" for o in sorted(self.writes))}


# ---- rows -------------------------------------------------------------------------------------
def row(name, expect, **inputs):
    return {"name": name, "expect": expect, **inputs}


CHARGING = (0, 2, 3, 4)
NON_GATTLING = {"is_gattling": 0}


def code_rows():
    out = []
    for code in [-1, *range(12), 12, 0x7FFFFFFF]:
        expect = "increase" if code in CHARGING else "update"
        out.append(row(f"gattling.code_{code}", expect, code=code))
        out.append(row(f"non_gattling.code_{code}", "none", code=code, type=NON_GATTLING))
    # Stage-weapon index reaches Fire unchanged; the shot uses the pre-update stage.
    out.append(row("gattling.ok_fires_stage_index_then_charges", "increase", code=0,
                   weapon_index=3, unit={"value": 400, "stage": 1, "latch": 1}))
    out.append(row("gattling.ok_latch_clear_draws", "increase", code=0,
                   unit={"value": 10, "stage": 0, "latch": 0}))
    out.append(row("gattling.ok_elite", "increase", code=0,
                   unit={"value": 200, "stage": 1, "latch": 1, "veterancy": 2.0}))
    out.append(row("gattling.range_decays_stage_down", "update", code=8,
                   unit={"value": 210, "stage": 1, "latch": 1}))
    out.append(row("gattling.range_value_to_zero_no_anim", "update", code=8,
                   unit={"value": 30, "stage": 0, "latch": 1}))
    return out


def target_rows():
    return [
        row("no_target.gattling_decays", "update_no_target", unit={"has_target": False}),
        row("no_target.gattling_value_to_zero", "update_no_target",
            unit={"has_target": False, "value": 50, "stage": 0}),
        row("no_target.gattling_value_zero_stays", "update_no_target",
            unit={"has_target": False, "value": 0, "stage": 0}),
        row("no_target.gattling_value_positive_after", "update_no_target",
            unit={"has_target": False, "value": 51, "stage": 0}),
        row("no_target.non_gattling", "none", unit={"has_target": False}, type=NON_GATTLING),
        row("null_slot0.gattling_decays", "update_no_target", slot0=False, code=0),
        row("null_slot0.non_gattling", "none", slot0=False, code=0, type=NON_GATTLING),
        row("null_slot0.no_target", "update_no_target", slot0=False, unit={"has_target": False}),
    ]


def deploy_rows():
    out = []
    for code in (0, 2, 3, 4, 8):
        expect = "none" if code in (0, 2) else ("increase" if code in CHARGING else "update")
        out.append(row(f"deploy_fire.code_{code}", expect, code=code, deploy_fire=True))
        out.append(row(f"deploy_fire.non_gattling.code_{code}", "none", code=code,
                       deploy_fire=True, type=NON_GATTLING))
    return out


def arm_rows():
    return [
        row("arm5.damage_positive_keeps_target", "update", code=5, damage=1),
        row("arm5.damage_zero_keeps_target", "update", code=5, damage=0),
        row("arm5.healer_vs_infantry_drops_target", "update", code=5, damage=-1,
            target={"kind": "infantry"}),
        row("arm5.healer_vs_full_unit_drops_target", "update", code=5, damage=-1),
        row("arm5.healer_vs_damaged_unit_keeps_target", "update", code=5, damage=-1,
            target={"health": 50}),
        row("arm6.spawn_manager_cleared", "update", code=6, unit={"spawn_manager": True}),
        row("arm9.can_fire_at_uncloaks", "update", code=9, can_fire_at=True),
        row("arm2.turretless_navcom", "increase", code=2, type={"turret": 0},
            unit={"navcom": True}),
        row("arm2.turretless_moving", "increase", code=2, type={"turret": 0},
            locomotor_moving=True),
        row("arm2.turretless_still_turns_body", "increase", code=2, type={"turret": 0},
            direction=0x6000),
        row("arm2.non_gattling_turret_turns", "none", code=2, type={"is_gattling": 0},
            direction=0x6000),
    ]


def rows():
    return code_rows() + target_rows() + deploy_rows() + arm_rows()


def generate():
    all_rows = rows()
    names = [r["name"] for r in all_rows]
    if len(set(names)) != len(names):
        raise OracleError("duplicate row names")
    fixture = Fixture()
    return {"defaults": DEFAULTS, "type_tables": YTNK,
            "rows": [fixture.execute(r) for r in all_rows]}


def metadata():
    return provenance(
        scope="UnitClass 0x00736DF0 (the per-frame firing update) end to end with the real "
              "IncreaseGattlingStage/UpdateGattlingStage: which gattling call runs (sites "
              "0x0073708A, 0x007370A9, 0x00737116) with its ticks, value/stage/latch, +0x148, "
              "+0x68D, facings and the ordered substituted-call log, over fire-error codes -1, "
              "0..12 and 0x7FFFFFFF, no target, a NULL slot-0 weapon, gattling and non-gattling "
              "types, vt+0x4E4 true/false and the arm 2/5/6/9 variants. Not UnitClass::AI "
              "placement, fire legality or the stubbed leaves.",
        assumptions=[
            "One emulator; per row the scratch region and stack are rewritten, general registers "
            "zeroed, FPCW 0x0E7F with an empty x87 stack; g_MainRng reseeded by 0x0065C6D0(0x2A61).",
            "Original Unit vtable 0x007F5C70 cloned; only vt+0x2E4, +0x3C0, +0x4E4, +0x1E8, +0x3CC, "
            "+0x3C8, +0x45C replaced. Type pointer +0x6C4; retail YTNK stage tables and rates; "
            "weapon slot n has Report [n+1], elite slot n [101+n].",
            "Unit: Location (0x1480, 0x1280, 0x68); flags +0x14 = 7; both FacingClass blocks "
            "desired/previous 0x4000, ROT 0; audio controllers zeroed; locomotor +0x674 a scratch "
            "COM object. Target flags +0x14 = 7 with the original Unit or Infantry vtable.",
            "Rules+0x16F8 = 1.0 and Frame 1000 are supplied; [0x008464AC] = 0 (PlayAt early exit); "
            "[0x0087E2A0] = 0 asserted.",
            "Each row's `expect` names the only gattling site it may reach; it is a required "
            "address and the observed site must match.",
            "Write hook: stack, unit +0x140/+0x144/+0x148/+0x4B8/+0x4D4/+0x68D, controller +8 "
            "stores of 0, the two facings, g_MainRng inside Next; anything else fails.",
        ],
        substitutions=[
            "Cloned-vtable stubs: weapon_index vt+0x2E4, fire_error vt+0x3C0, deploy_fire vt+0x4E4 "
            "return the row's verdicts; queue_mission vt+0x1E8, fire vt+0x3CC, set_target vt+0x3C8 "
            "and start_uncloaking vt+0x45C are recorded and return 0.",
            "Direct calls: 0x006F3970 damage and 0x006F77B0 can_fire_at return the row's values; "
            "0x005F3DB0 writes the row's direction word; 0x006B7BB0 is recorded; ILocomotion "
            "+0x10 returns locomotor_moving. All logged.",
            "[0x008464AC] = 0 (fixture global).",
        ],
        entry_points={"unit_fire_update": ENTRY, "increase_gattling_stage": 0x70DE70,
                      "update_gattling_stage": 0x70E000, "get_value": 0x70DDF0,
                      "get_weapon": 0x70E140, "set_desired": 0x4C9220, "health_ratio": 0x5F5C60,
                      "rng_seed": SEED})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
