"""Bounded FV Cell range/FireError composition over frozen native Anytown facts.

Read-only research for the next chain. Runtime range, Cell getters, map queries,
distance arithmetic, GetWeapon, line-of-fire and Unit GetFireError execute the
original image. Unit placement, sparse Cell projection and selected layer joining
are explicit inputs, not native Scenario/Unit lifecycle proof.
"""
from pathlib import Path
import gzip
import hashlib
import json
import struct

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import RET_MAGIC, run_checked, provenance
from tools.spatial_oracle import bridge_target_composed as composed
from tools.spatial_oracle import mapgen_range
from tools.spatial_oracle.building_body_rules import SP, dwords
from tools.projectile_oracle.bridge_render_inputs import assets_root, lexical

HERE = Path(__file__).resolve().parent
REPO = Path(__import__('tools.native_oracle', fromlist=['x']).__file__).resolve().parents[1]
FACTS = REPO / 'tools/spatial_oracle/anytown_damage/navigation.json.gz'
from tools.spatial_oracle.anytown_damage.inputs import ASSETS
from .publication import finish_vectors
MAP = ASSETS / 'XMP03T4.MAP'
THEATER = 0xAA0738
TEXT_START, TEXT_SIZE = 0x401000, 0x3E0000


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def state(m, p):
    return bytes(m.u.mem_read(p, 1012)).hex()


def setup(stage):
    m, source, unused_target, typ, weapon, inherited_cells, rules, inputs = composed.setup()
    u = m.u
    bootstrap = mapgen_range.Machine.startup(m)
    packet = json.loads(gzip.decompress(FACTS.read_bytes()))
    frozen = packet['initial'] if stage == 'healthy' else packet['stages'][{'damaged': 0, 'collapsed': 1, 'repaired': 2}[stage]]['state']
    facts = {tuple(c['coord']): c for c in frozen['cells']}
    overlays = {tuple(c[:2]): c[5] for c in packet['case']['cells']}
    if stage != 'healthy':
        for step in packet['stages'][:{'damaged': 1, 'collapsed': 2, 'repaired': 3}[stage]]:
            for event in step['trace']:
                if event['kind'] == 'overlay':
                    overlays[tuple(event['coord'])] = event['value']
    table = m.read32(0x87F7E8 + 0x13C)
    # Physical facts are produced by the independently replayed full native
    # constructor/reader/Recalc packet. Recreate only the reached vertical crop.
    cells = {}
    for y in range(46, 57):
        for x in range(86, 89):
            f = facts[x, y]
            cell = m.alloc(0x200)
            m.invoke(0x47BBF0, cell)
            u.mem_write(cell + 0x24, struct.pack('<hh', x, y))
            u.mem_write(cell + 0x38, dwords(f['tile']))
            u.mem_write(cell + 0x44, dwords(overlays[x, y] if overlays[x, y] is not None else -1))
            u.mem_write(cell + 0x4C, dwords(f['zone_type']))
            u.mem_write(cell + 0xEC, dwords(f['land']))
            u.mem_write(cell + 0x11A, bytes((f['subtile'], f['level'], f['slope'])))
            u.mem_write(cell + 0x140, dwords(f['flags']))
            u.mem_write(table + (y * 512 + x) * 4, dwords(cell))
            cells[x, y] = cell
    water = packet['theater']['globals'][str(THEATER)]
    u.mem_write(THEATER, dwords(water))
    u.mem_write(0x87F8DC, dwords(*packet['case']['size']))
    # Physical Hills and Anytown contain none of these type/weapon sections;
    # their joined relevant inputs therefore use the same retained native reads.
    map_sections = {}
    for name, path in [('prepared_Hills', assets_root() / 'Hills.map'), ('Anytown', MAP)]:
        raw = path.read_bytes()
        sections, _ = lexical(raw, {'FV', 'HoverMissile', 'AAHeatSeeker2', 'HE'})
        assert not sections, (name, sections)
        map_sections[name] = dict(sha256=sha(raw), relevant_sections=sections)
    return m, source, typ, weapon, cells, inputs, dict(
        source_packet_sha256=sha(FACTS.read_bytes()), stage=stage, water_base=water,
        cells=[dict(facts=facts[xy], overlay=overlays[xy]) for xy in cells],
        map_layer_join=map_sections, startup=bootstrap)


