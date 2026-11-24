"""Original pending-deploy Stop callback, Guard producer and action completion.

Supplied type/sequence/map state. Original gameplay code and vtables are retained.
The fixture observes disabled-audio requests and supplies only OS interlocked calls.
"""
from pathlib import Path
import hashlib
import struct
import sys

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX,
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

    def prepare_execution(self, row):
        """Additive callers may install their explicitly declared prior state."""
        pass

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
        self.prepare_execution(row)
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
        elif kind == 'unload':
            result = self.call(0x51F6E0, ACTOR)
        elif kind == 'move':
            result = self.call(0x51F660, ACTOR)
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


UNLOAD_SPANS = ((0x51F6E0, 0x51F7F9), (0x4DA2B0, 0x4DA2B5),
                (0x5B2EF0, 0x5B2F00), (0x5B2FD0, 0x5B3040),
                (0x70E120, 0x70E24B), (0x70DD70, 0x70DD8F),
                (0x51AA40, 0x51B3B0), (0x6FCDB0, 0x6FCF95),
                (0x4D94B0, 0x4D9710), (0x5657A0, 0x565810))
CELL, WEAPON, WEAPON_OTHER, NAV = [SCRATCH+n for n in (0x1A000,0x1B000,0x1C000,0x1D000)]
TABLE = 0xC00000


class UnloadFixture(Fixture):
    """Same native actor/Walk fixture; original whole Mission_Unload receiver."""
    def __init__(self):
        self.capture_writes = False
        self.writes = []
        super().__init__()
        self.unload_original = [bytes(self.u.mem_read(a,b-a)) for a,b in UNLOAD_SPANS]
        self.call(0x75AA90,LOCO)
        self.walk_vtables = [(self.read(LOCO+offset),bytes(self.u.mem_read(self.read(LOCO+offset),0x80)))
                            for offset in (0,4)]
        self.u.hook_add(UC_HOOK_MEM_WRITE,self.observe_write)

    def state(self):
        state = super().state()
        state.update(mission=self.read(ACTOR+0xAC), suspended=self.read(ACTOR+0xB0),
                     queued=self.read(ACTOR+0xB4), handler_state=self.read(ACTOR+0xBC),
                     mission_start=self.read(ACTOR+0xC0),
                     target=self.read(ACTOR+0x2B4), nav=self.read(ACTOR+0x5A4),
                     archive=self.read(ACTOR+0x218), firing=self.u.mem_read(ACTOR+0x68D,1)[0],
                     stage_aux=self.read(ACTOR+0x104), blockage_path_delay=self.read(RULES+0x1768))
        return state

    def rng_states(self):
        return {key:bytes(self.u.mem_read(ptr,0x3F4)).hex()
                for key,ptr in {'scenario':SCENARIO+0x218,'main':0x886B88,'mapgen':0xABE890}.items()}

    def prepare_execution(self,row):
        u=self.u
        table=bytearray(0x100000)
        for x,y,p in ((10,10,CELL),(11,10,NAV)):
            struct.pack_into('<I',table,(y*512+x)*4,p)
            u.mem_write(p,dwords(0x7E4EEC))
            u.mem_write(p+0x24,struct.pack('<hh',x,y))
            u.mem_write(p+0x116,struct.pack('<h',-1))
        u.mem_write(TABLE,bytes(table))
        u.mem_write(0x87F7E8+0x13C,dwords(TABLE,0x40000))
        u.mem_write(0x87F924,dwords(TABLE))
        u.mem_write(TYPE+0x24,row.get('type_id','E1').encode('ascii')+b'\0')
        u.mem_write(TYPE+0x691,bytes([row.get('spray_attack',0)]))
        u.mem_write(RULES+0x1768,dwords(row['blockage_path_delay']))
        for index,p in enumerate((WEAPON,WEAPON_OTHER)):
            u.mem_write(TYPE+0x898+index*28,dwords(p if row.get('weapon_present',True) else 0))
            u.mem_write(p+0x150,bytes([row.get('area_fire',0) if index==row.get('area_slot',1) else 0]))
        u.mem_write(ACTOR+0xAC,dwords(row.get('mission',7),row.get('suspended',-1),row.get('queued',-1)))
        u.mem_write(ACTOR+0xBC,dwords(row.get('handler_state',0),row.get('mission_start',0)))
        u.mem_write(ACTOR+0x5A4,dwords(NAV if row.get('nav',True) else 0))
        u.mem_write(ACTOR+0x2B4,dwords(CELL if row.get('target',False) else 0))
        u.mem_write(ACTOR+0x68D,bytes([row.get('firing',0)]))
        for ptr in (0x886B88,0xABE890):
            self.call(0x65C6D0,ptr,[31])
        self.writes=[]
        self.capture_writes=True
        self.before_rng=self.rng_states()

    def observe(self,u,address,size,data):
        if address in (0x51F6E0,0x70E120,0x70DD70,0x70E140,0x70E240,
                       0x51F792,0x5657A0,0x51B1F0,0x6FCDB0,0x6FCF3E,
                       0x5B2FD0,0x51AA40,0x4D94B0,0x4D9510,0x4D96BC,0x4DA2B0):
            event=dict(address=f'{address:08X}',state=self.state())
            if address in (0x51B1F0,0x6FCDB0,0x5B2FD0):
                event['argument']=self.read(u.reg_read(UC_X86_REG_ESP)+4)
            if address==0x51AA40:
                event['args']=list(struct.unpack('<ii',u.mem_read(u.reg_read(UC_X86_REG_ESP)+4,8)))
            self.events.append(event)
        super().observe(u,address,size,data)

    def observe_write(self,u,_access,address,size,value,_data):
        offsets=(0xAC,0xB0,0xB4,0xBC,0xC0,0xF8,0xFC,0x100,0x104,0x108,0x10C,0x110,
                 0x180,0x184,0x188,0x2A4,0x2B4,0x5A4,0x68D,0x6C4,0x6DB,0x6E4)
        fields={ACTOR+x:f'actor+{x:03X}' for x in offsets}
        fields.update({LOCO+x:f'walk+{x:02X}' for x in (0x1C,0x20,0x24,0x28,0x2C,0x30,0x34,0x36)})
        if self.capture_writes and address in fields:
            self.writes.append(dict(instruction=f'{u.reg_read(UC_X86_REG_EIP):08X}',
                                    field=fields[address],size=size,value=value&((1<<(8*size))-1)))

    def execute(self,row):
        self.capture_writes=False
        result=super().execute(row)
        self.capture_writes=False
        assert self.unload_original==[bytes(self.u.mem_read(a,b-a)) for a,b in UNLOAD_SPANS]
        assert [self.read(LOCO+offset) for offset in (0,4)]==[a for a,_ in self.walk_vtables]
        assert all(code==bytes(self.u.mem_read(a,len(code))) for a,code in self.walk_vtables)
        result.update(writes=self.writes,rng_streams_before=self.before_rng,rng_streams_after=self.rng_states())
        return result


