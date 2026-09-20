"""Original DistributedFire selection and retained-list expiry corpus.

Acceptance (established before implementation): execute original 709550, vector
construct/grow/copy/clear/find and CRT wrappers with only external OS allocation,
free, lock and copied-owner setter callbacks. Cover empty/all-seen/subset/stale/
duplicate histories, ordered signed scores/ties, growth beyond ten, and explicit
callback mutations. Observe original 707BCE..707CA4 first-occurrence removals.
No original instruction/vtable patch, decision hook, Rust/Cargo/Ghidra write.
Exclude candidate collection, FireAt, full lifecycle/save and AI recovery.

    python -m tools.spatial_oracle.distributed_fire --check
"""
from itertools import product
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, load_image,
    provenance, run_checked,
)
from tools.spatial_oracle.map_queries import dwords

OWNER, VTABLE = SCRATCH + 0x1000, SCRATCH + 0x2000
SCORES, CANDIDATES, HISTORY = SCRATCH + 0x3000, SCRATCH + 0x4000, SCRATCH + 0x5000
SETTER, ALLOC, FREE, LOCK = [SCRATCH + n for n in (0x6000, 0x7000, 0x8000, 0x9000)]
ALLOC_COUNT, HEAP_CURSOR = SCRATCH + 0xA000, SCRATCH + 0xA004
HEAP, HEAP_SIZE = 0x20100000, 0x100000
SP = STACK_BASE + STACK_SIZE - 0x1000
A, B, C, D = [SCRATCH + n for n in (0xB000, 0xB100, 0xB200, 0xB300)]
ENTRY, EXPIRY, EXPIRY_STOP = 0x709550, 0x707BCE, 0x707CA4
NATIVE_VTABLE = 0x7F5C70
RANGES = ((ENTRY, 0x7097F1), (EXPIRY, EXPIRY_STOP),
          (0x4E0E80, 0x4E0ECA), (0x477BE0, 0x477C2A),
          (0x4E0190, 0x4E01BA), (0x4E04A0, 0x4E0576),
          (0x477E10, 0x477EC0), (0x7C8E17, 0x7C8E25),
          (0x7C8B3D, 0x7C8B48), (0x7C93E8, 0x7C9430),
          (0x7C9442, 0x7C94BC), (0x7CD9F5, 0x7CDA6B),
          (0x7CF7BD, 0x7CF7E8))
VECTORS = {'scores': (0x440, SCORES), 'candidates': (0x458, CANDIDATES),
           'history': (0x470, HISTORY)}
TABLES = ((NATIVE_VTABLE, 0x600), (0x7E91EC, 0x20), (0x7E920C, 0x20),
          (0x7E4E78, 0x20), (0x7E4DB8, 0x20))
SAVED = ((UC_X86_REG_EBX, 0x13579BDF), (UC_X86_REG_EBP, 0x2468ACE0),
         (UC_X86_REG_ESI, 0x31415926), (UC_X86_REG_EDI, 0x27182818))


def u32(u, a):
    return struct.unpack('<I', u.mem_read(a, 4))[0]


def signed(value):
    return struct.unpack('<i', dwords(value))[0]


def snapshot(u):
    out = {'current_target': u32(u, OWNER + 0x2B4)}
    for name, (offset, _) in VECTORS.items():
        base = OWNER + offset
        pointer, capacity, count = u32(u, base + 4), u32(u, base + 8), u32(u, base + 16)
        assert 0 <= count <= 64 and 0 <= capacity <= 64
        values = [u32(u, pointer + i * 4) for i in range(count)]
        out[name] = dict(pointer=pointer, capacity=capacity, count=count,
                         owned=u.mem_read(base + 13, 1)[0],
                         values=[signed(v) for v in values] if name == 'scores' else values)
    return out


def stores(writes):
    return b''.join(b'\xC7\x05' + dwords(address, value) for address, value in writes)