def execute(row):
    m, source, typ, weapon, cells, inputs, projection = setup(row.get('stage', 'healthy'))
    u = m.u
    target_xy = tuple(row.get('target', [87, 54]))
    target = cells[target_xy]
    source_xy = tuple(row.get('source_cell', [87, 48]))
    source_out = m.alloc(12)
    m.invoke(0x486840, cells[source_xy], (source_out,))
    source_xyz = composed.coord(m, source_out)
    source_xyz[1] += row.get('source_y_delta', 0)
    if 'source_z' in row:
        source_xyz[2] = row['source_z']
    u.mem_write(source + 0x9C, dwords(*source_xyz))
    u.mem_write(source + 0x8C, bytes((row.get('source_on_bridge', 0),)))
    u.mem_write(source + 0x2B4, dwords(target))
    if 'target_flags' in row:
        u.mem_write(target + 0x140, dwords(row['target_flags']))
    facing = m.alloc(4)
    m.invoke(0x5F3DB0, source, (facing, target))
    for field in (0x388, 0x3A0):
        m.invoke(0x4C9300, source + field, (facing,))
    slot = m.invoke(0x6F3330, source, (target,))
    assert slot == 0
    assert m.read32(m.invoke(0x70E140, source, (slot,))) == weapon
    # Capture actual slot/FPCW/type tuning; no Range or MinimumRange override.
    projectile = m.read32(weapon + 0xA0)
    tuning = dict(source_type=m.string(typ + 0x24), weapon=m.string(weapon + 0x24),
        slot=slot, range=composed.signed(m, weapon + 0xB4), minimum=composed.signed(m, weapon + 0xB8),
        cell_rangefinding=u.mem_read(weapon + 0x134, 1)[0], air_range_bonus=composed.signed(m, typ + 0x68C),
        projectile=m.string(projectile + 0x24), arcing=u.mem_read(projectile + 0x29B, 1)[0],
        subject_to_elevation=u.mem_read(projectile + 0x297, 1)[0],
        source_facing=bytes(u.mem_read(facing, 4)).hex(),
        unit_vslots={hex(offset):f'{m.read32(m.read32(source)+offset):08x}' for offset in (0x48,0x50,0x54,0x3A8,0x3C0,0x53C)},
        cell_vslots={hex(offset):f'{m.read32(m.read32(target)+offset):08x}' for offset in (0x48,0x50,0x54)},
        fpcw=u.reg_read(UC_X86_REG_FPCW))
    scenario = m.read32(0xA8B230)
    rngs = {'main': 0x886B88, 'scenario': scenario + 0x218, 'mapgen': 0xABE890}
    before_rng = {k: state(m, p) for k, p in rngs.items()}
    initial_source = bytes(u.mem_read(source, 0x1000))
    code_hash = sha(bytes(u.mem_read(TEXT_START, TEXT_SIZE)))
    results = []
    for name, entry, args in [('can_fire_at', 0x6F77B0, (target, slot)),
                               ('unit_fire_error', 0x740FD0, (target, slot, 1)),
                               ('unit_fire_error_no_range', 0x740FD0, (target, slot, 0)),
                               ('unit_approach_range_prefix', 0x7414E0, (0,))]:
        u.mem_write(source, initial_source)
        events, writes, pending, geometry, distances = [], [], {}, [], []
        def observe(uc, pc, size, data):
            sp = u.reg_read(UC_X86_REG_ESP)
            if pc in pending:
                for event in pending.pop(pc):
                    event['returned_al'] = u.reg_read(UC_X86_REG_EAX) & 255
            if pc in (0x65C780, 0x65C7E0, 0x598030, 0x7258D0, 0x5F65F0, 0x68BCB0):
                raise AssertionError(('unexpected RNG/detach/identity', hex(pc)))
            if pc in (0x6F77B0, 0x6F7220, 0x6FC0B0, 0x6FCCEE, 0x4CC310, 0x486840, 0x4867E0, 0x5F6B90, 0x5F65A0, 0x7414E0, 0x4D5690, 0x4D5708):
                event = dict(pc=f'{pc:08x}', caller=f'{m.read32(sp):08x}')
                if pc == 0x6F7220:
                    event['source_xyz'] = composed.coord(m, m.read32(sp + 4))
                if pc in (0x486840, 0x4867E0):
                    receiver = u.reg_read(UC_X86_REG_ECX)
                    event['cell'] = list(struct.unpack('<hh', u.mem_read(receiver + 0x24, 4)))
                if pc in (0x4867E0, 0x5F6B90):
                    pending.setdefault(m.read32(sp), []).append(event)
                if pc == 0x4CC310:
                    event['source_xyz'] = composed.coord(m, u.reg_read(UC_X86_REG_ECX))
                    event['target_xyz'] = composed.coord(m, u.reg_read(UC_X86_REG_EDX))
                events.append(event)
            if pc in (0x565730, 0x578080):
                events.append(dict(pc=f'{pc:08x}', caller=f'{m.read32(sp):08x}',
                                   xyz=composed.coord(m, m.read32(sp + 4))))
            if pc == 0x6F7379:
                geometry.append(composed.coord(m, sp + 0x20))
            if pc in (0x6F73E4, 0x6F75F2):
                distances.append(dict(kind='minimum' if pc == 0x6F73E4 else 'maximum',
                    distance=struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0],
                    bound=composed.signed(m, sp + 0x18) if pc == 0x6F73E4 else u.reg_read(UC_X86_REG_EBX)))
        def written(uc, access, address, size, value, data):
            assert address + size <= TEXT_START or address >= TEXT_START + TEXT_SIZE
            if source <= address < source + 0x1000:
                writes.append(dict(offset=address-source, size=size, value=value & ((1 << (size*8))-1)))
        h1 = u.hook_add(UC_HOOK_CODE, observe)
        h2 = u.hook_add(UC_HOOK_MEM_WRITE, written)
        try:
            if name == 'unit_approach_range_prefix':
                u.mem_write(SP, dwords(RET_MAGIC, *args))
                u.reg_write(UC_X86_REG_ESP, SP)
                u.reg_write(UC_X86_REG_ECX, source)
                run_checked(u, entry, 0x4D570E, count=100000,
                            required_addresses=(0x741689,0x4D5690,0x4D5708,0x6F77B0,0x6F7220))
                answer = u.reg_read(UC_X86_REG_EAX)
            else:
                answer = m.invoke(entry, source, args)
        finally:
            u.hook_del(h1)
            u.hook_del(h2)
        after_rng = {k: state(m, p) for k, p in rngs.items()}
        assert before_rng == after_rng
        assert code_hash == sha(bytes(u.mem_read(TEXT_START, TEXT_SIZE)))
        results.append(dict(entry=name,returned=answer & 255 if name in ('can_fire_at','unit_approach_range_prefix') else answer,
                            result_boundary='returned range AL at Foot4D570E, not whole pursuit return' if name == 'unit_approach_range_prefix' else 'function return',
                            target_geometry=geometry,distances=distances,events=events,source_writes=writes,rng_after=after_rng))
    return dict(input=row,source_xyz=source_xyz,tuning=tuning,physical_inputs=inputs,
                projection=projection,results=results,rng_before=before_rng,text_sha256=code_hash)