def unload_inputs():
    base=dict(kind='unload',pending=0,prone=0,mission=7,nav=True,blockage_path_delay=0)
    rows=[base|dict(doing=doing,human=human,undeploy_delay=delay)
          for doing in (0,27,28,29,30,31) for human in (0,1) for delay in (-1,0,60)]
    rows += [base|dict(name=name)|control for name,control in (
        ('not_deployer',dict(deployer=0)),
        ('absent_Deploy_record',dict(counts=[[27,0]])),
        ('absent_Undeploy_record',dict(doing=28,counts=[[31,0]])),
        ('absent_Deploy_record_AreaFire',dict(counts=[[27,0]],area_fire=1)),
        ('DESO_AreaFire',dict(type_id='DESO',area_fire=1)),
        ('DESO_AreaFire_auto_delay',dict(type_id='DESO',area_fire=1,undeploy_delay=60)),
        ('other_AreaFire',dict(area_fire=1)),
        ('other_AreaFire_human',dict(area_fire=1,human=1)),
        ('SprayAttack_primary_AreaFire',dict(spray_attack=1,area_fire=1,area_slot=0)),
        ('SprayAttack_secondary_not_selected',dict(spray_attack=1,area_fire=1,area_slot=1)),
        ('no_weapon',dict(weapon_present=False)),
        ('NULL_destination_consumes_pending',dict(pending=1)),
        ('paid_head_retains_pending',dict(pending=1,head=[2944,2688,0])),
        ('DeployFire_false_still_deployer',dict(deploy_fire=0)),
        ('existing_same_Cell_target',dict(target=True,area_fire=1)),
        ('existing_firing_clear',dict(firing=1,area_fire=1)))]
    return rows


def generate_unload():
    fixture=UnloadFixture()
    return [fixture.execute(row) for row in unload_inputs()]


def unload_metadata():
    f=UnloadFixture()
    out=provenance(
        scope='Whole original Infantry Mission_Unload51F6E0, its original class action, slot query, Guard Assign and class NULL destination/Walk Stop callbacks. Supplied positive-health state; not complete deploy UI or whole Infantry AI.',
        assumptions=[
            'Extends the existing infantry_deploy_action actor/Walk/type/sequence fixture. Original gameplay code and class vtables are unchanged; no supplied gameplay return. Disabled audio observes native requests only.',
            'All42 signed sequence records and Type Deployer/DeployedCrushable/DeployFire/UndeployDelay/SprayAttack/sound/weapon state are explicit supplied inputs. Rules+1768 BlockagePathDelay=0 is supplied, with readback; the reader owner names its [AI] key. This mode does not execute the Type/INI constructors or certify retail reader parity.',
            'Original Map5657A0 resolves the supplied Cell10,10 table. Both ordinary Cell identities have no tube. CanDeploy700D50/UI tube-neighborhood484AE0 admission and full map loading are outside this handler boundary.',
            'Original Scenario, Main and MapGen constructors65C6D0 seed31. Every complete3F4-byte RNG object is retained before/after. Only original gameplay draws may advance them.',
            'Health100, alive1, ground movement, noncarried/nonfalling, no attached particles/radio/temporal links, no nonempty Foot5AC object vector. Human class destination refusal27..30 is retained.',
            'Stack-residue timer auxiliary dword104 is recorded but has no represented decision. Exact action/Stage/current mission/queued mission/NavCom/target/pending/crush/prone/passive timer/Walk fields and ordered writes are retained.'
        ],substitutions=['OS InterlockedIncrement/Decrement support from the shared fixture; no gameplay code or vtable is replaced.'],
        entry_points={'unload':0x51F6E0,'do_action':0x51D6F0,'walk_ctor':0x75AA90,'slot':0x70E120,
                      'spray_index':0x70DD70,'weapon':0x70E140,'valid_weapon':0x70E240,
                      'target':0x51B1F0,'base_target':0x6FCDB0,'assign_guard':0x5B2FD0,
                      'destination':0x51AA40,'foot_destination':0x4D94B0,
                      'walk_stop':0x75ADA0,'pending_callback':0x521B40,'fallback':0x4DA2B0})
    out['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',
       hex=code.hex(),sha256=hashlib.sha256(code).hexdigest())
       for (a,b),code in zip(SPANS+UNLOAD_SPANS,f.original+f.unload_original)]
    out['infantry_vtable']=dict(start='007EB058',sha256=hashlib.sha256(f.vtable).hexdigest(),hex=f.vtable.hex())
    out['walk_vtables']=[dict(start=f'{a:08X}',sha256=hashlib.sha256(code).hexdigest(),hex=code.hex()) for a,code in f.walk_vtables]
    return out


# Additive deployed Guard evidence. The three historical corpora retain their
# schemas and values; this mode shares their actor/map/action fixture owner.
GUARD_SPANS = ((0x522510, 0x522533), (0x51F620, 0x51F653),
               (0x51F330, 0x51F3D8), (0x51C8B0, 0x51CB95),
               (0x6FC0B0, 0x6FCD38), (0x51DF60, 0x51DFF0),
               (0x487C80, 0x487C87), (0x65B510, 0x65B528),
               (0x6F77B0, 0x6F7999), (0x469130, 0x46920B),
               (0x6FE549, 0x6FE57C), (0x6FF831, 0x6FF872))
RAD_SITE, GUARD_WARHEAD, GUARD_PROJECTILE = (SCRATCH + x for x in (0x1E000, 0x1F000, 0x1F800))
PRESERVED_GUARD_CORPORA = {
    'infantry_deploy_action': (355, '4c3749ed647ae5ccac4ed73a5d012afdc00ec5f9ea6090082874746a2960c9e9', 'b2888e3c2680e11dffe4b845f86fb74e8efd286c06e021e13e258e5cbcbbe62b'),
    'infantry_action_callback': (44, '525a74860c4f7688d7f90581d244bc80c508f5e28aac47d43421fef0be5911e8', '78eb1b9e0f5ef96eae283beab95008aecd1b1e49edec250a89f2c99f8cc11bc5'),
    'infantry_mission_unload': (52, '04bc8ca5d63ece95f3fb13b66bbe6c69ce8c2aae2415f178531504a2b02c0278', 'acc238fc326b4041f1a4261d230ff83c21acb4e80723f7fe9f39768a1d7b7300'),
}


