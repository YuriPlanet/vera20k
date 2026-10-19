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
import sys

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX,
    UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_EDI, UC_X86_REG_EIP,
)

from tools.native_oracle import NativeCallTrace, RET_MAGIC, finish_vectors, provenance, run_checked
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
AIRCRAFT_ABSENT_FILES = (
    'GTAIRCBB.SHP', 'GAAIRCBB.SHP', 'GTAIRC_AD.SHP', 'GGAIRC_AD.SHP',
    'GTAIRC_BD.SHP', 'GGAIRC_BD.SHP', 'GTAIRC_CD.SHP', 'GGAIRC_CD.SHP',
    'GAAIRCMK.SHP', 'FALCTUR.VXL', 'FALCTUR.HVA', 'FALCBARL.VXL',
    'FALCBARL.HVA', 'BEAGTUR.VXL', 'BEAGTUR.HVA', 'BEAGBARL.VXL', 'BEAGBARL.HVA',
)

SALE_BUILDINGS = ('NACNST', 'NAPOWR', 'NAHAND', 'NARADR')
SALE_INFANTRY = ('E2', 'FLAKT')


def sale_queue_inputs(root):
    """Same reader/VM owner, with physical Russian type and country inputs."""
    m = c.ready_building_inputs(root)
    m.reserve_arena(0x800000)
    u, get = m.u, m.read32
    m.country = next(row['pointer'] for row in m.death_country_sides['countries']
                     if row['name'] == 'Russians')
    for names, size, ctor, find, registry in (
            (SALE_BUILDINGS, 0x2000, 0x45DD90, 0x45E7B0, 0xA83C6C),
            (SALE_INFANTRY, 0x1900, 0x5236A0, 0x523C90, 0xA8E34C)):
        for name in names:
            index = m.invoke(find, m.cstring(name))
            if index == 0xFFFFFFFF:
                pointer = m.alloc(size)
                m.invoke(ctor, pointer, (m.cstring(name),))
            else:
                pointer = get(get(registry) + 4*index)
            m.types[name] = pointer
    art = sections((root / 'ARTMD.INI').read_bytes())
    selected = {name: values for name, values in art.items()
                if name in (*m.ready_art, *m.death_art_input_sections,
                            'CONS', 'ConSequence', 'FLAKT', 'FlakSequence')
                or name.startswith(SALE_BUILDINGS)}
    m.make_ini(selected)
    art_cache = bytes(u.mem_read(c.INI, 0x40))
    m.sale_layers = []
    for filename in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'Hills.map'):
        path = root / filename
        if not path.exists():
            assert filename == 'LANGRULE.INI'
            continue
        physical = sections(path.read_bytes())
        m.make_ini({name: values for name, values in physical.items()
                    if name in (*SALE_BUILDINGS, *SALE_INFANTRY,
                                'Russians', 'General', 'Sides')})
        ini = m.alloc(0x40)
        u.mem_write(ini, bytes(u.mem_read(c.INI, 0x40)))
        u.mem_write(c.INI, art_cache)
        for name in SALE_BUILDINGS:
            m.invoke(0x45FE50, m.types[name], (ini,))
        for name in SALE_INFANTRY:
            m.invoke(0x5240A0, m.types[name], (ini,))
        m.invoke(0x511850, m.country, (ini,))
        m.sale_layers.append(filename)
    m.sale_type_inputs = {
        name: dict(index=get(pointer + 0xDF8), cost=get(pointer + 0x610),
                   build_limit=get(pointer + 0x3B8))
        for name in (*SALE_BUILDINGS, *SALE_INFANTRY)
        for pointer in (m.types[name],)
    }
    return m


def sale_queue_setup(root, *, inputs=None):
    m = c.ReadyBuildingFixture(sale_queue_inputs(root) if inputs is None else inputs, root, unit_startup=True,
                              producer_types=('NACNST', 'NAHAND'))
    for pointer in (0xB0B328, 0xB0B408, 0xB04978, 0xB04910):
        m.invoke(0x69DCF0, pointer)
    m.invoke(0x6A4CE0)
    m.u.mem_write(c.SIDEBAR + 0x53A5, b'\1')
    owned = [m.yard, m.bar]
    for name, coord in (('NAPOWR', (22, 14)), ('NARADR', (25, 20))):
        pointer = sale_queue_building(m, name, coord)
        owned.append(pointer)
    m.f.house_prior(m.er.HOUSE, owned, native_building_counts=True)
    m.invoke(0x49F9B0, m.er.HOUSE + 0x55C8)
    for name in SALE_INFANTRY:
        pointer = m.f.types[name]
        assert m.invoke(0x4F7870, m.er.HOUSE, pointer, 0, 1) == 1, name
        assert m.invoke(0x5F7900, pointer, 1, 0, 0, m.er.HOUSE), name
        m.invoke(0x6A8710, c.STRIPS[2], 16, m.get(pointer + 0xDF8))
    m.sale_owned = owned
    m.save()
    return m


def sale_queue_building(m, name, coord):
    pointer = m.f.allocate(0x1000)
    m.f.building(name, pointer, m.f.allocate(0x100), coord=coord)
    m.u.mem_write(pointer + 0x74, b'\0')
    m.invoke(0x43F180, pointer, 3)
    m.u.mem_write(pointer + 0xAC, c.dwords(5))
    m.u.mem_write(pointer + 0xB4, c.dwords(-1))
    m.u.mem_write(pointer + 0x534, c.dwords(1))
    m.identities[pointer] = name.lower()
    return pointer


def sale_queue_start(m, successor, survivor=False):
    """One paid Russian infantry input owner for the queue and sidebar controls."""
    m.restore()
    u, get = m.u, m.get
    if survivor:
        second = sale_queue_building(m, 'NAHAND', (28, 14))
        m.f.house_prior(m.er.HOUSE, (*m.sale_owned, second), native_building_counts=True)
    for name in ('E2', successor):
        assert m.invoke(0x4FA350, m.er.HOUSE, 16,
                        get(m.f.types[name] + 0xDF8), 0, 0) == 0
    factory = get(m.er.HOUSE + 0x53B0)
    product = get(factory + 0x58)
    assert product and get(product + 0x6C0) == m.f.types['E2']
    m.factories['E2'] = factory
    m.identities[factory] = 'infantry_factory'
    m.identities[product] = 'held_e2'
    for _ in range(1000):
        u.mem_write(bc.FRAME, c.dwords(get(bc.FRAME) + 1))
        m.invoke(0x4C9B20, factory)
        if get(factory + 0x24):
            break
    assert get(factory + 0x24) == 1
    return factory, product


def sale_queue_history(m, successor, survivor):
    """Observe whole original Limbo, including its prelude and queue suffix."""
    factory, product = sale_queue_start(m, successor, survivor)
    u, get = m.u, m.get
    disposed = False
    def signed(address):
        return struct.unpack('<i', u.mem_read(address, 4))[0]
    def state():
        live = factory in [get(get(0xA83E34) + 4*i) for i in range(get(0xA83E40))]
        held = get(factory + 0x58) if live else 0
        queue = [m.type_names[get(get(factory + 0x44) + 4*i)]
                 for i in range(get(factory + 0x50))] if live else []
        return dict(frame=get(bc.FRAME), credits=signed(m.er.HOUSE + 0x30C),
            spent=signed(m.er.HOUSE + 0x2DC), factory_present=bool(get(m.er.HOUSE + 0x53B0)),
            factory_live=live, held_present=bool(held), held_id=get(held + 0x10) if held else None,
            held_type=m.type_names.get(get(held + 0x6C0)) if held else None,
            queue=queue, stage=get(factory + 0x24) if live else None,
            balance=signed(factory + 0x60) if live else None,
            timer=dict(start=signed(factory + 0x2C), duration=signed(factory + 0x34)) if live else None,
            rate=get(factory + 0x38) if live else None,
            cash_stall=u.mem_read(factory + 0x5C, 1)[0] if live else None,
            suspended=u.mem_read(factory + 0x70, 1)[0] if live else None,
            latch=u.mem_read(factory + 0x71, 1)[0] if live else None,
            product_disposed=disposed, barracks_limbo=u.mem_read(m.bar + 0x81, 1)[0],
            mission=signed(m.bar + 0xAC), identity_cursor=get(get(0xA8B230) + 0x214))
    initial = state()
    m.invoke(0x5B35E0, m.bar, 0x13, 0)
    m.invoke(0x5B3570, m.bar)
    after_selling = state()
    assert after_selling['mission'] == 0x13 and not after_selling['barracks_limbo']
    m.watch.update({0x44EBF0: ('building_detach_all', 1, 4),
                    0x4CA620: ('remove_queued_type', 1, 4),
                    0x4C98B0: ('factory_constructor', 0, 0),
                    0x517A50: ('infantry_constructor', 2, 8),
                    0x517D90: ('infantry_destructor', 0, 0),
                    0x68BCB0: ('next_identity', 0, 0)})
    m.calls.clear()
    m.call_trace.pending.clear()
    checkpoints = []
    calls_of_interest = {'house_abandon', 'factory_abandon', 'remove_queued_type', 'start_next_queued',
                         'begin_production', 'house_update_factory_queue'}
    original_returned = m.call_trace.returned
    def returned(pc, sp):
        nonlocal disposed
        complete = original_returned(pc, sp)
        for index in complete:
            row = m.calls[index]
            if row['name'] == 'infantry_destructor' and int(row['ecx'], 16) == product:
                disposed = True
            if row['name'] in calls_of_interest:
                checkpoints.append(dict(event=row['name'] + '_return', state=state()))
        return complete
    m.call_trace.returned = returned
    def observe(_u, pc, _size, _data):
        if pc in (0x44EBF0, 0x4FA350, 0x509140, 0x517D90):
            checkpoints.append(dict(event=m.watch[pc][0] + '_entry', state=state()))
    observer = u.hook_add(UC_HOOK_CODE, observe)
    before_rng = m.f.rng()
    di, ai = len(m.f.draws), len(m.f.advances)
    try:
        m.f.phase = 'selling_barracks_original_limbo'
        m.invoke(0x445880, m.bar)
    finally:
        u.hook_del(observer)
        m.call_trace.returned = original_returned
    final = state()
    after_rng = m.f.rng()
    constructor_count = sum(row['name'] == 'infantry_constructor' for row in m.calls)
    factory_start_count = sum(row['name'] in ('factory_constructor', 'start_production')
                              for row in m.calls)
    identity_delta = final['identity_cursor'] - after_selling['identity_cursor']
    assert m.f.code_unchanged() and before_rng == after_rng
    assert not m.f.draws[di:] and not m.f.advances[ai:]
    assert constructor_count == factory_start_count == identity_delta == 0
    assert final['held_present'] == survivor and final['factory_present'] == survivor
    assert final['product_disposed'] != survivor
    assert final['queue'] == ([successor] if survivor else [])
    if not survivor:
        destroying = next(row['state'] for row in checkpoints
                          if row['event'] == 'infantry_destructor_entry')
        assert destroying['held_id'] == initial['held_id']
        assert destroying['balance'] == destroying['rate'] == 0 and destroying['suspended'] == 1
        assert destroying['timer'] == dict(start=destroying['frame'], duration=0)
        assert (destroying['latch'], destroying['cash_stall']) == (initial['latch'], initial['cash_stall'])
    if survivor:
        assert final['held_id'] == initial['held_id']
        assert (final['stage'], final['balance']) == (initial['stage'], initial['balance'])
        assert not any(row['name'] == 'house_abandon' for row in m.calls)
    elif successor == 'E2':
        prelude = next(row['state'] for row in checkpoints if row['event'] == 'house_abandon_return')
        assert prelude['held_present'] and not prelude['queue']
        assert prelude['credits'] == initial['credits']
    else:
        promotion = next(row['state'] for row in checkpoints if row['event'] == 'begin_production_entry')
        assert not promotion['held_present'] and not promotion['queue']
        assert next(row['return_eax'] for row in m.calls if row['name'] == 'begin_production') == 3
    names = {'building_detach_all', 'house_abandon', 'remove_queued_type',
             'factory_abandon', 'start_next_queued', 'begin_production',
             'type_find_factory', 'can_build', 'house_update_factory_queue',
             'factory_scalar_delete', 'factory_constructor', 'start_production',
             'infantry_constructor', 'infantry_destructor',
             'next_identity'}
    byte_args = {'building_detach_all': (0,), 'house_abandon': (2, 3),
                 'begin_production': (2, 3), 'type_find_factory': (0, 1, 2),
                 'can_build': (1, 2), 'house_update_factory_queue': (1,)}
    calls = []
    for row in m.calls:
        if row['name'] not in names:
            continue
        args = [value & 255 if i in byte_args.get(row['name'], ()) else value
                for i, value in enumerate(row['args'])]
        args = [m.identity(value) if value and (value in m.identities or value in m.type_names)
                else value for value in args]
        call = dict(name=row['name'], caller=row['caller'], args=args)
        result = row.get('return_eax')
        if row['name'] == 'type_find_factory':
            call['found'] = bool(result)
        elif row['name'] in ('remove_queued_type', 'factory_abandon'):
            call['result'] = bool(result & 255)
        elif row['name'] in ('begin_production', 'can_build'):
            call['result'] = result
        calls.append(call)
    return dict(name='surviving_second_barracks' if survivor else 'last_barracks_' + successor.lower(),
                successor=successor, surviving_barracks=survivor, initial=initial,
                after_selling=after_selling, checkpoints=checkpoints, after_limbo=final,
                calls=calls, constructor_count=constructor_count,
                factory_start_count=factory_start_count, identity_delta=identity_delta,
                rng=dict(unchanged=before_rng == after_rng, request_count=len(m.f.draws)-di,
                    raw_advance_count=len(m.f.advances)-ai,
                    before_sha256={name: hashlib.sha256(bytes.fromhex(raw)).hexdigest()
                                   for name, raw in before_rng.items()},
                    after_sha256={name: hashlib.sha256(bytes.fromhex(raw)).hexdigest()
                                  for name, raw in after_rng.items()}))


def sale_queue_generate():
    root = Path(os.environ.get('VERA20K_FACTORY_SALE_ASSETS') or os.environ['VERA20K_READY_BUILDING_ASSETS'])
    m = sale_queue_setup(root)
    return dict(schema=1, input_arena_bytes=m.inputs.arena_size,
                type_inputs=m.inputs.sale_type_inputs,
                controls=[sale_queue_history(m, successor, survivor) for successor, survivor in
                          (('E2', False), ('FLAKT', False), ('E2', True))])


def sale_queue_metadata():
    root = Path(os.environ.get('VERA20K_FACTORY_SALE_ASSETS') or os.environ['VERA20K_READY_BUILDING_ASSETS'])
    record = provenance(
        scope='Three paid Russian E2 queue controls through whole original Selling-barracks '
              'BuildingLimbo, Detach_All, HouseAbandon, StartNext/Begin and509140. '
              'The completed-sale removal boundary is supplied; staged sale/crew are excluded.',
        assumptions=[
            'Reuse ReadyBuildingFixture and physical joined readers/VM/startup/allocator. '
            'Whole BuildingType45DD90/45FE50, InfantryType5236A0/5240A0 and country511850 '
            'execute over RULESMD, optional LANGRULE, Battle and Hills layers; fixed ART '
            'and SOUND remain unlayered. Physical cache selection and admitted preplaced '
            'House/building list are explicit boundaries, not full Rules::Process or VFS.',
            'Human Russians, GameMode5, technology10, wallet10000 and clear terrain are '
            'declared fixture priors. Whole NACNST/NAHAND/NAPOWR/NARADR constructors, '
            'Mark/Logic and original House/listener startup execute. Original CanBuild '
            'and FindFactory admit E2/FLAKT before whole Begin creates E2 and queues its successor.',
            'Original FactoryAI pays stage1. Original QueueMission/Commence set Selling0x13; '
            'whole445880 then executes44EBF0,4FAA10,4CA620,4CA5A0,4FA350 and509140 as reached. '
            'No native gameplay result, queue, stage, balance, timer, refund, constructor '
            'or RNG answer is supplied. Physical Buildup cadence, crew and whole sale are excluded.',
            'Actual shared request/raw-advance hooks and complete0x3F4 Main/Scenario/MapGen '
            'buffers are compared around Limbo; compact stream hashes are retained. '
            'Infantry constructor/identity entries and native cursor are observed. '
            'Setup constructor draws remain spent. This is not a zero-RNG claim for whole sale.',
        ],
        substitutions=['Existing allocator, declared physical cache/absence service, '
                       'DirectSound/COM and opaque Win32/presentation transport owners only.'],
        entry_points=dict(building_limbo=0x445880, building_detach=0x44EBF0,
            house_abandon=0x4FAA10, remove_queued=0x4CA620, start_next=0x4CA5A0,
            begin_production=0x4FA350, update_queue=0x509140,
            factory_abandon=0x4C9FF0, factory_scalar=0x4CA770,
            infantry_constructor=0x517A50, infantry_destructor=0x517D90,
            next_identity=0x68BCB0),
    )
    record['retail_inputs'] = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                               for p in sorted(root.iterdir()) if p.is_file()}
    record['retail_inputs']['LANGRULE.INI'] = None if not (root/'LANGRULE.INI').exists() else record['retail_inputs']['LANGRULE.INI']
    return record


