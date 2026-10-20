"""Original joined Unit/Building tank-bunker entry radio histories."""
import struct
from pathlib import Path

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX,
                               UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_EIP,
                               UC_X86_REG_ESI, UC_X86_REG_ESP)
from tools.native_oracle import (STACK_BASE, STACK_SIZE, NativeCallTrace,
                                 run_checked, finish_vectors, provenance)
from tools.spatial_oracle.unit_source_scatter import SCENARIO
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.refinery_dock import (
    make_dock_fixture, ACTOR, TYPE, LOCO, BLD, BTYPE, OTHER, HOUSE,
    MINER_ITEMS, BLD_ITEMS, OTHER_ITEMS, EXTRA, SP, cell,
)

WEAPON = EXTRA + 0x29000
OUT = EXTRA + 0x29800
SECOND, SECOND_LOCO, SECOND_ITEMS = EXTRA + 0x26000, EXTRA + 0x27000, EXTRA + 0x2A100
CALLS = {
    0x741970: ('unit_destination', 2), 0x4D94B0: ('foot_destination', 2),
    0x65A970: ('transmit', 3), 0x43C2D0: ('building_receive', 3),
    0x737430: ('unit_receive', 3), 0x65ACB0: ('contact_message', 1),
    0x4D9290: ('mission_enter', 0), 0x70FB50: ('can_enter_bunker', 0),
    0x447B20: ('dock_coordinate', 2), 0x5B35E0: ('queue_mission', 2),
    0x5B3570: ('commence', 0), 0x4D85D0: ('foot_per_cell', 1),
    0x55A910: ('power_off', 1), 0x4AFD40: ('drive_move', 4),
    0x458E50: ('update_bunker', 0), 0x738970: ('unit_idle', 2),
    0x65C7E0: ('random_range', 2),
    0x4DFF40: ('seek_bunker', 0),
    0x4AFB80: ('drive_is_moving', 1), 0x4AFE00: ('drive_stop', 1),
    0x447AC0: ('building_center', 1),
}
INPUTS = dict(
    map=dict(allocated=[32,32], width=16, local=[-16,-16,64,64], level=0,
             clear_speed_rows=[1.0]*9, water_speed_rows=[0.0]*9,
             supplied_building_cell_lists=True, units_in_cell_lists=False),
    unit=dict(type='PreparedTank', class_id=1, strength=150, speed_type=1, speed_type_name='Track',
              movement_zone=0, turret=True, bunkerable=True, weapon_slot0=True,
              mega_mission=-1, rot=5, facing=49152, nav_queue_count=0,
              nav_aux=None, path=[-1,0,0,0]),
    bunker=dict(type='PreparedBunker', foundation=3, dimensions=[2,2], strength=900,
                bunker=True, docks=1, location=[8,8], other_location=[20,20]),
    scenario=dict(seed=1, frame=200, game_mode=1),
    mission_rates=dict(guard=0.03, enter=0.016),
)


