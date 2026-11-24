"""Source-aware Scatter and additive original Infantry Can_Enter_Cell controls."""
from pathlib import Path
import sys
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.infantry_source_scatter import query

NEIGHBORS = [(10, 9), (11, 9), (11, 10), (11, 11),
             (10, 11), (9, 11), (9, 10), (9, 9)]

def generate():
    cases = [dict(),
             dict(raw=[[11, 9, 0x20, 0]]),
             dict(raw=[[11, 9, 0x1c, 0]]),
             dict(raw=[[x, y, 0x20, 0] for x, y in NEIGHBORS]),
             dict(blocked_terrain=[[11, 9]]),
             dict(blocked_terrain=NEIGHBORS),
             dict(cells=[[11, 9, 2, 0]]),
             dict(cells=[[11, 9, 1, 0]]),
             dict(cells=[[11, 9, 1, 0]], slopes=[[10, 10, 1]]),
             dict(cells=[[11, 9, 0, 0x100]], raw=[[11, 9, 0, 0x20]]),
             dict(cells=[[11, 9, 0, 0x100]], raw=[[11, 9, 0x20, 0]]),
             dict(cells=[[x, y, 0, 0x100] for x, y in NEIGHBORS]),
             dict(on_bridge=True, cells=[[10, 10, 0, 0x300], [11, 9, 0, 0x300]],
                  raw=[[11, 9, 0x20, 0]]),
             dict(on_bridge=True, cells=[[10, 10, 0, 0x300], [11, 9, 0, 0x300]],
                  raw=[[11, 9, 0, 0x20]])]
    return [query(dict(live_entry=True, **case)) for case in cases]