def sale_sidebar_root():
    return Path(os.environ.get('VERA20K_FACTORY_SIDEBAR_ASSETS') or
                os.environ.get('VERA20K_FACTORY_SALE_ASSETS') or
                os.environ['VERA20K_READY_BUILDING_ASSETS'])


def sale_sidebar_setup(root):
    from tools.sidebar_oracle import geometry
    inputs = sale_queue_inputs(root)
    inputs.sidebar_shapes = {}
    for name, source, destination in (('DARKEN.SHP', 0x83F92C, 0xB07BC0),
                                       ('GCLOCK2.SHP', 0x83FA58, 0xB0B484)):
        raw = (root / name).read_bytes()
        assert inputs.string(source) == name
        inputs.u.reg_write(UC_X86_REG_EDX, 0 if name == 'DARKEN.SHP' else 0xB0B48F)
        pointer = inputs.invoke(0x5B40B0 if name == 'DARKEN.SHP' else 0x4A38D0, source)
        assert pointer and bytes(inputs.u.mem_read(pointer, len(raw))) == raw
        inputs.sidebar_shapes[name] = dict(pointer=pointer, destination=destination,
            bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(), header8_hex=raw[:8].hex())
    m = sale_queue_setup(root, inputs=inputs)
    m.sidebar_side = next(row['native_side'] for row in inputs.death_country_sides['countries']
                          if row['name'] == 'Russians')
    geometry.initialize_layout(m.u, 640, 480, m.sidebar_side, m.f.allocate(64))
    m.u.mem_write(0x887300, c.dwords(m.get(0x880A04)))
    for row in inputs.sidebar_shapes.values():
        m.u.mem_write(row['destination'], c.dwords(row['pointer']))
    for tab, strip in enumerate(c.STRIPS):
        m.invoke(0x6A8220, strip, tab)
    m.invoke(0x6A7590, c.SIDEBAR, 2)
    assert m.get(c.SIDEBAR + 0x539C) == 2
    assert m.u.mem_read(c.STRIPS[2] + 0x1C, 1)[0] == 1
    # Consume the already requested startup pass while the base is online.
    # Later controls restore these executed settled bytes; SELL is not made
    # to request an extra pass by the fixture.
    house_tail(m)
    assert m.u.mem_read(m.er.HOUSE + 0x1FC, 1)[0] == 0
    m.save()
    return m


def sale_sidebar_state(m, factory, *, request_projection=False):
    """Shared sidebar/Factory observer with an explicit new request projection.

    Historical corpora called record+24 (the dirty byte) ``request_state``.
    Preserve their default schema/bytes; the opt-in reads actual request+10
    and observes its predicted clock, including the no-Factory interval.
    """
    from tools.spatial_oracle.factory_cadence import state as factory_state
    u, get = m.u, m.get
    strip = c.STRIPS[2]
    names = {get(m.f.types[name] + 0xDF8): name for name in SALE_INFANTRY}
    entries = []
    for slot in range(get(strip + 0x54)):
        record = strip + 0x58 + slot * 52
        index, kind = get(record), get(record + 4)
        if kind != 16 or index not in names:
            continue
        linked = get(record + 12)
        held = get(linked + 0x58) if linked else 0
        entry = dict(name=names[index], kind=kind, slot=slot,
            request_state=get(record + (16 if request_projection else 24)),
            linked=bool(linked),
            linked_held_type=m.type_names.get(get(held + 0x6C0)) if held else None)
        if request_projection:
            entry.update(predicted_stage=struct.unpack('<i', u.mem_read(record + 20, 4))[0],
                predicted_rate=struct.unpack('<i', u.mem_read(record + 40, 4))[0],
                increment=struct.unpack('<i', u.mem_read(record + 44, 4))[0],
                prediction_timer=dict(start=struct.unpack('<i', u.mem_read(record + 28, 4))[0],
                                      duration=struct.unpack('<i', u.mem_read(record + 36, 4))[0]))
        entries.append(entry)
    credits = struct.unpack('<i', u.mem_read(m.er.HOUSE + 0x30C, 4))[0]
    spent = struct.unpack('<i', u.mem_read(m.er.HOUSE + 0x2DC, 4))[0]
    current_factory = None
    if factory:
        held = get(factory + 0x58)
        raw = bytes(u.mem_read(factory, 0x74))
        current_factory = dict(factory_state(raw, credits, spent),
            changed=bool(raw[0x5D]),
            held_type=m.type_names.get(get(held + 0x6C0)) if held else None,
            held_id=get(held + 0x10) if held else None,
            queue=[m.type_names[get(get(factory + 0x44) + i * 4)]
                   for i in range(get(factory + 0x50))])
    result = dict(frame=get(bc.FRAME), house_recheck=u.mem_read(m.er.HOUSE + 0x1FC, 1)[0],
        total_entries=get(strip + 0x54), entries=entries,
        mission=struct.unpack('<i', u.mem_read(m.bar + 0xAC, 4))[0],
        barracks_limbo=u.mem_read(m.bar + 0x81, 1)[0],
        factory=current_factory)
    if request_projection:
        result.update(credits=credits, spent=spent,
            category_request_bytes=list(u.mem_read(m.er.HOUSE + 0x53D0, 9)))
    return result


def sale_sidebar_events(m):
    """Project original payload fields from the existing retained ring reader."""
    events = []
    for encoded in m.snapshot()['out_list']['events']:
        raw = bytes.fromhex(encoded)
        events.append(dict(kind=raw[0], frame=struct.unpack_from('<I', raw, 3)[0],
            instance_kind=struct.unpack_from('<I', raw, 7)[0],
            index=struct.unpack_from('<i', raw, 11)[0],
            naval=struct.unpack_from('<I', raw, 15)[0],
            cell=list(struct.unpack_from('<2h', raw, 19)) if raw[0] == 11 else None))
    return events


def advance_factory_to_stage(m, factory, target_stage):
    """Advance the existing paid head through original FactoryAI4C9B20."""
    for _ in range(1000):
        m.u.mem_write(bc.FRAME, c.dwords(m.get(bc.FRAME) + 1))
        m.invoke(0x4C9B20, factory)
        if m.get(factory + 0x24) == target_stage:
            break
    assert m.get(factory + 0x24) == target_stage


def sale_sidebar_control(m, name, *, selling=False, complete=False, recheck=False, survivor=False,
                         target_stage=None, auto_receive=False):
    from tools.sidebar_oracle import power_draw
    factory, _product = sale_queue_start(m, 'FLAKT', survivor)
    u, get = m.u, m.get
    house_tail(m)
    before = sale_sidebar_state(m, factory)
    assert before['house_recheck'] == 0
    if selling:
        m.invoke(0x447110, m.bar, 0xFFFFFFFF)
        assert get(m.bar + 0xAC) == 0x13
        assert u.mem_read(m.er.HOUSE + 0x1FC, 1)[0] == 0
    if complete or target_stage is not None:
        target_stage = 54 if complete else target_stage
        advance_factory_to_stage(m, factory, target_stage)
    prepared = sale_sidebar_state(m, factory)
    if recheck:
        u.mem_write(m.er.HOUSE + 0x1FC, b'\1')  # explicit external request input
    m.watch.update({0x6A7780: ('sidebar_action', 2, 8),
                    0x6A8B30: ('strip_ai', 2, 8), 0x6A9540: ('strip_draw', 1, 4),
                    0x6A7D20: ('sidebar_recalculate', 0, 0),
                    0x6AA600: ('strip_recalculate', 0, 0),
                    0x734E60: ('localized_text', 2, 8),
                    0x4C9B20: ('factory_ai', 0, 0),
                    0x4C98B0: ('factory_constructor', 0, 0),
                    0x517A50: ('infantry_constructor', 2, 8),
                    0x517D90: ('infantry_destructor', 0, 0),
                    0x68BCB0: ('next_identity', 0, 0)})
    m.calls.clear()
    m.call_trace.pending.clear()
    decisions, commands, lookups, place_path = [], [], [], []
    current, current_slot = None, None
    def observe(_u, pc, _size, _data):
        nonlocal current, current_slot
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == 0x6A9664:
            # Empty visible slots take6A9727->6A99BE without Type lookup.
            # Reset observation identity at the original loop, never dedup.
            current, current_slot = None, None
        elif pc == 0x6A976B:
            current_slot = get(sp + 0x3C)
            strip = get(sp + 0x24)
            assert strip == c.STRIPS[2] and current_slot < get(strip + 0x54)
            current = m.type_names.get(u.reg_read(UC_X86_REG_EAX))
            if current in SALE_INFANTRY:
                record = strip + 0x58 + current_slot * 52
                assert get(record) == get(m.f.types[current] + 0xDF8) and get(record + 4) == 16
        elif pc == 0x6A99C3 and current in SALE_INFANTRY:
            decisions.append(dict(name=current, slot=current_slot,
                unavailable=u.mem_read(sp + 0x15, 1)[0],
                linked=u.mem_read(sp + 0x12, 1)[0], complete=u.mem_read(sp + 0x14, 1)[0],
                final_bl=u.reg_read(UC_X86_REG_EBX) & 255, stage=get(sp + 0x38)))
        elif pc == 0x734E60 and current in SALE_INFANTRY:
            pointer = u.reg_read(UC_X86_REG_ECX)
            key = bytes(u.mem_read(pointer, 128)).split(b'\0', 1)[0].decode('latin1')
            lookups.append(dict(name=current, key=key, caller=hex(get(sp))))
        elif pc == 0x4AED70:
            row = power_draw.shape_command(u, get)
            kinds = {0x6A9A43: 'cameo', 0x6A9B4B: 'unavailable_darken',
                     0x6A9BC5: 'flash', 0x6A9E9C: 'clock'}
            if current in SALE_INFANTRY:
                args = row['args']
                commands.append(dict(name=current, kind=kinds.get(row['caller'], 'other_shape'),
                    caller=hex(row['caller']), frame=args[1], flags=args[4]))
            power_draw.presentation_return(u, get, 56)
        elif pc == 0x4A60E0 and current in SALE_INFANTRY and get(sp) == 0x6A9E3B:
            commands.append(dict(name=current, kind='ready_text', caller=hex(get(sp)),
                                 flags=get(sp + 32)))
        elif pc in (0x4FB534, 0x4FB542, 0x4FB512, 0x4FB5B5):
            place_path.append(dict(pc=hex(pc), eax=u.reg_read(UC_X86_REG_EAX)))
    observer = u.hook_add(UC_HOOK_CODE, observe)
    before_rng = m.f.rng()
    di, ai = len(m.f.draws), len(m.f.advances)
    cursor = get(get(0xA8B230) + 0x214)
    automatic_place = None
    try:
        m.f.phase = name
        house_tail(m)
        # One control isolates the READY drawing decision before StripAI;
        # the automatic-tail control executes the ordinary issuer/receiver.
        if not complete or auto_receive:
            m.invoke(0x6A7780, c.SIDEBAR, m.key, m.mouse + 0x1C)
        if auto_receive:
            automatic_place = dict(after_first_action=sale_sidebar_state(m, factory),
                                   first_events=sale_sidebar_events(m))
            m.receive()
            automatic_place['after_first_receive'] = sale_sidebar_state(m, factory)
            u.mem_write(bc.FRAME, c.dwords(get(bc.FRAME) + 1))
            m.invoke(0x4C9B20, factory)
            automatic_place['after_next_factory_ai'] = sale_sidebar_state(m, factory)
            second_calls = len(m.calls)
            m.invoke(0x6A7780, c.SIDEBAR, m.key, m.mouse + 0x1C)
            automatic_place['second_events'] = sale_sidebar_events(m)
            m.receive()
            automatic_place['after_second_receive'] = sale_sidebar_state(m, factory)
            automatic_place['second_house_place_calls'] = sum(
                row['name'] == 'house_place' for row in m.calls[second_calls:])
            assert not get(0xA802C8)
        # The lifecycle-only tail reaches cold font state after local receive;
        # its DRAW is separate from the READY decision control above. Do not
        # fabricate a font or presentation result to extend that boundary.
        if not auto_receive:
            m.invoke(0x6A9540, c.STRIPS[2], 1)
    finally:
        u.hook_del(observer)
    after = sale_sidebar_state(m, factory)
    after_rng = m.f.rng()
    assert before_rng == after_rng and not m.f.draws[di:] and not m.f.advances[ai:]
    assert m.f.code_unchanged()
    names = {'sidebar_action', 'strip_ai', 'strip_draw', 'sidebar_recalculate', 'strip_recalculate',
             'type_find_factory', 'can_build', 'factory_ai', 'place_event_constructor',
             'house_place', 'object_find_factory'}
    calls = []
    for row in m.calls:
        if row['name'] not in names:
            continue
        this = int(row['ecx'], 16)
        item = m.type_names.get(this) if row['name'] == 'type_find_factory' else None
        if row['name'] == 'can_build':
            item = m.type_names.get(row['args'][0])
        if row['name'] in ('type_find_factory', 'can_build') and item not in SALE_INFANTRY:
            continue
        call = dict(name=row['name'], caller=row['caller'], returned='return_eax' in row)
        if item:
            call['type'] = item
            call['result'] = bool(row['return_eax']) if row['name'] == 'type_find_factory' else row['return_eax']
        if this in c.STRIPS:
            call['tab'] = c.STRIPS.index(this)
        if row['name'] == 'house_place':
            call.update(instance_kind=row['args'][0], index=row['args'][1],
                        naval=row['args'][2] & 255, result=bool(row['return_eax'] & 255))
        elif row['name'] == 'object_find_factory':
            call.update(args=[value & 255 for value in row['args']], result=bool(row['return_eax']))
        calls.append(call)
    constructors = sum(row['name'] in ('factory_constructor', 'infantry_constructor', 'start_production')
                       for row in m.calls)
    detach = sum(row['name'] in ('object_detach', 'dispatch_pointer_expired') for row in m.calls)
    cleanup = sum(row['name'] in ('infantry_destructor', 'factory_scalar_delete') for row in m.calls)
    abandon = sum(row['name'] in ('house_abandon', 'factory_abandon') for row in m.calls)
    identity_delta = get(get(0xA8B230) + 0x214) - cursor
    assert constructors == detach == cleanup == abandon == identity_delta == 0
    entries = [row['name'] for row in after['entries']]
    assert entries == ([] if recheck and not survivor else ['E2', 'FLAKT'])
    if entries and not auto_receive:
        e2 = next(row for row in decisions if row['name'] == 'E2')
        flakt = next(row for row in decisions if row['name'] == 'FLAKT')
        assert e2['final_bl'] == 0 and e2['linked'] == 1
        assert e2['complete'] == int(complete)
        assert flakt['final_bl'] == int(selling and not survivor)
        assert any(row['kind'] == ('ready_text' if complete else 'clock') and row['name'] == 'E2'
                   for row in commands)
        assert [(row['name'], row['slot']) for row in decisions] == [
            (row['name'], row['slot']) for row in after['entries']]
    if auto_receive:
        assert complete and automatic_place['first_events'] == [dict(
            kind=11, frame=prepared['frame'], instance_kind=15, index=-1, naval=0,
            cell=list(struct.unpack('<2h', u.mem_read(0xB07DD8, 4))))]
        assert not automatic_place['second_events'] and not automatic_place['second_house_place_calls']
        assert after['factory']['held_id'] == prepared['factory']['held_id']
        assert after['factory']['credits'] == prepared['factory']['credits']
    events = []
    indices = {get(m.f.types[type_name] + 0xDF8): type_name for type_name in SALE_INFANTRY}
    for i in range(get(0xA802C8)):
        pointer = 0xA802D4 + ((get(0xA802CC) + i) & 127) * 111
        if get(pointer + 7) == 16 and get(pointer + 11) in indices:
            events.append(dict(kind=u.mem_read(pointer, 1)[0], type=indices[get(pointer + 11)]))
    return dict(name=name, inputs=dict(selling=selling, complete=complete,
                explicit_recheck=recheck, surviving_barracks=survivor,
                target_stage=target_stage, auto_receive=auto_receive),
        before=before, prepared=prepared, after=after, decisions=decisions,
        draw_commands=commands, text_lookups=lookups, calls=calls,
        selected_events=events, total_events=get(0xA802C8), automatic_place=automatic_place,
        place_path=place_path, constructor_count=constructors,
        detach_count=detach, cleanup_count=cleanup, abandon_count=abandon, identity_delta=identity_delta,
        rng=dict(unchanged=True, request_count=len(m.f.draws)-di, raw_advance_count=len(m.f.advances)-ai,
            before_sha256={key: hashlib.sha256(bytes.fromhex(raw)).hexdigest() for key, raw in before_rng.items()},
            after_sha256={key: hashlib.sha256(bytes.fromhex(raw)).hexdigest() for key, raw in after_rng.items()}))


