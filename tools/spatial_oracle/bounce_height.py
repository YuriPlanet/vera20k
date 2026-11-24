"""Bounded original BounceClass terrain transactions; no native code replacements."""
import struct
import hashlib
import json
import sys
from pathlib import Path
from functools import lru_cache
from tools.native_slope import initialize_slope_tables, slope_matrices
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import NativeCallTrace, load_image, run_checked, finish_vectors, provenance, NATIVE_FPCW

MEM=0x21000000
TABLE=MEM
CELLS=MEM+0x100000
BODY=MEM+0x120000
SP=MEM+0x1FF000
STOP=0x30000000
DUMMY=0xABDC50

@lru_cache(maxsize=1)
def matrices(): return slope_matrices()

def words(*v): return struct.pack('<'+'I'*len(v),*(x&0xFFFFFFFF for x in v))
def cases():
    def case(name,position=(128,128,420),velocity=(0,0,-8),cells=None,**kwargs):
        return dict(name=name,position=position,velocity=velocity,gravity=1.4,elasticity=0.8,
                    cells=cells if cells is not None else [(0,0,0,0,256,-1)],
                    dummy_level=0,dummy_slope=0,dummy_flags=0,**kwargs)
    result = [case('deck_fall'),case('deck_rest',velocity=(0,0,0)),case('deck_rise',position=(128,128,410),velocity=(0,0,12)),
            case('ordinary_floor',position=(128,128,4),cells=[(0,0,0,0,0,-1)]),
            case('negative_fraction',position=(-1,128,420)),
            case('dummy_ground',position=(-300,128,420),cells=[]),
            case('wall',position=(128,128,40),cells=[(0,0,0,0,0,2)]),
            case('sandbag',position=(128,128,40),cells=[(0,0,0,0,0,0)]),
            case('both_missing_retained',position=(-510,128,40),velocity=(-8,0,-4),cells=[]),
            case('old_only_structural',position=(255,128,420),velocity=(8,0,-8)),
            case('new_only_structural',position=(255,128,420),velocity=(8,0,-8),cells=[(1,0,0,0,256,-1)]),
            case('low_wood_no_deck',cells=[(0,0,0,0,0,237)])]
    for level in (-128,-1,0,1,127):
        c=case(f'signed_level_{level}',position=(128,128,level*104+2),cells=[(0,0,level,0,0,-1)])
        result.append(c)
    for slope in range(21):
        result.append(case(f'slope_{slope}',position=(128,128,1000),velocity=(3,-4,-8),
                           cells=[(0,0,0,slope,0,-1)]))
    for flag in (0,256):
        c=case(f'dummy_live_height_{flag}',position=(-510,128,630),velocity=(-8,0,-8),cells=[])
        c.update(dummy_level=2,dummy_flags=flag);result.append(c)
    for overlay in (26,243):
        result.append(case(f'wall_{overlay}',position=(128,128,40),cells=[(0,0,0,0,0,overlay)]))
    for objects in (['building'],['undeploy'],['large_undeploy'],['nonbuilding'],['undeploy','building'],['building','undeploy']):
        result.append(case('objects_'+'_'.join(objects),position=(128,128,40),
                           cells=[(0,0,0,0,0,-1)],objects=objects))
    return result


