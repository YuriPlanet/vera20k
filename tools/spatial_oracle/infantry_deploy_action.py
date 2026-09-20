"""Original pending-deploy Stop callback, Guard producer and action completion.

Supplied type/sequence/map state. Original gameplay code and vtables are retained.
The fixture observes disabled-audio requests and supplies only OS interlocked calls.
"""
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_ECX, UC_X86_REG_EDX,
    UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)
from tools.native_oracle import (
    load_image, run_checked, STACK_BASE, STACK_SIZE, SCRATCH, RET_MAGIC,
    finish_vectors, provenance,
)
from tools.spatial_oracle.map_queries import dwords

ACTOR, TYPE, SEQUENCES, LOCO, HOUSE, RULES, SCENARIO, DELAYS, ARCHIVE = [
    SCRATCH + n * 0x3000 for n in range(9)]
SP = STACK_BASE + STACK_SIZE - 0x1000
SPANS = ((0x521320, 0x5216C0), (0x521B40, 0x521B60),
         (0x51D6F0, 0x51DAF0), (0x520AE0, 0x520EFC),
         (0x75ADA0, 0x75ADFF), (0x6FABC4, 0x6FAC31),
         (0x70F770, 0x70F7D2))


class Fixture:
    def __init__(self):
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.u)
        self.u.mem_map(STACK_BASE, STACK_SIZE)
        self.u.mem_map(SCRATCH, 0x20000)
        self.u.mem_map(RET_MAGIC, 0x1000)
        self.original = [bytes(self.u.mem_read(a, b-a)) for a, b in SPANS]
        self.vtable = bytes(self.u.mem_read(0x7EB058, 0x600))
        self.imports = [self.read(0x7E11C8), self.read(0x7E11CC)]
        self.events = []
        self.u.hook_add(UC_HOOK_CODE, self.observe)

    def read(self, address):
        return struct.unpack('<i', self.u.mem_read(address, 4))[0]

    def state(self):
        u = self.u
        return dict(doing=self.read(ACTOR+0x6C4), frame=self.read(ACTOR+0xF8),
                    changed=u.mem_read(ACTOR+0xFC, 1)[0],
                    pending=u.mem_read(ACTOR+0x6E4, 1)[0],
                    prone=u.mem_read(ACTOR+0x6DB, 1)[0],
                    crush=u.mem_read(ACTOR+0x2A4, 1)[0],
                    stage=[self.read(ACTOR+x) for x in (0x100, 0x108, 0x10C, 0x110)],
                    reload=[self.read(ACTOR+x) for x in (0x180, 0x188)],
                    destination=list(struct.unpack('<iii', u.mem_read(LOCO+0x1C, 12))),
                    head=list(struct.unpack('<iii', u.mem_read(LOCO+0x28, 12))),
                    moving=u.mem_read(LOCO+0x34, 1)[0],
                    motion=u.mem_read(LOCO+0x36, 1)[0],
                    rng_indices=[self.read(SCENARIO+0x218+x) for x in (4, 8)])

    def observe(self, _u, address, _size, _data):
        u = self.u
        imports = self.imports
        if address in imports:
            sp = u.reg_read(UC_X86_REG_ESP)
            p = self.read(sp+4)
            value = self.read(p)+(1 if address == imports[0] else -1)
            u.mem_write(p, dwords(value))
            u.reg_write(UC_X86_REG_EAX, value & 0xffffffff)
            u.reg_write(UC_X86_REG_EIP, self.read(sp) & 0xffffffff)
            u.reg_write(UC_X86_REG_ESP, sp+8)
            return
        observed = (0x75ADA0, 0x521B40, 0x521B52, 0x51D6F0, 0x7509E0,
                    0x51D9D2, 0x51DA34, 0x51DA8C, 0x52167C,
                    0x520B4E, 0x520BAD, 0x70F770, 0x65C7E0)
        if address in observed:
            entry = dict(address=f'{address:08X}', state=self.state())
            if address == 0x51D6F0:
                entry['args'] = list(struct.unpack('<iii', u.mem_read(u.reg_read(UC_X86_REG_ESP)+4, 12)))
            elif address == 0x7509E0:
                entry['sound'] = u.reg_read(UC_X86_REG_ECX)
            self.events.append(entry)

    def call(self, address, owner, args=()):
        u = self.u
        u.mem_write(SP, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, owner)
        run_checked(u, address, RET_MAGIC, count=100000, required_addresses=[address])
        assert u.reg_read(UC_X86_REG_ESP) == SP+4*(len(args)+1)
        return u.reg_read(UC_X86_REG_EAX)

    def execute(self, row):
        u = self.u
        u.mem_write(SCRATCH, bytes(0x20000))
        u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
        self.events = []
        u.mem_write(ACTOR, dwords(0x7EB058))
        u.mem_write(ACTOR+0x6C0, dwords(TYPE))
        u.mem_write(TYPE+0xE3C, dwords(SEQUENCES))
        for action in range(42):
            u.mem_write(SEQUENCES+action*36, dwords(100, row.get('count', 6), 6, -1, 0, 0, 0, 0, 0))
        for action, count in row.get('counts', []):
            u.mem_write(SEQUENCES+action*36+4, dwords(count))
        u.mem_write(TYPE+0x56C, dwords(101, 102))
        u.mem_write(TYPE+0x6AC, bytes([row.get('deploy_fire', 1)]))
        u.mem_write(TYPE+0x6C4, dwords(row.get('undeploy_delay', -1)))
        u.mem_write(TYPE+0xEC8, bytes([row.get('deployer', 1), row.get('crushable', 0)]))
        u.mem_write(TYPE+0xD37, bytes([row.get('immune', 0)]))
        u.mem_write(ACTOR+0x6C4, dwords(row.get('doing', 0)))
        u.mem_write(ACTOR+0x6C, dwords(row.get('health', 100)))
        u.mem_write(ACTOR+0x90, b'\1')
        u.mem_write(ACTOR+0x8D, bytes([row.get('falling', 0)]))
        u.mem_write(ACTOR+0x6E4, bytes([row.get('pending', 1)]))
        u.mem_write(ACTOR+0x6DB, bytes([row.get('prone', 1)]))
        u.mem_write(ACTOR+0x2A4, bytes([row.get('crush', 0)]))
        u.mem_write(ACTOR+0xAC, dwords(row.get('mission', 5), -1, -1))
        u.mem_write(ACTOR+0xC0, dwords(row.get('mission_start', 0)))
        u.mem_write(ACTOR+0x100, dwords(row.get('stage_start', 17), 0, row.get('stage_duration', 91), row.get('repeat', 92), row.get('increment', 1)))
        u.mem_write(ACTOR+0xF8, dwords(row.get('image', 7)))
        u.mem_write(ACTOR+0x180, dwords(row.get('reload_start', 17), 0, row.get('reload_duration', 100)))
        u.mem_write(ACTOR+0x21C, dwords(HOUSE))
        u.mem_write(HOUSE+0x1EC, bytes([row.get('human', 0)]))
        u.mem_write(HOUSE+0x184, dwords(row.get('difficulty', 1)))
        u.mem_write(ACTOR+0x9C, dwords(2688, 2688, 0))
        if row.get('archive'):
            u.mem_write(ACTOR+0x218, dwords(ARCHIVE))
            u.mem_write(ARCHIVE, dwords(0x7EB058))
            u.mem_write(ARCHIVE+0x9C, dwords(*row['archive']))
        u.mem_write(ACTOR+0x5A4, dwords(TYPE if row.get('nav', False) else 0))
        u.mem_write(0xA8ED84, dwords(row.get('now', 100)))
        u.mem_write(0xA8B230, dwords(SCENARIO))
        u.mem_write(0x8871E0, dwords(RULES))
        u.mem_write(RULES+0xE30, dwords(DELAYS))
        u.mem_write(DELAYS, dwords(*row.get('delays', [15,25,100])))
        # Native MissionControl +10 Rate; only the current Mission row is needed.
        u.mem_write(0xA8E3A8+row.get('mission',5)*0x20+0x10, struct.pack('<d', 0.1))
        u.mem_write(0x8464AC, b'\0')
        u.mem_write(0xA8E7AC, dwords(row.get('map_editor', 0)))
        self.call(0x65C6D0, SCENARIO+0x218, [31])
        self.call(0x75AA90, LOCO)
        u.mem_write(LOCO+0xC, dwords(ACTOR))
        u.mem_write(LOCO+0x14, dwords(1))
        u.mem_write(ACTOR+0x674, dwords(LOCO+4))
        u.mem_write(LOCO+0x28, dwords(*row.get('head', [0,0,0])))
        u.mem_write(LOCO+0x1C, dwords(2688,2688,0))
        u.mem_write(LOCO+0x34, bytes([row.get('moving',1),0,row.get('motion',1)]))
        self.events = []
        before = self.state()
        rng_before = bytes(u.mem_read(SCENARIO+0x218, 0x3F4)).hex()
        kind = row['kind']
        result = None
        if kind == 'stop':
            self.call(0x75ADA0, 0, [LOCO+4])
        elif kind == 'guard':
            result = self.call(0x521320, ACTOR)
        elif kind == 'callback':
            self.call(0x521B40, ACTOR)
        elif kind == 'action':
            result = self.call(0x51D6F0, ACTOR,
                               [row.get('request',27),row.get('force',0),row.get('random_start',0)])
        elif kind == 'completion':
            self.call(0x520AE0, ACTOR)
        elif kind == 'reload':
            self.call(0x70F770, ACTOR)
        elif kind == 'stage':
            u.reg_write(UC_X86_REG_ESI, ACTOR)
            u.reg_write(UC_X86_REG_EBP, 0)
            u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(u, 0x6FABC4, 0x6FAC31, count=100)
        else:
            raise ValueError(kind)
        assert self.original == [bytes(u.mem_read(a,b-a)) for a,b in SPANS]
        assert self.vtable == bytes(u.mem_read(0x7EB058,0x600))
        return dict(input=row, before=before, after=self.state(), events=self.events,
                    rng_before=rng_before,
                    rng_after=bytes(u.mem_read(SCENARIO+0x218, 0x3F4)).hex(),
                    return_eax=result, original_code_and_vtable_unchanged=True)


