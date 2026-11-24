"""Original garrison allegiance decision, with explicit downstream call boundaries.

Run from the repository root:
    python -m tools.garrison_oracle.allegiance --check

The whole original CheckAutoSellOrCivilian458200 executes, including IsRedHP,
SideClass::Find_Index and its CRT string compare, GetOccupantCount, and
IsHumanPlayer. SellBuilding, animation refresh, ChangeOwner and notification
services are recorded supplied boundaries; their implementations are NOT proved
by this corpus. `owner_after_callback` and occupant/fire-index final fields name
the supplied boundary effects explicitly. `calls` is the native decision trace.

`caller_rows` independently executes four original Building::Update gate slices
(warp, alive, health, CanBeOccupied). It does not execute the intervening AI,
combat, delayed-fire or power/repair bodies, or establish whole-frame ordering.

`refresh_rows` executes the whole original458330 slot refresh, its live health
ratio and occupant-count queries, with CreateAnimForSlot451890 recorded at entry.
Explicit callback mutations are dependency-boundary controls, not claims about
what the animation constructor does. `calls` preserves slot/name/argument order.
"""

from copy import deepcopy
from itertools import product
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX,
                               UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
                               UC_X86_REG_FPCW)

import tools.native_oracle as native
from tools.native_oracle import (OracleError, NATIVE_FPCW, RET_MAGIC, SCRATCH,
                                 SCRATCH_SIZE, STACK_BASE, STACK_SIZE,
                                 finish_vectors, load_image, provenance,
                                 run_checked)
from tools.spatial_oracle.map_queries import dwords

ENTRY, COUNT, RED, SIDE, HUMAN = 0x458200, 0x4581F0, 0x5F5CD0, 0x6A46D0, 0x50B6F0
SELL, REFRESH, CHANGE = 0x457DE0, 0x458330, 0x448260
CREATE_SLOT, HEALTH_RATIO = 0x451890, 0x5F5C60
SOUND, RADAR, EVA = 0x750920, 0x65FA70, 0x752700
BUILDING_VTABLE = 0x7E3EBC
BUILDING, TYPE, RULES = (SCRATCH + offset for offset in (0, 0x1000, 0x3000))
HOUSES, HOUSE_TYPES, HOUSE_VECTOR = (SCRATCH + offset for offset in (0x5000, 0x6800, 0x8000))
SIDES, SIDE_VECTOR = (SCRATCH + offset for offset in (0x8100, 0x8900))
OCCUPANTS, OCCUPANT_VECTOR = (SCRATCH + offset for offset in (0x9000, 0xB800))
SP = STACK_BASE + STACK_SIZE - 0x1000
LOCATION = (2688, 2944, 0)

# Original complete bodies and exact original caller slices admitted to execute.
# Any other instruction address raises. Boundaries are handled before this test.
RANGES = ((ENTRY, 0x45832D), (COUNT, 0x4581F7), (RED, 0x5F5D1A),
          (REFRESH, 0x4585C0), (HEALTH_RATIO, 0x5F5C80),
          (0x459EE0, 0x459EE7), (SIDE, 0x6A470D), (0x7C8D20, 0x7C8DF0),
          (HUMAN, 0x50B724), (0x41BEA0, 0x41BEDD),
          (0x70C5B0, 0x70C5B7), (0x70C5C0, 0x70C5C7),
          (0x43FD08, 0x43FD2C), (0x43FE5B, 0x43FE69),
          (0x440042, 0x440076), (0x44019D, 0x4401B4))


def house(index):
    return 0 if index is None else HOUSES + index * 0x300


def occupant(index):
    return OCCUPANTS + index * 0x800


HOUSE_DEFAULTS = dict(name='', side_index=0, is_human=False, player_control=False)
DEFAULTS = dict(
    tech_level=-1, current_hp=100, strength=100,
    condition_red_bits='3fd0000000000000',
    side_names=['Allied', 'Soviet', 'Civilian', 'Special'],
    houses=[dict(name='Player', side_index=0, is_human=True),
            dict(name='Enemy', side_index=1), dict(name='Neutral', side_index=2)],
    current_owner=0, occupant_owners=[], game_mode=1, local_house=0,
    eject_retained_occupants=[], radar_event_success=True, fire_index=7)


