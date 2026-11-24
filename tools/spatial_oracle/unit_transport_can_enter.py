"""Bounded Unit transport and cargo-size controls using the existing dock VM owner.

python -m tools.spatial_oracle.unit_transport_can_enter --check

Supplied current-type Size/SizeLimit fields, cargo/alliance states; original Unit
receiver and reached admission/size leaves execute. No copied controller.
"""
import hashlib
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import finish_vectors, provenance, run_checked
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.refinery_dock import make_dock_fixture, cell, BLD, OTHER
from tools.spatial_oracle.unit_scatter_state import ACTOR, TYPE, SP
from tools.spatial_oracle.unit_entry import EXTRA, HOUSE
from tools.spatial_oracle.unit_source_scatter import SCENARIO


def unit_can_enter_cases():
    """Integer Size controls for Unit's existing radio owner, independently of UI."""
    base = dict(capacity=4, size_limit=2, passenger_size=1, cargo_sizes=[])
    controls = [
        ('empty', {}), ('exact_capacity', dict(cargo_sizes=[1, 2])),
        ('count_full_but_zero_sizes', dict(cargo_sizes=[0, 0, 0, 0])),
        ('size_full_not_count_full', dict(cargo_sizes=[2, 2])),
        ('individual_too_large', dict(passenger_size=3)),
        ('zero_size_limit', dict(size_limit=0)), ('no_capacity', dict(capacity=0)),
        ('negative_capacity', dict(capacity=-1)), ('missing_sender', dict(no_sender=True)),
        ('enemy', dict(other_house=True)),
        ('one_way_allied', dict(other_house=True, allied=True)),
        ('reverse_only_alliance', dict(other_house=True, reverse_allied=True)),
        ('warped_transport', dict(warped=True)),
        ('building_in_transport_cell', dict(building=True)),
        ('controlled_passenger', dict(controlled=True)),
        ('parasite_passenger', dict(parasite=True)),
        ('controller_passenger', dict(controlling=True)),
        ('empty_controller', dict(empty_controller=True)),
        ('fractional_limit_equal_1_25', dict(passenger_size=1.25, size_limit=1.25)),
        ('fractional_limit_reject_1_5', dict(passenger_size=1.5, size_limit=1.25)),
        ('capacity_equal_empty_sender4', dict(passenger_size=4, size_limit=4)),
        ('capacity_equal_cargo2_sender2', dict(passenger_size=2, size_limit=2, cargo_sizes=[2])),
        ('fractional_capacity_sender3_75', dict(passenger_size=3.75, size_limit=4)),
    ]
    return [dict(base, name=name, **changes) for name, changes in controls]


def _establish_winmain_fpcw(u):
    """Run the original WinMain controlfp/cache fragment before size consumers."""
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_FPCW, 0x027F)
    run_checked(u, 0x006BBFB7, 0x006BBFCE, count=1000,
                required_addresses=(0x007CBF49, 0x007C5EE4),
                context={'case': 'WinMain x87 control/cache initialization'})
    live = u.reg_read(UC_X86_REG_FPCW)
    cached = struct.unpack('<H', u.mem_read(0x00822D80, 2))[0]
    if (live, cached) != (0x0E3F, 0x0E3F):
        raise RuntimeError(f'WinMain FPCW mismatch: live={live:04X}, cached={cached:04X}')
    return dict(fragment='006BBFB7..006BBFCE', initial='027F', live='0E3F', cached='0E3F')