class Fixture:
    def __init__(self, case):
        self.case = case
        self.u, self.call, self.r = make_dock_fixture(dict(
            linked=False, mission='enter', miner_cell=case.get('unit_cell', [14, 14]),
            harvester=False, dock_unload=False, refinery_flag=False,
            west_building=False, human=case.get('human', True)))
        u = self.u
        # Constructor sentinels and live-object fields; actual objects are supplied.
        u.reg_write(UC_X86_REG_ESI, ACTOR)
        u.reg_write(UC_X86_REG_EBX, 0)
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, 0x4D32EC, 0x4D3308, required_addresses=(0x4D32EC,))
        for off in (0xB0, 0x520, 0x6CC):
            u.mem_write(ACTOR + off, dwords(-1))
        u.mem_write(ACTOR + 0x14, dwords(7))
        u.mem_write(ACTOR + 0x90, b'\x01')
        u.mem_write(ACTOR + 0x6C, dwords(150))
        u.mem_write(ACTOR + 0x5A0, dwords(0))
        u.mem_write(ACTOR + 0x5E0, dwords(-1,0,0,0))
        u.mem_write(TYPE + 0xA0, dwords(150))
        u.mem_write(TYPE + 0xD2E, b'\x01')  # Supplied Bunkerable=yes.
        u.mem_write(TYPE + 0xCA1, b'\x01')  # LTNK Turret=yes.
        u.mem_write(TYPE + 0x67C, dwords(1))  # Track.
        u.mem_write(TYPE + 0x898, dwords(0 if case.get('unarmed') else WEAPON))
        u.mem_write(ACTOR + 0xAC, dwords(case.get('mission', 7)))
        u.mem_write(BTYPE, dwords(0x7E4570))
        u.mem_write(BTYPE + 0xEF0, dwords(3))  # NATBNK Foundation=2x2.
        u.mem_write(BTYPE + 0x16AB, b'\x01')
        for obj, xy in ((BLD, (8, 8)), (OTHER, (20, 20))):
            u.mem_write(obj + 0x14, dwords(3))
            u.mem_write(obj + 0x90, b'\x01')
            u.mem_write(obj + 0x9C, dwords(xy[0]*256+128, xy[1]*256+128, 0))
            u.mem_write(obj + 0xB0, dwords(-1))
            for y in range(xy[1], xy[1]+2):
                for x in range(xy[0], xy[0]+2):
                    u.mem_write(cell(x,y)+0xE4, dwords(obj))
        # A second ordinary Unit supports the busy control through the same
        # original setter/HELLO history; no contact/admission answer is supplied.
        u.mem_write(SECOND, bytes(u.mem_read(ACTOR, 0x700)))
        u.mem_write(SECOND + 0x9C, dwords(3968,3712,0))
        u.mem_write(SECOND + 0xE4, dwords(SECOND_ITEMS))
        self.call(0x4AF540, SECOND_LOCO, [])
        u.mem_write(SECOND_LOCO+0xC, dwords(SECOND))
        u.mem_write(SECOND_LOCO+0x14, dwords(1))
        u.mem_write(SECOND+0x674, dwords(SECOND_LOCO+4))
        # This supplied owned-bunker vector is consumed by original4DFF40.
        u.mem_write(HOUSE+0xCC, dwords(EXTRA+0x2A200, 2))
        u.mem_write(HOUSE+0xD8, dwords(2))
        u.mem_write(EXTRA+0x2A200, dwords(BLD,OTHER))
        self.reached, self.steps = set(), []
        self.trace = NativeCallTrace(u, self.r)
        self.text = bytes(u.mem_read(0x401000, 0x3E0000))
        self.tables = [(address,bytes(u.mem_read(address,0x600)))
                       for address in (0x7F5C70,0x7F6218,0x7E3EBC,0x7E4570)]
        self.writes = []
        self.phase = 'setup'
        u.hook_add(UC_HOOK_CODE, self.observe)
        u.hook_add(UC_HOOK_MEM_WRITE, self.written)
        self.before = self.state()

    def name(self, pointer):
        if pointer in (0, ACTOR, BLD, OTHER, SECOND):
            return {0: None, ACTOR: 'unit', BLD: 'bunker', OTHER: 'other', SECOND: 'second_unit'}[pointer]
        from tools.spatial_oracle.refinery_dock import cell_xy
        return cell_xy(pointer)

    def ints(self, address, count=1):
        values = struct.unpack('<'+'i'*count, self.u.mem_read(address, 4*count))
        return values[0] if count == 1 else list(values)

    def state(self):
        u, r = self.u, self.r
        return dict(unit_mission=self.ints(ACTOR+0xAC), unit_queued=self.ints(ACTOR+0xB4),
                    unit_nav=self.name(r(ACTOR+0x5A4)), unit_contact=self.name(r(MINER_ITEMS)),
                    bunker_contact=self.name(r(BLD_ITEMS)), other_contact=self.name(r(OTHER_ITEMS)),
                    bunker_mission=self.ints(BLD+0xAC), bunker_queued=self.ints(BLD+0xB4),
                    bunker_state=self.ints(BLD+0x718), unit_coord=self.ints(ACTOR+0x9C,3),
                    drive_destination=self.ints(LOCO+0x34,3), drive_head=self.ints(LOCO+0x40,3),
                    drive_power=u.mem_read(LOCO+0x10,1)[0], mega_mission=self.ints(ACTOR+0x5C4),
                    unit_tether=u.mem_read(ACTOR+0x418,1)[0], bunker_tether=u.mem_read(BLD+0x418,1)[0],
                    archive=self.name(r(ACTOR+0x218)), pending_entry=self.name(r(ACTOR+0x500)),
                    second_nav=self.name(r(SECOND+0x5A4)), second_contact=self.name(r(SECOND_ITEMS)),
                    unit_bunker=self.name(r(ACTOR+0x2E4)), bunker_occupant=self.name(r(BLD+0x2E4)),
                    nav_aux=self.name(r(ACTOR+0x5A0)), path=self.ints(ACTOR+0x5E0,4),
                    drive_track=self.ints(LOCO+0x58), drive_cursor=self.ints(LOCO+0x5C),
                    rng_indices=[r(SCENARIO+0x21C),r(SCENARIO+0x220)])

    def observe(self, u, pc, size, data):
        self.reached.add(pc)
        assert pc not in (0x4B0500, 0x4D3920), 'movement/pathfinding is outside entry history'
        sp = u.reg_read(UC_X86_REG_ESP)
        self.trace.returned(pc, sp)
        if pc in CALLS:
            name, argc = CALLS[pc]
            self.trace.entered(pc,sp,(name,argc,argc*4))

    def written(self,u,access,address,size,value,data):
        mutable = [(STACK_BASE,STACK_SIZE),(ACTOR,0x700),(SECOND,0x700),
                   (LOCO,0x100),(SECOND_LOCO,0x100),(BLD,0x800),(OTHER,0x800),
                   (MINER_ITEMS,4),(BLD_ITEMS,4),(OTHER_ITEMS,4),(SECOND_ITEMS,4),
                   (SCENARIO+0x218,0x3F4),(OUT,12),(0xA8EC30,4),(0xABCD3C,4),
                   (0xA83DBC,4)]  # Original Foot PerCell empty team-list cursor.
        assert any(start<=address and address+size<=start+length for start,length in mutable), (
            self.phase,hex(u.reg_read(UC_X86_REG_EIP)),hex(address),size)
        if not STACK_BASE <= address < STACK_BASE+STACK_SIZE:
            self.writes.append(dict(pc=hex(u.reg_read(UC_X86_REG_EIP)), address=hex(address),
                                    size=size,value=value))

    def step(self, op):
        self.phase = op
        before = self.state()
        trace_start = len(self.trace.calls)
        write_start = len(self.writes)
        self.reached.clear()
        answer = None
        if op == 'set_destination':
            self.call(0x741970, ACTOR, [BLD, 1])
        elif op == 'enter':
            self.call(0x4D9290, ACTOR, [])
            answer = self.ints_value(self.u.reg_read(UC_X86_REG_EAX))
        elif op == 'other_set_destination':
            self.call(0x741970, SECOND, [BLD,1])
        elif op == 'replace_destination':
            self.call(0x741970, ACTOR, [OTHER,1])
        elif op == 'radio_docking':
            self.call(0x65A970, ACTOR, [14,0xA8EC30,BLD])
            answer = self.u.reg_read(UC_X86_REG_EAX)
        elif op == 'prepare_state3_destination':
            # Deliberate prepared state3 control, not a ForceTrack completion.
            self.call(0x447AC0,BLD,[OUT])
            center=bytes(self.u.mem_read(OUT,12))
            self.u.mem_write(ACTOR+0x9C,center)
            self.u.mem_write(LOCO+0x34,center)
            self.u.mem_write(BLD+0x718,dwords(3))
        elif op == 'seek_bunker':
            self.call(0x4DFF40, ACTOR, [])
            answer = self.u.reg_read(UC_X86_REG_EAX)&255
        elif op == 'commence':
            self.call(0x5B3570, ACTOR, [])
        elif op == 'update_bunker':
            self.call(0x458E50, BLD, [])
        elif op == 'idle_event_calls':
            self.u.reg_write(UC_X86_REG_ESI, ACTOR)
            self.u.reg_write(UC_X86_REG_EDI, 0)
            self.u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(self.u, 0x4C75DA, 0x4C762A,
                        required_addresses=(0x4C75E0,0x741970,0x4C75F8))
        elif op == 'dock_coordinate':
            self.call(0x447B20, BLD, [OUT, ACTOR])
        elif op == 'supply_arrival':
            self.u.mem_write(ACTOR+0x9C, bytes(self.u.mem_read(OUT,12)))
        elif op == 'per_cell':
            frame = SP-0x100
            self.u.mem_write(frame+0x14, dwords(self.r(MINER_ITEMS)))
            self.u.mem_write(frame+0x1C, struct.pack('<hh', *[x//256 for x in self.ints(ACTOR+0x9C,3)[:2]]))
            self.u.reg_write(UC_X86_REG_EBP, ACTOR)
            self.u.reg_write(UC_X86_REG_ESP, frame)
            run_checked(self.u, 0x73A31F, (0x73A540,0x73A5EA), required_addresses=(0x73A31F,))
        elif op == 'cancel_destination':
            self.call(0x741970, ACTOR, [0, 1])
        else:
            raise ValueError(op)
        self.trace.returned(self.u.reg_read(UC_X86_REG_EIP),self.u.reg_read(UC_X86_REG_ESP))
        assert not self.trace.pending, self.trace.pending
        required = {
            'set_destination': (0x741970,0x4D94B0), 'other_set_destination': (0x741970,0x65A970,0x43C2D0,0x4D94B0,0x4AFD40),
            'replace_destination': (0x741970,0x65A970,0x43C2D0,0x4D94B0,0x4AFD40),
            'enter': (0x4D9290,0x65C7E0), 'radio_docking': (0x65A970,0x43C2D0),
            'dock_coordinate': (0x447B20,), 'seek_bunker': (0x4DFF40,0x70FB50,0x741970,0x65A970,0x5B35E0),
            'commence': (0x5B3570,), 'update_bunker': (0x458E50,),
            'idle_event_calls': (0x4C75E0,0x741970,0x4C75F8),
            'cancel_destination': (0x741970,0x4D94B0), 'per_cell': (0x73A31F,0x447B20),
        }.get(op,())
        assert set(required)<=self.reached, (op,set(required)-self.reached)
        if op == 'update_bunker' and before['bunker_state'] == 3:
            assert {0x4591D6,0x4AFB80,0x459210}<=self.reached
            assert not {0x741970,0x4AFE00,0x459216} & self.reached
            required += (0x4591D6,0x4AFB80,0x459210)
        if op == 'per_cell':
            arrived = before['unit_coord'][:2] == self.ints(OUT,3)[:2]
            assert (0x73A507 in self.reached) == arrived
            if arrived:
                assert {0x4D85D0,0x65A970,0x43C2D0,0x5B35E0,0x55A910}<=self.reached
        radio = [dict(sender=self.name(int(call['ecx'],16)),
                      recipient=self.name(call['args'][2]), message=call['args'][0],
                      reply=call['return_eax'])
                 for call in self.trace.calls[trace_start:] if call['name'] == 'transmit']
        step = dict(op=op, before=before, after=self.state(), radio=radio,
                    native_write_count=len(self.writes)-write_start,
                    required_reached=[hex(x) for x in required],
                    reached=[hex(x) for x in sorted(self.reached & set(CALLS))])
        if answer is not None:
            step['return_value'] = answer
        if op in ('dock_coordinate','supply_arrival','prepare_state3_destination'):
            step['coordinate'] = self.ints(OUT,3)
        self.steps.append(step)
        assert bytes(self.u.mem_read(0x401000, 0x3E0000)) == self.text
        assert all(bytes(self.u.mem_read(address,len(raw))) == raw for address,raw in self.tables)
        return self.steps[-1]

    @staticmethod
    def ints_value(value):
        return struct.unpack('<i',dwords(value))[0]


def cases():
    return [
        (dict(name='far_enter_then_supplied_arrival'),
         ['set_destination','enter','dock_coordinate','per_cell','supply_arrival','per_cell']),
        (dict(name='busy_claim_from_original_second_setter'), ['other_set_destination','set_destination']),
        (dict(name='admitted_idle_event_releases_claim'), ['set_destination','idle_event_calls','update_bunker','other_set_destination']),
        (dict(name='replace_bunker_destination'), ['set_destination','replace_destination']),
        (dict(name='unarmed_fresh_then_contacted_docking',unarmed=True), ['radio_docking','radio_docking']),
        (dict(name='ai_seek_nearest_free',mission=5,human=False), ['seek_bunker','enter']),
        (dict(name='null_destination_retains_radio_contact'), ['set_destination','cancel_destination']),
        (dict(name='prepared_state3_live_drive_destination'),
         ['set_destination','cancel_destination','prepare_state3_destination','update_bunker']),
    ]


def generate():
    rows=[]
    for case,ops in cases():
        case=dict(dict(unit_cell=[14,14],mission=7,human=True,unarmed=False),**case)
        f=Fixture(case)
        for op in ops:
            f.step(op)
        rows.append(dict(input=case,before=f.before,steps=f.steps,
                         original_text_and_vtables_unchanged=True))
    rates = rate_receipts()
    return dict(schema=1,inputs=dict(INPUTS,mission_rate_conversions=rates),cases=rows)


def rate_receipts():
    """Original rate load/multiply/ftol, stopped before Enter's RNG request."""
    results = {}
    for name, mission in (('guard',5),('enter',7)):
        u, call, read32 = make_dock_fixture(dict(harvester=False,linked=False))
        u.mem_write(ACTOR+0xAC,dwords(mission))
        u.reg_write(UC_X86_REG_ESI,ACTOR)
        u.reg_write(UC_X86_REG_ESP,SP)
        before_rng = bytes(u.mem_read(SCENARIO+0x218,0x3F4))
        run_checked(u,0x4D946C,0x4D9481,
                    required_addresses=(0x5B3A00,0x4D9473,0x4D9476,0x7C5F00))
        assert bytes(u.mem_read(SCENARIO+0x218,0x3F4)) == before_rng
        assert u.reg_read(UC_X86_REG_ESP) == SP
        results[name] = dict(rate_bits=bytes(u.mem_read(0xA8E3A8+mission*32+0x10,8)).hex(),
                            frames=Fixture.ints_value(u.reg_read(UC_X86_REG_EAX)),
                            begin='0x4d946c',stop='0x4d9481',rng_unchanged=True)
    return results


def metadata():
    return provenance(
        scope='Original joined Unit destination, bunker radio, Enter cadence, bounded PerCell DOCK_NOW, cancellation and AI seek histories',
        entry_points=dict(unit_destination=0x741970,foot_destination=0x4D94B0,
            radio_transmit=0x65A970,building_receive=0x43C2D0,mission_enter=0x4D9290,
            dock_coordinate=0x447B20,per_cell_begin=0x73A31F,per_cell_return=0x73A540,
            idle_event_calls=0x4C75DA,idle_event_stop=0x4C762A,
            update_bunker=0x458E50,seek_bunker=0x4DFF40),
        assumptions=[
            'Synthetic mapped32x32 flat scene from make_dock_fixture; declared map bounds and supplied original Cell/Unit/Building/Type tables. Two live ordinary Units and two2x2 bunker buildings share one House. Building Cell lists are supplied, not Mark/Unlimbo outputs. Units are absent from Cell object lists; the declared arrival changes raw XYZ only. No physical scenario, INI reader or full object constructor claim.',
            'PreparedTank uses supplied health/strength150, Turret=yes, Bunkerable=yes, Track1, Normal0, one nonnull current WeaponType. Weapon contents are irrelevant to CanEnterBunker, which checks existence only. PreparedBunker uses Bunker=yes, Foundation2x2/index3, NumberOfDocks1, health/strength900. These are explicit kernel inputs, not complete retail LTNK/NATBNK type bindings or original INI-reader outputs.',
            'Original Drive/Facing constructors, ROT5, ScenarioRandom seed1 and direction/height startup execute. Original Foot constructor4D32EC..4D3308 establishes saved MegaMission=-1 and empty payload. Other unused object storage starts zero; actor alive, abstract flags7, Building flags3, queued/suspended mission-1 and selected Unit/Foot sentinels are supplied explicitly. Inherited NavAux/path are explicitly replaced by null/[-1,0,0,0]; snapshots retain these and Drive selector/cursor.',
            'Rates are supplied binary64 .016 Enter/.030 Guard table fields, not original INI binary32-to-binary64 parsing. Full original Enter handler and ScenarioRandomRanged execute. The frame stays200; this is callback cadence output, not scheduler timing.',
            'Busy is produced by the second Unit original setter; no contact or admission answer is fabricated. AI uses a supplied owned-bunker vector but original4DFF40 computes distance and eligibility and issues setter/HELLO/forcedEnter. Cancellation starts at admitted Idle-event calls4C75DA with ESI=Unit and EDI=0; event lookup, mission/tether admission and the later event tail are excluded. Bare NULL setter is a separate control.',
            'Far-to-near case supplies only Unit XYZ equal to the original GetDockCoord return, preserving all other state. Original PerCell Enter arm gets EBP=Unit, stack+14=retained Contact0 and stack+1C=current cell as its omitted prologue would produce, and stops at73A540 before epilogue or73A5EA before unrelated Unit tail. No Drive Process, pathfinding, physical arrival, full bunker installation, animation, parked position or installed destination retirement is claimed.',
            'Prepared state3 control retains the original setter/NULL-setter radio history but explicitly supplies bunker state3 and Unit XYZ/Drive destination at the original Building center. It has no owner NavCom/head and constructor selector-1. Full UpdateBunker must reach original IsMoving4AFB80 and branch459210, without StopMoving or Unit setter; this proves only the prepared state3 waiting gate, not reachability after ForceTrack completion.',
            'Every native invocation uses bounded run_checked completion. Nested calls are matched by return PC plus callee-cleaned stack. Required callees and positive/negative DOCK_NOW reachability are asserted, writes must stay in declared mutable state/stack/output regions, original text and four class vtables remain unchanged.',
        ],
        substitutions=['Inherited platform InterlockedIncrement/Decrement imports implement their integer operation. No gameplay callable, radio reply, admission predicate, destination setter, coordinate getter, RNG or mission handler is substituted.'],
    )


if __name__ == '__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