def sale_sidebar_manual_control(m):
    prior = sale_sidebar_control(m, 'manual_ready_prior', selling=True, complete=True,
                                 auto_receive=True)
    factory = m.factories['E2']
    u, get = m.u, m.get
    def state():
        live = factory in [get(get(0xA83E34) + 4*i) for i in range(get(0xA83E40))]
        return dict(factory_live=live, house_factory_present=bool(get(m.er.HOUSE + 0x53B0)),
            held=sale_sidebar_state(m, factory) if live else None,
            credits=struct.unpack('<i', u.mem_read(m.er.HOUSE + 0x30C, 4))[0],
            spent=struct.unpack('<i', u.mem_read(m.er.HOUSE + 0x2DC, 4))[0])
    before = state()
    m.calls.clear()
    m.call_trace.pending.clear()
    before_rng = m.f.rng()
    di, ai = len(m.f.draws), len(m.f.advances)
    cursor = get(get(0xA8B230) + 0x214)
    m.f.phase = 'selling_complete_manual_click_tail'
    m.click('E2', kind=16, tab=2)
    after_click, events = state(), sale_sidebar_events(m)
    m.receive()
    after = state()
    after_rng = m.f.rng()
    names = {'select_action', 'object_find_factory', 'type_find_factory', 'house_abandon',
        'factory_abandon', 'sidebar_abandon_factory', 'strip_abandon_factory',
        'infantry_destructor', 'factory_scalar_delete', 'dispatch_pointer_expired',
        'factory_pointer_expired', 'house_pointer_expired', 'play_gui_sound', 'eva',
        'start_next_queued', 'begin_production'}
    calls = []
    eva_requests = []
    for row in m.calls:
        if row['name'] not in names:
            continue
        call = dict(name=row['name'], caller=row['caller'], returned='return_eax' in row)
        if row['name'] in ('object_find_factory', 'type_find_factory'):
            call.update(args=[value & 255 for value in row['args'][:2]],
                        result=bool(row['return_eax']))
        elif row['name'] == 'house_abandon':
            call.update(kind=row['args'][0], index=row['args'][1],
                        naval=row['args'][2] & 255, all=row['args'][3] & 255)
        elif row['name'] == 'eva':
            pointer = int(row['ecx'], 16)
            eva_requests.append(bytes(u.mem_read(pointer, 128)).split(b'\0', 1)[0].decode('latin1'))
        calls.append(call)
    constructors = sum(row['name'] in ('factory_constructor', 'infantry_constructor', 'start_production')
                       for row in m.calls)
    detach = sum(row['name'] in ('object_detach', 'dispatch_pointer_expired') for row in m.calls)
    destructors = sum(row['name'] == 'infantry_destructor' for row in m.calls)
    identity_delta = get(get(0xA8B230) + 0x214) - cursor
    assert m.f.code_unchanged() and not get(0xA802C8)
    assert constructors == identity_delta == 0
    return dict(name='selling_complete_manual_click_tail', before=before,
        after_click=after_click, after=after, events=events, eva_requests=eva_requests,
        calls=calls, constructor_count=constructors, detach_count=detach,
        destructor_count=destructors, identity_delta=identity_delta,
        rng=dict(unchanged=before_rng == after_rng,
            request_count=len(m.f.draws)-di, raw_advance_count=len(m.f.advances)-ai,
            before_sha256={key: hashlib.sha256(bytes.fromhex(raw)).hexdigest() for key, raw in before_rng.items()},
            after_sha256={key: hashlib.sha256(bytes.fromhex(raw)).hexdigest() for key, raw in after_rng.items()}))


def sale_sidebar_generate():
    m = sale_sidebar_setup(sale_sidebar_root())
    return dict(schema=1, type_inputs=m.inputs.sale_type_inputs,
        presentation=dict(screen=[640, 480], scenario_side=m.sidebar_side,
            shapes={name: {key: row[key] for key in ('bytes', 'sha256', 'header8_hex')}
                    for name, row in m.inputs.sidebar_shapes.items()}),
        controls=[sale_sidebar_control(m, name, **values) for name, values in (
            ('settled_running', {}), ('selling_running', dict(selling=True)),
            ('selling_stage53', dict(selling=True, target_stage=53)),
            ('selling_complete_before_strip_ai', dict(selling=True, complete=True)),
            ('selling_complete_auto_place_tail', dict(selling=True, complete=True, auto_receive=True)),
            ('selling_recheck', dict(selling=True, recheck=True)),
            ('surviving_barracks_recheck', dict(selling=True, recheck=True, survivor=True)))]
                 + [sale_sidebar_manual_control(m)])


def sale_sidebar_metadata():
    root = sale_sidebar_root()
    record = provenance(
        scope='Eight selected E2/FLAKT retained-membership, original StripDraw command and event-tail controls '
              'around Selling and explicit local House1FC consumption; no raster or whole sale.',
        assumptions=[
            'Reuse paid Russian input/readers, ReadyBuildingFixture VM/allocator and native admission. '
            'Original layout sequence and native tab selection supply the Infantry tab. Original '
            'LoadFileFromMIX5B40B0 physical-byte service and4A38D0 bind real DARKEN and Soviet GCLOCK2.',
            'Startup House1FC is consumed before the paid baseline. Whole SellBack447110(-1) sets '
            'Selling without a new request. Recheck controls explicitly supply only House1FC=1; '
            'the original local suffix4F9265..4F9302 owns clearing, scanning, pruning and events.',
            'Full SidebarAction6A7780/StripAI run before whole StripDraw6A9540. Completed E2 is '
            'reached by original FactoryAI. One READY control draws before StripAI; the automatic '
            'tail control executes StripAI PLACE, existing6474C7..6474BF local receive, next-frame '
            'FactoryAI and another full SidebarAction/receive. Same WinMM transport supplies '
            'wall_ms0 for direct IAT and StripAI indirect CALL EBX through IAT7E1530.',
            'The automatic event-tail control excludes later DRAW/font execution; the separate '
            'READY control executes original DRAW before local receive. The final manual control '
            'reuses that completed retained state and existing SelectAction with type-kind16/tab2, '
            'then original local event receive. Actual indexed Abandon, EVA request and lifecycle '
            'are recorded; EVA device/queue/sample behavior remains the inherited sink boundary.',
            'Entries/decisions/commands project only E2 and FLAKT; total counts/events remain '
            'visible because the inherited bounded registry can admit other entries. No full '
            'Rules::Process, VFS/theater winner, complete scenario or membership census claim.',
            'Existing joined surface supplies original width/height getters. CC_Draw_Shape4AED70 '
            'uses the existing power_draw RET56/volatile-output transport; no pixels. Original '
            'StringTable lookup/text consumer execute with their cold diagnostic fallback, so '
            'TXT_READY key/decision/command are proven, not localized glyphs or font rendering.',
            'All three full RNG buffers and shared request/advance hooks are compared only around '
            'House/Sidebar/Draw after SellBack/completion preparation. Setup and click/audio '
            'effects remain outside that zero-draw boundary. EVA752700 remains observed RET4 sink.',
        ],
        substitutions=['Existing physical-cache/allocator/OS/COM/DirectSound/EVA boundaries; '
                       'CC_Draw_Shape records actual commands without raster execution.'],
        entry_points=dict(sell_back=0x447110, house_suffix=0x4F9265, sidebar_recalculate=0x6A7D20,
            strip_recalculate=0x6AA600, sidebar_action=0x6A7780, strip_ai=0x6A8B30,
            strip_draw=0x6A9540, draw_shape=0x4AED70, text_lookup=0x734E60, text_draw=0x4A60E0,
            local_receive=0x6474C7, house_place=0x4FB0E0, object_find_factory=0x5F5C20))
    record['retail_inputs'] = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                               for p in sorted(root.iterdir()) if p.is_file()}
    record['retail_inputs']['LANGRULE.INI'] = None if not (root / 'LANGRULE.INI').exists() else record['retail_inputs']['LANGRULE.INI']
    return record


def held_factory_observer(m, factory, product, *, extra_calls=(), extra_state=None,
                          capture_rng_requests=False):
    """One observer for paid-head hold controls; it supplies no gameplay state."""
    assert m.get(factory + 0x58) == product
    m.watch.update({0x4FA910: ('house_suspend', 3, 12),
                    0x4C9E60: ('factory_suspend', 1, 4),
                    0x4CA6E0: ('factory_rate_rewrite', 0, 0),
                    0x6F47A0: ('time_to_build', 0, 0),
                    0x4C98B0: ('factory_constructor', 0, 0),
                    0x517A50: ('infantry_constructor', 2, 8),
                    0x517D90: ('infantry_destructor', 0, 0),
                    0x68BCB0: ('next_identity', 0, 0)})
    m.calls.clear()
    m.call_trace.pending.clear()
    phases = []

    def state():
        observed = sale_sidebar_state(m, factory)
        result = dict(frame=observed['frame'], factory=observed['factory'])
        if extra_state is not None:
            result.update(extra_state())
        return result

    def identity_cursor():
        return m.get(m.get(0xA8B230) + 0x214)

    def hashes(streams):
        return {key: hashlib.sha256(bytes.fromhex(raw)).hexdigest()
                for key, raw in streams.items()}

    def phase(label, operation, *, allow_detach=False, allow_construction=False,
              allow_product_change=False, allow_rng=False):
        before, cursor = state(), identity_cursor()
        previous_product = m.get(factory + 0x58)
        evidence = m.step(label, operation)
        after = state()
        calls = []
        for row in evidence['native_calls']:
            if row['name'] not in ('select_action', 'house_suspend', 'factory_suspend',
                    'begin_production', 'set_production_rate', 'type_find_factory',
                    'sidebar_link_factory', 'factory_rate_rewrite', 'time_to_build',
                    'play_gui_sound', 'eva', *extra_calls):
                continue
            call = dict(name=row['name'], caller=row['caller'], args_raw=row['args'],
                        returned='return_eax' in row)
            if 'return_eax' in row:
                call['return_eax_raw'] = row['return_eax']
            if row['name'] in ('factory_suspend', 'set_production_rate'):
                # These original functions read the byte argument. Native
                # event callers may leave unrelated high bytes in its slot.
                call.update(argument_low_byte=row['args'][0] & 255,
                            return_low_byte=row['return_eax'] & 255)
            elif row['name'] in ('house_suspend', 'begin_production'):
                call['naval_low_byte'] = row['args'][2] & 255
                if row['name'] == 'begin_production':
                    call['force_low_byte'] = row['args'][3] & 255
            elif row['name'] == 'time_to_build':
                call.update(return_i32=struct.unpack('<i', c.dwords(row['return_eax']))[0],
                            held_object=int(row['ecx'], 16) == m.get(factory + 0x58))
            elif row['name'] == 'eva':
                call['request'] = m.inputs.string(int(row['ecx'], 16))
            calls.append(call)
        timers = []
        for row in evidence['timer_writes']:
            offset = int(row['address'], 16) - factory
            if offset in (0x2C, 0x34):
                timers.append(dict(field='start' if offset == 0x2C else 'duration',
                    pc=row['pc'], size=row['size'], before=row['before'], after=row['after']))
        all_calls = evidence['native_calls']
        constructors = sum(row['name'] in ('start_production', 'factory_constructor',
                           'infantry_constructor') for row in all_calls)
        detaches = sum(row['name'] in ('object_detach', 'dispatch_pointer_expired')
                       for row in all_calls)
        destructors = sum(row['name'] in ('infantry_destructor', 'factory_scalar_delete')
                          for row in all_calls)
        before_rng, after_rng = evidence['before']['rng'], evidence['after']['rng']
        # The inherited advance hook observes the inline raw step in Ranged
        # (65C84B). Whole Random (65C780) has a separate store at65C79D;
        # its existing request observation contains the full entry/return
        # states. An enabled call advances once; a disabled call changes none.
        raw_method_advances = sum(row['entry'] == 0x65C780
            and row['state_before'] != row['state_after']
            for row in evidence['rng_requests'])
        assert evidence['native_code_unchanged']
        if not allow_rng:
            assert before_rng == after_rng
            assert not evidence['rng_requests'] and not evidence['rng_advances']
        if not allow_construction:
            assert constructors == identity_cursor() - cursor == 0
        if not allow_detach:
            assert detaches == destructors == 0
        assert m.get(m.er.HOUSE + 0x53B0) == factory
        if not allow_product_change:
            assert m.get(factory + 0x58) == previous_product
        result = dict(name=label, before=before, after=after,
            events=sale_sidebar_events(m), calls=calls, timer_writes=timers,
            constructor_count=constructors, detach_count=detaches,
            destructor_count=destructors, identity_delta=identity_cursor() - cursor,
            factory_identity_unchanged=True,
            held_identity_unchanged=m.get(factory + 0x58) == previous_product,
            rng=dict(unchanged=before_rng == after_rng,
                request_count=len(evidence['rng_requests']),
                raw_advance_count=raw_method_advances + len(evidence['rng_advances']),
                before_sha256=hashes(before_rng), after_sha256=hashes(after_rng)))
        if capture_rng_requests:
            result['rng'].update(requests=evidence['rng_requests'],
                raw_method_advance_count=raw_method_advances,
                range_inline_advance_count=len(evidence['rng_advances']),
                range_inline_advances=evidence['rng_advances'])
        phases.append(result)
        return result

    return state, phase, phases


