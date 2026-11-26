"""Original stock GAWEAP/MTNK/FV cancellation while the factory is warped.

Composes factory_loss's physical input reader and ReadyBuildingFixture. All
admission, refund, promotion, suspension and charging decisions execute natively.
See temporal_vehicle_promotion.md for the deliberately supplied boundaries.
"""
from pathlib import Path
import hashlib
import struct

from tools.native_oracle import finish_vectors, provenance
from tools.native_oracle import require
from tools.sidebar_oracle import cameo_order as c, factory_loss as loss
from tools.spatial_oracle import building_construction as bc
from tools.spatial_oracle.factory_cadence import state as factory_state

VEHICLE_ASSETS = tuple(name + '.' + suffix for name in (
    'GTNK', 'GTNKTUR', 'GTNKBARL', 'FV', 'FVTUR',
    *(f'FVTUR{i}' for i in range(1, 15)),
) for suffix in ('VXL', 'HVA')) + (
    'MTNKICON.SHP', 'MTNKUICO.SHP', 'FVICON.SHP', 'FVUICO.SHP')
# Physical lookups separately establish these missing optional barrel/turret
# files. This is a declared file-service input, not native archive precedence.
ABSENT_ASSETS = tuple(name + '.' + suffix for name in (
    'FVBARL', *(f'FVBARL{i}' for i in range(1, 33)),
    *(f'FVTUR{i}' for i in range(15, 19)),
) for suffix in ('VXL', 'HVA'))
CALLS = frozenset((
    'house_abandon', 'factory_abandon', 'start_next_queued', 'begin_production',
    'start_production', 'set_production_rate', 'type_find_factory', 'can_build',
    'house_update_factory_queue', 'factory_suspend', 'unit_constructor',
    'building_pause', 'building_resume', 'temporal_let_go', 'factory_ai',
))


def history(m, backup):
    m.restore()
    u, get = m.u, m.get
    if backup:
        second = loss.sale_queue_building(m, 'GAWEAP', (25, 20))
        m.f.house_prior(m.er.HOUSE, (m.yard, m.bar, second), native_building_counts=True)
    for name in ('MTNK', 'FV'):
        require(m.invoke(0x4FA350, m.er.HOUSE, 40, get(m.f.types[name] + 0xDF8), 0, 0) == 0, 'native 0x4FA350 returned an unexpected result')
    factory = m.invoke(0x500510, m.er.HOUSE, 40, 0, 0)
    assert factory
    loss.advance_factory_to_stage(m, factory, 1)
    temporal = loss.temporal_building_link(m, m.bar)

    def state():
        held = get(factory + 0x58)
        signed = lambda address: struct.unpack('<i', u.mem_read(address, 4))[0]
        return dict(frame=get(bc.FRAME),
            factory=factory_state(bytes(u.mem_read(factory, 0x74)),
                                  signed(m.er.HOUSE + 0x30C), signed(m.er.HOUSE + 0x2DC)),
            held_type=m.type_names.get(get(held + 0x6C4)) if held else None,
            held_id=get(held + 0x10) if held else None,
            queue=[m.type_names[get(get(factory + 0x44) + i * 4)]
                   for i in range(get(factory + 0x50))],
            building_online=bool(u.mem_read(m.bar + 0x660, 1)[0]))

    initial = state()
    assert initial['held_type'] == 'MTNK' and initial['queue'] == ['FV']
    phases = []

    def phase(name, operation):
        before = state()
        evidence = m.step(name, operation)
        assert evidence['native_code_unchanged']
        after = state()
        if name != 'temporal_notice':
            expected_queue = ['FV'] if backup and name in (
                'resume_same_fv', 'resumed_factory_ai') else []
            assert after['held_type'] == 'FV' and after['queue'] == expected_queue
        phases.append(dict(name=name, before=before, after=after,
            calls=[dict(name=row['name'], args=row['args'], caller=row['caller'])
                   for row in evidence['native_calls'] if row['name'] in CALLS],
            native_code_unchanged=True))

    def advance(offset):
        u.mem_write(bc.FRAME, c.dwords(initial['frame'] + offset))
        m.invoke(0x4C9B20, factory)

    phase('temporal_notice', lambda: loss.temporal_building_notice(m, temporal, m.bar))
    phase('cancel_mtnk_promote_fv', lambda: m.invoke(
        0x4FAA10, m.er.HOUSE, 40, get(m.f.types['MTNK'] + 0xDF8), 0, 0))
    phase('warped_factory_ai', lambda: advance(20))
    phase('temporal_release', lambda: m.invoke(0x71ABC0, temporal))
    phase('release_factory_ai', lambda: advance(40))
    phase('later_factory_callback', lambda: m.invoke(0x509140, m.er.HOUSE, 40, 0, 0))
    phase('resume_same_fv', lambda: m.invoke(
        0x4FA350, m.er.HOUSE, 40, get(m.f.types['FV'] + 0xDF8), 0, 0))
    phase('resumed_factory_ai', lambda: advance(60))
    return dict(online_backup=backup, initial=initial, phases=phases)


