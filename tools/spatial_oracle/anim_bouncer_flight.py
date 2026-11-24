"""Original BounceClass::Update 0x00439B00 flights of zero-elasticity DBRIS debris.

Each flight starts from a body built by the original AnimClass constructor and
BounceClass::Init (tools/spatial_oracle/anim_bouncer_launch.py `launch`: retail
DBRIS1LG or DBRIS1SM, a Scenario seed, constructor coordinate = cell centre at
ground + 20, so Init's start is ground + 30), then calls the original Update once
per tick until it returns 2 (Stopped) or 400 ticks pass. After every tick the
original BounceClass coordinate truncation 0x004399A0 reads the position.

Update's result codes, from 0x00439D3F/0x0043A061/0x0043A098: 0 = no ground
contact this tick, 1 = contact (the bounce arm ran), 2 = the 0x00439A10 energy
estimate is below 2.5 (0x007E3D80), whether or not there was contact.

Terrain uses the same original map queries as bounce_height.py: MapClass cell
lookup 0x00565730 over a supplied 512-wide cell table, floor height 0x00578080,
the Bounce deck constant from 0x00439610, the retail slope matrices. Terrains:
`flat0` (all level 0), `flat2` (all level 2), `mesa` (start on a 3x3 level-4
plateau surrounded by level 0; a 4-level step with no ramp cells) and `pit`
(start in a 5x5 level-0 floor surrounded by level 4). Cells outside the supplied
25x25 block resolve to the retail dummy cell 0x00ABDC50 at level 0.

Row schema: `ticks[i]` = [Update result, truncated x, y, z (0x004399A0), position
f32 bits x3 (+0x18), velocity f32 bits x3 (+0x24)] after tick i+1;
`first_result1_tick`/`stop_tick` are 1-based tick numbers or null.

Rust consumer: src/sim/bounce.rs (Update) through src/sim/anim_class.rs.
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_FPCW

from tools.native_oracle import NATIVE_FPCW, finish_vectors, load_image, provenance, run_checked
from tools.spatial_oracle.anim_bouncer_launch import RETAIL, execute as launch
from tools.spatial_oracle.bounce_height import matrices

UPDATE, TRUNCATE, DECK_INIT = 0x439B00, 0x4399A0, 0x439610
MEM = 0x21000000
TABLE = MEM                 # 512 * 512 cell pointers
CELLS = MEM + 0x100000      # 0x200 bytes per cell
BODY = MEM + 0x1F0000
OUT = BODY + 0x100
SP = MEM + 0x3FF000
STOP = 0x30000000
DUMMY = 0xABDC50
CENTER = 100                # start cell (100, 100)
RADIUS = 12
MAX_TICKS = 400


def words(*values):
    return struct.pack("<" + "I" * len(values), *(v & 0xFFFFFFFF for v in values))


def level_at(terrain, x, y):
    ring = max(abs(x - CENTER), abs(y - CENTER))
    return {"flat0": 0, "flat2": 2, "mesa": 4 if ring <= 1 else 0,
            "pit": 0 if ring <= 2 else 4}[terrain]


def machine(terrain):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(MEM, 0x400000)
    uc.mem_map(STOP, 0x1000)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    uc.mem_write(0x822D80, struct.pack("<H", NATIVE_FPCW))
    uc.mem_write(0x87F924, words(TABLE, 0x40000))
    uc.mem_write(0x89E7C0, words(104))
    uc.mem_write(0x89C778, words(104))
    uc.mem_write(SP, words(STOP))
    uc.reg_write(UC_X86_REG_ESP, SP)
    run_checked(uc, DECK_INIT, STOP)
    assert struct.unpack("<i", uc.mem_read(0x89C76C, 4))[0] == 416
    for slope, matrix in enumerate(matrices()):
        uc.mem_write(0xB45188 + 48 * slope, struct.pack("<12I", *matrix))
    uc.mem_write(DUMMY, bytes(0x200))
    uc.mem_write(DUMMY + 0x44, words(-1))
    uc.mem_write(0xA8E9A0, b"\x01")
    index = 0
    for y in range(CENTER - RADIUS, CENTER + RADIUS + 1):
        for x in range(CENTER - RADIUS, CENTER + RADIUS + 1):
            cell = CELLS + index * 0x200
            index += 1
            uc.mem_write(cell + 0x24, struct.pack("<hh", x, y))
            uc.mem_write(cell + 0x44, words(-1))
            uc.mem_write(cell + 0x11B, bytes([level_at(terrain, x, y), 0]))
            uc.mem_write(TABLE + (y * 512 + x) * 4, words(cell))
    return uc


def call(uc, entry, *args):
    uc.mem_write(SP, words(STOP, *args))
    uc.reg_write(UC_X86_REG_ESP, SP)
    uc.reg_write(UC_X86_REG_ECX, BODY)
    run_checked(uc, entry, STOP, count=1_000_000)
    return uc.reg_read(UC_X86_REG_EAX)


def fly(case):
    level = level_at(case["terrain"], CENTER, CENTER)
    coord = [CENTER * 256 + 128, CENTER * 256 + 128, level * 104 + 20]
    elasticity, max_xy, min_z, rate = RETAIL[case["type"]]
    launched = launch(dict(kind="ctor", type=case["type"], elasticity=elasticity, max_xy=max_xy,
                           min_z=min_z, random_rate=list(rate), coord=coord, seed=case["seed"]))
    body = bytes.fromhex(launched["bounce_hex"])
    uc = machine(case["terrain"])
    uc.mem_write(BODY, body)
    ticks = []
    for _ in range(MAX_TICKS):
        result = call(uc, UPDATE)
        call(uc, TRUNCATE, OUT)
        ticks.append([result, *struct.unpack("<iii", uc.mem_read(OUT, 12)),
                      *struct.unpack("<6I", uc.mem_read(BODY + 0x18, 24))])
        if result == 2:
            break
    return dict(input=case, launch_coord=coord, launch_body_hex=body.hex(),
                velocity_draws=launched["velocity_draws"], ticks=ticks,
                final_body_hex=bytes(uc.mem_read(BODY, 0x50)).hex(),
                first_result1_tick=next((i + 1 for i, t in enumerate(ticks) if t[0] == 1), None),
                stop_tick=next((i + 1 for i, t in enumerate(ticks) if t[0] == 2), None))


SEEDS = [1, 7, 31, 0x5CA1AB1E, 0xFFFFFFFF, 0x12345678, 42, 1000]


def cases():
    for terrain in ("flat0", "flat2", "mesa", "pit"):
        for kind in ("DBRIS1LG", "DBRIS1SM"):
            for seed in SEEDS:
                yield dict(terrain=terrain, type=kind, seed=seed)


if __name__ == "__main__":
    finish_vectors(lambda: [fly(case) for case in cases()], Path(__file__).with_suffix(".json"),
        provenance=lambda: provenance(
            scope=("BounceClass::Update 0x00439B00 per tick, with the 0x004399A0 truncation, "
                   "from zero-elasticity DBRIS1LG/DBRIS1SM launches (original constructor and "
                   "Init, 8 seeds each) over flat level-0, flat level-2, a level-4 mesa and a "
                   "level-4-walled pit, until result 2 or 400 ticks."),
            assumptions=[
                "x87 control word 0x0E7F; 0x00822D80 holds 0x0E7F for ftol 0x007C5F00.",
                "Supplied cell scalar 104 (0x0089E7C0/0x0089C778); original 0x00439610 derives "
                "the deck height 416.",
                "Cells are flat (slope 0), no overlay, no objects, no bridge flags; steps are "
                "bare 4-level level changes without ramp cells.",
                "Launch bodies come from anim_bouncer_launch.py's constructor execution and "
                "carry its assumptions and substitutions.",
                "Update runs back to back; AnimClass::AI 0x00423930's result handling (bounce "
                "anim, cell damage, vt+0xF8 on 2) is not executed.",
            ],
            substitutions=[],
            entry_points={"update": UPDATE, "truncate": TRUNCATE, "deck_init": DECK_INIT}))
