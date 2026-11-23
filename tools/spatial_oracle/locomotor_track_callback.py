"""Original paid-loop caches across supplied callback mutations.

Execute original selection/read, accepted-chain publication, survivor tail and
Transform_Track_Coords. Callback bodies and intervening position/occupation work
are deliberately not executed; mutations are supplied counterfactual inputs.
"""
from pathlib import Path

from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX,
    UC_X86_REG_ESI, UC_X86_REG_ESP,
)

from tools.native_oracle import finish_vectors, provenance, run_checked, RET_MAGIC, SCRATCH
from tools.native_oracle import require
from tools.spatial_oracle.locomotor_track_cursor import OriginalCursor, LOCO, SP
from tools.spatial_oracle.map_queries import dwords


CACHE = {
    'drive': dict(turn=0x8C, raw_offset=0x3C, points=0x94,
                  chain_cache_end=0x4B1CF9, transform=0x4B4780,
                  facing=(0x4B16C1, 0x4B16D8, 0x30), null=0x8A0790,
                  residual=0x4B1F5C, residual_point=0x4B2312),
    'ship': dict(turn=0x98, raw_offset=0x54, points=0x38,
                 chain_cache_end=0x6A133C, transform=0x6A3DB0,
                 facing=(0x6A0D8A, 0x6A0DA0, 0x44), null=0xB077F8,
                 residual=0x6A159F, residual_point=0x6A1951),
}


class OriginalCallback(OriginalCursor):
    def selected(self):
        c, f = CACHE[self.name], self.f
        turn = (self.ints(SP + c['turn'])[0] - f['turns']) // 12
        raw = self.ints(SP + c['raw_offset'])[0] // 16
        points = self.ints(SP + c['points'])[0]
        assert points == self.ints(f['raw'] + raw * 16)[0]
        return dict(turn=turn, raw=raw, target_facing=self.table(turn)['facing'])

    def read_sample(self):
        selected = self.selected()
        cursor = self.state()['cursor']
        pointer = self.ints(SP + CACHE[self.name]['points'])[0]
        point = self.ints(pointer + cursor * 12, 3)
        assert point[:2] == self.ints(SP + self.f['xy'], 2)
        begin, end, output = CACHE[self.name]['facing']
        self.run(begin, end)
        assert point[2] == self.ints(SP + output)[0]
        return dict(selected=selected, cursor=cursor, point=point,
                    budget=self.ints(SP + self.f['budget'])[0])

    def transform(self, point):
        # Separate call frame preserves the enclosing Process_Track's locals.
        # All helper instructions execute, including its live selector/head reads.
        out, source, facing = SCRATCH + 0x4000, SCRATCH + 0x4100, SCRATCH + 0x4200
        call_sp = SP - 0x400
        u = self.uc
        u.mem_write(source, dwords(*point[:2]))
        u.mem_write(facing, dwords(point[2]))
        u.mem_write(call_sp, dwords(RET_MAGIC, out, source, facing))
        u.reg_write(UC_X86_REG_ECX, LOCO)
        u.reg_write(UC_X86_REG_ESP, call_sp)
        entry = CACHE[self.name]['transform']
        run_checked(u, entry, RET_MAGIC, count=100, required_addresses=(entry,))
        assert u.reg_read(UC_X86_REG_ESP) == call_sp + 16
        u.reg_write(UC_X86_REG_ESP, SP)
        return dict(xy=self.ints(out, 2), facing=self.ints(facing)[0])

    def callback(self, initial, mutation, chain=None):
        self.seed(initial['turn'], initial['cursor'], initial['reversed'], 15)
        u, f, c = self.uc, self.f, CACHE[self.name]
        # Distinct retained/local budgets expose their separate authorities.
        # Callback bodies and lifecycle exits are outside these witnesses.
        u.mem_write(LOCO + 0x4C, dwords(initial['residual']))
        u.mem_write(LOCO + 0x40, dwords(17, 29, 43))
        u.reg_write(UC_X86_REG_EDX, 15)
        u.reg_write(UC_X86_REG_EAX, 0xFFFFFFFF)
        require(self.run(f['gate'], (f['sample'], f['sentinel'])) == f['sample'], 'gate did not return the sample')
        first = self.read_sample()
        if chain is not None:
            pointer = f['turns'] + chain * 12
            if self.name == 'drive':
                u.mem_write(SP + 0x40, dwords(chain, pointer))
            else:
                u.mem_write(SP + 0x3C, dwords(chain))
                u.reg_write(UC_X86_REG_EBX, pointer)
            # These original instructions also publish the new raw pointer.
            # ESI points to the retained head cleared before PerCellProcess(2).
            u.reg_write(UC_X86_REG_ESI, LOCO + 0x40)
            self.run(f['chain'][0], c['chain_cache_end'])
        accepted = dict(progress=self.state(), selected=self.selected(),
                        head=self.ints(LOCO + 0x40, 3),
                        valid=bool(u.mem_read(LOCO + 0x63, 1)[0]),
                        mismatch=bool(u.mem_read(SP + 0x13, 1)[0]))
        if chain is not None:
            accepted['native_null'] = self.ints(c['null'], 3)
            assert accepted['head'] == accepted['native_null']
            assert accepted['valid'] and not accepted['mismatch']
        # Only fields named by the case are replaced at this supplied seam.
        if 'turn' in mutation:
            u.mem_write(LOCO + 0x58, dwords(mutation['turn']))
        if 'cursor' in mutation:
            u.mem_write(LOCO + 0x5C, dwords(mutation['cursor']))
        if 'reversed' in mutation:
            u.mem_write(LOCO + 0x60, bytes((mutation['reversed'],)))
        if 'residual' in mutation:
            u.mem_write(LOCO + 0x4C, dwords(mutation['residual']))
        u.mem_write(LOCO + 0x40, dwords(*mutation['head']))
        before_tail = self.state()
        require(self.run(f['tail'], (f['again'], f['exhausted'])) == f['again'], 'tail did not return again')
        require(self.run(f['again'], (f['sample'], f['sentinel'])) == f['sample'], 'again did not return the sample')
        second = self.read_sample()
        second['transformed'] = self.transform(second['point'])
        after_payment = self.state()
        retained_residual = self.ints(LOCO + 0x4C)[0]
        require(self.run(f['tail'], (f['again'], f['exhausted'])) == f['exhausted'], 'tail did not return exhausted')
        # Residual branch deliberately reselects from the mutated live instance.
        self.run(c['residual'], c['residual_point'])
        residual = dict(progress=self.state(), budget=self.ints(LOCO + 0x4C)[0],
                        xy=self.ints(SP + f['xy'], 2))
        residual['transformed_xy'] = self.transform([*residual['xy'], 0])['xy']
        return dict(first=first, accepted=accepted, before_tail=before_tail,
                    second=second, after_payment=after_payment,
                    retained_residual=retained_residual, residual=residual)


