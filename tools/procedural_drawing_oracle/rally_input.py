"""Original Building443860 click preparation, FNPC and event1E enqueue.

The map/zone fixture has one owner in walk_move_admission.prepare_fixture.
No gameplay call is replaced: only the OS wall-clock transport timestamp is
supplied. Objects, zones, bridge records and raw Cell fields are fixture inputs,
not a native Scenario/retail loader or event-dispatch comparison.
"""

from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP,
)

from tools.native_oracle import (
    RET_MAGIC, STACK_BASE, STACK_SIZE, finish_vectors, provenance, run_checked,
)
from tools.spatial_oracle import walk_move_admission as fixture
from tools.spatial_oracle.map_queries import dwords, packed

ARENA = 0x31000000
BUILDING, TYPE, HOUSE = ARENA, ARENA + 0x1000, ARENA + 0x4000
FRAME = 0xA8ED84
COUNTERS = {
    0x56DC20: 'fnpc', 0x56D230: 'zone', 0x578540: 'playfield',
    0x56E7C0: 'rectangle', 0x4834A0: 'passability', 0x6D6410: 'projection',
    0x6E6AB0: 'net_id_pack', 0x4C6780: 'event_constructor',
    0x70C610: 'archive_assignment', 0x65C780: 'random',
    0x65C7E0: 'random_range',
}


def cell(u, pointer):
    return list(struct.unpack('<2h', u.mem_read(pointer, 4)))


