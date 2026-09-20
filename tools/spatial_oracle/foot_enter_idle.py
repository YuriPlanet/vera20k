"""Bounded original Foot EnterIdle4D82B0, including live queue/callback order.

The complete Foot body and original END QueryInterface helper execute. Techno
base, Scatter, destination/speed setters and COM interfaces are declared fixture
callbacks. Team/legacy-planning and Infantry archive branches are excluded; this
does not establish complete Unit/Infantry/Aircraft virtual receiver parity.
"""
from itertools import product
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDI,
    UC_X86_REG_ESI, UC_X86_REG_EBP, UC_X86_REG_ESP,
)
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, load_image,
    provenance, run_checked,
)
from tools.spatial_oracle.map_queries import dwords

ENTRY, TECHNO, QUERY_HELPER = 0x4D82B0, 0x709A40, 0x45AF20
AI_ENTRY, AI_TECHNO, AI_LIVE, AI_DEAD = 0x4DA530, 0x6F9E50, 0x4DA554, 0x4DAF00
UNIT_VTABLE = 0x7F5C70
TARGET_VTABLES = (0x7E4EEC, UNIT_VTABLE, 0x7E3EBC)
OWNER, VTABLE, LOCO, END, RESTORED = (SCRATCH + x for x in
                                      (0x1000, 0x2000, 0x3000, 0x3100, 0x3200))
LOCO_VT, END_VT, QUEUE, REPLACEMENT = (SCRATCH + x for x in
                                     (0x4000, 0x4100, 0x5000, 0x5100))
TARGETS = [SCRATCH + 0x6000 + i * 0x100 for i in range(6)]
STUBS = {name: SCRATCH + 0x8000 + i * 0x100 for i, name in enumerate(
    ('techno', 'scatter', 'destination', 'speed', 'query', 'permission',
     'loco_release', 'restore', 'end_release'))}
REENTER_RESULT = SCRATCH + 0x7000
SP = STACK_BASE + STACK_SIZE - 0x1000
CODE_RANGES = ((ENTRY, 0x4D8557), (QUERY_HELPER, 0x45AF95), (0x746E20, 0x746E26),
               (AI_ENTRY, AI_LIVE), (AI_DEAD, 0x4DAF08))
SAVED = {UC_X86_REG_EBX: 0x31415926, UC_X86_REG_ESI: 0x27182818,
         UC_X86_REG_EDI: 0x13579BDF, UC_X86_REG_EBP: 0x2468ACE0}


def u32(u, a):
    return struct.unpack('<I', u.mem_read(a, 4))[0]


def put(a, value):
    return b'\xC7\x05' + dwords(a, value)


def ret(value=0, pop=0):
    return b'\xB8' + dwords(value) + (b'\xC2' + struct.pack('<H', pop) if pop else b'\xC3')


def reenter(row):
    # Real recursive CALL of the unchanged original Foot body. Only its AL is
    # recorded; the fixture does not decide whether the native latch blocks it.
    return (b'\x68' + dwords(row['args'][1]) + b'\x68' + dwords(row['args'][0])
            + b'\xB9' + dwords(OWNER) + b'\xB8' + dwords(ENTRY) + b'\xFF\xD0'
            + b'\xA2' + dwords(REENTER_RESULT))


def queue_labels(u):
    count = u32(u, OWNER + 0x598)
    assert count <= 6
    data = u32(u, OWNER + 0x58C)
    return [TARGETS.index(u32(u, data + 4 * i)) for i in range(count)]


def state(u):
    return dict(latch=u.mem_read(OWNER + 0x6B3, 1)[0],
                scatter_pending=u.mem_read(OWNER + 0x687, 1)[0],
                locomotor=u32(u, OWNER + 0x674), queue=queue_labels(u),
                queue_storage='replacement' if u32(u, OWNER + 0x58C) == REPLACEMENT else 'original')


