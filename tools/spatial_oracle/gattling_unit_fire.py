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


def shp_frames_generate():
    """Original DRON frame selection for every facing byte, plus counter edges.

    The 4/56 layout is the physical DRON UnitType reader's output, independently
    retained by the whole fire/parasite history. This is the already-admitted
    eight-facing, undisguised SHP arm, not a complete Draw_It or visible capture.
    """
    from tools.spatial_oracle import building_body_rules as body
    from tools.spatial_oracle import building_construction as construction

    fixture = body.Fixture()
    u = fixture.u
    source, kind, heading = body.TYPE, body.RULES, body.RAW
    u.mem_write(source + 0x6C4, body.dwords(kind))
    u.mem_write(kind + 0xE5D, b"\x04")
    u.mem_write(kind + 0xE30, body.dwords(56))
    spans = [(0x73C62B, 0x73C725), (0x7363C9, 0x7363DE), (0x4C91E0, 0x4C94F0)]
    original = [bytes(u.mem_read(a, b - a)) for a, b in spans]
    construction.invoke(u, 0x4C91E0, source + 0x388, 3)

    def frame(facing, counter):
        u.mem_write(heading, body.dwords(facing << 8))
        construction.invoke(u, 0x4C9300, source + 0x388, heading)
        u.mem_write(source + 0x6C0, body.dwords(counter))
        u.reg_write(UC_X86_REG_ESP, body.SP)
        u.reg_write(UC_X86_REG_EBP, source)
        run_checked(u, 0x73C62B, 0x73C725,
                    required_addresses=(0x4C93D0, 0x73C64B, 0x73C65B))
        return u.reg_read(UC_X86_REG_EBX)

    first_by_facing = [frame(facing, 7) for facing in range(256)]
    countdown = [dict(facing=facing, frames=[frame(facing, n) for n in range(7, -1, -1)])
                 for facing in range(0, 256, 32)]
    steps = []
    for before in (-0x80000000, -5, -1, 0, 1, 7, 0x7FFFFFFF):
        u.mem_write(source + 0x6C0, body.dwords(before))
        u.reg_write(UC_X86_REG_ESP, body.SP)
        u.reg_write(UC_X86_REG_ESI, source)
        run_checked(u, 0x7363C9, 0x7363DE, required_addresses=(0x7363D8,))
        steps.append(dict(before=before, after=struct.unpack("<i", u.mem_read(source + 0x6C0, 4))[0]))
    assert original == [bytes(u.mem_read(a, b - a)) for a, b in spans]
    return dict(input=dict(firing_frames=4, start_firing_frame=56, counter=7,
                           facings=8, disguised=False, binary_frame=0),
                first_by_facing=first_by_facing, countdown=countdown, counter_steps=steps)


def shp_frames_metadata():
    return provenance(
        scope="DRON original Unit SHP fire-frame arm for all256 facing bytes and eight countdown "
              "steps at eight canonical headings; original Unit AI counter arithmetic edges.",
        assumptions=[
            "The existing building_body_rules VM supplies stack/scratch storage only. Physical DRON "
            "reader output is FiringFrames4, StartFiringFrame56, Facings8; the whole fire history "
            "independently executes those retail readers. No VERA frame result is an input.",
            "Original Facing constructor4C91E0 and Set4C9300 initialize each declared byte heading. "
            "The admitted undisguised/eight-facing selector starts73C62B, executes Current4C93D0 "
            "and the firing branch, and stops73C725 before subsequent drawing.",
            "Counter-step inputs are supplied signed dwords; original7363C9..7363DE performs "
            "the decrement/clamp. The INT_MIN control preserves wrapping arithmetic. Full Unit AI "
            "admission and lifecycle are covered separately, not inferred from this slice.",
        ],
        substitutions=["No instructions, native calls, frame values or counter outputs are replaced. "
                       "The slices do not execute Draw_It's admission, blitter, pixels or negative-counter "
                       "Stand/Walk branches; no full-scene equivalence claim."],
        entry_points=dict(frame=0x73C62B, frame_boundary=0x73C725,
                          counter=0x7363C9, counter_boundary=0x7363DE,
                          facing_ctor=0x4C91E0, facing_set=0x4C9300, facing_current=0x4C93D0))