def resolve(sparse):
    if set(sparse) - set(DEFAULTS) - {'name'}:
        raise OracleError(f"unknown inputs in {sparse['name']}")
    row = dict(deepcopy(DEFAULTS), **deepcopy(sparse))
    row['houses'] = [dict(HOUSE_DEFAULTS, **value) for value in row['houses']]
    if len(row['houses']) > 8 or len(row['side_names']) > 8 or len(row['occupant_owners']) > 5:
        raise OracleError('fixture exceeds declared storage')
    for index in [row['current_owner'], row['local_house'], *row['occupant_owners']]:
        if index is not None and not 0 <= index < len(row['houses']):
            raise OracleError('unknown fixture house identity')
    if row['game_mode'] == 0 and row['current_owner'] is None:
        raise OracleError('campaign IsHumanPlayer would dereference a null House')
    if any(not 0 <= index < len(row['occupant_owners'])
           for index in row['eject_retained_occupants']):
        raise OracleError('ejection retains an absent occupant')
    if len(set(row['eject_retained_occupants'])) != len(row['eject_retained_occupants']):
        raise OracleError('ejection duplicated an occupant')
    return row


class Fixture:
    def __init__(self, row):
        self.row = row
        self.u = u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(SCRATCH, SCRATCH_SIZE)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(RET_MAGIC, 0x1000)
        self.original = [bytes(u.mem_read(a, b - a)) for a, b in RANGES]
        self.calls, self.count_reads, self.side_index = [], [], None
        self.ratio_reads = 0
        self.refresh_spec = None
        self.stops = {RET_MAGIC}
        self.callback_mode = True
        u.mem_write(BUILDING, dwords(BUILDING_VTABLE))
        u.mem_write(BUILDING + 0x520, dwords(TYPE))
        u.mem_write(BUILDING + 0x6C, dwords(row['current_hp']))
        u.mem_write(BUILDING + 0x21C, dwords(house(row['current_owner'])))
        u.mem_write(BUILDING + 0x688, dwords(OCCUPANT_VECTOR))
        u.mem_write(BUILDING + 0x694, dwords(len(row['occupant_owners'])))
        u.mem_write(BUILDING + 0x69C, dwords(row['fire_index']))
        u.mem_write(BUILDING + 0x9C, dwords(*LOCATION))
        u.mem_write(TYPE + 0xA0, dwords(row['strength']))
        u.mem_write(TYPE + 0x634, dwords(row['tech_level']))
        u.mem_write(0x8871E0, dwords(RULES))
        u.mem_write(RULES + 0x1708, struct.pack('<Q', int(row['condition_red_bits'], 16)))
        u.mem_write(RULES + 0x1C0, dwords(42))
        u.mem_write(0xA8B238, dwords(row['game_mode']))
        u.mem_write(0xA83D4C, dwords(house(row['local_house'])))
        for index, spec in enumerate(row['houses']):
            kind = HOUSE_TYPES + index * 0x300
            u.mem_write(house(index) + 0x34, dwords(kind))
            u.mem_write(kind + 0xBC, dwords(spec['side_index']))
            u.mem_write(kind + 0x24, spec['name'].encode('ascii') + b'\0')
            u.mem_write(house(index) + 0x1EC, bytes([spec['is_human'], spec['player_control']]))
            u.mem_write(HOUSE_VECTOR + 4 * index, dwords(house(index)))
        u.mem_write(0xA8022C, dwords(HOUSE_VECTOR))
        u.mem_write(0xA80238, dwords(len(row['houses'])))
        for index, name in enumerate(row['side_names']):
            side = SIDES + index * 0x100
            u.mem_write(side + 0x24, name.encode('ascii') + b'\0')
            u.mem_write(SIDE_VECTOR + 4 * index, dwords(side))
        u.mem_write(0x8B4124, dwords(SIDE_VECTOR))
        u.mem_write(0x8B4130, dwords(len(row['side_names'])))
        # Original CRT ASCII/C locale branch, rather than the Windows locale API.
        u.mem_write(0xB782A0, dwords(0))
        for index, owner in enumerate(row['occupant_owners']):
            u.mem_write(occupant(index) + 0x21C, dwords(house(owner)))
            u.mem_write(OCCUPANT_VECTOR + index * 4, dwords(occupant(index)))
        u.hook_add(UC_HOOK_CODE, self.observe)

    def word(self, address):
        return struct.unpack('<I', self.u.mem_read(address, 4))[0]

    def house_index(self, address):
        if address == 0:
            return None
        for index in range(len(self.row['houses'])):
            if address == house(index):
                return index
        raise OracleError(f'unknown house pointer 0x{address:08X}')

    def current_owner(self):
        return self.house_index(self.word(BUILDING + 0x21C))

    def occupant_owners(self):
        return [self.house_index(self.word(self.word(OCCUPANT_VECTOR + index * 4) + 0x21C))
                for index in range(self.word(BUILDING + 0x694))]

    def returned_boundary(self, value=0, cleaned=0):
        u = self.u
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleaned)
        u.reg_write(UC_X86_REG_EIP, self.word(sp))

    def observe(self, u, pc, _size, _data):
        if pc in self.stops:
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        if self.refresh_spec is not None and pc == CREATE_SLOT:
            if u.reg_read(UC_X86_REG_ECX) != BUILDING:
                raise OracleError('unexpected CreateAnimForSlot receiver')
            name_pointer = self.word(sp + 4)
            slot, damaged, garrisoned, delay = [self.word(sp + offset) for offset in (8, 12, 16, 20)]
            if slot not in REFRESH_SLOTS:
                raise OracleError('unexpected refresh slot')
            allowed_names = {TYPE + 0xF4C + slot * 0x44 + delta for delta in (0, 16, 32)}
            if name_pointer not in allowed_names:
                raise OracleError('refresh selected an undeclared type name')
            selected_name = bytes(u.mem_read(name_pointer, 16)).split(b'\0', 1)[0].decode('ascii')
            self.calls.append(dict(slot=slot, name=selected_name, damaged=bool(damaged & 0xFF),
                                   garrisoned=bool(garrisoned & 0xFF), delay=delay))
            for mutation in self.refresh_spec['callback_mutations']:
                if mutation['after_slot'] == slot:
                    for removed in mutation.get('remove_slots', []):
                        u.mem_write(BUILDING + 0x55C + 4 * removed, dwords(0))
                    for added in mutation.get('add_slots', []):
                        u.mem_write(BUILDING + 0x55C + 4 * added, dwords(SCRATCH + 0xE000 + added * 16))
                    if 'current_hp' in mutation:
                        u.mem_write(BUILDING + 0x6C, dwords(mutation['current_hp']))
                    if 'occupant_count' in mutation:
                        u.mem_write(BUILDING + 0x694, dwords(mutation['occupant_count']))
            self.returned_boundary(cleaned=20)
            return
        boundary = pc in (SELL, CHANGE, SOUND, RADAR, EVA) or (pc == REFRESH and self.refresh_spec is None)
        if self.callback_mode and boundary:
            if pc in (SELL, REFRESH, CHANGE) and u.reg_read(UC_X86_REG_ECX) != BUILDING:
                raise OracleError('unexpected downstream Building receiver')
            if pc == SELL:
                self.calls.append(dict(op='sell_occupants', hunt=self.word(sp + 4),
                                       inside_fallback=self.word(sp + 8)))
                # Supplied dependency effect, not execution of SellBuilding.
                for index, old_index in enumerate(self.row['eject_retained_occupants']):
                    u.mem_write(OCCUPANT_VECTOR + index * 4, dwords(occupant(old_index)))
                u.mem_write(BUILDING + 0x694, dwords(len(self.row['eject_retained_occupants'])))
                u.mem_write(BUILDING + 0x69C, dwords(0))
                self.returned_boundary(cleaned=8)
            elif pc == REFRESH:
                self.calls.append(dict(op='refresh_anims', owner=self.current_owner(),
                                       occupants=self.occupant_owners()))
                self.returned_boundary()
            elif pc == CHANGE:
                new_owner = self.house_index(self.word(sp + 4))
                self.calls.append(dict(op='change_owner', owner=new_owner,
                                       announce=self.word(sp + 8)))
                u.mem_write(BUILDING + 0x21C, dwords(house(new_owner)))
                self.returned_boundary(1, 8)
            elif pc == SOUND:
                self.calls.append(dict(op='abandoned_sound', sound=u.reg_read(UC_X86_REG_ECX),
                                       pan=u.reg_read(UC_X86_REG_EDX),
                                       volume_bits=f'{self.word(sp + 4):08x}', handle=self.word(sp + 8)))
                self.returned_boundary(cleaned=8)
            elif pc == RADAR:
                cell = struct.unpack('<hh', u.mem_read(sp + 4, 4))
                self.calls.append(dict(op='radar_event', event=u.reg_read(UC_X86_REG_ECX),
                                       cell=list(cell), accepted=self.row['radar_event_success']))
                self.returned_boundary(int(self.row['radar_event_success']), 4)
            else:
                if u.reg_read(UC_X86_REG_ECX) != 0x81926C:
                    raise OracleError('unexpected EVA identity')
                self.calls.append(dict(op='abandoned_eva'))
                self.returned_boundary(cleaned=4)
            return
        if not any(begin <= pc < end for begin, end in RANGES):
            raise OracleError(f'undeclared native instruction 0x{pc:08X}')
        if pc == COUNT:
            self.count_reads.append(self.word(BUILDING + 0x694))
        elif pc == HEALTH_RATIO:
            self.ratio_reads += 1
        elif pc == 0x458242:
            self.side_index = struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0]

    def run(self, begin, stops, required):
        self.stops = set(stops)
        u = self.u
        u.mem_write(SP, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, BUILDING)
        u.reg_write(UC_X86_REG_ESI, BUILDING)
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        result = run_checked(u, begin, tuple(stops), count=10000,
                             required_addresses=required, context=dict(case=self.row['name']))
        if self.original != [bytes(u.mem_read(a, b - a)) for a, b in RANGES]:
            raise OracleError('original instruction bytes changed')
        return result


