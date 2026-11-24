"""Original Find_Path's bounded copy, over a supplied AStar result.

Reuse the existing Unit setter/Drive-or-Ship Process/wrapper continuation. The
search result is supplied at exactly the owner's existing 4CBBA0 boundary.
Native copies, terminator presence, writes and caller continuation are observed.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP

from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle import track_path_continuation as owner

CAPACITY = 24
COPY_PREPARE, COPY, COPY_DONE = 0x4D3E98, 0x4D3E9F, 0x4D3EA1
BEFORE_CANARY, AFTER_CANARY = 0x12345678, 0x76543210
INITIAL_WORDS = [-1, *range(1001, 1024)]


def route_cases():
    """Supplied AStar inputs, not a second expected-copy implementation."""
    result = [dict(name='short', route=[2, 2, 2])]
    result += [dict(name=f'moves_{count}', route=[2] * 12 + [4] * (count - 12))
               for count in (23, 24, 25)]
    result.append(dict(name='tube_inside_window', route=[2] * 12 + [8] + [4] * 12,
                       tube_exit=[16, 12]))
    for row in result:
        cells = [[10, 10]]
        for word in row['route']:
            x, y = cells[-1]
            if word == 8:
                cells.append(row['tube_exit'])
            else:
                dx, dy = {2: (1, 0), 4: (0, 1)}[word]
                cells.append([x + dx, y + dy])
        row['route_cells'] = cells
    return result


def query(family, route):
    result = dict(input=dict(route, name=f'{family}.{route["name"]}', family=family,
                             prefix=0, reference_cell=[10, 10],
                             initial_words=INITIAL_WORDS))
    retained = {}

    def prepare(u, read32):
        retained['u'] = u
        path = owner.ACTOR + 0x5E0
        u.mem_write(path, dwords(*INITIAL_WORDS))
        reached, copy_writes = set(), []
        saved_canaries = None
        return_pc = None

        def signed(address, count=1):
            return list(struct.unpack('<' + 'i' * count, u.mem_read(address, count * 4)))

        def snapshot():
            words = signed(path, CAPACITY)
            nav = read32(owner.ACTOR + 0x5A4)
            return dict(words=words, live_words=words[:words.index(-1)] if -1 in words else words,
                        reference=list(struct.unpack('<2h', u.mem_read(owner.ACTOR + 0x558, 4))),
                        nav=list(struct.unpack('<2h', u.mem_read(nav + 0x24, 4))) if nav else None,
                        destination=signed(owner.LOCO + 0x34, 3),
                        head=signed(owner.LOCO + 0x40, 3),
                        movement_timer=[read32(owner.ACTOR + 0x640), read32(owner.ACTOR + 0x648)])

        result['before_process'] = snapshot()
        retained['snapshot'] = snapshot
        retained['original_text'] = bytes(u.mem_read(0x401000, 0x3E0000))
        retained['tables'] = {p: bytes(u.mem_read(p, 0x600))
                              for p in (0x7F5C70, 0x7F6218, 0x7E4EEC, read32(owner.LOCO + 4))}

        def observe(vm, pc, _size, _data):
            nonlocal saved_canaries, return_pc
            reached.add(pc)
            if pc == owner.FIND_PATH:
                assert return_pc is None, 'unexpected second Find_Path in one window witness'
                sp = vm.reg_read(UC_X86_REG_ESP)
                return_pc = read32(sp)
                packed = read32(sp + 4)
                result['request'] = dict(cell=[packed & 0xFFFF, packed >> 16],
                                         prefix=read32(sp + 8), urgency=read32(sp + 12))
            elif pc == 0x4D3E50:
                pointer = vm.reg_read(UC_X86_REG_EAX)
                result['supplied_astar'] = dict(cost=read32(pointer + 4),
                    length=read32(pointer + 8),
                    buffer=signed(read32(pointer + 12), read32(pointer + 8)))
            elif pc == COPY_PREPARE:
                # Only the REP copy sees these guards. Restore the neighboring
                # fields before Mark/timers resume: +640 is a real timer field.
                saved_canaries = [bytes(u.mem_read(path - 4, 4)), bytes(u.mem_read(path + 96, 4))]
                u.mem_write(path - 4, dwords(BEFORE_CANARY))
                u.mem_write(path + 96, dwords(AFTER_CANARY))
                result['copy'] = dict(count=vm.reg_read(UC_X86_REG_ECX), before=snapshot(),
                    canaries_before=[read32(path - 4), read32(path + 96)])
            elif pc == COPY_DONE:
                assert saved_canaries is not None
                result['copy'].update(after=snapshot(), writes=copy_writes,
                    canaries_after=[read32(path - 4), read32(path + 96)])
                assert result['copy']['canaries_after'] == result['copy']['canaries_before']
                for pointer, raw in zip((path - 4, path + 96), saved_canaries):
                    u.mem_write(pointer, raw)
                saved_canaries = None
            elif pc == return_pc:
                result['wrapper_return'] = snapshot()
                result['wrapper_return']['returned_al'] = vm.reg_read(UC_X86_REG_EAX) & 255
                return_pc = None

        def wrote(vm, _access, pointer, size, value, _data):
            if vm.reg_read(UC_X86_REG_EIP) == COPY:
                assert size == 4 and path <= pointer < path + 96, (hex(pointer), size)
                copy_writes.append(dict(index=(pointer - path) // 4,
                                        value=struct.unpack('<i', dwords(value))[0]))

        u.hook_add(UC_HOOK_CODE, observe)
        u.hook_add(UC_HOOK_MEM_WRITE, wrote)
        retained['reached'] = reached

    continued = owner.query(dict(family=family, find_path='native',
        destination=route['route_cells'][-1], route=route['route']), before_process=prepare)
    u = retained['u']
    assert {owner.FIND_PATH, owner.ASTAR, COPY_PREPARE, COPY, COPY_DONE, 0x4D4003, 0x4D4013} <= retained['reached']
    assert 'wrapper_return' in result
    assert result['request']['prefix'] == 0
    assert retained['original_text'] == bytes(u.mem_read(0x401000, 0x3E0000))
    assert all(raw == bytes(u.mem_read(p, len(raw))) for p, raw in retained['tables'].items())
    result['after_process_boundary'] = retained['snapshot']()
    result['after_process_boundary']['returned'] = continued['state']['returned']
    result['events'] = continued['events']
    return result


def generate():
    return dict(schema_version=1, inputs=dict(capacity=CAPACITY,
        source_cell=[10, 10], allocated_map=[32, 32],
        supplied_search_boundary='0x004CBBA0', copy_instruction='0x004D3E9F'),
        cases=[query(family, row) for family in ('drive', 'ship') for row in route_cases()])


def metadata():
    return provenance(
        scope='Original Unit destination -> Drive/Ship Process -> Foot Find_Path wrapper and its 24-dword REP copy. Short,23,24,25 movement-word AStar outputs plus a nonadjacent tube word inside a long output. Observed copy count/writes/all24words, transient canaries, wrapper return and outer Process boundary. Not an AStar search, route optimality, tube traversal, full trip or append-prefix parity claim.',
        entry_points=dict(unit_destination=0x741970, find_path=owner.FIND_PATH,
            supplied_astar=owner.ASTAR, copy=COPY, copy_done=COPY_DONE,
            reference_store=0x4D4003, wrapper_ret=0x4D4013,
            drive_process=owner.PROCESS['drive'], ship_process=owner.PROCESS['ship']),
        assumptions=[
            'Reuse track_path_continuation.query without changing its default corpus. Its original Unit/Drive-or-Ship/Cell vtables, real locomotor constructors, flat32x32 map, human House, selected Rules/zone inputs and frame100 setter/frame101 Process are retained.',
            'The full supplied AStar route remains available in the wrapper stack buffer. PathType cost is the movement-word count; length is that count plus the supplied -1 terminator, as documented by original reconstruction42AA90. Native wrapper code alone determines how many words are copied. The route_cells list is input geometry for Rust installation, with an explicit nonadjacent jump for word8; no original reconstruction/search or TubeArray lookup runs.',
            'The initial Foot backing has live head-1 followed by distinct stale values. Snapshot live_words stops at the first native -1, or includes all24 when no terminator is copied. Untouched backing beyond a short terminator is observed, not required of Rust Vec storage.',
            'Immediately before REP MOVSD only, distinctive dwords are supplied at Foot+5DC and+640. Their unchanged post-copy values are captured, then both prior values are restored before native Mark/timers continue. This measures copy containment without treating neighboring timer storage as a permanent canary.',
            'All native text and selected vtables are checked unchanged. Copy writes are captured from native memory-write events, and execution must reach the original reference-cell store and successful wrapper RET. Post-copy, wrapper-return and outer-Process snapshots are separate boundaries. Prefix0 only; IsTrain append remains outside scope.'],
        substitutions=[
            'Existing track_path_continuation AStar4CBBA0 hook supplies PathType and the complete directions-plus-terminator buffer, with native24-byte argument cleanup. Find_Path itself, Unit callback, Mark, timer/reference writes and Process continuation execute unchanged.',
            'Only the existing OS Interlocked imports are otherwise substituted. No copy count, copied word, canary result, wrapper return or path-consumption result is supplied.'],
    )


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