def query(row):
    # Building's literal movement-zone choice selects the fixture's raw-zone
    # slot; every reached GetZoneID/FNPC branch still executes original bytes.
    movement_zone = 4 if row.get('naval') else 9 if row.get('factory', 40) == 3 else 0
    setup = dict(row, movement_zone=movement_zone)
    u, r32, _ret, _call = fixture.prepare_fixture(setup)
    if 'prior_stack_cell' in row:
        # Explicit boundary control: native negative pool indices can consume
        # earlier stack contents. This is not a production caller substitution.
        u.mem_write(STACK_BASE, packed(*row['prior_stack_cell']) * (STACK_SIZE // 4))
    u.mem_map(ARENA, 0x20000)
    # Whole Building ctor43BAFA..43BB15 installs these four native tables;
    # NetID6E6AB0 calls the secondary table's +10 GetID slot on Building+4.
    u.mem_write(BUILDING, dwords(0x7E3EBC, 0x7E3EA0, 0x7E3E98, 0x7E3E90))
    u.mem_write(BUILDING + 0x10, dwords(101, 1))
    u.mem_write(BUILDING + 0x90, b'\1')
    u.mem_write(BUILDING + 0x9C, dwords(*row.get('source', [1344, 1344, 0])))
    u.mem_write(BUILDING + 0x218, dwords(fixture.CELLS + (5 * 16 + 6) * 0x200, HOUSE))
    u.mem_write(BUILDING + 0x520, dwords(TYPE))
    u.mem_write(TYPE + 0xEB8, dwords(row.get('factory', 40)))
    u.mem_write(TYPE + 0xCCE, bytes([row.get('naval', False)]))
    u.mem_write(HOUSE + 0x30, dwords(0))
    u.mem_write(HOUSE + 0x1EC, b'\1')
    u.mem_write(HOUSE + 0x1F8, b'\1')
    u.mem_write(0xA83D4C, dwords(HOUSE))
    u.mem_write(fixture.CLICK, packed(*row.get('clicked', [11, 5])))
    # The Foot corpus never packs Dummy; the whole Building caller can. Give
    # that existing shared Cell its original table, without changing its data.
    u.mem_write(fixture.DUMMY, dwords(0x7E4EEC))
    if row.get('zero_speed_table'):
        u.mem_write(0x89EA40, bytes(90 * 4))
    queue_count = row.get('queue_count', 0)
    u.mem_write(0xA802C8, dwords(queue_count, 0, row.get('queue_tail', 0)))
    calls = {name: 0 for name in COUNTERS.values()}
    events, zone, nearby, timestamps, final_selection = [], [], [], [], []
    code_ranges = ((0x443860, 0x326), (0x56D230, 0x26C),
                   (0x56DC20, 0xABC), (0x4834A0, 0x281), (0x4C6780, 0xA0))
    code_before = [bytes(u.mem_read(start, size)) for start, size in code_ranges]

    def observe(_u, pc, size, _data):
        if pc in COUNTERS:
            calls[COUNTERS[pc]] += 1
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == 0x56D230 and r32(sp) == 0x44391C:
            zone.append(dict(source_cell=cell(u, r32(sp + 4)),
                             movement_zone=r32(sp + 8),
                             bridge_aware=r32(sp + 12) & 0xFF))
        elif pc == 0x44391C:
            zone[-1]['result'] = u.reg_read(UC_X86_REG_EAX)
        elif pc == 0x56DC20:
            args = [r32(sp + 4 + i * 4) for i in range(15)]
            nearby.append(dict(clicked=cell(u, args[1]), speed_type=args[2],
                required_zone=args[3], movement_zone=args[4], bridge_aware=args[5] & 0xFF,
                width=args[6], height=args[7], reject_overlay=args[8] & 0xFF,
                check_height=args[9] & 0xFF, safety=args[10] & 0xFF,
                allow_bridge=args[11] & 0xFF, target_cell=cell(u, args[12]),
                ring_variant=args[13] & 0xFF, check_occupancy=args[14] & 0xFF))
        elif pc == 0x56E6A8:
            final_selection.append(dict(accepted_count=r32(sp + 0x18),
                valid_count=r32(sp + 0x24), fallback_count=r32(sp + 0x28)))
        elif pc in (0x56E6BF, 0x56E6DE):
            # Observe the signed IDIV result and the native pending load. Four
            # callee-saved registers have been popped since56E6A8.
            selection = final_selection[-1]
            index = struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EDX)))[0]
            valid = pc == 0x56E6BF
            offset = 0x11C if valid else 0x17C
            count = selection['valid_count' if valid else 'fallback_count']
            selection.update(pc=f'0x{pc:08X}', pool='valid' if valid else 'fallback',
                signed_index=index, read_cell=cell(u, sp + offset + index * 4),
                within_selected_pool=0 <= index < count)
            if index < 0:
                previous_count = selection['accepted_count' if valid else 'valid_count']
                selection.update(previous_pool='raw_accepted' if valid else 'valid',
                    previous_pool_slot=24 + index,
                    previous_slot_initialized=0 <= 24 + index < previous_count)
        elif pc == 0x44395C:
            nearby[-1]['result'] = cell(u, u.reg_read(UC_X86_REG_EAX))
            nearby[-1]['dummy_at_return'] = cell(u, fixture.DUMMY + 0x24)
        elif pc == 0x4C6780:
            args = [r32(sp + 4 + i * 4) for i in range(6)]
            events.append(dict(house=args[0], opcode=args[1] & 0xFF,
                               source_id=args[2], source_tag=args[3] & 0xFF,
                               target_id=args[4], target_tag=args[5] & 0xFF))
        elif pc in (0x4439FD, 0x443B4B):
            timestamps.append(dict(pc=f'0x{pc:08X}', wall_ms=0))
            u.reg_write(UC_X86_REG_EAX, 0)
            u.reg_write(UC_X86_REG_EIP, pc + size)

    u.hook_add(UC_HOOK_CODE, observe)
    before = bytes(u.mem_read(ARENA, 0x20000))
    clicked_before = bytes(u.mem_read(fixture.CLICK, 4))
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(sp, dwords(RET_MAGIC, fixture.CLICK, 0))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, BUILDING)
    run_checked(u, 0x443860, RET_MAGIC, count=3_000_000,
                required_addresses=[0x443860, 0x56D230, 0x56DC20], context={'case': row})
    assert u.reg_read(UC_X86_REG_ESP) == sp + 12
    assert bytes(u.mem_read(ARENA, 0x20000)) == before, 'input changed actor/type/House'
    assert bytes(u.mem_read(fixture.CLICK, 4)) == clicked_before, 'input changed clicked Cell'
    assert code_before == [bytes(u.mem_read(start, size)) for start, size in code_ranges]
    assert calls['random'] == calls['random_range'] == calls['archive_assignment'] == 0
    count_after, tail_after = r32(0xA802C8), r32(0xA802D0)
    count_added = count_after - queue_count
    assert count_added in (0, 1)
    queued = None
    if count_added:
        pointer = 0xA802D4 + row.get('queue_tail', 0) * 0x6F
        queued = bytes(u.mem_read(pointer, 0x11)).hex()
    return dict(input=row, binary_frame=r32(FRAME), source_zone=zone,
                nearby=nearby, constructed_events=events, calls=calls,
                queue=dict(added=count_added, count=count_after, tail=tail_after,
                           event_header_and_targets_hex=queued),
                timestamps=timestamps, final_selection=final_selection,
                dummy=cell(u, fixture.DUMMY + 0x24),
                archive_unchanged=True, inputs_unchanged=True, code_unchanged=True)