class PendingProductionObservation:
    """Observe selected native request/clock boundaries in the existing VM.

    The shared sidebar state reader owns the fields. This collector adds no
    admission, production, timer or arithmetic implementation.
    """

    NUMERIC_FIELDS = {
        0x6AB864: (('cost_times_ahead_low_i32', UC_X86_REG_ECX),),
        0x6AB879: (('minimum_step_credits_i32', UC_X86_REG_EDI),
                   ('available_money_i32', UC_X86_REG_EAX)),
        0x6AB8A9: (('type_base_duration_i32', UC_X86_REG_EAX),),
        0x6AB8E9: (('duration_i32', UC_X86_REG_EDI),
                   ('ahead_plus_fifteen_i32', UC_X86_REG_ECX)),
        0x6AB920: (('initial_rate_i32', UC_X86_REG_EDX),),
        0x6AB5B3: (('smoothed_stage_i32', UC_X86_REG_EAX),),
        0x6AB5C3: (('actual_rate_before_increment_i32', UC_X86_REG_EAX),),
    }

    def __init__(self, m):
        self.m = m
        self.numeric, self.timer_writes, self.rng_states = [], [], {}
        self.clock_records = self.records()
        m.watch.update({0x4FA910: ('house_suspend', 3, 12),
            0x4C9E60: ('factory_suspend', 1, 4),
            0x711EE0: ('type_base_duration', 0, 0),
            0x711F00: ('type_cost', 1, 4),
            0x4F6990: ('available_money', 1, 4),
            0x4C9FB0: ('factory_prediction_rate', 0, 0),
            0x6F47A0: ('time_to_build', 0, 0),
            0x4C98B0: ('factory_constructor', 0, 0),
            0x517A50: ('infantry_constructor', 2, 8),
            0x517D90: ('infantry_destructor', 0, 0),
            0x68BCB0: ('next_identity', 0, 0)})
        self.call_names = {value[0] for value in m.watch.values()}
        self.code_hook = m.u.hook_add(UC_HOOK_CODE, self.observe)
        self.write_hook = m.u.hook_add(UC_HOOK_MEM_WRITE, self.write)

    def close(self):
        self.m.u.hook_del(self.code_hook)
        self.m.u.hook_del(self.write_hook)

    def records(self):
        m, strip = self.m, c.STRIPS[2]
        names = {m.get(m.f.types[name] + 0xDF8): name for name in SALE_INFANTRY}
        return {names[m.get(record)]: record
                for slot in range(m.get(strip + 0x54))
                for record in (strip + 0x58 + slot * 52,)
                if m.get(record + 4) == 16 and m.get(record) in names}

    def observe(self, u, pc, _size, _data):
        m = self.m
        if pc == 0x4C98B0:
            factory = u.reg_read(UC_X86_REG_ECX)
            # Register only the actually entered constructor receiver. This
            # lets the existing timer observer see its initial real writes.
            m.factories['pending_infantry'] = factory
            m.identities[factory] = 'infantry_factory'
        elif pc == 0x517A50:
            m.identities[u.reg_read(UC_X86_REG_ECX)] = 'held_infantry'
        if pc in self.NUMERIC_FIELDS:
            self.numeric.append(dict(pc=hex(pc), **{
                name: struct.unpack('<i', c.dwords(u.reg_read(register)))[0]
                for name, register in self.NUMERIC_FIELDS[pc]}))

    def write(self, u, _access, address, size, value, _data):
        # Only selected presentation clocks; existing ReadyBuildingFixture
        # owns gameplay timer observation. Padding is deliberately excluded.
        for name, record in self.clock_records.items():
            if address in (record + 28, record + 36):
                self.timer_writes.append(dict(type=name,
                    field='start' if address == record + 28 else 'duration',
                    pc=hex(u.reg_read(UC_X86_REG_EIP)), size=size,
                    before=bytes(u.mem_read(address, size)).hex(),
                    after=(value & ((1 << (size * 8)) - 1)).to_bytes(size, 'little').hex()))

    def state(self):
        return sale_sidebar_state(self.m, self.m.get(self.m.er.HOUSE + 0x53B0),
                                  request_projection=True)

    def rng_refs(self, streams):
        result = {}
        for name, encoded in streams.items():
            digest = hashlib.sha256(bytes.fromhex(encoded)).hexdigest()
            self.rng_states.setdefault(digest, encoded)
            assert self.rng_states[digest] == encoded
            result[name] = digest
        return result

    def phase(self, name, operation, *, allow_construction=False, allow_detach=False):
        m = self.m
        before = self.state()
        cursor = m.get(m.get(0xA8B230) + 0x214)
        self.clock_records = self.records()
        marks = len(self.numeric), len(self.timer_writes)
        evidence = m.step(name, operation)
        after = self.state()
        assert evidence['native_code_unchanged']
        calls = []
        for row in evidence['native_calls']:
            if row['name'] not in self.call_names:
                continue
            call = dict(name=row['name'], caller=row['caller'], args_raw=row['args'])
            if 'return_eax' in row:
                call['return_eax_raw'] = row['return_eax']
                if row['name'] in ('type_base_duration', 'type_cost', 'available_money',
                                   'time_to_build', 'factory_prediction_rate'):
                    call['return_i32'] = struct.unpack('<i', c.dwords(row['return_eax']))[0]
            if row['name'] == 'eva':
                call['request'] = m.inputs.string(int(row['ecx'], 16))
            calls.append(call)
        constructors = sum(row['name'] in ('factory_constructor', 'infantry_constructor')
                           for row in calls)
        detaches = sum(row['name'] in ('object_detach', 'dispatch_pointer_expired') for row in calls)
        destructors = sum(row['name'] in ('infantry_destructor', 'factory_scalar_delete') for row in calls)
        identity_delta = m.get(m.get(0xA8B230) + 0x214) - cursor
        if not allow_construction:
            assert constructors == identity_delta == 0
        if not allow_detach:
            assert detaches == destructors == 0
        factory = m.get(m.er.HOUSE + 0x53B0)
        timers = [dict(field='start' if int(row['address'], 16) - factory == 0x2C else 'duration',
                       pc=row['pc'], size=row['size'], before=row['before'], after=row['after'])
                  for row in evidence['timer_writes']
                  if factory and int(row['address'], 16) - factory in (0x2C, 0x34)]
        before_rng, after_rng = evidence['before']['rng'], evidence['after']['rng']
        return dict(name=name, before=before, after=after,
            events=sale_sidebar_events(m), calls=calls,
            numeric_reads=self.numeric[marks[0]:],
            prediction_timer_writes=self.timer_writes[marks[1]:], factory_timer_writes=timers,
            constructor_count=constructors, detach_count=detaches, destructor_count=destructors,
            identity_delta=identity_delta, rng=dict(before=self.rng_refs(before_rng),
                after=self.rng_refs(after_rng), unchanged=before_rng == after_rng,
                requests=evidence['rng_requests'], ranged_advances=evidence['rng_advances']))


def pending_production_generate():
    """Original first-click controls, plus explicitly prepared numeric inputs."""
    m = sale_sidebar_setup(sale_sidebar_root())
    u, get = m.u, m.get
    observation = PendingProductionObservation(m)
    histories, numeric = [], []

    def prepare(*, frame=100, ahead=0):
        m.restore()
        u.mem_write(bc.FRAME, c.dwords(frame))
        # A test input, not proof of offline startup's initialized MaxAhead.
        u.mem_write(0xA8B550, c.dwords(ahead))
        assert not get(m.er.HOUSE + 0x53B0) and not get(0xA802C8)

    def click(name='E2', *, right=False, shift=False):
        return m.click(name, right=right, shift=shift, kind=16, tab=2)

    def advance_prediction(frames):
        u.mem_write(bc.FRAME, c.dwords(get(bc.FRAME) + frames))
        return m.invoke(0x6A8B30, c.STRIPS[2], 0, 0)

    def entries(phase):
        return {entry['name']: entry for entry in phase['after']['entries']}

    def assert_no_init(phase, request):
        entry = entries(phase)['E2']
        assert entry['request_state'] == request
        assert entry['predicted_stage'] == entry['predicted_rate'] == 0
        assert entry['prediction_timer']['duration'] == 0 and entry['increment'] == 1

    try:
        for shift in (False, True):
            prepare()
            phases = [observation.phase('left_idle', click),
                      observation.phase('first_predicted_due', lambda: advance_prediction(1)),
                      observation.phase('right_pending', lambda: click(right=True, shift=shift)),
                      observation.phase('repeat_right_pending', lambda: click(right=True, shift=shift)),
                      observation.phase('held_prediction_frozen', lambda: advance_prediction(5)),
                      observation.phase('receive_produce_then_suspend', m.receive, allow_construction=True)]
            assert entries(phases[0])['E2']['request_state'] == 1
            assert phases[0]['after']['factory'] is None
            assert entries(phases[2])['E2']['request_state'] == 2
            assert [event['kind'] for event in phases[2]['events']] == [14, 15]
            assert phases[3]['events'] == phases[2]['events']
            assert entries(phases[4])['E2']['predicted_stage'] == entries(phases[2])['E2']['predicted_stage']
            assert entries(phases[5])['E2']['request_state'] == 2
            assert entries(phases[5])['E2']['linked']
            assert phases[5]['after']['factory']['suspended']
            if not shift:
                phases += [observation.phase('left_held_resume', click),
                           observation.phase('receive_resume', m.receive),
                           observation.phase('same_frame_factory_ai',
                               lambda: m.invoke(0x4C9B20, get(m.er.HOUSE + 0x53B0))),
                           observation.phase('resumed_prediction_due', lambda: advance_prediction(2)),
                           observation.phase('next_factory_ai',
                               lambda: m.invoke(0x4C9B20, get(m.er.HOUSE + 0x53B0))),
                           observation.phase('strip_no_init', lambda: m.invoke(0x6A4F20, c.SIDEBAR, 0)),
                           observation.phase('strip_clear', lambda: m.invoke(0x6A81B0, c.STRIPS[2]))]
                assert not phases[7]['after']['factory']['suspended']
                assert_no_init(phases[-2], 1)
                assert phases[-1]['after']['total_entries'] == 0
            else:
                phases.append(observation.phase('held_no_init', lambda: m.invoke(0x6A4F20, c.SIDEBAR, 0)))
                assert_no_init(phases[-1], 2)
            histories.append(dict(name='pending_shift_right' if shift else 'idle_hold_resume',
                inputs=dict(max_ahead=0, initial_frame=100, shift=shift), phases=phases))

        prepare()
        phases = [observation.phase('left_idle', click),
                  observation.phase('receive_first_head', m.receive, allow_construction=True),
                  observation.phase('left_busy_other_type', lambda: click('FLAKT')),
                  observation.phase('receive_queued_copy', m.receive),
                  observation.phase('right_queued_only', lambda: click('FLAKT', right=True)),
                  observation.phase('receive_queued_abandon', m.receive)]
        assert entries(phases[2])['FLAKT']['request_state'] == 0
        assert not entries(phases[3])['FLAKT']['linked']
        assert [event['kind'] for event in phases[4]['events']] == [16]
        assert not any(row['name'] == 'eva' for row in phases[4]['calls'])
        assert not phases[5]['after']['factory']['suspended']
        histories.append(dict(name='busy_queued_only', inputs=dict(max_ahead=0, initial_frame=100), phases=phases))

        prepare()
        phases = [observation.phase('left_idle', click),
                  observation.phase('repeat_left_before_receive', click),
                  observation.phase('right_pending', lambda: click(right=True)),
                  observation.phase('unlinked_held_no_init', lambda: m.invoke(0x6A4F20, c.SIDEBAR, 0))]
        assert [event['kind'] for event in phases[1]['events']] == [14, 14]
        assert [event['kind'] for event in phases[2]['events']] == [14, 14, 15]
        assert_no_init(phases[-1], 2)
        assert not entries(phases[-1])['E2']['linked']
        histories.append(dict(name='repeat_left_and_unlinked_no_init',
            inputs=dict(max_ahead=0, initial_frame=100), phases=phases))

        for name, cost, speed, ahead, wallet in (
                ('ordinary_floor_not_ceil', 100, 1.0, 0, 10000),
                ('minimum_ahead_rounds_up', 100, 1.0, 54, 100),
                ('affordability_below', 100, 1.0, 54, 99),
                ('signed_low_product', 0x40000001, 1.0, 4, 0),
                ('signed_division_toward_zero', -1, 1.0, 53, -1),
                ('negative_duration_minimum', -100, 1.0, 0, 0),
                ('rate_upper_clamp', 100000, 1.0, 0, 10000)):
            prepare(ahead=ahead)
            pointer, rules = m.f.types['E2'], get(0x8871E0)
            u.mem_write(pointer + 0x610, c.dwords(cost))
            u.mem_write(rules + 0x1748, struct.pack('<d', speed))
            u.mem_write(m.er.HOUSE + 0x30C, c.dwords(wallet))
            base = m.invoke(0x711EE0, pointer)
            phase = observation.phase('left_numeric', click)
            numeric.append(dict(name=name, inputs=dict(cost=cost, build_speed_bits=struct.pack('<d', speed).hex(),
                max_ahead=ahead, wallet=wallet), base_duration_i32=struct.unpack('<i', c.dwords(base))[0], phase=phase))

        for name, predicted, speed in (('smooth_greater', 53, None),
                                       ('keep_actual_greater', 0, None),
                                       ('resumed_rate_256', 0, 100000.0)):
            factory, _product = sale_queue_start(m, 'E2')
            m.invoke(0x4FA910, m.er.HOUSE, 16, get(m.f.types['E2'] + 0xDF8), 0)
            record = observation.records()['E2']
            u.mem_write(record + 16, c.dwords(2))
            u.mem_write(record + 20, c.dwords(predicted))
            if speed is not None:
                u.mem_write(get(0x8871E0) + 0x1748, struct.pack('<d', speed))
            phase = observation.phase('left_resume_numeric', click)
            numeric.append(dict(name=name, inputs=dict(prepared_predicted_stage=predicted,
                prepared_request_state=2, prepared_build_speed_bits=None if speed is None else struct.pack('<d', speed).hex(),
                actual_stage_before=phase['before']['factory']['stage']), phase=phase))

        prepare()
        click()
        for _ in range(52):
            advance_prediction(1)
        phase = observation.phase('prediction_reaches_native_limit', lambda: advance_prediction(1))
        stopped = observation.phase('prediction_stays_stopped', lambda: advance_prediction(1))
        assert entries(phase)['E2']['predicted_stage'] == 53
        assert entries(phase)['E2']['predicted_rate'] == 0
        assert entries(stopped)['E2']['predicted_stage'] == 53
        numeric.append(dict(name='prediction_stops_at_53',
            inputs=dict(preparation='Whole Select followed by52 original StripAI visits; no assigned progress',
                        max_ahead=0, frame_increment=1), phase=phase, stopped_phase=stopped))
    finally:
        observation.close()
    return dict(schema=1, type_inputs=m.inputs.sale_type_inputs,
                histories=histories, numeric_controls=numeric, rng_states=observation.rng_states)


def pending_production_metadata():
    record = provenance(
        scope='Original idle E2 pending Left/Right/Shift, real event hold/resume, queued-only guard, prediction numeric inputs and NoInit/clear',
        assumptions=[
            'Reuse existing sale_sidebar_setup physical Russian Type/country readers, ReadyBuildingFixture VM/allocator/House/map/UI/audio owners and original selected skirmish mode5 store52E10F..52E119. No second VM/parser/admission owner; full process/menu/scenario loading is excluded.',
            'Whole Select6AAD00 creates request1 and pending Right creates2 before real local receive6474C7 executes PRODUCE14 then SUSPEND15. Real Factory/Infantry constructors, HouseBegin, Suspend1 and same-head Resume0 execute. Root production validation covers full payment/exit; native history visits only selected early FactoryAI/StripAI steps.',
            'MaxAhead0 and the numeric matrix MaxAhead values are explicitly prepared inputs, not initialized offline/network producer evidence. Numeric controls also prepare native E2 Cost, Rules BuildSpeed and wallet; original CostOf, AvailableMoney,711EE0 and selected comparison/rate arithmetic execute unchanged.',
            'Resumed numeric controls reuse actual paid E2+E2 preparation and original Suspend; request2/predicted stage and optional BuildSpeed are explicit component inputs. Actual Factory stage/debt/identity and Time_To_Build are observed, not reconstructed from Rust.',
            'The request_projection option reads actual record+10. Historical sale_sidebar_state default schemas retain their mislabeled request_state observation of record+24 dirty; those old fields are not request-state proof. New corpus alone uses the corrected explicit projection.',
            'Sidebar NoInit6A4F20 executes directly on linked request1/2 and unlinked request2, resetting progress while retaining request state. StripClear6A81B0 is a direct lifecycle boundary, not whole save stream/new-scenario execution. No native SAV compatibility or historical Factory-link persistence claim.',
            'Each phase observes unchanged original code, all three complete RNG buffers via deduplicated SHA references, native RNG requests/advances, actual constructor/identity/detach/destructor counts, meaningful Factory and predicted clock writes. Opaque timer padding is excluded. EVA remains an observed RET4 sink; no whole audio device RNG, Draw raster or pixel claim.',
        ],
        substitutions=['Inherited physical-cache/arena/CRT TLS/OS/COM/DirectSound/EVA services; explicit numeric/frame priors only. No Factory, admission, payment, clock, RNG or House method is replaced.'],
        entry_points=dict(select_action=0x6AAD00, local_receive=0x6474C7, house_begin=0x4FA350,
            house_suspend=0x4FA910, factory_suspend=0x4C9E60, factory_resume=0x4C9EA0,
            type_base_duration=0x711EE0, factory_prediction_rate=0x4C9FB0,
            strip_ai=0x6A8B30, sidebar_no_init=0x6A4F20, strip_clear=0x6A81B0))
    record['retail_inputs'] = sale_sidebar_metadata()['retail_inputs']
    return record