def priority_query(case):
    # Reuse the sole synthetic map/OS fixture; only the declared state changes.
    import hashlib
    import struct
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ESP
    from tools.native_oracle import SCRATCH, image_bytes, _sections
    from tools.spatial_oracle.unit_source_scatter import make_source_fixture, CELLS, SCENARIO
    from tools.spatial_oracle.unit_scatter_state import ACTOR, TYPE, LOCO, SP
    from tools.spatial_oracle.map_queries import dwords

    EXTRA = SCRATCH + 0xA0000
    HOUSE, ENEMY, WEAPON, BLOCKER, BTYPE, BLOCO = (
        EXTRA + offset for offset in (0, 0x6000, 0xC000, 0x10000, 0x11000, 0x13000))
    CELL = CELLS + (10 * 32 + 11) * 0x200
    bounds=[16,0,0,1,1] if case['rim'] else [16,-16,-16,64,64]
    u,call,read=make_source_fixture(dict(live_entry=True,seed=31,bounds=bounds))
    u.mem_map(EXTRA,0x30000)
    # Explicit supplied Infantry state; no constructor or input-reader claim.
    u.mem_write(ACTOR,dwords(0x7EB058));u.mem_write(TYPE,dwords(0x7EB610))
    u.mem_write(ACTOR+0x6C0,dwords(TYPE));u.mem_write(ACTOR+0x6C4,dwords(-1))
    u.mem_write(ACTOR+0x14,dwords(5));u.mem_write(ACTOR+0x21C,dwords(HOUSE))
    u.mem_write(ACTOR+0x3D5,b'\1');u.mem_write(ACTOR+0x418,b'\0')
    u.mem_write(ACTOR+0x5D4,dwords(0));u.mem_write(ACTOR+0x69C,dwords(0))
    u.mem_write(ACTOR+0x2DC,dwords(0));u.mem_write(ACTOR+0x5A4,dwords(0))
    call(0x75AA90,LOCO,[]);u.mem_write(LOCO+0xC,dwords(ACTOR))
    u.mem_write(ACTOR+0x674,dwords(LOCO+4))
    u.mem_write(TYPE+0x898,dwords(WEAPON if case['armed'] else 0))
    u.mem_write(TYPE+0x8B4,dwords(0))
    u.mem_write(WEAPON+0xA4,dwords(1));u.mem_write(WEAPON+0x98,dwords(0))
    u.mem_write(HOUSE+0x30,dwords(0));u.mem_write(ENEMY+0x30,dwords(1))
    u.mem_write(HOUSE+0x5788,dwords(1));u.mem_write(ENEMY+0x5788,dwords(2))
    u.mem_write(CELL+0x54,dwords(case['raw_owner'],-1))
    u.mem_write(CELL+0x124,dwords(case['raw_bits'],0))
    if case['occupant']!='none':
        kind=case['occupant_kind'];is_infantry=kind=='infantry';is_building=kind=='building'
        vtable,type_vtable={'infantry':(0x7EB058,0x7EB610),'unit':(0x7F5C70,0x7F6218),
            'building':(0x7E3EBC,0x7E4570),'aircraft':(0x7E22A4,0x7E2868)}[kind]
        u.mem_write(BLOCKER,dwords(vtable));u.mem_write(BTYPE,dwords(type_vtable))
        u.mem_write(BLOCKER+(0x520 if is_building else 0x6C0 if is_infantry else 0x6C4),dwords(BTYPE))
        if is_infantry:u.mem_write(BLOCKER+0x6C4,dwords(-1))
        u.mem_write(BLOCKER+0x14,dwords(5));u.mem_write(BLOCKER+0x21C,dwords(ENEMY))
        u.mem_write(BLOCKER+0x30,dwords(0));u.mem_write(BLOCKER+0x9C,dwords(2944,2688,0))
        call(0x75AA90 if is_infantry else 0x4AF540,BLOCO,[])
        u.mem_write(BLOCO+0xC,dwords(BLOCKER))
        u.mem_write(BLOCKER+0x674,dwords(BLOCO+4))
        if is_infantry:u.mem_write(BLOCO+0x34,bytes([case['occupant']=='moving']))
        elif case['occupant']=='moving':u.mem_write(BLOCO+0x34,dwords(3200,2688,0))
        u.mem_write(BLOCKER+0x6B6,b'\1')
        if case.get('gate'):u.mem_write(BTYPE+0x16B7,b'\1')
        u.mem_write(CELL+0xE4,dwords(BLOCKER))
    u.mem_write(0xA8E9A0,b'\1');u.mem_write(0xA8E7AC,dwords(case['priority']))
    u.mem_write(0xA8B238,dwords(case['mode']))
    call(0x65C6D0,0x886B88,[37]);call(0x65C6D0,0xABE890,[41])
    streams={'scenario':SCENARIO+0x218,'main':0x886B88,'mapgen':0xABE890}
    rng=lambda:{name:bytes(u.mem_read(at,0x3F4)).hex() for name,at in streams.items()}
    before=rng();seen=[]
    observed={0x51BF90,0x4D9C60,0x51C13A,0x51C144,0x578540,0x4DA1D0,
        0x51C482,0x51C4F5,0x4525F0,0x51C511,0x4F9A50,0x51C532,0x51C549,
        0x51C579,0x4F9A90,0x51C58D,0x51C650,0x75AB30,0x6F3970,0x4F9A10,
        0x51C7D0,0x51C7DF,0x51C7FA,0x51C841,0x51C879}
    def observe(_u,pc,_size,_data):
        if pc in (0x65C780,0x65C7E0,0x7C8E17,0x7C8B3D):
            raise AssertionError(('unexpected measured RNG/allocator',hex(pc)))
        if pc in observed:seen.append(hex(pc))
    hook=u.hook_add(UC_HOOK_CODE,observe)
    try:call(0x51BF90,ACTOR,[CELL,-1,-1,0,1])
    finally:u.hook_del(hook)
    result=u.reg_read(UC_X86_REG_EAX)
    assert u.reg_read(UC_X86_REG_ESP)==SP+24
    assert rng()==before
    raw=image_bytes();identity=[]
    for rva,offset,length,_,flags in _sections(raw):
        if length and flags&0x20000000:
            original=raw[offset:offset+length]
            assert bytes(u.mem_read(0x400000+rva,length))==original
            identity.append(dict(address=hex(0x400000+rva),length=length,sha256=hashlib.sha256(original).hexdigest()))
    assert list(struct.unpack('<I',u.mem_read(0x7EB058+0x1AC,4)))==[0x51BF90]
    return dict(input=case,result=result,calls=seen,
        counter_after=read(0xA8E7AC),mode_after=read(0xA8B238),
        rng_before=before,rng_after=rng(),rng_draws=[],
        native_executable_sections_unchanged=identity,measured_gameplay_substitutions=[])


def priority_generate():
    cases = []
    for mode in (0, 1):
        for priority in (0, 1, 2):
            for rim in (False, True):
                for armed in (False, True):
                    for occupant_kind in ('infantry', 'unit'):
                        for occupant in ('none', 'stationary', 'moving'):
                            cases.append(priority_query(dict(
                                mode=mode, priority=priority, rim=rim, armed=armed,
                                occupant=occupant, occupant_kind=occupant_kind,
                                raw_owner=-1, raw_bits=0)))
            for armed in (False, True):
                for occupant_kind in ('infantry', 'unit'):
                    for occupant in ('stationary', 'moving'):
                        cases.append(priority_query(dict(
                            mode=mode, priority=priority, rim=False, armed=armed,
                            occupant=occupant, occupant_kind=occupant_kind,
                            raw_owner=1, raw_bits=4 if occupant_kind == 'infantry' else 0x20)))
    structure_cases = []
    for mode in (0, 1):
        for priority in (0, 1, 2):
            for armed in (False, True):
                for kind in ('building', 'aircraft'):
                    structure_cases.append(priority_query(dict(
                        mode=mode, priority=priority, rim=False, armed=armed,
                        occupant='stationary', occupant_kind=kind,
                        raw_owner=-1, raw_bits=0)))
    for priority in (0, 1, 2):
        for armed in (False, True):
            structure_cases.append(priority_query(dict(
                mode=0, priority=priority, rim=False, armed=armed, gate=True,
                occupant='stationary', occupant_kind='building',
                raw_owner=-1, raw_bits=0)))
    return dict(schema_version=1, cases=cases, structure_cases=structure_cases)