def execute(sparse):
    row = resolve(sparse)
    fixture = Fixture(row)
    fixture.run(ENTRY, (RET_MAGIC,), (ENTRY,))
    if fixture.u.reg_read(UC_X86_REG_ESP) != SP + 4:
        raise OracleError('whole decision body did not balance its stack')
    return dict(input=row, output=dict(
        owner_after_callback=fixture.current_owner(),
        occupant_owners_after_callback=fixture.occupant_owners(),
        fire_index_after_callback=fixture.word(BUILDING + 0x69C),
        calls=fixture.calls, occupant_count_reads=fixture.count_reads,
        civilian_side_index=fixture.side_index))


def inputs():
    rows = [dict(name='empty_player_abandons'),
            dict(name='empty_civilian_stays', current_owner=2),
            dict(name='occupied_civilian_takes_first_owner', current_owner=2, occupant_owners=[1, 0]),
            dict(name='occupied_civilian_first_same_house', current_owner=2, occupant_owners=[2, 0]),
            dict(name='occupied_player_stays', occupant_owners=[0, 1]),
            dict(name='empty_nonlocal_human_is_silent', current_owner=0, local_house=1),
            dict(name='radar_refusal_suppresses_eva', radar_event_success=False),
            dict(name='campaign_human_abandons', game_mode=0, local_house=1),
            dict(name='campaign_player_control_abandons', game_mode=0,
                 houses=[dict(name='Controlled', side_index=0, player_control=True),
                         dict(name='Enemy', side_index=1), dict(name='Neutral', side_index=2)]),
            dict(name='campaign_ai_is_silent', game_mode=0, current_owner=1),
            dict(name='renamed_civilian_is_owner', current_owner=2, occupant_owners=[0],
                 houses=[dict(name='Neutral', side_index=0), dict(name='Special', side_index=1),
                         dict(name='MapPeople', side_index=2)]),
            dict(name='neutral_name_is_not_civilian_side', current_owner=0,
                 houses=[dict(name='Neutral', side_index=0), dict(name='Other', side_index=1),
                         dict(name='CivilianBySide', side_index=2)]),
            dict(name='first_civilian_house_wins', current_owner=0,
                 houses=[dict(name='Player', side_index=0), dict(name='FirstCivilian', side_index=2),
                         dict(name='Neutral', side_index=2)]),
            dict(name='other_civilian_house_is_not_selected', current_owner=2, occupant_owners=[0],
                 houses=[dict(name='Player', side_index=0), dict(name='FirstCivilian', side_index=2),
                         dict(name='Neutral', side_index=2)]),
            dict(name='case_insensitive_civilian_side', side_names=['Allied', 'Soviet', 'cIvIlIaN']),
            dict(name='civilian_side_first', side_names=['Civilian', 'Soviet', 'Other'], current_owner=1),
            dict(name='missing_civilian_side_requests_null', side_names=['Allied', 'Soviet', 'Other']),
            dict(name='missing_civilian_house_requests_null',
                 houses=[dict(name='Player', side_index=0), dict(name='Enemy', side_index=1)]),
            dict(name='missing_side_matches_negative_one_house', side_names=['Allied', 'Soviet'],
                 houses=[dict(name='Player', side_index=0), dict(name='NoSide', side_index=-1)]),
            dict(name='empty_house_array', houses=[], current_owner=None, local_house=None),
            dict(name='red_ejection_rereads_empty', current_hp=25, occupant_owners=[0, 1]),
            dict(name='red_civilian_ejection_stays_civilian', current_hp=25, current_owner=2,
                 occupant_owners=[0]),
            dict(name='red_boundary_keeps_second_occupant', current_hp=25, current_owner=2,
                 occupant_owners=[1, 0], eject_retained_occupants=[1]),
            dict(name='red_empty_still_calls_eject', current_hp=25),
            dict(name='above_red_does_not_eject', current_hp=26, occupant_owners=[0]),
            dict(name='zero_hp_is_not_red', current_hp=0, occupant_owners=[0]),
            dict(name='negative_hp_is_not_red', current_hp=-1, occupant_owners=[0]),
            dict(name='zero_strength_positive_hp_is_not_red', current_hp=1, strength=0,
                 occupant_owners=[0]),
            dict(name='negative_strength_positive_hp_is_red', current_hp=1, strength=-100,
                 occupant_owners=[0]),
            dict(name='unordered_red_comparison_positive_hp', current_hp=1,
                 condition_red_bits='7ff8000000000001', occupant_owners=[0])]
    rows += [dict(name=f'tech_level_{level}_empty', tech_level=level) for level in (-2, 0, 1, 10, -2147483648, 2147483647)]
    rows += [dict(name=f'tech_level_{level}_red_occupied', tech_level=level, current_hp=25,
                  current_owner=2, occupant_owners=[0]) for level in (-2, 0, 1, 10)]
    return rows