def generate():
    root = loss.sale_sidebar_root()
    inputs = loss.inputs(root, root, unit_names=('MTNK', 'FV'),
                         extra_assets=VEHICLE_ASSETS, absent_assets=ABSENT_ASSETS)
    m = c.ReadyBuildingFixture(inputs, root, unit_startup=True,
                              producer_types=('GACNST', 'GAWEAP'))
    for pointer in (0xB0B328, 0xB0B408, 0xB04978, 0xB04910):
        m.invoke(0x69DCF0, pointer)
    m.invoke(0x6A4CE0)
    m.u.mem_write(c.SIDEBAR + 0x53A5, b'\1')
    m.invoke(0x49F9B0, m.er.HOUSE + 0x55C8)
    m.f.house_prior(m.er.HOUSE, (m.yard, m.bar), native_building_counts=True)
    m.watch.update({0x4C9E60: ('factory_suspend', 1, 4),
                    0x7353C0: ('unit_constructor', 2, 8),
                    0x4521C0: ('building_pause', 0, 0),
                    0x452210: ('building_resume', 0, 0),
                    0x71ABC0: ('temporal_let_go', 0, 0),
                    0x4C9B20: ('factory_ai', 0, 0)})
    m.save()
    return dict(schema=1, controls=[history(m, backup) for backup in (False, True)])


def metadata():
    root = loss.sale_sidebar_root()
    record = provenance(
        scope='Original GAWEAP MTNK-to-FV indexed cancellation and forced promotion with/without an online backup.',
        assumptions=[
            'Existing factory_loss input owner supplies original sparse type/country/rules/ART readers and ReadyBuildingFixture supplies admitted healthy base, clear map, wallet10000 and initialized House counters. No new VM, parser, admission or production owner.',
            'One supplied lone Temporal link precedes original71B0EA..71B14E notice. Original whole LetGo71ABC0 executes. CLEG firing, earlier chain insertion and whole native frame/window are outside the comparison.',
            'Original Begin4FA350 and FactoryAI4C9B20 reach paid MTNK stage1 with FV queued. Whole HouseAbandon4FAA10 executes its refund/destructor/StartNext/forced Begin/constructor/Resume. No held-start flag, FindFactory answer, pause or stage is assigned.',
            'FactoryAI is visited at declared monotonic sample frames. Later509140 is an explicit callback boundary, and same-FV Begin is an admitted PRODUCE receiver boundary; a full building event or mouse/input transport is not claimed.',
            'A second healthy GAWEAP is a supplied admitted building in the online-backup control. Observed native70/71, debt/progress, wallet, meaningful timer, held identity and FIFO are compared by the Rust regression within its stated frame coverage.',
        ],
        substitutions=['Inherited physical byte cache, allocator and OS/COM/DirectSound/EVA transport only. Physical optional-file absence is explicit; full archive/theater winner selection and rendered HOLD pixels are excluded.'],
        entry_points=dict(house_abandon=0x4FAA10, start_next=0x4CA5A0,
            house_begin=0x4FA350, factory_resume=0x4C9EA0, factory_suspend=0x4C9E60,
            factory_ai=0x4C9B20, factory_callback=0x509140,
            temporal_notice=0x71B0EA, temporal_release=0x71ABC0))
    record['retail_inputs'] = {name: hashlib.sha256((root / name).read_bytes()).hexdigest()
        for name in sorted(set((*loss.EXTRA_ASSETS, *VEHICLE_ASSETS,
                               'RULESMD.INI', 'ARTMD.INI', 'SOUNDMD.INI',
                               'MPBattleMD.ini', 'Hills.map', 'Hills.mmx')))}
    record['absent_optional_files'] = list(ABSENT_ASSETS)
    return record


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
