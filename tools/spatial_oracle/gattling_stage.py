"""Original gattling stage bodies and their accessors, as retained per-object histories.

    IncreaseGattlingStage  TechnoClass 0x0070DE70  thiscall (ticks), RET 4
    UpdateGattlingStage    TechnoClass 0x0070E000  thiscall (ticks), RET 4  (the decay)
    GetStage 0x0070DDC0, SetStage 0x0070DDD0 (ignores v < 0), GetValue 0x0070DDF0,
    SetValue 0x0070DE00 (ignores v < 0), DecreaseValue 0x0070DE40 (BuildingClass::Update's decay)

Each history builds one object with an ORIGINAL class vtable (Unit 0x007F5C70 or Building
0x007E3EBC; nothing is cloned or stubbed), a supplied TechnoType block and supplied WeaponTypes,
seeds g_MainRng 0x00886B88 in place through RandomClass's seed routine 0x0065C6D0 (the routine
Init_Random_Number_System 0x0052FE3E/0x0052FE8D runs before copying 0xFD dwords into 0x00886B88 at
0x0052FE51/0x0052FEAB), then runs its operations in order on the same object and RNG. Every native
call starts from ECX = object, the argument and a return address on a fresh stack frame.

Runs natively: GetTechnoType vt+0x84 0x006F3270 -> vt+0x88 (Unit 0x00741490 +0x6C4, Building
0x00459EE0 +0x520); GetWeapon vt+0x3F8 (Unit 0x0070E140; Building 0x004526F0 -> vt+0x400 0x00458DD0,
not occupiable, -> 0x0070E140) with the type's weapon getters 0x007177C0 / 0x007177E0;
VeterancyClass::IsElite 0x00750010 (float at +0x150 >= [0x007E37B4] = 2.0f); the audio helpers
0x00405D40 (stop) and 0x00406060 (release) on the controllers at +0x4A4 ("a") and +0x4C0 ("b");
VocClass::PlayAt 0x007509E0; RandomClass::Next 0x0065C780.

Fixture globals: [0x0087E2A0] (audio manager) is 0 in the image and asserted 0, so both helpers
take their early exits (0x00405D53 -> 0x00405E71, 0x0040606E -> 0x004060E7). Those exits are NOT
write-free: each stores 0 into controller+8 (0x00405E71 `mov [eax+8],ebp`, 0x004060E7
`mov [esi+8],ebx`), i.e. +0x4AC for "a" and +0x4C8 for "b"; the write hook checks every such store
is 0 and nothing else is written. [0x008464AC] (sound enabled) is 1 in the image's .data; the
fixture writes 0 so PlayAt returns at once (0x007509EA -> 0x007509EC..0x007509EF, RET 4); reaching
0x007509F2 fails the row. PlayAt is recorded at its entry: ECX sound index, the coordinate EDX points
to (copied from +0x9C), the controller argument.

Recorded per call: op, arg, ret (getters), value +0x144, stage +0x140, latch +0x4B8, dead +0x4D4,
draws (every g_MainRng Next return value, in order; ECX must be 0x00886B88), plays, audio (ordered
helper calls, "stop a" / "stop b" / "release a"), writes (object offsets written, sorted) and path
(tags of executed key instructions, in order); audio, writes and path are joined strings:
    add    0x0070DEE5  value += RateUp*ticks (the pre-add value was below the cap)
    up     0x0070DF55  stage-up taken (old value >= next threshold)
    report 0x0070DF94  latch clear and Report.Count > 0: draw and play
    snap   0x0070E043  value forced to 0 (SUB result negative, or RateDown*ticks == 0)
    idle   0x0070E061  value == 0 and stage == 0 after the subtraction
    down   0x0070E0E9  stage-down taken (new value < current threshold)
Fixture-only operation: "veterancy" writes the float at +0x150 (no native code runs).

Row schema. A history states `class`, `type` (a name in TYPES), optional `type_overrides`
(weapon_stages, stage [6], elite_stage [6], rate_up, rate_down, weapons, elite_weapons), `seed`,
optional `initial` {value, stage, latch, dead, veterancy} (0, 0, 0, 0, 0.0) and `ops`, a list
of [op, arg, count] runs (count consecutive calls; getters take arg 0). A float
may be given as "0x" + 8 hex digits of binary32. A weapon is {"name", "report": [sound indices],
"report_count" (optional, default len(report))}; weapons[i] lands at TechnoType +0x898+0x1C*i,
elite_weapons[i] at +0xA94+0x1C*i (null = empty slot); Report is WeaponType +0xBC (items +0xC0,
count +0xCC). Report sound indices are placeholders that name the weapon slot (base n+1, elite
101+n); retail gives each stage's base and elite weapon the same GattlingGunAttackLoopN.

`faults` lists inputs that fault natively (verified here, excluded from the histories).
Rust consumer: src/sim/combat/gattling_tests.rs (`original_stage_histories`,
`original_fault_inputs_play_nothing`).
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
INCREASE, UPDATE = 0x70DE70, 0x70E000
ENTRIES = {"increase": (INCREASE, 1), "update": (UPDATE, 1), "get_stage": (0x70DDC0, 0),
           "set_stage": (0x70DDD0, 1), "get_value": (0x70DDF0, 0), "set_value": (0x70DE00, 1),
           "decrease_value": (0x70DE40, 1)}
VTABLES = {"unit": 0x7F5C70, "building": 0x7E3EBC}
TYPE_AT = {"unit": 0x6C4, "building": 0x520}
GET_WEAPON = {"unit": 0x70E140, "building": 0x4526F0}
STOP, RELEASE, PLAY_AT, PLAY_AT_CONTINUES = 0x405D40, 0x406060, 0x7509E0, 0x7509F2
NEXT, NEXT_RETURNS, RANDOM_RANGED, SEED = 0x65C780, (0x65C787, 0x65C7D0), 0x65C7E0, 0x65C6D0
MAIN_RNG, RNG_SIZE = 0x886B88, 0x3F4
AUDIO_MANAGER, SOUND_ENABLED, ELITE_THRESHOLD = 0x87E2A0, 0x8464AC, 0x7E37B4
CONTROLLERS = {0x4A4: "a", 0x4C0: "b"}
PATH_TAGS = {0x70DEE5: "add", 0x70DF55: "up", 0x70DF94: "report", 0x70E043: "snap",
             0x70E061: "idle", 0x70E0E9: "down"}
BODIES = ((0x70DDC0, 0x70E119),)

# ---- scratch layout ---------------------------------------------------------------------------
REGION = 0x40000
SP = STACK_BASE + STACK_SIZE - 0x1000
OBJ, TYPE = SCRATCH, SCRATCH + 0x2000
WEAPONS, REPORTS = SCRATCH + 0x8000, SCRATCH + 0xC000       # weapon n: +0x200*n, items +0x40*n
LOCATION = (0x1480, 0x1280, 0x68)
OBJECT_WRITES = {0x140: "stage", 0x144: "value", 0x4B8: "latch", 0x4D4: "dead",
                 0x4AC: "controller a +8", 0x4C8: "controller b +8"}


def u32(value):
    return struct.pack("<I", int(value) & 0xFFFFFFFF)


def i32(raw):
    return struct.unpack("<i", u32(raw))[0]


def f32(value):
    if isinstance(value, str):
        return u32(int(value, 16))
    return struct.pack("<f", float(value))


# ---- types ------------------------------------------------------------------------------------
def weapon(name, slot, elite=False):
    return {"name": name, "report": [(101 if elite else 1) + slot]}


def weapons(names, elite=False):
    return [weapon(name, n, elite) for n, name in enumerate(names)]


YTNK_ELITE = ["AGGattlingE", "AAGattlingE", "AGGattling2E", "AAGattling2E", "AGGattling3E",
              "AAGattling3E"]
TYPES = {   # retail rulesmd.ini [YTNK] / [YAGGUN] (lane section 1)
    "YTNK": {"weapon_stages": 3, "stage": [200, 400, 600, 0, 0, 0],
             "elite_stage": [100, 200, 300, 0, 0, 0], "rate_up": 1, "rate_down": 50,
             "weapons": weapons(["AGGattling", "AAGattling", "AGGattling2", "AAGattling2",
                                 "AGGattling3", "AAGattling3"]),
             "elite_weapons": weapons(YTNK_ELITE, elite=True)},
    "YAGGUN": {"weapon_stages": 3, "stage": [200, 400, 600, 0, 0, 0],
               "elite_stage": [100, 200, 300, 0, 0, 0], "rate_up": 1, "rate_down": 50,
               "weapons": weapons(["AGGattling", "AAGattCann", "AGGattling2", "AAGattCann2",
                                   "AGGattling3", "AAGattCann3"]),
               "elite_weapons": weapons(YTNK_ELITE, elite=True)},
}
INITIAL = {"value": 0, "stage": 0, "latch": 0, "dead": 0, "veterancy": 0.0}


def resolve(history):
    extra = set(history) - {"name", "class", "type", "type_overrides", "seed", "initial", "ops"}
    if extra:
        raise ValueError(f"{history['name']}: unknown keys {sorted(extra)}")
    kind = dict(TYPES[history["type"]])
    for key, value in history.get("type_overrides", {}).items():
        if key not in kind:
            raise ValueError(f"{history['name']}: unknown type field {key}")
        kind[key] = value
    for key in ("stage", "elite_stage"):
        if len(kind[key]) != 6:
            raise ValueError(f"{history['name']}: {key} needs 6 entries")
    initial = {**INITIAL, **history.get("initial", {})}
    if set(initial) != set(INITIAL):
        raise ValueError(f"{history['name']}: unknown initial fields")
    ops = [(op, arg) for op, arg, count in history["ops"] for _ in range(count)]
    for op, _ in ops:
        if op not in ENTRIES and op != "veterancy":
            raise ValueError(f"{history['name']}: unknown op {op}")
    return kind, initial, ops


class Fixture:
    def __init__(self):
        u = self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(SCRATCH, REGION)
        u.mem_map(RET_MAGIC, 0x1000)
        for name, address, expected in (("audio manager", AUDIO_MANAGER, 0),
                                        ("sound enabled", SOUND_ENABLED, 1)):
            value = self.u.mem_read(address, 1 if expected == 1 else 4)
            if int.from_bytes(value, "little") != expected:
                raise OracleError(f"image {name} [0x{address:08X}] = {value.hex()}, expected {expected}")
        if bytes(u.mem_read(ELITE_THRESHOLD, 4)) != struct.pack("<f", 2.0):
            raise OracleError("[0x007E37B4] is not 2.0f")
        u.mem_write(SOUND_ENABLED, b"\0")
        u.hook_add(UC_HOOK_CODE, self.on_stop, begin=STOP, end=STOP)
        u.hook_add(UC_HOOK_CODE, self.on_release, begin=RELEASE, end=RELEASE)
        u.hook_add(UC_HOOK_CODE, self.on_play, begin=PLAY_AT, end=PLAY_AT)
        u.hook_add(UC_HOOK_CODE, self.on_forbidden, begin=PLAY_AT_CONTINUES, end=PLAY_AT_CONTINUES)
        u.hook_add(UC_HOOK_CODE, self.on_forbidden, begin=RANDOM_RANGED, end=RANDOM_RANGED)
        u.hook_add(UC_HOOK_CODE, self.on_next, begin=NEXT, end=NEXT)
        for address in NEXT_RETURNS:
            u.hook_add(UC_HOOK_CODE, self.on_next_return, begin=address, end=address)
        for address in PATH_TAGS:
            u.hook_add(UC_HOOK_CODE, self.on_path, begin=address, end=address)
        u.hook_add(UC_HOOK_MEM_WRITE, self.on_write)
        self.seeding = False
        self.reset_log()

    def reset_log(self):
        self.draws, self.plays, self.audio, self.writes, self.path = [], [], [], set(), []
        self.violations, self.pending_next = [], None

    # -- memory helpers
    def word(self, address):
        return struct.unpack("<I", self.u.mem_read(address, 4))[0]

    def fail(self, message):
        self.violations.append(message)
        self.u.emu_stop()

    # -- hooks
    def controller(self, address):
        name = CONTROLLERS.get(address - OBJ)
        if name is None:
            self.fail(f"audio helper on 0x{address:08X}")
        return name

    def on_stop(self, u, _address, _size, _data):
        self.audio.append(f"stop {self.controller(u.reg_read(UC_X86_REG_ECX))}")

    def on_release(self, u, _address, _size, _data):
        self.audio.append(f"release {self.controller(u.reg_read(UC_X86_REG_ECX))}")

    def on_play(self, u, _address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        coord = list(struct.unpack("<3i", u.mem_read(u.reg_read(UC_X86_REG_EDX), 12)))
        self.plays.append({"sound": i32(u.reg_read(UC_X86_REG_ECX)), "coord": coord,
                           "controller": self.controller(self.word(sp + 4))})
        self.audio.append("play")

    def on_next(self, u, _address, _size, _data):
        if u.reg_read(UC_X86_REG_ECX) != MAIN_RNG:
            return self.fail(f"Random::Next on 0x{u.reg_read(UC_X86_REG_ECX):08X}")
        self.pending_next = self.word(u.reg_read(UC_X86_REG_ESP))

    def on_next_return(self, u, _address, _size, _data):
        if self.pending_next is None:
            return self.fail("Random::Next return without entry")
        self.draws.append(u.reg_read(UC_X86_REG_EAX))
        self.pending_next = None

    def on_path(self, _u, address, _size, _data):
        self.path.append(PATH_TAGS[address])

    def on_forbidden(self, u, address, _size, _data):
        self.fail(f"reached 0x{address:08X} from 0x{self.word(u.reg_read(UC_X86_REG_ESP)):08X}")

    def on_write(self, u, _access, address, size, value, _data):
        if STACK_BASE <= address and address + size <= STACK_BASE + STACK_SIZE:
            return
        if MAIN_RNG <= address and address + size <= MAIN_RNG + RNG_SIZE and (
                self.seeding or self.pending_next is not None):
            return
        offset = address - OBJ
        if offset in OBJECT_WRITES and size in (1, 4):
            if offset in (0x4AC, 0x4C8) and (size != 4 or value != 0):
                return self.fail(f"controller +8 store of {value:#x}")
            self.writes.add(offset)
            return
        self.fail(f"write of {size} at 0x{address:08X} from 0x{u.reg_read(UC_X86_REG_EIP):08X}")

    # -- fixture
    def build(self, cls, kind, initial, seed):
        u = self.u
        u.mem_write(SCRATCH, bytes(REGION))
        u.mem_write(SP - 0x3000, bytes(0x3100))
        blocks = {}

        def weapon_type(entry):
            if entry is None:
                return 0
            key = (entry["name"], tuple(entry["report"]), entry.get("report_count"))
            if key not in blocks:
                n = len(blocks)
                address, items = WEAPONS + 0x200 * n, REPORTS + 0x40 * n
                u.mem_write(items, b"".join(u32(v) for v in entry["report"]))
                u.mem_write(address + 0xC0, u32(items))
                u.mem_write(address + 0xCC, u32(entry.get("report_count", len(entry["report"]))))
                blocks[key] = address
            return blocks[key]

        u.mem_write(TYPE + 0xCD5, b"\1")                       # IsGattling (not read here)
        u.mem_write(TYPE + 0xCD8, u32(kind["weapon_stages"]))
        u.mem_write(TYPE + 0xCDC, b"".join(u32(v) for v in kind["stage"]))
        u.mem_write(TYPE + 0xCF4, b"".join(u32(v) for v in kind["elite_stage"]))
        u.mem_write(TYPE + 0xD0C, u32(kind["rate_up"]) + u32(kind["rate_down"]))
        for base, table in ((0x898, kind["weapons"]), (0xA94, kind["elite_weapons"])):
            for n, entry in enumerate(table):
                u.mem_write(TYPE + base + 0x1C * n, u32(weapon_type(entry)))
        u.mem_write(OBJ, u32(VTABLES[cls]))
        u.mem_write(OBJ + TYPE_AT[cls], u32(TYPE))
        u.mem_write(OBJ + 0x9C, b"".join(u32(v) for v in LOCATION))
        u.mem_write(OBJ + 0x140, u32(initial["stage"]))
        u.mem_write(OBJ + 0x144, u32(initial["value"]))
        u.mem_write(OBJ + 0x150, f32(initial["veterancy"]))
        u.mem_write(OBJ + 0x4B8, bytes([initial["latch"]]))
        u.mem_write(OBJ + 0x4D4, bytes([initial["dead"]]))
        # g_MainRng through the native seed routine, in place.
        self.seeding = True
        self.native(SEED, MAIN_RNG, [seed], count=50_000)
        self.seeding = False
        self.reset_log()

    def native(self, entry, this, args, count=20_000):
        u = self.u
        u.mem_write(SP, u32(RET_MAGIC) + b"".join(u32(a) for a in args))
        for register in (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EDX, UC_X86_REG_ESI,
                         UC_X86_REG_EDI, UC_X86_REG_EBP):
            u.reg_write(register, 0)
        u.reg_write(UC_X86_REG_ECX, this)
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        u.reg_write(UC_X86_REG_FPSW, 0)
        u.reg_write(UC_X86_REG_FPTAG, 0xFFFF)
        try:
            run_checked(u, entry, RET_MAGIC, count=count)
        except OracleError as error:
            if self.violations:
                raise OracleError(f"{self.violations}") from error
            raise
        if self.violations:
            raise OracleError(f"{self.violations}")
        if u.reg_read(UC_X86_REG_ESP) != SP + 4 + 4 * len(args):
            raise OracleError(f"0x{entry:08X}: unbalanced stack")
        return u.reg_read(UC_X86_REG_EAX)

    def state(self):
        u = self.u
        return {"value": i32(self.word(OBJ + 0x144)), "stage": i32(self.word(OBJ + 0x140)),
                "latch": u.mem_read(OBJ + 0x4B8, 1)[0], "dead": u.mem_read(OBJ + 0x4D4, 1)[0]}

    def run(self, history):
        kind, initial, ops = resolve(history)
        self.build(history["class"], kind, initial, history["seed"])
        calls = []
        for op, arg in ops:
            if op == "veterancy":
                self.u.mem_write(OBJ + 0x150, f32(arg))
                calls.append({"op": op, "arg": arg})
                continue
            entry, nargs = ENTRIES[op]
            self.reset_log()
            try:
                ret = self.native(entry, OBJ, [arg] if nargs else [])
            except OracleError as error:
                raise OracleError(f"{history['name']} {op}({arg}): {error}") from error
            if self.pending_next is not None:
                raise OracleError(f"{history['name']}: Random::Next did not return")
            if bytes(self.u.mem_read(AUDIO_MANAGER, 4)) != bytes(4):
                raise OracleError("audio manager global changed")
            call = {"op": op, "arg": arg}
            if op.startswith("get_"):
                call["ret"] = i32(ret)
            call.update(self.state())
            call.update(draws=self.draws, plays=self.plays, audio=", ".join(self.audio),
                        writes=" ".join(f"0x{o:03X}" for o in sorted(self.writes)),
                        path=" ".join(self.path))
            calls.append(call)
        return {"input": history, "calls": calls}

    def fault(self, case):
        """A history whose last op must fault; returns the faulting instruction."""
        history = case["history"]
        kind, initial, ops = resolve(history)
        self.build(history["class"], kind, initial, history["seed"])
        *prefix, (op, arg) = ops
        for p_op, p_arg in prefix:
            self.native(ENTRIES[p_op][0], OBJ, [p_arg] if ENTRIES[p_op][1] else [])
        self.reset_log()
        try:
            self.native(ENTRIES[op][0], OBJ, [arg] if ENTRIES[op][1] else [])
        except OracleError as error:
            if self.violations:
                raise
            pc = self.u.reg_read(UC_X86_REG_EIP)
            if "faulted" not in str(error):
                raise
            return {"input": case, "fault_at": f"0x{pc:08X}"}
        raise OracleError(f"{history['name']}: expected a native fault")


# ---- histories --------------------------------------------------------------------------------
def h(name, ops, *, cls="unit", kind="YTNK", seed=0x2A61, initial=None, overrides=None):
    runs = []
    for op, arg in ops:     # run-length [op, arg, count]
        if runs and runs[-1][:2] == [op, arg]:
            runs[-1][2] += 1
        else:
            runs.append([op, arg, 1])
    row = {"name": name, "class": cls, "type": kind, "seed": seed, "ops": runs}
    if initial:
        row["initial"] = initial
    if overrides:
        row["type_overrides"] = overrides
    return row


def repeat(op, arg, n):
    return [[op, arg] for _ in range(n)]


def retail_histories():
    out = [
        h("ytnk.rookie.charge_to_cap", repeat("increase", 1, 603)),
        h("ytnk.elite.charge_to_cap", repeat("increase", 1, 303), seed=0x51F0,
          initial={"veterancy": 2.0}),
        h("ytnk.rookie.decay_from_cap", repeat("update", 1, 14),
          initial={"value": 600, "stage": 2, "latch": 1}),
        h("ytnk.elite.decay_from_cap", repeat("update", 1, 8),
          initial={"value": 300, "stage": 2, "latch": 1, "veterancy": 2.0}),
        h("ytnk.rookie.charge_lose_target_reacquire",
          repeat("increase", 1, 250) + repeat("update", 1, 3) + repeat("increase", 1, 60)
          + repeat("update", 1, 1) + repeat("increase", 1, 2), seed=0x7777),
        h("ytnk.veteran_is_not_elite", repeat("increase", 1, 202), initial={"veterancy": 1.0}),
        h("yaggun.rookie.charge_to_cap", repeat("increase", 1, 603), cls="building",
          kind="YAGGUN", seed=0x0BAD),
        h("yaggun.elite.charge_to_cap", repeat("increase", 1, 303), cls="building",
          kind="YAGGUN", seed=0x0BAD, initial={"veterancy": 3.0}),
        h("yaggun.rookie.decay_from_cap", repeat("update", 1, 14), cls="building",
          kind="YAGGUN", initial={"value": 600, "stage": 2, "latch": 1}),
        # Building ticks come from the mission counter: charge and decay by several at once.
        h("yaggun.rookie.multi_tick_charge_decay",
          repeat("increase", 2, 110) + repeat("increase", 5, 45) + repeat("update", 2, 4)
          + repeat("update", 5, 3), cls="building", kind="YAGGUN", seed=0x1234),
    ]
    return out


def veterancy_histories():
    return [
        h("veterancy.promote_mid_charge",
          repeat("increase", 1, 250) + [["veterancy", 2.0]] + repeat("increase", 1, 55)),
        h("veterancy.demote_at_elite_cap",
          [["veterancy", 2.0]] + repeat("increase", 1, 301) + [["veterancy", 0.0]]
          + repeat("increase", 1, 3) + repeat("update", 1, 2)),
        h("veterancy.promote_mid_decay",
          repeat("update", 1, 3) + [["veterancy", 2.0]] + repeat("update", 1, 5),
          initial={"value": 600, "stage": 2, "latch": 1}),
        h("veterancy.float_edges",
          [["veterancy", "0x3FFFFFFF"], ["increase", 1], ["veterancy", 2.0], ["increase", 1],
           ["veterancy", "0x7F800000"], ["increase", 1], ["veterancy", "0x7FC00000"],
           ["increase", 1], ["veterancy", -3.0], ["increase", 1]],
          initial={"value": 150, "stage": 0}),
    ]


def stage_count_histories():
    out = []
    tables = {0: ([11, 22, 33, 44, 55, 66], [7, 8, 9, 10, 11, 12]),
              1: ([100, 0, 0, 0, 0, 0], [50, 0, 0, 0, 0, 0]),
              2: ([100, 300, 0, 0, 0, 0], [50, 150, 0, 0, 0, 0]),
              3: ([100, 200, 300, 0, 0, 0], [50, 100, 150, 0, 0, 0]),
              6: ([100, 200, 300, 400, 500, 600], [50, 100, 150, 200, 250, 300])}
    for stages, (stage, elite) in tables.items():
        over = {"weapon_stages": stages, "stage": stage, "elite_stage": elite, "rate_up": 50,
                "rate_down": 50,
                "weapons": weapons([f"W{n}" for n in range(12)]),
                "elite_weapons": weapons([f"E{n}" for n in range(12)], elite=True)}
        for vet in (0.0, 2.0):
            name = f"weapon_stages_{stages}.{'elite' if vet else 'rookie'}"
            out.append(h(name, repeat("increase", 1, 14) + repeat("update", 1, 14),
                         overrides=over, initial={"veterancy": vet}))
    return out


def rate_histories():
    out = []
    for rate in (0, 1, 50, -1, 0x7FFFFFFF):
        out.append(h(f"rate_up_{rate}", [["increase", 1], ["increase", 2], ["increase", 1],
                                         ["increase", 37]],
                     overrides={"rate_up": rate, "stage": [0x7FFFFFFF] * 6},
                     initial={"value": 150}))
        out.append(h(f"rate_down_{rate}", [["update", 1], ["update", 2], ["update", 1],
                                           ["update", 37]],
                     overrides={"rate_down": rate}, initial={"value": 1000, "stage": 2}))
    # Signed wrap at the top and the sign of the wrapped SUB result.
    out += [
        h("wrap.increase_past_int_max", repeat("increase", 1, 3),
          overrides={"rate_up": 0x20, "stage": [0x7FFFFFFF] * 6},
          initial={"value": 0x7FFFFFF0, "stage": 0, "latch": 1}),
        h("wrap.update_int_min_minus_one", [["update", 1]],
          overrides={"rate_down": 1}, initial={"value": -0x80000000, "stage": 1}),
        h("wrap.update_negative_value", [["update", 1]],
          overrides={"rate_down": 50}, initial={"value": -5, "stage": 1}),
        h("wrap.update_negative_rate_increases", repeat("update", 1, 3),
          overrides={"rate_down": -1}, initial={"value": -5, "stage": 2}),
        h("wrap.update_huge_value", [["update", 1]],
          initial={"value": 0x7FFFFFFF, "stage": 2}),
    ]
    return out


def tick_histories():
    return [
        h("ticks.increase_0_1_2_37", [["increase", 0], ["increase", 1], ["increase", 2],
                                       ["increase", 37], ["increase", 0]],
          initial={"value": 190}),
        h("ticks.increase_0_at_threshold", [["increase", 0], ["increase", 0]],
          initial={"value": 200, "latch": 1}),
        h("ticks.cap_overshoot_37", repeat("increase", 37, 3), initial={"value": 599, "stage": 2}),
        h("ticks.cap_overshoot_2", repeat("increase", 2, 2), initial={"value": 599, "stage": 2}),
        h("ticks.update_0_snaps", [["update", 0]], initial={"value": 500, "stage": 2}),
        h("ticks.update_2_37", [["update", 2], ["update", 37], ["update", 1]],
          initial={"value": 2000, "stage": 2}),
    ]


def stage_range_histories():
    """Stages outside 0..WeaponStages-1 (unreachable through the setters; body behaviour only)."""
    long = {"weapons": weapons([f"W{n}" for n in range(12)]),
            "elite_weapons": weapons([f"E{n}" for n in range(12)], elite=True)}
    return [
        h("stage_range.top_stage_never_rises", repeat("increase", 1, 2),
          initial={"value": 700, "stage": 2}),
        h("stage_range.stage_3_of_3_latched", [["increase", 1]] + repeat("update", 1, 4),
          initial={"value": 700, "stage": 3, "latch": 1}),
        h("stage_range.stage_3_of_3_weapon_slot_6", [["increase", 1], ["update", 1]],
          overrides=long, initial={"value": 700, "stage": 3}),
        h("stage_range.stage_5_of_3", [["update", 1], ["update", 1], ["increase", 1]],
          overrides=long, initial={"value": 100, "stage": 5}),
        h("stage_range.negative_stage_latched", [["increase", 1], ["update", 1]],
          initial={"value": 300, "stage": -1, "latch": 1}),
        h("stage_range.negative_stage_value_0", [["update", 1]],
          initial={"value": 0, "stage": -1}),
    ]


def report_histories():
    three = [{"name": f"AGGattling3x{n}", "report": [11 + n, 22 + n, 33 + n]} for n in range(6)]
    return [
        h("report.count_0", repeat("increase", 1, 3) + [["update", 1], ["increase", 1]],
          overrides={"weapons": [{"name": "Silent", "report": []}] * 6,
                     "elite_weapons": [None] * 6}),
        h("report.count_negative", repeat("increase", 1, 2),
          overrides={"weapons": [{"name": "Negative", "report": [5], "report_count": -1}] * 6,
                     "elite_weapons": [None] * 6}),
        h("report.count_3", repeat("increase", 1, 2) + [["update", 1]] + repeat("increase", 1, 1)
          + [["update", 1]] + repeat("increase", 1, 1) + [["update", 1]] + repeat("increase", 1, 1)
          + [["update", 1]] + repeat("increase", 1, 1),
          overrides={"weapons": three, "elite_weapons": [None] * 6}, seed=0x3333),
        h("report.elite_slot_empty_falls_back",
          repeat("increase", 1, 2), overrides={"elite_weapons": [None] * 6},
          initial={"veterancy": 2.0}),
    ]


def latch_histories():
    return [
        h("latch.set_no_stage_up", repeat("increase", 1, 2), initial={"value": 10, "latch": 1}),
        h("latch.set_stage_up_clears", repeat("increase", 1, 2),
          initial={"value": 200, "latch": 1}),
        h("latch.clear_every_call_draws_once", repeat("increase", 1, 3), initial={"value": 10}),
        h("latch.update_clears", [["update", 1], ["increase", 1]],
          initial={"value": 300, "stage": 1, "latch": 1}),
        h("dead.increase_clears", [["increase", 1]], initial={"value": 10, "dead": 1, "latch": 1}),
        h("dead.update_idle_stops_both", [["update", 1]],
          initial={"value": 0, "stage": 0, "dead": 1}),
        h("dead.update_idle_latch_cleared_first", [["update", 1]],
          initial={"value": 30, "stage": 0, "latch": 1}),
        h("dead.update_idle_quiet", [["update", 1], ["update", 1]], initial={"value": 0}),
        h("dead.update_stage_down_clears", [["update", 1]],
          initial={"value": 400, "stage": 2, "dead": 1, "latch": 1}),
    ]


def accessor_histories():
    return [
        h("accessors.stage", [["get_stage", 0], ["set_stage", 2], ["get_stage", 0],
                              ["set_stage", -1], ["get_stage", 0], ["set_stage", 0x7FFFFFFF],
                              ["get_stage", 0], ["set_stage", 0], ["get_stage", 0]]),
        h("accessors.value", [["get_value", 0], ["set_value", 450], ["get_value", 0],
                              ["set_value", -1], ["get_value", 0], ["set_value", -0x80000000],
                              ["set_value", 0x7FFFFFFF], ["get_value", 0], ["set_value", 0]]),
        h("accessors.decrease_value",
          [["set_value", 600], ["decrease_value", 50], ["decrease_value", 0], ["set_value", 600],
           ["decrease_value", 600], ["set_value", 600], ["decrease_value", 601],
           ["set_value", 600], ["decrease_value", -1], ["decrease_value", 0x7FFFFFFF],
           ["set_value", 5], ["decrease_value", -0x80000000]]),
        h("accessors.decrease_value_wraps", [["decrease_value", 1]],
          initial={"value": -0x80000000}),
    ]


def histories():
    return (retail_histories() + veterancy_histories() + stage_count_histories()
            + rate_histories() + tick_histories() + stage_range_histories()
            + report_histories() + latch_histories() + accessor_histories())


def fault_cases():
    return [
        {"why": "latch clear, GetWeapon(2*stage) slot empty: Report count read through a NULL "
                "WeaponType",
         "history": h("fault.stage_3_of_3_empty_slot", [["increase", 1]],
                      initial={"value": 700, "stage": 3})},
        {"why": "latch clear, NULL WeaponType in the stage-0 slot",
         "history": h("fault.null_weapon", [["increase", 1]],
                      overrides={"weapons": [None] * 6, "elite_weapons": [None] * 6})},
    ]


def generate():
    rows = histories()
    names = [row["name"] for row in rows]
    if len(set(names)) != len(names):
        raise OracleError("duplicate history names")
    fixture = Fixture()
    return {"types": TYPES, "initial": INITIAL, "location": list(LOCATION),
            "histories": [fixture.run(row) for row in rows],
            "faults": [fixture.fault(case) for case in fault_cases()]}


def metadata():
    return provenance(
        scope="TechnoClass IncreaseGattlingStage 0x0070DE70 and UpdateGattlingStage 0x0070E000 "
              "with GetStage/SetStage/GetValue/SetValue/DecreaseValue, as retained histories on "
              "one object: value, stage, report latch +0x4B8, dead latch +0x4D4, g_MainRng draws, "
              "PlayAt arguments, audio-helper order and object writes per call. Retail YTNK/YAGGUN "
              "charge and decay to the cap and back, veterancy flips, WeaponStages 0/1/2/3/6, "
              "RateUp/RateDown 0/1/50/-1/0x7FFFFFFF, ticks 0/1/2/37, out-of-range stages, Report "
              "count 0/1/3/-1, latches. Not the callers, weapon selection or audio playback.",
        assumptions=[
            "One emulator; per history the scratch region is zeroed and the object, type and "
            "weapons rebuilt; ops run in order on the same object; FPCW 0x0E7F, empty x87 "
            "stack and zeroed general registers per native call.",
            "g_MainRng 0x00886B88 is seeded per history by RandomClass seed routine 0x0065C6D0 "
            "(ECX = 0x00886B88, the history's seed); Init_Random_Number_System builds the same "
            "state on the stack (0x0052FE3E, 0x0052FE8D) and copies 0xFD dwords into "
            "0x00886B88 (0x0052FE51, 0x0052FEAB).",
            "Image globals asserted: [0x0087E2A0] = 0 (audio helpers take their early exits), "
            "[0x007E37B4] = 2.0f; [0x008464AC] = 1 in the image and written 0 by the fixture so "
            "PlayAt 0x007509E0 returns at 0x007509EF; reaching 0x007509F2 fails.",
            "Object: original Unit 0x007F5C70 or Building 0x007E3EBC vtable; type pointer +0x6C4 "
            "/ +0x520; Location +0x9C = (0x1480, 0x1280, 0x68); veterancy float +0x150; audio "
            "controllers +0x4A4 and +0x4C0 zeroed. Building +0x702 = 0 and type +0x157B = 0, so "
            "Building GetWeapon 0x004526F0 resolves through 0x0070E140.",
            "Writes allowed: stack; object +0x140, +0x144, +0x4B8, +0x4D4; controller +8 words "
            "+0x4AC/+0x4C8 only with 0 (the audio helpers' early-exit stores 0x00405E71, "
            "0x004060E7); g_MainRng only inside a Next call. Anything else fails.",
            "Report sound indices are fixture placeholders naming the weapon slot.",
            "Faulting inputs are verified and listed under `faults`, not used in histories.",
        ],
        substitutions=["[0x008464AC] = 0 (fixture global; no code replaced). No stubs."],
        entry_points={"increase_gattling_stage": INCREASE, "update_gattling_stage": UPDATE,
                      "get_stage": 0x70DDC0, "set_stage": 0x70DDD0, "get_value": 0x70DDF0,
                      "set_value": 0x70DE00, "decrease_value": 0x70DE40, "rng_seed": SEED,
                      "rng_next": NEXT, "is_elite": 0x750010, "play_at": PLAY_AT,
                      "audio_stop": STOP, "audio_release": RELEASE,
                      "unit_get_weapon": GET_WEAPON["unit"],
                      "building_get_weapon": GET_WEAPON["building"]})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
