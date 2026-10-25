"""Original low wooden bridge damage: both axes, full family and physical sequence.

Synthetic input cells isolate selector/root/leaf publication. Shared repository
oracle bodies own execution and declared callback substitutions. The physical
sequence projects only already-executed repair overlay stores onto its frozen
physical crop. Neither stage executes live occupants, Recalc, graph or rendering.
"""
from collections import Counter
import copy
import gzip
import hashlib
import json
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_FPCW
from tools.native_oracle import provenance
from tools.spatial_oracle.shrapnel_repair.packet_io import finish_vectors
from tools.spatial_oracle.bridge_ordinary_damage import OrdinaryDamage, KEYS
from tools.spatial_oracle.bridge_rim import OriginalRim, CELLS, DUMMY

HERE = Path(__file__).resolve().parent
REPO = Path(__import__('tools.native_oracle', fromlist=['x']).__file__).resolve().parents[1]
TEXT_START, TEXT_SIZE = 0x401000, 0x3E0000
ENTRIES = {0x57BAA0:'selector', 0x57BCF0:'ns_root', 0x57C2B0:'ew_root',
           0x57DD50:'ns_leaf', 0x57E2A0:'ew_leaf', 0x57B870:'ns_classifier',
           0x57B990:'ew_classifier', 0x57C990:'ns_endpoints',
           0x57C870:'ew_endpoints', 0x575EE0:'notify', 0x5868A0:'rectangle'}
FORBIDDEN = {0x47DD70:'structural_fallout', 0x65C780:'raw_rng',
             0x65C7E0:'range_rng', 0x598030:'mapgen_rng', 0x70D4A0:'caller_detach'}
STOCK = 'tools/spatial_oracle/shrapnel_repair/shrapnel_repair.json.gz'