def execute(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(RET_MAGIC, 0x1000)
    originals = [bytes(u.mem_read(a, b-a)) for a, b in CODE_RANGES]
    original_vtable = bytes(u.mem_read(UNIT_VTABLE, 0x600))
    assert u32(u, UNIT_VTABLE + 0x2C) == 0x746E20
    assert u32(u, UNIT_VTABLE + 0x174) == 0x743A50
    assert u32(u, UNIT_VTABLE + 0x480) == 0x741970
    u.mem_write(VTABLE, original_vtable)
    for offset, name in ((0x174, 'scatter'), (0x480, 'destination'), (0x544, 'speed')):
        u.mem_write(VTABLE + offset, dwords(STUBS[name]))
    u.mem_write(OWNER, dwords(VTABLE))
    u.mem_write(LOCO, dwords(LOCO_VT))
    u.mem_write(END, dwords(END_VT))
    u.mem_write(LOCO_VT, dwords(STUBS['query'], 0, STUBS['loco_release']))
    u.mem_write(END_VT + 8, dwords(STUBS['end_release']))
    u.mem_write(END_VT + 0x10, dwords(STUBS['restore'], STUBS['permission']))
    u.mem_write(OWNER + 0x520, dwords(-1))  # Explicit legacy team/planning exclusion.
    u.mem_write(OWNER + 0x674, dwords(0 if row['end'] == 'none' else LOCO))
    u.mem_write(OWNER + 0x6B3, bytes((row.get('latch', 0),)))
    u.mem_write(OWNER + 0x687, bytes((row.get('scatter_pending', 0),)))
    u.mem_write(OWNER + 0x58C, dwords(QUEUE))
    u.mem_write(OWNER + 0x598, dwords(len(row['queue'])))
    u.mem_write(QUEUE, dwords(*(TARGETS[x] for x in row['queue'])))
    u.mem_write(REENTER_RESULT, b'\xAA')
    # Original class vtables establish Cell/Unit/Building identities. Base never
    # calls their RTTI or dereferences them; their complete object state is absent.
    for i, target in enumerate(TARGETS):
        vtable = TARGET_VTABLES[i % 3]
        rtti = u32(u, vtable + 0x2C)
        assert bytes(u.mem_read(rtti, 6)) == b'\xB8' + dwords((11, 1, 6)[i % 3]) + b'\xC3'
        u.mem_write(target, dwords(vtable))
    for name, pop in (('techno', 8), ('scatter', 12), ('destination', 8),
                      ('speed', 8), ('loco_release', 4), ('end_release', 4)):
        body = reenter(row) if row.get('reenter') == name else b''
        if name == 'destination' and 'mutation' in row:
            mutation = row['mutation']
            data = REPLACEMENT if row.get('replace_storage') else QUEUE
            body += put(OWNER + 0x58C, data) + put(OWNER + 0x598, len(mutation))
            for index, target in enumerate(mutation):
                body += put(data + index * 4, TARGETS[target])
        if name == 'techno' and 'techno_queue' in row:
            body += put(OWNER + 0x598, len(row['techno_queue']))
            for index, target in enumerate(row['techno_queue']):
                body += put(QUEUE + index * 4, TARGETS[target])
        body += ret(row.get('callback_return', 0), pop)
        u.mem_write(STUBS[name], body)
    available = row['end'] != 'unsupported'
    u.mem_write(STUBS['query'], b'\x8B\x4C\x24\x0C\xC7\x01'
                + dwords(END if available else 0) + ret(0 if available else 0x80004002, 12))
    u.mem_write(STUBS['permission'], ret(int(row['end'] == 'success'), 4))
    body = reenter(row) if row.get('reenter') == 'restore' else b''
    body += b'\x8B\x4C\x24\x08\xC7\x01' + dwords(RESTORED) + ret(0, 8)
    u.mem_write(STUBS['restore'], body)
    u.mem_write(SP, dwords(RET_MAGIC, *row['args']))
    for register, value in SAVED.items():
        u.reg_write(register, value)
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, OWNER)
    events, writes, visits = [], [], []
    by_address = {address: name for name, address in STUBS.items()}

    def observe(uc, address, _size, _data):
        if address in (ENTRY, QUERY_HELPER, 0x746E20):
            visits.append(f'{address:08X}')
        if address not in by_address:
            return
        name = by_address[address]
        sp = uc.reg_read(UC_X86_REG_ESP)
        args_count = {'techno': 2, 'scatter': 3, 'destination': 2, 'speed': 2,
                      'query': 3, 'permission': 1, 'loco_release': 1,
                      'restore': 2, 'end_release': 1}[name]
        args = [u32(uc, sp + 4 * (i + 1)) for i in range(args_count)]
        events.append(dict(event=name, args=args, state=state(uc)))
        if name in ('techno', 'scatter', 'destination', 'speed'):
            assert uc.reg_read(UC_X86_REG_ECX) == OWNER
        if name == 'query':
            assert args[0] == LOCO and args[1] == 0x819088
        if name in ('permission', 'end_release', 'restore'):
            assert args[0] == END
        if name == 'restore':
            assert args[1] == OWNER + 0x674

    def observe_write(_uc, _access, address, size, value, _data):
        if address in (OWNER + 0x6B3, OWNER + 0x687, OWNER + 0x674,
                       OWNER + 0x598, OWNER + 0x58C):
            writes.append(dict(offset=f'{address-OWNER:X}', size=size, value=value))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_WRITE, observe_write)
    before = state(u)
    # Segmentation substitutes only the explicitly declared Techno base callee.
    # The original CALL and RET8 frame execute; no decision branch is redirected.
    current = ENTRY
    for _ in range(4):
        end = run_checked(u, current, (RET_MAGIC, TECHNO), count=2000,
                          required_addresses=(current,))
        if end == RET_MAGIC:
            break
        return_address = u32(u, u.reg_read(UC_X86_REG_ESP))
        run_checked(u, STUBS['techno'], return_address, count=2000,
                    required_addresses=(STUBS['techno'],))
        current = return_address
    else:
        raise AssertionError('Unexpected repeated Techno base entry')
    assert u.reg_read(UC_X86_REG_ESP) == SP + 12
    assert all(u.reg_read(reg) == val for reg, val in SAVED.items())
    assert originals == [bytes(u.mem_read(a, b-a)) for a, b in CODE_RANGES]
    assert bytes(u.mem_read(UNIT_VTABLE, 0x600)) == original_vtable
    return dict(input=row, before=before, after=state(u),
                returned_al=u.reg_read(UC_X86_REG_EAX) & 255,
                reentrant_al=u.mem_read(REENTER_RESULT, 1)[0],
                events=events, writes=writes, visits=visits)