def hold_resume_control(m, name, *, poor_wallet=False, complete=False):
    """Real Select/receive around Suspend1 and same-head Resume0.

    Reuse the paid Russian fixture and its evidence collector. No held flag,
    rate, timer, stage, event payload or admission result is supplied.
    """
    factory, product = sale_queue_start(m, 'E2')
    if complete:
        advance_factory_to_stage(m, factory, 54)
    state, phase, phases = held_factory_observer(m, factory, product)

    initial = state()
    if complete:
        # This is an admitted House request, not a completed portrait click
        # (which enters the separate PLACE route). Resume must still refuse.
        phase('completed_same_type_begin', lambda: m.invoke(0x4FA350, m.er.HOUSE, 16,
            m.get(m.f.types['E2'] + 0xDF8), 0, 0))
    else:
        def frame(offset):
            m.u.mem_write(bc.FRAME, c.dwords(initial['frame'] + offset))
        frame(1)
        phase('right_select', lambda: m.click('E2', right=True, kind=16, tab=2))
        phase('suspend_receive', m.receive)
        frame(10)
        phase('paused_ai', lambda: m.invoke(0x4C9B20, factory))
        phase('rate_refresh', lambda: m.invoke(0x4CA6E0, m.er.HOUSE))
        frame(11)
        phase('refreshed_paused_ai', lambda: m.invoke(0x4C9B20, factory))
        if poor_wallet:
            # A declared external spending request through the original
            # wallet owner; no wallet, pause or affordability answer is set.
            phase('spend_wallet', lambda: m.invoke(0x4F9790, m.er.HOUSE,
                                                  m.get(m.er.HOUSE + 0x30C)))
        frame(12)
        phase('left_select', lambda: m.click('E2', kind=16, tab=2))
        phase('resume_receive', m.receive)
        phase('same_frame_ai', lambda: m.invoke(0x4C9B20, factory))
        frame(13)
        phase('next_frame_ai', lambda: m.invoke(0x4C9B20, factory))
    return dict(name=name, inputs=dict(poor_wallet=poor_wallet, completed=complete,
        power_output=m.get(m.er.HOUSE + 0x53A4), power_drain=m.get(m.er.HOUSE + 0x53A8)),
        initial=initial, phases=phases)


def hold_resume_generate():
    m = sale_sidebar_setup(sale_sidebar_root())
    return dict(schema=1, type_inputs=m.inputs.sale_type_inputs,
        controls=[hold_resume_control(m, 'affordable'),
                  hold_resume_control(m, 'poor_wallet', poor_wallet=True),
                  hold_resume_control(m, 'completed_same_type', complete=True)])


def hold_resume_metadata():
    record = provenance(
        scope='Three paid E2 same-head user-hold, rate-refresh, Resume0 and completed '
              'HouseBegin controls; no system hold, forced held-start or Draw.',
        assumptions=[
            'Reuse sale_sidebar_setup/sale_queue_start, physical Russian type/country '
            'readers, constructors, paid E2+E2 FIFO and the same ReadyBuildingFixture '
            'VM/allocator/map/platform priors. Existing native layout opens Infantry; '
            'no new fixture, parser, event dispatcher or admission substitution.',
            'Whole Select6AAD00 Right/Left uses actual Infantry kind16/tab2. Existing '
            'local receive6474C7..6474BF executes SUSPEND15/PRODUCE14 and original '
            'HouseSuspend4FA910/Suspend1 or HouseBegin4FA350/Resume0. Native bool '
            'arguments/results are low-byte values; high argument bytes and void '
            'EAX are raw receipts, not additional semantics.',
            'Existing call trace observes actual Time_To_Build6F47A0 entry/return '
            'for the held object during rate_refresh/resume_receive. Its signed '
            'EAX return is the rate/resume kernel input, never inferred from rate.',
            'Only frame inputs are advanced between visits. Original4CA6E0 runs with '
            'the joined prior House power99/0; it is the rate consumer boundary, not '
            'a proof of the full power-dirty producer. FactoryAI supplies every stage '
            'and completion. Meaningful timer start/duration are recorded; opaque+30 '
            'is excluded. No flags, timer, stage, balance or result is primed.',
            'Poor-wallet control executes original SpendMoney4F9790 for the current '
            'wallet before Left. This is an explicit external spending input, not '
            'a simulated purchase. Resume itself performs no debit. Complete control '
            'reaches54 through originalAI then directly requests HouseBegin(force0); '
            'a real completed Left click instead takes the separate PLACE route.',
            'Each measured phase compares all three complete Main/Scenario/MapGen '
            'RNG buffers and existing request/raw-advance hooks, identity cursor, '
            'constructor, detach and destructor entries. Setup/paid-start/completion '
            'preparation are outside those measured zero-draw boundaries. Existing '
            'EVA752700 remains an observed RET4 sink; no whole cue/device RNG claim.',
            'Raw+71 is observed, not reproduced by a semantic manual flag. '
            'Suspend0/system holds, Resume1/forced held-start, special+68 products, '
            'GoOnline/Offline scheduling, native save streams and rendered HOLD '
            'remain outside these ordinary held-E2 controls.',
        ],
        substitutions=['Inherited physical-cache, allocator, OS/COM/DirectSound and '
                       'observed EVA boundary services; original gameplay code unchanged.'],
        entry_points=dict(select_action=0x6AAD00, local_receive=0x6474C7,
            house_suspend=0x4FA910, factory_suspend=0x4C9E60, house_begin=0x4FA350,
            factory_resume=0x4C9EA0, factory_rate_rewrite=0x4CA6E0,
            time_to_build=0x6F47A0, factory_ai=0x4C9B20, spend_money=0x4F9790))
    record['retail_inputs'] = sale_sidebar_metadata()['retail_inputs']
    return record


def temporal_building_link(m, building):
    """Declared already-linked lone Temporal head, before its native notice tail."""
    temporal = m.f.allocate(0x50)
    m.u.mem_write(temporal, bytes(0x50))
    m.u.mem_write(temporal + 0x28, c.dwords(building))
    m.u.mem_write(building + 0x278, c.dwords(temporal))
    return temporal


def temporal_building_notice(m, temporal, building):
    """Original71B0EA..71B14E:270, pause, dirty power/House1FC and Mark2."""
    m.u.reg_write(UC_X86_REG_ESP, bc.SP)
    m.u.reg_write(UC_X86_REG_ESI, temporal)
    m.u.reg_write(UC_X86_REG_ECX, building)
    run_checked(m.u, 0x71B0EA, 0x71B14E, count=2_000_000,
                required_addresses=(0x4521C0,))
    m.call_trace.returned(0x71B14E, m.u.reg_read(UC_X86_REG_ESP))


def system_hold_observer(m, factory, product, building):
    """Extend the same paid-head observer with the reached system lifecycle."""
    m.watch.update({0x4521C0: ('building_pause', 0, 0),
                    0x452210: ('building_resume', 0, 0),
                    0x71ABC0: ('temporal_let_go', 0, 0),
                    0x445880: ('building_limbo', 0, 0),
                    0x44EBF0: ('building_detach_all', 1, 4),
                    0x6ABCD0: ('production_tab', 1, 4),
                    0x6A60A0: ('sidebar_flag_redraw', 1, 4),
                    0x4F42F0: ('display_flag_redraw', 1, 4),
                    0x4C9B20: ('factory_ai', 0, 0)})

    def extra_state():
        return dict(building_online=bool(m.u.mem_read(building + 0x660, 1)[0]),
            building_limbo=bool(m.u.mem_read(building + 0x81, 1)[0]),
            building_warped=bool(m.u.mem_read(building + 0x270, 1)[0]),
            building_temporal_head=bool(m.get(building + 0x278)),
            house_recheck=m.u.mem_read(m.er.HOUSE + 0x1FC, 1)[0],
            house_power_dirty=m.u.mem_read(m.er.HOUSE + 0x5778, 1)[0],
            sidebar_active_tab=m.get(c.SIDEBAR + 0x539C),
            sidebar_redraw=m.u.mem_read(c.SIDEBAR + 0x53A6, 1)[0],
            infantry_strip_redraw=m.u.mem_read(c.STRIPS[2] + 0x3C, 1)[0])

    return held_factory_observer(m, factory, product, extra_state=extra_state,
        capture_rng_requests=True,
        extra_calls=('house_update_factory_queue', 'building_pause', 'building_resume',
            'temporal_let_go', 'building_limbo', 'building_detach_all',
            'production_tab', 'sidebar_flag_redraw', 'display_flag_redraw',
            'factory_ai', 'start_next_queued', 'start_production',
            'infantry_constructor', 'factory_completed'))


def system_hold_lifecycle(m, name, *, user_hold=False, recovery='left'):
    factory, product = sale_queue_start(m, 'E2', survivor=True)
    backup = next(m.get(m.get(m.er.HOUSE + 0x6C) + i * 4)
        for i in range(m.get(m.er.HOUSE + 0x78))
        if (pointer := m.get(m.get(m.er.HOUSE + 0x6C) + i * 4)) != m.bar
        and m.get(pointer + 0x520) == m.f.types['NAHAND'])
    m.identities[backup] = 'backup_barracks'
    temporal = temporal_building_link(m, backup)
    state, phase, phases = system_hold_observer(m, factory, product, backup)
    initial, rng_before = state(), m.f.rng()

    def frame(offset):
        m.u.mem_write(bc.FRAME, c.dwords(initial['frame'] + offset))

    frame(1)
    if user_hold:
        phase('prior_user_right', lambda: m.click('E2', right=True, kind=16, tab=2))
        phase('prior_user_suspend_receive', m.receive)
    phase('temporal_building_notice', lambda: temporal_building_notice(m, temporal, backup))
    frame(2)
    phase('warp_alone_factory_ai', lambda: m.invoke(0x4C9B20, factory))
    phase('begin_online_barracks_sale', lambda: m.invoke(0x447110, m.bar, 0xFFFFFFFF))
    phase('online_barracks_limbo', lambda: m.invoke(0x445880, m.bar), allow_detach=True)
    frame(3)
    phase('offline_backup_factory_ai', lambda: m.invoke(0x4C9B20, factory))
    frame(10)
    phase('original_temporal_release', lambda: m.invoke(0x71ABC0, temporal))
    phase('release_house_suffix', lambda: house_tail(m))
    phase('release_rate_refresh', lambda: m.invoke(0x4CA6E0, m.er.HOUSE))
    frame(11)
    phase('release_alone_factory_ai', lambda: m.invoke(0x4C9B20, factory))
    if recovery == 'callback':
        phase('later_infantry_factory_callback', lambda: m.invoke(0x509140, m.er.HOUSE, 16, 0, 0))
        frame(12)
        phase('after_callback_factory_ai', lambda: m.invoke(0x4C9B20, factory))
    else:
        phase('same_head_left', lambda: m.click('E2', kind=16, tab=2))
        phase('same_head_resume_receive', m.receive)
        frame(12)
        phase('after_left_factory_ai', lambda: m.invoke(0x4C9B20, factory))
    return dict(name=name, inputs=dict(prior_user_hold=user_hold, recovery=recovery),
                initial=initial, phases=phases, rng_before=rng_before, rng_after=m.f.rng())


def system_resume_kernel(m, name, *, held_start=False, poor_wallet=False):
    factory, product = sale_queue_start(m, 'E2')
    state, phase, phases = system_hold_observer(m, factory, product, m.bar)
    initial, rng_before = state(), m.f.rng()
    m.u.mem_write(bc.FRAME, c.dwords(initial['frame'] + 1))
    phase('system_suspend_kernel', lambda: m.invoke(0x4C9E60, factory, 0))
    if poor_wallet:
        phase('external_spend_wallet', lambda: m.invoke(0x4F9790, m.er.HOUSE,
                                                       m.get(m.er.HOUSE + 0x30C)))
    m.u.mem_write(bc.FRAME, c.dwords(initial['frame'] + 2))
    phase('resume_kernel', lambda: m.invoke(0x4C9EA0, factory, int(held_start)))
    phase('same_frame_factory_ai', lambda: m.invoke(0x4C9B20, factory))
    m.u.mem_write(bc.FRAME, c.dwords(initial['frame'] + 3))
    phase('next_frame_factory_ai', lambda: m.invoke(0x4C9B20, factory))
    return dict(name=name, inputs=dict(held_start=held_start, poor_wallet=poor_wallet),
                initial=initial, phases=phases, rng_before=rng_before, rng_after=m.f.rng())


def system_forced_promotion(m, name, *, poor_wallet=False):
    factory, product = sale_queue_start(m, 'E2')
    advance_factory_to_stage(m, factory, 54)
    temporal = temporal_building_link(m, m.bar)
    state, phase, phases = system_hold_observer(m, factory, product, m.bar)
    initial, rng_before = state(), m.f.rng()
    phase('declared_successful_delivery_completion', lambda: m.invoke(0x4CA1A0, factory),
          allow_product_change=True)
    phase('temporal_building_notice', lambda: temporal_building_notice(m, temporal, m.bar))
    if poor_wallet:
        phase('external_spend_wallet', lambda: m.invoke(0x4F9790, m.er.HOUSE,
                                                       m.get(m.er.HOUSE + 0x30C)))
    m.u.mem_write(bc.FRAME, c.dwords(initial['frame'] + 1))
    phase('original_start_next_queued', lambda: m.invoke(0x4CA5A0, factory),
          allow_construction=True, allow_product_change=True, allow_rng=True)
    m.u.mem_write(bc.FRAME, c.dwords(initial['frame'] + 2))
    phase('promoted_factory_ai', lambda: m.invoke(0x4C9B20, factory))
    phase('original_temporal_release', lambda: m.invoke(0x71ABC0, temporal))
    phase('later_infantry_factory_callback', lambda: m.invoke(0x509140, m.er.HOUSE, 16, 0, 0))
    return dict(name=name, inputs=dict(poor_wallet=poor_wallet), initial=initial,
                phases=phases, rng_before=rng_before, rng_after=m.f.rng())


def system_hold_generate():
    m = sale_sidebar_setup(sale_sidebar_root())
    return dict(schema=1, type_inputs=m.inputs.sale_type_inputs,
        lifecycle=[system_hold_lifecycle(m, 'system_hold_then_left'),
                   system_hold_lifecycle(m, 'system_hold_then_callback', recovery='callback'),
                   system_hold_lifecycle(m, 'user_hold_survives_callback',
                                          user_hold=True, recovery='callback')],
        resume=[system_resume_kernel(m, f'Resume{int(held_start)}_poor{int(poor)}',
                                    held_start=held_start, poor_wallet=poor)
                for held_start in (False, True) for poor in (False, True)],
        forced_promotion=[system_forced_promotion(m, 'affordable'),
                          system_forced_promotion(m, 'poor_wallet', poor_wallet=True)])


def system_hold_metadata():
    record = provenance(
        scope='Paid E2 with offline backup: actual BuildingLimbo/House509140 system '
              'hold, Temporal release, Left/receive or callback recovery, Resume0/1 '
              'affordability, and actual forced StartNext promotion',
        assumptions=[
            'Reuse sale_sidebar_setup/sale_queue_start and the existing physical '
            'Russian Type/country readers, VM, sized counters, flat-map/platform '
            'priors, paid E2 and retained E2 successor. No pause, stage, timer, '
            'event, FindFactory, affordability or resume answer is supplied.',
            'A supplied lone-head Temporal link precedes original InitiateWarp '
            'notice slice71B0EA..71B14E (270, pause, power/House dirty and Mark2). '
            'Whole LetGo71ABC0, its real Building resume and Mark execute. Full '
            'CLEG construction, attack/CanWarpTarget and the earlier chain insertion '
            'are outside this supplied state boundary.',
            'Whole SellBack447110 and BuildingLimbo445880 execute on the online '
            'barracks; whole sale animation/crew/refund is outside this Limbo '
            'boundary. Original509140, CanBuild, FindFactory, Suspend0 and local '
            '6ABCD0/6A60A0 active-strip redraw execute without a gameplay sink.',
            'Release runs the actual House dirty suffix and separate original '
            'rate consumer4CA6E0 under prior House power99/0. A later509140 call '
            'is the explicit same-category building-callback seam; those controls '
            'do not execute a new Building construction/capture. The alternative '
            'whole Left Select/local receive executes actual same-head Resume0.',
            'Kernel matrix calls whole Suspend0/Resume0 or1 on an actual paid '
            'head. Poor wallets use original SpendMoney for the current wallet. '
            'Forced promotion first reaches54 via original FactoryAI, then calls '
            'CompletedProduction as a declared successful-delivery boundary '
            'without the preceding physical exit. Whole StartNext, forced '
            'HouseBegin, original offline fallback, constructor and Resume1 run.',
            'Every phase observes70/71/rate/meaningful timer/stage/balance, wallet, '
            'held identity, FIFO, lifecycle calls and all three complete0x3F4 '
            'RNG buffers. Full before/after streams are retained per control; '
            'phases retain their hashes and actual draw counts. Promotion is '
            'allowed its real constructor/identity/RNG effects, and declared '
            'CompletedProduction releases its old product. Pure hold phases '
            'assert zero draws and unchanged held/Factory identity. The existing '
            'RAW65C780 request states count its separate65C79D advance; the '
            'inherited65C84B hook counts only inline ranged advances. Techno '
            'constructor6F3254/6F3259 consumes the Scenario word stored at3C8. '
            'Timer+30 '
            'opaque stack padding and whole native SAV/device/Draw are excluded.',
        ],
        substitutions=['Inherited physical-cache, allocator, OS/COM/DirectSound and '
                       'observed EVA boundary services only; gameplay code unchanged.'],
        entry_points=dict(building_notice=0x71B0EA, temporal_let_go=0x71ABC0,
            building_pause=0x4521C0, building_resume=0x452210,
            sell_back=0x447110, building_limbo=0x445880,
            house_update_factory_queue=0x509140, factory_suspend=0x4C9E60,
            factory_resume=0x4C9EA0, select_action=0x6AAD00,
            local_receive=0x6474C7, factory_ai=0x4C9B20,
            start_next=0x4CA5A0, factory_completed=0x4CA1A0,
            house_begin=0x4FA350, factory_rate_rewrite=0x4CA6E0,
            time_to_build=0x6F47A0, spend_money=0x4F9790,
            production_tab=0x6ABCD0, sidebar_flag_redraw=0x6A60A0))
    record['retail_inputs'] = sale_sidebar_metadata()['retail_inputs']
    return record


