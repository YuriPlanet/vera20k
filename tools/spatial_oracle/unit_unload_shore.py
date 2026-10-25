"""Failed shore-search handoff through original Unit/Foot/Cell/Hover code.

The FNPC empty result is supplied, not its search. See transport_unload.md.
Default checks the retained corpus; --write deliberately regenerates it.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP

from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.refinery_dock import ACTOR, TYPE, cell, make_dock_fixture
from tools.spatial_oracle.unit_scatter_state import LOCO, SOURCE
from tools.spatial_oracle.unit_source_scatter import SCENARIO, TABLE

DUMMY = 0xABDC50


def query(missing_zero, missing_west, height):
    u, call, read32 = make_dock_fixture(dict(
        harvester=False, linked=False, mission='unload', west_building=False, frame=200))
    call(0x513C20, LOCO, [])
    u.mem_write(LOCO + 0xC, dwords(ACTOR))
    u.mem_write(LOCO + 0x14, dwords(1))
    u.mem_write(ACTOR + 0x674, dwords(LOCO + 4))
    u.mem_write(ACTOR + 0xA4, dwords(height))
    u.mem_write(TYPE + 0x5E0, dwords(12))
    u.mem_write(TYPE + 0x5B4, dwords(0))  # Valid Normal MovementZone.
    u.mem_write(ACTOR + 0x2E4, dwords(0))
    u.mem_write(ACTOR + 0x5A4, dwords(0))
    u.mem_write(ACTOR + 0xBC, dwords(0))
    u.mem_write(cell(10, 10) + 0xEC, dwords(2))
    call(0x47BBF0, DUMMY, [])
    if missing_zero:
        u.mem_write(TABLE, dwords(0))
    if missing_west:
        u.mem_write(TABLE + (10 * 512 + 9) * 4, dwords(0))

    events = []

    def xyz(address):
        return list(struct.unpack('<iii', u.mem_read(address, 12)))

    def observe(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == 0x56DC20:
            result = read32(sp + 4)
            events.append(['supplied_fnpc_empty', read32(sp + 12)])
            u.mem_write(result, packed(0, 0))
            u.reg_write(UC_X86_REG_EAX, result)
            u.reg_write(UC_X86_REG_EIP, read32(sp))
            u.reg_write(UC_X86_REG_ESP, sp + 4 + 0x3C)
        elif address == 0x741970:
            target = read32(sp + 4)
            assert target == (DUMMY if missing_zero else cell(0, 0))
            events.append(['unit_destination', 'dummy' if target == DUMMY else 'real',
                           read32(sp + 8)])
        elif address == 0x514D90:
            events.append(['hover_move_to', xyz(sp + 8)])
        elif address in (0x5164D0, 0x514F70):
            events.append(['hover_helper', hex(address),
                           [read32(ACTOR + 0x640), read32(ACTOR + 0x648)],
                           u.mem_read(ACTOR + 0x5E0, 2).hex(), xyz(LOCO + 0x24)])
        elif address == 0x5657A0:
            events.append(['map_get_cell',
                           list(struct.unpack('<hh', u.mem_read(read32(sp + 4), 4)))])
        elif address in (0x7447B0, 0x4D3920):
            raise AssertionError(f'unexpected alternate assignment/pathfinding {address:x}')

    u.hook_add(UC_HOOK_CODE, observe)
    rng = bytes(u.mem_read(SCENARIO + 0x218, 1012))
    call(0x73D630, ACTOR, [])
    delay = u.reg_read(UC_X86_REG_EAX)
    nav = read32(ACTOR + 0x5A4)
    output = dict(
        delay=delay, nav='dummy' if nav == DUMMY else 'real',
        destination=xyz(LOCO + 0x18), head=xyz(LOCO + 0x24),
        path_word=struct.unpack('<h', u.mem_read(ACTOR + 0x5E0, 2))[0],
        movement_timer=[read32(ACTOR + 0x640), read32(ACTOR + 0x648)],
        movement_timer_pad=read32(ACTOR + 0x644),
        handler_state=read32(ACTOR + 0xBC),
        dummy_coord=list(struct.unpack('<hh', u.mem_read(DUMMY + 0x24, 4))),
        rng_unchanged=bytes(u.mem_read(SCENARIO + 0x218, 1012)) == rng)
    handler_events = list(events)
    # A valid MovementZone rejects packed destination cell(0,0) before any
    # zone/map lookup. This is the later Hover failed-path cleanup's gate.
    u.mem_write(SOURCE, dwords(128, 128, 0))
    call(0x4D3810, ACTOR, [SOURCE])
    output['empty_cell_reachable'] = bool(u.reg_read(UC_X86_REG_EAX) & 0xFF)
    # A later miss changes the retained Dummy, not a retained real Cell or
    # the XYZ already copied into Hover. Call the original retained +4C.
    u.mem_write(SOURCE, packed(40, 41))
    call(0x5657A0, 0x87F7E8, [SOURCE])
    call(read32(read32(nav) + 0x4C), nav, [SOURCE + 16, ACTOR])
    output['later_coordinate'] = xyz(SOURCE + 16)
    output['destination_after_later_lookup'] = xyz(LOCO + 0x18)
    return dict(input=dict(missing_zero=missing_zero, missing_west=missing_west,
                           height=height), output=output, events=handler_events)


def generate():
    return [query(False, False, 120), query(True, False, 120),
            query(True, True, 120), query(True, True, 0)]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Four bounded failed-FNPC handoffs through original Unit/Foot/Cell/Hover bodies.',
        entry_points=dict(unload=0x73D630, unit_destination=0x741970,
                          foot_destination=0x4D94B0, cell_coords=0x486840,
                          hover_move_to=0x514D90, path_and_arrival=0x5164D0,
                          process_movement=0x514F70, can_reach_destination=0x4D3810),
        assumptions=[
            'Inherited dock VM supplied type/House/map prestates, original class vtables, '
            'constructed Hover linked to Unit with COM reference1; no radio contacts.',
            'Stopped state0 Unload carrier at10,10 on Water, Passengers12, Harvesterfalse, '
            'NavComNULL; height120 or0. This branch does not inspect passenger links.',
            'Supplied valid Normal MovementZone0; the direct CanReachDestination '
            'control probes the empty-cell rejection before any zone comparison.',
            'All cells real except optional missing0,0 and missing9,10. Original Dummy constructor; '
            'later independent Map miss40,41 probes the retained target getter.',
            'Timer comparisons use authoritative start/duration. Native middle ClockPad '
            'retains destinationY from compiler stack reuse and is recorded separately.'],
        substitutions=[
            'FNPC56DC20 alone returns supplied packed empty0,0. No search or full-map '
            'reachability claim; no gameplay setter/getter/MoveTo body replaced.',
            'Inherited OS interlocked import substitutions. No whole match, global phases, '
            'locomotor Process history, passenger placement, rendering or audio execution.']))
