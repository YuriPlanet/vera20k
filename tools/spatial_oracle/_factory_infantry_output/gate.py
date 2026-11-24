"""Additive original51DFF0 -> 481180 Gate and incoming-coordinate controls.

Imports the immutable passenger fixture/Foot observation owner. The native
Gate lookup, mission/door reader, membership, placement, RNG, class mark and
class return execute unchanged; Foot4D7170 remains the inherited success seam.
No native gameplay function is ported here.
"""
from pathlib import Path
import hashlib
import json
import struct
import traceback

HERE = Path(__file__).resolve().parent
from tools import native_oracle as native
from tools.spatial_oracle import passenger_escape as inherited
from tools.spatial_oracle import refinery_dock
from tools.spatial_oracle.building_body_rules import Fixture, INI, SP, TYPE
from tools.spatial_oracle.unit_source_scatter import SCENARIO
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EBP, UC_X86_REG_ECX,
    UC_X86_REG_ESI, UC_X86_REG_ESP,
)

GATE = inherited.REGION + 0x24000
GATE_TYPE = inherited.REGION + 0x26000
RNGS = {'main': 0x886B88, 'scenario': SCENARIO + 0x218, 'mapgen': 0xABE890}
SPANS = [
    ('InfantryUnlimbo', 0x51DFF0, 0x51E134),
    ('CellPlaceInfantry', 0x481180, 0x48149B),
    ('CellFindObjectByRTTI', 0x47C4D0, 0x47C519),
    ('BuildingIsOpenGate', 0x4525F0, 0x45262D),
    ('DoorIsOpen', 0x4A51B0, 0x4A51C3),
    ('MapIncomingMembership', 0x578460, 0x578537),
    ('MapRetainedMembership', 0x578540, 0x5785EF),
    ('MapPlaceInfantry', 0x4ACA10, 0x4ACA83),
    ('BuildingTypeCtorZero', 0x45DD90, 0x45DDA6),
    ('BuildingTypeGateCtor', 0x45E115, 0x45E11B),
    ('BuildingTypeGateRead', 0x4609EA, 0x460A19),
    ('ReadBoolBody', 0x5295F0, 0x5297A6),
    ('ObjectUnlimboActiveByte', 0x5F4EC0, 0x5F4F0F),
    ('SlaveDeploy', 0x6B04C0, 0x6B068B),
    ('BuildingSurvivorsThroughCrewCounterCleanup', 0x442D90, 0x44328E),
    ('SaleCrewCaller', 0x44A699, 0x44A79A),
]


def require(value, message):
    if not value:
        raise ValueError(message)


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def rng(u):
    return {name: bytes(u.mem_read(address, 0x3F4)).hex()
            for name, address in RNGS.items()}


def coord(u, pointer):
    return list(struct.unpack('<3i', u.mem_read(pointer, 12)))


def cell_state(u, pointer):
    return dict(pointer=f'{pointer:08X}',
                coordinate=list(struct.unpack('<hh', u.mem_read(pointer + 0x24, 4))),
                level_slope=list(u.mem_read(pointer + 0x11B, 2)),
                ground_bits=u.mem_read(pointer + 0x124, 1)[0],
                bridge_bits=u.mem_read(pointer + 0x128, 1)[0],
                ground_owner=struct.unpack('<i', u.mem_read(pointer + 0x54, 4))[0],
                ground_first=f'{struct.unpack("<I", u.mem_read(pointer + 0xE4, 4))[0]:08X}')


def instructions():
    from capstone import Cs, CS_ARCH_X86, CS_MODE_32
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    image = native.image_bytes()
    rows = []
    for name, start, end in SPANS:
        offset, raw = native.file_span(image, start, end - start)
        rows.append(dict(name=name, start=f'{start:08X}', end_exclusive=f'{end:08X}',
                         file_offset=offset, bytes=raw.hex(),
                         sha256=hashlib.sha256(raw).hexdigest(),
                         instructions=[dict(pc=f'{i.address:08X}', bytes=i.bytes.hex(),
                                            mnemonic=i.mnemonic, operands=i.op_str)
                                       for i in md.disasm(raw, start)]))
    return rows


