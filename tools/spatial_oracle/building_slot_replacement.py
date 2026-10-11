"""Legacy slot frame handoff and full scalar destructor on supplied Anims.
Constructor output is supplied, and operator_delete releases a synthetic arena
allocation through an explicit recorded allocator boundary. Original destructor,
vector lookups, sound-handle teardown and replacement instructions execute.

Add --expiry for the separate retail-input expiry callback corpus. Its native
reader/JoinedFixture execution does not replace the 420 historical rows.
"""
from itertools import product
from pathlib import Path
import hashlib
import os
import struct
import sys
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
from tools.native_oracle import SCRATCH,run_checked,finish_vectors,provenance
from tools.spatial_oracle.building_art_transition import ArtFixture
from tools.spatial_oracle.object_health import OBJ,SP,dwords
OLD=SCRATCH+0xB000
NEW=SCRATCH+0xD000
# Exact static initializer stores, not inferred template vtables.
VECTORS=((0xB0F698,0x7E91EC,0x72586D),(0xA8E360,0x7E4F64,0x4E7AFD),
 (0xB0F720,0x7E91EC,0x7252ED),(0xB0F670,0x7E91EC,0x72536D),
 (0xB0F618,0x7E91EC,0x7253ED),(0xA8E9A8,0x7E9F24,0x4E6D7D))
SPANS=((0x4519F7,0x451A36),(0x426590,0x4265AC),(0x4228E0,0x422B1D),
       (0x5F3B80,0x5F3D84))

EXPIRY_BUILDINGS = ('NADEPT', 'GADEPT')
EXPIRY_ANIMS = ('NADEPT_B', 'NADEPT_BD') + tuple(f'NADEPT_C{i}' for i in range(1, 7)) + tuple(
    f'GADEPT_{suffix}' for suffix in ('A', 'AD', 'B', 'BD', 'C', 'CD', 'D', 'DD'))
EXPIRY_SLOTS = (3, 7, 8, 10, 11, 12, 14, 15, 16, 17, 18)
EXPIRY_SLOT_READER = (0x4615CA, 0x46421A)
EXPIRY_SPANS = ((0x44E8F0, 0x44EB08), (0x451B40, 0x451E14),
                (0x421EA0, 0x422716), (0x422720, 0x422814), (0x424B10, 0x424B88),
                (0x42435F, 0x42436D),
                (0x4255B0, 0x4256B0), (0x4581F0, 0x4581FA))