def caller_gates(spec):
    fixture = Fixture(resolve(dict(name=spec['name'])))
    fixture.callback_mode = False
    u = fixture.u
    u.mem_write(BUILDING + 0x270, bytes([spec['warped_out'], spec['warping_in']]))
    u.mem_write(BUILDING + 0x90, bytes([spec['alive']]))
    u.mem_write(BUILDING + 0x6C, dwords(spec['health']))
    u.mem_write(BUILDING + 0x544, dwords(spec['health']))
    u.mem_write(TYPE + 0x157B, bytes([spec['can_be_occupied']]))
    # Each result is an actual endpoint of its original slice. The intervening
    # body segments are not emulated or replaced by a guessed callable.
    warp = fixture.run(0x43FD08, (0x43FD2C, 0x4403D4), (0x43FD08,)) == 0x43FD2C
    alive = fixture.run(0x43FE5B, (0x43FE69, 0x440573), (0x43FE5B,)) == 0x43FE69
    health = fixture.run(0x440042, (0x440076, 0x4400F2), (0x440042,)) == 0x4400F2
    occupiable = fixture.run(0x44019D, (ENTRY, 0x4401B4), (0x44019D,)) == ENTRY
    if fixture.calls:
        raise OracleError('caller gate executed a downstream callback')
    return dict(input=spec, output=dict(warp_allows=warp, alive_allows=alive,
                                      health_allows=health, occupiable_allows=occupiable,
                                      all_gates_allow=warp and alive and health and occupiable))