def inputs():
    rows = [dict(kind='stop', doing=d, pending=p, head=h)
            for d in range(-1,42) for p in (0,1)
            for h in ([0,0,0],[2688,2688,0])]
    rows += [dict(kind='callback', doing=d, count=c, falling=f)
             for d in (0,27,31,33) for c in (0,-1,65536) for f in (0,1)]
    base = dict(kind='guard',doing=0,pending=0,head=[2688,2688,0])
    variants = [dict(moving=m,pending=p) for m in (0,1) for p in (0,1)]
    variants += [dict(human=1),dict(deployer=0),dict(deploy_fire=0),
                 dict(undeploy_delay=0),dict(nav=True),dict(immune=1),
                 dict(now=25),dict(now=26),dict(now=15,difficulty=0),
                 dict(now=101,difficulty=2),dict(archive=[2944,2688,0]),
                 dict(archive=[2689,2689,0]),dict(now=-2147483648,mission_start=2147483647,delays=[1,1,1]),
                 dict(moving=1,head=[0,0,0],pending=1)]
    rows += [base | v for v in variants]
    rows += [dict(kind='completion',doing=d,image=i,count=c,crush=k,crushable=a,
                  reload_duration=r,counts=[[28,z],[0,z]])
             for d in (27,31) for i,c in ((5,6),(6,6),(-1,-1),(65536,65536))
             for k,a in ((0,0),(1,1)) for r in (93,94) for z in (0,6)]
    rows += [dict(kind='reload',now=n,reload_start=s,reload_duration=d,map_editor=m)
             for n,s,d in ((100,90,20),(100,90,21),(100,-1,11),(100,-1,0),
                           (-2147483648,2147483647,12),(-1,-1,11),(100,90,-1))
             for m in (0,1)]
    rows += [dict(kind='stage',now=n,stage_start=s,stage_duration=d,repeat=r,image=i,increment=inc)
             for n,s,d in ((100,100,1),(101,100,1),(100,-1,0),(100,-1,1),
                           (-2147483648,2147483647,1),(-1,-1,0),(100,100,-1))
             for r in (0,1,-1) for i,inc in ((5,1),(2147483647,1),(0,-1))]
    return rows