def execute_ai(row):
    """Original FootAI entry through live-reset or dead-return boundary only."""
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(RET_MAGIC, 0x1000)
    originals = [bytes(u.mem_read(a, b-a)) for a, b in CODE_RANGES]
    u.mem_write(OWNER + 0x90, bytes((row['entry_alive'],)))
    u.mem_write(OWNER + 0x6B3, bytes((row['entry_latch'],)))
    u.mem_write(SP, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, OWNER)
    for register, value in SAVED.items():
        u.reg_write(register, value)
    events, writes = [], []

    def snapshot(uc):
        return dict(alive=uc.mem_read(OWNER + 0x90, 1)[0],
                    latch=uc.mem_read(OWNER + 0x6B3, 1)[0])

    def observe_write(_uc, _access, address, size, value, _data):
        if address in (OWNER + 0x90, OWNER + 0x6B3):
            writes.append(dict(offset=f'{address-OWNER:X}', size=size, value=value))

    u.hook_add(UC_HOOK_MEM_WRITE, observe_write)
    run_checked(u, AI_ENTRY, AI_TECHNO, count=100,
                required_addresses=(AI_ENTRY, 0x4DA539))
    assert u.reg_read(UC_X86_REG_ECX) == OWNER
    return_address = u32(u, u.reg_read(UC_X86_REG_ESP))
    assert return_address == 0x4DA53E
    events.append(dict(event='external_techno_ai', state=snapshot(u)))
    body = (b'\xC6\x05' + dwords(OWNER + 0x90) + bytes((row['callback_alive'],))
            + b'\xC6\x05' + dwords(OWNER + 0x6B3) + bytes((row['callback_latch'],))
            + ret(255))
    u.mem_write(STUBS['techno'], body)
    run_checked(u, STUBS['techno'], return_address, count=100,
                required_addresses=(STUBS['techno'],))
    events.append(dict(event='techno_ai_returned', state=snapshot(u)))
    end = run_checked(u, return_address, (AI_LIVE, AI_DEAD), count=100,
                      required_addresses=(0x4DA53E, 0x4DA548))
    if end == AI_DEAD:
        run_checked(u, AI_DEAD, RET_MAGIC, count=100, required_addresses=(AI_DEAD,))
        assert u.reg_read(UC_X86_REG_ESP) == SP + 4
        assert all(u.reg_read(reg) == val for reg, val in SAVED.items())
    else:
        assert u.reg_read(UC_X86_REG_ESP) == SP - 0x24
        assert u.reg_read(UC_X86_REG_ESI) == OWNER
    assert originals == [bytes(u.mem_read(a, b-a)) for a, b in CODE_RANGES]
    return dict(input=row, after=snapshot(u), events=events, writes=writes,
                boundary='live_foot_continuation' if end == AI_LIVE else 'dead_return')