class DeployedGuardFixture(UnloadFixture):
    """Original Guard shim and deployed predicate, with one declared tail seam.

    Stop controls reach real base FireAt6FDD50 before it executes. Tail controls
    supply only that body's EAX/stdcall return, and then execute the original
    Infantry wrapper/Guard suffix. The two evidence boundaries stay distinct.
    """
    def __init__(self):
        self.active_guard = False
        self.guard_calls = []
        self.guard_pending = {}
        self.guard_stop = None
        self.guard_callback_count = 0
        super().__init__()
        self.guard_original = [bytes(self.u.mem_read(a, b-a)) for a, b in GUARD_SPANS]
        self.text_sha256 = hashlib.sha256(bytes(self.u.mem_read(0x401000, 0x3E0000))).hexdigest()
        assert self.text_sha256 == '4cd5557a7490debc493ff965afc4483d8d2f1065f434f6b665cbb8fc4835b0cc'
        self.cell_vtable = bytes(self.u.mem_read(0x7E4EEC, 0x200))

    def guard_state(self):
        state = self.state()
        state.update(position=list(struct.unpack('<iii', self.u.mem_read(ACTOR+0x9C, 12))),
                     game_speed_index=self.read(0xA8EB60),
                     on_bridge=self.u.mem_read(ACTOR+0x8C, 1)[0],
                     ammo=self.read(ACTOR+0x2FC),
                     rearm=[self.read(ACTOR+x) for x in (0x2EC, 0x2F4)],
                     speed_fraction_bits=bytes(self.u.mem_read(ACTOR+0x578, 8)).hex(),
                     undeploy_delay=self.read(TYPE+0x6C4),
                     sequence_count29=self.read(SEQUENCES+29*36+4),
                     sequence_count31=self.read(SEQUENCES+31*36+4),
                     immune_to_radiation=self.u.mem_read(TYPE+0xD37, 1)[0],
                     deploy_fire=self.u.mem_read(TYPE+0x6AC, 1)[0],
                     rad_site=self.read(CELL+0xF8),
                     rad_site_words=[self.read(RAD_SITE+x) for x in (0x4C, 0x6C, 0x70)],
                     weapon_range=self.read(WEAPON_OTHER+0xB4),
                     weapon_rad_level=self.read(WEAPON_OTHER+0x158))
        return state

    def prepare_execution(self, row):
        super().prepare_execution(row)
        u = self.u
        # Exact receiver fields are supplied, not inferred native constructors.
        u.mem_write(TYPE+0x6AE, b'\1')
        u.mem_write(ACTOR+0x14, b'\7')
        u.mem_write(ACTOR+0x2FC, dwords(row['ammo']))
        u.mem_write(ACTOR+0x2EC, dwords(-1, 0, row['rearm']))
        u.mem_write(ACTOR+0x8C, bytes([row['bridge']]))
        u.mem_write(ACTOR+0x9C, dwords(2688, 2688, row['z']))
        u.mem_write(ACTOR+0x5A4, dwords(0))
        u.mem_write(ACTOR+0x2B4, dwords(NAV if row['target'] else 0))
        u.mem_write(ACTOR+0x578, struct.pack('<d', row['speed_fraction']))
        u.mem_write(CELL+0xEC, dwords(row['land']))
        u.mem_write(CELL+0x140, dwords(row['cell_flags']))
        u.mem_write(CELL+0xF8, dwords(RAD_SITE if row['site'] is not None else 0))
        if row['site'] is not None:
            site = row['site']
            u.mem_write(RAD_SITE+0x4C, dwords(site['level']))
            u.mem_write(RAD_SITE+0x6C, dwords(site['duration'], site['remaining']))
        for weapon in (WEAPON, WEAPON_OTHER):
            u.mem_write(weapon+0x9C, dwords(1, GUARD_PROJECTILE, 50))
            u.mem_write(weapon+0xAC, dwords(GUARD_WARHEAD))
            u.mem_write(weapon+0xB4, dwords(row['range']))
            u.mem_write(weapon+0x150, b'\1')
            u.mem_write(weapon+0x158, dwords(row['rad_level']))
        u.mem_write(GUARD_PROJECTILE+0x2A4, b'\1\1')
        u.mem_write(0x87F914, dwords(128, 128))
        u.mem_write(0xA8EB60, dwords(row['game_speed_index']))
        u.mem_write(0xA8E3A8+row['mission']*0x20+0x10, struct.pack('<d', row['mission_rate']))
        for pointer in (SCENARIO+0x218, 0x886B88, 0xABE890):
            self.call(0x65C6D0, pointer, [row['seed']])
        self.guard_before = self.guard_state()
        self.before_rng = self.rng_states()
        self.writes = []
        self.guard_calls = []
        self.guard_pending = {}
        self.guard_callback_count = 0

    def observe(self, u, address, size, data):
        if self.active_guard:
            if address in self.guard_pending:
                event = self.guard_pending.pop(address)
                event['returned_eax'] = u.reg_read(UC_X86_REG_EAX)
                if event['kind'] == 'cell_coords':
                    event['returned_coord'] = list(struct.unpack('<iii', u.mem_read(u.reg_read(UC_X86_REG_EAX), 12)))
            entries = {
                0x521320: ('guard', 0), 0x522510: ('deployed_predicate', 0),
                0x41BEA0: ('navigation_cell', 1), 0x5657A0: ('map_cell', 1),
                0x565730: ('map_world', 1), 0x486840: ('cell_coords', 1),
                0x487C80: ('rad_site_getter', 0), 0x65B510: ('current_rad_level', 0),
                0x70E140: ('weapon', 1), 0x51B1F0: ('infantry_target', 1),
                0x6FCDB0: ('techno_target', 1), 0x51C8B0: ('infantry_fire_error', 3),
                0x6FC0B0: ('techno_fire_error', 3), 0x6F77B0: ('in_range', 2),
                0x51DF60: ('infantry_fire', 2), 0x6FDD50: ('base_fire', 2),
                0x51D6F0: ('do_action', 3), 0x51F330: ('deployed_reacquire', 0),
                0x4D9920: ('foot_greatest_threat', 3), 0x5B3A00: ('mission_control', 0),
                0x65C7E0: ('rng_ranged', 2), 0x65C780: ('rng_next', 0),
            }
            if address in entries:
                kind, count = entries[address]
                sp = u.reg_read(UC_X86_REG_ESP)
                event = dict(kind=kind, address=f'{address:08X}',
                             receiver=u.reg_read(UC_X86_REG_ECX),
                             return_address=f'{self.read(sp)&0xFFFFFFFF:08X}',
                             args=list(struct.unpack('<'+'i'*count, u.mem_read(sp+4, count*4))) if count else [],
                             state=self.guard_state())
                if kind == 'map_cell':
                    event['query_cell'] = list(struct.unpack('<hh', u.mem_read(event['args'][0]&0xFFFFFFFF, 4)))
                if kind == 'map_world':
                    event['query_coord'] = list(struct.unpack('<iii', u.mem_read(event['args'][0]&0xFFFFFFFF, 12)))
                if kind in ('rng_ranged', 'rng_next'):
                    event['stream'] = next((key for key, pointer in {'scenario': SCENARIO+0x218, 'main': 0x886B88, 'mapgen': 0xABE890}.items()
                                            if pointer == event['receiver']), 'other')
                if kind == 'foot_greatest_threat':
                    event['supplied_coord'] = list(struct.unpack('<iii', u.mem_read(event['args'][1]&0xFFFFFFFF, 12)))
                self.guard_calls.append(event)
                self.guard_pending[self.read(sp)&0xFFFFFFFF] = event
            if address in (0x5213DF, 0x5213EC, 0x521499, 0x5214B0, 0x5214D8, 0x5214EE):
                self.guard_calls.append(dict(kind='native_register_observation', address=f'{address:08X}',
                    eax=u.reg_read(UC_X86_REG_EAX), ebx=u.reg_read(UC_X86_REG_EBX), edx=u.reg_read(UC_X86_REG_EDX)))
            if address == 0x6FDD50 and self.guard_row['base_fire_boundary'] == 'supplied_return':
                # Explicitly authorized callback tail: no base FireAt instruction
                # executes and no projectile/radiation/rearm/RNG effect is supplied.
                self.guard_callback_count += 1
                self.guard_calls[-1]['supplied_return_eax'] = self.guard_row['base_fire_return']
                sp = u.reg_read(UC_X86_REG_ESP)
                u.reg_write(UC_X86_REG_EAX, self.guard_row['base_fire_return']&0xFFFFFFFF)
                u.reg_write(UC_X86_REG_EIP, self.read(sp)&0xFFFFFFFF)
                u.reg_write(UC_X86_REG_ESP, sp+12)
                return
        super().observe(u, address, size, data)

    def call(self, address, owner, args=()):
        if address != 0x521320:
            return super().call(address, owner, args)
        u = self.u
        entry = 0x522510 if self.guard_row['kind'] == 'predicate' else 0x521320
        u.mem_write(SP, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, owner)
        u.reg_write(UC_X86_REG_EAX, 0x12345678)
        u.reg_write(UC_X86_REG_EDX, 0x23456789)
        self.active_guard = True
        ends = RET_MAGIC if self.guard_row['base_fire_boundary'] == 'supplied_return' else (RET_MAGIC, 0x6FDD50)
        try:
            self.guard_stop = run_checked(u, entry, ends, count=200000, required_addresses=[entry])
        finally:
            self.active_guard = False
        if self.guard_stop == RET_MAGIC:
            assert u.reg_read(UC_X86_REG_ESP) == SP+4
            return u.reg_read(UC_X86_REG_EAX)
        return None

    def execute(self, row):
        self.guard_row = row
        result = super().execute(row | dict(kind='guard'))
        result['input'] = row
        result['guard_before'] = self.guard_before
        result['guard_after'] = self.guard_state()
        result['calls'] = self.guard_calls
        result['query_order'] = [dict(kind=call['kind'], address=call['address'],
                                    query=call.get('query_cell', call.get('query_coord')),
                                    returned_cell=call.get('returned_eax'))
                                 for call in self.guard_calls if call['kind'] in ('map_cell', 'map_world')]
        result['execution'] = dict(entry='00522510' if row['kind'] == 'predicate' else '00521320',
                                   stop=f'{self.guard_stop:08X}', returned=self.guard_stop == RET_MAGIC,
                                   gameplay_return_supplied=self.guard_callback_count != 0,
                                   base_fire_callback_count=self.guard_callback_count,
                                   stopped_esp=self.u.reg_read(UC_X86_REG_ESP))
        result['return_signed'] = struct.unpack('<i', dwords(result['return_eax']))[0] if result['return_eax'] is not None else None
        assert self.guard_original == [bytes(self.u.mem_read(a, b-a)) for a, b in GUARD_SPANS]
        assert self.text_sha256 == hashlib.sha256(bytes(self.u.mem_read(0x401000, 0x3E0000))).hexdigest()
        assert self.cell_vtable == bytes(self.u.mem_read(0x7E4EEC, 0x200))
        result['original_text_and_vtables_unchanged'] = True
        return result


