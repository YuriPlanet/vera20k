"""Original Techno704E40 mind-control curve, projection, clipping and RGB565 stores.

Reuses Rally's original memory-surface/map arena. Coordinates, House RGB and
OS GetTickCount are supplied inputs; CaptureManager selection/FLH/lifecycle and
Tactical admission are outside these pixel comparisons. No gameplay bytepatch.
"""
from pathlib import Path
import struct
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP
from tools.procedural_drawing_oracle import rally
from tools.spatial_oracle.bridge_damage_admission import words, read32, SP
from tools.native_oracle import RET_MAGIC, run_checked, finish_vectors, provenance


def inputs():
    base = dict(source=[5248,5248,0], target=[5760,5248,70], rgb=[248,40,8],
                camera=[-80,515], clip=[0,0,160,120], wall_ms=0)
    for ms in (0,16,240,496,512,0xFFFFFFF0):
        yield dict(base, name=f'clock_{ms}', wall_ms=ms)
    yield dict(base, name='reverse', source=base['target'], target=base['source'])
    yield dict(base, name='coincident', target=base['source'])
    yield dict(base, name='higher_target', target=[5760,5248,416])
    yield dict(base, name='lower_source', source=[5248,5248,-104])
    yield dict(base, name='interior_clip', clip=[30,35,95,65])
    yield dict(base, name='cross_clip', camera=[0,515])
    yield dict(base, name='negative_world', source=[-384,128,0], target=[128,384,70], camera=[-100,-80])
    yield dict(base, name='black', rgb=[0,0,0])
    yield dict(base, name='white', rgb=[255,255,255], wall_ms=64)


def execute(case):
    m = rally.Rally(case)
    u = m.u
    u.mem_write(0x88731C, words(rally.SURFACE))
    clock = RET_MAGIC + 0x100
    u.mem_write(0x7E1530, words(clock))
    code = bytes(u.mem_read(0x704E40, 0x421))
    draws = []
    def observe(_u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == clock:
            u.reg_write(UC_X86_REG_EAX, case['wall_ms'])
            u.reg_write(UC_X86_REG_EIP, read32(u,sp))
            u.reg_write(UC_X86_REG_ESP, sp+4)
        elif pc in (0x65C780, 0x65C7E0):
            raise AssertionError('relationship drawing reached RNG')
        elif pc == 0x7BA5E0:
            args = struct.unpack('<III', u.mem_read(sp+4,12))
            points = [list(struct.unpack('<ii',u.mem_read(p,8))) for p in args[:2]]
            draws.append(dict(points=points, word=args[2]))
    u.hook_add(UC_HOOK_CODE, observe)
    rgb = struct.unpack('<I',bytes([*case['rgb'],0]))[0]
    args = [*case['source'], *case['target'], rgb]
    u.mem_write(SP, words(RET_MAGIC,*args))
    u.reg_write(UC_X86_REG_ESP,SP)
    u.reg_write(UC_X86_REG_ECX,rally.BUILDING)
    run_checked(u,0x704E40,RET_MAGIC,count=2_000_000,required_addresses=(0x704E40,0x6D2140))
    assert u.reg_read(UC_X86_REG_ESP)==SP+32
    assert bytes(u.mem_read(0x704E40,len(code)))==code
    return dict(input=case, draws=draws, pixels=m.pixels())


def metadata():
    return provenance(scope='Original Techno704E40 curve pixels with supplied endpoints/RGB/clock',
        assumptions=['Original code executes geometry, RGB6612C0 adjustment, projection6D2140, clip7BC2B0, endpoint fills and solid lines; FPCW0E7F, RGB565 and160x120 surface.',
            'World endpoints, RGB and Camera are explicit fixture inputs, not VERA-derived goldens. CaptureManager selection/timer/House creation and Tactical visibility are excluded.',
            'get_flh composes the existing ifv_fire_coord spawn_launch owner, with five supplied alternate triples in native Type+85C records, indices-1..-6 and burst0/1. Original GetFLH and Ship DrawMatrix execute; physical ART/RULES strings come from production asset extraction. Source pose/lifecycle and alternate data are prepared inputs; no complete Scenario or controller-class override replay is claimed.'],
        substitutions=['Existing Rally memory-surface adapter replaces DirectDraw storage/lock/pitch only; original BSurface opaque draw bodies execute.',
            'OS GetTickCount is supplied at its imported target. RNG calls fail. Gameplay .text bytes are unchanged.'],
        entry_points={'curve':0x704E40,'projection':0x6D2140,'rgb_adjust':0x6612C0,'clip':0x7BC2B0,'solid':0x7BA5E0})


def vectors():
    from tools.projectile_oracle.ifv_fire_coord import spawn_launch
    flh = spawn_launch(include_negative_flh=True)
    return dict(size=[160,120], background_word=rally.BACKGROUND,
        cases=[execute(case) for case in inputs()],
        get_flh=dict(supplied=flh['supplied'], layers=flh['layers'],
            cases=[row for row in flh['get_flh'] if 'alternate_flh' in row]))


if __name__=='__main__':
    finish_vectors(vectors, Path(__file__).with_suffix('.json'),
        provenance=metadata)