def inputs(root, extra, *, unit_names=("MGTK",), extra_assets=(), absent_assets=()):
    m = c.ready_building_inputs(root)
    m.phase = 'setup_factory_loss_inputs'
    u = m.u
    assets = {name: (extra / name).read_bytes() for name in (*EXTRA_ASSETS, *extra_assets)}
    m.assets.update(assets)
    m.platform_audio.configure_transport(prepared_files={
        **{name + '.SHP': None for name in ABSENT_SHAPES},
        **dict.fromkeys(absent_assets), **assets,
    })
    for name, size, ctor in (
        ('GAWEAP', 0x2000, 0x45DD90), ('GATECH', 0x2000, 0x45DD90),
    ):
        pointer = m.alloc(size)
        m.invoke(ctor, pointer, (m.cstring(name),))
        m.types[name] = pointer
    for name in unit_names:
        if name not in m.types:
            pointer = m.alloc(0xF00)
            m.invoke(0x7470D0, pointer, (m.cstring(name),))
            m.types[name] = pointer
    art = sections((root / 'ARTMD.INI').read_bytes())
    selected = {name: values for name, values in art.items()
                if name in (*m.ready_art, *m.death_art_input_sections, 'RTNK', *unit_names)
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
                    if name in ('GAWEAP', 'GATECH', *unit_names, 'E1',
                                'General', 'Americans', 'Sides')})
        ini = m.alloc(0x40)
        u.mem_write(ini, bytes(u.mem_read(c.INI, 0x40)))
        u.mem_write(c.INI, art_cache)
        for name in ('GAWEAP', 'GATECH'):
            m.invoke(0x45FE50, m.types[name], (ini,))
        for name in unit_names:
            m.invoke(0x747620, m.types[name], (ini,))
    # Original selected TechnoType post-read pass, after all rule layers.
    for name in unit_names:
        u.reg_write(UC_X86_REG_ESP, c.INI_SP)
        u.reg_write(UC_X86_REG_EBP, m.types[name])
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


def aircraft_tech_inputs(root):
    """Selected ALL05 inputs on the same reader/VM owner as loss controls.

    Explicit layers keep the inherited terrain crop while excluding Battle
    and Hills gameplay rules. Constructors retain one live global type list.
    """
    layers=('RULESMD.INI','LANGRULE.INI','ALL05UMD.MAP')
    m=c.ready_building_inputs(root,layers=layers)
    m.phase='setup_aircraft_tech_inputs'
    u=m.u
    assets={name:(root/name).read_bytes() for name in (
        'GGAIRCBB.SHP','GTAIRCMK.SHP','FALC.VXL','FALC.HVA','BEAG.VXL','BEAG.HVA','VOXELS.VPL')}
    m.assets.update(assets)
    m.platform_audio.configure_transport(prepared_files={
        **{name:None for name in AIRCRAFT_ABSENT_FILES},**assets})
    assert all(row['file'] in layers for row in m.layers+m.engineer_layers+m.ready_layers)
    base=sections((root/'RULESMD.INI').read_bytes())
    scenario=sections((root/'ALL05UMD.MAP').read_bytes())
    # Existing stock country registry precedes map additions. Run original
    # registration, preserving Player's identity rather than its parent name.
    for data in (base,scenario):
        m.make_ini({'Countries':data.get('Countries',{})})
        u.reg_write(UC_X86_REG_ESP,c.INI_SP)
        u.reg_write(UC_X86_REG_ESI,c.INI)
        u.reg_write(UC_X86_REG_EDI,m.rules)
        run_checked(u,0x668C8B,0x668CDB)
    countries=[m.read32(m.read32(0xA83C9C)+i*4) for i in range(m.read32(0xA83CA8))]
    m.country=next(ptr for ptr in countries if m.string(ptr+0x24)=='Player')
    m.make_ini({'Player':scenario['Player']})
    m.invoke(0x511850,m.country,(c.INI,))
    # Aircraft Type's actual CRT precedes the first aircraft constructor.
    u.reg_write(UC_X86_REG_ESP,c.INI_SP)
    run_checked(u,0x4E73E0,0x4E7416)
    for name,size,ctor in (('GAAIRC',0x2000,0x45DD90),
                           ('ORCA',0xE10,0x41C8B0),('BEAG',0xE10,0x41C8B0)):
        ptr=m.alloc(size)
        m.invoke(ctor,ptr,(m.cstring(name),))
        m.types[name]=ptr
    return aircraft_tech_layers(m,root,layers)


def aircraft_tech_layers(m,root,layers):
    """Whole selected aircraft/airport readers on the retained input owner."""
    u=m.u
    art=sections((root/'ARTMD.INI').read_bytes())
    selected={name:values for name,values in art.items()
              if name in (*m.ready_art,*m.death_art_input_sections,'FALC','BEAG')
              or name.startswith('GAAIRC')}
    m.make_ini(selected)
    art_cache=bytes(u.mem_read(c.INI,0x40))
    general_keys={'PadAircraft','PrerequisiteRadar','BuildSpeed','MultipleFactory',
        'LowPowerPenaltyModifier','MinLowPowerProductionSpeed',
        'MaxLowPowerProductionSpeed','WallBuildSpeedCoefficient','SeparateAircraft'}
    m.tech_layers=[]
    for filename in layers:
        path=root/filename
        if not path.exists():
            assert filename=='LANGRULE.INI'
            m.tech_layers.append(dict(file=filename,absent=True))
            continue
        data=sections(path.read_bytes())
        cached={name:data[name] for name in ('GAAIRC','ORCA','BEAG') if name in data}
        if 'General' in data:
            cached['General']={k:v for k,v in data['General'].items() if k in general_keys}
        m.make_ini(cached)
        layer=m.alloc(0x40)
        u.mem_write(layer,bytes(u.mem_read(c.INI,0x40)))
        u.mem_write(c.INI,art_cache)
        for name in ('GAAIRC','ORCA','BEAG'):
            m.invoke(0x45FE50 if name=='GAAIRC' else 0x41CC20,m.types[name],(layer,))
        m.invoke(0x66D530,m.rules,(layer,),timeout_us=60_000_000)
        m.tech_layers.append(dict(file=filename,sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
            selected_sections=cached))
    return m


def aircraft_tech_setup(root, *, inputs=None):
    inputs=inputs or aircraft_tech_inputs(root)
    m=c.ReadyBuildingFixture(inputs,root,unit_startup=True)
    u,get,f=m.u,m.get,m.f
    # The runtime fork copies the same constructed Type objects and global
    # headers, not alternate type shells or category indexes.
    for address in (0xA8EB00,0xA8B218):
        u.mem_write(address,bytes(inputs.u.mem_read(address,24)))
    m.invoke(0x4E6BE0)  # original Aircraft instance vector before413D20
    for offset in (0x553C,0x558C):
        m.invoke(0x49F9B0,m.er.HOUSE+offset)
    for pointer in (0xB0B328,0xB0B408,0xB04978,0xB04910):
        m.invoke(0x69DCF0,pointer)
    m.invoke(0x6A4CE0)
    u.mem_write(c.SIDEBAR+0x53A5,b'\1')
    u.mem_write(0xA8B238,c.dwords(0))
    owned=[m.yard,m.bar]
    for name,coord in (('GAPOWR',(22,14)),('GAAIRC',(25,20))):
        pointer=f.allocate(0x1000)
        f.building(name,pointer,f.allocate(0x100),coord=coord)
        u.mem_write(pointer+0x74,b'\0')
        m.invoke(0x43F180,pointer,3)
        u.mem_write(pointer+0xAC,c.dwords(5))
        u.mem_write(pointer+0xB4,c.dwords(-1))
        u.mem_write(pointer+0x534,c.dwords(1))
        m.identities[pointer]=name
        owned.append(pointer)
        # Original tracking derives airport dock capacity from the read type.
        m.invoke(0x4FF700,m.er.HOUSE,pointer)
    f.house_prior(m.er.HOUSE,owned,native_building_counts=True)
    m.tech_owned=owned
    # Completed played-world buildings have already passed the existing
    # health sampler and House power consumer, rather than the inherited
    # generic fixture's unsampled99/0 cache. Both original owners execute.
    for pointer in owned:
        u.reg_write(UC_X86_REG_ESI,pointer)
        u.reg_write(UC_X86_REG_ESP,bc.SP)
        run_checked(u,0x440042,0x440074)
    u.reg_write(UC_X86_REG_ESI,m.er.HOUSE)
    u.reg_write(UC_X86_REG_ESP,bc.SP)
    run_checked(u,0x4F84D9,0x4F84F1)
    house_tail(m)
    assert not get(0xA802C8)
    m.save()
    return m


def aircraft_tech_history(m,raw_actions,actions):
    """Actual paid ORCA ->106 pair -> House/Strip -> native event cleanup."""
    m.restore()
    u,get,f=m.u,m.get,m.f
    # Read the selected original constructor registry, not a full-retail slot
    # assumption. The same vtable+40 getter supplies native subtype identity.
    aircraft_registry=[]
    for i in range(get(0xA8B228)):
        typ=get(get(0xA8B21C)+i*4)
        aircraft_registry.append(dict(id=f.string(typ+0x24),
            index=m.invoke(get(get(typ)+0x40),typ)))
    f.draws.clear()
    f.advances.clear()
    f.events.clear()
    m.calls.clear()
    m.writes.clear()
    rng_before=f.rng()
    def state(name):
        factory=m.invoke(0x500510,m.er.HOUSE,3,0,0)
        held=get(factory+0x58) if factory else 0
        events=[]
        for i in range(get(0xA802C8)):
            ptr=0xA802D4+((get(0xA802CC)+i)&127)*111
            raw=bytes(u.mem_read(ptr,111))
            events.append(dict(kind=raw[0],frame=struct.unpack_from('<I',raw,3)[0],
                rtti=struct.unpack_from('<I',raw,7)[0],index=struct.unpack_from('<I',raw,11)[0]))
        return dict(name=name,frame=get(bc.FRAME),
            tech_levels={n:struct.unpack('<i',u.mem_read(f.types[n]+0x634,4))[0] for n in ('ORCA','BEAG')},
            can_build={n:struct.unpack('<i',c.dwords(m.invoke(0x4F7870,m.er.HOUSE,f.types[n],0,1)))[0]
                       for n in ('ORCA','BEAG')},
            dirty=bool(u.mem_read(m.er.HOUSE+0x1FC,1)[0]),
            credits=struct.unpack('<i',u.mem_read(m.er.HOUSE+0x30C,4))[0],spent=get(m.er.HOUSE+0x2DC),
            factory=None if not factory else dict(exists=True,stage=get(factory+0x24),balance=get(factory+0x60),
                rate=get(factory+0x38),timer=[get(factory+n) for n in (0x2C,0x30,0x34)],
                held_type=m.type_names.get(get(held+0x6C4)) if held else None),events=events,
            rng={k:hashlib.sha256(bytes.fromhex(v)).hexdigest() for k,v in f.rng().items()})
    initial=state('initial')
    assert initial['can_build']=={'ORCA':1,'BEAG':0},initial
    index=get(f.types['ORCA']+0xDF8)
    m.invoke(0x6A6300,c.SIDEBAR,3,index)
    # The Factory pointer does not exist before BeginProduction. Observe its
    # writes without choosing an address, then retain only the actual timer.
    begin_writes=[]
    def begin_write(u,_access,address,size,value,_data):
        begin_writes.append(dict(pc=hex(u.reg_read(UC_X86_REG_EIP)),
            address=hex(address),size=size,before=bytes(u.mem_read(address,size)).hex(),
            after=(value&((1<<(size*8))-1)).to_bytes(size,'little').hex()))
    hook=u.hook_add(UC_HOOK_MEM_WRITE,begin_write)
    try:
        m.invoke(0x4FA350,m.er.HOUSE,3,index,0,0)
    finally:
        u.hook_del(hook)
    factory=m.invoke(0x500510,m.er.HOUSE,3,0,0)
    assert factory
    m.writes.extend(row for row in begin_writes if factory+0x2C<=int(row['address'],16)<factory+0x38)
    m.factories['ORCA']=factory
    m.identities[factory]='aircraft_factory'
    held=get(factory+0x58)
    m.identities[held]='held_orca'
    delete_entry=get(get(held)+0x20)
    m.watch[delete_entry]=('held_aircraft_scalar_delete',1,4)
    for _ in range(2000):
        u.mem_write(bc.FRAME,c.dwords(get(bc.FRAME)+1))
        m.invoke(0x4C9B20,factory)
        if get(factory+0x24):break
    else:raise AssertionError('ORCA did not reach a paid stage')
    stages=[state('paid_orca')]
    action=f.allocate(0x94)
    for position,row in enumerate(actions):
        u.mem_write(action,bytes(0x94))
        u.mem_write(action+0x2C,c.dwords(row['kind']))
        u.mem_write(action+0x54,row['inline_type'].encode()+b'\0')
        u.mem_write(action+0x90,c.dwords(row['value']))
        assert m.invoke(0x6DD8B0,action,m.er.HOUSE,0,0,0)&255==1
        stages.append(state('action_orca' if position==0 else 'action_beag'))
    house_tail(m)
    stages.append(state('house_recheck'))
    m.receive()
    stages.append(state('receive'))
    assert stages[-1]['factory'] is None and stages[-1]['can_build']=={'ORCA':0,'BEAG':1}
    assert stages[-2]['events']==[dict(kind=kind,frame=stages[0]['frame'],rtti=3,index=index)
                                  for kind in (16,46)]
    assert all(row['rng']==stages[0]['rng'] for row in stages[1:])
    assert any(row.get('name')=='held_aircraft_scalar_delete' for row in m.calls)
    assert f.code_unchanged()
    from tools.spatial_oracle.unit_source_scatter import SCENARIO
    rng_names={0x886B88:'main',SCENARIO+0x218:'scenario',0xABE890:'mapgen'}
    return dict(input=dict(map='ALL05UMD.MAP',house='Player House',country='Player',
        country_index=get(m.inputs.country+0xB8),house_tech=get(m.er.HOUSE+0x1D4),
        initial_frame=initial['frame'],paid_frame=stages[0]['frame'],
        house_power=[get(m.er.HOUSE+0x53A4),get(m.er.HOUSE+0x53A8)],
        native_aircraft_registry=aircraft_registry,
        initial_credits=initial['credits'],owned_buildings=['GACNST','GAPILE','GAPOWR','GAAIRC'],
        building_cells={'GACNST':[14,14],'GAPILE':[18,14],'GAPOWR':[22,14],'GAAIRC':[25,20]},
        geometry=dict(map_size=f.flat_map['size'],coordinate_bounds=f.flat_map['coordinate_bounds'],
            terrain='declared_flat_ground_crop',
            owned_buildings=[dict(type=name,cell=list(xy)) for name,xy in
                (('GACNST',(14,14)),('GAPILE',(18,14)),('GAPOWR',(22,14)),('GAAIRC',(25,20)))]),
        raw_actions=raw_actions,airport_docks=get(m.er.HOUSE+0x2D4)),
        initial_state=initial,stages=stages,rng_seed_states=rng_before,
        rng_requests=[dict(entry=x['entry'],site=x['site'],args=x['args'],
            stream=rng_names[x['this']],
            frame=x['frame'],result=x['result']) for x in f.draws],
        timer_writes=[dict(owner='aircraft_factory',offset=int(row['address'],16)-factory,
            **{k:v for k,v in row.items() if k!='address'}) for row in m.writes
            if factory+0x2C<=int(row['address'],16)<factory+0x38],
        held_delete_entry=f'{delete_entry:08X}',
        lifecycle_calls=[row['name'] for row in m.calls if row.get('name') in
            ('house_abandon','factory_abandon','factory_scalar_delete','object_detach',
             'house_pointer_expired','factory_pointer_expired','sidebar_abandon_factory',
             'held_aircraft_scalar_delete')],
        code_unchanged=True)


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