def gate_reader():
    # Only the existing cached-INI input owner is reused. Native field-read
    # instructions use the actual Gate literal and actual type ID section.
    f = Fixture()
    u = f.u
    u.mem_write(TYPE + 0x1F8, b'GAGATE_A\0')
    u.reg_write(UC_X86_REG_EBX, 0xFFFFFFFF)
    native.run_checked(u, 0x45DD9A, 0x45DD9C, count=20)
    u.reg_write(UC_X86_REG_ESI, TYPE)
    native.run_checked(u, 0x45E115, 0x45E11B, count=20)
    ctor = u.mem_read(TYPE + 0x16B7, 1)[0]
    retail = json.loads((HERE / 'inputs/infantry-unlimbo-gate-retail.json').read_text(encoding='utf-8'))
    rows = []
    for layer in retail['authored_layers']:
        if not layer['source_present']:
            rows.append(dict(layer=layer['layer'], reader_invoked=False,
                             before=u.mem_read(TYPE + 0x16B7, 1)[0],
                             after=u.mem_read(TYPE + 0x16B7, 1)[0]))
            continue
        raw = layer['authored']['raw_value']
        f.ini(0x81AA8C, raw)
        before = u.mem_read(TYPE + 0x16B7, 1)[0]
        u.mem_write(SP, bytes(0x100))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_EBP, TYPE)
        u.reg_write(UC_X86_REG_EBX, TYPE + 0x1F8)
        u.reg_write(UC_X86_REG_ESI, INI)
        u.reg_write(UC_X86_REG_ECX, INI)
        native.run_checked(u, 0x4609EA, 0x460A19, count=100_000,
                           required_addresses=(0x4609FF, 0x5295F0, 0x460A04, 0x460A13))
        rows.append(dict(layer=layer['layer'], reader_invoked=True, raw=raw,
                         before=before, after=u.mem_read(TYPE + 0x16B7, 1)[0],
                         call='004609FF', entry='005295F0', return_pc='00460A04',
                         store='00460A13', current_byte_default=before))
    return dict(ctor_default=ctor, literal_address='0081AA8C',
                literal=bytes(u.mem_read(0x81AA8C, 8)).hex(), section='GAGATE_A',
                retail_source=sha(HERE / 'inputs/infantry-unlimbo-gate-retail.json'), rows=rows,
                limits=['Original constructor XOR and Gate store execute as two narrow original instruction slices, not a whole type constructor.',
                        'Original Gate reader block and full ReadBool execute on the existing cached-section/native CRC input owner with production-parsed physical layer values.',
                        'Section creation/cache preparation is supplied; LANGRULE is absent and therefore no native read is invoked for that layer.'])


def cases():
    center = [3968, 3968, 0]
    base = dict(input=center, cell=[15, 15], priority=0, bits=0x40,
                gate_present=True, gate_type=True, mission=24,
                door_state=[0, 1], floor_level=0, slope=0, seed=31)
    rows = []
    for name, changes in [
        ('ordinary_open_gate_center', {}),
        ('ordinary_closed_gate_center', dict(door_state=[0, 0])),
        ('ordinary_open_door_wrong_mission', dict(mission=5)),
        ('ordinary_non_gate_building', dict(gate_type=False)),
        ('ordinary_missing_ground_building', dict(gate_present=False)),
        ('ordinary_vehicle_refuses_before_gate', dict(bits=0x60)),
        ('priority_vehicle_and_closed_gate', dict(priority=1, bits=0x60, door_state=[0, 0])),
        ('ordinary_open_gate_all_spots_full', dict(bits=0x5F)),
        ('above_floor_skips_gate_and_place', dict(input=[3968, 3968, 71], bits=0x60, door_state=[0, 0])),
        ('below_floor_skips_gate_and_place', dict(input=[3968, 3968, -73], bits=0x60, door_state=[0, 0])),
        ('positive_linear_alias_outside_priority', dict(input=[527 * 256 + 128, 14 * 256 + 128, 0], bits=0x60, door_state=[0, 0])),
        ('negative_linear_alias_outside_priority', dict(input=[-481 * 256 - 128, 16 * 256 + 128, 0], cell=[31, 15], bits=0x60, door_state=[0, 0])),
        ('negative_one_ordinary_open_gate', dict(input=[-1, 15 * 256 + 128, 0], cell=[0, 15])),
        ('negative_half_ordinary_open_gate', dict(input=[-128, 15 * 256 + 128, 0], cell=[0, 15])),
        ('negative_half_priority_closed_gate', dict(input=[-128, 15 * 256 + 128, 0], cell=[0, 15], priority=1, bits=0x60, door_state=[0, 0])),
        ('negative_one_priority_closed_gate', dict(input=[-1, 15 * 256 + 128, 0], cell=[0, 15], priority=1, bits=0x60, door_state=[0, 0])),
        ('open_gate_nonzero_level_slope', dict(input=[3968, 3968, 208], floor_level=2, slope=0)),
        ('open_gate_sloped_input_floor', dict(input=[3968, 3968, 0], floor_level=2, slope=1, sample_input_floor=True)),
    ]:
        rows.append(dict(base, name=name, **changes))
    return rows


