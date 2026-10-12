"""Bounded Unit transport CAN_ENTER query, using the existing dock VM owner.

python -m tools.spatial_oracle.unit_transport_can_enter --check

Supplied integer type/cargo/alliance states; original Unit receiver and all
reached admission/size leaves execute. No copied fixture or native controller.
"""
import hashlib
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.refinery_dock import make_dock_fixture, cell, BLD, OTHER, source_paths as dock_source_paths
from tools.spatial_oracle.unit_scatter_state import ACTOR, TYPE
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
    ]
    return [dict(base, name=name, **changes) for name, changes in controls]


def unit_can_enter(case):
    """Execute untouched Unit Receive_Radio0F and reached query leaves.

    Inherits existing Drive/Cell/scenario construction. The sender/cargo live
    Infantry fields, House indexes/alliance and type sizes are supplied inputs;
    no radio/admission/size-sum/ftol result is substituted.
    """
    u, call, read32 = make_dock_fixture(dict(seed=31, linked=False, mission='guard'))
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
                scenario_rng_before=before.hex(), scenario_rng_unchanged=True,
                original_text_and_vtables_unchanged=True)


def unit_can_enter_metadata():
    return provenance(
        scope='Bounded Unit737430 Receive_Radio0F from supplied live transport/passenger state; '
              'integer cargo Size accumulation and capacity/SizeLimit comparisons execute.',
        entry_points={'unit_receive': 0x737430, 'is_ally_by_object': 0x4F9A90,
                      'ground_cell_building': 0x47C520, 'is_mind_controlled': 0x7105E0,
                      'is_controlling': 0x4722C0, 'cargo_total_size': 0x473460,
                      'ftol': 0x7C5F00},
        assumptions=['Reuses refinery_dock actual Drive/Cell/scenario fixture and unchanged '
                     'Unit/Infantry vtables. House indexes/one-way alliance, live sender flags, '
                     'cargo links and integer Size/Passengers/SizeLimit fields are supplied, '
                     'not retail-reader receipts. Constructor/read/physical arrival/radio '
                     'transmit and passenger attachment lifecycles are excluded.',
                     'Actual Unit receiver0F, alliance/warp/GetCell/building/mind-control/'
                     'controller/type queries and cargo linked-list sum/ftol execute without '
                     'supplied query replies. Fractional/NaN/overflow Size values are excluded; '
                     'these rows bound the integer Rust representation. Scenario RNG and '
                     'original text remain unchanged.'],
        substitutions=[])


def source_paths():
    return dock_source_paths() | {'unit_can_enter_generator': Path(__file__)}


if __name__ == '__main__':
    finish_vectors(lambda: dict(rows=[unit_can_enter(case) for case in unit_can_enter_cases()]),
                   Path(__file__).with_suffix('.json'),
                   provenance=unit_can_enter_metadata, source_paths=source_paths())