def deployed_guard_inputs():
    base = dict(kind='guard', doing=28, pending=0, prone=0, immune=1, deployer=1,
                deploy_fire=1, undeploy_delay=-1, mission=5, nav=False, target=False,
                firing=0, moving=0, motion=0, blockage_path_delay=0, type_id='DESO',
                count=6, counts=[[29, 7], [31, 1]], seed=31, now=100, mission_rate=0.1,
                game_speed_index=0,
                ammo=-1, rearm=0, speed_fraction=0.0, bridge=0, z=0, land=1,
                cell_flags=0, range=1024, rad_level=500, site=None,
                base_fire_boundary='stop', base_fire_return=0)
    rows = []
    def add(name, **fields):
        rows.append(base | dict(name=name) | fields)
    for doing in (27, 28, 29, 30):
        add(f'deployed_{doing}_absent_site_prefix', doing=doing, firing=1, target=True)
    for name, site in (
        ('absent', None), ('below165', dict(level=165, duration=1, remaining=1)),
        ('exact166', dict(level=166, duration=1, remaining=1)),
        ('above167', dict(level=167, duration=1, remaining=1)),
        ('duration_zero', dict(level=500, duration=0, remaining=1)),
        ('duration_negative', dict(level=500, duration=-1, remaining=1)),
        ('remaining_fraction', dict(level=500, duration=3, remaining=1)),
        ('multiply_wrap', dict(level=2147483647, duration=1, remaining=2))):
        add(f'site_{name}', site=site, firing=1, target=True)
    for level in (-2147483648, -500, -1, 0, 2, 2147483647):
        add(f'signed_rad_level_{level}', rad_level=level, site=dict(level=0, duration=1, remaining=1))
    for seed in (0, 1, 31):
        for rate in (0.0, 0.1, -0.1, 1.0):
            add(f'exact_site_seed{seed}_rate{rate}', seed=seed, mission_rate=rate,
                site=dict(level=166, duration=1, remaining=1), target=True, firing=1)
    for name, fields in (
        ('ammo_zero', dict(ammo=0)), ('rearm_one', dict(rearm=1)),
        ('speed_above_point_one', dict(speed_fraction=0.10000000000000002)),
        ('speed_exact_point_one', dict(speed_fraction=0.1)), ('falling', dict(falling=1))):
        add(f'fire_error_{name}', **fields)
    for limit in (415, 416, 1024):
        add(f'deck416_range{limit}', bridge=1, z=416, cell_flags=0x100, range=limit, firing=1, target=True)
    add('area_guard_exact_site', mission=11, site=dict(level=166, duration=1, remaining=1), target=True, firing=1)
    for count in (-2147483648, -1, 0, 1, 7, 65536, 2147483647):
        add(f'accepted_callback_count29_{count}', counts=[[29, count], [31, 1]],
            base_fire_boundary='supplied_return', firing=1, target=True)
        add(f'YURI_undeploy_count31_{count}', type_id='YURI', immune=0, undeploy_delay=150,
            counts=[[29, 7], [31, count]], target=True, firing=1)
    add('accepted_callback_below_site', site=dict(level=165, duration=1, remaining=1),
        base_fire_boundary='supplied_return', firing=1, target=True)
    add('accepted_callback_deck416', bridge=1, z=416, cell_flags=0x100,
        base_fire_boundary='supplied_return', firing=1, target=True)
    add('accepted_callback_return_nonzero', base_fire_boundary='supplied_return', base_fire_return=1)
    add('accepted_callback_same_doing29', doing=29, base_fire_boundary='supplied_return')
    for doing in (27, 29, 30):
        add(f'YURI_undeploy_prior_doing{doing}', type_id='YURI', immune=0, undeploy_delay=150, doing=doing)
    add('undeploy_precedes_DeployFire_false', undeploy_delay=0, deploy_fire=0, immune=0)
    add('deployed_DeployFire_false', deploy_fire=0)
    add('nonimmune_empty_registered_scan', immune=0, type_id='SUPPLIED_NONIMMUNE')
    add('nonimmune_retained_Cell_target', immune=0, type_id='SUPPLIED_NONIMMUNE', target=True, firing=1)
    predicates = [base | dict(name=f'deployed_predicate_{doing}', kind='predicate', doing=doing)
                  for doing in [*range(-1, 42), -2147483648, 2147483647, -65536, 65536]]
    return rows, predicates