def generate():
    rows = []
    for latch, args, pending, end, queue in product(
            (0, 1), ([0, 0], [0, 1], [1, 0], [1, 1]), (0, 1),
            ('none', 'unsupported', 'denied', 'success'), ([], [0], [1, 2, 0])):
        rows.append(dict(group='core', latch=latch, args=args,
                         scatter_pending=pending, end=end, queue=queue))
    for head, end, mutation, replace_storage in product(
            (0, 1, 2), ('none', 'denied', 'success'),
            ([], [3], [3, 4, 5]), (False, True)):
        rows.append(dict(group='queue_mutation', args=[0, 1], end=end,
                         queue=[head, 1, 2], mutation=mutation, replace_storage=replace_storage))
    for callback, args in product(('techno', 'scatter', 'destination', 'restore'),
                                  ([0, 0], [0, 1], [1, 0], [1, 1])):
        rows.append(dict(group='reentrant', args=args, end='success',
                         scatter_pending=1, queue=[1, 2], reenter=callback,
                         callback_return=255))
    for queue, live_queue in (([], [2, 1]), ([0, 1], []), ([0], [2])):
        rows.append(dict(group='techno_live_queue', args=[255, 2], end='none',
                         queue=queue, techno_queue=live_queue))
    results = [execute(row) for row in rows]
    assert len(results) == 265
    for result in results:
        row = result['input']
        names = [event['event'] for event in result['events']]
        if row.get('latch', 0):
            assert not names and result['returned_al'] == 0
        else:
            assert names.count('techno') == 1
            assert result['events'][0]['state']['latch'] == 1
        if row.get('reenter'):
            assert result['reentrant_al'] == 0 and result['visits'].count(f'{ENTRY:08X}') == 2
        if 'restore' in names:
            restore = result['events'][names.index('restore')]
            assert restore['state']['locomotor'] == 0 and 'speed' not in names
        if 'scatter' in names:
            scatter = result['events'][names.index('scatter')]
            assert scatter['state']['scatter_pending'] == 0
            assert scatter['args'] == [0x8B3DA8, 1, 0]
    for entry_alive, entry_latch, callback_alive, callback_latch in product(
            (0, 1), (0, 1), (0, 1), (0, 1)):
        result = execute_ai(dict(group='foot_ai_reset', entry_alive=entry_alive,
                                 entry_latch=entry_latch, callback_alive=callback_alive,
                                 callback_latch=callback_latch))
        assert result['after']['alive'] == callback_alive
        assert result['after']['latch'] == (0 if callback_alive else callback_latch)
        results.append(result)
    assert len(results) == 281
    return results