def execute(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(HEAP, HEAP_SIZE)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)
    original = {(a, b): bytes(u.mem_read(a, b-a)) for a, b in RANGES}
    tables = {(a, n): bytes(u.mem_read(a, n)) for a, n in TABLES}
    u.mem_write(VTABLE, tables[(NATIVE_VTABLE, 0x600)])
    u.mem_write(VTABLE + 0x3C8, dwords(SETTER))
    u.mem_write(OWNER, dwords(VTABLE))
    u.mem_write(OWNER + 0x2B4, dwords(D))
    for name, (offset, pointer) in VECTORS.items():
        values = row[name]
        table = 0x7E4E78 if name == 'scores' else 0x7E91EC
        u.mem_write(OWNER + offset, dwords(table, pointer, 64) + b'\x01\x01\x00\x00' + dwords(len(values), 10))
        u.mem_write(pointer, dwords(*values, *([0xDEADC0DE] * (64-len(values)))))
    # Native malloc takes ordinary HeapAlloc path; native free sees no small-block
    # heaps. These CRT globals and initialized lock are supplied loader state.
    for address, value in ((0x87C584, 0), (0xB78B94, 0), (0xB78B98, 0),
                           (0xB78B9C, 0x12340000), (0x87C2A8 + 9*4, SCRATCH + 0xA100)):
        u.mem_write(address, dwords(value))
    for slot, receiver in ((0x7E1278, ALLOC), (0x7E127C, FREE),
                           (0x7E11E8, LOCK), (0x7E11EC, LOCK)):
        u.mem_write(slot, dwords(receiver))
    u.mem_write(HEAP_CURSOR, dwords(HEAP))
    # Supplied HeapAlloc bump arena. Scripted first-call mutations are boundary
    # stress tests, not claims that Windows HeapAlloc ordinarily changes an actor.
    alloc = bytearray(b'\xA1' + dwords(ALLOC_COUNT) + b'\xFF\x05' + dwords(ALLOC_COUNT))
    mutations = stores(row.get('allocation_writes', []))
    if mutations:
        alloc += b'\x85\xC0\x0F\x85' + dwords(len(mutations)) + mutations
    alloc += b'\xA1' + dwords(HEAP_CURSOR) + b'\x81\x05' + dwords(HEAP_CURSOR, 0x1000) + b'\xC2\x0C\x00'
    u.mem_write(ALLOC, bytes(alloc))
    u.mem_write(FREE, b'\xB8\x01\x00\x00\x00\xC2\x0C\x00')
    u.mem_write(LOCK, b'\xC2\x04\x00')
    setter = b'\x8B\x44\x24\x04\x89\x81\xB4\x02\x00\x00'
    setter += stores(row.get('setter_writes', [])) + b'\xB8\x85\x00\xCE\xFA\xC2\x04\x00'
    u.mem_write(SETTER, setter)
    events, writes, visits, temporary = [], [], {}, []
    names = {SETTER: ('assign_target', 1), ALLOC: ('HeapAlloc', 3), FREE: ('HeapFree', 3), LOCK: ('critical_section', 1)}
    entries = {a for a, _ in RANGES} | {0x4E0550, 0x70963D, 0x709741, 0x709777, 0x7095C1, 0x707C0C, 0x707C52, 0x707C92}
    pending = []

    def observe(cpu, pc, size, _):
        assert (any(a <= pc < b for a, b in RANGES)
                or any(a <= pc < a+0x1000 for a in (SETTER, ALLOC, FREE, LOCK))), hex(pc)
        if pc in entries:
            visits[f'{pc:08X}'] = visits.get(f'{pc:08X}', 0) + 1
        if pc == 0x709714:
            sp = cpu.reg_read(UC_X86_REG_ESP)
            temp = {}
            for label, offset in (('scores', 0x18), ('candidates', 0x30)):
                ptr = u32(cpu, sp+offset+4)
                count = u32(cpu, sp+offset+16)
                values = [u32(cpu, ptr+i*4) for i in range(count)]
                temp[label] = [signed(v) for v in values] if label == 'scores' else values
            temporary.append(temp)
        if pending and pc == pending[-1][0]:
            _, event = pending.pop()
            event['after'] = snapshot(cpu)
            event['return_eax'] = cpu.reg_read(UC_X86_REG_EAX)
        if pc in names:
            name, nargs = names[pc]
            sp = cpu.reg_read(UC_X86_REG_ESP)
            event = dict(name=name, caller=u32(cpu, sp), ecx=cpu.reg_read(UC_X86_REG_ECX),
                         args=[u32(cpu, sp+4+i*4) for i in range(nargs)], before=snapshot(cpu))
            if name == 'HeapAlloc':
                assert event['args'][0] == 0x12340000 and event['args'][1] == 0
                assert 0 < event['args'][2] <= 0x1000
            if name == 'HeapFree':
                assert event['args'][2] in (SCORES, CANDIDATES, HISTORY) or HEAP <= event['args'][2] < HEAP + HEAP_SIZE
            events.append(event)
            pending.append((u32(cpu, sp), event))

    def write(cpu, access, address, size, value, _):
        tracked = (OWNER <= address < OWNER + 0x600 or SCORES <= address < HISTORY + 0x100
                   or HEAP <= address < HEAP + HEAP_SIZE)
        if tracked:
            pc = cpu.reg_read(UC_X86_REG_EIP)
            writes.append(dict(pc=f'{pc:08X}', address=address, size=size, value=value,
                               origin='external' if SCRATCH <= pc < SCRATCH + 0x10000 else 'native'))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_WRITE, write)
    for register, value in SAVED:
        u.reg_write(register, value)
    u.reg_write(UC_X86_REG_ESP, SP)
    u.mem_write(SP, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ECX, OWNER)
    u.reg_write(UC_X86_REG_EAX, 0xA5B6C75E)
    before = snapshot(u)
    if row['entry'] == 'select':
        run_checked(u, ENTRY, RET_MAGIC)
        assert u.reg_read(UC_X86_REG_ESP) == SP + 4
        assert all(u.reg_read(r) == v for r, v in SAVED)
    else:
        u.reg_write(UC_X86_REG_ESI, OWNER)
        u.reg_write(UC_X86_REG_EBP, row['expired'])
        u.reg_write(UC_X86_REG_EBX, 0)
        run_checked(u, EXPIRY, EXPIRY_STOP)
        assert u.reg_read(UC_X86_REG_ESP) == SP
    assert not pending
    assert all(bytes(u.mem_read(a, b-a)) == code for (a, b), code in original.items())
    assert all(bytes(u.mem_read(a, n)) == code for (a, n), code in tables.items())
    assert u32(u, HEAP_CURSOR) < HEAP + HEAP_SIZE
    return dict(input=row, before=before, after=snapshot(u), events=events, writes=writes,
                native_visits=visits, temporary_before_selection=temporary, eax=u.reg_read(UC_X86_REG_EAX),
                backing_after={name: bytes(u.mem_read(pointer, max(4, len(row[name])*4))).hex()
                               for name, (_, pointer) in VECTORS.items()})


