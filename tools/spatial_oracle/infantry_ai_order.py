"""Original Infantry AI caller order with explicit callback boundaries.

The whole 51BAB0 caller executes with the original Infantry and Walk vtables.
The existing infantry_deploy_action fixture owns image, stack, supplied object
layout, Walk construction and Scenario seeding. Foot AI, FireAtTarget, sequencer
and movement-action bodies are observed return boundaries, not replayed gameplay.
This proves caller admission/order, not a shot, full Infantry AI or Scenario.
"""
from pathlib import Path
import hashlib
import struct

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools import native_inspect, native_oracle
from tools.native_oracle import SCRATCH, RET_MAGIC, finish_vectors, provenance, run_checked
from tools.native_inspect import decode_ranges, instruction_row, selected_ranges
from tools.native_oracle import image_bytes
from tools.spatial_oracle import infantry_deploy_action, map_queries
from tools.spatial_oracle.infantry_deploy_action import Fixture as Base, ACTOR, LOCO, SCENARIO, SP
from tools.spatial_oracle.map_queries import dwords

AI = 0x51BAB0
VTABLE = 0x7EB058
SLICES = ((AI, 0x51BF87), (0x517A60, 0x517AD8), (0x5202F0, 0x520329),
          (0x521C90, 0x521D0F), (0x70C5B0, 0x70C5C7), (0x521B60, 0x521C0F),
          (0x55B5FF, 0x55B61B))
NAMES = {AI: 'infantry_ai', 0x521B60: 'ready_to_commence', 0x5B3570: 'commence',
         0x51CBA0: 'enter_idle_mode', 0x4DA530: 'foot_ai', 0x5202F0: 'thief_capture',
         0x5200B0: 'fear', 0x5206B0: 'fire', 0x520AE0: 'sequencer',
         0x520F40: 'movement_actions', 0x51D6F0: 'do_action', 0x51B350: 'tube_prefix',
         0x70D990: 'virtual_4a0', 0x65C780: 'raw_rng', 0x65C7E0: 'range_rng'}


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