REFRESH_SLOTS = (18, 3, 4, 5, 6)
REFRESH_DEFAULTS = dict(current_hp=100, strength=100,
                        condition_yellow_bits='3fe0000000000000',
                        retained_damage_state=False, occupant_count=1,
                        live_slots=list(REFRESH_SLOTS), callback_mutations=[],
                        slot_names={str(slot):dict(normal=f'N{slot}', damaged=f'D{slot}',
                                                   garrisoned=f'G{slot}') for slot in REFRESH_SLOTS})


def refresh_execute(sparse):
    if set(sparse) - set(REFRESH_DEFAULTS) - {'name'}:
        raise OracleError('unknown refresh input')
    spec = dict(deepcopy(REFRESH_DEFAULTS), **deepcopy(sparse))
    if 'slot_names' in sparse:
        spec['slot_names'] = deepcopy(REFRESH_DEFAULTS['slot_names'])
        for slot, names in sparse['slot_names'].items():
            if slot not in spec['slot_names'] or set(names) - {'normal', 'damaged', 'garrisoned'}:
                raise OracleError('unknown slot/name variant')
            spec['slot_names'][slot].update(names)
    all_slots = list(spec['live_slots'])
    for mutation in spec['callback_mutations']:
        if set(mutation) - {'after_slot', 'remove_slots', 'add_slots', 'current_hp', 'occupant_count'}:
            raise OracleError('unknown refresh callback mutation')
        if mutation['after_slot'] not in REFRESH_SLOTS:
            raise OracleError('mutation at a non-refreshed slot')
        all_slots += mutation.get('remove_slots', []) + mutation.get('add_slots', [])
    if any(not 0 <= slot < 21 for slot in all_slots):
        raise OracleError('invalid Building animation slot')
    fixture = Fixture(resolve(dict(name=spec['name'], current_hp=spec['current_hp'],
                                   strength=spec['strength'])))
    fixture.refresh_spec = spec
    u = fixture.u
    u.mem_write(RULES + 0x1700, struct.pack('<Q', int(spec['condition_yellow_bits'], 16)))
    u.mem_write(BUILDING + 0x694, dwords(spec['occupant_count']))
    u.mem_write(BUILDING + 0x6E6, bytes([spec['retained_damage_state']]))
    for slot in spec['live_slots']:
        u.mem_write(BUILDING + 0x55C + 4 * slot, dwords(SCRATCH + 0xE000 + slot * 16))
    for slot, names in spec['slot_names'].items():
        for variant, delta in (('normal', 0), ('damaged', 16), ('garrisoned', 32)):
            name = names[variant].encode('ascii')
            if len(name) >= 16:
                raise OracleError('BuildingAnim name exceeds its fixed storage')
            u.mem_write(TYPE + 0xF4C + int(slot) * 0x44 + delta, name + b'\0')
    fixture.run(REFRESH, (RET_MAGIC,), (REFRESH,))
    if u.reg_read(UC_X86_REG_ESP) != SP + 4:
        raise OracleError('whole refresh body did not balance its stack')
    counts = [struct.unpack('<i', dwords(count))[0] for count in fixture.count_reads]
    return dict(input=spec, output=dict(calls=fixture.calls,
        health_ratio_reads=fixture.ratio_reads, occupant_count_reads=counts,
        live_slots_after_callback=[slot for slot in range(21)
                                  if fixture.word(BUILDING + 0x55C + slot * 4)]))