def shp_history_generate():
    import hashlib
    import json
    import os
    from tools.spatial_oracle import building_death_anims as host,building_construction as bc
    from tools.spatial_oracle.building_body_rules import dwords,SP
    from tools.procedural_drawing_oracle.electric_bolt import RngTrace
    root=Path(os.environ['VERA20K_BUILDING_DEATH_ASSETS'])
    print('Reading physical DRON fire inputs',flush=True)
    inputs=host.grapple_inputs(root,drone=True,firing=True)
    f=host.grapple_fixture(inputs,root,drone=True,start_attached=False)
    u,read=f.u,f.read32
    source,victim,manager=f.grapple_source,f.grapple_victim,f.grapple_manager
    kind=f.types['DRON'];scenario=read(0xA8B230)
    signed=lambda p:struct.unpack('<i',u.mem_read(p,4))[0]
    def xyz(p):return list(struct.unpack('<3i',u.mem_read(p,12)))
    def timer(p):return list(struct.unpack('<3i',u.mem_read(p,12)))
    def member(p):return p in [read(read(0x87F778+4)+i*4) for i in range(read(0x87F778+16))]
    def state():
        return dict(counter=signed(source+0x6C0),alive=u.mem_read(source+0x90,1)[0],limbo=u.mem_read(source+0x81,1)[0],
            health=signed(source+0x6C),position=xyz(source+0x9C),logic_member=member(source),
            target_is_host=read(source+0x2B4)==victim,target_is_null=read(source+0x2B4)==0,
            host_has_eater=read(victim+0x694)==source,manager_has_host=read(manager+0x28)==victim,
            rearm_timer=timer(source+0x2EC),source_paralysis=timer(source+0x6A0),
            host_paralysis=timer(victim+0x6A0),manager_timer_2c=timer(manager+0x2C),
            manager_timer_38=timer(manager+0x38),body_facing_fields_hex=bytes(u.mem_read(source+0x388,24)).hex())
    rng=RngTrace.__new__(RngTrace);rng.u=u
    rng.streams={'main':0x886B88,'scenario':scenario+0x218,'mapgen':0xABE890}
    rng.requests,rng.pending=[],[]
    u.hook_add(UC_HOOK_CODE,rng.observe)
    roles={0:None,source:'drone',victim:'host',manager:'parasite'}
    def role(p):return roles.get(p,f'0x{p:08X}')
    phase='constructor';last_pc=None;calls=[];writes=[];returns=[];bullets=[]
    watched={0x736DF0:'UnitFireAtTarget',0x741340:'UnitFire',0x6FDD50:'TechnoFire',
        0x5F4D30:'ObjectLimbo',0x62A980:'AttachTo',0x4664C0:'BulletCtor',
        0x468670:'BulletFire',0x4666E0:'BulletAI',0x629FD0:'ParasiteAI',
        0x62A4A0:'ExitUnit',0x7360C0:'UnitAI',0x7258D0:'AnnounceExpired'}
    regions=[('firing_counter',source+0x6C0,4),('rearm_timer',source+0x2EC,12),
        ('passive_scan_timer',source+0x180,12),('source_paralysis',source+0x6A0,12),
        ('host_paralysis',victim+0x6A0,12),('manager_timer_2c',manager+0x2C,12),
        ('manager_timer_38',manager+0x38,12)]
    def observe(_u,pc,size,data):
        nonlocal last_pc
        last_pc=pc;sp=u.reg_read(UC_X86_REG_ESP)
        if returns and (pc,sp)==returns[-1][:2]:
            _,_,row=returns.pop();row['result']=role(u.reg_read(UC_X86_REG_EAX))
        if pc in watched:
            this=u.reg_read(UC_X86_REG_ECX)
            if pc==0x4664C0:
                roles[this]='bullet'
                if this not in bullets:bullets.append(this)
            row=dict(phase=phase,event=watched[pc],receiver=role(this),frame=read(bc.FRAME),
                counter=signed(source+0x6C0),rng_request_count=len(rng.requests))
            calls.append(row)
            if pc==0x741340:returns.append((read(sp),sp+12,row))
    def write(_u,access,address,size,value,data):
        for name,start,length in regions:
            if address<start+length and start<address+size:
                writes.append(dict(phase=phase,pc=f'{last_pc:08X}',field=name,offset=address-start,
                    size=size,value=value,frame=read(bc.FRAME),rng_request_count=len(rng.requests)))
    u.hook_add(UC_HOOK_CODE,observe);u.hook_add(UC_HOOK_MEM_WRITE,write)
    layout=dict(firing_frames=struct.unpack('<b',u.mem_read(kind+0xE5D,1))[0],start_firing_frame=signed(kind+0xE30),
        facings=signed(kind+0xE3C),sync=[signed(kind+0xE40),signed(kind+0xE44)],walk_frames=struct.unpack('<b',u.mem_read(kind+0xE5C,1))[0])
    kernel=json.loads(Path(host.__file__).with_name('unit_shp_fire_frames.json').read_text())['input']
    assert all(layout[k]==kernel[k] for k in ('firing_frames','start_firing_frame','facings'))
    history={'constructor':state()};rng_states={'before_target_assignment':rng.states()}
    phase='target_assignment_and_aim'
    bc.invoke(u,0x6FCDB0,source,victim)
    heading=f.allocate(4);bc.invoke(u,0x5F3DB0,source,heading,victim)
    for offset in (0x388,0x3A0):bc.invoke(u,0x4C9300,source+offset,heading)
    slot=bc.invoke(u,0x746CD0,source,victim)
    error=bc.invoke(u,0x740FD0,source,victim,slot,1)
    assert slot==0 and error==0
    phase='ordinary_per_frame_fire_visit'
    print('Whole original UnitFireAtTarget736DF0',flush=True)
    bc.invoke(u,0x736DF0,source)
    assert not returns and bullets and any(c['event']=='UnitFire' and c.get('result')=='bullet' for c in calls)
    bullet=bullets[-1]
    assert read(source+0x6C0)>0 and u.mem_read(source+0x81,1)[0]
    history['after_fire']=state();rng_states['after_fire']=rng.states()
    flight=[]
    for frame in range(1,30):
        phase=f'bullet_frame_{frame}';u.mem_write(bc.FRAME,dwords(frame));bc.invoke(u,0x4666E0,bullet)
        flight.append(dict(frame=frame,source=state(),bullet_alive=u.mem_read(bullet+0x90,1)[0]))
        if read(manager+0x28)==victim or not u.mem_read(bullet+0x90,1)[0]:break
    assert read(manager+0x28)==victim and read(victim+0x694)==source
    history['flight']=flight;rng_states['after_attachment']=rng.states()
    attached=[]
    for frame in range(frame+1,frame+4):
        phase=f'host_parasite_frame_{frame}';u.mem_write(bc.FRAME,dwords(frame));bc.invoke(u,0x629FD0,manager)
        attached.append(dict(frame=frame,source=state(),host_health=signed(victim+0x6C)))
    history['attached']=attached;rng_states['after_parasite_visits']=rng.states()
    def draw():
        u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EBP,source)
        run_checked(u,0x73C62B,0x73C725,required_addresses=(0x73C64B,))
        return u.reg_read(UC_X86_REG_EBX)
    phase='whole_exit_unit';bc.invoke(u,0x62A4A0,manager)
    assert not u.mem_read(source+0x81,1)[0] and u.mem_read(source+0x90,1)[0]
    history['release']=dict(frame=frame,source=state(),body_frame=draw())
    rng_states['after_release']=rng.states();recovery=[]
    for frame in range(frame+1,frame+11):
        phase=f'whole_unit_ai_frame_{frame}';u.mem_write(bc.FRAME,dwords(frame))
        assert member(source)
        bc.invoke(u,0x7360C0,source)
        recovery.append(dict(frame=frame,source=state(),body_frame=draw()))
    history['recovery']=recovery;rng_states['after_recovery']=rng.states()
    assert f.code_unchanged() and not rng.pending and not returns
    assets={name:dict(bytes=(root/name).stat().st_size,sha256=hashlib.sha256((root/name).read_bytes()).hexdigest())
        for name in ('ARTMD.INI','DRON.SHP','DRONP.SHP','GTNK.VXL','GTNK.HVA','VOXELS.VPL')}
    result=dict(input=dict(context=f.grapple_context,layers=inputs.grapple_layers,layout=layout,assets=assets,
        weapon_slot=slot,fire_error=error,release='direct original ExitUnit after three host ParasiteAI visits'),
        history=history,calls=calls,writes=writes,rng=dict(states=rng_states,requests=rng.requests))
    print('Native recovery counters:',[r['source']['counter'] for r in recovery],flush=True)
    print('Native recovery frames:',[r['body_frame'] for r in recovery],flush=True)
    return result

