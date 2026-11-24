"""Original occupied-Building weapon, occupancy, foundation and scan-bound queries.

Reproduce from the repository root with Unicorn 2.1.4 and the original executable:
    python -m tools.garrison_oracle.weapon_range --check

The original Building/Infantry vtables and complete GetWeapon, IsOccupied,
HalfFoundation, GetCurrentWeapon and GetWeaponRange bodies execute. The occupied
Greatest_Threat bound is a bounded original slice (0x6F917D..0x6F91A7), stopping
before its air pre-pass and ring walk. Nothing substitutes a callable or writes
original code. Types, occupant order/rank, firing index and prior bound are supplied;
this is not boarding, fire-index lifecycle, target selection or full scan parity.

Rust schema: `cases` contains full `input`, `occupied`, `half_foundation`,
`weapon_ids` in root `weapon_indices` order, `current_weapon_id`, `weapon_ranges`
for slots 0/1, and `ring_bound_cells`. IDs are fixture WeaponType identities,
null for no WeaponType. `foundations` executes all 22 original width/height table
entries and HalfFoundation. Root `weapon_range_leptons` gives the supplied Range
of each symbolic weapon. All output values come from original instructions.
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import (UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESI,
                               UC_X86_REG_ESP)

from tools.native_oracle import (OracleError, SCRATCH, SCRATCH_SIZE, STACK_BASE,
                                 STACK_SIZE, call, finish_vectors, load_image,
                                 provenance, run_checked)
from tools.spatial_oracle.map_queries import dwords

BUILDING_VTABLE, INFANTRY_VTABLE = 0x7E3EBC, 0x7EB058
GET_WEAPON, IS_OCCUPIED, HALF_FOUNDATION = 0x4526F0, 0x458DD0, 0x458E00
GET_CURRENT_WEAPON, GET_WEAPON_RANGE = 0x70E1A0, 0x7012C0
FOUNDATION_WIDTH, FOUNDATION_HEIGHT = 0x45EC90, 0x45ECA0
RING, RING_END = 0x6F917D, 0x6F91A7
BUILDING, TYPE, RULES, VECTOR = (SCRATCH + offset for offset in (0, 0x1000, 0x3000, 0x5000))
OCCUPANTS, OCCUPANT_TYPES, WEAPONS = (SCRATCH + offset for offset in (0x6000, 0x8000, 0xE000))
SLOTS = (('primary', 0x898), ('secondary', 0x8B4),
         ('elite_primary', 0xA94), ('elite_secondary', 0xAB0))
WEAPON_INDICES = (0, 1, -1)
WEAPON_RANGE_LEPTONS = {
    'B_P': 512, 'B_S': 768, 'B_EP': 1024, 'B_ES': 1280,
    'O_P': 1024, 'O_S': 256, 'O_EP': 1280, 'O_ES': 512,
    'O_UC': 1536, 'O_EUC': 1792, 'OTHER_UC': 768,
}
OCCUPANT_DEFAULTS = dict(veterancy=0.0, primary='O_P', secondary='O_S',
                         elite_primary='O_EP', elite_secondary='O_ES',
                         occupy_weapon='O_UC', elite_occupy_weapon='O_EUC')
DEFAULTS = dict(can_be_occupied=True, can_occupy_fire=True, foundation_id=3,
                occupy_range=5, previous_ring=99, fire_index=0,
                building_veterancy=0.0, building_turret_count=0,
                current_weapon_number=0,
                building_weapons=dict(primary='B_P', secondary='B_S',
                                      elite_primary='B_EP', elite_secondary='B_ES'))


def resolve(sparse):
    unknown = set(sparse) - set(DEFAULTS) - {'name', 'occupants'}
    if unknown:
        raise OracleError(f"{sparse['name']}: unknown inputs {sorted(unknown)}")
    row = dict(DEFAULTS, **sparse)
    row['building_weapons'] = {**DEFAULTS['building_weapons'], **sparse.get('building_weapons', {})}
    row['occupants'] = []
    for occupant in sparse.get('occupants', [{}]):
        if set(occupant) - set(OCCUPANT_DEFAULTS):
            raise OracleError(f"{sparse['name']}: unknown occupant inputs")
        row['occupants'].append({**OCCUPANT_DEFAULTS, **occupant})
    if len(row['occupants']) > 3 or not 0 <= row['foundation_id'] < 22:
        raise OracleError(f"{sparse['name']}: outside the declared fixture layout")
    return row


def fixture(row):
    """Supply only the runtime fields these original pure queries read."""
    memory = bytearray(SCRATCH_SIZE)

    def write(address, value):
        offset = address - SCRATCH
        if not 0 <= offset <= len(memory) - len(value):
            raise OracleError('fixture write outside the shared scratch mapping')
        memory[offset:offset + len(value)] = value

    weapons = {name: WEAPONS + 0x100 * index
               for index, name in enumerate(WEAPON_RANGE_LEPTONS)}
    for name, pointer in weapons.items():
        write(pointer + 0xB4, dwords(WEAPON_RANGE_LEPTONS[name]))

    def slots(kind, spec):
        for key, offset in SLOTS:
            name = spec[key]
            write(kind + offset, dwords(weapons[name] if name is not None else 0))

    write(BUILDING, dwords(BUILDING_VTABLE))
    write(BUILDING + 0x14, b'\x03')
    write(BUILDING + 0x138, dwords(row['current_weapon_number']))
    write(BUILDING + 0x150, struct.pack('<f', row['building_veterancy']))
    write(BUILDING + 0x520, dwords(TYPE))
    write(BUILDING + 0x688, dwords(VECTOR))
    write(BUILDING + 0x694, dwords(len(row['occupants'])))
    write(BUILDING + 0x69C, dwords(row['fire_index']))
    write(TYPE + 0x808, dwords(row['building_turret_count']))
    write(TYPE + 0xEF0, dwords(row['foundation_id']))
    write(TYPE + 0x157B, bytes([int(row['can_be_occupied'])]))
    write(TYPE + 0x157C, bytes([int(row['can_occupy_fire'])]))
    write(RULES + 0xF48, dwords(row['occupy_range']))
    slots(TYPE, row['building_weapons'])
    for index, spec in enumerate(row['occupants']):
        occupant, kind = OCCUPANTS + index * 0x800, OCCUPANT_TYPES + index * 0x2000
        write(VECTOR + index * 4, dwords(occupant))
        write(occupant, dwords(INFANTRY_VTABLE))
        write(occupant + 0x14, b'\x07')
        write(occupant + 0x150, struct.pack('<f', spec['veterancy']))
        write(occupant + 0x6C0, dwords(kind))
        slots(kind, spec)
        for key, offset in (('occupy_weapon', 0xE04), ('elite_occupy_weapon', 0xE20)):
            name = spec[key]
            write(kind + offset, dwords(weapons[name] if name is not None else 0))
    return {SCRATCH: bytes(memory), 0x8871E0: dwords(RULES)}, {pointer: name for name, pointer in weapons.items()}


def query(entry, writes, args=(), this=BUILDING):
    packet = call(entry, ecx=this, stack_args=[arg & 0xFFFFFFFF for arg in args], writes=writes,
                  dumps={'scratch': (SCRATCH, SCRATCH_SIZE)}, timeout_instr=2000,
                  required_addresses=[entry])
    after = bytes.fromhex(packet['dumps']['scratch'])
    if after != writes[SCRATCH]:
        raise OracleError(f'pure query 0x{entry:08X} mutated fixture state')
    return packet['eax'], after


def weapon_id(packet, identities):
    slot, memory = packet
    if slot == 0:
        return None
    if not SCRATCH <= slot < SCRATCH + SCRATCH_SIZE - 3:
        raise OracleError(f'GetWeapon returned an undeclared slot 0x{slot:08X}')
    pointer = struct.unpack_from('<I', memory, slot - SCRATCH)[0]
    if pointer == 0:
        return None
    if pointer not in identities:
        raise OracleError(f'GetWeapon returned an undeclared WeaponType 0x{pointer:08X}')
    return identities[pointer]


def ring_bound(writes, previous):
    # This interior slice is not a callable. Use the shared loader/checked runner
    # with its original live-register and ESP+38 frame inputs, no return shim.
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(SCRATCH, SCRATCH_SIZE)
    u.mem_map(STACK_BASE, STACK_SIZE)
    for address, data in writes.items():
        u.mem_write(address, data)
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(sp + 0x38, dwords(previous))
    u.reg_write(UC_X86_REG_ESI, BUILDING)
    u.reg_write(UC_X86_REG_EDX, BUILDING_VTABLE)
    u.reg_write(UC_X86_REG_ECX, BUILDING)
    u.reg_write(UC_X86_REG_ESP, sp)
    run_checked(u, RING, RING_END, count=2000,
                required_addresses=[RING, 0x6F917F, IS_OCCUPIED])
    if u.reg_read(UC_X86_REG_ESP) != sp or bytes(u.mem_read(SCRATCH, SCRATCH_SIZE)) != writes[SCRATCH]:
        raise OracleError('scan-bound slice changed its frame or object/type inputs')
    return struct.unpack('<i', u.mem_read(sp + 0x38, 4))[0]


def execute(sparse):
    row = resolve(sparse)
    writes, identities = fixture(row)
    return dict(input=row, occupied=bool(query(IS_OCCUPIED, writes)[0]),
                half_foundation=query(HALF_FOUNDATION, writes)[0],
                weapon_ids=[weapon_id(query(GET_WEAPON, writes, [index]), identities)
                            for index in WEAPON_INDICES],
                current_weapon_id=weapon_id(query(GET_CURRENT_WEAPON, writes), identities),
                weapon_ranges=[query(GET_WEAPON_RANGE, writes, [index])[0] for index in (0, 1)],
                ring_bound_cells=ring_bound(writes, row['previous_ring']))


def inputs():
    rows = [dict(name=f'foundation_{index}', foundation_id=index) for index in range(22)]
    rows += [
        dict(name='not_occupiable', can_be_occupied=False),
        dict(name='occupants_cannot_fire', can_occupy_fire=False),
        dict(name='empty', occupants=[]),
        dict(name='firing_index_at_count', fire_index=1),
        dict(name='firing_index_past_count', fire_index=7),
        dict(name='second_occupant_selected', fire_index=1,
             occupants=[{}, {'occupy_weapon': 'OTHER_UC'}]),
        dict(name='veteran_uses_normal_occupy_weapon', occupants=[{'veterancy': 1.0}]),
        dict(name='elite_uses_elite_occupy_weapon', occupants=[{'veterancy': 2.0}]),
        dict(name='normal_without_occupy_weapon_uses_primary', occupants=[{'occupy_weapon': None}]),
        dict(name='elite_without_elite_occupy_uses_elite_primary',
             occupants=[{'veterancy': 2.0, 'elite_occupy_weapon': None}]),
        dict(name='elite_primary_absent_uses_normal_primary',
             occupants=[{'veterancy': 2.0, 'elite_occupy_weapon': None, 'elite_primary': None}]),
        dict(name='normal_occupy_absent_does_not_use_elite_occupy',
             occupants=[{'occupy_weapon': None, 'primary': None}]),
        dict(name='elite_occupy_absent_does_not_use_normal_occupy',
             occupants=[{'veterancy': 2.0, 'elite_occupy_weapon': None,
                         'elite_primary': None, 'primary': None}]),
        dict(name='occupied_building_without_own_weapons',
             building_weapons={key: None for key, _ in SLOTS}),
        dict(name='elite_building_does_not_promote_normal_occupant', building_veterancy=2.0),
        dict(name='unoccupied_elite_building_uses_own_elite_weapons',
             building_veterancy=2.0, occupants=[]),
        dict(name='turreted_current_slot_still_uses_occupant',
             building_turret_count=1, current_weapon_number=1),
        dict(name='unoccupied_turreted_current_slot_uses_secondary', occupants=[],
             building_turret_count=1, current_weapon_number=1),
    ]
    rows += [dict(name=f'ring_rule_{value}_prior_{previous}', occupy_range=value,
                  previous_ring=previous, foundation_id=18)
             for value in (5, 0, -1, -3, 0x7FFFFFFF, -0x80000000)
             for previous in (0, 1000)]
    return rows


def generate():
    foundations = []
    for index in range(22):
        writes, _ = fixture(resolve(dict(name=f'foundation_{index}', foundation_id=index)))
        foundations.append(dict(foundation_id=index,
                                width=query(FOUNDATION_WIDTH, writes, this=TYPE)[0],
                                height=query(FOUNDATION_HEIGHT, writes, [0], this=TYPE)[0],
                                half_foundation=query(HALF_FOUNDATION, writes)[0]))
    return dict(weapon_indices=list(WEAPON_INDICES), weapon_range_leptons=WEAPON_RANGE_LEPTONS,
                foundations=foundations, cases=[execute(row) for row in inputs()])


def metadata():
    return dict(provenance(
        scope='Original Building GetWeapon/IsOccupied/HalfFoundation, inherited GetCurrentWeapon/GetWeaponRange and all 22 foundation dimensions; original occupied Greatest_Threat bound slice before its air pre-pass. No full scan, boarding, fire-index writer or shot/lifecycle parity.',
        assumptions=['Original Building7E3EBC and Infantry7EB058 vtables, no overrides; type pointers +520/+6C0, occupants +688/+694, index+69C and rank+150 supplied. Building upgrades+702=0, no open-topped cargo or range bonuses. WeaponType identities and Range+ B4 are supplied fixture inputs.',
                     'All 22 original Foundation width8192B8/height819310 table entries run through45EC90/45ECA0(0) and458E00. Types, field flags, occupant order and weapon links are supplied, not their INI readers/constructors.',
                     'Scan bound starts at6F917D with this inESI, original vtable inEDX and prior bound atESP+38; stops at6F91A7. Both flag-false/empty preservation and occupied overwrite execute. Full prior-bound computation, air pre-pass, cell ring walk, scoring and rejection gates are excluded.',
                     'Every whole-function query uses the shared fresh-call runner and confirms supplied scratch state is unchanged. Ring slice uses the shared checked runner and confirms ESP and scratch state. No RNG, timer or detach call lies in the executed query bodies/slice.'],
        substitutions=[],
        entry_points=dict(building_get_weapon=GET_WEAPON, is_occupied=IS_OCCUPIED,
                          half_foundation=HALF_FOUNDATION, get_current_weapon=GET_CURRENT_WEAPON,
                          get_weapon_range=GET_WEAPON_RANGE, foundation_width=FOUNDATION_WIDTH,
                          foundation_height=FOUNDATION_HEIGHT, ring_bound=RING, ring_stop=RING_END)),
        commands=['python -m tools.garrison_oracle.weapon_range --write',
                  'python -m tools.garrison_oracle.weapon_range --check'])


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata,
                   source_paths={'garrison_weapon_range': Path(__file__),
                                 'native_oracle': Path(__file__).parents[1] / 'native_oracle.py',
                                 'map_queries': Path(__file__).parents[1] / 'spatial_oracle' / 'map_queries.py'})