def naval_output_type_state(m, name):
    """Observe selected native fields; never decide exit or animation admission."""
    u, read, p = m.u, m.read32, m.types[name]
    signed = lambda off: struct.unpack('<i', u.mem_read(p + off, 4))[0]
    result = dict(name=name, pointer=p, native_name=m.string(p + 0x24),
                  strength=signed(0xA0), cost=signed(0x610),
                  naval=u.mem_read(p + 0xCCE, 1)[0],
                  deploy_time_f64_hex=bytes(u.mem_read(p + 0x3C8, 8)).hex())
    if name == 'GAYARD':
        result.update(factory=signed(0xEB8), foundation=signed(0xEF0),
            weapons_factory=u.mem_read(p + 0x16BD, 1)[0],
            refinery=u.mem_read(p + 0x16BB, 1)[0],
            weeder=u.mem_read(p + 0x16BC, 1)[0],
            production_anim=m.string(p + 0x116C),
            production_anim_damaged=m.string(p + 0x117C),
            idle_anim=m.string(p + 0x1414), idle_anim_damaged=m.string(p + 0x1424),
            idle_control=list(struct.unpack('<3i', u.mem_read(p + 0xF10, 12))))
    return result


def naval_output_inputs(root):
    """Add stock GAYARD to the existing stock Ship/Anim input-reader owner."""
    from tools.spatial_oracle import building_death_anims as death
    from tools.spatial_oracle.anytown_damage import next_family_native as identity

    # Existing grapple_inputs owns DEST/ASW, Ship GUID, splash and wake readers.
    # Its SQD and inherited Engineer/MTNK inputs are unused preparation, not a
    # second parser, scheduler or executable image assembled for this profile.
    m = death.grapple_inputs(root)
    m.reserve_arena(0x800000)
    u = m.u
    m.phase = 'setup_naval_output_inputs'
    art = sections((root / 'ARTMD.INI').read_bytes())
    anim_names = tuple(name for name in art if name.startswith('GAYARD_'))
    art_names = set(m.death_art_input_sections) | set(anim_names) | {
        'GAYARD', 'SQD', 'DEST', 'ASW', 'SQDG', 'WAKE1',
        'H2O_EXP1', 'H2O_EXP2', 'H2O_EXP3'}
    selected_art = {name: values for name, values in art.items() if name in art_names}
    # Reader already loaded physical cache members. RawFile transport admits
    # only those exact members; unexpected filename prerequisites fail closed.
    m.platform_audio.configure_transport(prepared_files=dict(m.assets))
    m.make_ini(selected_art)
    art_cache = bytes(u.mem_read(c.INI, 0x40))
    typ = m.alloc(0x2000)
    m.invoke(0x45DD90, typ, (m.cstring('GAYARD'),))
    m.types['GAYARD'] = typ
    defaults = naval_output_type_state(m, 'GAYARD')
    # Preserve selected physical registration order; original lookup/constructors
    # own type identities. This is a sparse catalog, not full Rules::Process.
    listed = sections((root / 'RULESMD.INI').read_bytes())['Animations'].values()
    ordered_anims = list(dict.fromkeys(name for name in listed if name in anim_names))
    ordered_anims.extend(name for name in anim_names if name not in ordered_anims)
    for name in ordered_anims:
        m.invoke(0x428B80, m.cstring(name))
    layers = []
    for filename in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'Hills.map'):
        path = root / filename
        if not path.exists():
            assert filename == 'LANGRULE.INI'
            layers.append(dict(file=filename, absent=True))
            continue
        raw = path.read_bytes()
        physical = sections(raw)
        m.make_ini({name: values for name, values in physical.items()
                    if name in ('GAYARD', 'General', 'AudioVisual')})
        ini = m.alloc(0x40)
        u.mem_write(ini, bytes(u.mem_read(c.INI, 0x40)))
        u.mem_write(c.INI, art_cache)
        admitted = m.invoke(0x45FE50, typ, (ini,)) & 255
        layers.append(dict(file=filename, sha256=hashlib.sha256(raw).hexdigest(),
            physical_section=physical.get('GAYARD', {}), admitted_al=admitted,
            retained=naval_output_type_state(m, 'GAYARD')))
    u.mem_write(c.INI, art_cache)
    anims = []
    for name in ordered_anims:
        p = m.invoke(0x428B80, m.cstring(name))
        mark = len(m.asset_loaded)
        result = m.invoke(0x427D00, p, (c.INI,))
        anims.append(dict(pointer=p, **m.result(name, p, result),
                          physical_art=selected_art[name],
                          asset_loads=m.asset_loaded[mark:]))
    # The optional parameter reuses this joined INI reader for TileSet ReadInt.
    # The existing TheaterReader still owns General/project in its separate
    # prerequisite VM; no second runtime or new theater parser is introduced.
    previous = identity.ri.ASSETS
    identity.ri.ASSETS = root
    try:
        theater = identity.theater(tile_reader=m)
    finally:
        identity.ri.ASSETS = previous
        u.mem_write(c.INI, art_cache)
    assert m.string(typ + 0x116C) and m.string(typ + 0x1414)
    assert u.mem_read(typ + 0xCCE, 1)[0] == 1
    assert u.mem_read(typ + 0x16BD, 1)[0] == 1
    assert u.mem_read(m.types['DEST'] + 0xCCE, 1)[0] == 1
    m.naval_output_theater = theater
    water = theater['globals'][0xAA0738]
    theater_receipt = {key:theater[key] for key in ('sha256', 'general', 'globals', 'count')}
    theater_receipt.update(tiles={i:theater['tiles'][i] for i in (0, water)},
        sets=[row for row in theater['sets'] if row['base'] in (0, water)])
    m.naval_output_receipts = dict(constructor_default=defaults, layers=layers,
        inherited_ship_layers=m.grapple_layers,
        selected_types=[naval_output_type_state(m, name) for name in ('GAYARD', 'DEST')],
        physical_art={name: selected_art[name] for name in ('GAYARD', 'DEST', 'ASW')},
        animations=anims, theater=theater_receipt,
        mission_control={name:bytes(u.mem_read(0xA8E3A8 + mission * 32, 32)).hex()
                         for name, mission in (('Guard', 5), ('Unload', 16))})
    return m


