"""Original Aircraft AI sinking suffix after the Foot AI Alive check.

Prepared retail ORCA/type/flat-cell state reuses magnetron_aircraft_lift.
No Chronosphere selection, warp, full Foot AI or whole-game execution is
claimed. SetLocation, GetHeight, Scenario RandomRanged and native argument
preparation execute; Anim construction and RecordKill/UnInit are boundaries.
"""
from pathlib import Path
import struct
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
from tools.native_oracle import run_checked, finish_vectors, provenance
from tools.spatial_oracle.magnetron_aircraft_lift import Fixture
from tools.spatial_oracle.unit_scatter_state import ACTOR, SP
from tools.spatial_oracle.unit_source_scatter import SCENARIO, CELLS
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.building_body_rules import RULES
from tools.projectile_oracle.bridge_render_inputs import lexical

HERE = Path(__file__).resolve().parent
ENTRY, END = 0x414DA8, 0x414ED0


def execute(case):
    f = Fixture(dict(z=case['relative_z'] + case.get('floor', 0)))
    m, u = f.m, f.u
    # The fixture is a prepared object, not a new native scenario spawn.
    # Unmarked controls isolate the shared SetLocation coordinate tail.
    u.mem_write(ACTOR + 0x74, b'\0')
    u.mem_write(ACTOR + 0x90, bytes([case.get('alive', 1)]))
    u.mem_write(ACTOR + 0x3CD, bytes([case.get('sinking', 1)]))
    u.mem_write(CELLS + (10 * 32 + 10) * 0x200 + 0x11B,
                bytes([case.get('floor', 0) // 104, 0]))
    u.mem_write(CELLS + (10 * 32 + 10) * 0x200 + 0x140, dwords(0x100))
    # Same supplied Cell level scalar as the shared naval/crash fixtures.
    # Assert that the native Cell height cache has not sampled it yet.
    assert not (u.mem_read(0x89E770, 1)[0] & 3)
    u.mem_write(0x89E7C0, dwords(104))
    assert m.invoke(0x5F5F40, ACTOR) == (case['relative_z'] & 0xFFFFFFFF)
    sections, _ = lexical(Path('ini/RULESMD.INI').read_bytes(), {'General'})
    m.rules_cache({'General': {'Wake': sections['General']['Wake']}})
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ESI, m.rules)
    u.reg_write(UC_X86_REG_EDI, RULES)
    run_checked(u, 0x66D847, 0x66D894)
    wake = m.read32(m.rules + 0x94)
    assert m.string(wake + 0x24) == 'WAKE1'
    rngs = {'main': 0x886B88, 'scenario': SCENARIO + 0x218, 'mapgen': 0xABE890}
    for p in rngs.values():
        m.invoke(0x65C6D0, p, (case.get('seed', 1),))
    before = {n: bytes(u.mem_read(p, 0x3F4)).hex() for n, p in rngs.items()}
    u.mem_write(0xA8ED84, dwords(case['frame']))
    events = []

    def observe(u, pc, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == 0x65C7E0:
            assert u.reg_read(UC_X86_REG_ECX) == rngs['scenario']
            events.append(dict(kind='range', bounds=list(struct.unpack('<2i', u.mem_read(sp + 4, 8)))))
        elif pc == 0x65C87C:
            raw = u.reg_read(UC_X86_REG_ESI)
            candidate = raw & u.reg_read(UC_X86_REG_EBP)
            events.append(dict(kind='raw', raw=raw, rejected=candidate > u.reg_read(UC_X86_REG_EDI)))
        elif pc == 0x421EA0:
            args = struct.unpack('<7I', u.mem_read(sp + 4, 28))
            events.append(dict(kind='wake', type=m.string(args[0] + 0x24),
                               coord=list(struct.unpack('<3i', u.mem_read(args[1], 12))),
                               args=list(args[2:])))
        elif pc == 0x702D40:
            events.append(dict(kind='record_kill', attacker=m.read32(sp + 4)))
    u.hook_add(UC_HOOK_CODE, observe)
    code = bytes(u.mem_read(ENTRY, END - ENTRY))
    u.reg_write(UC_X86_REG_ESI, ACTOR)
    u.reg_write(UC_X86_REG_ESP, SP)
    endpoint = run_checked(u, ENTRY, (END, 0x4151B0, 0x421EA0, 0x702D40),
                           required_addresses=(ENTRY,), count=300000)
    assert bytes(u.mem_read(ENTRY, END - ENTRY)) == code
    f.assert_immutable()
    # Alternate endpoints pass through the observer before run_checked stops;
    # no boundary body or fabricated return is executed.
    return dict(input=case, output=dict(endpoint=hex(endpoint),
        coord=list(struct.unpack('<3i', u.mem_read(ACTOR + 0x9C, 12))), events=events,
        rng={n: dict(before=before[n], after=bytes(u.mem_read(p, 0x3F4)).hex()) for n, p in rngs.items()}))


def cases():
    for frame in range(1000, 1004):
        yield dict(name=f'surface_{frame}', relative_z=0, frame=frame)
    for z in (-394, -395, -396, -400):
        for frame in (1000, 1001):
            yield dict(name=f'threshold_{z}_{frame}', relative_z=z, floor=208, frame=frame)
    for seed in (0, 0xFFFFFFFF):
        yield dict(name=f'seed_{seed}', relative_z=-100, frame=1000, seed=seed)
    yield dict(name='not_sinking', relative_z=0, frame=1000, sinking=0)
    yield dict(name='not_alive', relative_z=0, frame=1000, alive=0)


def generate():
    return dict(schema_version=1, cases=[execute(c) for c in cases()])


def metadata():
    return provenance(scope=__doc__, assumptions=[
        'Prepared unmarked Aircraft actor and original-read ORCA type, flat level0/2 cells, supplied Cell height scalar104. Each input height is independently checked through original GetHeight before the measured suffix. Actor is not produced by a native Chronosphere history.',
        'Physical RULESMD General Wake executes original66D847..66D894. Existing LiftInputs retains original selected type readers and supplies optional asset/cache/CRT/COM transport boundaries.',
        'Entry414DA8 supplies the post-FootAI stack/register frame. Frozen prologue, Foot AI, marked relocation and suffix after414ED0 are outside this corpus.',
        'Native Aircraft vtable SetLocation4DB810 and GetHeight5F5F40 run; full RNG states compare all three streams. Wake allocation uses the inherited bump allocator; Anim421EA0 stops at its entry.',
        'Terminal rows stop at real Aircraft vtable RecordKill702D40 entry with NULL attacker. RecordKill, UnInit, deferred cleanup, rendering and animation lifetime are separate production dependencies, not native executions here.'
    ], substitutions=['Inherited fixture setup and operator_new bump allocation; no sinking predicate, lowering, height or RNG result substituted.'],
        entry_points={'aircraft_suffix': ENTRY, 'end': END, 'set_location': 0x4DB810,
                      'get_height': 0x5F5F40, 'range': 0x65C7E0,
                      'anim_boundary': 0x421EA0, 'record_kill_boundary': 0x702D40})


if __name__ == '__main__':
    finish_vectors(generate, HERE / 'aircraft_sink_tick.json', provenance=metadata)
