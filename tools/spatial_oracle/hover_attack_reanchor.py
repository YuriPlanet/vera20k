"""Original grounded Unit Guard/Attack re-anchor after a supplied FNPC miss.

Both FNPC results are supplied; NearbyLocation, class setters, Cell getters,
Jumpjet MoveTo and its placement/NavCom rewrite execute original instructions.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP

from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.track_destination import make_destination_fixture
from tools.spatial_oracle.unit_scatter_state import ACTOR, LOCO, TYPE
from tools.spatial_oracle.unit_source_scatter import CELLS, SCENARIO, TABLE

MAP, DUMMY, ZONES, RAW = 0x87F7E8, 0xABDC50, 0x40000000, 0x40010000


def query(mission, missing_zero):
    u, call, read32 = make_destination_fixture(dict(
        family='jumpjet', head=[0, 0, 0], prior=[0, 0, 0]))
    u.mem_write(ACTOR + 0x9C, dwords(2688, 2688, 0))
    u.mem_write(ACTOR + 0x2B4, dwords(CELLS + (10 * 32 + 14) * 0x200))
    u.mem_write(ACTOR + 0xAC, dwords(5 if mission == 'guard' else 1))
    u.mem_write(TYPE + 0x390, b'\x01')  # HoverAttack.
    u.mem_write(TYPE + 0x5B4, dwords(9))  # Fly.
    u.mem_write(TYPE + 0x67C, dwords(3))  # Hover.
    # Supplied retained zone cache using bridge_repair_zones' existing layout.
    # Original GetZoneID executes unchanged; every supplied node has zone1.
    u.mem_map(ZONES, 0x20000)
    u.mem_write(MAP + 0xF4, dwords(32, 32))
    u.mem_write(MAP + 0x68, dwords(ZONES, 65 * 65))
    u.mem_write(ZONES, b'\0\0\x01\0' * (65 * 65))
    for zone in range(13):
        u.mem_write(MAP + 0x18 + zone * 4, dwords(RAW))
    u.mem_write(RAW, struct.pack('<HH', 0, 1))
    call(0x47BBF0, DUMMY, [])
    if missing_zero:
        u.mem_write(TABLE, dwords(0))
    events = []

    def xyz(address):
        return list(struct.unpack('<iii', u.mem_read(address, 12)))

    def observe(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == 0x56DC20:
            result = read32(sp + 4)
            events.append(['supplied_fnpc_empty', hex(read32(sp)),
                           list(struct.unpack('<hh', u.mem_read(read32(sp + 8), 4))),
                           read32(sp + 12), read32(sp + 20)])
            u.mem_write(result, packed(0, 0))
            u.reg_write(UC_X86_REG_EAX, result)
            u.reg_write(UC_X86_REG_EIP, read32(sp))
            u.reg_write(UC_X86_REG_ESP, sp + 4 + 0x3C)
        elif address == 0x741970:
            target = read32(sp + 4)
            assert target == (DUMMY if missing_zero else CELLS)
            events.append(['unit_destination', 'dummy' if target == DUMMY else 'real',
                           read32(sp + 8)])
        elif address == 0x54B1C0:
            events.append(['jumpjet_move_to', xyz(sp + 8)])
        elif address == 0x5657A0:
            events.append(['map_get_cell',
                           list(struct.unpack('<hh', u.mem_read(read32(sp + 4), 4)))])
        elif address == 0x7447B0:
            raise AssertionError('unexpected bridge-preflight redirect')

    u.hook_add(UC_HOOK_CODE, observe)
    before = bytes(u.mem_read(SCENARIO + 0x218, 1012))
    # Stop after the concrete setter returns, before later mission targeting
    # and cadence work whose weapon/type inputs are outside this fixture.
    continuation = 0x4D5225 if mission == 'guard' else 0x4D4E0D
    call(0x740810 if mission == 'guard' else 0x4D4DC0, ACTOR, [], continuation)
    nav = read32(ACTOR + 0x5A4)
    assert nav == (DUMMY if missing_zero else CELLS), (mission, missing_zero, hex(nav), events)
    return dict(input=dict(mission=mission, missing_zero=missing_zero), events=events,
                output=dict(continuation=hex(continuation),
                            nav='dummy' if nav == DUMMY else 'real',
                            destination=xyz(LOCO + 0x40),
                            moving=bool(u.mem_read(LOCO + 0x4C, 1)[0]),
                            phase=read32(LOCO + 0x50),
                            movement_timer=[read32(ACTOR + 0x640), read32(ACTOR + 0x648)],
                            rng_unchanged=bytes(u.mem_read(SCENARIO + 0x218, 1012)) == before))


def generate():
    return [query(mission, missing) for mission in ('guard', 'attack')
            for missing in (False, True)]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Four grounded Unit Guard/Attack prefixes through failed re-anchor setter return.',
        entry_points=dict(unit_guard=0x740810, foot_attack=0x4D4DC0,
                          nearby_location=0x703590, unit_destination=0x741970,
                          jumpjet_move_to=0x54B1C0, unit_placement=0x54D6D0),
        assumptions=[
            'Inherited track_destination supplied Unit/House/map prestates and original '
            'class vtables. Original Jumpjet constructor, linked Unit, phase0 and no NavCom.',
            'Grounded HoverAttack1/SpeedTypeHover3/MovementZoneFly9, matching stock SHAD/SCHP '
            'fields; no BalloonHover, deployment in progress, bridge or queued mission.',
            'Held Cell target, original Dummy constructor, optional unallocated0,0 and a '
            'supplied uniform retained zone cache. No physical type reader or landing history.'],
        substitutions=[
            'Only the two FNPC56DC20 results are supplied empty0,0: NearbyLocation and '
            'Jumpjet MoveTo placement. Search scanning is outside these controls.',
            'Inherited OS interlocked import substitutions. No original destination, '
            'getter, placement, RNG or Jumpjet decision body replaced. Stop at4D5225/4D4E0D '
            'after setter return, before later mission/cadence work; no Process/full match.']))
