"""Original source-aware Infantry Scatter through destination dispatch.

Runs the real entry, house/mission/ability/Walk readers, heading math, Scenario
RNG, Foot navigation coordinate, map lookup/playfield, height and projection.
Default Can_Enter_Cell answers and QueueMission/SetDestination effects are
observable seams. Null-source rows take the null arm with FNPC answering
NullCell, so its fallback reaches the same eight-neighbour search; the
immediate locomotor Process call site is observed and skipped. The companion infantry_scatter_entry corpus selects the real
+1AC body instead. The infantry_scatter_destination corpus additionally executes QueueMission,
Infantry/Foot SetDestination and Walk MoveTo, observing first Process separately.
"""
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP,
    UC_X86_REG_FPCW, UC_X86_REG_ESI, UC_X86_REG_EBP, UC_X86_REG_EDI, UC_X86_REG_EBX,
)
from tools.native_oracle import (
    load_image, run_checked, finish_vectors, provenance,
    SCRATCH, STACK_BASE, STACK_SIZE, RET_MAGIC,
)
from tools.spatial_oracle.map_queries import dwords, packed

ACTOR, TYPE, HOUSE, LOCO, RULES, SOURCE, VT, SCENARIO = [
    SCRATCH + i * 0x2000 for i in range(8)]
CELLS = SCRATCH + 0x10000
ENTRY, QUEUE, SET = [SCRATCH + 0xF000 + i * 0x100 for i in range(3)]
MAP, TABLE, DUMMY = 0x87F7E8, 0xC00000, 0xABDC50
BOUNDS = (16, -16, -16, 64, 64)