def terrain_machine(case, *, native_startup=False):
    uc=Uc(UC_ARCH_X86,UC_MODE_32);load_image(uc)
    uc.mem_map(MEM,0x200000);uc.mem_map(STOP,0x1000)
    uc.reg_write(UC_X86_REG_FPCW,NATIVE_FPCW)
    uc.mem_write(0x822D80,struct.pack('<H',NATIVE_FPCW))
    uc.mem_write(0x87F924,words(TABLE,0x40000));uc.mem_write(0x89E7C0,words(104))
    uc.mem_write(0x89C778,words(104))
    uc.mem_write(SP,words(STOP));uc.reg_write(UC_X86_REG_ESP,SP)
    run_checked(uc,0x439610,STOP)
    assert struct.unpack('<i',uc.mem_read(0x89C76C,4))[0]==416
    if native_startup:
        initialize_slope_tables(uc, SP, STOP)
    else:
        for slope,matrix in enumerate(matrices()):
            uc.mem_write(0xB45188+48*slope,struct.pack('<12I',*matrix))
    uc.mem_write(DUMMY,bytes(0x200));uc.mem_write(DUMMY+0x24,struct.pack('<hh',*case.get('dummy_coord',(1234,-2345))))
    uc.mem_write(DUMMY+0x44,words(-1));uc.mem_write(DUMMY+0x11B,bytes([case['dummy_level']&255,case['dummy_slope']]))
    uc.mem_write(DUMMY+0x140,words(case['dummy_flags']))
    for i,(x,y,level,slope,flags,overlay) in enumerate(case['cells']):
        ptr=CELLS+i*0x200
        uc.mem_write(ptr+0x24,struct.pack('<hh',x,y));uc.mem_write(ptr+0x44,words(overlay))
        uc.mem_write(ptr+0x11B,bytes([level&255,slope]));uc.mem_write(ptr+0x140,words(flags))
        uc.mem_write(TABLE+(y*512+x)*4,words(ptr))
    uc.mem_write(0xA8E9A0,b'\x01') # running game:47C520 list admission
    objects=case.get('objects',[])
    for i,kind in enumerate(objects):
        ptr=BODY+0x2000+i*0x2000;typ=ptr+0x800
        uc.mem_write(ptr,words(0x7F5C70 if kind=='nonbuilding' else 0x7E3EBC))
        uc.mem_write(ptr+0x30,words(ptr+0x2000 if i+1<len(objects) else 0))
        uc.mem_write(ptr+0x520,words(typ))
        uc.mem_write(typ+0x408,words(typ if 'undeploy' in kind else 0))
        uc.mem_write(typ+0xEF0,words(3 if kind=='large_undeploy' else 0))
    if objects:uc.mem_write(CELLS+0xE4,words(BODY+0x2000))
    return uc


def execute(case):
    uc = terrain_machine(case)
    uc.mem_write(BODY,struct.pack('<ddd6f8f',case['elasticity'],case['gravity'],0,
        *case['position'],*case['velocity'],0,0,0,1,0,0,0,1))
    events=[];sampled_ground=[]
    def observe(u,address,_size,_data):
        if address==0x439BF8: sampled_ground.append(struct.unpack("<i",words(u.reg_read(UC_X86_REG_EAX)))[0])
        if address in (0x578080,0x565730):
            sp=u.reg_read(UC_X86_REG_ESP);arg=struct.unpack('<I',u.mem_read(sp+4,4))[0]
            events.append([hex(address),*struct.unpack('<iii',u.mem_read(arg,12))])
        elif address in (0x47C520,0x480510):
            receiver=u.reg_read(UC_X86_REG_ECX)
            events.append([hex(address),*struct.unpack('<hh',u.mem_read(receiver+0x24,4)),receiver==DUMMY])
    uc.hook_add(UC_HOOK_CODE,observe)
    uc.mem_write(SP,words(STOP));uc.reg_write(UC_X86_REG_ESP,SP);uc.reg_write(UC_X86_REG_ECX,BODY)
    run_checked(uc,0x439B00,STOP,count=1000000,required_addresses=(0x578080,0x439A10))
    return dict(**case,ground_z=sampled_ground[0],outcome=uc.reg_read(UC_X86_REG_EAX),
        position_bits=struct.unpack('<3I',uc.mem_read(BODY+0x18,12)),
        velocity_bits=struct.unpack('<3I',uc.mem_read(BODY+0x24,12)),events=events,
        dummy_coord=struct.unpack('<hh',uc.mem_read(DUMMY+0x24,4)))

def _cliff_input(name, cells, **kwargs):
    return dict(name=name, cells=cells, dummy_coord=[1234,-2345], dummy_level=0, dummy_slope=0,
                dummy_flags=0, **kwargs)