def guard_preserved_corpora():
    import json
    out = {}
    for name, (count, canonical, file_hash) in PRESERVED_GUARD_CORPORA.items():
        path = Path(__file__).with_name(name+'.json')
        raw = path.read_bytes()
        value = json.loads(raw)
        assert len(value) == count
        assert hashlib.sha256(raw).hexdigest() == file_hash, name
        assert hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest() == canonical, name
        out[name] = dict(rows=count, file_sha256=file_hash, canonical_sha256=canonical)
    return out


def guard_retail_lexical_inputs():
    from tools.projectile_oracle.bridge_render_inputs import lexical
    root = Path(__file__).resolve().parents[2]/'ini'
    out = {}
    for name, wanted in (
        ('RULESMD.INI', {'DESO', 'YURI', 'YURIPR', 'RadEruptionWeapon', 'InvisibleLow', 'RadEruptionWarhead'}),
        ('ARTMD.INI', {'DESO', 'YURI', 'YURIX', 'DesoSequence', 'YuriSequence', 'YuriXSequence'})):
        raw = (root/name).read_bytes()
        sections, lines = lexical(raw, wanted)
        out[name] = dict(sha256=hashlib.sha256(raw).hexdigest(), sections=sections, lines=lines,
                        boundary='Physical lexical strings only. This fixture does not execute the type/weapon/ART readers or INI file loading.')
    return out


def generate_deployed_guard():
    preserved = guard_preserved_corpora()
    rows, predicates = deployed_guard_inputs()
    result = dict(schema_version=1, preserved_corpora=preserved,
                  retail_lexical_inputs=guard_retail_lexical_inputs(),
                  guard_rows=[DeployedGuardFixture().execute(row) for row in rows],
                  predicate_rows=[DeployedGuardFixture().execute(row) for row in predicates])
    assert guard_preserved_corpora() == preserved
    return result


