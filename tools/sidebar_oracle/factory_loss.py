"""Original prerequisite-loss Factory -> House -> local event-tail controls.

Reuse the physical readers, joined VM and retained Sidebar fixture. Python
supplies declared admitted-map/wallet priors and observes original instructions;
it never decides build admission, cancellation, charging, refund or deletion.
See factory_loss.md for exact boundaries and cache inputs.
"""

import hashlib
import os
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_ESP

from tools.native_oracle import finish_vectors, provenance, run_checked
from tools.rules_oracle.bridge_child_sound import sections
from tools.sidebar_oracle import cameo_order as c
from tools.spatial_oracle import building_construction as bc

EXTRA_ASSETS = (
    'GGWEAP.SHP', 'GGWEAP_A.SHP', 'GGWEAP_B.SHP', 'GGWEAPBB.SHP',
    'GGWEAP_1.SHP', 'GGWEAP_2.SHP', 'GGWEAP_3.SHP', 'GGWEAP_4.SHP',
    'GGTECH.SHP', 'GGTECH_A.SHP', 'RTNK.HVA', 'RTNK.VXL',
)
# Absent names reached by the original theater/fallback reader, not fake SHPs.
ABSENT_SHAPES = (
    'GTWEAP', 'GTWEAP_A', 'GTWEAP_AD', 'GTWEAP_B', 'GTWEAP_BD',
    'GTWEAP_1', 'GTWEAP_2', 'GTWEAP_3', 'GTWEAP_4', 'GTWEAPBB',
    'GTTECH', 'GTTECH_A', 'GTTECH_AD', 'GAWEAPMK', 'GTWEAPMK',
    'GATECHMK', 'GTTECHMK', 'GGWEAP_AD', 'GGWEAP_BD', 'GGTECH_AD',
)


def inputs(root, extra):
    m = c.ready_building_inputs(root)
    m.phase = 'setup_factory_loss_inputs'
    u = m.u
    assets = {name: (extra / name).read_bytes() for name in EXTRA_ASSETS}
    m.assets.update(assets)
    m.platform_audio.configure_transport(prepared_files={
        **{name + '.SHP': None for name in ABSENT_SHAPES}, **assets,
    })
    for name, size, ctor in (
        ('GAWEAP', 0x2000, 0x45DD90), ('GATECH', 0x2000, 0x45DD90),
        ('MGTK', 0xF00, 0x7470D0),
    ):
        pointer = m.alloc(size)
        m.invoke(ctor, pointer, (m.cstring(name),))
        m.types[name] = pointer
    art = sections((root / 'ARTMD.INI').read_bytes())
    selected = {name: values for name, values in art.items()
                if name in (*m.ready_art, *m.death_art_input_sections, 'RTNK')
                or name.startswith(('GAWEAP', 'GATECH'))}
    m.make_ini(selected)
    art_cache = bytes(u.mem_read(c.INI, 0x40))
    for filename in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'Hills.map'):
        path = root / filename
        if not path.exists():
            assert filename == 'LANGRULE.INI'
            continue
        physical = sections(path.read_bytes())
        m.make_ini({name: values for name, values in physical.items()
                    if name in ('GAWEAP', 'GATECH', 'MGTK', 'E1',
                                'General', 'Americans', 'Sides')})
        ini = m.alloc(0x40)
        u.mem_write(ini, bytes(u.mem_read(c.INI, 0x40)))
        u.mem_write(c.INI, art_cache)
        for name in ('GAWEAP', 'GATECH'):
            m.invoke(0x45FE50, m.types[name], (ini,))
        m.invoke(0x747620, m.types['MGTK'], (ini,))
    # Original selected TechnoType post-read pass, after all rule layers.
    u.reg_write(UC_X86_REG_ESP, c.INI_SP)
    u.reg_write(UC_X86_REG_EBP, m.types['MGTK'])
    run_checked(u, 0x715B10, 0x715F9E)
    return m


