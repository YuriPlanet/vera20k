"""Bounded original producer -> restamp -> empty-cell admission probe.

Run from repository root: python -m tools.spatial_oracle.bridge_restamp_retail --check.
Input .local/restamp-deadman-export.json is exported by the ignored Rust test
with VERA20K_RESTAMP_MAPS=Deadman.mmx and VERA20K_RESTAMP_OUTPUT naming that file.
This does not prove the preceding native loader. Native records remain in place.
"""
import json
import hashlib
from pathlib import Path
import struct
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import load_image, run_checked, STACK_BASE, STACK_SIZE, RET_MAGIC, provenance
from tools.spatial_oracle.map_queries import packed, dwords

ROOT = Path('.local').resolve()
case = json.loads((ROOT / 'restamp-deadman-export.json').read_text(encoding='utf-8'))[0]
assert case['map'] == 'Deadman.mmx'
assert not case['tubes'], 'Tube registry setup is outside this empty-registry witness'
MAP, TABLE, RECORDS, DUMMY = 0x87F7E8, 0xC00000, 0xB90000, 0xABDC50
CELLS, TYPES, TYPE_TABLE = 0x40000000, 0x60000000, 0x61000000
uc = Uc(UC_ARCH_X86, UC_MODE_32)
load_image(uc)
for address, size in [(STACK_BASE, STACK_SIZE), (RET_MAGIC, 0x1000),
                      (CELLS, 0x2000000), (TYPES, 0x100000), (TYPE_TABLE, 0x10000)]:
    uc.mem_map(address, size)
sp = STACK_BASE + STACK_SIZE - 0x1000
def call(address, this=MAP, args=(), count=1000000, required=()):
    uc.mem_write(sp, dwords(RET_MAGIC, *args))
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_ECX, this)
    run_checked(uc, address, RET_MAGIC, count=count, required_addresses=list(required))
    return uc.reg_read(UC_X86_REG_EAX)
call(0x49F2F0, count=100)
uc.mem_write(MAP + 0xF4, dwords(*case['size']))
uc.mem_write(MAP + 0xFC, dwords(*case['local_size']))
uc.mem_write(MAP + 0x13C, dwords(TABLE, 0x40000))
uc.mem_write(TABLE, bytes(0x100000))
ptrs, before = {}, {}
for index, row in enumerate(case['native_cells']):
    x, y, tile, sub, flags, land, tube, level, slope = row
    ptr = CELLS + index * 0x200
    ptrs[x, y] = ptr
    before[x, y] = flags
    uc.mem_write(ptr + 0x24, packed(x, y))
    uc.mem_write(ptr + 0x38, dwords(tile))
    uc.mem_write(ptr + 0x44, dwords(-1))  # Consumer only uses verified empty witness.
    uc.mem_write(ptr + 0xEC, dwords(land))
    uc.mem_write(ptr + 0x116, struct.pack('<H', tube & 65535))
    uc.mem_write(ptr + 0x11A, bytes((sub, level, slope)))
    uc.mem_write(ptr + 0x140, dwords(flags))
    uc.mem_write(TABLE + (y * 512 + x) * 4, dwords(ptr))
uc.mem_write(0xAA0E28, dwords(case['bridge_bases'][0]))
uc.mem_write(0xABAD1C, dwords(case['bridge_bases'][1]))
uc.mem_write(DUMMY, bytes(0x200))
uc.mem_write(DUMMY + 0x24, packed(1234, -2345))
uc.mem_write(DUMMY + 0x38, dwords(65535))
uc.mem_write(DUMMY + 0x116, b'\xff\xff')
uc.mem_write(0x8B4148, dwords(0))
uc.mem_write(MAP + 0x50, dwords(0x7ED4C0, 0, 0, 0, 7, 10))
uc.mem_write(sp, dwords(RET_MAGIC))
uc.reg_write(UC_X86_REG_ESP, sp)
uc.reg_write(UC_X86_REG_ECX, MAP)
run_checked(uc, 0x56D6E0, 0x56D6F7, count=100, required_addresses=[0x588CE0])
assert struct.unpack('<I', uc.mem_read(MAP + 0x60, 4))[0] == 0
uc.mem_write(MAP + 0x54, dwords(RECORDS, 256))
run_checked(uc, 0x56D6F7, RET_MAGIC, count=10000000, required_addresses=[0x578290])
count = struct.unpack('<I', uc.mem_read(MAP + 0x60, 4))[0]
assert count < 256
record_bytes = bytes(uc.mem_read(RECORDS, count * 16))
records = []
for index in range(count):
    raw = record_bytes[index*16:(index+1)*16]
    records.append(dict(a=list(struct.unpack('<hh', raw[:4])),
                        b=list(struct.unpack('<hh', raw[4:8])), active=bool(raw[8]),
                        kind=struct.unpack('<I', raw[12:])[0]))