def inputs():
    yield dict(name='ordinary_clear')
    for frame in (0, 1, 2, 5, 100, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF):
        yield dict(name=f'split_frame_{frame}', split=True, frame=frame)
    for raw in (0x1F, 0x20, 0x100, 0xFFFFFFFF):
        yield dict(name=f'clicked_raw_{raw:08x}', cells=[dict(xy=[11, 5], raw=raw)])
    yield dict(name='no_passable_speed', zero_speed_table=True)
    yield dict(name='outside_click', clicked=[16, 5])
    yield dict(name='negative_source_truncation', source=[-1, 1344, 0])
    yield dict(name='physical_source_not_foundation_center', source=[2047, 1344, 0], split=True)
    bridge = dict(split=True, clicked=[9, 5], cells=[dict(xy=[9, 5], flags=256)])
    yield dict(bridge, name='clicked_bridge_record', records=[[[7, 5], [11, 5], 1, 0]])
    yield dict(bridge, name='clicked_bridge_missing_record')
    yield dict(bridge, name='source_bridge_clicked_bridge', source=[2368, 1344, 416],
               records=[[[7, 5], [11, 5], 1, 0]])
    yield dict(bridge, name='source_bridge_clicked_ground', source=[2368, 1344, 416],
               clicked=[11, 5], records=[[[7, 5], [11, 5], 1, 0]])
    yield dict(name='infantry_factory', factory=16)
    yield dict(name='aircraft_factory_argument_control', factory=3)
    yield dict(name='naval_factory_argument_control', naval=True)
    yield dict(name='naval_overrides_aircraft_argument_control', factory=3, naval=True)
    yield dict(name='full_event_queue', queue_count=128)
    yield dict(name='event_queue_wrap', queue_tail=127)


def native_boundary_inputs():
    # Deliberately outside the general native parity domain. These witnesses
    # establish initialized-array versus prior-stack reads, not intended rules.
    yield dict(name='negative_frame_poisoned_stack', split=True, frame=0xFFFFFFFF,
               prior_stack_cell=[6, 6])
    yield dict(name='negative_frame_24_initialized_slots', clicked=[8, 8],
               frame=0xFFFFFFFF, cells=[dict(xy=[x, y], raw=31)
                   for x in range(6, 11) for y in range(6, 11)])