def generate():
    rows = [dict(name='maximum_exact'),dict(name='maximum_inside_one',source_y_delta=1),
            dict(name='maximum_outside_one',source_y_delta=-1),
            dict(name='maximum_outside_two',source_y_delta=-2),
            dict(name='minimum_exact',source_cell=[87,53]),
            dict(name='minimum_inside_one',source_cell=[87,53],source_y_delta=1),
            dict(name='minimum_outside_one',source_cell=[87,53],source_y_delta=-1),
            dict(name='nonwater_cell_maximum',source_cell=[87,47],target=[87,53]),
            dict(name='water_raw100_control',target_flags=256),
            dict(name='nonwater_raw100_control',source_cell=[87,47],target=[87,53],target_flags=256)]
    rows += [dict(name=stage+'_maximum',stage=stage) for stage in ['damaged','collapsed','repaired']]
    return dict(rows=[execute(row) for row in rows])


def metadata():
    result = provenance(
        scope=__doc__, assumptions=['Physical FV/HoverMissile/AAHeatSeeker2 constructors and retained readers come from existing composed owner; source Unit/House lifecycle is supplied.',
        'Physical target/current Cell fields are projected from frozen full-native Anytown packet; this crop is not the full map loader. Native Cell constructor and coordinate/getter bodies execute.',
        'Source placement is supplied at the original Cell GetCoords result; original5F3DB0 direction and4C9300 setters align the supplied stationary body/turret. One/two-lepton XY and raw100 rows are explicit controls.',
        'Unit approach7414E0 executes through originalFoot4D5690 and its first6F77B0 call; stops immediately after the actual range return at4D570E. No whole pursuit, candidate search or production Rust execution is claimed.',
        'Original CRT/WinMain floating-control fragments establish the measured FPCW; the intervening Windows startup and callback history are excluded. No runtime RNG/Detach/identity callable may be reached.'],
        substitutions=['Inherited type/input/heap/OS preparation seams only; no range, Cell getter, source getter, weapon selection, FireError or line-of-fire result substitution.'],
        entry_points={'range_source':0x6F77B0,'range':0x6F7220,'unit_fire_error':0x740FD0,'techno_fire_error':0x6FC0B0,'T61':0x6FCCEE,'cell_low':0x4867E0,'cell_center':0x486840,'unit_approach':0x7414E0,'approach_range_return':0x4D570E})
    result['native_input_packet_sha256'] = sha(FACTS.read_bytes())
    return result


if __name__ == '__main__':
    finish_vectors(generate, HERE/'range.json.gz', provenance=metadata)