def expiry_inputs():
    """Compose existing physical lexical/CRC/ART readers for selected depots.

    These are reached original read/store blocks, with original constructors
    and the physical SHP lookup boundary already owned by Reader. No scalar
    value or constructor output is supplied by Rust or by this Python code.
    """
    from tools.native_oracle import image_bytes, _sections
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.rules_oracle.bridge_anim_inputs import Reader
    from tools.spatial_oracle.building_body_rules import INI, SP as READER_SP
    from tools.spatial_oracle.building_repair import depot_inputs_root

    root = depot_inputs_root()
    art_path = root / 'ARTMD.INI' if (root / 'ARTMD.INI').is_file() else Path('ini/ARTMD.INI')
    asset_root = Path(os.environ.get('VERA20K_BUILDING_SLOT_EXPIRY_ASSETS',
                                    'target/asset/building-slot-expiry/extract'))
    required = ('NGDEPT_B.SHP', 'NGDEPT_C.SHP', 'GGDEPT_A.SHP',
                'GGDEPT_B.SHP', 'GGDEPT_C.SHP', 'GGDEPT_D.SHP')
    assert all((asset_root / name).is_file() for name in required), str(asset_root)
    art_raw = art_path.read_bytes()
    art_sections, art_lines = lexical(art_raw, set(EXPIRY_BUILDINGS + EXPIRY_ANIMS))
    m = Reader(asset_root, art_sections)
    u = m.u
    native_image = image_bytes()
    code = [(0x400000 + rva, raw, size) for rva, raw, size, _, flags
            in _sections(native_image) if size and flags & 0x20000000]
    assert all(bytes(u.mem_read(address, size)) == native_image[raw:raw + size]
               for address, raw, size in code)
    m.rules = m.alloc(0x5000)
    u.mem_write(0x8871E0, dwords(m.rules))
    m.invoke(0x665650, m.rules)
    m.invoke(0x4E7CF0, 0)
    m.types = {}
    for name in EXPIRY_BUILDINGS:
        pointer = m.alloc(0x2000)
        m.invoke(0x45DD90, pointer, (m.cstring(name),))
        m.types[name] = pointer

    def state():
        return dict(condition_yellow_bits=bytes(u.mem_read(m.rules + 0x1700, 8)).hex(),
                    buildings={name: dict(strength=struct.unpack('<i', u.mem_read(p + 0xA0, 4))[0],
                                          unit_repair=u.mem_read(p + 0x16A9, 1)[0],
                                          grinding=u.mem_read(p + 0x16AD, 1)[0],
                                          is_anim_delayed_fire=u.mem_read(p + 0x16A7, 1)[0])
                               for name, p in m.types.items()})

    calls, writes, layer_name = [], [], None
    watched = {m.rules + 0x1700: 'condition_yellow'}
    for name, pointer in m.types.items():
        for field, offset in (('strength', 0xA0), ('unit_repair', 0x16A9),
                              ('grinding', 0x16AD), ('is_anim_delayed_fire', 0x16A7)):
            watched[pointer + offset] = name + '.' + field

    def observe(u, pc, _size, _data):
        if pc in (0x5276D0, 0x5283D0, 0x5295F0):
            sp = u.reg_read(UC_X86_REG_ESP)
            calls.append(dict(layer=layer_name, pc=hex(pc), caller=hex(m.read32(sp)),
                              args=[m.read32(sp + i * 4)
                                    for i in range(1, 5 if pc == 0x5283D0 else 4)]))

    def written(u, _access, address, size, value, _data):
        if address in watched:
            writes.append(dict(layer=layer_name, pc=hex(u.reg_read(UC_X86_REG_EIP)),
                               field=watched[address], size=size, value=value))

    from unicorn import UC_HOOK_MEM_WRITE
    hooks = [u.hook_add(UC_HOOK_CODE, observe), u.hook_add(UC_HOOK_MEM_WRITE, written)]

    def block(begin, end, registers, required_addresses=()):
        u.reg_write(UC_X86_REG_ESP, READER_SP)
        for register, value in registers:
            u.reg_write(register, value)
        run_checked(u, begin, end, required_addresses=required_addresses, count=2_000_000)

    constructor, layers, animation_list = state(), [], []
    for filename in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'XMP03T4.MAP'):
        path = root / filename
        if not path.is_file():
            assert filename == 'LANGRULE.INI', str(path)
            layers.append(dict(file=filename, absent=True))
            continue
        raw = path.read_bytes()
        selected, lines = lexical(raw, set(EXPIRY_BUILDINGS) | {'AudioVisual', 'Animations'})
        if filename == 'RULESMD.INI':
            for key, name in selected.get('Animations', {}).items():
                if name in EXPIRY_ANIMS:
                    m.invoke(0x428B80, m.cstring(name))
                    animation_list.append(dict(key=key, name=name))
            assert {row['name'] for row in animation_list} == set(EXPIRY_ANIMS)
        selected.pop('Animations', None)
        m.make_ini(selected)
        before, call_start, write_start = state(), len(calls), len(writes)
        layer_name = filename
        for name, pointer in m.types.items():
            if not m.invoke(0x526810, INI, (m.cstring(name),)):
                continue
            block(0x5F94D3, 0x5F94F3,
                  ((UC_X86_REG_EBX, pointer), (UC_X86_REG_ESI, INI),
                   (UC_X86_REG_EBP, pointer + 0x24)), (0x5276D0,))
            for begin, end in ((0x460906, 0x46092F), (0x460968, 0x460982)):
                block(begin, end, ((UC_X86_REG_EBP, pointer), (UC_X86_REG_ESI, INI),
                                   (UC_X86_REG_EBX, pointer + 0x24)), (0x5295F0,))
        block(0x66B35E, 0x66B385, ((UC_X86_REG_ESI, m.rules), (UC_X86_REG_EDI, INI)),
              (0x5283D0,))
        layers.append(dict(file=filename, absent=False, bytes=len(raw),
                           sha256=hashlib.sha256(raw).hexdigest(),
                           selected={name: {key: value for key, value in keys.items()
                                            if key in ('Strength', 'UnitRepair', 'Grinding', 'ConditionYellow')}
                                     for name, keys in selected.items()},
                           lines=[line for line in lines if line['key'] in
                                  ('Strength', 'UnitRepair', 'Grinding', 'ConditionYellow')],
                           before=before, after=state(), calls=calls[call_start:],
                           writes=writes[write_start:]))

    layer_name = 'ARTMD.INI'
    m.make_ini(art_sections)
    before, call_start, write_start = state(), len(calls), len(writes)
    constructor_slots = {name: bytes(u.mem_read(pointer + 0xF4C, 21 * 0x44)).hex()
                         for name, pointer in m.types.items()}
    type_rows = []
    for name, pointer in m.types.items():
        block(0x4611A3, 0x4611C0,
              ((UC_X86_REG_EBP, pointer), (UC_X86_REG_EDI, pointer + 0x1F8)),
              (0x5295F0, 0x4611BA))
        u.mem_write(READER_SP, bytes(0x400))
        u.mem_write(READER_SP + 0x36C, dwords(INI))
        block(*EXPIRY_SLOT_READER,
              ((UC_X86_REG_EBP, pointer), (UC_X86_REG_ESI, INI),
               (UC_X86_REG_EDI, pointer + 0x1F8)))
        type_rows.append(dict(name=name, body_controls={
            field: list(struct.unpack('<3i', u.mem_read(pointer + offset, 12)))
            for field, offset in (('idle', 0xF10), ('active', 0xF1C), ('aux1', 0xF28), ('aux2', 0xF34))},
            slots=[dict(
            slot=slot, normal=m.string(pointer + 0xF4C + slot * 0x44),
            damaged=m.string(pointer + 0xF5C + slot * 0x44),
            garrisoned=m.string(pointer + 0xF6C + slot * 0x44),
            x=struct.unpack('<i', u.mem_read(pointer + 0xF7C + slot * 0x44, 4))[0],
            y=struct.unpack('<i', u.mem_read(pointer + 0xF80 + slot * 0x44, 4))[0],
            z_adjust=struct.unpack('<i', u.mem_read(pointer + 0xF84 + slot * 0x44, 4))[0],
            flags=bytes(u.mem_read(pointer + 0xF8C + slot * 0x44, 4)).hex())
            for slot in EXPIRY_SLOTS]))
    art = dict(file='ARTMD.INI', bytes=len(art_raw), sha256=hashlib.sha256(art_raw).hexdigest(),
               selected=art_sections, lines=art_lines, before=before, after=state(),
               constructor_slots=constructor_slots, building_slots=type_rows,
               calls=calls[call_start:], writes=writes[write_start:])
    anims = []
    for item in animation_list:
        name = item['name']
        pointer = m.invoke(0x428B80, m.cstring(name))
        asset_start = len(m.asset_loaded)
        admitted = m.invoke(0x427D00, pointer, (INI,))
        row = m.result(name, pointer, admitted)
        row.update(physical_art_keys=art_sections[name],
                   asset_loads=m.asset_loaded[asset_start:],
                   downstream={field: struct.unpack('<i', u.mem_read(pointer + offset, 4))[0]
                               for field, offset in (('make_infantry', 0x34C), ('running_frames', 0x350),
                                                     ('next_type', 0x2C8), ('spawns_particle', 0x2CC),
                                                     ('spawns', 0x2F0), ('spawn_count', 0x2F4))},
                   native_type_bytes=bytes(u.mem_read(pointer, 0x3AC)).hex())
        anims.append(row)
    for hook in hooks:
        u.hook_del(hook)
    assert all(bytes(u.mem_read(address, size)) == native_image[raw:raw + size]
               for address, raw, size in code)
    files = [dict(name=name, bytes=(asset_root / name).stat().st_size,
                  sha256=hashlib.sha256((asset_root / name).read_bytes()).hexdigest())
             for name in required]
    receipt_path = asset_root.parent / 'receipts.json'
    receipts = []
    if receipt_path.is_file():
        import json
        receipts = [{key: value for key, value in row.items()
                     if key in ('asset', 'source_archive', 'entry_id', 'size', 'format', 'detail')}
                    for row in json.loads(receipt_path.read_text())]
    m.expiry_inputs = dict(constructor=constructor, layers=layers, art=art, after=state(),
                           native_anim_types=anims, selected_animation_list=animation_list,
                           shp_files=files, extraction_receipts=receipts,
                           original_code_unchanged=True)
    return m

