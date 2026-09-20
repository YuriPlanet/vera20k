"""Original BuildingType+84 cost used by House base-center weighting."""
from pathlib import Path
import struct
from tools.native_oracle import finish_vectors, call, provenance, SCRATCH
from tools.spatial_oracle.map_queries import dwords
HOUSE = SCRATCH
COUNTRY = SCRATCH + 24576
BUILDING = SCRATCH + 28672
FREE = SCRATCH + 36864
RULES = SCRATCH + 45056
PAD = SCRATCH + 53248
DOCK = SCRATCH + 61440

def query(row):
    writes = {HOUSE + 52: dwords(COUNTRY), BUILDING: dwords(8275312), FREE: dwords(8348184), BUILDING + 1552: dwords(row['cost']), BUILDING + 3744: dwords(FREE if row['free'] else 0), FREE + 1552: dwords(row['free']), 8942048: dwords(RULES), RULES + 2908: dwords(PAD), RULES + 6120: b'\x01', PAD: dwords(PAD + 256, PAD + 256), PAD + 256 + 1004: dwords(DOCK), DOCK: dwords(0), COUNTRY + 276: struct.pack('<5f', 1, row['country_unit'], 1, row['country_building'], 1), HOUSE + 21392: struct.pack('<5f', 1, row['unit'], 1, row['building'], 1)}
    result = call(4582864, ecx=BUILDING, stack_args=[HOUSE], writes=writes, required_addresses=[4582864, 7413504, 5291504, 5291696, 4582736, 7413424], timeout_instr=10000)
    return dict(input=row, cost84=struct.unpack('<i', struct.pack('<I', result['eax']))[0])

def generate():
    rows = []
    for cost, free in ((1500, 0), (2000, 1400), (1000, 1400)):
        for building, unit in ((1, 1), (1, 0.75), (0.75, 1), (0.75, 0.75)):
            row = dict(cost=cost, free=free, building=building, unit=unit, country_building=1, country_unit=1)
            rows.append(query(row))
    # The shared finite loader now owns these values too. Exercise original
    # multiply/ftol through the cost receiver, including the optional FreeUnit.
    for bits in (0x00000000, 0x80000000, 0x00000001, 0x80000001,
                 0x007FFFFF, 0x807FFFFF, 0x00800000, 0x80800000):
        factor = struct.unpack('<f', struct.pack('<I', bits))[0]
        for cost, free in ((2147483647, 0), (-2147483648, 0), (2000, 1400)):
            rows.append(query(dict(cost=cost, free=free, building=factor, unit=factor,
                                   country_building=1, country_unit=1)))
    return rows
if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='original BuildingType virtual+84 callback45EDD0, Techno711F00 and '
              'actualcost+AC45ED50. Declared type costs/free unit and House cost '
              'multipliers; stock SeparateAircraft=true. No producer for live '
              'multipliers/country fields, full House recalc, or Rust parity claim.',
        assumptions=[
            'Original BuildingTypeVT7E4570 and UnitTypeVT7F6218; native Cost+610 '
            'and FreeUnit+EA0 populated directly. Supplied HouseType cost '
            'fields114..124 and House5390..53A0 floats. Unused object storagezero; BuildingBuildCat0.',
            'PadAircraft first pointer and Dock first pointer valid but do not '
            'equal Building; supplied SeparateAircrafttrue excludes legacy '
            'bundled-aircraft adjustment. No null/malformed configuration claim.',
            'Twelve existing normal-factor rows are retained. Twenty-four added '
            'rows cross signed zero, minimum/maximum subnormal and minimum normal '
            'factors with extreme i32 costs or an optional FreeUnit. Raw input '
            'bits are decoded for fixture storage only; original instructions '
            'produce all expected integer costs. These are arithmetic-domain '
            'fixtures, not assertions about stock rule values.',
        ], substitutions=[], entry_points={
            'building_cost84': 4582864, 'techno_cost84': 7413504,
            'country_factor': 5291504, 'house_factor': 5291696,
            'building_actual_cost': 4582736, 'raw_cost': 7413424}))
