"""Original Drive/Ship selector, paid-sample, chain-tail and terminal expressions.

Interior blocks execute with read-only observation hooks and no patched code. They are
separate witnesses: supplied frames replace the intervening admission, position,
occupation and arrival callbacks. This is not a full movement simulation.
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_EDX,
    UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)
from tools.native_oracle import (
    load_image, run_checked, finish_vectors, provenance,
    STACK_BASE, STACK_SIZE, SCRATCH, SCRATCH_SIZE, RET_MAGIC,
)
from tools.spatial_oracle.map_queries import dwords


FAMILIES = {
    'drive': dict(turns=0x7E7B28, raw=0x7E7A28, count=72,
                  fresh=(0x4B4016, 0x4B4034), zero=(0x4B4659, 0x4B4660),
                  gate=0x4B150D, read=0x4B1596, sample=0x4B15CE,
                  sentinel=0x4B1F97, tail=0x4B1F48, again=0x4B158F,
                  exhausted=0x4B1F5C, budget=0x38, xy=0x84,
                  chain=(0x4B1C78, 0x4B1CA7),
                  refund=(0x4B1F97, 0x4B2006), clear=(0x4B210E, 0x4B211C)),
    'ship': dict(turns=0x7F2A40, raw=0x7F2960, count=64,
                 fresh=(0x6A3642, 0x6A3660), zero=(0x6A3C88, 0x6A3C8F),
                 gate=0x6A0BD5, read=0x6A0C52, sample=0x6A0C8E,
                 sentinel=0x6A15DA, tail=0x6A158B, again=0x6A0C52,
                 exhausted=0x6A159F, budget=0x34, xy=0x9C,
                 chain=(0x6A12C2, 0x6A12ED),
                 refund=(0x6A15DA, 0x6A1649), clear=(0x6A1751, 0x6A175F)),
}
LOCO, FOOT = SCRATCH + 0x100, SCRATCH + 0x1000
SP = STACK_BASE + STACK_SIZE - 0x1000


class OriginalCursor:
    def __init__(self, family):
        self.name, self.f = family, FAMILIES[family]
        self.uc = u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(SCRATCH, SCRATCH_SIZE)
        u.mem_map(RET_MAGIC, 0x1000)
        # Hardware startup witnesses in bounce_startup_capture.json record this
        # control word at WinMain. Its continued use here is a supplied state.
        u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
        u.mem_write(0x822D80, dwords(0x0E7F))

    def ints(self, address, count=1):
        return list(struct.unpack('<' + 'i' * count, self.uc.mem_read(address, count * 4)))

    def seed(self, turn, cursor, reverse, budget):
        u = self.uc
        u.mem_write(LOCO, bytes(0x100))
        u.mem_write(SP, bytes(0x100))
        u.mem_write(LOCO + 0xC, dwords(FOOT))
        u.mem_write(LOCO + 0x58, dwords(turn, cursor))
        u.mem_write(LOCO + 0x60, bytes((int(reverse),)))
        u.mem_write(SP + self.f['budget'], dwords(budget))
        u.reg_write(UC_X86_REG_EBP, LOCO)
        u.reg_write(UC_X86_REG_ESP, SP)

    def run(self, begin, ends, required=()):
        end = run_checked(self.uc, begin, ends, count=300,
                          required_addresses=(begin, *required))
        assert self.uc.reg_read(UC_X86_REG_ESP) == SP
        return end

    def state(self):
        return dict(turn=self.ints(LOCO + 0x58)[0], cursor=self.ints(LOCO + 0x5C)[0],
                    reversed=bool(self.uc.mem_read(LOCO + 0x60, 1)[0]))

    def table(self, index):
        normal, short, facing, flags = struct.unpack(
            '<bb2xii', self.uc.mem_read(self.f['turns'] + index * 12, 12))
        return dict(normal=normal, short=short, facing=facing, flags=flags)

    def raw(self, index):
        pointer, chain, entry, handoff = struct.unpack(
            '<Iiii', self.uc.mem_read(self.f['raw'] + index * 16, 16))
        points = []
        for cursor in range(300):
            point = self.ints(pointer + cursor * 12, 3)
            points.append(point)
            if cursor and point[:2] == [0, 0]:
                return dict(chain=chain, entry=entry, handoff=handoff, points=points)
        raise AssertionError('Original raw track has no bounded terminal sample')

    def fresh(self, first, second):
        self.seed(-7, 77, True, 123)
        self.uc.reg_write(UC_X86_REG_EBX, first)
        self.uc.reg_write(UC_X86_REG_ESI, second)
        self.run(*self.f['fresh'])
        selected = self.state()
        self.run(*self.f['zero'])
        return dict(selected=selected, accepted=self.state())

    def sample(self, turn, reverse, cursor, budget):
        self.seed(turn, cursor, reverse, budget)
        self.uc.reg_write(UC_X86_REG_EDX, budget)
        # Supplied remaining path sentinel: skips the unrelated direction gate.
        self.uc.reg_write(UC_X86_REG_EAX, 0xFFFFFFFF)
        f = self.f
        end = self.run(f['gate'], (f['exhausted'], f['sample'], f['sentinel']))
        result = dict(kind='unpaid', after=self.state(),
                      budget=self.ints(SP + f['budget'])[0])
        if end == f['exhausted']:
            return result
        result.update(kind='terminal' if end == f['sentinel'] else 'point',
                      xy=self.ints(SP + f['xy'], 2))
        if end == f['sample']:
            # No position/occupation/chain callbacks are executed between these
            # blocks. The original tail itself publishes the next cursor.
            end = self.run(f['tail'], (f['again'], f['exhausted']))
            result.update(after=self.state(), continue_paid=end == f['again'])
        return result

    def chain(self, turn, budget):
        self.seed(9, 45, True, budget)
        pointer = self.f['turns'] + turn * 12
        if self.name == 'drive':
            self.uc.mem_write(SP + 0x40, dwords(turn, pointer))
        else:
            self.uc.mem_write(SP + 0x3C, dwords(turn))
            self.uc.reg_write(UC_X86_REG_EBX, pointer)
        self.run(*self.f['chain'])
        accepted = self.state()
        # Survivor/common-tail witness only. Native owner death/limbo/falling
        # after the intervening callback can leave the accepted entry-1 state.
        end = self.run(self.f['tail'], (self.f['again'], self.f['exhausted']))
        return dict(accepted=accepted, after=self.state(), continue_paid=end == self.f['again'],
                    budget=self.ints(SP + self.f['budget'])[0])

    def terminal(self, current, head, budget):
        self.seed(9, 45, True, budget)
        self.uc.mem_write(LOCO + 0x40, dwords(*head))
        self.uc.mem_write(FOOT + 0x9C, dwords(*current))
        self.uc.mem_write(FOOT + 0x6B6, bytes((0, 1)))
        self.run(*self.f['refund'], required=(0x7C5F00,))
        result = dict(budget=self.ints(SP + self.f['budget'])[0],
                      occupation=list(self.uc.mem_read(FOOT + 0x6B6, 2)))
        # Explicitly skip terminal coordinate/arrival/head-clear callbacks.
        self.run(*self.f['clear'])
        result['selectors_after_clear'] = self.state()
        assert self.ints(LOCO + 0x40, 3) == head
        assert self.ints(FOOT + 0x9C, 3) == current
        return result


def terminal_cases(raw):
    offsets = [('supplied_offset', offset) for offset in
               ([0, 0], [0, 1], [0, 3], [0, 7], [0, 10], [0, 11], [0, 12],
                [-8, 8], [96, 85], [-670, -68])]
    offsets += [(f'raw_{index}_last_real_point', track['points'][-2][:2])
                for index, track in sorted(raw.items(), key=lambda item: int(item[0]))]
    for source, offset in offsets:
        head = [2176, 2176, 731]
        # Signed flips/swaps of the actual transform preserve this Manhattan
        # distance. The supplied pose is not a complete curve-execution result.
        current = [head[0] + offset[0], head[1] + offset[1], -347]
        yield source, dict(current=current, head=head, budget=1)


def generate():
    families = {}
    for name, f in FAMILIES.items():
        native = OriginalCursor(name)
        tables = [native.table(i) for i in range(f['count'])]
        raw_ids = sorted({t[key] for t in tables for key in ('normal', 'short')} - {0})
        raw = {str(i): native.raw(i) for i in raw_ids}
        samples = []
        for raw_id in raw_ids:
            turn, reverse = next((i, key == 'short') for i, t in enumerate(tables)
                                 for key in ('normal', 'short') if t[key] == raw_id)
            # Execute every real and terminal sample. Boundary budgets separately
            # prove the strict >7 admission and same-pass tail decision.
            for cursor in range(len(raw[str(raw_id)]['points'])):
                for budget in (7, 8, 14, 15):
                    case = dict(turn=turn, reverse=reverse, cursor=cursor, budget=budget)
                    samples.append(dict(input=case, output=native.sample(**case)))
        chains = []
        for turn, t in enumerate(tables[:64]):
            if t['normal'] and raw[str(t['normal'])]['entry'] > 0:
                for budget in (7, 8):
                    chains.append(dict(input=dict(turn=turn, budget=budget),
                                       output=native.chain(turn, budget)))
        terminals = []
        for source, case in terminal_cases(raw):
            terminals.append(dict(source=source, input=case, output=native.terminal(**case)))
        families[name] = dict(tables=tables, raw=raw, samples=samples, chains=chains,
                              terminals=terminals,
                              fresh=[dict(input=dict(first=a, second=b), output=native.fresh(a, b))
                                     for a in range(8) for b in range(8)])
    return families


def metadata():
    return provenance(
        scope='Original Drive/Ship retained selectors, paid raw samples, chain tail and terminal budget expressions',
        assumptions=[
            'Interior blocks use supplied register/stack frames and admitted selectors; no complete Process call is executed',
            'Original TurnTrack/RawTrack data includes every real point and the trailing XY-zero sentinel, including its facing',
            'Every point uses budgets 7/8/14/15; original tail decides cursor increment and whether another paid step is due',
            'Fresh selection and cursor reset are separate blocks; chain cases supply an already admitted nonzero-entry successor',
            'Chain after-state executes the survivor common tail; callback-triggered death/limbo/falling exits can retain entry-minus-one and are excluded',
            'Terminal expressions execute original ftol with supplied FPCW and ftol control word 0xE7F, consistent with saved startup capture; runtime immutability is not proved',
            'Terminal offsets include every referenced raw track last-real-point offset and separate scalar boundaries/distant poses; they are supplied coordinates, not complete curve-execution results',
        ], substitutions=[
            'Admission, movement/occupation/chain callbacks, coordinate commits, head clears, arrival and residual interpolation between blocks are not executed',
            'The original point read and original common tail are composed across those callback seams; no instructions or call results are patched',
        ], entry_points={f'{name}_{key}': value[0] if isinstance(value, tuple) else value
                         for name, f in FAMILIES.items()
                         for key, value in f.items()
                         if key in ('fresh', 'zero', 'gate', 'read', 'tail', 'chain', 'refund', 'clear')})


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
