"""Original Walk prospective-coordinate/height caller joined to Infantry entry.

The interior Walk frame, actor/type and cells are supplied. Original49F3A0
initializes direction vectors;75B59C..75B696 produces query arguments and runs
concrete51BF90. Stop before head selection/refusal response; no full route claim.
"""
from pathlib import Path
import argparse, hashlib, json, struct
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ
from unicorn.x86_const import *
from tools.native_oracle import run_checked, provenance, finish_vectors
from tools.spatial_oracle.astar_capture_neighbor import prepare, synthetic_cases, digest
from tools.spatial_oracle.building_body_rules import SP, dwords


def execute(row):
    m, actor, cells, _, layers = prepare(row)
    u = m.u
    m.invoke(0x49F3A0, 0)
    loco = m.alloc(0x80)
    m.invoke(0x75AA90, loco)
    u.mem_write(loco + 0xC, dwords(actor))
    stack = SP - 0x1000
    u.mem_write(stack, bytes(0x80))
    u.mem_write(stack + 0x14, dwords(row['direction']))
    u.reg_write(UC_X86_REG_ESP, stack)
    u.reg_write(UC_X86_REG_EBP, loco)
    u.reg_write(UC_X86_REG_EAX, actor)
    queries, entry, reached, owner_reads = [], [], [], []
    original = [(a, bytes(u.mem_read(a, b - a))) for a, b in
                ((0x75B59C, 0x75B6A0), (0x51BF90, 0x51C890), (0x5F5F00, 0x5F5F40))]

    def observe(_u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc in (0x7C8E17, 0x7C8B3D, 0x7D140B, 0x5B40B0, 0x65C780, 0x65C7E0):
            raise AssertionError(('unexpected measured seam/RNG', hex(pc)))
        if pc in (0x565730, 0x5657A0):
            ptr = m.read32(sp + 4)
            fmt = '<3i' if pc == 0x565730 else '<2h'
            queries.append(dict(callee=hex(pc), caller=hex(m.read32(sp)),
                                coordinate=list(struct.unpack(fmt, u.mem_read(ptr, struct.calcsize(fmt))))))
        if pc in (0x5F5F00, 0x5F6960, 0x51BF90, 0x4D9C60):
            reached.append(hex(pc))
        if pc == 0x51BF90:
            candidate, direction, height, previous, flag = struct.unpack('<5I', u.mem_read(sp + 4, 20))
            assert candidate == cells[tuple(row['candidate'])]
            entry.append(dict(candidate=list(struct.unpack('<2h', u.mem_read(candidate + 0x24, 4))),
                              direction=struct.unpack('<i', dwords(direction))[0],
                              height=struct.unpack('<i', dwords(height))[0],
                              previous_null=previous == 0, flag=flag))

    def observe_owner_read(_u, _access, address, size, _value, _data):
        for at, cell in cells.items():
            if address < cell + 0x5C and address + size > cell + 0x54:
                owner_reads.append(dict(cell=at, offset=address-cell, size=size,
                                        pc=hex(u.reg_read(UC_X86_REG_EIP))))

    hook = u.hook_add(UC_HOOK_CODE, observe)
    read_hook = u.hook_add(UC_HOOK_MEM_READ, observe_owner_read)
    try:
        endpoint = run_checked(u, 0x75B59C, (0x75BC13, 0x75B6A0), count=200000,
                               required_addresses=(0x5F5F00, 0x5F6960, 0x51BF90))
    finally:
        u.hook_del(hook)
        u.hook_del(read_hook)
    assert len(entry) == 1 and entry[0]['previous_null'] and entry[0]['flag'] == 1
    assert u.reg_read(UC_X86_REG_ESP) == stack
    assert all(bytes(u.mem_read(a, len(raw))) == raw for a, raw in original)
    projected_owners = {tuple(cell['coord']) for cell in row['cells']
                        if cell['occupation_owners'] != [-1, -1]}
    assert not any(tuple(read['cell']) in projected_owners for read in owner_reads), (
        "Captured source owner identity is not a native House construction", owner_reads)
    return dict(input=row, input_sha256=digest(row), native_entry_arguments=entry,
                can_enter_class=u.reg_read(UC_X86_REG_ESI),
                outcome='head_selection' if endpoint == 0x75BC13 else 'refusal_response',
                boundary=hex(endpoint), prospective_xyz=list(struct.unpack('<3i', u.mem_read(stack + 0x3C, 12))),
                bridge_mismatch=bool(u.mem_read(actor + 0x68B, 1)[0]),
                queries=queries, reached=reached, code_unchanged=True,
                occupation_owner_reads=owner_reads,
                native_layers=layers, measured_substitutions=[])


def generate(input_path=None):
    rows = json.loads(input_path.read_text())['cases'] if input_path else [
        row for row in synthetic_cases()
        if row['name'] in ('synthetic_flat_capture', 'synthetic_iron_curtain_control')]
    return dict(schema_version=1, scope=__doc__,
                source=input_path.name if input_path else 'explicit synthetic controls',
                source_sha256=hashlib.sha256(input_path.read_bytes()).hexdigest() if input_path else None,
                cases=[execute(row) for row in rows])


def metadata():
    return provenance(scope=__doc__, entry_points={
        'direction_initializer': 0x49F3A0, 'walk_constructor': 0x75AA90,
        'prospective_caller': 0x75B59C, 'height_reader': 0x5F5F00,
        'object_get_cell': 0x5F6960, 'infantry_entry': 0x51BF90,
        'head_boundary': 0x75BC13, 'refusal_boundary': 0x75B6A0}, assumptions=[
        'Shared astar_capture_neighbor preparation supplies labelled actor/type/cells and executes original physical land-speed layer readers. Sparse map, object storage, mission/NavCom and type scalars are projected inputs, not native full constructors.',
        'Original Walk constructor and direction-vector initializer execute. Interior frame supplies only the receiver and retained direction; original prospective coordinate, truncation, Map query, signed height and concrete Infantry admission bodies execute unchanged.',
        'Stops before head selection or the first refusal callback. No full route, paid movement, repair, RNG, sound, lifecycle or persistence equivalence is claimed.'], substitutions=[
        'Preparation INI reader inherits bounded allocation/free/TLS/file seams. No such seam or RNG entry is reached in the measured Walk caller/Infantry corridor.'])


if __name__ == '__main__':
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--input', type=Path)
    args, remaining = parser.parse_known_args()
    finish_vectors(lambda: generate(args.input), Path(__file__).with_suffix('.json'),
                   provenance=metadata, argv=remaining)