def run(case, image_sections):
    prior = dict(name=case['name'], cell=case['cell'], seed=case['seed'], sub=[128, 128])
    u, call, read32 = inherited.make_fixture(prior)
    require(all(bytes(u.mem_read(a, len(b))) == b for a, b in image_sections),
            'Inherited fixture changed original executable sections')
    here = inherited.cell(*case['cell'])
    u.mem_write(here + 0x11B, bytes([case['floor_level'] & 255, case['slope']]))
    u.mem_write(here + 0x124, inherited.dwords(case['bits']))
    u.mem_write(0xA8E7AC, inherited.dwords(case['priority']))
    refinery_dock.place_building(u, GATE, case['cell'])
    u.mem_write(GATE + 0x520, inherited.dwords(GATE_TYPE))
    u.mem_write(GATE_TYPE + 0x16B7, bytes([case['gate_type']]))
    u.mem_write(GATE + 0xAC, inherited.dwords(case['mission']))
    u.mem_write(GATE + 0xB4, inherited.dwords(-1))
    u.mem_write(GATE + 0x30, inherited.dwords(0))
    call(0x4A50F0, GATE + 0x350, [])
    u.mem_write(GATE + 0x368, bytes(case['door_state']))
    u.mem_write(here + 0xE4, inherited.dwords(GATE if case['gate_present'] else 0))
    gate_prior = dict(pointer=f'{GATE:08X}',
                      primary_vtable=f'{read32(GATE):08X}',
                      rtti_vtable=f'{read32(GATE + 4):08X}',
                      rtti_function=f'{read32(read32(GATE) + 0x2C):08X}',
                      mission_function=f'{read32(read32(GATE) + 0x184):08X}',
                      type_pointer=f'{read32(GATE + 0x520):08X}',
                      gate_byte=u.mem_read(GATE_TYPE + 0x16B7, 1)[0],
                      owner_pointer=f'{read32(GATE + 0x21C):08X}',
                      owner_index=read32(refinery_dock.HOUSE + 0x30),
                      current_mission=struct.unpack('<i', u.mem_read(GATE + 0xAC, 4))[0],
                      queued_mission=struct.unpack('<i', u.mem_read(GATE + 0xB4, 4))[0],
                      original_door_ctor='004A50F0',
                      door_state=list(u.mem_read(GATE + 0x368, 2)),
                      next_pointer=f'{read32(GATE + 0x30):08X}')
    infantry_prior = dict(pointer=f'{inherited.pax(0):08X}',
                          primary_vtable=f'{read32(inherited.pax(0)):08X}',
                          type_pointer=f'{read32(inherited.pax(0) + 0x6C0):08X}',
                          type_vtable=f'{read32(inherited.ITYPE):08X}',
                          type_jumpjet=read32(inherited.ITYPE + 0x5E8),
                          type_speed=read32(inherited.ITYPE + 0x67C),
                          type_movement_zone=read32(inherited.ITYPE + 0x5B4),
                          owner_pointer=f'{read32(inherited.pax(0) + 0x21C):08X}',
                          owner_index=read32(refinery_dock.HOUSE + 0x30),
                          doing=read32(inherited.pax(0) + 0x6C4),
                          water_sentinel=read32(inherited.pax(0) + 0x6E8),
                          limbo=u.mem_read(inherited.pax(0) + 0x81, 1)[0],
                          marked=u.mem_read(inherited.pax(0) + 0x74, 1)[0],
                          on_bridge=u.mem_read(inherited.pax(0) + 0x8C, 1)[0])
    u.mem_write(inherited.COORD, inherited.dwords(*case['input']))
    setup_sample = None
    if case.get('sample_input_floor'):
        call(0x578080, inherited.MAP, [inherited.COORD])
        setup_sample = struct.unpack('<i', inherited.dwords(u.reg_read(UC_X86_REG_EAX)))[0]
        u.mem_write(inherited.COORD + 8, inherited.dwords(setup_sample))
    requested_xyz = coord(u, inherited.COORD)
    call(0x578540, inherited.MAP, [here, 1])
    retained_membership = u.reg_read(UC_X86_REG_EAX) & 255
    before = rng(u)
    events = []
    inherited_events = []
    returns = {}
    functions = {0x578080: 'floor', 0x578460: 'incoming_membership',
                 0x565730: 'world_cell', 0x481180: 'place',
                 0x47C4D0: 'ground_building_lookup', 0x4525F0: 'is_open_gate',
                 0x4A51B0: 'door_is_open', 0x5217C0: 'class_occupation_mark',
                 0x65C7E0: 'random_ranged', 0x65C780: 'random_raw',
                 0x4D7170: 'foot_handoff'}

    def observer(_u, pc, _size, _data):
        for index in returns.pop(pc, []):
            event = events[index]
            result = u.reg_read(UC_X86_REG_EAX)
            event['return_eax'] = result
            if event['name'] == 'place':
                event['returned_coordinate'] = coord(u, result)
            elif event['name'] == 'world_cell':
                event['returned_cell'] = cell_state(u, result)
            elif event['name'].startswith('random_'):
                event['rng_after'] = rng(u)
        if pc not in functions:
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        this = u.reg_read(UC_X86_REG_ECX)
        caller = read32(sp)
        name = functions[pc]
        event = dict(name=name, entry=f'{pc:08X}', return_pc=f'{caller:08X}', this=f'{this:08X}')
        if name in ('floor', 'world_cell', 'class_occupation_mark', 'foot_handoff'):
            event['coordinate'] = coord(u, read32(sp + 4))
        if name == 'incoming_membership':
            event['cell'] = list(struct.unpack('<hh', u.mem_read(read32(sp + 4), 4)))
            event['mode'] = read32(sp + 8)
        elif name == 'place':
            event['cell'] = cell_state(u, this)
            event['input_coordinate'] = coord(u, read32(sp + 8))
            event['priority_bridge_center'] = [read32(sp + off) & 255 for off in (12, 16, 20)]
        elif name == 'ground_building_lookup':
            event['rtti_layer'] = [read32(sp + 4), read32(sp + 8)]
        elif name == 'is_open_gate':
            event['gate_type'] = u.mem_read(GATE_TYPE + 0x16B7, 1)[0]
            event['mission'] = struct.unpack('<i', u.mem_read(GATE + 0xAC, 4))[0]
            event['door_state'] = list(u.mem_read(GATE + 0x368, 2))
        elif name.startswith('random_'):
            event['stream'] = next((name for name, pointer in RNGS.items() if pointer == this), None)
            event['rng_before'] = rng(u)
            if name == 'random_ranged':
                event['range'] = [read32(sp + 4), read32(sp + 8)]
        if name == 'foot_handoff':
            event['facing'] = read32(sp + 8)
        index = len(events)
        events.append(event)
        returns.setdefault(caller, []).append(index)

    u.hook_add(UC_HOOK_CODE, observer)
    inherited.observe(u, read32, prior, inherited_events)
    cell_before = cell_state(u, here)
    try:
        call(inherited.UNLIMBO, inherited.pax(0), [inherited.COORD, 128])
    except Exception as exc:
        # Read-only failure retention; no native result or prior is supplied.
        exc.factory_gate_context = dict(input=case, requested_xyz=requested_xyz,
            gate_prior=gate_prior, infantry_prior=infantry_prior,
            cell_before=cell_before, cell_after=cell_state(u, here),
            rng_before=before, rng_after=rng(u), events=events,
            inherited_events=inherited_events,
            original_code_unchanged=all(bytes(u.mem_read(a, len(b))) == b
                                        for a, b in image_sections))
        raise
    result = u.reg_read(UC_X86_REG_EAX) & 255
    after = rng(u)
    require(all(bytes(u.mem_read(a, len(b))) == b for a, b in image_sections),
            'Original executable sections changed during class control')
    require(all(event.get('stream') in (None, 'scenario') for event in events),
            'Unexpected RNG stream in class placement')
    return dict(input=case, class_entry='0051DFF0', synthetic_return=f'{native.RET_MAGIC:08X}',
                active_cell_list_byte=u.mem_read(0xA8E9A0, 1)[0],
                map_fields=[struct.unpack('<i', u.mem_read(inherited.MAP + 0xF4, 4))[0],
                            *struct.unpack('<4i', u.mem_read(inherited.MAP + 0xFC, 16))],
                setup_native_floor_sample=setup_sample, requested_xyz=requested_xyz,
                retained_membership_diagnostic=dict(entry='00578540', mode=1, inside=retained_membership),
                gate_prior=gate_prior, infantry_prior=infantry_prior,
                cell_before=cell_before, cell_after=cell_state(u, here),
                result=result, published_infantry_xyz=coord(u, inherited.pax(0) + 0x9C),
                water_sentinel=read32(inherited.pax(0) + 0x6E8),
                rng_before=before, rng_after=after, events=events,
                inherited_events=inherited_events, original_code_unchanged=True)