def passability_inputs():
    """Bounded prerequisite controls for the original FNPC passability leaf."""
    for identity in ('real', 'dummy'):
        base = dict(identity=identity, land_type=0, speed_type=0, movement_zone=0,
                    table_clear_factor=0.0, table_water_factor=1.0, flags=0,
                    level=0, ground_raw=0, deck_raw=0, required_zone=-1,
                    required_level=-1, bridge_aware=False,
                    ignore_infantry=False, ignore_vehicles=False)
        for land in (0, 2):
            for speed, zone in ((0, 0), (6, 4)):
                for clear_factor in (0.0, 1.0):
                    yield dict(base, name=f'{identity}_land{land}_speed{speed}_clear{int(clear_factor)}',
                               land_type=land, speed_type=speed, movement_zone=zone,
                               table_clear_factor=clear_factor,
                               table_water_factor=1.0-clear_factor)
        for land in (0, 2):
            # Winged returns before the supplied mismatched zone/height and
            # occupied plane can be read, and before the zero cost is loaded.
            yield dict(base, name=f'{identity}_winged_land{land}_early',
                       land_type=land, speed_type=4, movement_zone=9,
                       table_clear_factor=float(land == 2),
                       table_water_factor=float(land == 0), required_zone=12345,
                       required_level=7, ground_raw=255, deck_raw=255)
        yield dict(base, name=f'{identity}_bridge_zero_cost_deck', flags=0x100,
                   ground_raw=0x40)
        yield dict(base, name=f'{identity}_bridge_deck_occupied', flags=0x100,
                   deck_raw=0x40)
        yield dict(base, name=f'{identity}_bridge_base_height_zero_cost', flags=0x100,
                   required_level=0, bridge_aware=True)
        yield dict(base, name=f'{identity}_water_bridge_deck_height', land_type=2,
                   table_clear_factor=1.0, table_water_factor=0.0, flags=0x100,
                   required_level=4, ground_raw=0x40)


def passability_query(row):
    """Execute original4834A0 with recorded Cell, arguments and table bytes.

    Building443860 reaches this same body through56DC20/56E7C0. The direct
    controls isolate its retained Cell+EC and bridge-layer reads; they do not
    replace the existing whole-caller comparison or establish map construction.
    """
    u, _r32, _ret, call = fixture.prepare_fixture(dict(movement_zone=row['movement_zone']))
    pointer = fixture.DUMMY if row['identity'] == 'dummy' else fixture.CELLS + (5*16+5)*0x200
    u.mem_write(pointer, dwords(0x7E4EEC))
    u.mem_write(pointer + 0x44, dwords(-1))
    u.mem_write(pointer + 0xEC, dwords(row['land_type']))
    u.mem_write(pointer + 0x11B, bytes([row['level']]))
    u.mem_write(pointer + 0x124, dwords(row['ground_raw'], row['deck_raw']))
    u.mem_write(pointer + 0x140, dwords(row['flags']))
    table = [1.0]*90
    table[0:9] = [row['table_clear_factor']]*9
    table[18:27] = [row['table_water_factor']]*9
    table_bytes = struct.pack('<90f', *table)
    u.mem_write(0x89EA40, table_bytes)
    args = [row['speed_type'], int(row['ignore_infantry']), int(row['ignore_vehicles']),
            row['required_zone'] & 0xFFFFFFFF, row['movement_zone'],
            row['required_level'] & 0xFFFFFFFF, int(row['bridge_aware'])]
    before = bytes(u.mem_read(pointer, 0x148))
    code_before = bytes(u.mem_read(0x4834A0, 0x168))
    table_loads, zone_queries = [], []

    def observe(_u, pc, _size, _data):
        if pc == 0x4835DE:
            index = u.reg_read(UC_X86_REG_EAX)
            address = 0x89EA40 + 4*index
            raw = bytes(u.mem_read(address, 4))
            table_loads.append(dict(index=index, address=f'0x{address:08X}',
                                    raw_hex=raw.hex(), value=struct.unpack('<f', raw)[0]))
        elif pc == 0x56D230:
            zone_queries.append(pc)

    u.hook_add(UC_HOOK_CODE, observe)
    call(0x4834A0, pointer, args)
    accepted = bool(u.reg_read(UC_X86_REG_EAX) & 255)
    assert before == bytes(u.mem_read(pointer, 0x148))
    assert table_bytes == bytes(u.mem_read(0x89EA40, len(table_bytes)))
    assert code_before == bytes(u.mem_read(0x4834A0, 0x168))
    return dict(input=row, args=args, args_hex=dwords(*args).hex(),
                cell_coord=cell(u, pointer + 0x24), cell_before_hex=before.hex(),
                speed_table_hex=table_bytes.hex(), table_loads=table_loads,
                zone_queries=len(zone_queries), accepted=accepted,
                inputs_unchanged=True, code_unchanged=True)