def unit_can_enter(case):
    """Execute untouched Unit Receive_Radio0F and reached query leaves.

    Inherits existing Drive/Cell/scenario construction. The sender/cargo live
    Infantry fields, House indexes/alliance and type sizes are supplied inputs;
    no radio/admission/size-sum/ftol result is substituted.
    """
    u, call, read32 = make_dock_fixture(dict(seed=31, linked=False, mission='guard'))
    startup_fpcw = _establish_winmain_fpcw(u)
    passenger_type = OTHER + 0x1000
    sender_house = EXTRA + 0x22000
    manager = EXTRA + 0x28000
    # Unit receiver is the existing real Drive fixture; Infantry+84 reads6C0.
    u.mem_write(OTHER, dwords(0x7EB058))
    u.mem_write(OTHER + 0x14, dwords(5))
    u.mem_write(OTHER + 0x6C0, dwords(passenger_type))
    u.mem_write(OTHER + 0x21C, dwords(sender_house if case.get('other_house') else HOUSE))
    u.mem_write(passenger_type + 0x380, struct.pack('<d', case['passenger_size']))
    u.mem_write(TYPE + 0x5E0, dwords(case['capacity']))
    u.mem_write(TYPE + 0x388, struct.pack('<d', case['size_limit']))
    u.mem_write(HOUSE + 0x30, dwords(0))
    u.mem_write(HOUSE + 0x5788, dwords(2 if case.get('allied') else 0))
    u.mem_write(sender_house + 0x30, dwords(1))
    u.mem_write(sender_house + 0x5788, dwords(1 if case.get('reverse_allied') else 0))
    u.mem_write(ACTOR + 0x270, bytes([case.get('warped', False)]))
    u.mem_write(OTHER + 0x2C0, dwords(BLD if case.get('controlled') else 0))
    u.mem_write(OTHER + 0x694, dwords(BLD if case.get('parasite') else 0))
    u.mem_write(OTHER + 0x2BC, dwords(manager if case.get('controlling') or case.get('empty_controller') else 0))
    u.mem_write(manager + 0x34, dwords(int(case.get('controlling', False))))
    nodes = [EXTRA + 0x23000 + index * 0x1000 for index in range(len(case['cargo_sizes']))]
    u.mem_write(ACTOR + 0x114, dwords(len(nodes), nodes[0] if nodes else 0))
    for index, (node, size) in enumerate(zip(nodes, case['cargo_sizes'])):
        u.mem_write(node, dwords(0x7EB058))
        u.mem_write(node + 0x6C0, dwords(node + 0x800))
        u.mem_write(node + 0x800 + 0x380, struct.pack('<d', size))
        u.mem_write(node + 0x30, dwords(nodes[index + 1] if index + 1 < len(nodes) else 0))
    if case.get('building'):
        u.mem_write(cell(10, 10) + 0xE4, dwords(BLD))
    trail = []
    watch = {0x737430, 0x4F9A90, 0x47C520, 0x7105E0, 0x4722C0,
             0x473460, 0x7C5F00, 0x73769E, 0x73780F}
    u.hook_add(UC_HOOK_CODE, lambda _u, pc, _size, _data:
               trail.append(hex(pc)) if pc in watch else None)
    before = bytes(u.mem_read(SCENARIO + 0x218, 1012))
    code = hashlib.sha256(bytes(u.mem_read(0x401000, 0x3E0000))).hexdigest()
    vtables = [bytes(u.mem_read(address, 0x500)) for address in (0x7F5C70, 0x7EB058)]
    call(0x737430, ACTOR, [0 if case.get('no_sender') else OTHER, 0xF, 0xA8EC30])
    reply = u.reg_read(UC_X86_REG_EAX)
    assert before == bytes(u.mem_read(SCENARIO + 0x218, 1012))
    assert code == hashlib.sha256(bytes(u.mem_read(0x401000, 0x3E0000))).hexdigest()
    assert vtables == [bytes(u.mem_read(address, 0x500)) for address in (0x7F5C70, 0x7EB058)]
    return dict(input=case, reply=reply, original_entries=trail,
                startup_fpcw=startup_fpcw,
                fpcw_after=f'{u.reg_read(UC_X86_REG_FPCW):04X}',
                fpcw_cache_after=f'{struct.unpack("<H", u.mem_read(0x00822D80, 2))[0]:04X}',
                scenario_rng_before=before.hex(), scenario_rng_unchanged=True,
                original_text_and_vtables_unchanged=True)


def unit_can_enter_metadata():
    return provenance(
        scope='Bounded Unit737430 Receive_Radio0F from supplied live transport/passenger state; '
              'cargo Size accumulation and capacity/SizeLimit comparisons execute.',
        entry_points={'unit_receive': 0x737430, 'is_ally_by_object': 0x4F9A90,
                      'ground_cell_building': 0x47C520, 'is_mind_controlled': 0x7105E0,
                      'is_controlling': 0x4722C0, 'cargo_total_size': 0x473460,
                      'ftol': 0x7C5F00, 'winmain_fpcw': 0x6BBFB7},
        assumptions=['Reuses refinery_dock actual Drive/Cell/scenario fixture and unchanged '
                     'Unit/Infantry vtables. House indexes/one-way alliance, live sender flags, '
                     'cargo links and selected Size/Passengers/SizeLimit fields are supplied, '
                     'not retail-reader receipts. Constructor/read/physical arrival/radio '
                     'transmit and passenger attachment lifecycles are excluded.',
                     'Actual Unit receiver0F, alliance/warp/GetCell/building/mind-control/'
                     'controller/type queries and cargo linked-list sum/ftol execute without '
                     'supplied query replies. Fractional current-type binary64 sender Size and '
                     'SizeLimit boundaries are direct consumer inputs, not outputs of ReadDouble. '
                     'The existing zero SizeLimit row records native rejection for a positive '
                     'sender. Scenario RNG and original text remain unchanged.',
                     'Each case executes original WinMain controlfp/cache fragment '
                     '006BBFB7..006BBFCE from initial word027F; live FPCW and cached '
                     '[00822D80] are checked as0E3F before Unit737430.'],
        substitutions=[])


def source_paths():
    # Keep this fixture's source snapshot local to its direct execution chain.
    # The dock source_paths closure imports unrelated map/LZO tooling.
    return {
        'unit_can_enter_generator': Path(__file__),
        'dock_fixture': Path('tools/spatial_oracle/refinery_dock.py'),
        'destination_fixture': Path('tools/spatial_oracle/track_destination.py'),
        'scenario_fixture': Path('tools/spatial_oracle/unit_source_scatter.py'),
        'unit_inputs': Path('tools/spatial_oracle/unit_scatter_state.py'),
        'entry_inputs': Path('tools/spatial_oracle/unit_entry.py'),
        'map_queries': Path('tools/spatial_oracle/map_queries.py'),
        'native_oracle': Path('tools/native_oracle.py'),
    }


