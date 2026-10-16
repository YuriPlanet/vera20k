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

from unicorn import UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_EDI, UC_X86_REG_EIP,
)

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
AIRCRAFT_ABSENT_FILES = (
    'GTAIRCBB.SHP', 'GAAIRCBB.SHP', 'GTAIRC_AD.SHP', 'GGAIRC_AD.SHP',
    'GTAIRC_BD.SHP', 'GGAIRC_BD.SHP', 'GTAIRC_CD.SHP', 'GGAIRC_CD.SHP',
    'GAAIRCMK.SHP', 'FALCTUR.VXL', 'FALCTUR.HVA', 'FALCBARL.VXL',
    'FALCBARL.HVA', 'BEAGTUR.VXL', 'BEAGTUR.HVA', 'BEAGBARL.VXL', 'BEAGBARL.HVA',
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


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