def cases():
    rows = []
    def add(name, candidates, scores, history, entry='select', **extra):
        assert len(candidates) == len(scores)
        rows.append(dict(name=name, entry=entry, candidates=candidates, scores=scores,
                         history=history, **extra))
    for history in ([], [A], [A, A, B], [D, A, D]):
        add(f'empty_{len(rows)}', [], [], history)
    histories = ([], [A], [B], [C], [A, B], [C, A], [A, B, C],
                 [C, B, A], [D], [D, A, D], [A, A], [A, D, B, D, A],
                 [A, B, C, A, B, C])
    scoresets = ([1, 2, 3], [3, 2, 1], [5, 5, 5], [0, 0, 0],
                 [-1, -2, -3], [0, -1, 1], [-2147483648, 2147483647, 2147483647])
    for history, scores in product(histories, scoresets):
        add(f'history_score_{len(rows)}', [A, B, C], scores, history)
    for candidates, scores in (([A, B, A], [1, 2, 9]), ([A, A, B], [9, 1, 2]),
                               ([C, B, A], [5, 5, 5]), ([0, A, B], [9, 8, 7])):
        for history in ([], [A], [B], [A, B], [A, A, D]):
            add(f'duplicate_order_{len(rows)}', candidates, scores, history)
    for count in (10, 11, 12, 21, 23):
        candidates = [A + i*16 for i in range(count)]
        add(f'growth_{count}', candidates, list(range(count)), [candidates[-1]])
    for candidates, history in (([], []), ([A, B, C], []), ([A, B, C], [D, A, A, B]),
                                ([A, B, A, C, A], [A, D, A, B, A]),
                                ([B, A, C], [B, A, C])):
        for expired in (0, A, B, C, D):
            add(f'expiry_{len(rows)}', candidates, list(range(1,len(candidates)+1)),
                history, entry='expiry', expired=expired)
    for history in ([], [A], [A, B, C]):
        add(f'setter_mutation_{len(rows)}', [A, B, C], [3, 2, 1], history,
            setter_writes=[[OWNER+0x2B4, D], [OWNER+0x474, HISTORY], [OWNER+0x478, 64], [OWNER+0x480, 1], [HISTORY, B]])
    add('allocation_changes_current_and_next_score', [A, B, C], [1, 2, 3], [A],
        allocation_writes=[[SCORES+4, 50], [SCORES+8, 99]])
    add('allocation_changes_later_candidate', [A, B, C], [1, 2, 3], [A],
        allocation_writes=[[CANDIDATES+8, D]])
    add('allocation_clears_live_history_count_cached_count_remains', [A, B, C], [1, 2, 3], [A],
        allocation_writes=[[OWNER+0x480, 0]])
    return rows


