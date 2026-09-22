"""Original Unit Scatter's unconditional state refusals, before cell search.

Both flags are true so mission Scatter and an existing NavCom do not refuse.
Runs real Unit readers, FacingClass, and constructed Drive/Teleport COM objects.
Accepted rows stop before the NULL/source split; this is not destination parity.
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import (
    SCRATCH, STACK_BASE, STACK_SIZE, RET_MAGIC, load_image, run_checked,
    finish_vectors, provenance,
)
from tools.spatial_oracle.map_queries import dwords

ACTOR, TYPE, LOCO, SOURCE = [SCRATCH + i * 0x2000 for i in range(4)]
SP = STACK_BASE + STACK_SIZE - 0x1000


def make_fixture(case):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)

    def read32(a):
        return struct.unpack('<I', u.mem_read(a, 4))[0]

    def interlocked(_u, address, _size, _data):
        if address not in imports:
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        pointer = read32(sp + 4)
        value = read32(pointer) + imports[address]
        u.mem_write(pointer, dwords(value))
        u.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 8)

    imports = {read32(0x7E11C8): 1, read32(0x7E11CC): -1}
    u.hook_add(UC_HOOK_CODE, interlocked)

    def call(entry, this, args, stop=RET_MAGIC):
        u.mem_write(SP, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ECX, this)
        u.reg_write(UC_X86_REG_ESP, SP)
        return run_checked(u, entry, stop, count=300000, required_addresses=[entry])

    frame = case.get('frame', 100)
    u.mem_write(0xA8ED84, dwords(frame))
    call(0x718000 if case.get('teleport') else 0x4AF540, LOCO, [])
    u.mem_write(LOCO + 0xC, dwords(ACTOR))
    u.mem_write(LOCO + 0x14, dwords(1))
    u.mem_write(LOCO + 0x10, bytes([not case.get('power_off', False)]))
    u.mem_write(ACTOR, dwords(0x7F5C70))
    u.mem_write(ACTOR + 0x6C4, dwords(TYPE))
    u.mem_write(ACTOR + 0x674, dwords(LOCO + 4))
    u.mem_write(ACTOR + 0xAC, dwords(case.get('mission', 5)))
    u.mem_write(ACTOR + 0xB4, dwords(case.get('queued', -1)))
    # Train is absent in retail rules and in VERA's parsed ObjectType. It is
    # fixed false here, not an asserted port of the unused IsTrain extension.
    u.mem_write(TYPE + 0xC94, b'\0')
    for i, name in enumerate(('deployed', 'deploying', 'undeploying')):
        u.mem_write(ACTOR + 0x6E0 + i, bytes([case.get(name, False)]))
    u.mem_write(ACTOR + 0x5A4, dwords(TYPE if case.get('nav') else 0))
    # Original Facing constructor/SetROT and Set. All timer answers come from
    # the real4C9480 call inside Scatter, including elapsed and paused epochs.
    call(0x4C91C0, ACTOR + 0x388, [])
    call(0x4C9680, ACTOR + 0x388, [case.get('rot', 5)])
    if case.get('turn'):
        u.mem_write(SOURCE, struct.pack('<H', 0x4000))
        call(0x4C9220, ACTOR + 0x388, [SOURCE])
    u.mem_write(0xA8ED84, dwords((frame + case.get('elapsed', 0)) & 0xFFFFFFFF))
    # Set all current mission Scatter bytes false: the first flag bypasses
    # this policy, but cannot bypass the unconditional Sleep/Sticky/Unload gate.
    u.mem_write(0xA8E3A8 - 32 + 9, b'\0')
    for i in range(32):
        u.mem_write(0xA8E3A8 + i * 32 + 9, b'\0')
    u.mem_write(SOURCE, dwords(0, 0, 0))
    return u, call, read32


def query(case):
    u, call, read32 = make_fixture(case)
    before = bytes(u.mem_read(ACTOR, 0x700))
    stop = call(0x743A50, ACTOR, [SOURCE, 1, 1], (0x743BAC, RET_MAGIC))
    assert bytes(u.mem_read(ACTOR, 0x700)) == before
    admitted = stop == 0x743BAC
    assert u.reg_read(UC_X86_REG_ESP) == SP + (-88 if admitted else 16)
    assert read32(LOCO + 0x14) == (2 if admitted else 1)
    return dict(input=case, admitted=admitted)


def generate():
    cases = [dict(mission=m) for m in range(-1, 32)]
    cases += [dict(mission=-1, queued=m) for m in (-1, 0, 1, 5, 6, 16, 23)]
    cases += [dict(teleport=True), dict(power_off=True), dict(nav=True)]
    cases += [dict(**{name: True}) for name in ('deployed', 'deploying', 'undeploying')]
    cases += [dict(turn=True, rot=rot, elapsed=elapsed)
              for rot in (0, 1, 5, 127, -1) for elapsed in (0, 1, 12, 64)]
    cases += [dict(turn=True, frame=0xFFFFFFFF, elapsed=elapsed) for elapsed in (0, 1, 12)]
    return [query(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Unit743A50 entry through the unconditional state gates. Both force flags true; accepted rows stop at743BAC before source selection. No FNPC, entry, destination, or complete scatter parity claim.',
        entry_points={'scatter': 0x743A50, 'eligible': 0x6F3280,
                      'facing_rotating': 0x4C9480, 'powered': 0x55A930,
                      'drive_constructor': 0x4AF540, 'teleport_constructor': 0x718000},
        assumptions=['Original Unit vtable, real Drive/Teleport constructors and COM QueryInterface/GetClassID/refcount calls. Supplied valid owner/type state; IsTrain=false as stock retail rules.',
                     'All three deploy-family bytes are separate supplied native prestates; their animation producers are not executed.',
                     'Current mission -1..31, queued fallbacks, original Facing SetROT/Set and time progression. Current MissionControl Scatter=false and optional existing NavCom are bypassed only by literal true flags.',
                     'Accepted stop retains the temporary IPersist reference; refusals return through native Release. Actor memory remains unchanged.'],
        substitutions=['Only OS InterlockedIncrement/Decrement imports implement pointed integer increments/decrements. No gameplay callable is substituted.']))
