"""Original AnimClass constructor 0x00421EA0 through its Bouncer arm and BounceClass::Init.

Executes the whole constructor natively on a fresh AnimClass: the ObjectClass
constructor, the anim-array append, the RandomRate draw 0x004221F5, the layer
height 0x005F6060, Unlimbo 0x005F4EC0 (with the anim's own vt+0x1AC/+0x88/+0x1B4
and the AnimType's vt+0x6C, all original), the Bouncer arm 0x004224D9..0x00422648
(start = Location + (0,0,10); three Random::Next draws 0x00422552/0x00422564/
0x00422579; ftol/idiv velocity arithmetic) and BounceClass::Init 0x004397E0
(three RandomRanged(-0xFFFF, 0xFFFF) axis draws, sqrt normalisation and the
quaternion builders). The Scenario RNG (`[0x00A8B230]+0x218`) is seeded through
0x0065C6D0; every draw is logged with its call site, arguments and result, and the
raw generator advances (0x0065C788 in Next, 0x0065C837 in Ranged) are counted.

`debris_loop` rows execute the retail TechnoClass::ReceiveDamage MetallicDebris
loop 0x007024E0..0x00702572 with the piece count in EBX: per piece `new(0x1C8)`,
the techno's vt+0x48 coordinate + (0,0,20), idx = RandomRanged(0, Rules+0x14C - 1)
at 0x0070253A, then this constructor at 0x00702566 with (Rules+0x140)[idx],
delay 0, loop 1, flags 0x600, 0, 0.

`rate_read` executes the AnimType RandomRate post-read 0x00428777..0x004287DC
(900/x per bound, non-positive -> 0, max < 0 -> 0, min clamped to max) on the INI
pair, and the constructor then reads the stored +0x2E4/+0x2E8.

Supplied: AnimType fields the path reads (retail artmd.ini DBRIS values, AnimType
constructor defaults elsewhere); Rules (+0x140/+0x14C list, +0x7B4, +0x147C);
the anim array (0x00A8E9A8); stubs for MapClass cell lookup 0x00565730, the
redraw mark 0x005F5850 (reached through vt+0x124), the display-layer submit
0x004A9720, AnimClass::Start 0x00424CE0 (recorded, not run), operator new
0x007C8E17 (bump allocator) and, in the debris loop, the techno's vt+0x48.

Schema: `ctor[]` rows hold `input`, `random_rate_stored` ([+0x2E4, +0x2E8]), `events`
(ordered calls: `ranged` {site,min,max,result}, `next` {site,result}, `unlimbo`,
`layer_submit`, `bounce_init`, `start`, ...), `raw_draw_count` (generator
advances), `velocity_draws` ([a, b, c] from 0x00422552/64/79), `location`
(+0x9C), `bounce_hex` (+0x128, 0x50 bytes) and its decoded `bounce` bits,
`is_bouncing` (+0x194), `rng_before`/`rng_after` (0x3F4-byte state hex).
`debris_loop[]` rows hold `input` {pieces, coord, seed}, `events`,
`raw_draw_count`, `anims[]` (per piece, same state fields) and the RNG states.

Rust consumer: src/sim/anim_class.rs and src/sim/bounce.rs (debris launch).
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDI,
                               UC_X86_REG_EBP, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
                               UC_X86_REG_FPCW)

from tools.native_oracle import NATIVE_FPCW, finish_vectors, load_image, provenance, run_checked

CTOR, SEED, INIT = 0x421EA0, 0x65C6D0, 0x4397E0
NEXT, RANGED, NEXT_ADVANCE, RANGED_ADVANCE = 0x65C780, 0x65C7E0, 0x65C788, 0x65C837
UNLIMBO, GET_CELL, MARK, SUBMIT, START, NEW = (0x5F4EC0, 0x565730, 0x5F5850, 0x4A9720,
                                               0x424CE0, 0x7C8E17)
LOOP_BEGIN, LOOP_END = 0x7024E0, 0x702572
RATE_BEGIN, RATE_END = 0x428777, 0x4287DC
SCENARIO_PTR, RULES_PTR, FRAME = 0xA8B230, 0x8871E0, 0xA8ED84
ANIMS_ITEMS, ANIMS_CAPACITY, ANIMS_COUNT = 0xA8E9AC, 0xA8E9B0, 0xA8E9B8
GAME_RUNNING, SCENARIO_INIT = 0xA8E9A0, 0xA8E7AC
ANIM_TYPE_VT = 0x7E3608  # written by the AnimTypeClass constructor 0x00427731

MEM = 0x21000000
SCENARIO, RULES, TECHNO, TECHNO_VT, ANIM_LIST, CELL, STUB = (MEM + n * 0x1000 for n in range(7))
TYPES = MEM + 0x10000  # 0x400 bytes per AnimType
HEAP = MEM + 0x40000
SP = MEM + 0x1F0000
STOP = 0x30000000
TECHNO_COORDS = STUB
ANIM_SIZE = 0x1C8


def dwords(*values):
    return struct.pack("<" + "I" * len(values), *[v & 0xFFFFFFFF for v in values])


def read32(uc, address):
    return struct.unpack("<I", uc.mem_read(address, 4))[0]


def signed(value):
    return struct.unpack("<i", dwords(value))[0]


def f64_bits(value):
    return struct.unpack("<Q", struct.pack("<d", value))[0]


# (name, Elasticity, MaxXYVel, MinZVel, RandomRate INI pair or None). Retail from artmd.ini.
RETAIL = {
    "DBRIS1LG": (0.0, 25.0, 25.0, (220, 600)),
    "DBRIS1SM": (0.0, 30.0, 20.0, (220, 600)),
    "DBRIS1HV": (0.0, 10.0, 40.0, (220, 600)),
    "DBRIS2HV": (0.0, 5.0, 30.0, (220, 600)),
    "DBRS10SM": (0.0, 30.0, 20.0, (350, 450)),
}
METALLIC_DEBRIS = ([f"DBRIS{i}LG" for i in range(1, 10)] + ["DBRS10LG"]
                   + [f"DBRIS{i}SM" for i in range(1, 10)] + ["DBRS10SM"])


def type_values(name):
    if name in RETAIL:
        return RETAIL[name]
    return (0.0, 25.0, 25.0, (220, 600)) if name.endswith("LG") else (0.0, 30.0, 20.0, (220, 600))


class Machine:
    def __init__(self, seed):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(uc)
        uc.mem_map(MEM, 0x200000)
        uc.mem_map(STOP, 0x1000)
        uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        self.heap = HEAP
        self.events = []
        self.advances = 0
        self.pending = {}
        self.types = {}
        uc.mem_write(SCENARIO_PTR, dwords(SCENARIO))
        uc.mem_write(RULES_PTR, dwords(RULES))
        uc.mem_write(FRAME, dwords(1000))
        uc.mem_write(GAME_RUNNING, b"\x01")
        uc.mem_write(SCENARIO_INIT, dwords(0))
        uc.mem_write(ANIMS_ITEMS, dwords(ANIM_LIST))
        uc.mem_write(ANIMS_CAPACITY, dwords(64))
        uc.mem_write(ANIMS_COUNT, dwords(0))
        uc.mem_write(RULES + 0x7B4, dwords(0))
        uc.mem_write(RULES + 0x147C, dwords(0))
        uc.mem_write(SP, dwords(STOP, seed))
        uc.reg_write(UC_X86_REG_ESP, SP)
        uc.reg_write(UC_X86_REG_ECX, SCENARIO + 0x218)
        run_checked(uc, SEED, STOP, count=100_000)
        uc.hook_add(UC_HOOK_CODE, self.hook)

    def rng(self):
        return bytes(self.uc.mem_read(SCENARIO + 0x218, 0x3F4)).hex()

    def ret(self, value, pops):
        uc = self.uc
        sp = uc.reg_read(UC_X86_REG_ESP)
        uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_EIP, read32(uc, sp))
        uc.reg_write(UC_X86_REG_ESP, sp + 4 + pops)

    def hook(self, uc, address, _size, _data):
        if address in self.pending:
            self.pending.pop(address)["result"] = signed(uc.reg_read(UC_X86_REG_EAX))
        if address in (NEXT_ADVANCE, RANGED_ADVANCE):
            self.advances += 1
            return
        sp = uc.reg_read(UC_X86_REG_ESP)
        if address in (NEXT, RANGED):
            caller = read32(uc, sp) - 5
            event = dict(call="next" if address == NEXT else "ranged", site=f"0x{caller:08X}")
            if address == RANGED:
                event.update(min=signed(read32(uc, sp + 4)), max=signed(read32(uc, sp + 8)))
            self.events.append(event)
            self.pending[read32(uc, sp)] = event
        elif address == INIT:
            self.events.append(dict(call="bounce_init"))
        elif address == GET_CELL:
            self.ret(CELL, 4)
        elif address == MARK:
            self.ret(1, 4)
        elif address == SUBMIT:
            self.events.append(dict(call="layer_submit"))
            self.ret(1, 4)
        elif address == START:
            self.events.append(dict(call="start", anim=uc.reg_read(UC_X86_REG_ECX) - HEAP))
            self.ret(0, 0)
        elif address == NEW:
            size = read32(uc, sp + 4)
            out = self.heap
            self.heap += (size + 15) & ~15
            self.events.append(dict(call="new", size=size))
            self.ret(out, 0)  # cdecl
        elif address == TECHNO_COORDS:
            out = read32(uc, sp + 4)
            uc.mem_write(out, struct.pack("<iii", *self.techno_coord))
            self.events.append(dict(call="techno_coords"))
            self.ret(out, 4)
        elif address == UNLIMBO:
            self.events.append(dict(call="unlimbo",
                                    coord=list(struct.unpack("<iii", uc.mem_read(read32(uc, sp + 4), 12)))))
        elif address == CTOR:
            self.events.append(dict(call="anim_ctor", type=self.types.get(read32(uc, sp + 4)),
                                    coord=list(struct.unpack("<iii", uc.mem_read(read32(uc, sp + 8), 12))),
                                    delay=read32(uc, sp + 12), loop=read32(uc, sp + 16),
                                    flags=read32(uc, sp + 20)))

    def rate_read(self, pointer, pair):
        """Execute the AnimType RandomRate post-read on the parsed INI pair (-1,-1 = key absent)."""
        uc = self.uc
        low, high = pair if pair is not None else (-1, -1)
        uc.mem_write(SP - 0x20, dwords(0xDEAD0001, 0xDEAD0002))  # popped into ebp, edi
        uc.reg_write(UC_X86_REG_ESP, SP - 0x20)
        uc.reg_write(UC_X86_REG_ESI, pointer)
        uc.reg_write(UC_X86_REG_EBP, 0xFFFFFFFF)
        scratch = SP - 0x40
        uc.mem_write(scratch, dwords(low, high))
        uc.reg_write(UC_X86_REG_EAX, scratch)
        # 0x00428777 reads the pair 0x00529880 returned through [eax].
        run_checked(uc, RATE_BEGIN, RATE_END, count=1000)
        return [signed(read32(uc, pointer + 0x2E4)), signed(read32(uc, pointer + 0x2E8))]

    def anim_type(self, name, elasticity, max_xy, min_z, rate, max_z=3.5):
        pointer = TYPES + len(self.types) * 0x400
        self.types[pointer] = name
        uc = self.uc
        uc.mem_write(pointer, bytes(0x400))
        uc.mem_write(pointer, dwords(ANIM_TYPE_VT))
        # AnimTypeClass constructor 0x00427530 defaults for the fields this path reads.
        uc.mem_write(pointer + 0x2B0, dwords(1))              # Rate
        uc.mem_write(pointer + 0x2BC, dwords(16))             # frame counts (image-derived;
        uc.mem_write(pointer + 0x2C0, dwords(16))             # not -1, so vt+0x9C is not read)
        uc.mem_write(pointer + 0x2C4, dwords(0xFFFFFFFF))     # LoopCount=-1
        uc.mem_write(pointer + 0x2F8, dwords(0xFFFFFFFF, 0xFFFFFFFF))
        uc.mem_write(pointer + 0x310, struct.pack("<d", elasticity))
        uc.mem_write(pointer + 0x318, struct.pack("<d", min_z))
        uc.mem_write(pointer + 0x320, struct.pack("<d", max_z))  # 3.5 from 0x00427627; no INI key
        uc.mem_write(pointer + 0x328, struct.pack("<d", max_xy))
        uc.mem_write(pointer + 0x35A, b"\x01")                # Bouncer=yes
        uc.mem_write(pointer + 0x364, dwords(3))              # Layer default
        rates = self.rate_read(pointer, rate)
        return pointer, rates


def body_fields(raw):
    elasticity, gravity, clamp = struct.unpack_from("<QQQ", raw, 0)
    return dict(elasticity_bits=elasticity, gravity_bits=gravity, clamp_bits=clamp,
                position_bits=list(struct.unpack_from("<3I", raw, 0x18)),
                velocity_bits=list(struct.unpack_from("<3I", raw, 0x24)),
                orientation_bits=list(struct.unpack_from("<4I", raw, 0x30)),
                rotation_bits=list(struct.unpack_from("<4I", raw, 0x40)))


def anim_state(uc, anim):
    raw = bytes(uc.mem_read(anim + 0x128, 0x50))
    return dict(location=list(struct.unpack("<iii", uc.mem_read(anim + 0x9C, 12))),
                bounce_hex=raw.hex(), bounce=body_fields(raw),
                is_bouncing=uc.mem_read(anim + 0x194, 1)[0])


def execute(case):
    machine = Machine(case["seed"])
    uc = machine.uc
    pointer, rates = machine.anim_type(case["type"], case["elasticity"], case["max_xy"],
                                       case["min_z"], case["random_rate"])
    before = machine.rng()
    machine.events.clear()
    machine.advances = 0
    anim = machine.heap
    machine.heap += 0x200
    coord = MEM + 0x1F8000
    uc.mem_write(coord, struct.pack("<iii", *case["coord"]))
    uc.mem_write(SP, dwords(STOP, pointer, coord, 0, 1, 0x600, 0, 0))
    uc.reg_write(UC_X86_REG_ESP, SP)
    uc.reg_write(UC_X86_REG_ECX, anim)
    run_checked(uc, CTOR, STOP, count=2_000_000, required_addresses=(0x4224D9, 0x422648, INIT))
    assert uc.reg_read(UC_X86_REG_ESP) == SP + 4 + 0x1C, "the constructor pops 0x1C"
    draws = [e for e in machine.events if e["call"] in ("next", "ranged")]
    velocity_draws = [e["result"] for e in draws if e["site"] in ("0x00422552", "0x00422564",
                                                                   "0x00422579")]
    return dict(input=case, random_rate_stored=rates, events=machine.events,
                raw_draw_count=machine.advances, velocity_draws=velocity_draws,
                **anim_state(uc, anim), rng_before=before, rng_after=machine.rng())


def execute_loop(case):
    machine = Machine(case["seed"])
    uc = machine.uc
    items = MEM + 0x1F9000
    for index, name in enumerate(METALLIC_DEBRIS):
        elasticity, max_xy, min_z, rate = type_values(name)
        pointer, _ = machine.anim_type(name, elasticity, max_xy, min_z, rate)
        uc.mem_write(items + index * 4, dwords(pointer))
    uc.mem_write(RULES + 0x140, dwords(items))
    uc.mem_write(RULES + 0x14C, dwords(len(METALLIC_DEBRIS)))
    uc.mem_write(TECHNO, dwords(TECHNO_VT))
    uc.mem_write(TECHNO_VT + 0x48, dwords(TECHNO_COORDS))
    machine.techno_coord = case["coord"]
    before = machine.rng()
    machine.events.clear()
    machine.advances = 0
    first_anim = machine.heap
    uc.reg_write(UC_X86_REG_ESP, SP - 0x400)
    uc.reg_write(UC_X86_REG_ESI, TECHNO)
    uc.reg_write(UC_X86_REG_EBX, case["pieces"])
    run_checked(uc, LOOP_BEGIN, LOOP_END, count=5_000_000, required_addresses=(0x70253A, 0x702566))
    anims = []
    for piece in range(case["pieces"]):
        # new(0x1C8) is the only allocation per piece, rounded to 0x1D0 by the bump allocator.
        anims.append(anim_state(uc, first_anim + piece * 0x1D0))
    return dict(input=case, events=machine.events, raw_draw_count=machine.advances, anims=anims,
                rng_before=before, rng_after=machine.rng())


SEEDS = [1, 7, 31, 0x5CA1AB1E, 0xFFFFFFFF, 0x12345678, 42, 1000, 0x7FFFFFFF, 0x80000000,
         3, 99, 123456, 0xDEADBEEF, 0x0BADF00D, 2024]
COORDS = [[2688, 2688, 0], [128, 64, 30], [-300, 1500, 208], [40000, -12, -104]]


def cases():
    for name, (elasticity, max_xy, min_z, rate) in RETAIL.items():
        for index, seed in enumerate(SEEDS):
            yield dict(kind="ctor", type=name, elasticity=elasticity, max_xy=max_xy, min_z=min_z,
                       random_rate=list(rate), coord=COORDS[index % len(COORDS)], seed=seed)
    odd = [("elastic_0.8", 0.8, 25.0, 25.0), ("max_xy_0.5", 0.0, 0.5, 25.0),
           ("max_xy_1", 0.0, 1.0, 25.0), ("max_xy_7.25", 0.0, 7.25, 20.0),
           ("max_xy_100", 0.3, 100.0, 25.0), ("min_z_neg10", 0.0, 25.0, -10.0),
           ("min_z_neg3.25", 0.5, 12.6, -3.25), ("min_z_3.5", 0.0, 25.0, 3.5),
           ("min_z_2.25", 0.0, 25.0, 2.25), ("min_z_60.7", 1.0, 0.75, 60.7)]
    for name, elasticity, max_xy, min_z in odd:
        for index, seed in enumerate(SEEDS[:6]):
            yield dict(kind="ctor", type=name, elasticity=elasticity, max_xy=max_xy, min_z=min_z,
                       random_rate=[220, 600], coord=COORDS[(index + 1) % len(COORDS)], seed=seed)
    for rate in ([100, 900], [900, 100], [300, 300], None, [0, 450], [220, -5]):
        for seed in (1, 31, 0x5CA1AB1E):
            yield dict(kind="ctor", type=f"rate_{rate}", elasticity=0.0, max_xy=25.0, min_z=25.0,
                       random_rate=rate, coord=COORDS[0], seed=seed)


def loop_cases():
    for pieces in (1, 2, 3):
        for seed in (1, 31, 0x5CA1AB1E):
            yield dict(kind="debris_loop", pieces=pieces, coord=[3000, 2500, 104], seed=seed)


def generate():
    return dict(ctor=[execute(case) for case in cases()],
                debris_loop=[execute_loop(case) for case in loop_cases()])


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope=("AnimClass::AnimClass 0x00421EA0 on Bouncer AnimTypes (retail DBRIS LG/SM/HV and "
               "DBRS10SM values, odd Elasticity/MaxXYVel/MinZVel), through Unlimbo, the Bouncer "
               "arm and BounceClass::Init 0x004397E0, over 16 Scenario seeds and four "
               "coordinates; the RandomRate post-read 0x00428777..0x004287DC; and the "
               "ReceiveDamage MetallicDebris loop 0x007024E0..0x00702572 for 1..3 pieces."),
        assumptions=[
            "x87 control word 0x0E7F (PC53, chop) on entry; ftol 0x007C5F00 loads the image "
            "value of 0x00822D80 (0x0E7F) when it differs.",
            "AnimType fields are supplied: artmd.ini Elasticity/MaxXYVel/MinZVel, MaxZVel 3.5 "
            "(constructor 0x00427627, no INI key), Rate 1, Layer 3, LoopCount -1, frame counts "
            "16, sounds -1; ObjectType +0xAC/+0x234/+0x23A zero so Unlimbo's alpha/radar/light "
            "arms do not run.",
            "Game running (0x00A8E9A0=1), 0x00A8E7AC=0, anim array capacity 64, "
            "Rules+0x147C matches no fixture type.",
            "AnimClass::Start 0x00424CE0 is not executed; its own RNG sites (Middle 0x00424F00, "
            "the Tiberium arm 0x00424DDF) lie outside these rows.",
        ],
        substitutions=[
            "MapClass cell lookup 0x00565730 returns a dummy cell (AnimClass vt+0x1AC 0x004264C0 "
            "ignores it and returns 0)",
            "0x005F5850 (reached through AnimClass vt+0x124 0x004238B0) returns 1",
            "DisplayClass layer submit 0x004A9720 is recorded and returns 1",
            "AnimClass::Start 0x00424CE0 is recorded and returns",
            "operator new 0x007C8E17 is a bump allocator (16-byte rounding)",
            "debris loop: the techno's vt+0x48 returns the case coordinate",
        ],
        entry_points={"anim_ctor": CTOR, "bouncer_arm": 0x4224D9, "bounce_init": INIT,
                      "debris_loop": LOOP_BEGIN, "random_rate_read": 0x428777, "seed": SEED},
    ))
