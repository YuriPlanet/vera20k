"""Original within-point handoff/chain cursor gates after supplied world effects.

The original paid selection/read establishes the call's cached raw descriptor.
Supply retained-state mutations, then execute the original post-placement gates.
No Mark, discovery, placement, handoff receiver or chain admission body executes.
"""
from pathlib import Path

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EDX

from tools.native_oracle import finish_vectors, provenance
from tools.native_oracle import require
from tools.spatial_oracle.locomotor_track_callback import OriginalCallback
from tools.spatial_oracle.locomotor_track_cursor import LOCO, SP
from tools.spatial_oracle.map_queries import dwords


GATES = {
    'drive': dict(handoff=(0x4B1AC6, 0x4B1AD9, 0x4B1B13),
                  chain=(0x4B1B35, 0x4B1B50, 0x4B1F48)),
    'ship': dict(handoff=(0x6A1102, 0x6A1115, 0x6A114F),
                 chain=(0x6A1171, 0x6A118C, 0x6A158B)),
}


def sample(native, initial, mutation, chain_cursor):
    native.seed(initial['turn'], 0, initial['reversed'], 15)
    u, f = native.uc, native.f
    u.mem_write(LOCO + 0x4C, dwords(6))
    u.reg_write(UC_X86_REG_EDX, 15)
    u.reg_write(UC_X86_REG_EAX, 0xFFFFFFFF)
    require(native.run(f['gate'], (f['sample'], f['sentinel'])) == f['sample'], 'gate did not return the sample')
    cached = native.selected()
    # This is an explicit supplied seam inside the SAME paid point. Neither
    # the survivor increment nor another payment runs before these gates.
    u.mem_write(LOCO + 0x58, dwords(mutation['turn'], mutation['cursor']))
    u.mem_write(LOCO + 0x60, bytes((int(mutation['reversed']),)))
    u.mem_write(LOCO + 0x4C, dwords(mutation['residual']))
    gates = GATES[native.name]
    begin, yes, no = gates['handoff']
    handoff = native.run(begin, (yes, no)) == yes
    after_handoff = native.state()
    # The later gate reloads again; a cached boolean pair is insufficient.
    # The handoff receiver is NOT executed or asserted to make this mutation.
    u.mem_write(LOCO + 0x5C, dwords(chain_cursor))
    begin, yes, no = gates['chain']
    chain = native.run(begin, (yes, no)) == yes
    return dict(cached=cached, handoff=handoff, chain=chain,
                after_handoff=after_handoff, after_chain=native.state(),
                local_budget=native.ints(SP + f['budget'])[0],
                retained_residual=native.ints(LOCO + 0x4C)[0])


def generate():
    result = {}
    for family in GATES:
        native = OriginalCallback(family)
        selectors = {}
        for turn in range(native.f['count']):
            table = native.table(turn)
            for reverse, key in ((False, 'normal'), (True, 'short')):
                if table[key]:
                    selectors.setdefault(table[key], dict(turn=turn, reversed=reverse))
        cases = []
        for raw_index, initial in selectors.items():
            raw = native.raw(raw_index)
            alternate = next(selector for other, selector in selectors.items()
                             if other != raw_index
                             and (native.raw(other)['chain'], native.raw(other)['handoff'])
                             != (raw['chain'], raw['handoff']))
            cursors = sorted({-1, 0, 1, raw['chain'] - 1, raw['chain'],
                              raw['chain'] + 1, raw['handoff'] - 1,
                              raw['handoff'], raw['handoff'] + 1})
            for cursor in cursors:
                for selector in (initial, alternate):
                    mutation = dict(**selector, cursor=cursor, residual=971)
                    for chain_cursor in sorted({cursor, raw['chain']}):
                        inputs = dict(initial=initial, mutation=mutation,
                                      chain_cursor=chain_cursor)
                        cases.append(dict(input=inputs,
                                          output=sample(native, **inputs)))
        result[family] = dict(raw_descriptors=len(selectors), cases=cases)
    return result


def metadata():
    return provenance(
        scope='Drive/Ship post-placement cursor gates use live progress and the retained raw descriptor',
        assumptions=[
            'Supplied admitted initial curve; original selection and point-zero payment execute',
            'Within-point world mutations are supplied counterfactual inputs, not proven gameplay callback outcomes',
            'Only the cursor portion of chain admission executes; direction, mismatch, entry and lifecycle gates are outside coverage',
            'All distinct retail raw descriptors selected by either family are covered, with same and different live selectors and cursor-zero exclusion',
        ], substitutions=[
            'Direct retained selector/cursor/short/residual writes replace omitted placement/Mark/world effects without patching any original instructions',
            'A second supplied cursor write replaces the omitted handoff receiver before executing the later chain gate',
            'Execution stops at each original branch destination, before handoff or chain effects',
        ], entry_points={f'{family}_{kind}': addresses[0]
                         for family, gates in GATES.items()
                         for kind, addresses in gates.items()})


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