def generate():
 f=ArtFixture();u=f.u; events=[];current_slot=0
 originals=[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
 def observe(u,a,n,d):
  if a in (0x426590,0x4228E0,0x4255B0,0x7C8B3D):
   events.append(dict(address=f'{a:08X}',slot_pointer=f.read(OBJ+0x55C+current_slot*4),
       copied_frame=f.read(NEW+0xAC)))
  if a==0x7C8B3D:
   sp=u.reg_read(UC_X86_REG_ESP)
   assert f.read(sp+4)==OLD
   u.reg_write(UC_X86_REG_EIP,f.read(sp));u.reg_write(UC_X86_REG_ESP,sp+4)
 u.hook_add(UC_HOOK_CODE,observe)
 rows=[]
 for slot,old_frame,has_old,new_frame in product(range(21),(-2147483648,-1,0,17,2147483647),(False,True),(0,23)):
  f.prepare(50,100,1,'all');f.capture=False;events.clear();current_slot=slot
  for a,vt,_ in VECTORS:
   u.mem_write(a,bytes(24));u.mem_write(a,dwords(vt))
  u.mem_write(OLD,bytes(0x1C8));u.mem_write(NEW,bytes(0x1C8))
  u.mem_write(OLD,dwords(0x7E3354));u.mem_write(OLD+0xAC,dwords(old_frame))
  u.mem_write(NEW+0xAC,dwords(new_frame))
  # Different retained runtime fields prove the handoff copies only+AC.
  u.mem_write(OLD+0xB4,dwords(17,19,23));u.mem_write(NEW+0xB4,dwords(91,0,3))
  u.mem_write(OLD+0x195,b'\x08');u.mem_write(NEW+0x195,b'\xff')
  u.mem_write(OLD+0x19C,b'\0');u.mem_write(NEW+0x19C,b'\1')
  u.mem_write(OBJ+0x55C+slot*4,dwords(OLD if has_old else 0))
  u.mem_write(SP+0x3C,dwords(slot))
  for reg,value in ((UC_X86_REG_ESI,OBJ),(UC_X86_REG_EBP,NEW),(UC_X86_REG_ESP,SP)):
   u.reg_write(reg,value)
  run_checked(u,0x4519F7,0x451A36,count=4000)
  rows.append(dict(input=dict(slot=slot,old_frame=old_frame,has_old=has_old,new_frame=new_frame),
      output=dict(frame=f.read(NEW+0xAC),timer=[f.read(NEW+x) for x in (0xB4,0xB8,0xBC)],
        loop_remaining=int(u.mem_read(NEW+0x195,1)[0]),first_guard=int(u.mem_read(NEW+0x19C,1)[0]),
        installed=f.read(OBJ+0x55C+slot*4),events=list(events))))
 assert originals==[bytes(u.mem_read(a,b-a)) for a,b in SPANS]
 return rows

def metadata():
 f=ArtFixture()
 p=provenance(scope='Building4519F7 frame handoff/scalar delete/full destructor through451A36 pointer install',
   assumptions=['420 rows,21slots,5 signed frames,old presence,2 supplied constructor frames',
     'Old Anim+CC/type pointer null, no Logic membership, vectors empty with exact original initializer vtables',
     'Constructor output supplied with independent timer/loop/firstAI values; constructor and nonnull type cleanup excluded'],
   substitutions=['operator_delete7C8B3D records arena release and returns; no original code patch'],
   entry_points={'replacement':0x4519F7,'scalar_deleting_destructor':0x426590,'destructor':0x4228E0})
 p['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',
   hex=bytes(f.u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
 p['vector_initializers']=[dict(vector=f'{a:08X}',vtable=f'{vt:08X}',store=f'{store:08X}',
   bytes=bytes(f.u.mem_read(store,10)).hex()) for a,vt,store in VECTORS]
 return p


def expiry_cases():
    """Supplied controls over each original callback branch, plus live AI."""
    rows = []
    for building in EXPIRY_BUILDINGS:
        for health in (1200, 600):
            for slot in (10, 12):
                rows.append(dict(name=f'{building}_{health}_{slot}_natural', building=building,
                                 health=health, slot=slot, natural=True))
    base = dict(building='NADEPT', slot=10)
    for name, controls in (
            ('extension_completion_false', dict(completion=0)),
            ('extension_completion_true', dict(completion=1)),
            ('health599_damaged', dict(health=599)),
            ('health601_normal', dict(health=601)),
            ('no_contact', dict(contact=False)),
            ('wrong_mission', dict(mission=5)),
            ('current_none_queued_repair', dict(mission=-1, queued=20)),
            ('current_guard_queued_repair', dict(mission=5, queued=20)),
            ('current_repair_queued_guard', dict(mission=20, queued=5)),
            ('current_health_changes_replacement', dict(callback_health=600)),
            ('ready_true_preserved', dict(ready=1)),
            ('later_sparse_contact', dict(contacts=[None, None, 'unit'], contact_count=3)),
            ('zero_count_stale_contact', dict(contacts=['unit'], contact_count=0)),
            ('not_unit_repair', dict(unit_repair=0)),
            ('slot_marker_false', dict(slot_marker=0)),
            ('building_dead', dict(alive=0)),
            ('unmatched', dict(unmatched=True)),
            ('empty_work_normal', dict(slot_names={'11': {'normal': ''}})),
            ('empty_work_damaged', dict(health=600, slot_names={'11': {'damaged': ''}})),
            ('empty_idle_normal', dict(contact=False, slot_names={'18': {'normal': ''}})),
            ('empty_idle_damaged', dict(contact=False, health=600,
                                       slot_names={'18': {'damaged': ''}})),
            ('scalar_clear_completed', dict(scalar_clear=True, completion=1))):
        rows.append(dict(base, name=name, **controls))
    for name, controls in (
            ('retract_incomplete', dict(slot=12, completion=0)),
            ('retract_completed', dict(slot=12, completion=1)),
            ('retract_not_unit_repair', dict(slot=12, completion=1, unit_repair=0)),
            ('work_clear_only', dict(slot=11, completion=1)),
            ('idle_clear_only', dict(slot=18, completion=1)),
            ('ordinary_clear_only', dict(slot=3, initial_type='NADEPT_C1', completion=1)),
            ('ai_cancel_without_completion', dict(slot=12, cancel_ai=True)),
            ('outer_slot8_idle', dict(slot=8, initial_type='NADEPT_C1', completion=0)),
            ('outer_slot8_idle_damaged', dict(slot=8, initial_type='NADEPT_C1', health=600)),
            ('outer_slot8_marker_false', dict(slot=8, initial_type='NADEPT_C1', slot_marker=0)),
            ('outer_slot8_dead_repeated_broadcast', dict(slot=8, initial_type='NADEPT_C1',
                                                        alive=0, natural=True)),
            ('outer_slot8_empty_idle', dict(slot=8, initial_type='NADEPT_C1',
                                           slot_names={'18': {'normal': ''}})),
            ('outer_grinding10_active', dict(grinding=1, unit_repair=0, completion=0)),
            ('outer_grinding10_active_damaged', dict(grinding=1, unit_repair=0, health=600)),
            ('outer_grinding10_marker_false', dict(grinding=1, unit_repair=0, slot_marker=0)),
            ('outer_grinding10_also_repair_work', dict(grinding=1, unit_repair=1)),
            ('outer_grinding10_dead_repeated_broadcast', dict(grinding=1, unit_repair=0,
                                                             alive=0, natural=True)),
            ('outer_grinding10_empty_active', dict(grinding=1, unit_repair=0,
                                                  slot_names={'3': {'normal': ''}}))):
        if controls.get('grinding'):
            controls = {'slot_names': {'3': {'normal': 'NADEPT_B', 'damaged': 'NADEPT_BD'}},
                        **controls}
        rows.append(dict(base, name=name, **controls))
    active_names = {'3': {'normal': 'NADEPT_B', 'damaged': 'NADEPT_BD', 'garrisoned': 'GADEPT_B'}}
    for name, controls in (
            ('delayed_fire_incomplete', dict(completion=0)),
            ('delayed_fire_normal', dict(completion=1)),
            ('delayed_fire_damaged', dict(completion=1, health=600)),
            ('delayed_fire_garrisoned', dict(completion=1, garrison_count=1)),
            ('delayed_fire_negative_garrison_count', dict(completion=1, garrison_count=-1)),
            ('delayed_fire_damaged_garrisoned', dict(completion=1, health=600, garrison_count=1))):
        rows.append(dict(base, name=name, unit_repair=0, is_anim_delayed_fire=1,
                         slot_names=active_names, **controls))
    for slot, target in ((15, 16), (17, 14)):
        for completion in (0, 1):
            for health in (1200, 600):
                rows.append(dict(base, name=f'shared_slot{slot}_{health}_completion{completion}',
                                 slot=slot, initial_type='NADEPT_C1', completion=completion,
                                 health=health, slot_names={str(target): {
                                     'normal': 'NADEPT_B', 'damaged': 'NADEPT_BD'}}))
        rows.append(dict(base, name=f'shared_slot{slot}_empty_name', slot=slot,
                         initial_type='NADEPT_C1', completion=1))
    return rows


def expiry_fixture(inputs, case):
    from tools.spatial_oracle.building_construction import FRAME, JoinedFixture
    from tools.native_oracle import file_span, image_bytes
    from tools.spatial_oracle.refinery_dock import ACTOR, BLD, BLD_ITEMS
    from unicorn import UC_HOOK_MEM_WRITE

    f = JoinedFixture(inputs, seed=case.get('seed', 31))
    u, read = f.u, f.read32
    u.mem_write(FRAME, dwords(200))
    f.building(case['building'], BLD, BLD_ITEMS, health=case.get('health', 1200))
    # These original constructor stores supply the empty inherited vectors
    # full Techno7077C0 / Building44E8F0 requires, not callback results.
    for register, value in ((UC_X86_REG_ESI, BLD), (UC_X86_REG_EBX, 0)):
        u.reg_write(register, value)
    run_checked(u, 0x6F3041, 0x6F30D6)
    u.reg_write(UC_X86_REG_ESI, BLD)
    run_checked(u, 0x43B68D, 0x43B71F)
    # The real listener-vector initializer and Building constructor append
    # give Anim's original AnnounceExpiry an active, original-vtable listener.
    run_checked(u, 0x7254D0, 0x725506)
    u.mem_write(0xB0F5BC, dwords(f.vbuf + 0x7400, 128))
    u.reg_write(UC_X86_REG_ESI, BLD)
    u.reg_write(UC_X86_REG_EBX, 0)
    run_checked(u, 0x43BB81, 0x43BBD4)
    assert read(0xB0F5C8) == 1 and read(read(0xB0F5BC)) == BLD
    u.mem_write(BLD + 0xAC, dwords(case.get('mission', 20)))
    u.mem_write(BLD + 0xB4, dwords(case.get('queued', -1)))
    contacts = case.get('contacts', ['unit'] if case.get('contact', True) else [None])
    assert len(contacts) <= 4
    u.mem_write(BLD_ITEMS, dwords(*(ACTOR if item == 'unit' else 0 for item in contacts)))
    u.mem_write(BLD + 0xE8, dwords(case.get('contact_count', len(contacts))))
    u.mem_write(BLD + 0x6DD, bytes([case.get('ready', 0)]))
    u.mem_write(BLD + 0x694, dwords(case.get('garrison_count', 0)))
    building_type = f.types[case['building']]
    for field, offset in (('unit_repair', 0x16A9), ('grinding', 0x16AD),
                          ('is_anim_delayed_fire', 0x16A7)):
        if field in case:
            u.mem_write(building_type + offset, bytes([case[field]]))
    for slot, variants in case.get('slot_names', {}).items():
        for variant, name in variants.items():
            assert name == '' or name in EXPIRY_ANIMS
            offset = {'normal': 0xF4C, 'damaged': 0xF5C, 'garrisoned': 0xF6C}[variant]
            u.mem_write(building_type + int(slot) * 0x44 + offset,
                        name.encode('ascii').ljust(16, b'\0'))
    if 'initial_type' in case:
        assert case['initial_type'] in EXPIRY_ANIMS
        for offset in (0xF4C, 0xF5C):
            u.mem_write(building_type + case['slot'] * 0x44 + offset,
                        case['initial_type'].encode('ascii').ljust(16, b'\0'))
    f.transitions = []
    f.expiry_current_pc = 0
    observed = {0x423AC0: 'anim_ai', 0x424B31: 'normal_completion',
                0x4255B0: 'anim_uninit', 0x5F65F0: 'object_uninit',
                0x7258D0: 'announce_expiry', 0x44E8F0: 'building_pointer_expired',
                0x451B40: 'slot_expiry', 0x451890: 'slot_allocate', 0x421EA0: 'anim_ctor',
                0x426590: 'scalar_delete', 0x4228E0: 'anim_dtor', 0x406060: 'sound_release'}

    def hook(u, pc, _size, _data):
        f.expiry_current_pc = pc
        if pc not in observed:
            return
        sp, this = u.reg_read(UC_X86_REG_ESP), u.reg_read(UC_X86_REG_ECX)
        row = dict(event=observed[pc], pc=f'{pc:08X}', this=this, frame=read(FRAME),
                   slots=[read(BLD + 0x55C + i * 4) for i in range(21)])
        if pc in (0x451B40, 0x44E8F0):
            row['expired'] = read(sp + 4)
        if pc == 0x451890:
            row.update(name=bytes(u.mem_read(read(sp + 4), 16)).split(b'\0')[0].decode('ascii'),
                       slot=read(sp + 8), damaged=read(sp + 12),
                       garrisoned=read(sp + 16), extra=read(sp + 20))
        if pc == 0x421EA0:
            row.update(**f.anim_type_identity(read(sp + 4)))
        f.transitions.append(row)

    def written(u, _access, address, size, value, _data):
        row = dict(pc=f'{f.expiry_current_pc:08X}', frame=read(FRAME), size=size, value=value)
        if BLD + 0x55C <= address < BLD + 0x5B0:
            row.update(event='slot_write', slot=(address - BLD - 0x55C) // 4,
                       previous=read(address))
        elif (BLD + 0xF8 <= address < BLD + 0x114
              or BLD + 0x620 <= address < BLD + 0x63C or address == BLD + 0x6DD):
            row.update(event='building_clock_write', address=address)
        else:
            allocation = next((item for item in f.allocations
                               if item['size'] == 0x1C8 and address == item['pointer'] + 0x179), None)
            if allocation is None:
                return
            row.update(event='completion_write', anim=allocation['pointer'])
        f.transitions.append(row)

    f.expiry_hooks = [u.hook_add(UC_HOOK_CODE, hook), u.hook_add(UC_HOOK_MEM_WRITE, written)]
    native = image_bytes()
    # All class tables are file-backed original data. Declared field/name
    # controls never replace a vtable or a native instruction.
    addresses = {0x7E3EBC, 0x7E3354, 0x7E3338} | {
        inputs.read32(pointer) for pointer in inputs.types.values()}
    f.expiry_vtables = {address: file_span(native, address, 0x600)[1]
                       for address in addresses}
    assert all(bytes(u.mem_read(address, len(raw))) == raw
               for address, raw in f.expiry_vtables.items())
    return f


def expiry_row(inputs, case):
    from tools.spatial_oracle.building_construction import FRAME, invoke
    from tools.spatial_oracle.refinery_dock import BLD

    f = expiry_fixture(inputs, case)
    u, read = f.u, f.read32
    game_speed = read(0xA8EB60)
    rng_before = f.rng()
    slot = case['slot']
    # The supplied creation variant is deliberately independent of the
    # callback's original current-health choice, whose threshold is executed.
    invoke(u, 0x451750, BLD, slot, int(case.get('health', 1200) <= 600), 0, 0)
    anim = read(BLD + 0x55C + slot * 4)
    assert anim, case
    default_marker = u.mem_read(anim + 0x179, 1)[0]
    assert default_marker == 0
    if 'completion' in case:
        u.mem_write(anim + 0x179, bytes([case['completion']]))
    if 'slot_marker' in case:
        u.mem_write(anim + 0x118, bytes([case['slot_marker']]))
    if 'alive' in case:
        u.mem_write(BLD + 0x90, bytes([case['alive']]))
    if 'callback_health' in case:
        u.mem_write(BLD + 0x6C, dwords(case['callback_health']))
    if case.get('unmatched'):
        u.mem_write(BLD + 0x55C + slot * 4, dwords(0))

    def clocks():
        return dict(body=bytes(u.mem_read(BLD + 0xF8, 28)).hex(),
                    service=bytes(u.mem_read(BLD + 0x620, 28)).hex(),
                    ready=u.mem_read(BLD + 0x6DD, 1)[0])

    def snapshot():
        value = f.snapshot(BLD)
        for row in value['anims']:
            row.update(completion_marker=u.mem_read(row['pointer'] + 0x179, 1)[0],
                       slot_marker=u.mem_read(row['pointer'] + 0x118, 1)[0],
                       alive=u.mem_read(row['pointer'] + 0x90, 1)[0])
        return value

    retained = clocks()
    before = snapshot()
    creation_events = list(f.events)
    creation_transitions = list(f.transitions)
    f.events.clear()
    f.transitions.clear()
    frames = []
    if case.get('natural') or case.get('cancel_ai'):
        if case.get('cancel_ai'):
            u.mem_write(anim + 0x19B, b'\1')
        for frame in range(200, 601):
            u.mem_write(FRAME, dwords(frame))
            invoke(u, 0x423AC0, anim)
            frames.append(dict(frame=frame, current=struct.unpack('<i', u.mem_read(anim + 0xAC, 4))[0],
                               timer=list(struct.unpack('<3i', u.mem_read(anim + 0xB4, 12))),
                               loop=u.mem_read(anim + 0x195, 1)[0],
                               completion_marker=u.mem_read(anim + 0x179, 1)[0],
                               alive=u.mem_read(anim + 0x90, 1)[0],
                               first_guard=u.mem_read(anim + 0x19C, 1)[0]))
            if not u.mem_read(anim + 0x90, 1)[0]:
                break
        assert not u.mem_read(anim + 0x90, 1)[0], (case, 'no natural completion')
        assert {0x4255B0, 0x7258D0, 0x44E8F0, 0x451B40} <= f.executed
        if case.get('natural'):
            assert 0x424B31 in f.executed
        else:
            assert 0x424B31 not in f.executed
            assert u.mem_read(anim + 0x179, 1)[0] == 0
    elif case.get('scalar_clear'):
        invoke(u, 0x451E40, BLD, slot)
    else:
        invoke(u, 0x44E8F0, BLD, anim, 1)
    after = snapshot()
    assert f.rng() == rng_before and not f.draws and not f.advances
    assert f.code_unchanged()
    assert all(bytes(u.mem_read(address, len(raw))) == raw
               for address, raw in f.expiry_vtables.items())
    assert retained == clocks()
    assert not any(row['event'] == 'building_clock_write' for row in f.transitions)
    result = dict(input=case, game_speed=game_speed, expired_anim=anim,
                  default_completion_marker=default_marker,
                  before=before, after=after, frames=frames, transitions=f.transitions,
                  creation_events=creation_events, creation_transitions=creation_transitions,
                  native_events=f.events, retained_clocks=clocks(),
                  rng_before=rng_before, rng_after=f.rng(), rng_requests=f.draws,
                  rng_advances=f.advances, original_code_unchanged=True,
                  original_vtables_unchanged=True)
    for hook in f.expiry_hooks:
        u.hook_del(hook)
    return result


def expiry_body_readiness(inputs):
    """Separate whole stock body producer controls; expiry does not step it."""
    from tools.spatial_oracle.building_construction import invoke
    from tools.spatial_oracle.refinery_dock import BLD

    rows = []
    for building in EXPIRY_BUILDINGS:
        f = expiry_fixture(inputs, dict(building=building, slot=10))
        u = f.u
        game_speed = f.read32(0xA8EB60)
        prior = f.rng()
        invoke(u, 0x447780, BLD, 1)
        before = f.snapshot(BLD)
        before['ready_query_raw_eax'] = invoke(u, 0x454250, BLD)
        before['ready_query'] = bool(before['ready_query_raw_eax'] & 255)
        invoke(u, 0x4509D0, BLD)
        after = f.snapshot(BLD)
        after['ready_query_raw_eax'] = invoke(u, 0x454250, BLD)
        after['ready_query'] = bool(after['ready_query_raw_eax'] & 255)
        assert prior == f.rng() and not f.draws and not f.advances and f.code_unchanged()
        assert all(bytes(u.mem_read(address, len(raw))) == raw
                   for address, raw in f.expiry_vtables.items())
        rows.append(dict(building=building, game_speed=game_speed, native_idle_control=list(struct.unpack(
            '<3i', u.mem_read(f.types[building] + 0xF10, 12))), before=before, after=after,
            transitions=f.transitions, native_events=f.events, rng_before=prior, rng_after=f.rng(),
            rng_requests=f.draws, rng_advances=f.advances, original_code_unchanged=True,
            original_vtables_unchanged=True))
        for hook in f.expiry_hooks:
            u.hook_del(hook)
    return rows


def expiry_observer_validation(inputs):
    """Original asymmetric bytes validate the one shared runtime decoder.

    The sound-suppression controls are readback evidence, not additional Rust
    sound behavior expectations. Full option/audio loading remains excluded.
    """
    from tools.spatial_oracle.anim_bouncer_launch import constructor_state
    from tools.spatial_oracle.building_construction import invoke
    from tools.spatial_oracle.refinery_dock import BLD

    rows = []
    controls = (('ordinary_ai', 0, 0, 0x423AC0), ('sound_only_ai', 1, 0, 0x423AC0),
                ('inactive_only_ai', 0, 1, 0x423AC0), ('both_ai', 1, 1, 0x423AC0),
                ('direct_uninit', 1, 1, 0x4255B0))
    for name, sound, inactive, entry in controls:
        f = expiry_fixture(inputs, dict(building='NADEPT', slot=12))
        u = f.u
        invoke(u, 0x451750, BLD, 12, 0, 0, 0)
        anim = f.read32(BLD + 0x55C + 12 * 4)

        def observe():
            state = constructor_state(u, anim)
            state['raw_bytes'] = {f'{offset:X}': u.mem_read(anim + offset, 1)[0]
                                  for offset in (0x198, 0x19B, 0x179, 0x118)}
            assert state['runtime']['inactive'] == state['raw_bytes']['19B']
            return state

        constructed = observe()
        assert constructed['raw_bytes']['198'] == constructed['raw_bytes']['19B'] == 0
        u.mem_write(anim + 0x198, bytes([sound]))
        u.mem_write(anim + 0x19B, bytes([inactive]))
        before = observe()
        prior = f.rng()
        invoke(u, entry, anim)
        after = observe()
        assert after['raw_bytes']['198'] == sound and after['raw_bytes']['19B'] == inactive
        assert f.rng() == prior and not f.draws and not f.advances and f.code_unchanged()
        assert all(bytes(u.mem_read(address, len(raw))) == raw
                   for address, raw in f.expiry_vtables.items())
        rows.append(dict(input=dict(name=name, byte198=sound, byte19b=inactive,
                                    entry=f'{entry:08X}'),
                         game_speed=f.read32(0xA8EB60), constructed=constructed,
                         before=before, after=after, transitions=f.transitions,
                         native_events=f.events, executed_uninit=0x4255B0 in f.executed,
                         executed_cancel_test=0x42435F in f.executed,
                         rng_before=prior, rng_after=f.rng(), rng_requests=f.draws,
                         rng_advances=f.advances, original_code_unchanged=True,
                         original_vtables_unchanged=True))
        for hook in f.expiry_hooks:
            u.hook_del(hook)
    return rows


def generate_expiry():
    from tools.native_oracle import NATIVE_SHA256
    inputs = expiry_inputs()
    rows = [expiry_row(inputs, case) for case in expiry_cases()]
    return dict(schema_version=1, source='unicorn/gamemd.exe', native_sha256=NATIVE_SHA256,
                inputs=inputs.expiry_inputs, rows=rows, stock_body_readiness=expiry_body_readiness(inputs),
                observer_validation=expiry_observer_validation(inputs))


def expiry_metadata():
    from tools.native_oracle import file_span, image_bytes
    p = provenance(scope='Original Building Anim expiry callback and slot lifecycle: '
                         'stock NADEPT/GADEPT physical ART/SHP inputs, live AnimAI completion, '
                         'full AnnounceExpiry listener dispatch, clear-before-construction, '
                         'normal/damaged/effective-Mission/contact gates and all shared slot branches',
        entry_points={'building_pointer_expired': 0x44E8F0, 'inherited_pointer_expired': 0x7077C0,
                      'slot_expiry': 0x451B40, 'slot_allocate': 0x451890,
                      'play_anim': 0x451750, 'clear_anim': 0x451E40,
                      'anim_ctor': 0x421EA0, 'completion_default_store': 0x421FB0,
                      'load_constructor_completion_default_store': 0x422803,
                      'inactive_constructor_clear': 0x422003, 'inactive_ai_test': 0x42435F,
                      'sound_suppressed_constructor_clear': 0x421FF1,
                      'sound_suppressed_uninit_test': 0x4255DA,
                      'anim_ai': 0x423AC0, 'normal_completion_store': 0x424B31,
                      'anim_uninit': 0x4255B0, 'object_uninit': 0x5F65F0,
                      'announce_expiry': 0x7258D0, 'scalar_delete': 0x426590,
                      'anim_dtor': 0x4228E0, 'contact': 0x65AE30,
                      'effective_mission': 0x5B3040, 'garrison_count': 0x4581F0,
                      'begin_mode': 0x447780, 'update_animation': 0x4509D0,
                      'ready_query': 0x454250, 'stock_idle_ready_store': 0x451218,
                      'building_type_ctor': 0x45DD90, 'anim_type_ctor': 0x427530,
                      'anim_type_read': 0x427D00, 'building_slot_read': EXPIRY_SLOT_READER[0],
                      'listener_initializer': 0x7254D0, 'building_listener_append': 0x43BB81},
        assumptions=[
            'Selected original Rules/BuildingType/AnimType constructors and native Strength/UnitRepair/'
            'Grinding/ConditionYellow reads in RULESMD→optionalLANGRULE→MPBattleMD→XMP03T4 order; '
            'original ART slot/IsAnimDelayedFire and complete AnimType reads bind six actual retail SHPs.',
            'Existing Reader owns physical-byte lookup, signedCRC/cache/allocator; physical INI/archive '
            'loading and whole Rules Process are excluded. Selected AnimType allocation follows the '
            'physical RULESMD Animations source order; IDs are fixture-relative.',
            'Existing JoinedFixture supplies Building/House/map admission and exact original vector/'
            'listener constructor slices. One Building slot Anim is made by original PlayAnim451750; '
            'body and independent repair clocks/readiness are supplied prior state and retain bytes.',
            'GameOptions game speed is existing supplied JoinedFixture state. Every expiry and '
            'stock-body row records read32(A8EB60) as game_speed; native option/UI loading is excluded.',
            'Live rows invoke unchanged whole AnimAI for one supplied slot Anim per frame200 '
            'onward through original normal completion/UnInit/broadcast/Building callback; they stop '
            'at deferred UnInit, not native global Logic/disk serialization or the later scalar sweep.',
            'Direct receiver controls supply completion/alive/contact/Mission/name/family bytes and '
            'call full Building44E8F0. Slots8/15/17 and alternate delayed-fire/grinding are shared '
            'synthetic controls over actual retail-created AnimTypes, not active selected depot routes.',
            'Normal completion+179 is an independent false-at-construction/true-at-normal-expiry '
            'marker, not a damage selector. Scalar ClearAnim clears the slot before its real destructor '
            'broadcast and therefore has no expiry replacement. No Rust values generate outputs.',
            'Five separate observer_validation controls execute original AnimAI or UnInit with '
            'asymmetric supplied sound+198/inactive+19B bytes, reusing the canonical constructor_state '
            'decoder. They validate that inactive observes19B and UnInit preserves it; they do not '
            'establish full sound behavior or add Rust sound-suppression expectations.',
            'Both full RNG states are retained; each selected row asserts zero requested/raw draws, '
            'unchanged original executable text, and no body/repair Stage/readiness writes. The '
            'separate stock-body controls execute BeginMode1 and whole UpdateAnimation; they record '
            'its body/ready writes and decode ReadyToCommence Boolean from AL, retaining unspecified '
            'upper EAX only as raw machine evidence.',
            'Normal Anim UnInit emits original AnnounceExpiry twice: ObjectUnInit5F6616 and '
            'Limbo DetachAll5F4D61. Ordinary leaf matches clear on the first call and the second '
            'constructs nothing; alivefalse outer8/Grinding controls retain their repeated constructors.'],
        substitutions=[
            'Existing Reader supplies physical selected SHP bytes at original5B40B0 asset lookup; '
            'native filename formation, original full ART scalar/type reads and image frame reads execute.',
            'Existing JoinedFixture arena supplies operator_new7C8E17 and records operator_delete7C8B3D; '
            'original allocation caller, constructors, Unlimbo/Start/Middle, callbacks and destructors run.',
            'Existing JoinedFixture presentation sinks451F60/452170/456FB0/705D70 are retained unchanged.'])
    native = image_bytes()
    p['fixture_inputs'] = dict(game_speed=dict(address='00A8EB60',
        owner='tools/spatial_oracle/building_construction.py:JoinedFixture',
        native_read='read32', recorded_in=['rows[].game_speed', 'stock_body_readiness[].game_speed',
                                         'observer_validation[].game_speed'],
        boundary='Existing supplied GameOptions state; native option/UI loading excluded'))
    p['original_slices'] = [dict(start=f'{begin:08X}', end_exclusive=f'{end:08X}',
                                hex=file_span(native, begin, end - begin)[1].hex(),
                                sha256=hashlib.sha256(file_span(native, begin, end - begin)[1]).hexdigest())
                            for begin, end in EXPIRY_SPANS]
    p['legacy_payload_preserved'] = dict(rows=420,
        file_sha256='261582222f8dbf26484991d9c5cc35c2c4d54e28fabc5f6bf159a0d2d0669ec9',
        meta_file_sha256='f9afe25a815ec6412d1a4b78034716b0fc3f8adc8696a13e665fdca7451a50bd')
    return p




def generate_display():
    """Slot ART -> allocator stores -> independent Anim/Unit Display order.

    The supplied objects/coordinates isolate the slot field transfer and
    Display registration; this does not execute the full Anim constructor,
    building placement or pixel rendering. Readers and stored sort arithmetic
    execute original instructions, including zero slot values replacing the
    deliberately unequal preexisting Anim fields.
    """
    import tempfile
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.rules_oracle.bridge_anim_inputs import Reader
    from tools.spatial_oracle.building_body_rules import INI, SP as READER_SP
    from tools.spatial_oracle.crate_speed_effect import fixture
    from tools.spatial_oracle.crate_ground_membership import LAYERS, BUFFERS, LOCOMOTORS
    from tools.native_oracle import RET_MAGIC

    art_path = Path(os.environ.get('VERA20K_BUILDING_ANIM_ART', 'ini/ARTMD.INI'))
    raw = art_path.read_bytes()
    names = {'NATBNK', 'CAOILD', 'GAHPAD'}
    sections, lines = lexical(raw, names)
    # This original slot-reader slice never requests a SHP. Keep the Reader's
    # asset boundary empty rather than feeding it unrelated checkout files.
    with tempfile.TemporaryDirectory(prefix='building-anim-display-') as empty:
        m = Reader(Path(empty), sections)
    m.invoke(0x4E7CF0, 0)
    rows = []
    origin = [16384, 15360, 0]
    unit_origin = [16640, 15616, 0]
    for name in sorted(names):
        pointer = m.alloc(0x2000)
        m.invoke(0x45DD90, pointer, (m.cstring(name),))
        m.u.mem_write(READER_SP, bytes(0x400))
        m.u.mem_write(READER_SP + 0x36C, dwords(INI))
        for register, value in ((UC_X86_REG_ESP, READER_SP), (UC_X86_REG_EBP, pointer),
                                (UC_X86_REG_ESI, INI), (UC_X86_REG_EDI, pointer + 0x1F8)):
            m.u.reg_write(register, value)
        run_checked(m.u, *EXPIRY_SLOT_READER, count=2_000_000)
        for slot in ([10, 11, 12, 13] if name == 'NATBNK' else [3]):
            address = pointer + 0x44 * slot
            scalar = lambda offset: struct.unpack('<i', m.u.mem_read(address + offset, 4))[0]
            fields = dict(slot=slot, normal=m.string(address + 0xF4C),
                          damaged=m.string(address + 0xF5C),
                          garrisoned=m.string(address + 0xF6C),
                          x=scalar(0xF7C), y=scalar(0xF80),
                          z_adjust=scalar(0xF84), y_sort=scalar(0xF88))
            building, anim = m.alloc(0x800), m.alloc(0x1C8)
            m.u.mem_write(building + 0x520, dwords(pointer))
            m.u.mem_write(anim, dwords(0x7E3354))
            m.u.mem_write(anim + 0x9C, dwords(*origin))
            m.u.mem_write(anim + 0x100, dwords(999, 888))
            for register, value in ((UC_X86_REG_ESI, building),
                                    (UC_X86_REG_EDI, 0x44 * slot), (UC_X86_REG_EBP, anim)):
                m.u.reg_write(register, value)
            run_checked(m.u, 0x45197B, 0x4519A8, count=100)
            copied = dict(z_adjust=struct.unpack('<i', m.u.mem_read(anim + 0x100, 4))[0],
                          y_sort=struct.unpack('<i', m.u.mem_read(anim + 0x104, 4))[0],
                          building_anim=m.u.mem_read(anim + 0x118, 1)[0],
                          sort_key=m.invoke(0x422BC0, anim))
            rows.append(dict(building=name, slot=fields, copied=copied))
    assert not m.asset_loaded

    orders = []
    for reversed_insertion in (False, True):
        u, sp, actors, _ = fixture(dict(actors=[dict(kind='unit') for _ in range(3)]))
        def invoke(address, receiver, args=()):
            u.reg_write(UC_X86_REG_ECX, receiver)
            u.reg_write(UC_X86_REG_ESP, sp)
            u.mem_write(sp, dwords(RET_MAGIC, *args))
            run_checked(u, address, RET_MAGIC, count=10_000)
            assert u.reg_read(UC_X86_REG_ESP) == sp + 4 + 4 * len(args)
            return u.reg_read(UC_X86_REG_EAX)
        u.reg_write(UC_X86_REG_ESP, sp)
        run_checked(u, 0x4A8630, 0x4A866D, count=100)
        for layer in range(5):
            u.mem_write(LAYERS + layer * 24 + 4, dwords(BUFFERS + layer * 0x100, 16))
        for index, actor in enumerate(actors):
            u.mem_write(actor + 0x74, b'\1')
            u.mem_write(actor + 0x94, dwords(-1))
            if index == 1:
                u.mem_write(actor + 0x9C, dwords(*unit_origin))
                loco = LOCOMOTORS + 0x100
                invoke(0x4AF540, loco)
                u.mem_write(loco + 0xC, dwords(actor))
                u.mem_write(actor + 0x674, dwords(loco + 4))
            else:
                u.mem_write(actor, dwords(0x7E3354))
                u.mem_write(actor + 0x9C, dwords(*origin))
                config = actor + 0x800
                u.mem_write(actor + 0xC8, dwords(config))
                u.mem_write(config + 0x364, dwords(2))  # supplied retail Ground layer
                source = next(row for row in rows if row['building'] == 'NATBNK'
                              and row['slot']['slot'] == (10 if index == 2 else 11))
                u.mem_write(actor + 0x104, dwords(source['copied']['y_sort']))
        keys = [invoke(0x5F6BD0 if i == 1 else 0x422BC0, actor)
                for i, actor in enumerate(actors)]
        inserted = [2, 1, 0] if reversed_insertion else [0, 1, 2]
        for index in inserted:
            invoke(0x4A9720, 0x87F7E8, (actors[index],))
        count = struct.unpack('<i', u.mem_read(LAYERS + 48 + 16, 4))[0]
        pointers = struct.unpack('<' + 'I' * count, u.mem_read(BUFFERS + 0x200, count * 4))
        orders.append(dict(inserted=inserted, keys=keys,
                           order=[actors.index(pointer) for pointer in pointers]))
    return dict(art=dict(file='ARTMD.INI', sha256=hashlib.sha256(raw).hexdigest(),
                         physical_sections=sections, lines=lines),
                origin=origin, unit_origin=unit_origin, rows=rows, orders=orders)


def display_provenance():
    return provenance(
        entry_points=dict(building_type_constructor=0x45DD90, art_slots=0x4615CA,
                          slot_instance_stores=0x45197B, anim_sort=0x422BC0,
                          display_submit=0x4A9720, object_sort=0x5F6BD0),
        assumptions=[
            'Physical ARTMD lexical strings go through the existing native Reader. Original INI file loading and archive lookup are outside this comparison. Slot stores cover 45197B..4519A8 after a supplied Anim construction; full allocator/constructor not claimed.',
            'Supplied stored world coordinates (16384,15360,0), Unit centre (16640,15616,0), null Anim owner and Ground layer. Registration uses actual Anim/Unit vtables and native Drive constructor with capacity16 display buffers.',
            'Normal/damaged names are native slot-reader results; damaged variant admission, frame timers, lighting, GPU pixels and full game scene parity are not claimed.'
        ], substitutions=[],
        scope='Six retail building animation slot reader/store/sort rows plus two native back-wall, parked-Unit, front-wall Display insertion histories. CAOILD Active and GAHPAD Active offset controls share the same owner.')

if __name__ == '__main__':
    if '--display' in sys.argv:
        sys.argv.remove('--display')
        finish_vectors(generate_display, Path(__file__).with_name('building_slot_replacement.display.json'), provenance=display_provenance)
        raise SystemExit(0)
    if '--expiry' in sys.argv:
        paths = ('tools/native_oracle.py', 'tools/native_slope.py',
                 'tools/spatial_oracle/building_slot_replacement.py',
                 'tools/spatial_oracle/building_construction.py',
                 'tools/spatial_oracle/refinery_dock.py',
                 'tools/spatial_oracle/building_repair.py',
                 'tools/spatial_oracle/building_body_rules.py',
                 'tools/spatial_oracle/anim_bouncer_launch.py',
                 'tools/rules_oracle/bridge_anim_inputs.py',
                 'tools/rules_oracle/bridge_anim_lists.py',
                 'tools/projectile_oracle/bridge_render_inputs.py')
        finish_vectors(generate_expiry, Path(__file__).with_name('building_slot_replacement.expiry.json'),
                       provenance=expiry_metadata,
                       argv=[argument for argument in sys.argv[1:] if argument != '--expiry'],
                       source_paths={path: Path(path) for path in paths})
    else:
        finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