class LowWoodDamage(OrdinaryDamage):
    def __init__(self, case, physical=False):
        self.raw_writes = []
        self.native_entries = Counter()
        if physical:
            # Same shared callbacks; the170-cell crop needs no synthetic TMP
            # headers because Recalc is a declared return seam in this runner.
            self.case = case
            self.heap = 0x45000000
            self.ordinals = {}
            self.trace = []
            self.entries = 0
            self.callback = None
            self.heads = {}
            OriginalRim.__init__(self, case)
            self.uc.mem_map(self.heap, 0x100000)
        else:
            super().__init__(case)
        self.text_hash = self.hash_text()

    def hash_text(self):
        return hashlib.sha256(bytes(self.uc.mem_read(TEXT_START, TEXT_SIZE))).hexdigest()

    def observe(self, uc, address, size, data):
        if address in FORBIDDEN:
            raise AssertionError(f"Unexpected excluded native body {FORBIDDEN[address]} {address:08X}")
        if address in ENTRIES:
            self.native_entries[ENTRIES[address]] += 1
        super().observe(uc, address, size, data)

    def write(self, uc, access, address, size, value, data):
        assert address + size <= TEXT_START or address >= TEXT_START + TEXT_SIZE, 'CPU text write'
        if CELLS <= address < CELLS + len(self.ptrs) * 0x200:
            base = CELLS + ((address - CELLS) // 0x200) * 0x200
            identity = self.coords[base]
        elif DUMMY <= address < DUMMY + 0x200:
            base = DUMMY
            identity = 'dummy'
        else:
            base = None
        if base is not None:
            self.raw_writes.append(dict(cell=identity, offset=address-base, size=size,
                                        value=value & ((1 << (8 * size)) - 1)))
        super().write(uc, access, address, size, value, data)

    def raw_cells(self):
        # Every512-byte supplied Cell block, not a reconstructed Rust projection.
        return [dict(cell=list(coord), bytes=bytes(self.uc.mem_read(p, 0x200)).hex())
                for coord, p in self.ptrs.items()]

    def run(self):
        self.raw_writes.clear()
        self.native_entries.clear()
        before = self.raw_cells()
        dummy_before = bytes(self.uc.mem_read(DUMMY, 0x200)).hex()
        fpcw_before = self.uc.reg_read(UC_X86_REG_FPCW)
        result = copy.deepcopy(super().run())
        assert self.hash_text() == self.text_hash
        return dict(**result, raw_before=before, raw_after=self.raw_cells(),
                    raw_dummy_before=dummy_before,
                    raw_dummy_after=bytes(self.uc.mem_read(DUMMY, 0x200)).hex(),
                    raw_writes=copy.deepcopy(self.raw_writes),
                    native_entries=dict(self.native_entries),
                    fpcw_before=fpcw_before, fpcw_after=self.uc.reg_read(UC_X86_REG_FPCW),
                    text_sha256_unchanged=self.text_hash)


def inputs():
    for overlay in range(73, 103):
        ns = 74 <= overlay <= 82 or 92 <= overlay <= 95 or overlay == 100
        for across in (-1, 0, 1):
            cells = [[100+x, 100+y, 0xffff, 0, 0, overlay,
                      (y if ns else x)+1, None, 2, 1]
                     for x in range(-2 if ns else -1, 3 if ns else 2)
                     for y in range(-1 if ns else -2, 2 if ns else 3)]
            yield dict(name=f'wood_{overlay}_across_{across}', family='low',
                       axis=('ns' if ns else 'ew') if 74 <= overlay <= 101 else 'rejected',
                       entry=0x57BAA0, start=[100 if ns else 100+across,
                                             100+across if ns else 100],
                       size=[136,140], bridge_base=100, wood_base=500,
                       rim_keys=KEYS, cells=cells)


def generate():
    rows = [dict(input=c, result=LowWoodDamage(c).run()) for c in inputs()]
    source = next(c for c in json.loads(gzip.decompress((REPO/STOCK).read_bytes()))['cases']
                  if c['stage']=='controller' and c['input']['name']=='hut_117_56')
    sequences = []
    for x in (114, 115, 116):
        case = copy.deepcopy(source['input'])
        case.update(name=f'shrapnel_repaired_width_{x}', family='low',
                    entry=0x57BAA0, start=[x,59])
        for event in source['steps'][0]['trace']:
            if event['kind']=='overlay':
                next(row for row in case['cells'] if row[:2]==event['coord'])[5]=event['value']
        machine = LowWoodDamage(case, physical=True)
        sequences.append(dict(input=case, steps=[machine.run() for _ in range(3)]))
    return dict(schema_version=1, cases=rows, physical_sequences=sequences)


def metadata():
    result = provenance(scope=__doc__, assumptions=[
        '90 supplied finite15-cell strips: overlays73..102, with74..101 selecting both original axes, each at all3 width inputs; 73/102 are rejected controls',
        'Supplied scalar cells: tile65535/subtile0, flags0, level2, Land1, frame0/1/2 across width; remaining512-byte Cell storage starts zero. No native Cell constructor/load equivalence',
        'Three physical Shrapnel sequences project the frozen executed first-repair overlays onto its170-cell controller crop. Recalc is not run, so retained pre-repair land is not healthy-native initialization proof',
        'Original575EE0 executes on absent CellTags; original5868A0 enumerates the rectangle with successful bounded allocation/free',
        'Every raw Cell/Dummy byte before/after and ordered CPU writes are retained. All callbacks execute in original order. Executable .text hash is unchanged and CPU .text writes fail',
        'No RNG producer, structural47DD70 or caller70D4A0 may be reached. This body-boundary assertion is not a full stream/timer proof for excluded callbacks',
        'FPCW is observed from fixture reset state, not established active-retail startup. No simulated arithmetic/rounding claims are made here',
    ], substitutions=[
        'Shared repository47D2B0/487A10/56C510/586990 callbacks record and return; occupants/movement/damage/land/nav results are excluded',
        '47FDE0/47FB90 supply zero display rectangles; projection/radar/display sinks do not establish pixels',
        '7C8E17/7C8B3D supply successful bounded allocation/free; no actual CRT heap',
        'No outer489280 admission, strength RNG, full physical INI load, firing chronology, tagged notification, restore or rendering executes',
    ], entry_points={**{name:address for address,name in ENTRIES.items()},
                     'recalc_boundary':0x47D2B0,'occupants_boundary':0x487A10,
                     'connectivity_boundary':0x56C510,'hierarchy_boundary':0x586990})
    result['source_sha256'] = {STOCK: hashlib.sha256((REPO/STOCK).read_bytes()).hexdigest()}
    return result


if __name__=='__main__':
    finish_vectors(generate, HERE/'scalar.json.gz', provenance=metadata,
                   promotion_path=HERE/'scalar_promotion.json')