def setup(root, extra):
    m = c.ReadyBuildingFixture(inputs(root, extra), root, unit_startup=True)
    u, get, f = m.u, m.get, m.f
    for pointer in (0xB0B328, 0xB0B408, 0xB04978, 0xB04910):
        m.invoke(0x69DCF0, pointer)
    m.invoke(0x6A4CE0)
    # Ordinary already-open sidebar; cold GUI startup is outside this fixture.
    u.mem_write(c.SIDEBAR + 0x53A5, b'\1')
    owned = [m.yard, m.bar]
    m.loss_buildings = {}
    for name, coord in (('GAWEAP', (22, 14)), ('GATECH', (25, 20))):
        pointer = f.allocate(0x1000)
        f.building(name, pointer, f.allocate(0x100), coord=coord)
        u.mem_write(pointer + 0x74, b'\0')
        m.invoke(0x43F180, pointer, 3)
        u.mem_write(pointer + 0xAC, c.dwords(5))
        u.mem_write(pointer + 0xB4, c.dwords(-1))
        u.mem_write(pointer + 0x534, c.dwords(1))
        m.identities[pointer] = name.lower()
        m.loss_buildings[name] = pointer
        owned.append(pointer)
    f.house_prior(m.er.HOUSE, owned, native_building_counts=True)
    for name, kind, tab in (('MGTK', 40, 3), ('E1', 16, 2)):
        pointer = f.types[name]
        assert m.invoke(0x4F7870, m.er.HOUSE, pointer, 0, 1) == 1
        assert m.invoke(0x5F7900, pointer, 1, 0, 0, m.er.HOUSE)
        index = get(pointer + 0xDF8)
        m.invoke(0x6A8710, c.STRIPS[tab], kind, index)
        m.invoke(0x4FA350, m.er.HOUSE, kind, index, 0, 0)
        factory = m.invoke(0x500510, m.er.HOUSE, kind, 0, 0)
        assert factory
        m.factories[name] = factory
        m.identities[factory] = name.lower() + '_factory'
        m.identities[get(factory + 0x58)] = name.lower() + '_product'
        if name == 'MGTK':
            for _ in range(1000):
                u.mem_write(bc.FRAME, c.dwords(get(bc.FRAME) + 1))
                m.invoke(0x4C9B20, factory)
                if get(factory + 0x24):
                    break
            assert get(factory + 0x24) == 1
    house_tail(m)
    assert not get(0xA802C8) and u.mem_read(m.er.HOUSE + 0x1FC, 1) == b'\0'
    m.save()
    return m


def state(m):
    u, get = m.u, m.get
    factories = {}
    for name, kind in (('MGTK', 40), ('E1', 16)):
        pointer = m.invoke(0x500510, m.er.HOUSE, kind, 0, 0)
        factories[name] = None if not pointer else dict(
            stage=get(pointer + 0x24), balance=get(pointer + 0x60),
        )
    return dict(credits=struct.unpack('<i', u.mem_read(m.er.HOUSE + 0x30C, 4))[0],
                spent=get(m.er.HOUSE + 0x2DC),
                dirty=bool(u.mem_read(m.er.HOUSE + 0x1FC, 1)[0]),
                factories=factories)


def sweep(m):
    m.u.reg_write(UC_X86_REG_ESP, bc.SP)
    run_checked(m.u, 0x55B66A, 0x55B68D, count=2_000_000)


def house_tail(m):
    m.u.reg_write(UC_X86_REG_ESI, m.er.HOUSE)
    m.u.reg_write(UC_X86_REG_ESP, bc.SP)
    run_checked(m.u, 0x4F9265, 0x4F9302, count=5_000_000)


def history(m, wallet, loss):
    m.restore()
    u, get = m.u, m.get
    u.mem_write(m.er.HOUSE + 0x30C, c.dwords(0))
    # Advance original timers while both factories stall, to a common due
    # visit. No stage, balance, timer or refund is authored by the harness.
    for _ in range(2000):
        next_frame = get(bc.FRAME) + 1
        u.mem_write(bc.FRAME, c.dwords(next_frame))
        if all(next_frame >= get(p + 0x2C) + get(p + 0x34)
               for p in m.factories.values()):
            break
        sweep(m)
    else:
        raise AssertionError('no common due timers')
    u.mem_write(m.er.HOUSE + 0x30C, c.dwords(wallet))
    frame, rng = get(bc.FRAME), m.f.rng()
    row = dict(wallet=wallet, loss=loss, before=state(m))
    m.f.phase = 'remove_' + loss
    m.invoke(0x445880, m.loss_buildings[loss])
    row['after_removal'] = state(m)
    m.f.phase = 'native_factory_sweep'
    sweep(m)
    row['after_factories'] = state(m)
    m.f.phase = 'native_house_dirty_suffix'
    house_tail(m)
    row['after_house'] = state(m)
    row['events'] = []
    for index in range(get(0xA802C8)):
        address = 0xA802D4 + ((get(0xA802CC) + index) & 127) * 111
        raw = bytes(u.mem_read(address, 111))
        assert struct.unpack_from('<I', raw, 3)[0] == frame
        assert struct.unpack_from('<I', raw, 7)[0] == 40
        assert struct.unpack_from('<I', raw, 11)[0] == get(m.f.types['MGTK'] + 0xDF8)
        row['events'].append(raw[0])
    m.f.phase = 'native_local_tail_events'
    m.receive()
    assert get(bc.FRAME) == frame and not get(0xA802C8)
    assert m.f.code_unchanged()
    assert m.f.rng() == rng
    row['after_events'] = state(m)
    return row