def cargo_total_size_cases():
    """Seven conversion-sensitive controls represented as exact binary64 bits."""
    return [
        dict(name='single_1_25', size_f64_bits=['3FF4000000000000']),
        dict(name='single_2_8', size_f64_bits=['4006666666666666']),
        dict(name='two_1_5', size_f64_bits=['3FF8000000000000', '3FF8000000000000']),
        dict(name='negative_then_positive', size_f64_bits=['BFE0000000000000', '3FF8000000000000']),
        dict(name='positive_then_negative', size_f64_bits=['3FF8000000000000', 'BFE0000000000000']),
        dict(name='positive_ftol_overflow', size_f64_bits=['41E0000000000000']),
        dict(name='negative_ftol_overflow', size_f64_bits=['C1E0000000200000']),
    ]


def cargo_total_size(case):
    """Execute original CargoTotalSize and each reached per-passenger FTOL."""
    u, call, _read32 = make_dock_fixture(dict(seed=31, linked=False, mission='guard'))
    startup_fpcw = _establish_winmain_fpcw(u)
    sizes = [struct.unpack('<d', struct.pack('<Q', int(bits, 16)))[0]
             for bits in case['size_f64_bits']]
    nodes = [EXTRA + 0x23000 + index * 0x1000 for index in range(len(sizes))]
    u.mem_write(ACTOR + 0x114, dwords(len(nodes), nodes[0] if nodes else 0))
    for index, (node, size) in enumerate(zip(nodes, sizes)):
        u.mem_write(node, dwords(0x7EB058))
        u.mem_write(node + 0x6C0, dwords(node + 0x800))
        u.mem_write(node + 0x800 + 0x380, struct.pack('<d', size))
        u.mem_write(node + 0x30, dwords(nodes[index + 1] if index + 1 < len(nodes) else 0))
    entries = []
    watch = {0x473460, 0x7C5F00}
    hook = u.hook_add(UC_HOOK_CODE, lambda _u, pc, _size, _data:
                      entries.append(f'{pc:08X}') if pc in watch else None)
    code = hashlib.sha256(bytes(u.mem_read(0x401000, 0x3E0000))).hexdigest()
    call(0x473460, ACTOR + 0x114, [])
    u.hook_del(hook)
    assert code == hashlib.sha256(bytes(u.mem_read(0x401000, 0x3E0000))).hexdigest()
    raw_eax = u.reg_read(UC_X86_REG_EAX) & 0xFFFFFFFF
    total_size = struct.unpack('<i', struct.pack('<I', raw_eax))[0]
    expected_entries = [f'{0x473460:08X}', *[f'{0x7C5F00:08X}' for _ in sizes]]
    assert entries == expected_entries
    return dict(input=case, total_size_i32=total_size, eax_hex=f'{raw_eax:08X}',
                original_entries=entries, startup_fpcw=startup_fpcw,
                fpcw_after=f'{u.reg_read(UC_X86_REG_FPCW):04X}',
                fpcw_cache_after=f'{struct.unpack("<H", u.mem_read(0x00822D80, 2))[0]:04X}',
                original_text_unchanged=True)


def cargo_total_size_metadata():
    return provenance(
        scope='Seven original CargoTotalSize 0x473460 controls with per-passenger double-to-int FTOL at 0x7C5F00',
        assumptions=['Each passenger current-type Size qword at TechnoType+0x380 is supplied by exact binary64 bits; these are consumer controls, not retail INI parser outputs.',
                     'The original WinMain controlfp/cache fragment 0x006BBFB7..0x006BBFCE runs from initial 0x027F; live and cached FPCW are checked as 0x0E3F before CargoTotalSize.',
                     'The original linked-list owner and current-type vtable dispatch execute; only object links/type Size fields are supplied. Constructor, ReadINI, boarding lifecycle and transport admission are excluded.'],
        substitutions=[],
        entry_points={'winmain_fpcw': 0x6BBFB7, 'cargo_total_size': 0x473460, 'ftol': 0x7C5F00})


if __name__ == '__main__':
    import sys
    if '--cargo-total-size' in sys.argv[1:]:
        finish_vectors(lambda: dict(rows=[cargo_total_size(case) for case in cargo_total_size_cases()]),
                       Path(__file__).with_name('cargo_total_size.json'),
                       argv=[arg for arg in sys.argv[1:] if arg != '--cargo-total-size'],
                       provenance=cargo_total_size_metadata, source_paths=source_paths())
    else:
        finish_vectors(lambda: dict(rows=[unit_can_enter(case) for case in unit_can_enter_cases()]),
                       Path(__file__).with_suffix('.json'),
                       provenance=unit_can_enter_metadata, source_paths=source_paths())