def generate():
    inputs = cases()
    assert len({r['name'] for r in inputs}) == len(inputs)
    rows = [execute(row) for row in inputs]
    return dict(schema_version=1, row_count=len(rows), pointers=dict(owner=OWNER,a=A,b=B,c=C,d=D), rows=rows)


def metadata():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    result = provenance(
        scope='Original DistributedFire709550 selection/history/vector operations and bounded707BCE..707CA4 pointer-expiry loops',
        assumptions=[
            'Acceptance is recorded in module docstring before implementation. Inputs use equal candidate/score counts, valid native vector headers, capacity64, owned fixture backing, and identity sentinels which selection does not dereference.',
            'Full709550 executes unchanged with native constructors4E0E80/477BE0, grows4E04A0/477E10, clear4E0190 and original CRT new/delete/malloc/free/locking wrappers. Native vector vtables remain untouched. Owner uses copied retail Unit vtable with only AssignTarget slot replaced.',
            'Initialized CRT loader state supplies no small-block heaps, allocation small-block threshold0, a nonnull ordinary heap handle and initialized lock9. External HeapAlloc returns fresh4096-byte arena chunks; all requested lengths are observed. External HeapFree succeeds without reclaiming memory; locks are noops. OOM, small-block allocation and multithreading are not covered.',
            'Allocation/setter mutation rows are explicit callback-boundary stress probes, not asserted production behavior of Windows or native AssignTarget. Setter normally stores its argument at live target+2B4, then applies declared writes; its other effects are excluded.',
            'Original native hooks are observation only. Code and retail vtable bytes are checked unchanged. Results retain callback arguments, before/after vectors, selected-pointer writes, native removal writes, raw backing arrays, original visit counts and raw EAX (not a boolean result).',
            'Expiry slice supplies ESI owner, EBP expired pointer and EBX0 established by the enclosing7077C0 prologue. It stops before707CA4 epilogue; earlier lifecycle logic and callers are excluded. Original4E0550 find runs; native loops remove only first matching candidate/parallel score and first history occurrence.',
            'Rows cover empty/history absent/all seen/subsets/stale/duplicates/null sentinel, signed nonpositive and extreme scores, first ties, candidate order, native temporary growth beyond ten, and live memory mutations. This is finite comparison evidence, not exhaustive input or production reachability proof.',
            'GreatestThreat candidate production, scanner dispatch, actual target setter, FireAt history append/mission exception, enclosing PointerExpired, save/hash/backing serialization, AI estimate recovery and Rust parity are not executed or claimed.',
        ],
        substitutions=[
            'Only fixture copied owner+3C8 points at external setter; imported HeapAlloc7E1278, HeapFree7E127C, EnterCriticalSection7E11E8 and LeaveCriticalSection7E11EC point at external x86 stubs. Native executable instructions and retail vtables are never patched.',
            'External callbacks execute declared machine-code mutations and ABI returns. No hook changes memory/registers/control flow.',
        ],
        entry_points=dict(select=ENTRY, expiry_slice=EXPIRY, expiry_stop=EXPIRY_STOP,
                          pointer_find=0x4E0550, pointer_clear=0x4E0190))
    result['row_counts'] = {entry:sum(r['entry']==entry for r in cases()) for entry in ('select','expiry')}
    result['original_code'] = {f'{a:08X}..{b:08X}':dict(hex=bytes(u.mem_read(a,b-a)).hex(),
                                sha256=hashlib.sha256(bytes(u.mem_read(a,b-a))).hexdigest()) for a,b in RANGES}
    result['original_tables'] = {f'{a:08X}':bytes(u.mem_read(a,n)).hex() for a,n in TABLES}
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
