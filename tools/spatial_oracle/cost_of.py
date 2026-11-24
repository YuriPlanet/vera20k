"""Original TechnoType virtual +0x84 (Cost_Of): 0x711F00 and BuildingType 0x45EDD0."""
from pathlib import Path
import struct
from tools.native_oracle import finish_vectors, call, provenance, SCRATCH
from tools.spatial_oracle.map_queries import dwords

HOUSE = SCRATCH
COUNTRY = SCRATCH + 0x6000
BUILDING = SCRATCH + 0x7000
FREE = SCRATCH + 0x9000
INFANTRY = SCRATCH + 0xA000
RULES = SCRATCH + 0xB000
PADS = SCRATCH + 0xD000
PAD1 = SCRATCH + 0xD100
PAD2 = SCRATCH + 0xD800
DOCK = SCRATCH + 0xF000
RULES_GLOBAL = 0x8871E0

# Type vtables: WhatAmI (vt+0x2C) selects the factor slot; +0x84/+0xAC are
# the cost virtuals.
VTABLE = {'building': 0x7E4570, 'unit': 0x7F6218, 'infantry': 0x7EB610,
          'aircraft': 0x7E2868}
# The type each 'techno' row calls 0x711F00 on.
TECHNO = {'unit': FREE, 'infantry': INFANTRY, 'aircraft': PAD1}
COST_OF = {'building': 0x45EDD0, 'techno': 0x711F00}


def f32(value):
    return struct.unpack('<I', struct.pack('<f', value))[0]


def factors(bits):
    """Five float fields, in slot order Infantry, Units, Aircraft, Buildings, Defenses."""
    return dwords(*bits)


def query(row):
    writes = {
        HOUSE + 0x34: dwords(COUNTRY),
        COUNTRY + 0x114: factors(row['country']),
        HOUSE + 0x5390: factors(row['plant']),
        RULES_GLOBAL: dwords(RULES),
        RULES + 0xB5C: dwords(PADS),
        RULES + 0x17E8: bytes([row['separate_aircraft']]),
        PADS: dwords(PAD1, PAD2),
        BUILDING: dwords(VTABLE['building']),
        BUILDING + 0x610: dwords(row['cost']),
        BUILDING + 0xE08: dwords(row['build_cat']),
        BUILDING + 0xEA0: dwords(FREE if row['free'] is not None else 0),
        FREE: dwords(VTABLE['unit']), FREE + 0x610: dwords(row['free'] or 0),
        INFANTRY: dwords(VTABLE['infantry']), INFANTRY + 0x610: dwords(row['cost']),
        PAD1: dwords(VTABLE['aircraft']), PAD1 + 0x3EC: dwords(DOCK),
        PAD1 + 0x610: dwords(row['pads'][0]),
        PAD2: dwords(VTABLE['aircraft']), PAD2 + 0x3EC: dwords(DOCK),
        PAD2 + 0x610: dwords(row['pads'][1]),
        DOCK: dwords(BUILDING if row['docks'] else 0),
    }
    if row['kind'] == 'techno':
        if row['techno'] == 'aircraft':
            writes[PAD1 + 0x610] = dwords(row['cost'])
        elif row['techno'] == 'unit':
            writes[FREE + 0x610] = dwords(row['cost'])
        this = TECHNO[row['techno']]
    else:
        this = BUILDING
    result = call(COST_OF[row['kind']], ecx=this, stack_args=[HOUSE if row['house'] else 0],
                  writes=writes, required_addresses=[COST_OF[row['kind']]],
                  timeout_instr=10000)
    return dict(input=row, cost=struct.unpack('<i', struct.pack('<I', result['eax']))[0])


def row(kind, cost, *, techno=None, house=True, country=None, plant=None, free=None,
        build_cat=0, separate_aircraft=1, docks=False, pads=(0, 0)):
    ones = [f32(1.0)] * 5
    return dict(kind=kind, techno=techno, cost=cost, house=house,
                country=list(country or ones), plant=list(plant or ones), free=free,
                build_cat=build_cat, separate_aircraft=separate_aircraft, docks=docks,
                pads=list(pads))


def slots(**values):
    """Distinct factor bits per slot so a wrong slot changes the result."""
    names = ('infantry', 'units', 'aircraft', 'buildings', 'defenses')
    defaults = dict(infantry=1.5, units=1.25, aircraft=2.0, buildings=1.75, defenses=3.0)
    defaults.update(values)
    return [defaults[name] if isinstance(defaults[name], int) and defaults[name] > 0xFFFF
            else f32(defaults[name]) for name in names]