def _cliff_cases():
    directions = [('north', 0, -1), ('east', 1, 0), ('south', 0, 1), ('west', -1, 0)]
    for name, dx, dy in directions:
        yield _cliff_input(name, [(10, 10, 0, 0, 0, -1),
                                 (10+dx, 10+dy, 2, 0, 0, -1)], delta=[dx, dy])
    for name, dx, dy in [('ne', 1, -1), ('se', 1, 1), ('sw', -1, 1), ('nw', -1, -1)]:
        for sides, a, b in [('neither', 0, 0), ('a_only', 2, 0),
                            ('b_only', 0, 2), ('both', 2, 2)]:
            ax, by = (dx, dy) if dx == dy else (dy, dx)
            yield _cliff_input(f'{name}_{sides}',
                [(10, 10, 0, 0, 0, -1), (10+dx, 10+dy, 2, 0, 0, -1),
                 (10+ax, 10, a, 0, 0, -1), (10, 10+by, b, 0, 0, -1)], delta=[dx, dy])


class _CliffObservation:
    """Observe original calls and matched returns; never replace a body."""
    def __init__(self, uc, *, allowed_cells=None):
        self.uc = uc
        self.calls = NativeCallTrace(uc, self.read32)
        self.lookups, self.directions, self.matrices, self.transforms = [], [], [], []
        self.updates = []
        self.allowed_cells = allowed_cells
        self.visits = set()
        self.hook = uc.hook_add(UC_HOOK_CODE, self.observe)

    def read32(self, ptr):
        return struct.unpack('<I', self.uc.mem_read(ptr, 4))[0]

    def observe(self, uc, pc, _size, _data):
        sp = uc.reg_read(UC_X86_REG_ESP)
        self.visits.add(pc)
        for index in self.calls.returned(pc, sp):
            row = self.calls.calls[index]
            eax = uc.reg_read(UC_X86_REG_EAX)
            if row['name'] in ('world', 'packed'):
                row['observation'].update(selected=list(struct.unpack('<hh', uc.mem_read(eax+0x24, 4))),
                                          dummy=eax == DUMMY)
                if self.allowed_cells is not None:
                    assert eax != DUMMY and tuple(row['observation']['selected']) in self.allowed_cells, row['observation']
            elif row['name'] == 'direction':
                row['observation']['result'] = struct.unpack('<i', words(eax))[0]
            elif row['name'] == 'matrix':
                row['observation']['matrix_bits'] = list(struct.unpack('<12I', uc.mem_read(eax, 48)))
            elif row['name'] == 'transform':
                row['observation']['output_bits'] = list(struct.unpack('<3I', uc.mem_read(eax, 12)))
            elif row['name'] == 'update':
                row['observation'].update(outcome=eax,
                    final_body_hex=bytes(uc.mem_read(int(row['ecx'], 16), 0x50)).hex())
        specs = {0x565730: ('world', 1, 4), 0x5657A0: ('packed', 1, 4),
                 0x4848B0: ('direction', 1, 4), 0x755C60: ('matrix', 0, 0),
                 0x5AFB80: ('transform', 1, 4), 0x439B00: ('update', 0, 0)}
        if pc not in specs:
            return
        index = self.calls.entered(pc, sp, specs[pc])
        row = self.calls.calls[index]
        if pc in (0x565730, 0x5657A0):
            point = self.read32(sp+4)
            fmt, size = ('<3i', 12) if pc == 0x565730 else ('<2h', 4)
            observation = dict(kind=row['name'], requested=list(struct.unpack(fmt, uc.mem_read(point, size))))
            self.lookups.append(observation)
        elif pc == 0x4848B0:
            receiver = uc.reg_read(UC_X86_REG_ECX)
            point = self.read32(sp+4)
            observation = dict(receiver=list(struct.unpack('<hh', uc.mem_read(receiver+0x24, 4))),
                               receiver_is_dummy=receiver == DUMMY,
                               world_coord=list(struct.unpack('<3i', uc.mem_read(point, 12))))
            self.directions.append(observation)
        elif pc == 0x755C60:
            observation = dict(direction=uc.reg_read(UC_X86_REG_EDX))
            self.matrices.append(observation)
        elif pc == 0x439B00:
            observation = dict(initial_body_hex=bytes(uc.mem_read(uc.reg_read(UC_X86_REG_ECX), 0x50)).hex())
            self.updates.append(observation)
        else:
            observation = dict(input_bits=list(struct.unpack('<3I', uc.mem_read(self.read32(sp+4), 12))),
                matrix_bits=list(struct.unpack('<12I', uc.mem_read(uc.reg_read(UC_X86_REG_EDX), 48))))
            self.transforms.append(observation)
        row['observation'] = observation

    def finish(self):
        self.calls.returned(STOP, self.uc.reg_read(UC_X86_REG_ESP))
        assert not self.calls.pending, self.calls.pending