class Probe(Base):
    def __init__(self):
        self.phase = 'setup'
        self.calls = []
        self.row = {}
        super().__init__()
        self.code = bytes(self.u.mem_read(AI, 0x51BF87-AI))
        assert self.read(VTABLE+0x5C) == AI

    def retained(self):
        return dict(alive=self.u.mem_read(ACTOR+0x90, 1)[0], health=self.read(ACTOR+0x6C),
                    current=self.read(ACTOR+0xAC), queued=self.read(ACTOR+0xB4),
                    mission_start=self.read(ACTOR+0xC0), counter=self.read(ACTOR+0xC4),
                    mission_timer=[self.read(ACTOR+x) for x in (0xC8, 0xD0)],
                    doing=self.read(ACTOR+0x6C4), fear=self.read(ACTOR+0x6D4),
                    prone=self.u.mem_read(ACTOR+0x6DB, 1)[0],
                    raw_latch_6da=self.u.mem_read(ACTOR+0x6DA, 1)[0],
                    raw_timer_6c8=[self.read(ACTOR+x) for x in (0x6C8, 0x6D0)],
                    firing=self.u.mem_read(ACTOR+0x68D, 1)[0],
                    nav_present=self.read(ACTOR+0x5A4) != 0,
                    speed_fraction_bits=bytes(self.u.mem_read(ACTOR+0x578, 8)).hex(),
                    head=list(struct.unpack('<iii',self.u.mem_read(LOCO+0x28, 12))),
                    moving=self.u.mem_read(LOCO+0x34, 1)[0],
                    motion=self.u.mem_read(LOCO+0x36, 1)[0])

    def return_boundary(self, value=0, pop=0):
        u = self.u
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_EIP, self.read(sp) & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_ESP, sp+4+pop)

    def observe(self, u, address, size, data):
        if self.phase != 'run':
            return super().observe(u, address, size, data)
        if address in NAMES:
            sp = u.reg_read(UC_X86_REG_ESP)
            event = dict(kind=NAMES[address], address=hex(address),
                         return_pc=hex(self.read(sp) & 0xFFFFFFFF),
                         actor_receiver=u.reg_read(UC_X86_REG_ECX) == ACTOR,
                         state=self.retained())
            if address in (0x51CBA0, 0x70D990, 0x51D6F0):
                count = {0x51CBA0:2, 0x70D990:1, 0x51D6F0:3}[address]
                event['args'] = [self.read(sp+4+n*4) for n in range(count)]
            self.calls.append(event)
        if address in (0x51BC22, 0x51BED7):
            self.calls.append(dict(kind='ready_result', address=hex(address),
                                   value=u.reg_read(UC_X86_REG_EAX) & 255))
        if address == 0x4DA530:
            if self.row.get('kill_at') == 'foot_ai':
                u.mem_write(ACTOR+0x90, b'\0')
            if 'motion_after_foot' in self.row:
                u.mem_write(LOCO+0x34, bytes((self.row['motion_after_foot'], 0,
                                            self.row['motion_after_foot'])))
            self.return_boundary()
            return
        if address in (0x5206B0, 0x520AE0, 0x520F40):
            if self.row.get('kill_at') == NAMES[address]:
                u.mem_write(ACTOR+0x90, b'\0')
            self.return_boundary()
            return
        if address == 0x5202F0 and self.row.get('supplied_thief_success'):
            self.return_boundary(1)
            return
        if address in (0x51B350, 0x70D990):
            self.return_boundary(pop=4 if address==0x70D990 else 0)
            return
        # Field predicates, Ready, Mission Commence, ordinary Thief=false and
        # zero-fear handler execute original bodies; no native RNG is replaced.
        super().observe(u, address, size, data)

    def probe(self, row):
        self.phase = 'setup'
        self.row = row
        super().execute(dict(kind='guard', mission=row.get('mission', 1),
                             doing=0, health=row.get('health', 100),
                             deployer=0, human=1, pending=0, prone=0,
                             repeat=row.get('repeat', 92),
                             moving=row.get('motion', 0), motion=row.get('motion', 0),
                             head=row.get('head', [0,0,0]),
                             nav=row.get('nav', False), now=row.get('now', 116)))
        u = self.u
        u.mem_write(ACTOR+0x6C4, dwords(row.get('doing', 0)))
        u.mem_write(ACTOR+0x578, struct.pack('<d', row.get('speed_fraction', 0.0)))
        u.mem_write(ACTOR+0x684, bytes((row.get('raw_684', 255),)))
        u.mem_write(ACTOR+0x68D, bytes((row.get('firing', 0),)))
        u.mem_write(ACTOR+0xB4, dwords(row.get('queued', -1)))
        u.mem_write(ACTOR+0xC4, dwords(91))
        u.mem_write(ACTOR+0xC8, dwords(71, 0, 29))
        u.mem_write(ACTOR+0x270, bytes((row.get('warped', 0), 0)))
        u.mem_write(ACTOR+0x6DA, bytes((row.get('raw_latch', 0), row.get('prone', 0))))
        u.mem_write(ACTOR+0x6C8, dwords(row.get('timer_start', 100), 0,
                                      row.get('timer_delay', 16)))
        # Already admitted Attack/default rows avoid Guard/AreaGuard's building
        # scatter and off-map cleanup. Neither actor nor type is constructed here.
        self.rngs = dict(scenario=SCENARIO+0x218, main=0x886B88, mapgen=0xABE890)
        for p in self.rngs.values():
            self.call(0x65C6D0, p, (31,))
        before = self.retained()
        rng_before = {key: bytes(u.mem_read(p, 0x3F4)).hex() for key, p in self.rngs.items()}
        self.calls = []
        self.phase = 'run'
        # Same original Logic object loop used by anytown_damage.mission.
        # Membership is supplied; this is not registration/removal execution.
        u.mem_write(SCRATCH+0x1B000, dwords(ACTOR))
        u.mem_write(0x87F77C, dwords(SCRATCH+0x1B000))
        u.mem_write(0x87F788, dwords(1))
        u.mem_write(SP, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_EDI, 0x87F778)
        run_checked(u, 0x55B5FF, 0x55B61B, count=100000,
                    required_addresses=(0x55B610, AI))
        assert u.reg_read(UC_X86_REG_ESP) == SP
        self.phase = 'setup'
        rng_after = {key: bytes(u.mem_read(p, 0x3F4)).hex() for key, p in self.rngs.items()}
        assert self.code == bytes(u.mem_read(AI, len(self.code)))
        assert self.vtable == bytes(u.mem_read(VTABLE, 0x600))
        assert rng_before == rng_after
        return dict(input=row, before=before, calls=self.calls, after=self.retained(),
                    rng_before=rng_before, rng_after=rng_after, original_code_and_vtable_unchanged=True)