def generate():
    return dict(source='unicorn/gamemd.exe', map_size=[8, 8],
                local_size=[0, 0, 8, 8], supplied_cell_extent=[16, 16],
                source_location_default=[1344, 1344, 0], factory_default=40,
                binary_frame_default=100, cases=[query(row) for row in inputs()],
                native_boundary_cases=[query(row) for row in native_boundary_inputs()],
                passability_cases=[passability_query(row) for row in passability_inputs()])


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Whole original Building443860 rally preparation, GetZone56D230, FNPC56DC20, passability/projection and event1E construction/enqueue on supplied native grid; bounded direct4834A0 real/Dummy prerequisites',
        assumptions=[
            'Shared walk_move_admission.prepare_fixture owns native map, Cell, zone, bridge and direction-table preparation. Map Size8,8/LocalSize0,0,8,8; 16x16 Cell slots; raw group labels2/3; cells/zone flood-fill and bridge-record production are supplied, not established here. Shared Dummy receives the original Cell table7E4EEC for NetID packing; its other prepared fields remain unchanged.',
            'Original Building vtable7E3EBC; supplied physical Location+9C, Type+520 Factory+EB8/Naval+CCE, owner+21C and previous ArchiveTarget+218. Factory40 defaults to Foot0/Normal0; Factory3 and Naval controls establish caller options, not input-action admission.',
            'Whole-caller cases use land speed multiplier1.0 except the explicit zero-speed control; land is Clear0 with no overlay, zero raw occupation unless specified. Direct passability controls supply the separate tables recorded per row. These synthetic tables isolate code behavior, not retail terrain balancing.',
            'Original direction initializer49F2F0; FPCW0E7F, height104/bridge416 startup constants supplied as in the existing map fixture. Binary frame is explicit. Native56E6AF/CDQ and56E6B2/56E6D1 signed IDIV select a signed remainder. General comparisons use frames0..7FFFFFFF; frame80000000 is only a separately observed remainder-zero control. FrameFFFFFFFF is a negative pool-index boundary, not a portable native gameplay golden.',
            'Final selector metadata observes the pending native load at56E6BF/56E6DE. Boundary controls demonstrate that a negative index reads the prior local array: eight-candidate zeroed/poisoned stack results differ; a24-candidate control reads initialized raw slot23. No general native parity is claimed for high-bit frame values, and native stack underflow is not prescribed as VERA behavior.',
            'announce=false; whole443860 reaches original event packing/constructor and outgoing queue. ConYard=false; redeploy fallback, EVA, input action selection, event dispatch, gameplay-side ArchiveTarget assignment and rendered output are outside this corpus.',
            'Actor/type/House and click bytes remain unchanged, original code ranges remain unchanged, and original Random/RandomRanged and SetArchive entry counts stay zero. Event output records only initialized header/NetID fields; unused event payload is not asserted.',
            'The separate28 passability_cases execute original4834A0 directly through the same map fixture. Both real and Dummy Cell+EC select opposite supplied Clear0/Water2 speed rows. Foot0/Normal0 and Amphibious6/AmphibiousCrusher4 use zero/nonzero table controls; Winged4/Fly9 controls bypass zero speed, occupied planes, mismatched height and zone before any table/zone access.',
            'Direct bridge controls use flag0x100 and base level0: no requested level selects the deck and permits zero land cost; deck occupation rejects; requested base height0 with bridge_aware=true selects ground and rejects zero cost; requested height4 selects the deck. Cell bytes, seven literal dword arguments, float-table bytes, actual loaded table indices/values and output AL are recorded. This28-row matrix is representative, not exhaustive; no native TerrainRules parser or map/dummy lifecycle execution is added.',
        ],
        substitutions=['Only imported timeGetTime at4439FD/443B4B returns0 for the outgoing command wall timestamp. No gameplay, zone, passability, projection or event callable is replaced.'],
        entry_points={'set_rally': 0x443860, 'direction_initializer': 0x49F2F0,
                      'get_zone': 0x56D230, 'fnpc': 0x56DC20, 'rectangle': 0x56E7C0,
                      'passability': 0x4834A0, 'projection': 0x6D6410,
                      'net_id_pack': 0x6E6AB0, 'event_constructor': 0x4C6780}),
        source_paths={'rally_input': Path(__file__), 'shared_map_fixture': Path(fixture.__file__)})