def deployed_guard_metadata():
    fixture = DeployedGuardFixture()
    out = provenance(
        scope='Original deployed Infantry Guard521320 and shared Doing predicate522510. Real fire-error/current-cell/radiation-level/target/action/cadence bodies execute. Accepted-fire prefixes stop at real6FDD50; separate supplied-base-return tails execute its original Guard continuation. Not a whole RadEruption shot or loaded-world proof.',
        assumptions=[
            'Same supplied actor/type/42 signed sequence/House/map fixture as infantry_deploy_action, with original Infantry/Cell/Walk vtables and original Walk constructor. No actor/type/weapon/Warhead/Projectile/RadSite constructors or full readers execute in this mode. The physical INIs are separately recorded lexical premises, not reader-return goldens.',
            'Type normal weapon slots0/1 are explicit supplied identical scalar weapon states. Their Projectile AG/AA bytes are1, Warhead fields are suppliedzero, AreaFire1, Burst1, Damage50, Range/RadLevel from each input. Rules/types/sequence storage startszero and inputs override only declared fields. Elite rank0, TypeFraidycat0, ordinary land movement, HP100, Alive1, Limbo0, no attached particles/radio/temporal/transport links.',
            'Original Map5657A0/565730 and Cell486840 run on fixed512-stride table with Cell10,10 and11,10, ground level/slope0. Map dimensions128x128 and empty native registered scan storage are supplied. Nonimmune no-target row executes original51F330→Foot4D9920 and scanner under that empty premise; this is not populated scanner registration/topology/placement parity.',
            'OnBridge/deck416/CellHasBridge0x100 are explicit supplied poses/geometry flags, not native bridge Unlimbo/occupancy production. Self-fire targets the actual current Cell and its own ground coordinate. Range415/416 controls execute real native admission.',
            'Cell+F8 RadSite pointer and site+4C level/+6C duration/+70 remaining are explicit supplied retained fields. Original65B510 performs signed DWORD multiplication/division; original5213CB..5213DD performs signed RadLevel/3. No Python expected arithmetic supplies decisions.',
            'Original65C6D0 constructs all three complete3F4-byte RNG objects using the declared seed. Original MissionControl+10 Rate and FPCW0E7F are supplied; native900 multiplier/ftol and ScenarioRandomRanged execute. Main/MapGen are retained, including the callback-tail excluded-base boundary.',
            'Stop rows reach original6FDD50 before its first instruction, after real51DF60 clears68D. They do not return from Guard and do not prove projectile, rearm, sound, radiation, damage or downstream action/count results. Tail rows instead supply only base6FDD50 EAX/stdcall8 return, then execute unchanged51DF60 suffix, targetNULL, DoAction29 and signed action29 count return. Base FireAt effects/RNG are excluded, not synthesized.',
            'Supplied YURI UndeployDelay150 and varied signed Undeploy Count31 controls run original early52134C arm. Physical RULESMD YURI/YURIPR author UndeployDelay150/75; ARTMD YuriSequence/YuriXSequence author Undeploy301,6,0/274,6,0, with Count31=6 in both. The arm precedes DeployFire/Immune checks, requests unforced31 and returns Count31 rather than UndeployDelay even when action admission refuses. Negative count returns may equal fallback sentinel−1; whole51F620/51F640 Foot fallback scheduling is excluded.',
            'Sequence sound counts are suppliedzero, GameOptions+A8EB60 speed index is supplied0 (no Options constructor/settings load), and original global audio gate8464AC is false. Sound request order is observed; configured audio/device/Main audio RNG are outside this fixture. Timer middle104 stack word is recorded, not semantic timer authority.',
            'Physical RadEruptionWeapon IsRadEruption=no/ProjectileInvisibleLow/positiveRadLevel and RadEruptionWarhead are lexical evidence. Saved original FireAt Bullet-create/configure/fire and BulletDetonate469130..469206 slices establish the impact arming path by instruction reading; no full retail DESO firing execution or RadSite activation result is claimed.'
        ],
        substitutions=[
            'Inherited OS InterlockedIncrement/Decrement support; no code or vtable patches.',
            'Only rows input.base_fire_boundary=supplied_return replace original base6FDD50 with declared input.base_fire_return EAX and original8-byte argument cleanup. This is a bounded callback tail, not an original FireAt result. Stop/refusal/high-site/predicate rows supply no gameplay return.'
        ],
        entry_points={'guard':0x521320, 'deployed_predicate':0x522510, 'guard_caller':0x51F620,
                      'area_guard_caller':0x51F640, 'current_cell':0x41BEA0, 'map_cell':0x5657A0,
                      'map_world':0x565730, 'cell_coords':0x486840, 'rad_site':0x487C80,
                      'current_rad_level':0x65B510, 'target':0x51B1F0, 'fire_error':0x51C8B0,
                      'base_fire_error':0x6FC0B0, 'infantry_fire':0x51DF60, 'base_fire_boundary':0x6FDD50,
                      'do_action':0x51D6F0, 'deployed_reacquire':0x51F330, 'greatest_threat':0x4D9920,
                      'rng_ctor':0x65C6D0, 'rng_ranged':0x65C7E0, 'bullet_create_static_caller':0x6FE55D,
                      'bullet_configure_static_caller':0x6FF859, 'bullet_fire_static_caller':0x6FF86C,
                      'bullet_detonate_rad_level':0x469130})
    out['preserved_corpora'] = guard_preserved_corpora()
    out['original_text'] = dict(start='00401000', bytes=0x3E0000, sha256=fixture.text_sha256)
    out['original_slices'] = [dict(start=f'{a:08X}', end_exclusive=f'{b:08X}', hex=code.hex(),
                                  sha256=hashlib.sha256(code).hexdigest())
                             for (a, b), code in zip(SPANS+UNLOAD_SPANS+GUARD_SPANS,
                                                     fixture.original+fixture.unload_original+fixture.guard_original)]
    out['infantry_vtable'] = dict(start='007EB058', sha256=hashlib.sha256(fixture.vtable).hexdigest())
    out['cell_vtable'] = dict(start='007E4EEC', bytes=0x200, sha256=hashlib.sha256(fixture.cell_vtable).hexdigest())
    out['walk_vtables'] = [dict(start=f'{a:08X}', sha256=hashlib.sha256(code).hexdigest()) for a, code in fixture.walk_vtables]
    return out


class AutomaticGuardFixture(UnloadFixture):
    """Original Guard-family caller plus the existing Stop/action fixture.

    The caller entry is selected without changing gameplay code or vtables.
    Foot+520=-1 is the original constructor4D31F1 premise required by idle.
    """
    def __init__(self):
        super().__init__()
        self.automatic_original = [bytes(self.u.mem_read(a, b-a)) for a, b in GUARD_SPANS]
        self.text_sha256 = hashlib.sha256(bytes(self.u.mem_read(0x401000, 0x3E0000))).hexdigest()

    def prepare_execution(self, row):
        super().prepare_execution(row)
        self.u.mem_write(ACTOR+0x520, dwords(-1))
        self.u.mem_write(0xA8EB60, dwords(row['game_speed_index']))
        self.u.mem_write(RULES+0x1710, struct.pack('<d', row['idle_action_frequency']))
        self.u.mem_write(ACTOR+0x9C, dwords(*row.get('position', [2688, 2688, 0])))
        if row.get('archive'):
            self.u.mem_write(ARCHIVE+0x6C, dwords(100))
            self.u.mem_write(ARCHIVE+0x90, b'\1')

    def state(self):
        result = super().state()
        result['idle_timer'] = [self.read(ACTOR+0x168), self.read(ACTOR+0x170)]
        return result

    def call(self, address, owner, args=()):
        if address == 0x521320:
            if self.automatic_row.get('caller', True):
                address = 0x51F640 if self.automatic_row['mission'] == 11 else 0x51F620
        return super().call(address, owner, args)

    def execute(self, row):
        self.automatic_row = row
        result = super().execute(row)
        result['return_signed'] = struct.unpack('<i', dwords(result['return_eax']))[0]
        result['entry'] = ('00521320' if not row.get('caller', True) else
                           '0051F640' if row['mission'] == 11 else '0051F620')
        assert self.automatic_original == [bytes(self.u.mem_read(a, b-a)) for a, b in GUARD_SPANS]
        assert self.text_sha256 == hashlib.sha256(bytes(self.u.mem_read(0x401000, 0x3E0000))).hexdigest()
        result['original_text_and_vtables_unchanged'] = True
        return result