def naval_output_history(inputs, root, case):
    """Original naval ExitObject followed by the existing live Logic tick owner."""
    from types import SimpleNamespace
    from tools.spatial_oracle import building_death_anims as death
    from tools.spatial_oracle import engineer_repair_admission as er
    from tools.spatial_oracle.anim_bouncer_launch import constructor_state
    from tools.spatial_oracle.anytown_damage.mission import Mission
    from tools.spatial_oracle.anytown_damage.navigation_inputs import bind_primary_tiles
    from tools.spatial_oracle.anytown_damage.unit_unlimbo import initialize_unit_geometry
    from tools.spatial_oracle.refinery_dock import cell, place_building
    from tools.spatial_oracle.unit_source_scatter import SCENARIO
    from tools.rules_oracle.bridge_anim_inputs import Reader

    f = death.joined_fixture(inputs, 374, seed=case['seed'], building_ai=True,
                             unit_startup=True)
    u, read = f.u, f.read32
    view = SimpleNamespace(u=u, alloc=f.allocate, read32=read,
        invoke=lambda address, obj=0, args=(): bc.invoke(u, address, obj, *args))
    view.cstring = lambda name: Reader.cstring(view, name)
    rngs = dict(main=0x886B88, scenario=SCENARIO + 0x218, mapgen=0xABE890)
    # One declared seed boundary after inherited, unrelated fixture admission.
    # From here selected constructors/placement/Logic/expiry share the streams.
    for pointer in rngs.values():
        bc.invoke(u, 0x65C6D0, pointer, case['seed'])
    rng_initial = f.rng()
    draw_start, advance_start = len(f.draws), len(f.advances)
    allocation_start, event_start = len(f.allocations), len(f.events)
    u.mem_write(bc.FRAME, c.dwords(0))
    u.mem_write(0xA8B218, bytes(inputs.u.mem_read(0xA8B218, 24)))
    for entry in (0x69EBF0, 0x4E6BE0, 0x629190, 0x6B6C10):
        bc.invoke(u, entry, 0)
    for house in f.owner_houses:
        for offset in (0x553C, 0x558C):
            bc.invoke(u, 0x49F9B0, house + offset)
        bc.invoke(u, 0x748FD0, house + 0x328)
    geometry = initialize_unit_geometry(SimpleNamespace(m=view, u=u,
        resident=SimpleNamespace(rngs=rngs)))
    theater = inputs.naval_output_theater
    u.mem_write(0xA8ED28, c.dwords(0x7EB6D4, 0x44000000, theater['count'],
                                  1, theater['count'], 10))
    water = theater['globals'][0xAA0738]
    tiles = bind_primary_tiles(view, theater, (0, water), preserve_abstract_registry=True)
    # Keep the selected membership receipts, rather than duplicate the shared
    # binder's complete unrelated theater catalog in every control.
    tiles['archive_inputs']['members'] = [row for row in tiles['archive_inputs']['members']
                                          if row['tile'] in (0, water)]
    for address, value in theater['globals'].items():
        u.mem_write(address, c.dwords(value))
    water_cells = [(x, y) for y in range(13, 28) for x in range(14, 28)]
    # Only physical primary tile/subtile is supplied. Real Cell Recalc derives
    # land; real FNPC/UnitCanEnter/Unlimbo later decide whether DEST fits.
    for y in range(1, 32):
        for x in range(1, 32):
            p = cell(x, y)
            tile = water if (x, y) in water_cells else 0
            u.mem_write(p + 0x38, c.dwords(tile))
            u.mem_write(p + 0x11A, b'\0')
            bc.invoke(u, 0x47D2B0, p, 0xFFFFFFFF)
    assert all(read(cell(x, y) + 0xEC) == 2 for x, y in water_cells)

    yard, product = f.allocate(0x1000), f.allocate(0x1000)
    peer = f.allocate(0x1000) if case['sparse_contact'] else 0
    selected_anims, calls, timer_writes, transitions = set(), [], [], []
    trace = NativeCallTrace(u, read, calls)
    names = {yard:'yard', product:'product', peer:'contact_peer', er.HOUSE:'local_house'}
    specs = {
        0x443C60: ('exit_object', 2, 8),
        0x65AE30: ('in_radio_contact', 0, 0),
        0x5B35E0: ('queue_mission', 2, 8), 0x5B3570: ('commence', 0, 0),
        0x737BA0: ('unit_unlimbo', 2, 8),
        0x4D3780: ('foot_mark', 1, 4), 0x4DB810: ('foot_set_location', 1, 4),
        0x44D880: ('mission_unload', 0, 0),
        0x4A51F0: ('door_open', 2, 8), 0x4A5150: ('door_update', 0, 0),
        0x44D6A0: ('return_to_idle', 2, 8),
        0x451E40: ('clear_slot', 1, 4),
        0x44E8F0: ('building_pointer_expired', 2, 8),
        0x451B40: ('slot_expired', 1, 4),
    }
    observed_entries = {0x451890:'slot_allocate', 0x421EA0:'anim_constructor',
        0x4255B0:'anim_uninit', 0x5F65F0:'object_uninit',
        0x5F5280:'object_detach', 0x70D4A0:'techno_detach',
        0x7258D0:'announce_expiry', 0x725C70:'deferred_drain',
        0x426590:'anim_scalar_delete', 0x4228E0:'anim_destructor',
        0x5F3B80:'object_destructor', 0x56DC20:'find_nearby_passable'}

    def i32(address):
        return struct.unpack('<i', u.mem_read(address, 4))[0]

    def members(vector):
        return [read(read(vector + 4) + i * 4) for i in range(read(vector + 16))]

    def object_state(p):
        if not p:
            return None
        return dict(pointer=p, id=read(p + 0x10), health=i32(p + 0x6C),
            alive=u.mem_read(p + 0x90, 1)[0], limbo=u.mem_read(p + 0x81, 1)[0],
            marked=u.mem_read(p + 0x74, 1)[0], logic=u.mem_read(p + 0x98, 1)[0],
            in_playfield=u.mem_read(p + 0x3D5, 1)[0],
            location=list(struct.unpack('<3i', u.mem_read(p + 0x9C, 12))),
            current=i32(p + 0xAC), queued=i32(p + 0xB4), status=i32(p + 0xBC),
            ai_counter=read(p + 0xC4), mission_start=i32(p + 0xC0),
            dispatch_timer=[i32(p + 0xC8), i32(p + 0xD0)],
            tether=u.mem_read(p + 0x418, 1)[0],
            radio_capacity=read(p + 0xE8),
            contacts=[read(read(p + 0xE4) + i * 4) for i in range(read(p + 0xE8))])

    def state(*, with_rng=False):
        slots = [read(yard + 0x55C + i * 4) for i in range(21)]
        anims = members(0xA8E9A8)
        result = dict(frame=read(bc.FRAME), game_speed=i32(0xA8EB60), producer=object_state(yard),
            product=object_state(product), peer=object_state(peer),
            bstate=i32(yard + 0x534), queued_bstate=i32(yard + 0x538),
            ready=u.mem_read(yard + 0x6DD, 1)[0],
            stage=list(struct.unpack('<5i', u.mem_read(yard + 0xFC, 20))),
            door=dict(active=u.mem_read(yard + 0x368, 1)[0],
                opening=u.mem_read(yard + 0x369, 1)[0],
                timer=[i32(yard + 0x358), i32(yard + 0x360)],
                retained_duration=i32(yard + 0x364)),
            slots=[dict(slot=i, pointer=p, id=read(p + 0x10) if p else None,
                        type=f.string(read(p + 0xC8) + 0x24) if p else None)
                   for i, p in enumerate(slots)],
            selected_anims=[dict(pointer=p, type=f.string(read(p + 0xC8) + 0x24),
                in_anim_array=p in anims, in_logic=p in members(0x87F778),
                **constructor_state(u, p)) for p in sorted(selected_anims)],
            logic_members=members(0x87F778), deferred_members=members(0xB0F698),
            identity_cursor=read(SCENARIO + 0x214),
            lifecycle_bytes={f'{off:03X}':u.mem_read(yard + off, 1)[0]
                             for off in (0x6E4, 0x6E6, 0x6E8, 0x6E9, 0x6EA)},
            scenario_active=u.mem_read(0xA8E9A0, 1)[0],
            scenario_scope=read(0xA8E7AC))
        if with_rng:
            result['rng'] = f.rng()
        return result

    def record_returns(pc, sp):
        for index in trace.returned(pc, sp):
            row = calls[index]
            row['returned_frame'] = read(bc.FRAME)
            promoted = row['name'] == 'commence' and bool(row['return_eax'] & 255)
            if (row['name'] in ('exit_object', 'mission_unload') or promoted) and int(row['ecx'], 16) == yard:
                row['return_state'] = state(with_rng=row['name'] in ('exit_object', 'mission_unload'))
                transitions.append(dict(event=row['name'] + '_return', call=index,
                                        frame=read(bc.FRAME)))

    def observe(_u, pc, _size, _data):
        sp, this = u.reg_read(UC_X86_REG_ESP), u.reg_read(UC_X86_REG_ECX)
        record_returns(pc, sp)
        if pc in specs and (this in names or (pc == 0x4A51F0 and this == yard + 0x350)
                            or (pc == 0x4A5150 and this == yard + 0x350)):
            index = trace.entered(pc, sp, specs[pc])
            calls[index].update(frame=read(bc.FRAME), phase=f.phase,
                                receiver=names.get(this, 'yard_door'))
            if pc in (0x443C60, 0x44D880) and this == yard:
                calls[index]['entry_state'] = state(with_rng=True)
            if pc in (0x737BA0, 0x4DB810):
                calls[index]['coordinate'] = list(struct.unpack('<3i', u.mem_read(read(sp + 4), 12)))
            if pc == 0x737BA0:
                calls[index]['facing_input'] = read(sp + 8)
        elif pc in observed_entries:
            row = dict(name=observed_entries[pc], entry=f'{pc:08X}',
                caller=f'{read(sp):08X}', frame=read(bc.FRAME), phase=f.phase, this=this)
            if pc == 0x421EA0:
                typ = read(sp + 4)
                row['type'] = f.string(typ + 0x24)
                if row['type'].startswith('GAYARD_'):
                    selected_anims.add(this)
            elif pc == 0x451890:
                row['slot'] = read(sp + 8)
            elif pc == 0x451E40:
                row['slot'] = read(sp + 4)
            elif pc in (0x5F5280, 0x70D4A0):
                row['argument'] = read(sp + 4)
            elif pc == 0x7258D0:
                row['control_dl'] = u.reg_read(UC_X86_REG_EDX) & 255
            if (this in selected_anims or this == yard
                    or pc in (0x421EA0, 0x725C70, 0x56DC20)):
                calls.append(row)

    def write(_u, _access, address, size, value, _data):
        # Field-write receipts, not another clock/mission implementation.
        if (yard + 0xAC <= address < yard + 0xD4
                or yard + 0x350 <= address < yard + 0x36A
                or yard + 0x534 <= address < yard + 0x53C
                or yard + 0x55C <= address < yard + 0x5B0
                or address == yard + 0x6DD):
            timer_writes.append(dict(pc=f'{u.reg_read(UC_X86_REG_EIP):08X}',
                phase=f.phase, frame=read(bc.FRAME), offset=address - yard,
                size=size, before=bytes(u.mem_read(address, size)).hex(), value=value))

    hooks = [u.hook_add(UC_HOOK_CODE, observe), u.hook_add(UC_HOOK_MEM_WRITE, write)]

    def invoke(address, obj=0, *args):
        # Whole ExitObject plus the shared per-instruction observers can
        # exceed the default wall budget under host load. Keep the original
        # two-million-instruction bound and all endpoint/fault checks.
        result = bc.invoke(u, address, obj, *args,
            timeout_us=30_000_000 if address == 0x443C60 else 10_000_000)
        record_returns(RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
        return result

    # Mission.tick is the existing Logic/frame/drain owner. This adapter only
    # supplies its observation API names on the same joined VM and allocator.
    f.m = view
    f.resident = SimpleNamespace(trace=[], ptrs={(18, 17):cell(18, 17)},
        snapshot=lambda p: dict(tile=read(p + 0x38), land=read(p + 0xEC),
                                ground_head=read(p + 0xE4)))
    f.frames = []
    f.state = state
    try:
        f.phase = 'setup_selected_yard_constructor'
        f.building('GAYARD', yard, 0, constructor_only=True)
        constructor = dict(f.building_constructor)
        ctor_type, ctor_owner = read(yard + 0x520), read(yard + 0x21C)
        ctor_prefix = bytes(u.mem_read(yard, 8))
        ctor_flags = read(yard + 0x14)
        assert ctor_type == f.types['GAYARD'] and ctor_owner == er.HOUSE
        constructor.update(type_pointer=ctor_type, owner=ctor_owner)
        constructor['lifecycle_bytes'] = {
            f'{off:03X}':u.mem_read(yard + off, 1)[0]
            for off in (0x6E4, 0x6E6, 0x6E8, 0x6E9, 0x6EA)}
        assert constructor['lifecycle_bytes']['6E6'] == 0
        assert constructor['lifecycle_bytes']['6E8'] == 0
        assert constructor['lifecycle_bytes']['6EA'] == 1
        radio_ctor = dict(capacity=read(yard + 0xE8), items=read(yard + 0xE4),
            bytes_hex=bytes(u.mem_read(yard + 0xE0, 16)).hex())
        place_building(u, yard, (18, 17), constructor_abstract_flags=ctor_flags)
        # The inherited admitted-position helper carries its partial refinery
        # type/House prior. Keep this full constructor's actual identities, as
        # the existing non-constructor-only Building fixture does for its type.
        u.mem_write(yard + 0x520, c.dwords(ctor_type))
        u.mem_write(yard + 0x21C, c.dwords(ctor_owner))
        assert bytes(u.mem_read(yard, 8)) == ctor_prefix
        assert read(yard + 0x14) == ctor_flags
        assert bytes(u.mem_read(yard + 0xE0, 16)).hex() == radio_ctor['bytes_hex']
        # Explicit already-completed/admitted healthy producer boundary; native
        # constructor values/IDs and native radio storage are retained.
        # 6E4 is the declared completed-opening prior (native446A6E writer).
        # Retain constructor6E6=0 (damaged art),6E8=0 (no paid repair),
        # actual InitManagers-derived6E9, and constructor6EA=1 unchanged.
        # A Buildup name alone is not an established6E9 producer: full43B740
        # already calls442C40, whose442CB2..442CCF branch tests the original
        # type vtC0/465960 returned resource before writing6E9.
        for offset, value in ((0x90, 1), (0x81, 0), (0x74, 0), (0x98, 0), (0x6E4, 1)):
            u.mem_write(yard + offset, bytes([value]))
        u.mem_write(yard + 0x534, c.dwords(-1, -1))
        u.mem_write(yard + 0x544, c.dwords(read(yard + 0x6C)))
        assert invoke(0x43F180, yard, 3) & 255
        assert invoke(0x55BAA0, 0x87F778, yard, 0) & 255
        f.house_prior(er.HOUSE, (yard,), native_building_counts=True)
        f.phase = 'setup_original_idle'
        invoke(0x44D6A0, yard, 0, 1)
        # The supplied settled prior has the ordinary current Guard mission.
        # The selected ExitObject's later Unload is never manually commenced.
        invoke(0x5B3570, yard)
        if not read(yard + 0x5A4):
            # Explicit already-running Idle18 boundary if BeginIdle alone does
            # not initialize it. Execute the existing slot/Anim constructor
            # owner with the actual native-read slot name; no slot/Anim state
            # or subsequent replacement answer is supplied.
            invoke(0x451890, yard, f.types['GAYARD'] + 0x1414, 18, 0, 0, 0)
        assert read(yard + 0x5A4), 'Original stock Idle18 allocation failed; prerequisite open'
        # Native full constructors create both the held DEST and optional actual
        # peer. No Factory progress/payment, type mutation or object UID supplied.
        f.phase = 'setup_completed_held_dest_constructor'
        invoke(0x7353C0, product, f.types['DEST'], er.HOUSE)
        assert u.mem_read(product + 0x81, 1)[0] == 1
        if peer:
            invoke(0x7353C0, peer, f.types['DEST'], er.HOUSE)
            location = f.allocate(12)
            u.mem_write(location, c.dwords(26 * 256 + 128, 25 * 256 + 128, 0))
            assert invoke(0x737BA0, peer, location, 128) & 255
            capacity = max(3, radio_ctor['capacity'])
            contacts = f.allocate(4 * capacity)
            u.mem_write(contacts, c.dwords(*(peer if i == 2 else 0 for i in range(capacity))))
            u.mem_write(yard + 0xE4, c.dwords(contacts, capacity, 1))
        phases = [dict(label='settled_idle', state=state(with_rng=True))]
        f.phase = 'selected_original_exit'
        if case['inactive_scenario']:
            u.mem_write(0xA8E9A0, b'\0')
        second_arg = read(0xA8EF98)
        answer = invoke(0x443C60, yard, product, second_arg)
        phases.append(dict(label='exit_return', returned_eax=answer, state=state(with_rng=True)))
        visits = []
        if case['inactive_scenario']:
            assert answer == 0 and read(yard + 0xB4) == 16
            assert u.mem_read(product + 0x81, 1)[0] == 1
        elif case['sparse_contact']:
            assert answer == 2 and i32(yard + 0xB4) == -1
            Mission.tick(f, snapshot_cell=(18, 17))
            record_returns(RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
            phases.append(dict(label='next_logic_pass', state=state(with_rng=True)))
            assert not any(row['name'] == 'mission_unload' for row in calls)
        else:
            assert answer == 2 and i32(yard + 0xAC) == 5 and i32(yard + 0xB4) == 16
            initial_idle = read(yard + 0x5A4)
            seen_production, seen_guard, restored = False, False, False
            previous_signature = None
            for _ in range(400):
                Mission.tick(f, snapshot_cell=(18, 17))
                record_returns(RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
                snapshot = state()
                signature = (snapshot['producer']['current'], snapshot['producer']['queued'],
                    snapshot['producer']['status'], snapshot['bstate'], snapshot['ready'],
                    snapshot['door']['active'], snapshot['door']['opening'],
                    tuple((s['slot'], s['pointer']) for s in snapshot['slots']))
                if signature != previous_signature:
                    visits.append(snapshot)
                    previous_signature = signature
                if read(yard + 0x57C) and not seen_production:
                    seen_production = True
                    phases.append(dict(label='production_started', state=state(with_rng=True)))
                if seen_production and i32(yard + 0xAC) == 5 and not seen_guard:
                    seen_guard = True
                    phases.append(dict(label='guard_after_unload', state=state(with_rng=True)))
                if seen_production and not read(yard + 0x57C) and read(yard + 0x5A4):
                    restored = True
                    phases.append(dict(label='idle_restored', state=state(with_rng=True)))
                    assert read(yard + 0x5A4) != initial_idle
                    break
                f.frames.clear()
            assert seen_production and seen_guard and restored, 'Live stock18/8/18 chain not reached'
        assert not trace.pending, trace.pending
        assert not f.pending, f.pending
        assert f.code_unchanged()
        return dict(input=case, setup=dict(yard_constructor=constructor,
            radio_constructor=radio_ctor, radio_supplied_capacity=read(yard + 0xE8),
            sparse_array_is_synthetic=bool(peer and radio_ctor['capacity'] < 3),
            yard_cell=[18, 17], water_cells=[list(xy) for xy in water_cells],
            physical_tiles=tiles, geometry=geometry,
            inherited_actors=dict(gapowr=er.BLD, mtnk=f.source),
            exit_second_argument_global='00A8EF98', exit_second_argument=second_arg),
            phases=phases, visits=visits, transitions=transitions, calls=calls,
            timer_writes=timer_writes, allocations=f.allocations[allocation_start:],
            inherited_events=[row for row in f.events[event_start:]
                if row.get('event') in ('slot_allocate', 'anim_ctor', 'logic_add',
                    'logic_remove', 'scalar_delete', 'anim_dtor', 'object_dtor',
                    'arena_release', 'operational_edge', 'sound_play')],
            reached_entries=[f'{pc:08X}' for pc in sorted(f.executed)
                if pc in set(specs) | set(observed_entries)],
            rng_initial=rng_initial,
            rng_final=f.rng(), draws=f.draws[draw_start:],
            advances=f.advances[advance_start:], original_code_unchanged=True)
    finally:
        for hook in hooks:
            u.hook_del(hook)


def naval_output_generate():
    root = Path(os.environ['VERA20K_FACTORY_NAVAL_ASSETS'])
    inputs = naval_output_inputs(root)
    controls = [
        dict(name='stock_dest_no_rally', seed=31, sparse_contact=False, inactive_scenario=False),
        dict(name='sparse_actual_contact_slot2', seed=31, sparse_contact=True, inactive_scenario=False),
        dict(name='inactive_scenario_refuses_unlimbo', seed=31, sparse_contact=False, inactive_scenario=True),
    ]
    return dict(schema=1, source='unicorn/gamemd.exe',
        input_receipts=inputs.naval_output_receipts,
        controls=[naval_output_history(inputs, root, case) for case in controls])


def naval_output_metadata():
    result = provenance(
        scope='Stock GAYARD/DEST original ExitObject, deferred producer Unload and live18/8/18 animation cleanup',
        assumptions=[
            'Reuse existing grapple_inputs/joined_fixture readers, runtime, allocator, native Ship/COM factories, physical TMP binder and Mission.tick Logic/frame/drain owner. Theater General/project uses its existing separate prerequisite TheaterReader VM; TileSet ReadInt reuses the joined reader. This is not full process/Rules/map loading.',
            'Physical RULESMD, optional LANGRULE, MPBattleMD and Hills layers feed original selected type readers; ARTMD is unlayered. A sparse registered type/animation catalog and physical raw-file cache remain input boundaries. Constructor defaults and each GAYARD retained reader result are recorded.',
            'The producer is supplied healthy/completed/online/admitted at18,17, with6E4=1 as the446A6E completed-opening boundary. Constructor6E6=0,6E8=0,6EA=1 and actual InitManagers-derived6E9 are retained and recorded, without arming damaged art or paid repair. Full Building43B740, native Mark3, Logic admission and Idle transition execute; full construction/Unlimbo/GrandOpening is outside this settled boundary. Full Unit7353C0 creates a held limbo DEST. Factory payment/progress/House placement policy is excluded.',
            'Whole ExitObject443C60 receives the held DEST and literal active caller globalA8EF98. No rally is supplied. Original InRadioContact, QueueMission, FNPC, UnitUnlimbo, Mark0/SetLocation/Mark1 and live BuildingAI/Unload/Door/AnimAI/expiry/deferred drain execute without gameplay return substitution.',
            'One original seed boundary initializes all3 complete RNG objects after inherited Engineer/MTNK fixture admission and before selected constructors. Full buffers remain uninterrupted through selected phase boundaries; requests, raw ranged advances, allocations, IDs, mission/Door/slot writes and detach/expiry calls are observed. Each state reads actual GameOptions+A8EB60, inherited from the Construction fixture supplied stored speed3; complete options loading is excluded.',
            'Sparse actual-contact control uses a real constructed/on-map DEST at slot2. Native constructor capacity and supplied array capacity are both recorded; a capacity extension is explicitly synthetic, not a stock capacity/reachability claim. The failure control supplies ScenarioActive=false and observes real refusal; ordinary blocked-exit integration is a separate Rust/runtime bound.',
        ],
        substitutions=[
            'Inherited physical-cache lexical indexes/raw-byte IO, arena allocation/delete, CRT TLS, Windows COM/audio transport and existing presentation sinks are unchanged. Physical tile/subtile/map/House and settled producer priors are supplied; no FNPC, Unlimbo, radio, queue, mission, animation or expiry answer is supplied.',
            'Inherited GAPOWR/MTNK actors remain in the joined object world. Global House/Factory/Scenario phases outside existing Mission.tick object pass, native client command production, real payment and rendering/audio hardware output are excluded.',
        ],
        entry_points=dict(exit_object=0x443C60, in_radio_contact=0x65AE30,
            queue=0x5B35E0, commence=0x5B3570, building_ai=0x43FB20,
            mission_unload=0x44D880, idle=0x44D6A0, door_open=0x4A51F0,
            door_update=0x4A5150, unit_ctor=0x7353C0, unit_unlimbo=0x737BA0,
            foot_mark=0x4D3780, foot_set_location=0x4DB810,
            fnpc=0x56DC20, anim_ai=0x423AC0, pointer_expired=0x44E8F0,
            slot_expired=0x451B40, deferred_drain=0x725C70,
            logic_begin=0x55B5FF, logic_end=0x55B61B))
    root = Path(os.environ['VERA20K_FACTORY_NAVAL_ASSETS'])
    result['retail_inputs'] = {p.name:hashlib.sha256(p.read_bytes()).hexdigest()
                               for p in sorted(root.iterdir()) if p.is_file()}
    result['retail_inputs'].setdefault('LANGRULE.INI', None)
    return result


def main(argv=None):
    args = list(sys.argv[1:] if argv is None else argv)
    if '--pending-production' in args:
        args.remove('--pending-production')
        finish_vectors(pending_production_generate, Path(__file__).with_name('pending_production.json'),
                       provenance=pending_production_metadata, argv=args)
    elif '--naval-output' in args:
        args.remove('--naval-output')
        finish_vectors(naval_output_generate, Path(__file__).with_name('factory_naval_output.json'),
                       provenance=naval_output_metadata, argv=args)
    elif '--system-hold' in args:
        args.remove('--system-hold')
        finish_vectors(system_hold_generate, Path(__file__).with_name('factory_system_hold.json'),
                       provenance=system_hold_metadata, argv=args)
    elif '--hold-resume' in args:
        args.remove('--hold-resume')
        finish_vectors(hold_resume_generate, Path(__file__).with_name('factory_hold_resume.json'),
                       provenance=hold_resume_metadata, argv=args)
    elif '--sale-sidebar' in args:
        args.remove('--sale-sidebar')
        finish_vectors(sale_sidebar_generate, Path(__file__).with_name('factory_sale_sidebar.json'),
                       provenance=sale_sidebar_metadata, argv=args)
    elif '--sale-queue' in args:
        args.remove('--sale-queue')
        finish_vectors(sale_queue_generate, Path(__file__).with_name('factory_sale_queue.json'),
                       provenance=sale_queue_metadata, argv=args)
    else:
        finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata, argv=args)


if __name__ == '__main__':
    main()