def query(case, *, teleport_records=None, teleport_type_inputs=None, teleport_overlay_inputs=None):
    teleport = bool(case.get('teleport_destination'))
    if teleport:
        assert case.get('live_entry') and case.get('live_setter')
        assert teleport_records is not None and len(teleport_records) == 42
        assert teleport_type_inputs is not None
        assert teleport_type_inputs['records'] == teleport_records
    if case.get('overlays'):
        assert teleport and teleport_overlay_inputs is not None
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(SCRATCH, 0xA0000)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)
    u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
    table = bytearray(0x100000)
    overrides = {(x, y): (level, flags) for x, y, level, flags in case.get('cells', [])}
    for y in range(32):
        for x in range(32):
            ptr = CELLS + (y * 32 + x) * 0x200
            struct.pack_into('<I', table, (y * 512 + x) * 4, ptr)
            u.mem_write(ptr, dwords(0x7E4EEC))
            u.mem_write(ptr + 0x24, packed(x, y))
            level, flags = overrides.get((x, y), (0, 0))
            u.mem_write(ptr + 0x11B, bytes([level & 255, 0]))
            u.mem_write(ptr + 0x140, dwords(flags))
    if case.get('live_entry'):
        # Declared empty lists/overlays/raw owners. The real +1AC body reads
        # Cell land rows and raw occupation independently of object lists.
        for y in range(32):
            for x in range(32):
                ptr = CELLS + (y * 32 + x) * 0x200
                u.mem_write(ptr + 0x44, dwords(-1))
                u.mem_write(ptr + 0x54, dwords(-1, -1))
        for x, y, ground, deck in case.get('raw', []):
            ptr = CELLS + (y * 32 + x) * 0x200
            u.mem_write(ptr + 0x124, dwords(ground, deck))
        for x, y in case.get('blocked_terrain', []):
            ptr = CELLS + (y * 32 + x) * 0x200
            u.mem_write(ptr + 0xEC, dwords(1))
        for x, y, slope in case.get('slopes', []):
            ptr = CELLS + (y * 32 + x) * 0x200
            u.mem_write(ptr + 0x11C, bytes([slope]))
        u.mem_write(0x89EA40, struct.pack('<18f', *([1.0] * 9 + [0.0] * 9)))
    u.mem_write(TABLE, bytes(table))
    u.mem_write(MAP + 0x13C, dwords(TABLE, 0x40000))
    bounds = case.get('bounds', BOUNDS)
    u.mem_write(MAP + 0xF4, dwords(bounds[0]))
    u.mem_write(MAP + 0xFC, dwords(*bounds[1:]))
    u.mem_write(DUMMY + 0x11B, b'\0\0')
    u.mem_write(DUMMY + 0x140, dwords(0))
    u.mem_write(DUMMY + 0x24, packed(123, -234))
    u.mem_write(VT, bytes(u.mem_read(0x7EB058, 0x600)))
    assert struct.unpack('<I', u.mem_read(VT + 0x1AC, 4))[0] == 0x51BF90
    for offset, pointer in [(0x1AC, ENTRY), (0x1E8, QUEUE), (0x480, SET)]:
        if (offset == 0x1AC and not case.get('live_entry')
                or offset != 0x1AC and not case.get('live_setter')):
            u.mem_write(VT + offset, dwords(pointer))
    if teleport:
        assert bytes(u.mem_read(VT, 0x600)) == bytes(u.mem_read(0x7EB058, 0x600))
    u.mem_write(ACTOR, dwords(VT))
    u.mem_write(ACTOR + 0x6C0, dwords(TYPE))
    u.mem_write(ACTOR + 0x6C4, dwords(case.get('doing', -1)))
    u.mem_write(ACTOR + 0x21C, dwords(HOUSE))
    u.mem_write(ACTOR + 0x674, dwords(LOCO))
    u.mem_write(ACTOR + 0x684, b'\xff')
    u.mem_write(ACTOR + 0xAC, dwords(case.get('mission', 5)))
    u.mem_write(ACTOR + 0xB4, dwords(-1))
    actor = case.get('actor', [2688, 2688, 0])
    u.mem_write(ACTOR + 0x9C, dwords(*actor))
    u.mem_write(ACTOR + 0x8C, bytes([case.get('on_bridge', False)]))
    u.mem_write(TYPE + 0xEBF, b'\x01')
    u.mem_write(LOCO, dwords(0x7F69F8))
    u.mem_write(LOCO + 8, dwords(ACTOR))
    u.mem_write(LOCO + 0x24, dwords(*case.get('head', [0, 0, 0])))
    u.mem_write(0xA8E3A8 + 5 * 32 + 9, b'\x01')
    u.mem_write(0x8871E0, dwords(RULES))
    u.mem_write(0xA8B230, dwords(SCENARIO))
    u.mem_write(0xA8F1E0, packed(0, 0))
    for address in [0x89C848, 0xA8F200, 0x8B3DA8]:
        u.mem_write(address, dwords(0, 0, 0))
    u.mem_write(SOURCE, dwords(*case.get('source', [1000, 2688, 0])))

    def read32(address):
        return struct.unpack('<I', u.mem_read(address, 4))[0]

    def ret(cleanup, value=0):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    events = []
    checks = []
    destination = None
    start_direction = None
    pending_entry = None
    entry_return = None
    fnpc_seed = None
    phase = 'setup'
    native_trace = []
    pending_native = []
    native_returns = []
    rngs = {'main': 0x886B88, 'scenario': SCENARIO + 0x218, 'mapgen': 0xABE890}

    def rng():
        return {name: bytes(u.mem_read(ptr, 1012)).hex() for name, ptr in rngs.items()}

    def rng_indices():
        return {name: list(struct.unpack('<2i', u.mem_read(ptr + 4, 8)))
                for name, ptr in rngs.items()}

    def cell_pointer(coord):
        x, y = coord
        assert 0 <= x < 32 and 0 <= y < 32, coord
        return CELLS + (y * 32 + x) * 0x200

    tracked_cells = sorted({(actor[0] // 256, actor[1] // 256)} |
                           {tuple(command['cell']) for command in case.get('commands', [])
                            if 'cell' in command})

    if case.get('transport'):
        tracked_cells = sorted(set(tracked_cells) |
                               {tuple(value // 256 for value in command['coords'][:2])
                                for command in case['commands'] if 'coords' in command} |
                               {tuple(value // 256 for value in case['transport']['coords'][:2])})

    def cell_snapshot(coord):
        ptr = cell_pointer(coord)
        state = dict(coord=list(coord), pointer=hex(ptr),
                     raw=[read32(ptr + 0x124), read32(ptr + 0x128)],
                     owners=[read32(ptr + 0x54), read32(ptr + 0x58)],
                     structural_flags=read32(ptr + 0x140),
                     level=u.mem_read(ptr + 0x11B, 1)[0],
                     slope=u.mem_read(ptr + 0x11C, 1)[0], land=read32(ptr + 0xEC))
        if case.get('transport'):
            state['content_head'] = hex(read32(ptr + 0xE4))
            state['deck_content_head'] = hex(read32(ptr + 0xE8))
        if case.get('overlays'):
            state.update(overlay_id=struct.unpack('<i', u.mem_read(ptr + 0x44, 4))[0],
                         overlay_data=u.mem_read(ptr + 0x11E, 1)[0],
                         wall_owner=struct.unpack('<i', u.mem_read(ptr + 0x50, 4))[0])
        return state

    def teleport_snapshot():
        nav = read32(ACTOR + 0x5A4)
        navcell = (list(struct.unpack('<2h', u.mem_read(nav + 0x24, 4))) if nav else None)
        if nav and case.get('transport') and nav == SCRATCH + 0x92000:
            navcell = [value // 256 for value in case['transport']['coords'][:2]]
        state = dict(
            actor=dict(coords=list(struct.unpack('<3i', u.mem_read(ACTOR + 0x9C, 12))),
                       mission=struct.unpack('<i', u.mem_read(ACTOR + 0xAC, 4))[0],
                       queued_mission=struct.unpack('<i', u.mem_read(ACTOR + 0xB4, 4))[0],
                       mission_status=read32(ACTOR + 0xBC),
                       mission_timer=[struct.unpack('<i', u.mem_read(ACTOR + x, 4))[0] for x in (0xC8, 0xD0)],
                       doing=struct.unpack('<i', u.mem_read(ACTOR + 0x6C4, 4))[0],
                       stage=[struct.unpack('<i', u.mem_read(ACTOR + offset, 4))[0]
                              for offset in (0xF8, 0xFC, 0x100, 0x108, 0x10C, 0x110)],
                       on_bridge=u.mem_read(ACTOR + 0x8C, 1)[0],
                       prone=u.mem_read(ACTOR + 0x6DB, 1)[0],
                       navcom=hex(nav), navcell=navcell,
                       aux=read32(ACTOR + 0x5A0), path=list(struct.unpack('<4i', u.mem_read(ACTOR + 0x5E0, 16))),
                       reference=list(struct.unpack('<2h', u.mem_read(ACTOR + 0x558, 4))),
                       movement_timer=[struct.unpack('<i', u.mem_read(ACTOR + x, 4))[0] for x in (0x640, 0x648)],
                       blocked_timer=[struct.unpack('<i', u.mem_read(ACTOR + x, 4))[0] for x in (0x668, 0x670)],
                       retries=read32(ACTOR + 0x64C), entry_blocked=u.mem_read(ACTOR + 0x6DC, 1)[0]),
            locomotor=dict(armed_xyz=list(struct.unpack('<3i', u.mem_read(LOCO + 0x1C, 12))),
                           resolved_xyz=list(struct.unpack('<3i', u.mem_read(LOCO + 0x28, 12))),
                           request_byte=u.mem_read(LOCO + 0x34, 1)[0],
                           secondary_byte=u.mem_read(LOCO + 0x36, 1)[0],
                           powered=u.mem_read(LOCO + 0x10, 1)[0], reference_count=read32(LOCO + 0x14)),
            cells=[cell_snapshot(coord) for coord in tracked_cells], rng_indices=rng_indices())
        if case.get('refused_enter') or case.get('chrono_timer'):
            state['actor']['warping_in'] = bool(u.mem_read(ACTOR + 0x271, 1)[0])
            state['actor']['target'] = hex(read32(ACTOR + 0x2B4))
            state['locomotor']['timer_state'] = struct.unpack('<i', u.mem_read(LOCO + 0x38, 4))[0]
            state['locomotor']['timer'] = [struct.unpack('<i', u.mem_read(LOCO + x, 4))[0]
                                         for x in (0x3C, 0x40, 0x44)]
        return state

    # Observations only: none of these original gameplay bodies are replaced.
    # Argument counts also pin the actual callee stack cleanup at each return.
    teleport_points = {
        0x51AA40: ('infantry_destination', 2), 0x4D94B0: ('foot_destination', 2),
        0x718080: ('is_moving', 1), 0x4834A0: ('clear_cell', 7),
        0x51DAF0: ('stop_driver', 0), 0x51D6F0: ('do_action', 3),
        0x51BF90: ('can_enter', 5), 0x718230: ('stop_moving', 1),
        0x718100: ('move_to', 4), 0x718B70: ('resolve', 1),
        0x481180: ('place_in_cell', 5), 0x5217C0: ('raw_put', 1),
        0x521850: ('raw_remove', 1), 0x65C7E0: ('random_range', 2),
        0x5B3040: ('effective_mission', 0), 0x5B35E0: ('queue_mission', 2),
        0x5B3570: ('commence', 0), 0x521B60: ('ready', 0),
        0x50B730: ('is_human', 0),
    }
    if case.get('transport'):
        teleport_points.update({0x746E20: ('unit_rtti', 0),
                                0x41BEA0: ('object_mapcoords', 1),
                                0x4DBDF0: ('foot_navigation_coord', 2),
                                0x578080: ('ground_height', 1)})
    if case.get('chrono_timer'):
        teleport_points[0x719BF0] = ('timer_check', 1)
    if case.get('refused_enter'):
        teleport_points.update({0x51D0D0: ('scatter', 3), 0x56DC20: ('fnpc', 15),
                                0x7192F0: ('process', 1), 0x719BF0: ('timer_check', 1)})
    if case.get('overlays'):
        teleport_points.update({0x701120: ('is_armed', 0), 0x70E140: ('get_weapon', 1),
                                0x70E1A0: ('get_primary', 0), 0x772AC0: ('weapon_can_destroy_wall', 0)})

    def observe_teleport(address, sp):
        for index in range(len(pending_native) - 1, -1, -1):
            pending = pending_native[index]
            if address == pending['return_pc'] and sp == pending['return_sp']:
                pending_native.pop(index)
                returned = dict(kind='return', call=pending['call'], pc=hex(address),
                                name=pending['name'], eax=u.reg_read(UC_X86_REG_EAX),
                                state=teleport_snapshot())
                if case.get('transport') and pending['name'] == 'place_in_cell':
                    returned['output_xyz'] = list(struct.unpack(
                        '<3i', u.mem_read(u.reg_read(UC_X86_REG_EAX), 12)))
                if case.get('refused_enter') and pending['name'] == 'fnpc':
                    returned['result_cell'] = list(struct.unpack(
                        '<2h', u.mem_read(u.reg_read(UC_X86_REG_EAX), 4)))
                native_trace.append(returned)
                break
        if address in (0x56DC20, 0x7192F0) and not case.get('refused_enter'):
            raise AssertionError(('selected Cell request reached excluded native body', hex(address), phase))
        if address in (0x51B1DE, 0x7181F6, 0x719283, 0x718259):
            native_returns.append(dict(pc=hex(address), phase=phase, esp=hex(sp), eax=u.reg_read(UC_X86_REG_EAX)))
        if case.get('overlays') and address in (0x51C17C, 0x51C193, 0x51C1BB, 0x51C1D7,
                                                0x51C1E9, 0x51C205, 0x51C7D0):
            native_trace.append(dict(kind='overlay_gate', pc=hex(address), phase=phase,
                                     eax=u.reg_read(UC_X86_REG_EAX), state=teleport_snapshot()))
        if address not in teleport_points:
            return
        name, argc = teleport_points[address]
        args = [read32(sp + 4 + i * 4) for i in range(argc)]
        row = dict(kind='call', call=len(native_trace), name=name, pc=hex(address), phase=phase,
                   this=hex(u.reg_read(UC_X86_REG_ECX)), args=args,
                   return_pc=hex(read32(sp)), state=teleport_snapshot())
        if name in ('raw_put', 'raw_remove', 'resolve', 'ground_height'):
            row['xyz'] = list(struct.unpack('<3i', u.mem_read(args[0], 12)))
        elif name == 'place_in_cell':
            row['requested_xyz'] = list(struct.unpack('<3i', u.mem_read(args[1], 12)))
            row['cell'] = list(struct.unpack('<2h', u.mem_read(u.reg_read(UC_X86_REG_ECX) + 0x24, 4)))
            row['priority'] = args[2] & 255
            row['deck'] = args[3] & 255
            row['use_cell_coords'] = args[4] & 255
        elif case.get('refused_enter') and name == 'fnpc':
            row['seed_cell'] = list(struct.unpack('<2h', u.mem_read(args[1], 4)))
        elif name == 'can_enter':
            row['cell'] = list(struct.unpack('<2h', u.mem_read(args[0] + 0x24, 4)))
        native_trace.append(row)
        pending_native.append(dict(call=row['call'], name=name, return_pc=read32(sp),
                                   return_sp=sp + 4 * (argc + 1)))

    def observe(_u, address, _size, _data):
        nonlocal destination, start_direction, pending_entry, entry_return, fnpc_seed
        sp = u.reg_read(UC_X86_REG_ESP)
        if teleport:
            if address in [read32(0x7E11C8), read32(0x7E11CC)]:
                pointer = read32(sp + 4)
                value = read32(pointer) + (1 if address == read32(0x7E11C8) else -1)
                u.mem_write(pointer, dwords(value))
                ret(4, value)
            else:
                observe_teleport(address, sp)
            return
        if address == 0x51D487:
            start_direction = read32(sp + 0x1C) & 7
        elif address == 0x56DC20:
            # Null arm 51D41D: literal arguments as the hut corpus pins them;
            # the answer is NullCell.
            args = [read32(sp + 4 + i * 4) for i in range(15)]
            fnpc_seed = list(struct.unpack('<hh', u.mem_read(args[1], 4)))
            assert args[2:5] == [0, 0xFFFFFFFF, 0], args
            assert args[5] & 0xFF == int(case.get('on_bridge', False)), args
            assert args[6:12] == [1, 1, 0, 1, 0, 1], args
            assert list(struct.unpack('<hh', u.mem_read(args[12], 4))) == [0, 0]
            assert args[13:] == [0, 0], args
            events.append('fnpc')
            u.mem_write(args[0], packed(0, 0))
            ret(60, args[0])
        elif address == 0x51D478:
            # The found-cell arm's locomotor Process call: observed, not run.
            events.append('process')
            u.reg_write(UC_X86_REG_EIP, 0x51D47B)
            u.reg_write(UC_X86_REG_ESP, sp + 4)
        elif address == 0x65C7E0:
            events.append('random')
        elif address == entry_return and pending_entry is not None:
            checks.append([*pending_entry, u.reg_read(UC_X86_REG_EAX)])
            pending_entry = None
        elif address == ENTRY or (case.get('live_entry') and address == 0x51BF90):
            args = [read32(sp + 4 + i * 4) for i in range(5)]
            coord = list(struct.unpack('<hh', u.mem_read(args[0] + 0x24, 4)))
            assert args[3:] == [0, 1]
            if case.get('live_entry'):
                pending_entry = [coord, args[1], struct.unpack('<i', dwords(args[2]))[0]]
                entry_return = read32(sp)
                events.append('entry')
                return
            code = case.get('answers', [0] * 8)[args[1]]
            checks.append([coord, args[1], struct.unpack('<i', dwords(args[2]))[0], code])
            events.append('entry')
            ret(20, code)
        elif address == QUEUE or (case.get('live_setter') and address == read32(0x7EB058 + 0x1E8)):
            assert [read32(sp + 4), read32(sp + 8)] == [2, 0]
            events.append('queue_move')
            if not case.get('live_setter'):
                ret(8)
        elif address == SET or (case.get('live_setter') and address == 0x51AA40):
            assert read32(sp + 8) == 1
            destination = list(struct.unpack('<hh', u.mem_read(read32(sp + 4) + 0x24, 4)))
            events.append('destination')
            if not case.get('live_setter'):
                ret(8)
        elif case.get('live_setter'):
            if address in [read32(0x7E11C8), read32(0x7E11CC)]:
                pointer = read32(sp + 4)
                value = read32(pointer) + (1 if address == read32(0x7E11C8) else -1)
                u.mem_write(pointer, dwords(value))
                ret(4, value)
            elif address in [0x4D94B0, 0x75ACB0, 0x75ADA0, 0x51DAF0, 0x4834A0, 0x75AEC0]:
                events.append(hex(address))

    u.hook_add(UC_HOOK_CODE, observe)

    def call(entry, this, args):
        sp = STACK_BASE + STACK_SIZE - 0x1000
        u.mem_write(sp, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ECX, this)
        u.reg_write(UC_X86_REG_ESP, sp)
        run_checked(u, entry, RET_MAGIC, count=300000, required_addresses=[entry],
                    context=dict(owner='infantry_source_scatter', phase=phase, case=case,
                                 native_entry=hex(entry)) if teleport else None)
        assert u.reg_read(UC_X86_REG_ESP) == sp + 4 * (1 + len(args))
        if teleport:
            # emu_start's end address is reached before its code hooks run.
            # Complete the outer observation from the actual returned machine.
            observe_teleport(RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
        return u.reg_read(UC_X86_REG_EAX)

    call(0x49F2F0, 0, [])  # native startup populates the neighbour table
    call(0x65C6D0, SCENARIO + 0x218, [case.get('seed', 1)])
    if teleport:
        # Separate original initialization route; legacy Walk inputs/outputs
        # below remain unchanged. Every gameplay slot retains its image value.
        from tools.spatial_oracle.infantry_entry_raw import STARTUP
        for ptr in (rngs['main'], rngs['mapgen']):
            call(0x65C6D0, ptr, [case.get('seed', 1)])
        startup_before = rng()
        startup_tables = [(0x8129FC, 13), (0x813490, 10), (0x8150B8, 12)]
        startup = []
        for pointer, length in startup_tables:
            entries = list(struct.unpack(f'<{length}I', u.mem_read(pointer, length * 4)))
            if pointer == 0x813490:
                assert entries == STARTUP
            if pointer == 0x8150B8:
                assert entries == [0x717DF0, 0x717E20, 0x717E40, 0x717E60, 0x717E80,
                                   0x717EA0, 0x717EC0, 0x717F00, 0x717F30, 0x717F60,
                                   0x717F90, 0x717FA0]
            for entry in entries:
                call(entry, 0, [])
            startup.append(dict(table=hex(pointer), entries=[hex(entry) for entry in entries]))
        assert read32(0x812B28) == 0x48E480
        offsets_before = bytes(u.mem_read(0x89E9F0, 60)).hex()
        call(0x48E480, 0, [])
        for entry in (0x6D1830, 0x6D18C0, 0x6D1BF0):
            call(entry, 0, [])
        assert startup_before == rng(), 'original geometry startup changed a seeded RNG'
        startup = dict(tables=startup, subcell_entry='0x48e480', subcell_pointer='0x812b28',
                       offsets_before_hex=offsets_before, offsets_after_hex=bytes(u.mem_read(0x89E9F0, 60)).hex(),
                       globals={hex(ptr): struct.unpack('<i', u.mem_read(ptr, 4))[0]
                                for ptr in (0x89E7C0, 0x89E7B4, 0xA8F240, 0xA8F234, 0xB0EC38)},
                       teleport_null=list(struct.unpack('<3i', u.mem_read(0xB0EBF8, 12))),
                       rng_before=startup_before, rng_after=rng())
        call(0x718000, LOCO, [])
        constructor = dict(class_hex=bytes(u.mem_read(LOCO, 0x50)).hex(),
                           fields=teleport_snapshot()['locomotor'],
                           ilocomotion=hex(read32(LOCO + 4)), ipiggyback=hex(read32(LOCO + 0x18)))
        call(0x55A710, 0, [LOCO + 4, ACTOR])
        u.mem_write(LOCO + 0x14, dwords(1))
        u.mem_write(ACTOR + 0x674, dwords(LOCO + 4))
        assert read32(LOCO + 4) == 0x7F5000 and read32(LOCO + 0xC) == ACTOR
        u.mem_write(DUMMY, dwords(0x7E4EEC))
        # Inputs are projected from the separate original constructor/layered
        # reader executed by the existing Mission owner. The selected command
        # fixture does not supply guessed CLEG scalar defaults.
        type_fields = teleport_type_inputs['fields']
        for offset, key in ((0xD94, 'jumpjet'),
                            (0xEBD, 'crawls'), (0xEBF, 'fraidycat'), (0xEAC, 'deployer')):
            u.mem_write(TYPE + offset, bytes([type_fields[key]]))
        for offset, key in ((0x5B4, 'movement_zone'), (0x67C, 'speed_type'), (0xA0, 'strength')):
            u.mem_write(TYPE + offset, dwords(type_fields[key]))
        u.mem_write(TYPE + 0xE40, dwords(*type_fields['fire_frames']))
        u.mem_write(TYPE + 0x34C, bytes.fromhex(type_fields['locomotor_guid']))
        if case.get('overlays'):
            # Only these additive rows receive the independently native-read
            # overlay/normal weapon context. Historical22 prestates stay exact.
            table, overlay_base = SCRATCH + 0x98000, SCRATCH + 0x99000
            u.mem_write(0xA83D84, dwords(table))
            for index, (name, fields) in enumerate(teleport_overlay_inputs['fields'].items()):
                ptr = overlay_base + index * 0x400
                u.mem_write(ptr, bytes.fromhex(teleport_overlay_inputs['class_hex'][name]))
                assert read32(ptr + 0x294) == fields['index']
                u.mem_write(table + fields['index'] * 4, dwords(ptr))
            for overlay in case['overlays']:
                ptr = cell_pointer(overlay['cell'])
                fields = teleport_overlay_inputs['fields'][overlay['name']]
                u.mem_write(ptr + 0x44, dwords(fields['index']))
                u.mem_write(ptr + 0x50, dwords(overlay['owner']))
                u.mem_write(ptr + 0x11E, bytes([overlay['data']]))
            weapon, warhead = SCRATCH + 0x9A000, SCRATCH + 0x9B000
            u.mem_write(weapon, bytes.fromhex(teleport_overlay_inputs['weapon_class_hex']))
            u.mem_write(warhead, bytes.fromhex(teleport_overlay_inputs['warhead_class_hex']))
            u.mem_write(weapon + 0xAC, dwords(warhead))
            u.mem_write(TYPE + 0x898, bytes.fromhex(teleport_overlay_inputs['primary_slot_hex']))
            u.mem_write(TYPE + 0x898, dwords(weapon))
            u.mem_write(TYPE + 0x808, dwords(teleport_overlay_inputs['weapon_count']))
            u.mem_write(TYPE + 0xCD5, bytes([teleport_overlay_inputs['is_gattling']]))
        sequences = SCRATCH + 0x90000
        for index, record in enumerate(teleport_records):
            u.mem_write(sequences + index * 36, struct.pack('<9i', *record))
        u.mem_write(TYPE + 0xE3C, dwords(sequences))
        u.mem_write(HOUSE + 0x30, dwords(case.get('house_index', 0)))
        u.mem_write(HOUSE + 0x1EC, bytes([case.get('human', True)]))
        u.mem_write(HOUSE + 0x1ED, bytes([case.get('player_control', False)]))
        if 'game_mode' in case:
            u.mem_write(0xA8B238, dwords(case['game_mode']))
        human_rng_before = rng()
        human_eax = call(0x50B730, HOUSE, [])
        human_input = dict(game_mode=read32(0xA8B238), house_index=read32(HOUSE + 0x30),
                           is_human_byte=u.mem_read(HOUSE + 0x1EC, 1)[0],
                           player_control_byte=u.mem_read(HOUSE + 0x1ED, 1)[0],
                           original_predicate_eax=human_eax, original_predicate_al=human_eax & 255,
                           rng_before=human_rng_before, rng_after=rng())
        u.mem_write(ACTOR + 0x6C, dwords(type_fields['strength']))
        u.mem_write(ACTOR + 0x74, b'\1')
        u.mem_write(ACTOR + 0xB4, dwords(case.get('queued_mission', -1)))
        u.mem_write(ACTOR + 0x6DB, bytes([case.get('prone', False)]))
        u.mem_write(ACTOR + 0xF8, dwords(2, 1, 17, 0, 91, 92, 1))
        u.mem_write(ACTOR + 0x6DC, b'\1')
        u.mem_write(ACTOR + 0x5A0, dwords(123))
        u.mem_write(ACTOR + 0x5E0, dwords(2, 3, 4, 5))
        u.mem_write(ACTOR + 0x558, packed(9, 8))
        u.mem_write(ACTOR + 0x640, dwords(50, 0, 5))
        u.mem_write(ACTOR + 0x668, dwords(40, 0, 6))
        u.mem_write(ACTOR + 0x64C, dwords(7))
        u.mem_write(0xA8ED84, dwords(case.get('frame', 100)))
        u.mem_write(0xA8EB60, dwords(case.get('game_speed', 1)))
        u.mem_write(RULES + 0x1768, dwords(22))
        call(0x4C91C0, ACTOR + 0x388, [])
        u.mem_write(SOURCE + 0x100, struct.pack('<H', case.get('facing', 0x4000)))
        call(0x4C9300, ACTOR + 0x388, [SOURCE + 0x100])
        if 'navcell' in case:
            u.mem_write(ACTOR + 0x5A4, dwords(cell_pointer(case['navcell'])))
        if case.get('transport'):
            # Declared live NavCom object fields, not a Unit constructor or
            # radio/transport lifecycle. Every virtual dispatch retains the
            # image's Unit vtable; GetMapCoords executes its original body.
            transport = SCRATCH + 0x92000
            u.mem_write(transport, dwords(0x7F5C70))
            assert read32(0x7F5C70 + 0x2C) == 0x746E20
            u.mem_write(transport + 0x9C, dwords(*case['transport']['coords']))
            u.mem_write(transport + 0x6C, dwords(case['transport']['health']))
            u.mem_write(transport + 0x74, b'\1')
            u.mem_write(transport + 0x8C, bytes([case['transport'].get('on_bridge', False)]))
            u.mem_write(transport + 0x684, b'\xff')
            transport_loco = SCRATCH + 0x96000
            call(0x75AA90, transport_loco, [])
            call(0x55A710, 0, [transport_loco + 4, transport])
            u.mem_write(transport_loco + 0x14, dwords(1))
            u.mem_write(transport + 0x674, dwords(transport_loco + 4))
            u.mem_write(transport + 0x21C, dwords(HOUSE))
            u.mem_write(ACTOR + 0x5A4, dwords(transport))
            if case['transport'].get('cell_content'):
                coords = case['transport']['coords']
                u.mem_write(cell_pointer([coords[0] // 256, coords[1] // 256]) +
                            (0xE8 if case['transport'].get('on_bridge') else 0xE4),
                            dwords(transport))
        transport_input = None
        if case.get('transport'):
            phase = 'transport_coordinates'
            transport_rng_before = rng()
            rtti = call(read32(0x7F5C70 + 0x2C), transport, [])
            call(read32(0x7F5C70 + 0x1B8), transport, [SOURCE + 0x300])
            mapcoords = list(struct.unpack('<2h', u.mem_read(SOURCE + 0x300, 4)))
            call(read32(0x7F5C70 + 0x4C), transport, [SOURCE + 0x310, 0])
            navcoords = list(struct.unpack('<3i', u.mem_read(SOURCE + 0x310, 12)))
            transport_input = dict(rtti=rtti, mapcoords=mapcoords, navcoords=navcoords,
                                   vtable=hex(read32(transport)), health=read32(transport + 0x6C),
                                   active=u.mem_read(transport + 0x74, 1)[0],
                                   cell_content=bool(case['transport'].get('cell_content')),
                                   rng_before=transport_rng_before, rng_after=rng(),
                                   limits='Supplied live Unit/House/coordinates and Cell content; original Walk constructor/link and Unit virtual coordinate bodies execute. Unit construction, type read, radio, passenger attach and Teleport Process are excluded.')
            assert rtti == 1
            assert navcoords == case['transport']['coords']
            assert transport_rng_before == rng()
        if case.get('refused_enter') or case.get('chrono_timer'):
            u.mem_write(ACTOR + 0x271, bytes([case.get('warping_in', False)]))
            u.mem_write(LOCO + 0x3C, dwords(*case.get('teleport_timer', [100, 0, 30])))
        if case.get('chrono_timer'):
            u.mem_write(ACTOR + 0x2B4, dwords(SCRATCH + 0x92000))
            u.mem_write(LOCO + 0x38, dwords(case.get('timer_state', 0)))
        phase = 'initial_mark'
        mark_before = teleport_snapshot()
        mark_trace_start = len(native_trace)
        call(0x5217C0, ACTOR, [ACTOR + 0x9C])
        assert not pending_native
        initial_mark = dict(before=mark_before, after=teleport_snapshot(),
                            trace=native_trace[mark_trace_start:])
        native_trace.clear()
        native_returns.clear()
        code_before = bytes(u.mem_read(0x401000, 0x3E0000))
        initial = teleport_snapshot()
        initial_rng = rng()
        boundaries = []
        for index, command in enumerate(case['commands']):
            phase = f'command_{index}'
            before = teleport_snapshot()
            rng_before = rng()
            trace_start, ret_start = len(native_trace), len(native_returns)
            if command['kind'] == 'set_destination':
                eax = call(0x51AA40, ACTOR, [cell_pointer(command['cell']), command.get('flag', 1)])
            elif command['kind'] == 'stop_moving':
                eax = call(0x718230, 0, [LOCO + 4])
            elif command['kind'] == 'queue_mission':
                eax = call(0x5B35E0, ACTOR, [command['mission'], command.get('start', 0)])
            elif command['kind'] == 'timer_check':
                eax = call(0x719BF0, 0, [LOCO])
            elif command['kind'] == 'refused_enter':
                # Supplied branch-entry frame and denied condition, not a
                # substituted RadioCanEnter result or whole PerCell fixture.
                outer_sp = STACK_BASE + STACK_SIZE - 0x1000
                frame_sp = outer_sp - 0x50
                u.mem_write(frame_sp, bytes(0x50) + dwords(RET_MAGIC, 2))
                u.reg_write(UC_X86_REG_ESP, frame_sp)
                u.reg_write(UC_X86_REG_ESI, ACTOR)
                u.reg_write(UC_X86_REG_EBP, 0)
                u.reg_write(UC_X86_REG_EDI, SCRATCH + 0x92000)
                u.reg_write(UC_X86_REG_EBX, 0)
                run_checked(u, 0x51A488, RET_MAGIC, count=300000, required_addresses=[0x51A488],
                            context=dict(owner='infantry_source_scatter', phase=phase, case=case))
                observe_teleport(RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
                assert u.reg_read(UC_X86_REG_ESP) == outer_sp + 8
                eax = u.reg_read(UC_X86_REG_EAX)
            elif command['kind'] == 'resolve':
                # The requested coordinate and NavCom are independent native
                # inputs. This probes the numeric resolver before Process,
                # without supplying a RadioCanEnter or passenger result.
                u.mem_write(SOURCE + 0x200, dwords(*command['coords']))
                eax = call(0x718B70, LOCO, [SOURCE + 0x200])
            else:
                raise AssertionError(('unknown native Cell command', command))
            assert not pending_native, pending_native
            phase = f'query_after_command_{index}'
            query_eax = call(0x718080, 0, [LOCO + 4])
            if command['kind'] == 'set_destination':
                returns = native_returns[ret_start:]
                assert any(row['pc'] == '0x51b1de' for row in returns), returns
                reached = {row['name'] for row in native_trace[trace_start:] if row['kind'] == 'call'}
                if case.get('class_early_return'):
                    assert 'is_human' in reached and 'foot_destination' not in reached, reached
                    assert 'move_to' not in reached and 'place_in_cell' not in reached, reached
                    assert before == teleport_snapshot(), 'Original early return mutated represented state'
                    assert rng_before == rng(), 'Original early return changed a seeded RNG'
                else:
                    assert {'foot_destination', 'move_to', 'resolve', 'raw_remove',
                            'place_in_cell', 'can_enter', 'raw_put'} <= reached, reached
            boundaries.append(dict(input=command, before=before, after=teleport_snapshot(),
                                   eax=eax, query_eax=query_eax, query_al=query_eax & 255,
                                   rng_before=rng_before, rng_after=rng(),
                                   trace=native_trace[trace_start:], returns=native_returns[ret_start:]))
        assert code_before == bytes(u.mem_read(0x401000, 0x3E0000))
        assert u.reg_read(UC_X86_REG_FPCW) == 0x0E7F
        result = dict(input=case, before=initial, after=teleport_snapshot(), boundaries=boundaries,
                      rng_before=initial_rng, rng_after=rng(), startup=startup, constructor=constructor,
                      human_input=human_input, initial_mark=initial_mark, type_fields=type_fields,
                      class_slots={hex(slot): hex(read32(VT + slot))
                                   for slot in (0x38, 0x48, 0xF0, 0xF4, 0x1AC, 0x480, 0x500, 0x558)},
                      locomotor_interface_vtable=hex(read32(LOCO + 4)), locomotor_owner=hex(read32(LOCO + 0xC)),
                      text_sha256=hashlib.sha256(code_before).hexdigest(), code_unchanged=True,
                      original_infantry_vtable_sha256=hashlib.sha256(bytes(u.mem_read(VT, 0x600))).hexdigest(),
                      fpcw=hex(u.reg_read(UC_X86_REG_FPCW)))
        if case.get('overlays'):
            result['overlay_setup'] = dict(fields=teleport_overlay_inputs['fields'],
                                           weapon=teleport_overlay_inputs['weapon'],
                                           rookiestate=read32(ACTOR + 0x138),
                                           registry=hex(read32(0xA83D84)))
        if transport_input is not None:
            result['transport_input'] = transport_input
        return result
    if 'facing' in case:
        # Original Facing constructor and Set_Current; the null arm reads
        # Current when the physical coordinate is at its cell centre.
        call(0x4C91C0, ACTOR + 0x388, [])
        u.mem_write(SOURCE + 0x100, struct.pack('<H', case['facing']))
        call(0x4C9300, ACTOR + 0x388, [SOURCE + 0x100])
    if case.get('live_setter'):
        call(0x6D1830, 0, [])
        call(0x6D18C0, 0, [])
        call(0x6D1BF0, 0, [])
        call(0x75AA90, LOCO, [])
        u.mem_write(LOCO + 0xC, dwords(ACTOR))
        u.mem_write(LOCO + 0x14, dwords(1))
        u.mem_write(ACTOR + 0x674, dwords(LOCO + 4))
        u.mem_write(LOCO + 0x28, dwords(*case.get('head', [0, 0, 0])))
        u.mem_write(LOCO + 0x10, bytes([not case.get('power_off', False)]))
        u.mem_write(LOCO + 0x34, bytes([case.get('moving', False)]))
        if case.get('moving'):
            u.mem_write(LOCO + 0x1C, dwords(7808, 2688, 0))
        u.mem_write(TYPE + 0xE3C, dwords(SCRATCH + 0x90000))
        u.mem_write(ACTOR + 0x5A0, dwords(123))
        u.mem_write(ACTOR + 0x5E0, dwords(2, 3, 4, 5))
        u.mem_write(ACTOR + 0x558, packed(9, 8))
        u.mem_write(ACTOR + 0x6B7, b'\x01')
        u.mem_write(ACTOR + 0x6DC, b'\x01')
        u.mem_write(ACTOR + 0x640, dwords(50, 0, 5))
        u.mem_write(ACTOR + 0x668, dwords(40, 0, 6))
        u.mem_write(ACTOR + 0x64C, dwords(7))
        u.mem_write(ACTOR + 0x388, struct.pack('<HH', 0x4000, 0x4000))
        u.mem_write(ACTOR + 0x6AD, bytes([case.get('swap_active', False)]))
        u.mem_write(ACTOR + 0x82, bytes([case.get('open_transport', False)]))
        u.mem_write(ACTOR + 0x2E4, dwords(TYPE if case.get('bunker', False) else 0))
        u.mem_write(ACTOR + 0x270, bytes([case.get('warp_out', False), case.get('warp_in', False)]))
        u.mem_write(0xA8ED84, dwords(100))
        u.mem_write(RULES + 0x1768, dwords(22))
        u.mem_write(0xA8E3A8 + case.get('mission', 5) * 32 + 9, b'\x01')
    call(0x51D0D0, ACTOR, [SOURCE, case.get('force', False), False])
    assert pending_entry is None
    if case.get('live_entry'):
        assert checks, 'live-entry witnesses must reach the original body'
    result = dict(input=case, destination=destination, checks=checks, events=events,
                  start_direction=start_direction,
                  random_indices=[read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)])
    if fnpc_seed is not None:
        result['fnpc_seed'] = fnpc_seed
    if case.get('live_setter'):
        result['setter'] = dict(
            nav=list(struct.unpack('<hh', u.mem_read(read32(ACTOR + 0x5A4) + 0x24, 4))) if read32(ACTOR + 0x5A4) else None,
            aux=read32(ACTOR + 0x5A0),
            destination=list(struct.unpack('<iii', u.mem_read(LOCO + 0x1C, 12))),
            head=list(struct.unpack('<iii', u.mem_read(LOCO + 0x28, 12))),
            queue=list(struct.unpack('<iiii', u.mem_read(ACTOR + 0x5E0, 16))),
            reference=list(struct.unpack('<hh', u.mem_read(ACTOR + 0x558, 4))),
            queued_mission=struct.unpack('<i', u.mem_read(ACTOR + 0xB4, 4))[0],
            facing=read32(ACTOR + 0x388),
            moving=u.mem_read(LOCO + 0x34, 1)[0],
            powered=u.mem_read(LOCO + 0x10, 1)[0],
            blocked=u.mem_read(ACTOR + 0x6B7, 1)[0],
            movement_timer=[read32(ACTOR + 0x640), read32(ACTOR + 0x648)],
            blocked_timer=[read32(ACTOR + 0x668), read32(ACTOR + 0x670)],
            retries=read32(ACTOR + 0x64C),
            entry_blocked=u.mem_read(ACTOR + 0x6DC, 1)[0],
        )
        assert '0x75aec0' not in events, 'source-aware Scatter must not Process immediately'
        if case.get('process_probe'):
            before_events = len(events)
            endpoint = 0x75BD29 if any(result['setter']['head']) else 0x4D3920
            sp = STACK_BASE + STACK_SIZE - 0x1000
            u.mem_write(sp, dwords(RET_MAGIC, 0))
            u.reg_write(UC_X86_REG_ESP, sp)
            u.reg_write(UC_X86_REG_ECX, LOCO)
            run_checked(u, 0x75AEC0, endpoint, count=100000, required_addresses=[0x75AEC0])
            result['process'] = dict(endpoint=hex(endpoint), events=events[before_events:])
            del events[before_events:]

    return result


def generate():
    cases = [dict(seed=seed, source=source) for seed in (1, 31, 42)
             for source in ([1000, 2688, 0], [4000, 2688, 0], [2688, 1000, 0],
                            [2688, 4000, 0], [1000, 1000, 0], [4000, 4000, 0],
                            [1000, 4000, 0], [4000, 1000, 0], [2688, 2688, 1])]
    cases += [dict(answers=[code] * 8) for code in range(1, 8)]
    cases += [dict(answers=[0 if i == direction else 7 for i in range(8)])
              for direction in range(8)]
    cases += [dict(cells=[[11, 9, 0, 0x100]]),
              dict(cells=[[x, y, 0, 0x100] for x, y in
                          [(10, 9), (11, 9), (11, 10), (11, 11),
                           (10, 11), (9, 11), (9, 10), (9, 9)]]),
              dict(cells=[[12, 10, 8, 0]]),
              dict(cells=[[11, 9, 0, 0x1000], [12, 10, 0, 0x100]]),
              dict(cells=[[10, 10, -1, 0]], on_bridge=True),
              dict(head=[15 * 256 + 128, 15 * 256 + 128, 0]),
              dict(bounds=[16, 0, 0, 1, 1]),
              dict(actor=[128, 128, 0], source=[-256, 128, 0]),
              dict(doing=7), dict(force=True, doing=31),
              dict(source=[-2147483648, 2688, 0]),
              dict(source=[2688, -2147483648, 0]),
              dict(actor=[384, 384, 0], source=[384, 1000, 0]),
              dict(actor=[384, 384, 0], source=[1000, 1000, 0],
                   cells=[[0, 1, 0, 0x100]])]
    # Null source: the centre reads the body facing, elsewhere the heading
    # from the centre; FNPC fails, so the eight-neighbour fallback runs.
    null = [0, 0, 0]
    cases += [dict(source=null, facing=facing, seed=seed)
              for facing in (0, 0x1000, 0x4000, 0x9000, 0xF000) for seed in (1, 31)]
    cases += [dict(source=null, facing=0x4000, actor=actor, seed=42)
              for actor in ([2600, 2688, 0], [2688, 2600, 0], [2800, 2800, 0],
                            [2561, 2815, 0], [2688, 2689, 0])]
    cases += [dict(source=null, facing=0x4000, answers=[7] * 8),
              dict(source=null, facing=0x4000, answers=[0 if i == 5 else 7 for i in range(8)]),
              dict(source=null, facing=0xC000, head=[15 * 256 + 128, 15 * 256 + 128, 0]),
              dict(source=null, facing=0x4000, doing=7),
              dict(source=null, facing=0x4000, cells=[[10, 10, -1, 0]], on_bridge=True)]
    return [query(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Source-aware and null-coordinate (FNPC NullCell fallback) Infantry Scatter selection, native RNG and dispatch order; supplied Can_Enter_Cell answers and observed destination receiver, not full movement parity.',
        entry_points={'scatter': 0x51D0D0, 'navigation_coord': 0x4DBDF0,
                      'projection': 0x6D6410, 'height': 0x5F5F00,
                      'direction_startup': 0x49F2F0,
                      'playfield': 0x578460, 'random': 0x65C7E0,
                      'facing_current': 0x4C93D0, 'facing_snap': 0x4C9300},
        assumptions=['Valid Fraidycat Infantry with native Walk interface, AI house, Guard Scatter enabled; zero NullCoord/NullCell.',
                     '32x32 allocated cells and supplied raw playfield fields widened for boundary selection; not a map-loader or retail boundary-reachability fixture. Original heading arithmetic under chop53 control word.',
                     'Original49F2F0 initializes the runtime direction table before Scatter; never use its cold image zeros.',
                     'Null-source rows: original Facing constructor/Set_Current; FNPC answers NullCell. No true/true DoAction31 cases.'],
        substitutions=['Infantry+1AC returns per-direction supplied numeric answers.',
                       'QueueMission and SetDestination are argument-checking observers.',
                       'FNPC 56DC20 checks its null-arm arguments and answers NullCell; the 51D478 Process call is recorded and skipped.']))