def generate():
    result = {}
    for family in CACHE:
        native = OriginalCallback(family)
        initial = dict(turn=0, cursor=0, reversed=False, residual=6)
        cases = []
        # Include every transform-flag combination reachable in the family.
        # Selector, cursor and short-byte mutations are separate from chain adoption.
        turns = {}
        for index in range(native.f['count']):
            table = native.table(index)
            if table['normal'] and table['short']:
                turns.setdefault(table['flags'], index)
        for turn in turns.values():
            for reverse in (False, True):
                mutation = dict(turn=turn, cursor=2, reversed=reverse, residual=971,
                                head=[-731, 2176, 997])
                case = dict(initial=initial, mutation=mutation)
                cases.append(dict(input=case, output=native.callback(**case)))
        for chain in (1, 8, 10):
            for mutate_selector in (False, True):
                mutation = dict(head=[2176, -731, -997], residual=971)
                if mutate_selector:
                    mutation.update(turn=27, cursor=9, reversed=False)
                case = dict(initial=initial, mutation=mutation, chain=chain)
                cases.append(dict(input=case, output=native.callback(**case)))
        case = dict(initial=dict(turn=1, cursor=0, reversed=False, residual=6),
                    mutation=dict(cursor=11, reversed=True, residual=971,
                                  head=[2176, 2176, 997]))
        cases.append(dict(input=case, output=native.callback(**case)))
        result[family] = cases
    return result


def metadata():
    return provenance(
        scope='Drive/Ship paid raw-cache retention, accepted-chain cache publication, live transforms and residual reselection',
        assumptions=[
            'Supplied admitted initial curve and callback mutations; this does not prove a particular gameplay callback produces each mutation',
            'Original read and survivor tail are composed across omitted owner callbacks and placement work; no complete Process invocation executes',
            'All eight retail transform-flag combinations, both short-byte states and explicit accepted-chain cache replacement are exercised for both families',
            'Residual comparison stops after original raw XY lookup, before residual math and placement',
        ], substitutions=[
            'Callback mutations directly write retained selector/cursor/short/head/residual data at a supplied seam; original code and call results remain unpatched',
            'Accepted-chain admission is supplied; its original cache publication, full Null head clear and valid flag execute up to the callback callsite; PerCellProcess and later lifecycle/placement work are excluded',
            'Transform_Track_Coords executes as a separate complete original helper call using the paid cached raw point and current instance fields',
        ], entry_points={f'{family}_{key}': value
                         for family, cache in CACHE.items()
                         for key, value in cache.items()
                         if key in ('chain_cache_end', 'transform', 'residual', 'residual_point')})


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