def generate():
    rows = [dict(name='ordinary_attack'),
            *[dict(name='dies_in_'+where, kill_at=where) for where in ('foot_ai','fire','sequencer')],
            dict(name='thief_success_boundary', supplied_thief_success=True),
            dict(name='moving_to_stationary_second_commence', mission=2, queued=1,
                 motion=1, head=[2944,2688,0], speed_fraction=1.0, motion_after_foot=0),
            dict(name='moving_retains_queue', mission=2, queued=1, motion=1,
                 head=[2944,2688,0], speed_fraction=1.0),
            dict(name='warped_exit', warped=1), dict(name='nonnegative_684_exit', raw_684=0),
            dict(name='dead_ready_health_reset', health=0, doing=0),
            dict(name='death_doing_retains_zero_health', health=0, doing=11),
            dict(name='completed_fire_clears_before_second_commence', firing=1,
                 repeat=0, doing=4, queued=2),
            dict(name='running_fire_retains_latch', firing=1, repeat=2, doing=4),
            dict(name='completed_deployed_fire_requests_guard', firing=1, repeat=0, doing=29)]
    rows += [dict(name='latch_'+name, raw_latch=raw, **fields) for name, raw, fields in (
        ('before_deadline',1,dict(now=115)), ('due',1,{}), ('due_raw255',255,{}),
        ('stopped_nonzero',1,dict(timer_start=-1)), ('stopped_zero',1,dict(timer_start=-1,timer_delay=0)),
        ('nav_retains',1,dict(nav=True)), ('prone_retains',1,dict(prone=1)),
        ('signed_wrap_due',1,dict(now=-2147483648,timer_start=2147483647,timer_delay=1)))]
    f = Probe()
    cases = [f.probe(row) for row in rows]
    return dict(schema=1,
                infantry_ai_vtable=dict(base=hex(VTABLE), slot='0x5c', target=hex(f.read(VTABLE+0x5C))),
                vtable_slots={hex(slot):hex(f.read(VTABLE+slot))
                              for slot in (0x5C,0x78,0xF8,0x124,0x174,0x184,0x1D4,0x1D8,0x1EC,0x200,0x3A0,0x484,0x4A0,0x558)},
                cases=cases)


def metadata():
    raw = image_bytes()
    snippets = []
    for a, b in SLICES:
        span = selected_ranges(raw, a, b-a, code_only=True)
        decoded = decode_ranges(span, lambda ins:[instruction_row(ins)])
        snippets.append(dict(start=hex(a), end_exclusive=hex(b), bytes=span[0]['bytes'].hex(),
                             sha256=sha(span[0]['bytes']), **decoded))
    return provenance(scope=__doc__, entry_points={'logic_loop':0x55B5FF,'infantry_ai':AI,'foot_ai':0x4DA530,
        'readiness':0x521B60,'commence':0x5B3570,'thief_capture':0x5202F0,'fear':0x5200B0,
        'fire':0x5206B0,'sequencer':0x520AE0,'movement_actions':0x520F40},
        assumptions=['Existing infantry_deploy_action fixture supplies raw Infantry/type/House/sequence state and executes the original Walk constructor and RNG seed31. Its original Deployer=false guard call is setup only.',
            'The original55B5FF..55B61B Logic loop invokes the whole AI caller through unchanged vtable+5C. One-member Logic input is supplied; registration/removal is not executed. Default Attack avoids Guard/AreaGuard building-scatter/off-map branches; temporal head is null and warp flags are supplied. Full object/type construction and rules/ART load are not executed.',
            'Ready predicate and Commence execute originally; one supplied Foot callback changes retained Walk motion to demonstrate the second live Ready query. Thief type flagEC5=0 and fear=0 use actual original early-out bodies.',
            'Raw6DA and6C8/6D0 timer are unnamed retained inputs. Constructor clears6DA and duration; no arming producer is claimed. Raw1/255 cases establish this caller clear only. No deployed/prone reinterpretation.',
            'No full shot/emission, locomotor Process, fear transition, sequencer result, Scenario or retail-map comparison is claimed. No original bytes or vtable entries are patched.'],
        substitutions=['FootAI4DA530, Fire5206B0, sequencer520AE0 and movement actions520F40 return at entry after observation; selected callback controls explicitly clear Object+90. Foot may supply a motion-byte change.',
            'One Thief success control substitutes AL1 at5202F0 to prove caller early exit; all ordinary rows execute its actual EC5=false body.',
            'The raw684>=0 control observes then returns from51B350 and virtual+4A0/70D990; it proves caller exclusion only.']) | dict(original_slices=snippets)


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata,
                   source_paths={'producer': Path(__file__),
                                 **{module.__name__: Path(module.__file__) for module in
                                    (native_oracle, native_inspect, infantry_deploy_action, map_queries)}})