def shp_history_metadata():
    return provenance(scope='Retail DRON per-frame FireAtTarget736DF0 through actual Unit/Techno FireAt, Bullet flight/Parasite attachment, ExitUnit and ten whole UnitAI visits; retained firing counter and bounded SHP frame reads',
        assumptions=['Existing building_death_anims owns original retail readers, VM, allocator, physical actors, flat map/House projection and platform/presentation boundaries. Its opt-in fresh actor boundary runs native constructors/Unlimbo, without supplied Limbo or AttachTo.',
            'Original physical RULESMD, optional LANGRULE, MPBattleMD and Hills map layers plus fixed ART/DRONP are read; DRON/MTNK are admitted at declared (8,8)/(9,8). Target assignment and instantaneous aim use original owners. This is a bounded projected map, not full scenario startup.',
            'One real736DF0 invokes original741340 and6FDD50; no FireAt, fire legality, Bullet or parasite answer is substituted. BulletAI runs until real attachment; three real host ParasiteAI visits precede direct whole ExitUnit. Later whole UnitAI visits run only for the genuinely registered released source.',
            'Draw73C62B..73C725 reads admitted undisguised eight-facing DRON frame state; it is not DrawIt admission, blitter execution or original pixels. Other actors/particles/animations and the whole Logic schedule are not advanced in this component.',
            'RNG streams are retained from actual constructor inputs without reseeding. All subsequent raw/ranged requests and continuation states are recorded. Watched firing/rearm/passive/paralysis/parasite timer stores and Limbo/expiry calls preserve phase/order; this does not establish exhaustive whole-game timer/detach coverage.'],
        substitutions=['Only existing declared IO/allocator/presentation/cache seams of the joined runtime; exact original code remains unchanged. The direct ExitUnit invocation is the release boundary, not a native Sonic-hit producer or user input event. Native full-match/pixel equivalence is not claimed.'],
        entry_points=dict(unit_fire_visit=0x736DF0,unit_fire=0x741340,techno_fire=0x6FDD50,unit_ctor=0x7353C0,unit_unlimbo=0x737BA0,
            bullet_ai=0x4666E0,attach=0x62A980,parasite_ai=0x629FD0,exit_unit=0x62A4A0,unit_ai=0x7360C0,counter=0x7363C9,frame=0x73C62B))



def main(argv=None):
    import sys
    args = list(sys.argv[1:] if argv is None else argv)
    if "--shp-history" in args:
        args.remove("--shp-history")
        finish_vectors(shp_history_generate, Path(__file__).with_name("unit_shp_fire_history.json"),
                       provenance=shp_history_metadata, argv=args)
    elif "--shp-frames" in args:
        args.remove("--shp-frames")
        finish_vectors(shp_frames_generate, Path(__file__).with_name("unit_shp_fire_frames.json"),
                       provenance=shp_frames_metadata, argv=args)
    else:
        finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata, argv=args)


if __name__ == "__main__":
    main()
