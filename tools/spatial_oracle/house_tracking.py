"""Original HouseClass::Add_Tracking 0x004FF700 and Remove_Tracking 0x004FF550.

Each case builds one Techno of a fake type and calls the original function on a
house once. The routing into the house's plain counters executes: +0x2E8
(units, and unit-like buildings), +0x2EC (naval units with no Passengers=),
+0x2F0 (buildings, the short-game defeat count), +0x2F4 (infantry, latched by
the Techno's +0x438), +0x2F8 (aircraft), the Techno's tracked flag +0x3CC and
the infantry latch. The per-type counters (IndexClass Increment 0x0049FA00 /
Decrement 0x0049FA70 on house +0x5500 building, +0x5514 unit, +0x5528
infantry, +0x553C aircraft) and the owned-type set 0x00749020 are recorded at
entry and not run; so are the virtuals GetTechnoType (+0x84), WhatAmI (+0x2C),
the building pre-check (+0x80) and the type's ArrayIndex (+0x40).

Rust consumer: src/sim/house_tracking_tests.rs.
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_FPCW

from tools.native_oracle import (load_image, run_checked, SCRATCH, STACK_BASE, STACK_SIZE,
                                 RET_MAGIC, NATIVE_FPCW, finish_vectors, provenance)
from tools.spatial_oracle.map_queries import dwords

REGION = 0x10000
SP = STACK_BASE + STACK_SIZE - 0x1000
ADD_TRACKING, REMOVE_TRACKING = 0x4FF700, 0x4FF550
HOUSE = SCRATCH                     # counters up to +0x5600
TECHNO = SCRATCH + 0x6000
TECHNO_VT = SCRATCH + 0x7000
TYPE = SCRATCH + 0x8000
TYPE_VT = SCRATCH + 0x9800
UNDEPLOYS = SCRATCH + 0xA000
STUBS = SCRATCH + 0xC000
VIRTUALS = {0x84: "get_type", 0x2C: "what_am_i", 0x80: "precheck"}
TYPE_VIRTUALS = {0x40: "array_index"}
NATIVES = {0x49FA00: "increment", 0x49FA70: "decrement", 0x749020: "owned_type"}
COUNTERS = {0x5500: "buildings", 0x5514: "units", 0x5528: "infantry", 0x553C: "aircraft",
            0x1B40: "building_types", 0x1338: "unit_types", 0xB30: "infantry_types",
            0x328: "aircraft_types"}
PLAIN = {"units": 0x2E8, "naval": 0x2EC, "buildings": 0x2F0, "infantry": 0x2F4,
         "aircraft": 0x2F8}
TYPE_INDEX = 7


class Fixture:
    def __init__(self):
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.u)
        self.u.mem_map(SCRATCH, REGION)
        self.u.mem_map(STACK_BASE, STACK_SIZE)
        self.u.mem_map(RET_MAGIC, 0x1000)
        self.u.hook_add(UC_HOOK_CODE, self.observe)

    def word(self, address):
        return struct.unpack("<I", self.u.mem_read(address, 4))[0]

    def signed(self, address):
        return struct.unpack("<i", self.u.mem_read(address, 4))[0]

    def ret(self, value, cleaned):
        sp = self.u.reg_read(UC_X86_REG_ESP)
        self.u.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        self.u.reg_write(UC_X86_REG_EIP, self.word(sp))
        self.u.reg_write(UC_X86_REG_ESP, sp + 4 + cleaned)

    def observe(self, u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        this = u.reg_read(UC_X86_REG_ECX)
        if STUBS <= pc < STUBS + 0x100:
            name = VIRTUALS[pc - STUBS]
            if name == "get_type":
                self.ret(TYPE, 0)
            elif name == "what_am_i":
                self.ret(self.case["rtti"], 0)
            else:
                self.ret(self.case.get("precheck", 0), 0)
        elif STUBS + 0x100 <= pc < STUBS + 0x200:
            self.ret(TYPE_INDEX, 0)
        elif pc in NATIVES:
            index = self.word(sp + 4)
            counter = COUNTERS.get(this - HOUSE, hex(this - HOUSE))
            self.events.append([NATIVES[pc], counter, index])
            self.ret(0, 4)

    def execute(self, case):
        u = self.u
        self.case = case
        self.events = []
        u.mem_write(SCRATCH, bytes(REGION))
        u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        for offset in VIRTUALS:
            u.mem_write(STUBS + offset, b"\xCC")
            u.mem_write(TECHNO_VT + offset, dwords(STUBS + offset))
        u.mem_write(STUBS + 0x100 + 0x40, b"\xCC")
        u.mem_write(TYPE_VT + 0x40, dwords(STUBS + 0x100 + 0x40))
        u.mem_write(TECHNO, dwords(TECHNO_VT))
        u.mem_write(TYPE, dwords(TYPE_VT))
        u.mem_write(TYPE + 0x232, bytes([case.get("insignificant", 0)]))
        u.mem_write(TYPE + 0xC9F, bytes([case.get("dont_score", 0)]))
        u.mem_write(TYPE + 0xCCE, bytes([case.get("naval", 0)]))
        u.mem_write(TYPE + 0x5E0, struct.pack("<i", case.get("passengers", 0)))
        # A building's type pointer (+0x520) and its UndeploysInto (+0x408).
        u.mem_write(TECHNO + 0x520, dwords(TYPE))
        if "undeploys_into" in case:
            u.mem_write(TYPE + 0x408, dwords(UNDEPLOYS))
            u.mem_write(UNDEPLOYS + 0x5EC, bytes([case["undeploys_into"].get("resource_gatherer", 0)]))
        u.mem_write(TECHNO + 0x438, bytes([case.get("latched", 0)]))
        u.mem_write(TECHNO + 0x439, bytes([case.get("byte_439", 0)]))
        for name, offset in PLAIN.items():
            u.mem_write(HOUSE + offset, struct.pack("<i", case.get("start", 0)))
        u.mem_write(SP, dwords(RET_MAGIC) + dwords(TECHNO))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, HOUSE)
        entry = ADD_TRACKING if case["call"] == "add" else REMOVE_TRACKING
        run_checked(u, entry, RET_MAGIC, count=50_000)
        counts = {name: self.signed(HOUSE + offset) - case.get("start", 0)
                  for name, offset in PLAIN.items()}
        return dict(input=case, events=self.events, counts=counts,
                    tracked=u.mem_read(TECHNO + 0x3CC, 1)[0],
                    latched=u.mem_read(TECHNO + 0x438, 1)[0])


def inputs():
    cases = []
    kinds = [
        dict(name="unit", rtti=1),
        dict(name="unit_insignificant", rtti=1, insignificant=1),
        dict(name="unit_dont_score", rtti=1, dont_score=1),
        dict(name="naval_no_passengers", rtti=1, naval=1),
        dict(name="naval_transport", rtti=1, naval=1, passengers=3),
        dict(name="aircraft", rtti=2),
        dict(name="aircraft_dont_score", rtti=2, dont_score=1),
        dict(name="building", rtti=6),
        dict(name="building_insignificant", rtti=6, insignificant=1),
        dict(name="building_dont_score", rtti=6, dont_score=1),
        dict(name="building_1x1_undeployer", rtti=6, precheck=1),
        dict(name="building_undeploys_to_gatherer", rtti=6, undeploys_into=dict(resource_gatherer=1)),
        dict(name="building_undeploys_to_other", rtti=6, undeploys_into=dict(resource_gatherer=0)),
        dict(name="infantry", rtti=15),
        dict(name="infantry_latched", rtti=15, latched=1),
        dict(name="infantry_byte_439", rtti=15, byte_439=1),
        dict(name="infantry_dont_score", rtti=15, dont_score=1),
        dict(name="other_rtti", rtti=3),
    ]
    for call in ("add", "remove"):
        for kind in kinds:
            cases.append(dict(kind, name=f"{call}_{kind['name']}", call=call, start=5))
    return cases


def generate():
    fixture = Fixture()
    return [fixture.execute(case) for case in inputs()]


def metadata():
    return provenance(
        scope="Original 0x004FF700 HouseClass::Add_Tracking and 0x004FF550 HouseClass::Remove_Tracking",
        assumptions=[
            "House +2E8 units, +2EC naval without passengers, +2F0 buildings, +2F4 infantry, +2F8 aircraft; per-type counters +5500/+5514/+5528/+553C; owned-type sets +1B40/+1338/+B30/+328",
            "Techno +3CC tracked, +438 infantry latch, +439, +520 building type; TechnoType +232 Insignificant, +C9F DontScore, +CCE Naval, +5E0 Passengers, +408 UndeploysInto, +5EC ResourceGatherer",
        ],
        substitutions=[
            "Recorded at entry and returned without running: GetTechnoType (+84), WhatAmI (+2C), the building pre-check (+80, the case's value), the type's ArrayIndex (+40, 7), IndexClass Increment 0x0049FA00 / Decrement 0x0049FA70, the owned-type set 0x00749020",
        ],
        entry_points={"add_tracking": ADD_TRACKING, "remove_tracking": REMOVE_TRACKING})


class NominalFixture(Fixture):
    """Independent opt-in on the existing VM; real active CounterClass bodies."""
    OTHER_HOUSE = SCRATCH + 0x40000

    def __init__(self, *, nominal=False, dont_score=False, start=0):
        super().__init__()
        from unicorn.x86_const import UC_X86_REG_EBX, UC_X86_REG_ESI
        from tools.spatial_oracle.building_construction import invoke
        self.u.mem_map(SCRATCH + REGION, 0x50000)
        self.case = dict(rtti=15)
        self.events, self.rng_calls = [], []
        self.native_counters = True
        self.u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        self.u.mem_write(TECHNO, dwords(TECHNO_VT))
        self.u.mem_write(TYPE, dwords(TYPE_VT))
        self.u.mem_write(TECHNO + 0x6C0, dwords(TYPE))
        self.u.mem_write(TECHNO + 0x21C, dwords(HOUSE))
        for offset in VIRTUALS:
            self.u.mem_write(TECHNO_VT + offset, dwords(STUBS + offset))
        self.u.mem_write(TYPE_VT + 0x40, dwords(STUBS + 0x140))
        # Original constructor byte store; EBX=0 is its established prior.
        self.u.mem_write(TECHNO + 0x6D9, b'\xCC')
        self.u.reg_write(UC_X86_REG_ESI, TECHNO)
        self.u.reg_write(UC_X86_REG_EBX, 0)
        run_checked(self.u, 0x517A98, 0x517A9E)
        self.constructor_default = self.flag()
        if nominal:
            self.mark_death(True, True)
        self.u.mem_write(TYPE + 0xC9F, bytes([dont_score]))
        for n, house in enumerate((HOUSE, self.OTHER_HOUSE)):
            items = SCRATCH + 0x28000 + n * 0x100
            self.u.mem_write(house + 0x5578, dwords(0x7E449C, items, 16, 1, start))
            self.u.mem_write(items + TYPE_INDEX * 4, dwords(start))
            self.u.mem_write(house + 0x160A8, dwords(37, -91, 55))
        self.rngs = [SCRATCH + 0x30000 + n * 0x1000 for n in range(3)]
        for p in self.rngs:
            invoke(self.u, 0x65C6D0, p, 31)

    def observe(self, u, pc, size, data):
        if pc in (0x65C780, 0x65C7E0):
            self.rng_calls.append(f'{pc:08X}')
        if getattr(self, 'native_counters', False) and pc in (0x49FA00, 0x49FA70):
            self.events.append('increment' if pc == 0x49FA00 else 'decrement')
            return
        if getattr(self, 'native_counters', False) and pc in (0x49FA47, 0x49FA50, 0x49FAB7, 0x49FAC0):
            self.events.append({0x49FA47: 'increment_slot', 0x49FA50: 'increment_total',
                                0x49FAB7: 'decrement_slot', 0x49FAC0: 'decrement_total'}[pc])
        super().observe(u, pc, size, data)

    def flag(self):
        return self.u.mem_read(TECHNO + 0x6D9, 1)[0]

    def rng(self):
        import hashlib
        return [hashlib.sha256(bytes(self.u.mem_read(p, 0x3F4))).hexdigest() for p in self.rngs]

    def state(self):
        rows = {}
        for name, house in (('old', HOUSE), ('new', self.OTHER_HOUSE)):
            items = self.word(house + 0x557C)
            rows[name] = dict(slot=self.signed(items + TYPE_INDEX * 4),
                total=self.signed(house + 0x5588),
                force_values=[self.signed(house + 0x160A8 + i * 4) for i in range(3)])
        return dict(nominal=self.flag(), owner='old' if self.word(TECHNO + 0x21C) == HOUSE else 'new',
                    houses=rows)

    def mark_death(self, buildup, type_nominal):
        from unicorn.x86_const import UC_X86_REG_EBX, UC_X86_REG_ESI, UC_X86_REG_EDI
        building = SCRATCH + 0x18000
        self.u.mem_write(building + 0x6E9, bytes([buildup]))
        self.u.mem_write(TYPE + 0xC9E, bytes([type_nominal]))
        for reg, value in ((UC_X86_REG_EBX, 0), (UC_X86_REG_ESI, TECHNO),
                           (UC_X86_REG_EDI, building)):
            self.u.reg_write(reg, value)
        run_checked(self.u, 0x443111, 0x44312E)

    def active(self, operation, house=HOUSE):
        from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EDI
        count_sp = SP - 0x1000
        self.u.reg_write(UC_X86_REG_ESP, count_sp)
        self.u.reg_write(UC_X86_REG_ESI, TECHNO)
        self.u.reg_write(UC_X86_REG_EDI, house)
        entry, stops = ((0x502C3C, (0x502C7F, 0x502D04)) if operation == 'add'
                        else (0x50297D, (0x5029B2, 0x502A4C)))
        before, rng, mark = self.state(), self.rng(), len(self.events)
        stop = run_checked(self.u, entry, stops)
        after = self.state()
        assert self.u.reg_read(UC_X86_REG_ESP) == count_sp
        assert all(before['houses'][h]['force_values'] == after['houses'][h]['force_values']
                   for h in ('old', 'new'))
        assert self.rng() == rng and not self.rng_calls
        return dict(operation=operation, receiver='old' if house == HOUSE else 'new',
                    before=before, after=after, counter_calls=self.events[mark:],
                    stop=f'{stop:08X}')

    def producer(self, route, buildup, type_nominal, admitted):
        from unicorn.x86_const import UC_X86_REG_EBX, UC_X86_REG_ESI, UC_X86_REG_EDI
        before, rng = self.state(), self.rng()
        self.u.mem_write(TYPE + 0xC9E, bytes([type_nominal]))
        if route == 'death':
            self.mark_death(buildup, type_nominal)
            at_unlimbo = self.flag()
            count = self.active('add') if admitted else None
        else:
            # Pause the actual virtual call, compose its admitted counter phase,
            # restore registers only, then let the original post-return mark run.
            self.u.mem_write(TECHNO_VT + 0xD8, dwords(0x51DFF0))
            self.u.mem_write(SP, dwords(RET_MAGIC, 0, 0))
            self.u.reg_write(UC_X86_REG_ESP, SP)
            self.u.reg_write(UC_X86_REG_EDI, TECHNO)
            self.u.reg_write(UC_X86_REG_ECX, TECHNO)
            self.u.reg_write(UC_X86_REG_EAX, TECHNO_VT)
            run_checked(self.u, 0x44A72D, 0x51DFF0)
            context = self.u.context_save()
            at_unlimbo = self.flag()
            count = self.active('add') if admitted else None
            self.u.context_restore(context)
            self.ret(int(admitted), 8)
            run_checked(self.u, self.u.reg_read(UC_X86_REG_EIP), (0x44A74E, 0x44A78B))
        after = self.state()
        assert self.rng() == rng and not self.rng_calls
        return dict(input=dict(route=route, buildup=buildup, type_nominal=type_nominal,
                               unlimbo_admitted=admitted), constructor_default=self.constructor_default,
                    before=before, nominal_at_unlimbo=at_unlimbo, after=after,
                    active_admission=count, rng_before=rng, rng_after=self.rng(), rng_calls=list(self.rng_calls))

    def capture(self, limbo):
        from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_EBP
        self.u.mem_write(TECHNO + 0x81, bytes([limbo]))
        self.u.reg_write(UC_X86_REG_ESP, SP)
        self.u.reg_write(UC_X86_REG_ESI, TECHNO)
        prefix = run_checked(self.u, 0x70158A, (0x5025F0, 0x7015A2))
        phases = []
        if prefix == 0x5025F0:
            phases.append(self.active('remove', self.u.reg_read(UC_X86_REG_ECX)))
        # Constructor-tracking/kill/sight work between these original pieces
        # remains outside the active-counter component, as metadata declares.
        self.u.reg_write(UC_X86_REG_ESI, TECHNO)
        self.u.reg_write(UC_X86_REG_EBP, self.OTHER_HOUSE)
        run_checked(self.u, 0x701735, 0x70173B)
        self.u.reg_write(UC_X86_REG_ESP, SP)
        self.u.reg_write(UC_X86_REG_EDI, 0)
        tail = run_checked(self.u, 0x701757, (0x502A80, 0x701793))
        if tail == 0x502A80:
            phases.append(self.active('add', self.u.reg_read(UC_X86_REG_ECX)))
        return dict(operation='capture_limbo' if limbo else 'capture_on_map', phases=phases,
                    guard_stops=[f'{prefix:08X}', f'{tail:08X}'], after=self.state())


def nominal_physical_readers():
    """Physical file/cache/parser transport stays with retail_history."""
    from tools.rules_oracle.weapon_speed_order import retail_history
    from unicorn.x86_const import UC_X86_REG_EBX, UC_X86_REG_ESI, UC_X86_REG_EBP
    layers = [('RULESMD.INI', '[CONTROL]\nNominal=yes\n'),
              ('LANGRULE.INI', '[CONTROL]\nName=present\n'),
              ('MODE.INI', '[CONTROL]\nNominal=on\n'),
              ('MAP.MAP', '[CONTROL]\nNominal=no\n'),
              ('KEYCASE.INI', '[CONTROL]\nNOMINAL=yes\n'),
              ('EMPTY.INI', '[CONTROL]\nName=present\nNominal=\n'),
              ('OTHER.INI', '[OTHER]\nNominal=yes\n'),
              ('TRIM.INI', '[CONTROL]\nNominal= \tyeSplease \t\n')]
    class Inputs:
        def lookup(self, name): return dict(layers)[name.upper()].encode('ascii')
    m = retail_history(Inputs())
    try:
        typ = m.alloc(0x1000)
        m.u.mem_write(typ + 0x24, b'CONTROL\0')
        m.u.mem_write(typ + 0xC9E, b'\xCC')
        m.u.reg_write(UC_X86_REG_ESI, typ)
        m.u.reg_write(UC_X86_REG_EBX, 0)
        run_checked(m.u, 0x7113F0, 0x7113F6)
        default = m.u.mem_read(typ + 0xC9E, 1)[0]
        rows = []
        for name, body in layers:
            ini, _receipt = m.load_physical_ini(name)
            before = m.u.mem_read(typ + 0xC9E, 1)[0]
            m.u.reg_write(UC_X86_REG_ESP, SP)
            m.u.reg_write(UC_X86_REG_ECX, ini)
            m.u.reg_write(UC_X86_REG_EBP, typ)
            m.u.reg_write(UC_X86_REG_EBX, typ + 0x24)
            run_checked(m.u, 0x713F2C, 0x713F44, required_addresses=(0x5295F0,))
            rows.append(dict(file=name, physical_ini=body, before=before,
                             after=m.u.mem_read(typ + 0xC9E, 1)[0]))
        m.check_code()
        return dict(constructor_default=default, rows=rows)
    finally:
        m.close()


def nominal_generate():
    producers = [NominalFixture().producer(*case) for case in (
        ('death', True, True, True), ('death', False, True, True),
        ('death', True, False, True), ('death', True, True, False),
        ('sale', True, True, True), ('sale', True, False, True),
        ('sale', True, True, False))]
    histories = []
    cases = [dict(name='ordinary', operations=['add', 'remove', 'add', 'remove']),
             dict(name='marked', nominal=True, operations=['add', 'remove', 'add', 'remove']),
             dict(name='ordinary_dont_score', dont_score=True, operations=['add', 'remove']),
             dict(name='marked_dont_score', nominal=True, dont_score=True, operations=['add', 'remove']),
             dict(name='increment_wrap', start=2147483647, operations=['add']),
             dict(name='decrement_wrap', start=-2147483648, operations=['remove']),
             dict(name='on_map_capture', nominal=True, operations=['add', 'capture_on_map', 'remove_new']),
             dict(name='limbo_capture', nominal=True, operations=['capture_limbo', 'add_new', 'remove_new'])]
    for case in cases:
        f = NominalFixture(nominal=case.get('nominal', False),
                           dont_score=case.get('dont_score', False), start=case.get('start', 0))
        initial, rng, steps = f.state(), f.rng(), []
        for operation in case['operations']:
            if operation.startswith('capture'):
                steps.append(f.capture(operation == 'capture_limbo'))
            else:
                steps.append(f.active(operation.split('_')[0],
                    f.OTHER_HOUSE if operation.endswith('_new') else HOUSE))
        histories.append(dict(input=case, initial=initial, steps=steps,
                              rng_before=rng, rng_after=f.rng(), rng_calls=f.rng_calls))
    return dict(reader_history=nominal_physical_readers(), producer_rows=producers,
                count_histories=histories)


def nominal_metadata():
    return provenance(scope='Original Nominal building survivor flag producers and active-infantry count slices',
        assumptions=[
            'Infantry constructor byte517A98=false; Type Nominal constructor7113F0=false and current-default physical ReadBool713F2C..713F44. Synthetic file sequence is explicit; no whole startup/language digest/registry allocation replay.',
            'Death443111..44312E marks before declared Unlimbo admission; sale44A72D..44A74E executes the actual virtual call then marks only after an admitted return. Same VM composes active addition at that boundary. Full placement, whole death/selling, Scatter, force-value arithmetic and constructor tracking are excluded.',
            'Actual CounterClass49FA00/49FA70 execute for valid existing ArrayIndex7/capacity16. Both slot and native total+10 are retained; signed edges, repeated reveal/limbo and explicit receiving-House capture phases are inputs. Force-value sentinel region is unchanged because slices start after its update.',
            'Three native65C6D0 seed31 RandomClass states are initialized before cases. Complete-state hashes and observed Random entry list bound unchanged RNG; selected bodies contain no timer or detach work.'],
        substitutions=[
            'Inherited Fixture supplies Techno GetType and Type ArrayIndex virtual answers. Unlimbo0/1 is an explicitly supplied external admission at original51DFF0 entry; its internals do not run. Capture executes original limbo guards70158A/701757 and owner store701735, composing reached callbacks with the same count slices; intervening tracking/kill/sight work, lifecycle scheduling and native streams do not run.',
            'Physical INI loading uses existing rules_oracle.weapon_speed_order.retail_history; no second parser/VM/CounterClass implementation.'],
        entry_points=dict(type_nominal_default=0x7113F0, type_nominal_reader=0x713F2C,
            infantry_default=0x517A98, death_mark=0x443111, sale_mark=0x44A72D,
            added_infantry=0x502C3C, removed_infantry=0x50297D,
            increment=0x49FA00, decrement=0x49FA70, owner_store=0x701735,
            capture_removed_guard=0x70158A, capture_added_guard=0x701757))


if __name__ == "__main__":
    import sys
    if '--nominal-survivor' in sys.argv:
        args = [arg for arg in sys.argv[1:] if arg != '--nominal-survivor']
        finish_vectors(nominal_generate, Path(__file__).with_name('nominal_building_survivor.json'),
                       provenance=nominal_metadata, argv=args)
    else:
        finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
