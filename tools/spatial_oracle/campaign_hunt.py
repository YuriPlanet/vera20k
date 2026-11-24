"""Original Foot Hunt's no-target campaign tail, with live destination owners.

The scanner result is an explicit boundary: the original prologue runs, then
execution resumes at its no-target join (4D54EE). No instruction, vtable or
gameplay return is patched. See campaign_hunt.md for the separate ALL06 producer.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EBP, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP,
)

from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.track_destination import make_destination_fixture
from tools.spatial_oracle.unit_source_scatter import ACTOR, LOCO, CELLS, SCENARIO
from tools.spatial_oracle.unit_entry import EXTRA, HOUSE
from tools.spatial_oracle.unit_scatter_state import SP

PLAYER = EXTRA + 0x6000
MANAGER = EXTRA + 0x20000
HUNT, MOVE, GUARD = 15, 2, 5
RNG_SIZE = 0x3F4
ENTRY, JOIN = 0x4D5350, 0x4D54EE
DEFAULTS = dict(
    game_mode=0, human=False, player_control=False, primary=[14, 14],
    alternate=[0, 0], owner_primary=[6, 6], owner_alternate=[7, 7],
    frame=100, seed=1, queued=-1, ready=False, deploying=False, manager=None,
)
RATES = {HUNT: 0.016, MOVE: 0.027, GUARD: 0.03}


def controls():
    return [
        dict(name='player_primary'),
        dict(name='player_alternate', alternate=[13, 12]),
        dict(name='alternate_zero_uses_primary', primary=[12, 13], alternate=[0, 0]),
        dict(name='alternate_x_zero_is_valid', alternate=[0, 12]),
        dict(name='empty_player_base', primary=[0, 0]),
        dict(name='reset_manager_before_destination', manager=dict(state=5, frame=17)),
        dict(name='empty_base_does_not_reset_manager', primary=[0, 0], manager=dict(state=5, frame=17)),
        dict(name='pending_sticky_is_replaced', queued=6, seed=31),
        dict(name='deploying_keeps_move_queued', deploying=True),
        dict(name='human_campaign_idle', human=True),
        dict(name='player_control_campaign_idle', player_control=True),
        dict(name='computer_noncampaign_idle', game_mode=3),
    ]


def query(spec):
    row = dict(DEFAULTS, **spec)
    u, call, read32 = make_destination_fixture(dict(
        family='drive', mission=HUNT, queued=row['queued'], seed=row['seed'],
        frame=row['frame'], deploying=row['deploying'], prior=[0, 0, 0], head=[0, 0, 0],
    ))
    signed = lambda p: struct.unpack('<i', u.mem_read(p, 4))[0]
    coord = lambda p: list(struct.unpack('<3i', u.mem_read(p, 12)))
    cell_xy = lambda p: list(struct.unpack('<2h', u.mem_read(p, 4)))
    u.mem_write(0xA83D4C, dwords(PLAYER))
    u.mem_write(0xA8B238, dwords(row['game_mode']))
    u.mem_write(HOUSE + 0x1EC, bytes([row['human'], row['player_control']]))
    for house, primary, alternate in (
        (HOUSE, row['owner_primary'], row['owner_alternate']),
        (PLAYER, row['primary'], row['alternate']),
    ):
        u.mem_write(house + 0x5490, packed(*primary))
        u.mem_write(house + 0x5494, packed(*alternate))
    # Explicit selected class priors; no native Unit constructor is claimed.
    u.mem_write(ACTOR + 0xB0, dwords(-1))
    u.mem_write(ACTOR + 0xB8, bytes([row['ready']]))
    u.mem_write(ACTOR + 0x5C4, dwords(-1))
    u.mem_write(ACTOR + 0x5A0, dwords(0))
    u.mem_write(ACTOR + 0x5E0, dwords(-1, 0, 0, 0))
    u.mem_write(ACTOR + 0x6B7, b'\0')
    u.mem_write(ACTOR + 0x64C, dwords(0))
    u.mem_write(ACTOR + 0x640, dwords(-1, 0, 0))
    u.mem_write(ACTOR + 0x668, dwords(-1, 0, 0))
    for mission, rate in RATES.items():
        u.mem_write(0xA8E3A8 + mission * 32 + 0x10, struct.pack('<d', rate))
    call(0x4D30F0, 0, [])
    call(0x4F50A0, 0, [])
    assert cell_xy(0x8B3D88) == cell_xy(0xA8EF98) == [0, 0]
    if row['manager']:
        u.mem_write(ACTOR + 0x2D8, dwords(MANAGER))
        u.mem_write(MANAGER + 0x48, dwords(0))
        u.mem_write(MANAGER + 0x5C, dwords(row['manager']['state'], row['manager']['frame']))

    def reference(pointer):
        if not pointer:
            return None
        assert CELLS <= pointer < CELLS + 32 * 32 * 0x200, hex(pointer)
        return cell_xy(pointer + 0x24)

    def state():
        return dict(
            mission=signed(ACTOR + 0xAC), queued=signed(ACTOR + 0xB4),
            ready=bool(u.mem_read(ACTOR + 0xB8, 1)[0]),
            nav=reference(read32(ACTOR + 0x5A4)),
            destination=coord(LOCO + 0x34), head=coord(LOCO + 0x40),
            manager=(dict(state=signed(MANAGER + 0x5C), frame=signed(MANAGER + 0x60))
                     if row['manager'] else None),
        )

    calls, pending, reached = [], [], set()
    entries = {0x50B730: ('human_gate', 0), 0x50DEF0: ('player_base', 1),
               0x6B0C80: ('manager_reset', 0), 0x5657A0: ('map_cell', 1),
               0x741970: ('unit_destination', 2), 0x4D94B0: ('foot_destination', 2),
               0x4AFD40: ('drive_move_to', 4), 0x5B35E0: ('queue_mission', 2),
               0x5B3614: ('queue_store', None), 0x744270: ('unit_ready', 0),
               0x5B3570: ('commence', 0),
               0x5B3A00: ('mission_rate', 0), 0x65C7E0: ('random_ranged', 2)}

    def observe(vm, pc, _size, _data):
        assert pc not in (0x4B0500, 0x4D3920), 'Hunt tail entered Process/Find_Path'
        reached.add(pc)
        sp = vm.reg_read(UC_X86_REG_ESP)
        for item in pending[:]:
            if pc == item[0]:
                pending.remove(item)
                if item[1]['kind'] == 'player_base':
                    item[1]['cell'] = cell_xy(item[2])
                else:
                    item[1]['returned'] = vm.reg_read(UC_X86_REG_EAX)
                    if item[1]['kind'] in ('human_gate', 'unit_ready'):
                        item[1]['returned'] &= 255
        if pc not in entries:
            return
        kind, count = entries[pc]
        event = dict(kind=kind)
        if count is None:
            calls.append(event)
            return
        args = [read32(sp + 4 + i * 4) for i in range(count)]
        if kind == 'player_base':
            assert vm.reg_read(UC_X86_REG_ECX) == PLAYER
        elif kind == 'manager_reset':
            assert vm.reg_read(UC_X86_REG_ECX) == MANAGER
            event['before'] = state()['manager']
        elif kind == 'map_cell':
            event['cell'] = cell_xy(args[0])
        elif kind in ('unit_destination', 'foot_destination'):
            event.update(cell=reference(args[0]), flag=args[1])
        elif kind == 'drive_move_to':
            assert args[0] == LOCO + 4
            event['coordinate'] = [struct.unpack('<i', dwords(value))[0] for value in args[1:]]
        elif kind in ('queue_mission', 'random_ranged'):
            event['args'] = args
        elif kind == 'mission_rate':
            event['mission'] = signed(ACTOR + 0xAC)
        calls.append(event)
        if kind in ('human_gate', 'player_base', 'unit_ready', 'random_ranged'):
            pending.append((read32(sp), event, args[0] if args else None))

    original = bytes(u.mem_read(0x401000, 0x3E0000))
    tables = {p: bytes(u.mem_read(p, 0x600)) for p in (0x7F5C70, 0x7F6218, 0x7E4EEC)}
    before, rng_before = state(), bytes(u.mem_read(SCENARIO + 0x218, RNG_SIZE)).hex()
    preserved = {reg: 0x10203040 + i for i, reg in enumerate(
        (UC_X86_REG_EBX, UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_EDI))}
    for reg, value in preserved.items():
        u.reg_write(reg, value)
    # Run the real prologue, then supply the no-target join as the entry seam.
    u.mem_write(SP, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ECX, ACTOR)
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, ENTRY, 0x4D5357, required_addresses=[ENTRY])
    hook = u.hook_add(UC_HOOK_CODE, observe)
    try:
        stop = run_checked(u, JOIN, (RET_MAGIC, 0x4D5578), count=300000,
                           required_addresses=[JOIN, 0x50B730])
    finally:
        u.hook_del(hook)
    idle_boundary = stop == 0x4D5578
    if not idle_boundary:
        assert u.reg_read(UC_X86_REG_ESP) == SP + 4
        assert all(u.reg_read(reg) == value for reg, value in preserved.items())
        assert {0x4D5582, 0x5B3A00, 0x7C5F00, 0x65C7E0, 0x4D55B5} <= reached
    assert not pending
    assert original == bytes(u.mem_read(0x401000, 0x3E0000))
    assert all(raw == bytes(u.mem_read(p, len(raw))) for p, raw in tables.items())
    return dict(input=row, before=before, after=state(), calls=calls,
                idle_boundary=idle_boundary,
                returned=None if idle_boundary else u.reg_read(UC_X86_REG_EAX),
                rng_before=rng_before,
                rng_after=bytes(u.mem_read(SCENARIO + 0x218, RNG_SIZE)).hex())


def generate():
    return dict(schema_version=1, inputs=dict(
        rates={str(m): value for m, value in RATES.items()},
        actor_cell=[10, 10], allocated_map=[32, 32], player_distinct_from_owner=True,
        empty_cell=[0, 0], no_target_boundary='0x004D54EE',
        producer_corpus='trigger_all_to_hunt.json',
    ), cases=[query(case) for case in controls()])


def metadata():
    return provenance(
        scope='Original Foot Mission_Hunt no-target join4D54EE through RET4D55B5, or stop before the existing idle owner4D5578. Live House base selection, empty-cell gate, optional empty SlaveManager reset, Unit/Foot/Drive destination, QueueMission and current-mission cadence with Scenario RNG. Separate existing ALL06 Action6/Team data establishes the stock producer; no full scanner, campaign or movement Process claim.',
        entry_points=dict(hunt=ENTRY, no_target=JOIN, human_gate=0x50B730,
                          player_base=0x50DEF0, reset_manager=0x6B0C80,
                          unit_destination=0x741970, foot_destination=0x4D94B0,
                          drive_move_to=0x4AFD40, queue=0x5B35E0,
                          unit_ready=0x744270, commence=0x5B3570,
                          mission_rate=0x5B3A00, random_ranged=0x65C7E0,
                          empty_cell_initializer=0x4D30F0,
                          alternate_cell_initializer=0x4F50A0),
        assumptions=[
            'Fresh track_destination fixture per row: constructed Drive and Facing owners, original Unit/Cell/type vtables, 32x32 flat allocated cells and supplied Unit/House class fields. No Unit/House constructor, physical placement or map loading is claimed.',
            'The unchanged Hunt prologue runs before execution resumes at its no-target join. Neither RetaliateAndScan nor StupidHunt admission runs. Human and noncampaign rows stop before invoking idle; their output is only the native branch boundary and unchanged prestate.',
            'PlayerPtr is a separate House with different primary/alternate cells from the actor owner. Original static initializers4D30F0/4F50A0 establish the zero EmptyCell and alternate sentinel. Selected rates, mission/ready/queue, Unit deploying byte+6E1, manager state/frame and empty node vector are explicit inputs. Slave reset effects on live nodes belong to the existing slave_manager oracle.',
            'Original Random Seed initializes Scenario RNG. Full before/after state is exported. Native text and class vtables remain byte-identical; returning rows preserve callee-save registers and native stack cleanup.'],
        substitutions=['Inherited OS InterlockedIncrement/Decrement transports only. No gameplay return, class vtable slot or native instruction is replaced.'],
    )


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