def automatic_guard_inputs():
    base = dict(kind='guard', doing=0, pending=0, prone=0, head=[2688, 2688, 0],
                moving=0, motion=0, mission=5, nav=False, count=15, human=0,
                difficulty=1, delays=[15, 25, 100], mission_start=0, now=100,
                deployer=1, deploy_fire=1, undeploy_delay=-1, immune=0,
                type_id='E1', blockage_path_delay=0, weapon_present=False, game_speed_index=0,
                idle_action_frequency=0.0)
    variants = [dict(name='stationary'), dict(name='moving_paid_head', moving=1, motion=1),
                dict(name='moving_no_head', moving=1, motion=1, head=[0, 0, 0]),
                dict(name='moving_old_pending_no_head', moving=1, motion=1,
                     head=[0, 0, 0], pending=1),
                dict(name='human', human=1), dict(name='not_deployer', deployer=0),
                dict(name='not_deploy_fire', deploy_fire=0),
                dict(name='undeploy_delay_zero', undeploy_delay=0),
                dict(name='negative_undeploy_delay', undeploy_delay=-2),
                dict(name='destination_present', nav=True), dict(name='radiation_immune', immune=1),
                dict(name='deadline_exact', now=25), dict(name='deadline_after', now=26),
                dict(name='hard_exact', now=15, difficulty=0),
                dict(name='hard_after', now=16, difficulty=0),
                dict(name='easy_exact', now=100, difficulty=2),
                dict(name='easy_after', now=101, difficulty=2),
                dict(name='archive_same_cell', archive=[2689, 2689, 416]),
                dict(name='archive_other_cell', archive=[2944, 2688, 0]),
                dict(name='archive_signed_truncate', position=[-1, -255, 0], archive=[0, 0, 416]),
                dict(name='archive_word_wrap', archive=[2688+0x1000000, 2688, 0]),
                dict(name='signed_frame_boundary', now=-2147483648,
                     mission_start=2147483647, delays=[1, 1, 1]),
                dict(name='signed_frame_after', now=-2147483647,
                     mission_start=2147483647, delays=[1, 1, 1]),
                dict(name='signed_negative_delay', now=0, delays=[-1, -1, -1])]
    variants += [dict(name=f'signed_count_{count}', count=count)
                 for count in (-2147483648, -1, 0, 1, 65536, 2147483647)]
    return [base | variant | dict(name=f'{name}_{variant["name"]}', mission=mission,
                                 caller=not (mission == 11 and variant['name'] == 'archive_other_cell'))
            for name, mission in (('Guard', 5), ('Sticky', 6), ('AreaGuard', 11))
            for variant in variants]


class ReacquireGuardFixture(DeployedGuardFixture):
    """Original GI SelectWeapon(NULL)/InRange, empty rescan and idle suffix."""
    def prepare_execution(self, row):
        super().prepare_execution(row)
        u = self.u
        u.mem_write(ACTOR+0x520, dwords(-1))
        u.mem_write(TYPE+0x6A8, dwords(1))
        u.mem_write(TYPE+0xD94, bytes([row['jumpjet']]))
        u.mem_write(WEAPON+0xB4, dwords(1024))
        u.mem_write(WEAPON_OTHER+0xB4, dwords(row['secondary_range']))
        u.mem_write(GUARD_PROJECTILE+0x2A4, b'\1\0')
        for weapon in (WEAPON, WEAPON_OTHER):
            u.mem_write(weapon+0x150, b'\0')
        if row['target']:
            if row['target_kind'] == 'cell':
                u.mem_write(NAV+0x24, struct.pack('<hh', 15, 10))
                u.mem_write(TABLE+(10*512+15)*4, dwords(NAV))
            else:
                # Same supplied original Infantry vtable/type, HP100/Alive1;
                # no registration is supplied to the native greatest-threat list.
                u.mem_write(NAV, bytes(u.mem_read(ACTOR, 0x700)))
                u.mem_write(NAV+0x9C, dwords(2688+row['delta'], 2688, 0))
                u.mem_write(NAV+0x2B4, dwords(0))
                u.mem_write(NAV+0x6C4, dwords(0))
                u.mem_write(NAV+0x68D, b'\0')
        self.guard_before = self.guard_state()


def reacquire_guard_inputs():
    rows, _ = deployed_guard_inputs()
    base = next(row for row in rows if row['name'] == 'nonimmune_retained_Cell_target')
    base = base | dict(type_id='E1', doing=28, immune=0, jumpjet=0,
                       secondary_range=1280, target_kind='entity', delta=1280)
    return ([base | dict(name=f'GI_target_delta{delta}', delta=delta)
             for delta in (1279, 1280, 1281, 1282)]
            + [base | dict(name=f'GI_Cell_range{limit}', target_kind='cell', secondary_range=limit)
               for limit in (1278, 1279, 1280, 1281)]
            + [base | dict(name=f'GI_AreaGuard_empty_jumpjet{jumpjet}', target=False,
                           mission=11, jumpjet=jumpjet)
               for jumpjet in (0, 1)])


def generate_automatic_guard():
    preserved = guard_preserved_corpora()
    result = dict(schema_version=1, preserved_corpora=preserved,
                  automatic_rows=[AutomaticGuardFixture().execute(row) for row in automatic_guard_inputs()],
                  reacquire_rows=[ReacquireGuardFixture().execute(row) for row in reacquire_guard_inputs()])
    assert guard_preserved_corpora() == preserved
    return result


def automatic_guard_metadata():
    fixture = AutomaticGuardFixture()
    out = metadata()
    out['scope'] = ('Original Infantry Guard/Sticky51F620 and AreaGuard51F640 callers, '
                    'automatic deploy5214F7 admission, stationary action, moving Stop/callback/latch '
                    'and signed-1 Foot fallback. Supplied actor/type/sequence/House/map state; '
                    'not a complete retail object lifecycle.')
    out['assumptions'] += [
        'Shares the existing UnloadFixture map/type/prior Mission state and original vtables. Weapon slots are supplied absent to isolate the Guard producer and its empty-scan Foot fallback, not to represent a retail weapon load. Foot+520=-1 is supplied from constructor4D31F1; that constructor is not executed. IdleActionTimer168/170 has explicit zero start/duration from the supplied prior storage; both words and native writes are compared. Empty native registered scan storage is supplied.',
        'House difficulty0/1/2 and Rules AIAutoDeployFrameDelay vector are supplied declared input values, not reader results. Production retail reader validation is separate. Object virtual+48 executes for actor/archive; signed truncation, WORD narrowing and wrapping signed frame admission execute original instructions.',
        'MissionControl Rate0.1, GameOptions speed index0 and Rules IdleActionFrequency0 are supplied for the committed selector and idle-action timer/SpeedNormalize. Original class caller, Foot fallback, cadence arithmetic and ScenarioRandomRanged execute without gameplay substitutions. Signed Count27 may refuse DoAction or return the caller fallback sentinel-1.',
        'AreaGuard_archive_other_cell runs the original shared521320 shim only, returning-1. The subsequent FootAreaGuard return-to-post navigation is outside this automatic-deploy comparison; its path state is not supplied or synthesized.',
        'Reacquire controls extend the existing DeployedGuardFixture with nonimmune GI Doing28, supplied DeployFireWeapon1, PrimaryRange1024/SecondaryRange input, ProjectileAG1/AA0 and AreaFire0. Original SelectWeapon(NULL)5218E0, CanFireAt6F77B0/InRange6F7220, empty greatest-threat scan, AssignTargetNULL and Infantry/Foot EnterIdle execute; no baseFireAt callback occurs.',
        'Reacquire entity targets copy the supplied original Infantry state with physical X delta varied and Doing0; Cells use real Cell15,10 coordinates. Target registration storage remains empty. Controls establish selected-slot range rounding and target/firing-latch cleanup, not populated scanner or full projectile behavior.',
    ]
    out['entry_points'].update(guard_caller=0x51F620, area_guard_caller=0x51F640,
                               deployed_reacquire=0x51F330, foot_guard=0x4D5070,
                               foot_area_guard=0x4D6F00)
    out['preserved_corpora'] = guard_preserved_corpora()
    out['original_text'] = dict(start='00401000', bytes=0x3E0000, sha256=fixture.text_sha256)
    out['original_slices'] += [dict(start=f'{a:08X}', end_exclusive=f'{b:08X}', hex=code.hex(),
                                  sha256=hashlib.sha256(code).hexdigest())
                             for (a,b),code in zip(GUARD_SPANS,fixture.automatic_original)]
    return out