def _cliff_call(uc, observer, entry, ecx, arguments=(), edx=None, required=()):
    uc.mem_write(SP, words(STOP, *arguments))
    uc.reg_write(UC_X86_REG_ESP, SP)
    uc.reg_write(UC_X86_REG_ECX, ecx)
    if edx is not None:
        uc.reg_write(UC_X86_REG_EDX, edx)
    run_checked(uc, entry, STOP, count=1000000, required_addresses=(entry, *required))
    # Observe the stop boundary explicitly: run_checked stops before its hook.
    observer.observe(uc, STOP, 0, None)
    return uc.reg_read(UC_X86_REG_EAX)


def cliff_vectors():
    source_path = Path(__file__).parents[1] / 'projectile_oracle/typed_warhead_debris.json'
    source = json.loads(source_path.read_text())
    physical = source['initial']['tire']
    tire = source['rows'][0]['voxels'][0]
    base_body = bytes.fromhex(tire['bounce_hex'])
    assert len(base_body) == 0x50 and tire['type'] == 'TIRE'
    assert struct.unpack('<Q', base_body[:8])[0] == int(physical['physics_bits']['elasticity'], 16)
    rng_input = source['rows'][0]['rng_after']
    rng_addresses = {'main': 0x886B88, 'scenario': MEM+0x130218, 'mapgen': 0xABE890}

    def prepare(case):
        uc = terrain_machine(case, native_startup=True)
        uc.mem_write(0xA8B230, words(MEM+0x130000))
        for name, address in rng_addresses.items():
            uc.mem_write(address, bytes.fromhex(rng_input[name]))
        return uc

    def rng_hashes(uc):
        return {name: hashlib.sha256(uc.mem_read(address, 0x3F4)).hexdigest()
                for name, address in rng_addresses.items()}

    def finish(uc, observer, before):
        observer.finish()
        after = rng_hashes(uc)
        assert before == after
        return dict(lookup_trace=observer.lookups, direction_calls=observer.directions,
            matrix_calls=observer.matrices, transform_calls=observer.transforms,
            final_dummy_coord=list(struct.unpack('<hh', uc.mem_read(DUMMY+0x24, 4))),
            rng_before_sha256=before, rng_after_sha256=after,
            reached=[f'{address:08X}' for address in
                (0x4848B0, 0x755C60, 0x5AFB80, 0x439B00, 0x439FB7)
                if address in observer.visits])

    direction_cases, update_cases = [], []
    cases = list(_cliff_cases())
    direct = [dict(case, receiver=[10+case['delta'][0], 10+case['delta'][1]],
                   world_coord=[2688, 2688, 0]) for case in cases]
    direct += [
        _cliff_input('same_cell', [(10,10,0,0,0,-1)], receiver=[10,10], world_coord=[2688,2688,0]),
        _cliff_input('old_dummy_side_a_dummy_side_b_real', [(11,11,2,0,0,-1), (10,11,2,0,0,-1)],
                     receiver=[11,11], world_coord=[2688,2688,0]),
        _cliff_input('retained_dummy_reselected_by_argument', [], receiver=[-10,-10],
                     world_coord=[-128,-128,0]),
    ]
    initialized = None
    for case in direct:
        uc = prepare(case)
        if initialized is None:
            initialized = [list(struct.unpack('<12I', uc.mem_read(0xB450B8+48*i,48))) for i in range(5)]
        before = rng_hashes(uc)
        observer = _CliffObservation(uc)
        point, receiver_point, matrix, vector, out = (BODY+0x100, BODY+0x110, BODY+0x120, BODY+0x160, BODY+0x180)
        uc.mem_write(point, struct.pack('<3i', *case['world_coord']))
        uc.mem_write(receiver_point, struct.pack('<hh', *case['receiver']))
        receiver = _cliff_call(uc, observer, 0x5657A0, 0x87F7E8, (receiver_point,))
        direction = _cliff_call(uc, observer, 0x4848B0, receiver, (point,))
        velocity = [-3.0, 4.0, -5.0]
        uc.mem_write(vector, struct.pack('<3f', *velocity))
        if direction != 0xFFFFFFFF:
            _cliff_call(uc, observer, 0x755C60, matrix, edx=direction)
            _cliff_call(uc, observer, 0x5AFB80, out, (vector,), edx=matrix)
        direction_cases.append(dict(input=dict(case, vector_bits=list(struct.unpack('<3I', struct.pack('<3f', *velocity)))),
                                    **finish(uc, observer, before)))
    for case in cases + [dict(cases[1], name='no_cliff_one_level', cells=[(10,10,0,0,0,-1),(11,10,1,0,0,-1)]),
                         dict(cases[1], name='no_cliff_equal_level', cells=[(10,10,0,0,0,-1),(11,10,0,0,0,-1)]),
                         dict(cases[1], name='no_contact_airborne')]:
        uc = prepare(case)
        dx, dy = case['delta']
        position = [2561.0 if dx < 0 else 2815.0 if dx > 0 else 2688.0,
                    2561.0 if dy < 0 else 2815.0 if dy > 0 else 2688.0,
                    1000.0 if case['name'] == 'no_contact_airborne' else 206.0]
        velocity = [4.0*dx, 4.0*dy, -4.0]
        initial = bytearray(base_body)
        initial[0x18:0x30] = struct.pack('<6f', *position, *velocity)
        uc.mem_write(BODY, bytes(initial))
        before = rng_hashes(uc)
        observer = _CliffObservation(uc)
        outcome = _cliff_call(uc, observer, 0x439B00, BODY, required=(0x439A10,))
        update_cases.append(dict(input=case, initial_body_hex=initial.hex(),
            final_body_hex=bytes(uc.mem_read(BODY, 0x50)).hex(), outcome=outcome,
            **finish(uc, observer, before)))
    assert {row['direction_calls'][0]['result'] for row in direction_cases} == set(range(9)) | {-1}
    assert all(len(row['direction_calls']) == 1 for row in update_cases[:20])
    assert all(not row['direction_calls'] for row in update_cases[20:])
    assert update_cases[18]['direction_calls'][0]['result'] == 8
    return dict(physical_tire=physical, source_body_hex=base_body.hex(),
        source_fixture='tools/projectile_oracle/typed_warhead_debris.json:rows[0].voxels[0]',
        rng_input=rng_input, initialized_matrix_bits=initialized,
        direction_cases=direction_cases, update_cases=update_cases,
        physical_hills=physical_hills_cliff())