def generate():
    f = Fixture()
    return [f.execute(row) for row in inputs()]


def metadata():
    fixture = Fixture()
    result = provenance(
        scope='Original Infantry pending deploy Stop callback, undeployed Guard producer, Deploy/Undeploy completion, common stage tick and reload shortening. Not whole Infantry AI/loaded campaign parity.',
        assumptions=[
            'Original Infantry/Walk vtables and Walk constructor. Supplied Infantry/type/42 signed sequence records, House/difficulty/Rules vectors and MissionControl rate. Original Infantry/type/INI constructors are not executed.',
            'Supplied positive HP100, ordinary land zone0, noncarried/nonfalling except explicit falling contrasts, unmarked actor, original coordinate receivers. No zero-HP failed-path, water reclassification or sequence-sound records in these rows.',
            'Original global audio gate8464AC is false; sound entries observe request order only. Native RNG constructor65C6D0 seeds Scenario RNG31; all gameplay draws execute original RandomRanged.',
            'Completion executes whole520AE0 with supplied current stage. Stage rows execute actual TechnoAI interior6FABC4..6FAC31 with ESI actor, EBP0; outer AI gates/scheduler order are separately required.',
            'Return EAX is compared only for Guard521320. Ignored timer middle stack words are excluded. Native code and original Infantry vtable are checked unchanged.'
        ],
        substitutions=['OS InterlockedIncrement/Decrement imports emulate their integer operation and stdcall cleanup; no gameplay call replacement.'],
        entry_points={'guard':0x521320,'stop':0x75ADA0,'callback':0x521B40,
                      'do_action':0x51D6F0,'completion':0x520AE0,'stage':0x6FABC4,
                      'reload':0x70F770,'rng_constructor':0x65C6D0})
    result['original_slices'] = [dict(start=f'{a:08X}', end_exclusive=f'{b:08X}',
        hex=code.hex(), sha256=hashlib.sha256(code).hexdigest())
        for (a,b),code in zip(SPANS,fixture.original)]
    result['infantry_vtable'] = dict(start='007EB058', hex=fixture.vtable.hex(),
        sha256=hashlib.sha256(fixture.vtable).hexdigest())
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