def refresh_inputs():
    rows = [dict(name='all_present_healthy_occupied'),
            dict(name='all_present_healthy_empty', occupant_count=0),
            dict(name='negative_count_is_empty', occupant_count=-1),
            dict(name='max_positive_count_is_occupied', occupant_count=2147483647),
            dict(name='yellow_boundary_damaged_over_garrison', current_hp=50),
            dict(name='above_yellow_is_garrisoned', current_hp=51),
            dict(name='retained_damaged_does_not_override_live_healthy', retained_damage_state=True),
            dict(name='retained_healthy_does_not_override_live_damaged', current_hp=25),
            dict(name='unordered_yellow_chooses_damaged', condition_yellow_bits='7ff8000000000001'),
            dict(name='zero_strength_zero_hp_unordered_is_damaged', current_hp=0, strength=0),
            dict(name='zero_strength_positive_hp_is_healthy', current_hp=1, strength=0),
            dict(name='all_absent', live_slots=[]),
            dict(name='only_other_slots_present', live_slots=[0, 1, 2, 7, 8, 10, 20]),
            dict(name='other_slots_are_not_refreshed', live_slots=list(range(21))),
            dict(name='empty_selected_garrison_name_skips', slot_names={'18':dict(garrisoned=''), '4':dict(garrisoned='')}),
            dict(name='empty_selected_damaged_name_does_not_fallback', current_hp=25,
                 slot_names={'18':dict(damaged=''), '4':dict(damaged='')}),
            dict(name='empty_selected_normal_name_does_not_fallback', occupant_count=0,
                 slot_names={'18':dict(normal=''), '4':dict(normal='')}),
            dict(name='callback_removes_later_slot', callback_mutations=[dict(after_slot=18, remove_slots=[4])]),
            dict(name='callback_adds_later_slot', live_slots=[18, 6],
                 callback_mutations=[dict(after_slot=18, add_slots=[3])]),
            dict(name='callback_adds_earlier_slot_not_revisited', live_slots=[3],
                 callback_mutations=[dict(after_slot=3, add_slots=[18])]),
            dict(name='callback_changes_later_live_queries',
                 callback_mutations=[dict(after_slot=18, current_hp=25, occupant_count=0)])]
    rows += [dict(name=f'only_slot_{slot}', live_slots=[slot]) for slot in REFRESH_SLOTS]
    return rows