def physical_hills_cliff():
    """Compose existing physical input/constructor/AI owners without substitutes."""
    from tools.native_oracle import configured_gamemd
    from tools.sidebar_oracle.stock import mix, mix_hash
    from tools.spatial_oracle.shrapnel_repair.map_facts import decode_cells
    from tools.spatial_oracle.bridge_shadow_render import physical_assets
    from tools.spatial_oracle.tmp_clear_render import prepare, theater_tile_files
    from tools.rules_oracle.theater_general_reader import TheaterReader
    from tools.projectile_oracle.ifv_isolated_tail import typed_setup
    from tools.projectile_oracle.ifv_impact import retirement_transport
    from tools.spatial_oracle.building_body_rules import SP as reader_sp
    from tools.procedural_drawing_oracle.electric_bolt import RngTrace

    wrapper = (configured_gamemd().parent / 'Hills.mmx').read_bytes()
    raw = mix(wrapper)[mix_hash('Hills.map')]
    _sections, authored = decode_cells(raw)
    footprint = [(x, y) for y in range(63, 67) for x in range(100, 105)]
    assert all(authored[xy]['overlay'] is None for xy in footprint)
    assets, _sources = physical_assets(tile=True, hills=True)
    ini = assets['TEMPERATMD.INI']
    reader = TheaterReader()
    tile_files, _mapping = theater_tile_files(reader, ini, 505, 'tem')
    names = sorted({tile_files[authored[xy]['tile']] for xy in footprint})
    tmp, _tmp_inputs, files = prepare(tmp_names=tuple(names), hills=True)
    # prepare owns the already-loaded descriptor chain. Do not install a
    # second file transport or mistake linked siblings for vector entries.
    heads = {}
    head = tmp.read32(tmp.read32(0xA8ED2C))
    while head:
        name = tmp.string(head+0x2F5)
        assert name not in heads and name in names, name
        heads[name] = head
        head = tmp.read32(head+0x2BC)
    assert set(heads) == set(names)
    facts, cells = [], []
    for x, y in footprint:
        item = authored[x, y]
        name = tile_files[item['tile']]
        slope = tmp.invoke(0x5471B0, heads[name], (item['subtile'],))
        land = tmp.invoke(0x544BE0, heads[name], (item['subtile'],))
        assert 0 <= slope < 21
        assert land != 2, (x, y, land)
        cells.append([x, y, item['level'], slope, 0, -1])
        facts.append(dict(x=x, y=y, **item, tmp=name, slope=slope, land=land))

    m, _bullet, warhead, _cell, _obj, _typ, initial = typed_setup({})
    uc = m.u
    initialize_slope_tables(uc, reader_sp, STOP)
    uc.mem_write(0x89E7C0, words(104));uc.mem_write(0x89C778, words(104))
    m.invoke(0x439610, 0)
    table = m.read32(0x87F924)
    # The supplied registration boundary is sparse and every query is checked.
    uc.mem_write(table, bytes(0x100000))
    for fact, (x, y, level, slope, flags, overlay) in zip(facts, cells):
        cell = m.alloc(0x200)
        uc.mem_write(cell, words(0x7E4EEC))
        uc.mem_write(cell+0x24, struct.pack('<hh', x, y))
        item = authored[x, y]
        uc.mem_write(cell+0x38, words(item['tile']))
        uc.mem_write(cell+0x44, words(overlay))
        uc.mem_write(cell+0xEC, words(fact['land']))
        uc.mem_write(cell+0x11A, bytes([item['subtile'], level, slope]))
        uc.mem_write(cell+0x140, words(flags))
        uc.mem_write(table+4*(y*512+x), words(cell))
    scenario = m.read32(0xA8B230)
    tire = m.read32(m.read32(warhead+0x190))
    m.invoke(0x65C6D0, scenario+0x218, (5,))
    # Attach the existing observer without reseeding any stream.
    rng = RngTrace.__new__(RngTrace)
    rng.u = uc
    rng.streams = {'main': 0x886B88, 'scenario': scenario+0x218, 'mapgen': 0xABE890}
    rng.requests, rng.pending = [], []
    rng_hook = uc.hook_add(UC_HOOK_CODE, rng.observe)
    observation = _CliffObservation(uc, allowed_cells=set(footprint))
    before_rng = rng.states()
    coord = m.alloc(12);uc.mem_write(coord, struct.pack('<3i', 26240, 16256, 208))
    piece = m.alloc(0x180)
    m.invoke(0x7493B0, piece, (tire, coord, 0))
    observation.observe(uc, STOP, 0, None)
    constructor_body = bytes(uc.mem_read(piece+0xB0, 0x50)).hex()
    constructor_rng = rng.states()
    constructor_requests = list(rng.requests)
    assert not rng.pending and len(constructor_requests) == 7

    def member(base):
        return piece in [m.read32(m.read32(base+4)+4*i) for i in range(m.read32(base+16))]

    def state():
        return dict(body_hex=bytes(uc.mem_read(piece+0xB0,0x50)).hex(),
            duration=struct.unpack('<i', uc.mem_read(piece+0x140,4))[0],
            location=list(struct.unpack('<3i', uc.mem_read(piece+0x9C,12))),
            alive=uc.mem_read(piece+0x90,1)[0], limbo=uc.mem_read(piece+0x81,1)[0],
            marked_for_deletion=uc.mem_read(piece+0x110,1)[0],
            pending_delete_count=m.read32(0xB0F6A8),
            logic_registered=member(0x87F778), registry_registered=member(0x887388),
            display_layers=[i for i in range(5) if member(0x8A0360+24*i)])

    constructor_state = state()
    checkpoints = []
    stop_visit = delete_visit = None
    for visit in range(1, 181):
        old = state()
        indices = [len(observation.updates),len(observation.lookups),len(observation.directions),
                   len(observation.matrices),len(observation.transforms)]
        uc.mem_write(0xA8ED84, words(100+visit))
        m.invoke(0x749F30, piece)
        observation.observe(uc, STOP, 0, None)
        new = state()
        updates = observation.updates[indices[0]:]
        if updates and updates[-1]['outcome'] == 2:
            stop_visit = visit
        if new['marked_for_deletion'] or not new['logic_registered'] or not new['registry_registered']:
            delete_visit = visit
        if visit in (25,26,27) or visit in (stop_visit,delete_visit):
            checkpoints.append(dict(visit=visit,binary_frame=100+visit,before=old,after=new,
                updates=updates,lookup_trace=observation.lookups[indices[1]:],
                direction_calls=observation.directions[indices[2]:],
                matrix_calls=observation.matrices[indices[3]:],transform_calls=observation.transforms[indices[4]:]))
        if delete_visit is not None:
            break
    assert stop_visit is not None and delete_visit is not None, (stop_visit,delete_visit)
    assert next(row for row in checkpoints if row['visit']==26)['direction_calls'][0]['result']==2
    assert constructor_rng == rng.states(), 'Native flight consumed RNG unexpectedly'
    assert len(rng.requests)==7 and not rng.pending
    before_drain = state()
    drain_transport = []
    drain_hook = uc.hook_add(UC_HOOK_CODE,
        lambda _u, pc, _size, _data: retirement_transport(m, pc, drain_transport))
    m.invoke(0x725C70, 0)
    observation.observe(uc, STOP, 0, None)
    uc.hook_del(drain_hook)
    after_drain = state()
    assert not after_drain['registry_registered'] and after_drain['pending_delete_count'] == 0, after_drain
    assert constructor_rng == rng.states() and len(rng.requests)==7 and not rng.pending
    observation.finish()
    uc.hook_del(rng_hook);uc.hook_del(observation.hook)
    return dict(input=dict(map='Hills.mmx',inner='Hills.map',constructor_entry_seed=5,
            source_coord=[26240,16256,208],physical_cells=facts,cells=cells,
            dummy_level=0,dummy_slope=0,dummy_flags=0),
        physical=dict(wrapper_sha256=hashlib.sha256(wrapper).hexdigest(),
            inner_sha256=hashlib.sha256(raw).hexdigest(),theater_sha256=hashlib.sha256(ini).hexdigest(),
            tmps=[dict(name=name,sha256=hashlib.sha256(files[name]).hexdigest()) for name in names],
            tire=initial['tire']),
        constructor_body_hex=constructor_body,constructor_state=constructor_state,
        constructor_rng_before=before_rng,constructor_rng_after=constructor_rng,
        constructor_requests=constructor_requests,checkpoints=checkpoints,
        stop_visit=stop_visit,delete_visit=delete_visit,final_state=before_drain,
        before_drain=before_drain,after_drain=after_drain,drain_transport=drain_transport,
        boundary='Constructor-entry seed5, sparse actual Hills cells/native TMP slopes, full retained VoxelAI host and original deferred drain/scalar cleanup. Existing allocator retains readable storage; no ordinary death/world RNG prefix or renderer claim.')