def generate():
    root = Path(os.environ['VERA20K_READY_BUILDING_ASSETS'])
    extra = Path(os.environ.get('VERA20K_FACTORY_LOSS_ASSETS', root))
    m = setup(root, extra)
    return dict(schema=1, controls=[history(m, wallet, loss) for wallet, loss in
                                  ((0, 'GATECH'), (100, 'GATECH'), (0, 'GAWEAP'))])


def metadata():
    record = provenance(
        scope='Three stock prerequisite-loss controls through original BuildingLimbo, '
              'Factory sweep, House dirty suffix, Strip and local event receive. '
              'Fixture admission and native process/window startup are bounded.',
        assumptions=[
            'Existing ReadyBuildingFixture physical RULESMD/optional LANGRULE/Battle/Hills '
            'readers, ART/sound, initialized VM/arena/map/House/listener priors. Additional '
            'whole BuildingType45DD90/45FE50 and UnitType7470D0/747620 readers execute '
            'for GAWEAP/GATECH/MGTK; selected715B10..715F9E post-read pass executes.',
            'Whole CRT4E6C60 (table813234) initializes Unit vector8B4108 before the '
            'selected Unit constructor. Whole6A4CE0 and69DCF0 initialize reached '
            'tab/scroll gadgets. Sidebar+53A5 is the declared ordinary already-open prior.',
            'Healthy completed preplaced GAWEAP/GATECH, House technology10, country '
            'Americans and flat terrain are admitted priors. Original constructors, '
            'Mark/Logic, counters, BeginProduction, linking and paid Factory AI execute. '
            'MGTK stage1 is paid; E1 starts later. Original zero-wallet visits reach '
            'a common due frame before supplied wallet0/100 and original Limbo.',
            '55B66A..55B68D executes all factories in native construction order. '
            'House suffix4F9265..4F9302 and whole Strip bodies execute. Existing receive '
            '6474C7..6474BF runs original OutList->DoList/ExecuteDoList/Event dispatcher '
            'without frame rewrite; all observed events and receive remain same-frame.',
            'GAWEAP removal reaches earlier Building cleanup HouseAbandon before '
            'Limbo UpdateFactoryQueue. This control bounds before-sweep refund, not '
            'equivalence of VERA inline cleanup or every factory-loss callsite.',
            'No code bytes, admission answers, RNG answers, stage/balance/timer or '
            'refund are replaced. Code unchanged and all three RNG states unchanged '
            'from loss through receive are checked. Queue promotion, network transport, '
            'whole sidebar startup/ordering and ordinary full-map loading are excluded.',
        ],
        substitutions=[
            'Reuse existing joined allocator/physical cache/signed-CRC file service, '
            'DirectSound and opaque OS transport boundaries. Additional physical '
            'asset bytes and pinned absent theater/fallback filenames supply file '
            'service inputs; mounted MIX/theater precedence is not established.',
        ],
        entry_points=dict(building_limbo=0x445880, factory_ai=0x4C9B20,
                          logic_factory_sweep=0x55B66A, house_dirty_suffix=0x4F9265,
                          strip_recalculate=0x6AA600, local_receive=0x6474C7,
                          house_abandon=0x4FAA10, factory_abandon=0x4C9FF0),
    )
    root = Path(os.environ['VERA20K_READY_BUILDING_ASSETS'])
    extra = Path(os.environ.get('VERA20K_FACTORY_LOSS_ASSETS', root))
    record['retail_inputs'] = {
        name: hashlib.sha256(path.read_bytes()).hexdigest() if path.exists() else None
        for name, path in (
            *((name, root / name) for name in ('RULESMD.INI', 'LANGRULE.INI',
                                               'ARTMD.INI', 'SOUNDMD.INI',
                                               'MPBattleMD.ini', 'Hills.map')),
            *((name, extra / name) for name in EXTRA_ASSETS),
        )
    }
    return record


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