def generate():
    image = native.image_bytes()
    sections = [(native.IMAGE_BASE + rva, image[raw:raw + size])
                for rva, raw, size, virtual, flags in native._sections(image)
                if flags & 0x20000000]
    rows = []
    fault = None
    readers = None
    for case in cases():
        try:
            rows.append(run(case, sections))
        except Exception as exc:
            fault = dict(type=type(exc).__name__, message=str(exc),
                traceback=traceback.format_exc(),
                case=case, original_context=getattr(exc, 'factory_gate_context', None),
                native_diagnostics=getattr(exc, 'diagnostics', None))
            break
    if fault is None:
        try:
            readers = gate_reader()
        except Exception as exc:
            fault = dict(type=type(exc).__name__, message=str(exc),
                traceback=traceback.format_exc(), case='gate_reader',
                native_diagnostics=getattr(exc, 'diagnostics', None))
    return dict(schema_version=1,
                gate_reader=readers, original_instructions=instructions(), rows=rows,
                bounds=['Prepared immutable passenger map/House/type/Walk owner with supplied object state, active cell-list byte1, original Door ctor and supplied stable state/mission; no whole GAGATE_A constructor or live Gate mission.',
                        'Original51DFF0 and481180 execute with original47C4D0/Building4525F0/Door4A51B0, original membership/floor, class mark and class return. The inherited Foot4D7170 observer supplies success and writes location/mark/limbo; full Object/Techno/Idle remains outside these controls.',
                        'Full three 1012-byte RNG buffers are retained before/after and at original random entries/returns; only Scenario is originally seeded31 in the inherited fixture. Main/MapGen retain supplied mapped prior bytes and are observed unchanged.',
                        'Original executable sections are checked before/after every control. Native vtables are unchanged; no Gate, membership, height, RNG, placement or occupation decision is supplied.',
                        'Incoming positive/negative linear aliases select the existing real Cell through the original512-stride table. Negative-1/128 rows also preserve signed truncating lookup versus incoming AND-to-256 placement origin.'],
                fault=fault)