def cliff_metadata():
    return provenance(scope='Original4848B0 directions,755C60 initialized rows0..4,5AFB80,23 complete439B00 controls and physical Hills TIRE constructor/749F30 flight through retirement/drain',
        assumptions=[
            'Original native_slope.initialize_slope_tables runs full startup in each same VM before any direction, matrix or Update call; fifth selector row is read literally, including its adjacency to slope storage. FPCW0E7F is restored.',
            'TIRE scalar bits and complete constructed Bounce0x50 body are inputs from existing consumed typed_warhead_debris original physical reader/constructor row. Only position/velocity are supplied numerical boundary priors for single-tick cliff controls; no ordinary death-to-cliff trajectory is claimed.',
            'Mapped sparse Cell table with explicit signed levels/slope/flags/overlay and one shared initialized Dummy. No objects, OS, scheduler, damage, renderer or complete save/load lifecycle. Numeric three-stream states are supplied prior native outputs and asserted unchanged.',
            'Direct direction rows cover four cardinals, four diagonals times four side admissions, same cell and two shared-Dummy histories. Update rows cover the20 directional cases and three no-cliff/contact controls. No host-computed direction/matrix/velocity is an expected native output.',
            'Physical extension unwraps loose Hills.mmx with existing archive owner and decodes actual map packs; native theater reader, TMP loader, slope5471B0 and land544BE0 supply20 Cell priors. Sparse enrollment is supplied; every queried Cell must return one of those coordinates, never Dummy.',
            'Existing typed_setup runs physical TIRE scalar reader/full7493B0 constructor, using authored Smashing list only for type allocation. Constructor-entry Scenario seed5 is explicit; no ordinary death/world-seed prefix. The SAME retained object/VM then executes full749F30 until actual stop144/logical retirement145 and original725C70 drain. No lifetime is inferred by subtraction.',
        ], substitutions=[
            'The23+23 numeric controls replace no native code. Physical extension inherits typed_setup lexical INI caches, archive bytes, allocator/delete/CRT/TLS transport; native Cell enrollment is a declared input boundary. No constructor, Bounce, VoxelAI, RNG, duration, UnInit or scalar cleanup result is supplied.',
            'Original deferred drain uses existing ifv_impact.retirement_transport OS readability boundary. The fixture allocator/delete retain readable storage; native registry removal and pending compaction execute, not host physical memory deallocation.',
        ], entry_points=dict(direction=0x4848B0, mirror=0x755C60,
            transform=0x5AFB80, update=0x439B00, world_cell=0x565730,
            packed_cell=0x5657A0, startup=0x754CB0, voxel_constructor=0x7493B0,
            voxel_ai=0x749F30, deferred_drain=0x725C70, tmp_slope=0x5471B0,
            tmp_land=0x544BE0))


if __name__=='__main__' and '--cliff' in sys.argv[1:]:
    finish_vectors(cliff_vectors, Path(__file__).with_name('bounce_cliff.json'),
        provenance=cliff_metadata, argv=[arg for arg in sys.argv[1:] if arg != '--cliff'])
elif __name__=='__main__':
    finish_vectors(lambda:[execute(c) for c in cases()],Path(__file__).with_suffix('.json'),
        provenance=lambda:provenance(scope='Original439B00 full function with bounded terrain fixtures',
            assumptions=['Supplied initialized Cell scalar104; original439610 derives Bounce deck416',
                'Flat contacts and airborne slope queries only; zero quaternion spin; no cliff rollback fixtures',
                'No OS, scheduler, debris constructor, renderer or host effects emulated'],substitutions=[],
            entry_points={'update':0x439B00,'deck_init':0x439610}))