def metadata():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    result = provenance(
        scope='Bounded original Foot4D82B0 latch, END lifecycle calls, live NavQueue removal and speed-setter gates; complete Foot body with declared external callbacks; separately labelled FootAI alive/latch reset entry slice',
        assumptions=[
            'Fresh full RET8 function entry; real Unit vtable copied to scratch with only Scatter+174, destination+480 and speed+544 replaced. Original Unit RTTI746E20 executes. Every return checks stack and all nonvolatile registers; native code spans and original Unit vtable remain unchanged',
            'Owner+520=-1 excludes legacy team/planning branch4D83F8..8445; original Unit RTTI=1 excludes Infantry RTTI15 archive branch4D8472..852A. Team/script effects, Infantry archive pursuit and outer Unit/Infantry/Aircraft leaf receivers are not covered',
            'Original QI helper45AF20 executes against supplied COM interfaces: no locomotor, E_NOINTERFACE, permission denied, permission accepted. Restore writes a supplied replacement pointer. Actual END class policy, lifetime/refcount correctness and allocator behavior are not executed',
            'Target addresses remain opaque to Foot base. Fixture labels0/3=Cell,1/4=Unit,2/5=Building contain original vtables7E4EEC/7F5C70/7E3EBC; their native RTTI instruction bytes returning11/1/6 are asserted. Whole objects are not initialized; no target RTTI is called or needed in this body. Destination acceptance/effects are supplied, so this is category-unfiltered transport evidence, not complete category-aware setter parity',
            '192 core rows cross entered latch, four arg pairs, Scatter pending, four END outcomes and three queues.54 rows mutate live count/storage in destination callback;16 reenter original Foot from external callbacks;3 change live queue during Techno and forward noncanonical arg bytes',
            'Reentrant calls execute original native Foot entry under already-published latch; no fixture computes their result. Event/store snapshots record callback-visible state and native post-callback reloads. Callback-mutated queues are deliberately supplied interference, with no claim that all mutations have an active native producer',
            'Techno709A40 has real Temporal, attackmove and PlanMgr effects omitted here; its entire body is explicitly substituted. This oracle cannot justify skipping those prerequisites in production',
            '16 separately labelled foot_ai_reset rows execute original FootAI4DA530, original CALL6F9E50, supplied TechnoAI callback and original postcallback +90 branch/latch clear. Matrix crosses entry alive/latch and callback alive/latch. Alive rows stop before4DA554; dead rows execute original4DAF00 epilogue to return. Later FootAI work, object dispatcher and whole TechnoAI behavior are excluded',
        ],
        substitutions=[
            'At original direct callee709A40 only, segmented execution runs scratch callback with original CALL stack and RET8 then resumes original return address. It optionally reenters Foot or mutates queue; it does not execute actual Techno base semantics',
            'Copied owner virtual slots+174/+480/+544 use explicit scratch callbacks; Scatter observes cleared+687, destination can replace queue contents/count/storage, speed records original binary64 argument. Other slots remain original',
            'Scratch COM QueryInterface RET12 writes supplied output pointer/HRESULT; permission RET4 returns supplied AL; old locomotor Release RET4 records invocation; restore RET8 writes supplied pointer; queried END Release RET4 records invocation. No real COM implementation or lifetime is claimed',
            'FootAI slice separately substitutes entire TechnoAI6F9E50 with scratch RET callback writing declared owner+90 and+6B3. The original caller reload and branch execute; callback return255 demonstrates that caller gate reads owner memory rather than callback AL',
        ],
        entry_points={'foot_enter_idle': ENTRY, 'techno_external_boundary': TECHNO,
                      'query_interface_helper': QUERY_HELPER, 'unit_rtti': 0x746E20,
                      'unit_scatter_original': 0x743A50, 'unit_destination_original': 0x741970,
                      'foot_ai_entry': AI_ENTRY, 'techno_ai_external_boundary': AI_TECHNO,
                      'foot_ai_live_boundary': AI_LIVE, 'foot_ai_dead_epilogue': AI_DEAD},
    )
    result['original_code_range_sha256'] = {
        f'{a:08X}..{b:08X}': hashlib.sha256(bytes(u.mem_read(a, b-a))).hexdigest()
        for a, b in CODE_RANGES}
    result['case_counts'] = dict(total=281, core=192, queue_mutation=54,
                                 reentrant=16, techno_live_queue=3, foot_ai_reset=16)
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