def generate():
    gate_inputs = [dict(name=f'warp_{int(warped)}_{int(warping)}_alive_{int(alive)}_occupied_{int(occupied)}',
                        warped_out=warped, warping_in=warping, alive=alive,
                        can_be_occupied=occupied, health=100)
                   for warped, warping, alive, occupied in product((False, True), repeat=4)]
    gate_inputs += [dict(name=f'health_{health}', warped_out=False, warping_in=False,
                         alive=True, can_be_occupied=True, health=health) for health in (0, -1, 1)]
    return dict(schema_version=1, rows=[execute(row) for row in inputs()],
                caller_rows=[caller_gates(row) for row in gate_inputs],
                refresh_rows=[refresh_execute(row) for row in refresh_inputs()])


def metadata():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    return dict(provenance(
        scope='Whole original CheckAutoSellOrCivilian458200 decision and original458330 refresh selection with recorded supplied downstream calls; independent original Building::Update warp/alive/health/CanBeOccupied gate slices. No ejection, animation construction/lifecycle, transfer, notification or whole-frame implementation parity.',
        assumptions=[
            'House and Side array order, names, HouseType Side+BC, TechLevel+634, health/strength/Red threshold and occupant vector order are supplied runtime inputs. No constructors or INI readers execute. Retail reachability needs separate production validation.',
            'Original Building7E3EBC vtable, IsRedHP5F5CD0, GetType459EE0, GetOccupantCount4581F0, SideClass FindIndex6A46D0, CRT strcmpi7C8D20, IsHumanPlayer50B6F0 and GetMapCoords41BEA0 execute. C-locale globalB782A0=0 is supplied; ASCII side names only. Ambient x87PC53/chop FPCW0E7F, masked exceptions.',
            'Original458200 uses first matching House side even if Side lookup returns-1; no matching House yields NULL. NULL ChangeOwner calls are recorded decisions only: the supplied scalar callback accepts NULL, while native Building::ChangeOwner may dereference it. These invalid-data controls do not establish successful gameplay ownership.',
            'Whole decision runs balance the stack. Every executed PC belongs to declared original ranges or a recorded callback boundary, and original bytes are compared before/after every checked run. No source, vtable or native instruction is patched.',
            'Caller slices execute43FD08..43FD2C/4403D4,43FE5B..43FE69/440573,440042..440076/4400F2,44019D..458200/4401B4 separately. Intermediate Building AI/mission/fire/power work and mutations are outside this corpus; all_gates_allow is only the conjunction of the four executed gate results.',
            'refresh_rows executes whole458330, live GetHealthRatio5F5C60 and GetOccupantCount4581F0 for each currently present slot18,3,4,5,6. Supplied live slot pointers are only null/non-null inputs; the original refresh does not dereference Anim objects. ConditionYellow, names and retained6E6 are supplied. Selection executes damaged-before-garrison-before-normal, signed count>0, empty-name skip and live per-slot re-reads.',
            'No RNG, timer write or detach call occurs in the executed decision/query bodies. Downstream SellBuilding can release/UnInit occupants; refresh458330 reconstructs animation slots; ChangeOwner includes mission timer, sight/detach, house tracking and dependent effects. Those bodies are supplied boundaries and their draws/timers/detach are not certified here.'],
        substitutions=[
            'SellBuilding457DE0 records both native args, supplies the explicit eject_retained_occupants vector and clears firing index+69C. It does not execute placement, ejection or death; retained controls demonstrate that458200 re-reads the count and index0 after the call.',
            'Refresh458330 records the current owner/occupant vector and returns without anim work. ChangeOwner448260 records requested House and announce argument then supplies only owner+21C. Final *_after_callback fields describe this callback contract, not execution of the dependency.',
            'Only in refresh_rows,458330 executes and CreateAnimForSlot451890 is the recorded boundary. It records native slot/name/damaged/garrisoned/delay without allocating. callback_mutations explicitly supply later slot removal/addition or health/count changes to expose live re-reads; these controls are not claims the native constructor performs those mutations.',
            'PlayGlobal750920 records sound42, pan8192, volume and handle. Radar65FA70 records event15, original GetMapCoords cell and supplies radar_event_success. EVA752700 records the original EVA_StructureAbandoned literal; audio/UI bodies do not execute.'],
        entry_points=dict(allegiance=ENTRY, red=RED, civilian_side=SIDE, human=HUMAN,
                          occupant_count=COUNT, refresh_selection=REFRESH, health_ratio=HEALTH_RATIO,
                          warp_gate=0x43FD08, alive_gate=0x43FE5B,
                          health_gate=0x440042, occupiable_gate=0x44019D)),
        commands=['python -m tools.garrison_oracle.allegiance --write',
                  'python -m tools.garrison_oracle.allegiance --check'],
        original_code_sha256={f'{a:08X}..{b:08X}':hashlib.sha256(bytes(u.mem_read(a, b-a))).hexdigest()
                              for a, b in RANGES},
        supplied_building_location=list(LOCATION), supplied_abandoned_sound=42)


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata,
                   source_paths={'garrison_allegiance': Path(__file__),
                                 'native_oracle': Path(native.__file__),
                                 'map_queries': Path(native.__file__).parent / 'spatial_oracle' / 'map_queries.py'})