class MoveFixture(UnloadFixture):
    """The existing action/navigation fixture, entered at the original Move slot."""
    def __init__(self):
        super().__init__()
        self.text_sha256 = hashlib.sha256(bytes(self.u.mem_read(0x401000, 0x3E0000))).hexdigest()

    def prepare_execution(self, row):
        super().prepare_execution(row)
        self.u.mem_write(0xA8B238, dwords(int(row['game_mode_nonzero'])))

    def execute(self, row):
        result = super().execute(row)
        result['return_signed'] = struct.unpack('<i', dwords(result['return_eax']))[0]
        assert self.text_sha256 == hashlib.sha256(bytes(self.u.mem_read(0x401000, 0x3E0000))).hexdigest()
        return result


def move_inputs():
    base = dict(kind='move', mission=2, pending=0, prone=0, nav=True, game_mode_nonzero=True,
                moving=0, motion=0, head=[0, 0, 0], count=6,
                blockage_path_delay=0, type_id='E1', weapon_present=False)
    rows = [base | dict(name=f'doing{doing}_human{human}', doing=doing, human=human)
            for doing in (27, 28, 29, 30) for human in (0, 1)]
    rows += [base | dict(doing=28, human=0, name=name) | control for name, control in (
        ('delay_below_minus_one', dict(undeploy_delay=-2)),
        ('delay_zero_inherits', dict(undeploy_delay=0)),
        ('delay_positive_inherits', dict(undeploy_delay=60)),
        ('no_undeploy_frames_refuses_action', dict(counts=[[31, 0]])),
        ('signed_undeploy_frames', dict(counts=[[31, -1]])),
        ('not_deployer_still_uses_doing', dict(deployer=0)),
        ('doing26_inherits', dict(doing=26)),
        ('doing31_inherits', dict(doing=31)),
    )]
    return rows


def generate_move():
    fixture = MoveFixture()
    return [fixture.execute(row) for row in move_inputs()]


def move_metadata():
    return provenance(
        scope='Whole original Infantry Move51F660, original human NULL-setter refusal, '
              'AI unforced Undeploy31 and signed sequence return, and Foot4D4200 moving cadence. '
              'Supplied deployed state and native sequence records; no whole match or deployment producer claim.',
        assumptions=[
            'Shares UnloadFixture original Infantry and Walk vtables, action/Stage/type/House/map envelope. '
            'Doing27..30 and absent/present sequence records are supplied inputs. Type UndeployDelay is supplied, '
            'not an INI-reader result. Every row retains a Cell NavCom so Foot fallback takes moving cadence.',
            'The human original51AA40 NULL setter refuses before mutation for Doing27..30; '
            'the handler still returns1. AI delay<=-1 calls unforced51D6F0(31,0,0) and returns raw Count31 '
            'even if action admission fails. Other rows execute original4D4200 and ScenarioRandomRanged(0,2).',
            'ModeA8B238=1 supplies the nonzero-mode branch of House50B730, with PlayerControl1ED=0. '
            'MissionControl Move Rate0.1 and Scenario/Main/MapGen seed31 are supplied through the existing fixture. '
            'All complete RNG objects before/after are retained; no RNG or cadence output is synthesized. '
            'MissionClass dispatch due/alive gate and timer epilogue are outside this receiver boundary.',
            'Disabled audio observes native sound requests. No radio/temporal/bunker/transport links, '
            'nonempty Foot vector or paid movement head. Existing source corpora are not regenerated by this mode.',
        ],
        substitutions=['Only the existing OS InterlockedIncrement/Decrement emulation; '
                       'all gameplay text and class vtables remain original.'],
        entry_points={'infantry_move': 0x51F660, 'foot_move': 0x4D4200,
                      'destination': 0x51AA40, 'do_action': 0x51D6F0,
                      'controlled_by_human': 0x50B730, 'rng_ranged': 0x65C7E0},
    )


if __name__ == '__main__':
    args=sys.argv[1:]
    if '--mission-move' in args:
        args.remove('--mission-move')
        finish_vectors(generate_move, Path(__file__).with_name('infantry_mission_move.json'),
                       provenance=move_metadata, argv=args)
    elif '--automatic-guard' in args:
        args.remove('--automatic-guard')
        finish_vectors(generate_automatic_guard,Path(__file__).with_name('infantry_auto_deploy.json'),
                       provenance=automatic_guard_metadata,argv=args,
                       source_paths={'generator':Path(__file__),
                                     'native_oracle':Path(__file__).parents[1]/'native_oracle.py',
                                     'map_queries':Path(__file__).with_name('map_queries.py')})
    elif '--deployed-guard' in args:
        args.remove('--deployed-guard')
        finish_vectors(generate_deployed_guard,Path(__file__).with_name('infantry_deployed_guard.json'),
                       provenance=deployed_guard_metadata,argv=args,
                       source_paths={'generator':Path(__file__),
                                     'native_oracle':Path(__file__).parents[1]/'native_oracle.py',
                                     'map_queries':Path(__file__).with_name('map_queries.py')})
    elif '--mission-unload' in args:
        args.remove('--mission-unload')
        finish_vectors(generate_unload,Path(__file__).with_name('infantry_mission_unload.json'),
                       provenance=unload_metadata,argv=args,source_paths={'generator':Path(__file__)})
    else:
        finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata,argv=args)