def generate():
    rows = []
    # Retail inputs: Industrial Plant units 0.75, country 1.0, FreeUnit
    # refineries, SeparateAircraft=yes.
    for cost, free in ((1500, None), (2000, 1400), (1000, 1400), (1, None)):
        for building, unit in ((1.0, 1.0), (1.0, 0.75), (0.75, 1.0), (0.75, 0.75)):
            rows.append(row('building', cost, free=free,
                            plant=[f32(1.0), f32(unit), f32(1.0), f32(building), f32(1.0)]))
    # Country Cost*Mult against the plant factor, with distinct slot values;
    # 0x3F599999 is 0.85 as a chop store of the 85% double leaves it.
    for country in (slots(), slots(buildings=0x3F599999, units=0x3F59999A)):
        for plant in (slots(units=0.75), slots(buildings=0.5, defenses=0.25)):
            for cost, free, build_cat in ((2000, 1400, 0), (2500, None, 5), (777, 333, 5),
                                          (3, None, 0)):
                rows.append(row('building', cost, free=free, build_cat=build_cat,
                                country=country, plant=plant))
    # The Pad aircraft bundle (SeparateAircraft=no, Building is the first
    # pad's first Dock), with and without the House and FreeUnit.
    for house in (True, False):
        for pads, free, docks in (((1200, 1000), None, True), ((1001, 1000), None, True),
                                  ((1001, 1000), 400, True), ((1200, 1000), None, False)):
            rows.append(row('building', 500, house=house, free=free, docks=docks, pads=pads,
                            separate_aircraft=0, country=slots(), plant=slots(aircraft=0.5)))
    # A negative cost stays unclamped without a FreeUnit and clamps with one.
    for free in (None, 10):
        rows.append(row('building', -100, free=free, country=slots(), plant=slots()))
    # The other TechnoTypes (0x711F00 directly), with and without a House.
    for techno in ('infantry', 'unit', 'aircraft'):
        for cost in (600, 1400, 2147483647, -7):
            for house in (True, False):
                rows.append(row('techno', cost, techno=techno, house=house,
                                country=slots(), plant=slots(units=0.75)))
    # Extreme factor bits through the complete original arithmetic: signed
    # zero, subnormals and minimum normal against extreme costs.
    for bits in (0x00000000, 0x80000000, 0x00000001, 0x80000001,
                 0x007FFFFF, 0x807FFFFF, 0x00800000, 0x80800000):
        for cost, free in ((2147483647, None), (-2147483648, None), (2000, 1400)):
            rows.append(row('building', cost, free=free, plant=[bits] * 5))
    return [query(entry) for entry in rows]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original TechnoType virtual +0x84: TechnoTypeClass::Cost_Of 0x711F00 on '
              'Infantry, Unit and Aircraft types and the BuildingType override 0x45EDD0 '
              '(actual cost 0x45ED50, the Pad aircraft bundle, FreeUnit and its clamp), '
              'with the country (0x50BDF0) and FactoryPlant (0x50BEB0) factor getters. '
              'No claim about which callers pass which House.',
        assumptions=[
            'Original type vtables Building 0x7E4570, Unit 0x7F6218, Infantry 0x7EB610 and '
            'Aircraft 0x7E2868 supply WhatAmI and the cost virtuals. Cost +0x610, '
            'BuildCat +0xE08, FreeUnit +0xEA0, aircraft Dock items +0x3EC; House country '
            '+0x34 and factors +0x5390..+0x53A0; HouseType Cost*Mult +0x114..+0x124; '
            'Rules at [0x8871E0] with PadAircraft items +0xB5C and SeparateAircraft '
            '+0x17E8. Unused storage zero; default native FPCW 0x0E7F.',
            'Factor values are fixtures chosen to separate the five slots and cover the '
            'retail values (Industrial Plant UnitsCostBonus 0.75, country 1.0, '
            'SeparateAircraft=yes, 1400-credit FreeUnit refineries); extreme bits cross '
            'signed zero, subnormal and minimum normal factors with extreme costs. They '
            'are arithmetic-domain inputs, not a claim that every value occurs in play.',
        ], substitutions=[], entry_points={
            'building_cost_of': 0x45EDD0, 'techno_cost_of': 0x711F00,
            'country_factor': 0x50BDF0, 'house_factor': 0x50BEB0,
            'building_actual_cost': 0x45ED50, 'raw_cost': 0x711EB0}))