def priority_metadata():
    return provenance(
        scope='Complete original Infantry51BF90 priority-counter admission: 192 Infantry/Unit cases and30 additional Building/Aircraft/closedGate controls. No whole crew lifecycle or retail producer claim.',
        entry_points={'infantry_entry': 0x51BF90, 'height_list': 0x4D9C60,
                      'usable_area_gate': 0x51C144, 'late_occupant_gate': 0x51C58D,
                      'building_gate': 0x51C4F5, 'gate_alliance': 0x4F9A50,
                      'current_alliance': 0x4F9A90, 'raw_owner_alliance': 0x4F9A10,
                      'damage_value': 0x6F3970, 'walk_ctor': 0x75AA90,
                      'drive_ctor': 0x4AF540, 'rng_ctor': 0x65C6D0},
        assumptions=[
            'Fresh synthetic32x32 originalCell map from existing unit_source_scatter owner; width16 and declared usable/rim rectangles. Ground level0, no overlay/tube/team/slave. Original Infantry/Type vtables and supplied Guard/Doing-1, SpeedType1, actor3D5=1, owner indexes0/1 and self-alliance masks1/2.',
            'Priority is the supplied DWORD A8E7AC0/1/2; mode is separately supplied actualDWORD A8B2380/1. Complete native51BF90 does not read game mode. Neither whole native counter producer nor whole Infantry constructor/retail reader is asserted.',
            'Armed supplies ordinary slot0WeaponDamage1/AmbientDamage0 and slot1NULL; unarmed suppliesNULL. Original GetWeapon and6F3970 execute, with no damage answer supplied.',
            'EnemyInfantry and Unit have original Walk/Drive construction plus declared stationary/moving state. List-only rows have rawowner-1/mask0; coherent rawenemyowner1/bit4or20 controls retain independent late admission. Priority must not globally change alliance.',
            'Additional original Building/Aircraft vtables pin the late switch. ClosedGate supplies type16B7true before native4525F0/reverseHouse4F9A50/IsArmed; no openGate, target admission, Selling, capture/garrison or complete object lifecycle claim.',
            'All3 complete0x3F4 RNG buffers are initialized by original65C6D0 and retained byte-identical. RNG/allocator calls fail the measured query. Entire original PE executable sections and actualInfantryvt1AC are checked unchanged.'],
        substitutions=[
            'Reuses unit_source_scatter/unit_scatter_state/map_queries/native_oracle fixture and OS Interlocked owner. No measured gameplay return, vtable dispatch or executable instruction is substituted.'])


def priority_source_paths():
    root = Path(__file__).resolve().parents[2]
    return {name: root / name for name in (
        'tools/native_oracle.py', 'tools/spatial_oracle/infantry_scatter_entry.py',
        'tools/spatial_oracle/unit_source_scatter.py',
        'tools/spatial_oracle/unit_scatter_state.py',
        'tools/spatial_oracle/map_queries.py')}


def scatter_metadata():
    return provenance(
        scope='Original Infantry Scatter through real Can_Enter_Cell: source selection, raw occupation, speed rows, height and bridge planes. Destination is observed; no movement continuation claim.',
        entry_points={'scatter': 0x51D0D0, 'entry': 0x51BF90,
                      'height_list': 0x4D9C60, 'height': 0x5F5F00},
        assumptions=['Inherits infantry_source_scatter setup with native Walk interface, source, RNG, map lookups and widened synthetic playfield.',
                     'No overlay or object list members, no Tubes; owner indices are -1. Land0 has nine nonzero speed rows, Land1 nine zero rows. Raw masks, cell levels/slopes/flags and actor OnBridge are declared inputs.',
                     'Native class +1AC and +1B0 execute unchanged. These cases do not establish overlay/object-list parity or retail boundary reachability.'],
        substitutions=['QueueMission and SetDestination are argument-checking observers.'])


if __name__ == '__main__':
    argv = list(sys.argv[1:])
    if '--priority' in argv:
        argv.remove('--priority')
        finish_vectors(priority_generate,
            Path(__file__).with_name('infantry_scatter_entry_priority.json'),
            provenance=priority_metadata, argv=argv, source_paths=priority_source_paths())
    else:
        finish_vectors(generate, Path(__file__).with_suffix('.json'),
            provenance=scatter_metadata, argv=argv,
            source_paths={'tools/spatial_oracle/infantry_scatter_entry.py': Path(__file__)})
