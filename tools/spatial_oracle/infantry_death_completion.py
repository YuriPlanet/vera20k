"""Original InfantryClass sequence completion 0x00520AE0 for its death arms.

Executes the whole body for an infantryman whose Doing (+0x6C4) is a death
sequence and records which corpse anim it builds, with what arguments, and the
Scenario RNG around it:
- Die1..Die5 (0xB..0xF) with the stage (+0xF8) at or past the DoControls
  count leave `DeadBodies=` (the type's list at +0xE50, else Rules+0x124
  unless NotHuman +0xEAD) through one raw Random() % count, then UnInit;
- below the count nothing happens;
- WetDie (0x14/0x15) UnInits with no corpse.

Supplied: the InfantryType (DoControls array +0xE3C, DeadBodies vector
+0xE50/+0xE54/+0xE60, NotHuman), Rules DeadBodies (+0x124/+0x130), and machine
stubs for operator new 0x7C8E17 (a buffer, or NULL), the AnimClass constructor
0x421EA0, and the vtable slots GetCoords (+0x48), UnInit (+0xF8) and Do_Action
(+0x558). The original Random 0x65C780 executes on a seeded Scenario RNG.
DoControls sound entries (+0x10) are 0, so the tail plays nothing.

Rust consumer: src/sim/world/infantry_terminal.rs (Simulation::leave_dead_body).
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import (RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors,
                                 load_image, provenance, run_checked)
from tools.spatial_oracle.map_queries import dwords

ENTRY, INFANTRY_VTABLE = 0x520AE0, 0x7EB058
NEW, ANIM_CTOR, SEED, RANDOM = 0x7C8E17, 0x421EA0, 0x65C6D0, 0x65C780
OWNER, VTABLE, TYPE, CONTROLS, TYPE_BODIES, RULES, RULE_BODIES, SCENARIO, STUBS, ANIM = (
    SCRATCH + n * 0x1000 for n in range(10))
SP = STACK_BASE + STACK_SIZE - 0x1000
COORD = (0x1480, 0x2A40, 0x1A0)
TYPE_ANIM = [0x2F00 + n for n in range(4)]
RULE_ANIM = [0x2E00 + n for n in range(6)]


def execute(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(RET_MAGIC, 0x1000)
    u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
    events = []

    u.mem_write(VTABLE, bytes(u.mem_read(INFANTRY_VTABLE, 0x600)))
    stubs = {}
    for n, (slot, name, pop) in enumerate(((0x48, 'get_coords', 4), (0xF8, 'uninit', 0),
                                           (0x558, 'do_action', 12))):
        address = STUBS + n * 0x80
        body = b''
        if name == 'get_coords':
            # MOV EAX,[ESP+4]; MOV [EAX],x; MOV [EAX+4],y; MOV [EAX+8],z
            body += b'\x8B\x44\x24\x04'
            for k, value in enumerate(COORD):
                body += b'\xC7\x40' + bytes([4 * k]) + dwords(value)
        body += (b'\xC2' + struct.pack('<H', pop)) if pop else b'\xC3'
        u.mem_write(address, body)
        u.mem_write(VTABLE + slot, dwords(address))
        stubs[address] = name

    u.mem_write(OWNER, dwords(VTABLE))
    u.mem_write(OWNER + 0x6C0, dwords(TYPE))
    u.mem_write(OWNER + 0x6C4, dwords(row['doing']))
    u.mem_write(OWNER + 0xF8, dwords(row['stage']))
    u.mem_write(TYPE + 0xE3C, dwords(CONTROLS))
    for doing in range(42):
        u.mem_write(CONTROLS + doing * 36, dwords(0, row['count'], 0, -1, 0))
    type_bodies = TYPE_ANIM[:row['type_bodies']]
    u.mem_write(TYPE + 0xE54, dwords(TYPE_BODIES))
    u.mem_write(TYPE + 0xE60, dwords(len(type_bodies)))
    u.mem_write(TYPE_BODIES, dwords(*type_bodies) if type_bodies else b'')
    u.mem_write(TYPE + 0xEAD, bytes([row['not_human']]))
    u.mem_write(0x8871E0, dwords(RULES))
    u.mem_write(RULES + 0x124, dwords(RULE_BODIES))
    u.mem_write(RULES + 0x130, dwords(len(RULE_ANIM)))
    u.mem_write(RULE_BODIES, dwords(*RULE_ANIM))
    u.mem_write(0xA8B230, dwords(SCENARIO))

    u.mem_write(SP, dwords(RET_MAGIC, row['seed']))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, SCENARIO + 0x218)
    run_checked(u, SEED, RET_MAGIC, count=100000)
    before = bytes(u.mem_read(SCENARIO + 0x218, 0x3F4)).hex()

    def observe(uc, address, _size, _data):
        if address in stubs:
            events.append(dict(call=stubs[address]))
        elif address == RANDOM:
            events.append(dict(call='random'))

    hook = u.hook_add(UC_HOOK_CODE, observe)
    u.mem_write(SP, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, OWNER)
    current = ENTRY
    for _ in range(4):
        boundary = run_checked(u, current, (RET_MAGIC, NEW, ANIM_CTOR), count=200000)
        if boundary == RET_MAGIC:
            break
        sp = u.reg_read(UC_X86_REG_ESP)
        back = struct.unpack('<I', u.mem_read(sp, 4))[0]
        args = struct.unpack('<7I', u.mem_read(sp + 4, 28))
        if boundary == NEW:
            events.append(dict(call='new', size=args[0]))
            u.reg_write(UC_X86_REG_EAX, ANIM if row['alloc'] else 0)
            u.reg_write(UC_X86_REG_ESP, sp + 4)  # cdecl: the caller pops
        else:
            coord = list(struct.unpack('<3i', u.mem_read(args[1], 12)))
            events.append(dict(call='anim', this=u.reg_read(UC_X86_REG_ECX) == ANIM,
                               anim=args[0], coord=coord, rest=list(args[2:])))
            u.reg_write(UC_X86_REG_EAX, ANIM)
            u.reg_write(UC_X86_REG_ESP, sp + 4 + 28)  # thiscall: 7 arguments
        current = back
    else:
        raise AssertionError('unexpected repeated direct calls')
    u.hook_del(hook)
    assert u.reg_read(UC_X86_REG_ESP) == SP + 4
    return dict(input=row, events=events, rng_before=before,
                rng_after=bytes(u.mem_read(SCENARIO + 0x218, 0x3F4)).hex())


def generate():
    rows = []
    for doing in (0xB, 0xC, 0xF):
        for type_bodies, not_human in ((0, 0), (0, 1), (1, 0), (3, 1), (4, 0)):
            for seed in (1, 31, 0x5CA1AB1E, 0xFFFFFFFF):
                rows.append(dict(doing=doing, stage=15, count=15, type_bodies=type_bodies,
                                 not_human=not_human, seed=seed, alloc=1))
    for stage, count in ((14, 15), (16, 15), (0, 0), (0, 1), (-1, 15)):
        rows.append(dict(doing=0xB, stage=stage, count=count, type_bodies=0, not_human=0,
                         seed=7, alloc=1))
    rows.append(dict(doing=0xB, stage=15, count=15, type_bodies=0, not_human=0, seed=7, alloc=0))
    for doing in (0x14, 0x15):
        rows.append(dict(doing=doing, stage=15, count=15, type_bodies=2, not_human=0, seed=7,
                         alloc=1))
    return [execute(row) for row in rows]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope=('InfantryClass sequence completion 0x00520AE0 for Die1/Die2/Die5 and WetDie1/2 '
               'Doings: stage against the DoControls count, the corpse pick from the type or '
               'Rules DeadBodies (NotHuman), the Scenario Random() draw, the AnimClass '
               'constructor arguments and UnInit.'),
        assumptions=['x87 control word 0x0E7F, the harness default.',
                     'DoControls sound entries are 0, so the post-switch tail plays no sound.',
                     'The InfantryType and Rules vectors are supplied; their readers are not run.'],
        substitutions=['operator new 0x7C8E17 returns a buffer or NULL; the AnimClass '
                       'constructor 0x421EA0 records its arguments; GetCoords, UnInit and '
                       'Do_Action are vtable stubs.'],
        entry_points={'completion': ENTRY},
    ))