expected = [dict(a=r['endpoint_a'], b=r['endpoint_b'], active=r['active'],
                 kind=0 if r['bridge_kind'] == 'High' else 1) for r in case['records']]
assert records == expected, (records, expected)

witness = next(c['cell'] for c in case['affected_cells'] if c['coord'] == [57, 42])
assert witness['overlay_id'] is None and witness['techno_occupants'] == 0 and not witness['terrain_object']
assert witness['live_new_tiberium_admission']
uc.mem_write(0xA8E9A0, b'\x01')
for land, value in enumerate(case['land_buildable']):
    assert value is not None
    uc.mem_write(0x89EA60 + land * 0x24, bytes((int(value),)))
uc.mem_write(0xA8ED38, dwords(len(case['tile_allows_tiberium'])))
uc.mem_write(0xA8ED2C, dwords(TYPE_TABLE))
for index, allowed in enumerate(case['tile_allows_tiberium']):
    ptr = TYPES + index * 0x400
    uc.mem_write(TYPE_TABLE + index * 4, dwords(ptr))
    uc.mem_write(ptr + 0x306, bytes((int(allowed),)))
admission_before = call(0x4838E0, ptrs[57, 42], (0,), required=(0x578460,)) & 255
address_to_coord = {ptr + 0x140: coord for coord, ptr in ptrs.items()}
writes = []
def observe(machine, access, address, size, value, user):
    base = address if address in address_to_coord else address - 1
    if base in address_to_coord and address in (base, base + 1):
        writes.append(dict(coord=address_to_coord[base], offset=address-base, size=size, value=value))
uc.hook_add(UC_HOOK_MEM_WRITE, observe)
call(0x586BF0, count=1000000)
assert bytes(uc.mem_read(RECORDS, count * 16)) == record_bytes, 'records retained unchanged'
admission_after = call(0x4838E0, ptrs[57, 42], (0,), required=(0x578460,)) & 255
changes = []
for coord, ptr in ptrs.items():
    flags = struct.unpack('<I', uc.mem_read(ptr + 0x140, 4))[0]
    if flags != before[coord]:
        changes.append(dict(coord=coord, before=before[coord], after=flags))
result = dict(map=case['map'], records=records, record_bytes=record_bytes.hex(),
              source_export_sha256=hashlib.sha256((ROOT/'restamp-deadman-export.json').read_bytes()).hexdigest(),
              map_sha256='8d17f1937e7215ae2bd210506640172e954cc00efa2e620a64717b4da132890d',
              changes=changes, writes=writes, witness=[57,42],
              rust_admission=witness['live_new_tiberium_admission'],
              native_admission_before=admission_before, native_admission_after=admission_after,
              dummy_coord=struct.unpack('<hh', uc.mem_read(DUMMY+0x24,4)),
              provenance=provenance(scope='Original producer to gap restamp and empty-cell tiberium admission',
                  assumptions=['Rust retail-loaded cell/type/land scalar inputs supplied; native loader not executed',
                               'Native connectivity/all-zone-level calls between producer and restamp in684C30 are omitted; this is bounded leaf composition, not uninterrupted fresh-load execution',
                               'Native record clear executes; adequate record storage supplied after clear',
                               'Empty registry and empty witness object list; non-witness overlays are unused',
                               'Map hash is bound to separate actual-file receipt restamp-retail-map-hashes.json; this harness hashes the scalar export only',
                               'No native code patches or substituted returns; source Size/LocalSize supplied'],
                  substitutions=['Adequate native record vector backing supplied after original clear'],
                  entry_points={'producer':0x56D6E0,'restamp':0x586BF0,'admission':0x4838E0}))
if '--check' in sys.argv:
    expected = json.loads(Path(__file__).with_suffix('.json').read_text(encoding='utf-8'))
    assert json.loads(json.dumps(result)) == expected, 'native comparison differs from saved receipt'
else:
    (ROOT / 'restamp-native-deadman.json').write_text(json.dumps(result, indent=2)+'\n', encoding='utf-8')
print(json.dumps({k:result[k] for k in ['map','records','witness','rust_admission','native_admission_before','native_admission_after']}))
print(f'changed_cells={len(changes)} native_writes={len(writes)}')
