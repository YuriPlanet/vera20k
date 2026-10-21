"""Bounded native House power consumers, composed from existing fixture owners.

Original instructions own all arithmetic, timers, rate updates, radar decisions
and advice predicates. This fixture supplies admitted state and presentation
interfaces; see house_power_consumers.md for coverage and reproduction.
"""
import hashlib,json,struct,sys
from collections import Counter
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX,UC_X86_REG_EBP,UC_X86_REG_EBX,UC_X86_REG_ECX,
    UC_X86_REG_EDI,UC_X86_REG_ESI,UC_X86_REG_ESP,UC_X86_REG_FPCW,
)
from tools import native_oracle as native
from tools.spatial_oracle import building_repair as br,slave_manager as sm
from tools.spatial_oracle import factory_cadence as fc,time_to_build as ttb,building_construction as bc
from tools.spatial_oracle.building_sale import ret
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.unit_source_scatter import SCENARIO

ARENA=0x24000000
PENDING,PENDING_TYPE,COUNTRY=ARENA,ARENA+0x1000,ARENA+0x4000
FACTORY,FACTORY_LIST=ARENA+0x6000,ARENA+0x7000
PEER,PEER_TYPE=ARENA+0x8000,ARENA+0x9000
BUILDING_LIST,POWER_TYPES,OWNED_COUNTS,HOUSE_LIST=ARENA+0xB000,ARENA+0xB100,ARENA+0xB200,ARENA+0xB400
RADAR,RADAR_TYPE,RADAR2=ARENA+0xC000,ARENA+0xD000,ARENA+0xF000
TEXT=ARENA+0x11000
IMAGE=native.image_bytes()
CODE=[(native.IMAGE_BASE+rva,raw,IMAGE[offset:offset+raw])
      for rva,offset,raw,_,flags in native._sections(IMAGE) if flags & 0x20000000 and raw]
SP=native.STACK_BASE+native.STACK_SIZE-0x1000

class Fixture:
    # Data bindings and observation only; gameplay remains in the original PE.
    def __init__(self,health=374,game_speed=1):
        case=dict(name='power_consumer_order_research',strength=750,cost=800,health=health,
                  repairing=True,human=True,frame=196,seed=31)
        self.u,_,self.read,self.events=br.fixture(case)
        self.draws,self.repair_state=br.prepare_update(self.u,self.read,self.events,case)
        self.u.mem_map(ARENA,0x20000)
        self.b,self.t,self.h,self.r=sm.YAREFN,sm.YTYPE,br.HOUSE,br.RULES
        self.visited=Counter();self.entry_trace=[];self.presentation=[]
        self.before_code=self.code_check()
        self.u.hook_add(UC_HOOK_CODE,self.observe)
        self.u.mem_write(self.h,dwords(0x7EA8A0)) # ctor4F5DCC
        self.u.mem_write(self.b+0x660,b'\1')
        self.u.mem_write(self.b+0x544,dwords(health))
        self.u.mem_write(self.b+0x74,b'\1')
        self.u.mem_write(self.b+0x81,b'\0')
        self.u.mem_write(self.b+0x41B,b'\1')
        self.u.mem_write(self.t+0xEE0,dwords(200,0))
        self.u.mem_write(self.t+0xDF8,dwords(0)) # supplied type-index/counter admission
        self.u.mem_write(PEER,dwords(0x7E3EBC))
        self.u.mem_write(PEER+0x520,dwords(PEER_TYPE))
        self.u.mem_write(PEER+0x660,b'\1')
        self.u.mem_write(PEER+0x74,b'\1')
        self.u.mem_write(PEER+0x81,b'\0')
        self.u.mem_write(PEER+0x41B,b'\1')
        self.u.mem_write(PEER+0x6C,dwords(500))
        self.u.mem_write(PEER+0x544,dwords(500))
        self.u.mem_write(PEER_TYPE+0xA0,dwords(500))
        self.u.mem_write(PEER_TYPE+0xEE0,dwords(0,100))
        for provider in (RADAR,RADAR2):
            self.u.mem_write(provider,dwords(0x7E3EBC))
            self.u.mem_write(provider+0x520,dwords(RADAR_TYPE))
            self.u.mem_write(provider+0x21C,dwords(self.h))
            self.u.mem_write(provider+0x660,b'\1')
            self.u.mem_write(provider+0x74,b'\1')
            self.u.mem_write(provider+0x41B,b'\1')
            self.u.mem_write(provider+0xAC,dwords(5))
            self.u.mem_write(provider+0xB4,dwords(-1))
            self.u.mem_write(provider+0x6C,dwords(500))
            self.u.mem_write(provider+0x544,dwords(500))
        self.u.mem_write(RADAR_TYPE+0x16A4,b'\1')
        self.u.mem_write(self.h+0x6C,dwords(BUILDING_LIST))
        self.u.mem_write(self.h+0x78,dwords(3))
        self.u.mem_write(BUILDING_LIST,dwords(self.b,PEER,RADAR))
        self.u.mem_write(self.h+0x2A4,dwords(-1,0,0,-1,0,0))
        self.u.mem_write(self.h+0x57D4,dwords(100,0,1000)) # prior running funds timer
        self.u.mem_write(self.h+0x5550,dwords(0,OWNED_COUNTS,3))
        self.u.mem_write(OWNED_COUNTS,dwords(1,0,0))
        self.u.mem_write(self.r+0x8B0,dwords(POWER_TYPES))
        self.u.mem_write(POWER_TYPES,dwords(self.t,self.t,self.t))
        self.u.mem_write(self.r+0x16A8,struct.pack('<d',2.0)) # SpeakDelay, stock scalar
        self.u.mem_write(self.r+0x14C0,struct.pack('<d',ttb.widened(.6))) # MessageDelay scalar
        self.u.mem_write(0xA8B238,dwords(1))
        self.u.mem_write(br.PLAYER_PTR,dwords(self.h))
        self.u.mem_write(SCENARIO+0x34A4,b'\0')
        self.u.mem_write(0xA8B538,dwords(0))
        self.u.mem_write(0xA8F040,dwords(0))
        self.u.mem_write(0x87F7E8+0x14D8,b'\1')
        self.u.mem_write(0x87F7E8+0x14B0,dwords(1))
        self.u.mem_write(0x87F7E8+0x14AC,dwords(0))
        self.u.mem_write(TEXT,b'P\0o\0w\0e\0r\0\0\0')
        bc.invoke(self.u,0x5FA350,br.GAME_SPEED)
        self.u.mem_write(br.GAME_SPEED,dwords(game_speed))
        self.game_speed=game_speed
        self.bind_build(ttb.case('unit',600,power=(99,100)))
        self.before_rng=self.rng()

    def code_check(self):
        for address,size,expected in CODE:
            if bytes(self.u.mem_read(address,size))!=expected:
                raise RuntimeError('Original executable bytes changed')
        return True

    def rng(self):
        return {name:bytes(self.u.mem_read(address,0x3F4)).hex()
                for name,address in [('main',0x886B88),('scenario',SCENARIO+0x218)]}

    def bind_build(self,row):
        # Relocate the existing Time_To_Build fixture's data bindings, not its algorithm.
        targets=[(ttb.OBJECT,0x1000,PENDING),(ttb.TYPE,0x2000,PENDING_TYPE),
                 (ttb.HOUSE,0x6000,self.h),(ttb.COUNTRY,0x1000,COUNTRY),
                 (ttb.RULES,0x5000,self.r)]
        for address,blob in ttb.fixture_writes(row).items():
            if address==ttb.RULES_GLOBAL:
                destination,blob=address,dwords(self.r)
            else:
                destination=next(dst+address-src for src,size,dst in targets if src<=address<src+size)
            if address==ttb.OBJECT+0x21C:blob=dwords(self.h)
            if address==ttb.OBJECT+ttb.KINDS[row['kind']][1]:blob=dwords(PENDING_TYPE)
            if address==ttb.HOUSE+0x34:blob=dwords(COUNTRY)
            self.u.mem_write(destination,blob)
        self.build_input=row
        self.u.mem_write(self.h+0x24,dwords(fc.HOUSE_MONEY_VTABLE))

    def new_factory(self,offset=0,*,status='live',owner=None,rate=0):
        pointer=FACTORY+offset
        raw=bytearray(fc.new_factory(self.build_input['cost']))
        struct.pack_into('<I',raw,0,0x7E88D0) # ctor4C991A
        struct.pack_into('<I',raw,0x58,0 if status=='null' else PENDING)
        struct.pack_into('<I',raw,0x6C,self.h if owner is None else owner)
        struct.pack_into('<i',raw,0x38,rate)
        raw[0x70]=int(status in ('held','complete'))
        if status=='complete':struct.pack_into('<i',raw,0x24,54)
        self.u.mem_write(pointer,bytes(raw))
        return pointer

    def set_factory_list(self,pointers):
        self.u.mem_write(FACTORY_LIST,dwords(*pointers))
        self.u.mem_write(0xA83E34,dwords(FACTORY_LIST))
        self.u.mem_write(0xA83E40,dwords(len(pointers)))
        self.u.mem_write(HOUSE_LIST,dwords(self.h))
        self.u.mem_write(0xA8022C,dwords(HOUSE_LIST))
        self.u.mem_write(0xA80238,dwords(1))

    def observe(self,u,address,size,_):
        self.visited[(address,size)]+=1
        if address in (0x4C9B20,0x4F8440,0x508C30,0x4CA6E0,0x508DF0,0x508F60,0x50BC90,
                       0x4F8B08,0x4F8C56,0x49FAE0,0x752700,0x65C7E0,0x5FB2E0):
            self.entry_trace.append(dict(address=f'{address:08X}',frame=self.read(br.FRAME),
                                         receiver=u.reg_read(UC_X86_REG_ECX),
                                         health=self.read(self.b+0x6C),sampled_health=self.read(self.b+0x544),
                                         output=self.read(self.h+0x53A4),drain=self.read(self.h+0x53A8),
                                         power_dirty=u.mem_read(self.h+0x5778,1)[0],
                                         radar_dirty=u.mem_read(self.h+0x5779,1)[0],
                                         factory_stage=self.read(FACTORY+0x24),
                                         factory_rate=self.read(FACTORY+0x38),
                                         factory_timer=list(struct.unpack('<iii',u.mem_read(FACTORY+0x2C,12)))))
        sp=u.reg_read(UC_X86_REG_ESP)
        if address==0x4068E0: # cdecl debug logging, return ignored
            self.presentation.append(dict(kind='debug_log',frame=self.read(br.FRAME)))
            ret(u,self.read,0,0)
        elif address==0x750920: # audio device/sample boundary; return ignored
            self.presentation.append(dict(kind='sound',index=u.reg_read(UC_X86_REG_ECX),
                                           frame=self.read(br.FRAME)))
            ret(u,self.read,8,0)
        elif address==0x734E60: # localized text lookup; caller treats pointer as presentation
            self.presentation.append(dict(kind='text_lookup',key=u.reg_read(UC_X86_REG_ECX),
                                           args=[self.read(sp+4),self.read(sp+8)],
                                           frame=self.read(br.FRAME)))
            # Complete calling convention is verified from original734E60.
            ret(u,self.read,8,TEXT)
        elif address==0x5D3BA0: # MessageList presentation; return ignored
            self.presentation.append(dict(kind='message',args=[self.read(sp+i*4) for i in range(1,8)],
                                           frame=self.read(br.FRAME)))
            ret(u,self.read,28,0)

    def snapshot(self,phase):
        raw=bytes(self.u.mem_read(FACTORY,fc.FACTORY_SIZE))
        return dict(phase=phase,frame=self.read(br.FRAME),game_speed=self.read(br.GAME_SPEED),
                    blackout_timer=list(struct.unpack('<iii',self.u.mem_read(self.h+0x2A4,12))),
                    radar_outage_timer=list(struct.unpack('<iii',self.u.mem_read(self.h+0x2B0,12))),
                    health=self.read(self.b+0x6C),
                    sampled_health=self.read(self.b+0x544),output=self.read(self.h+0x53A4),
                    drain=self.read(self.h+0x53A8),
                    power_dirty=self.u.mem_read(self.h+0x5778,1)[0],
                    radar_dirty=self.u.mem_read(self.h+0x5779,1)[0],
                    drained_positive_source=self.u.mem_read(self.h+0x577B,1)[0],
                    radar_available=self.u.mem_read(0x87F7E8+0x14D8,1)[0],
                    eva_guard=self.read(0xA8F040),
                    low_power_timer=list(struct.unpack('<iii',self.u.mem_read(self.h+0x57BC,12))),
                    factory=fc.state(raw,self.read(self.h+0x30C),self.read(self.h+0x2DC)))

    def sample(self):
        self.u.reg_write(UC_X86_REG_ESI,self.b)
        self.u.reg_write(UC_X86_REG_ESP,SP)
        native.run_checked(self.u,0x440042,0x440072)

    def house_prefix(self,global_factory=False):
        self.u.reg_write(UC_X86_REG_ESP,SP)
        self.u.mem_write(SP,dwords(native.RET_MAGIC))
        self.u.reg_write(UC_X86_REG_ECX,self.h)
        begin=0x55B66A if global_factory else 0x4F8440
        native.run_checked(self.u,begin,0x4F8511,count=2000000)

    def advice(self):
        self.u.reg_write(UC_X86_REG_ESI,self.h)
        self.u.reg_write(UC_X86_REG_EBP,0)
        self.u.reg_write(UC_X86_REG_EDI,self.h+0x57D4)
        self.u.reg_write(UC_X86_REG_ESP,SP)
        native.run_checked(self.u,0x4F8B08,0x4F8DB1,count=2000000)

    def receipt(self):
        self.code_check()
        if self.rng()!=self.before_rng:raise RuntimeError('RNG changed in no-draw bounded route')
        instructions=[]
        for (address,size),count in sorted(self.visited.items()):
            if address==native.RET_MAGIC:continue
            if not any(a<=address and address+size<=a+n for a,n,_ in CODE):
                raise RuntimeError(f'Unexpected non-native instruction {address:X}')
            _,raw=native.file_span(IMAGE,address,size)
            instructions.append(dict(address=f'{address:08X}',size=size,visits=count,
                                     pe_bytes_sha256=hashlib.sha256(raw).hexdigest()))
        return dict(original_executable_sections_match_file_before_and_after=True,
                    x87_fpcw=f'{self.u.reg_read(UC_X86_REG_FPCW):04X}',
                    instruction_visits=sum(r['visits'] for r in instructions),
                    unique_instructions=len(instructions),instructions=instructions,
                    entry_trace=self.entry_trace,presentation=self.presentation,
                    repair_draws=self.draws,rng_before=self.before_rng,rng_after=self.rng())

def rewrite_controls():
    output=[]
    inputs=[ttb.case('unit',cost,power=power)
            for cost,power in [(0,(101,100)),(85,(101,100)),(86,(101,100)),
                              (600,(99,100)),(600,(101,100)),(600,(0,100)),
                              (21858,(101,100)),(22000,(101,100)),(30000,(101,100))]]
    for row in inputs:
        f=Fixture();f.bind_build(row)
        pointers=[f.new_factory(i*0x100,status=status,owner=f.h+0x6000 if status=='foreign' else None,rate=77)
                  for i,status in enumerate(('live','held','complete','null','foreign'))]
        f.set_factory_list(pointers)
        before=[bytes(f.u.mem_read(p,fc.FACTORY_SIZE)) for p in pointers]
        bc.invoke(f.u,0x4CA6E0,f.h)
        after=[bytes(f.u.mem_read(p,fc.FACTORY_SIZE)) for p in pointers]
        changes=[[i for i,(a,b) in enumerate(zip(old,new)) if a!=b] for old,new in zip(before,after)]
        output.append(dict(input=row,statuses=['live','held','complete','null','foreign'],
                           rates=[struct.unpack_from('<i',a,0x38)[0] for a in after],
                           changed_byte_offsets=changes,
                           timer_bytes_before=[a[0x2C:0x38].hex() for a in before],
                           timer_bytes_after=[a[0x2C:0x38].hex() for a in after],
                           receipt=f.receipt()))
    return output

def timeline(prior_dirty):
    f=Fixture()
    pointer=f.new_factory();f.set_factory_list([pointer])
    f.u.mem_write(f.h+0x5778,b'\1')
    f.u.mem_write(br.FRAME,dwords(100));f.house_prefix()
    bc.invoke(f.u,0x4C9EA0,pointer,0)
    start_accepted=bool(f.u.reg_read(UC_X86_REG_EAX)&0xFF)
    trace=[f.snapshot('initial_assessment_and_native_factory_start')]
    # Only Factory AI executes before196; damaged Building/repair prior state is supplied.
    for frame in range(100,196):
        f.u.mem_write(br.FRAME,dwords(frame));bc.invoke(f.u,0x4C9B20,pointer)
    trace.append(f.snapshot('factory_prior_state_before_join'))
    for frame in range(196,213):
        f.u.mem_write(br.FRAME,dwords(frame))
        f.sample();trace.append(f.snapshot('plant_sample'))
        if frame==196:
            if prior_dirty:f.u.mem_write(f.h+0x5778,b'\1')
            br.run_update(f.u,f.repair_state)
            trace.append(f.snapshot('paid_repair'))
        f.house_prefix(global_factory=True)
        trace.append(f.snapshot('actual_global_factory_then_house_prefix'))
        # Advice is a later native slice. Its intervening ordinary House prior state is supplied.
        f.advice();trace.append(f.snapshot('later_local_house_advice'))
    return dict(prior_power_dirty_at_repair=prior_dirty,start_accepted=start_accepted,trace=trace,
                events=f.events,receipt=f.receipt())

def radar_controls():
    cases=[
        dict(name='surplus_provider'),
        dict(name='deficit_provider',power=(99,100)),
        dict(name='free_radar_deficit_no_provider',power=(99,100),free=True,radar_type=False),
        dict(name='nonlocal_deficit',power=(99,100),local=False),
        dict(name='current_selling',mission=19),
        dict(name='queued_selling',queued=19),
        dict(name='unmarked',marked=False),
        dict(name='limbo',limbo=True),
        dict(name='offline',online=False),
        dict(name='emp',emp=1),
        dict(name='warped',warped=True),
        dict(name='first_emp_then_eligible_provider',emp=1,second=True),
        dict(name='first_warped_then_eligible_provider',warped=True,second=True),
        dict(name='first_selling_then_eligible_provider',mission=19,second=True),
        dict(name='free_radar_with_active_outage',free=True,outage=(196,0,2)),
        dict(name='free_radar_with_expired_outage',free=True,outage=(194,0,2)),
    ]
    rows=[]
    for case in cases:
        f=Fixture();f.set_factory_list([])
        f.u.mem_write(f.h+0x53A4,dwords(*case.get('power',(101,100))))
        f.u.mem_write(f.h+0x5779,b'\1')
        f.u.mem_write(br.PLAYER_PTR,dwords(f.h if case.get('local',True) else f.h+0x6000))
        f.u.mem_write(SCENARIO+0x34A4,bytes([case.get('free',False)]))
        f.u.mem_write(RADAR_TYPE+0x16A4,bytes([case.get('radar_type',True)]))
        f.u.mem_write(RADAR+0xAC,dwords(case.get('mission',5)))
        f.u.mem_write(RADAR+0xB4,dwords(case.get('queued',-1)))
        f.u.mem_write(RADAR+0x74,bytes([case.get('marked',True)]))
        f.u.mem_write(RADAR+0x81,bytes([case.get('limbo',False)]))
        f.u.mem_write(RADAR+0x660,bytes([case.get('online',True)]))
        f.u.mem_write(RADAR+0x504,dwords(case.get('emp',0)))
        f.u.mem_write(RADAR+0x270,bytes([case.get('warped',False)]))
        f.u.mem_write(f.h+0x2B0,dwords(*case.get('outage',(-1,0,0))))
        if case.get('second'):
            f.u.mem_write(f.h+0x78,dwords(4))
            f.u.mem_write(BUILDING_LIST,dwords(f.b,PEER,RADAR,RADAR2))
        bc.invoke(f.u,0x508DF0,f.h)
        rows.append(dict(input=case,output=f.snapshot('actual_radar_update'),receipt=f.receipt()))
    return rows

def advice_controls():
    rows=[]
    cases=[
        dict(name='local_short_first',power=(99,100),guard=0),
        dict(name='local_short_repeat',power=(99,100),guard=1),
        dict(name='local_restored',power=(101,100),guard=1),
        dict(name='short_without_power_type',power=(99,100),guard=1,count=0),
        dict(name='nonlocal_short',power=(99,100),guard=0,local=False),
    ]
    cases += [dict(name=f'local_short_speed_{speed}',power=(99,100),guard=0,speed=speed)
              for speed in range(8)]
    for case in cases:
        f=Fixture(game_speed=case.get('speed',1));f.set_factory_list([])
        f.u.mem_write(f.h+0x53A4,dwords(*case['power']))
        f.u.mem_write(0xA8F040,dwords(case['guard']))
        f.u.mem_write(OWNED_COUNTS,dwords(case.get('count',1),0,0))
        f.u.mem_write(br.PLAYER_PTR,dwords(f.h if case.get('local',True) else f.h+0x6000))
        f.advice()
        rows.append(dict(input=case,output=f.snapshot('actual_later_advice'),events=f.events,
                         receipt=f.receipt()))
    return rows

def blackout_controls():
    rows=[]
    for remaining,start,duration in ((0,194,2),(1,195,2),(2,196,2)):
        for dirty in (False,True):
            f=Fixture();f.set_factory_list([])
            f.u.mem_write(f.h+0x53A4,dwords(0,100))
            f.u.mem_write(f.h+0x5778,bytes([dirty]))
            f.u.mem_write(f.h+0x5779,b'\0')
            f.u.mem_write(f.h+0x2A4,dwords(start,0,duration))
            before=f.snapshot('supplied_prior_timer_state')
            f.house_prefix()
            rows.append(dict(input=dict(remaining_control=remaining,prior_dirty=dirty,
                                       frame=196,start=start,opaque=0,duration=duration),
                             before=before,output=f.snapshot('actual_house_prefix'),
                             receipt=f.receipt()))
    return rows

def blackout_setter_controls():
    # Execute the whole existing House setter. The prior timer/cached House,
    # current frame, duration and opaque stack residue are boundary inputs;
    # ForceShield/Spy producers and subsequent House assessment are not run.
    cases=[
        dict(name='no_prior',prior_timer=(0,0,0),duration=30,cached_output=99,
             prior_dirty=False,prior_radar_dirty=False),
        dict(name='shorter_overlap',prior_timer=(100,0,120),duration=4,cached_output=0,
             prior_dirty=False,prior_radar_dirty=False),
        dict(name='longer_overlap',prior_timer=(100,0,120),duration=80,cached_output=0,
             prior_dirty=False,prior_radar_dirty=False),
        dict(name='zero_replacement',prior_timer=(100,0,120),duration=0,cached_output=0,
             prior_dirty=True,prior_radar_dirty=True),
    ]
    rows=[]
    for case in cases:
        f=Fixture();f.set_factory_list([])
        frame,stack_opaque=196,0
        f.u.mem_write(br.FRAME,dwords(frame))
        f.u.mem_write(f.h+0x53A4,dwords(case['cached_output'],100))
        f.u.mem_write(f.h+0x5778,bytes([case['prior_dirty'],case['prior_radar_dirty']]))
        f.u.mem_write(f.h+0x2A4,dwords(*case['prior_timer']))
        #50BCB0 reads entrySP-8 after its local allocation and PUSH ESI. This
        #unread timer dword is retained as supplied stack residue, not math.
        f.u.mem_write(bc.SP-8,dwords(stack_opaque))
        before=f.snapshot('supplied_prior_setter_state')
        before_visits=f.visited.copy()
        bc.invoke(f.u,0x50BC90,f.h,case['duration'])
        receipt=f.receipt()
        setter=[]
        for instruction in receipt['instructions']:
            key=int(instruction['address'],16),instruction['size']
            visits=f.visited[key]-before_visits[key]
            if visits:setter.append(dict(instruction,visits=visits))
        rows.append(dict(input=dict(**case,frame=frame,stack_opaque=stack_opaque),
                         before=before,output=f.snapshot('actual_blackout_replacement_setter'),
                         setter_execution=dict(instruction_visits=sum(i['visits'] for i in setter),
                                               unique_instructions=len(setter),instructions=setter),
                         receipt=receipt))
    return rows

def generate():
    packet=dict(schema_version=1,kind='bounded-original-house-power-consumers',
                limits=[
                    'Composition of existing native fixture owners; no Rust-derived gameplay answers. The preserved44 research controls are extended with first-warped and first-Selling radar-provider order controls and four whole50BC90 replacement-setter controls.',
                    'Supplied admitted Strength750/Power200/HP374 plant, drain100 consumer, zero-power Radar provider, House/country and Cost600 pending Unit state. Constructors, Unlimbo, arbitrary damage and full Building/common AI excluded. Factory starts live/suspended0;4C9EA0 returnsfalse and retains initial start100/duration0. Native FactoryAI produces the joined frame195 prior; this is not Begin_Production startup coverage.',
                    'Real global Factory loop55B66A enters real Factory AI and real House Update, stopped4F8511 after power/radar/SpySat prefix. Full508C30/4CA6E0/Time_To_Build execute.',
                    'Later local advice4F8B08..4F8DB1 executes separately; interposed House state (anger/activation/teams/etc.) is supplied and does not execute. Full GameOptions5FA350 constructor executes; selected speed1 or speed controls0..7 are explicit boundary inputs, not Options dialog initialization proof.',
                    'Native radio is not reached. The existing repair fixture supplies GetCurrentFrame/flash/active-slot allocation and voice-device callbacks. The374-to382 repair reaches two synthetic N00/N02 slot-allocation requests; their actual allocation, IDs, Anim lifecycle and RNG footprint are excluded, so unchanged RNG here does not certify full stock GAPOWR repair. Extra presentation boundaries are debug logging, localized message text and MessageList dispatch.',
                    'Numeric rewrite/status/provider cases are explicit boundary controls, not a claim all combinations occur in stock production. EMP, independent online1/warp1 and active radar-outage inputs have no ordinary Rust producer represented here; nonlocal native radar output is not the per-owner Rust projection. Low-power speak timer is written but no reader is established.',
                    'Whole50BC90 executes with supplied current frame196, prior timer/cached totals/dirty flags and duration30/4/80/0. Its timer+2A8 dword comes from explicit entrySP-8 residue0 and has no represented reader. This establishes replacement semantics and the power-only dirty write, not ForceShield/Spy producer lifecycle or numeric conversion.',
                    'Full Main/Scenario RNG buffers and original executable bytes match before/after. Ambient x87FPCW0E7F is supplied by the existing native fixture; no active-game invariant is claimed. This is not whole House AI/Logic, loader, rendering, audio-device or object equivalence.'
                ],
                rewrite_controls=rewrite_controls(),
                repair_timelines=[timeline(False),timeline(True)],
                radar_controls=radar_controls(),advice_controls=advice_controls(),
                blackout_controls=blackout_controls(),blackout_setter_controls=blackout_setter_controls())
    return packet



def metadata():
    return native.provenance(
        scope='Bounded original Building sample440042, paid repair450630, global Factory55B66A/AI4C9B20 and House4F8440 prefix through4F8511; full power508C30, Factory rate4CA6E0/Time_To_Build6F47A0, radar508DF0, whole blackout setter50BC90 and separately later advice4F8B08..4F8DB1. Selected timers/order/consumers, not whole House/object/startup equivalence.',
        assumptions=[
            'Strength750/Power200/HP374 admitted generator with sample374; drain100 admitted consumer, zero-power Radar provider; House/country/type vectors and registered mode inputs supplied through existing building_repair and Time_To_Build owners. Factory starts already live; original4C9EA0 refuses restart and retains initial timer duration0. Native FactoryAI produces the frame195 joined prior.',
            'Rules scalars supplied through existing fixture owners: BuildSpeed.7 widened from f32, MultipleFactory.8f32, Country/BuildTimeMultiplier1f32, low-power factors1/.5/.8f32, RepairStep8, RepairPercent15percent, RepairRate.016f32 widened; SpeakDelay2 and MessageDelay.6f32 widened. These are explicit scalar bindings, not new reader/default claims.',
            'Ambient x87FPCW0E7F supplied by the reused native owner. Each control checks unchanged original executable sections, all visited PE instructions and complete Main/Scenario RNG buffers. Numeric and status controls do not establish all combinations are reachable in retail.',
            'Global Factory loop executes before House power/radar prefix, stopped before anger4F8511. Later advice4F8B08..4F8DB1 executes separately with the interposed House prior supplied. Full House/Logic AI, startup, production delivery, SW/Spy/ForceShield and presentation are excluded.',
            'Radar local output is Tactical+14D8, not House-persisted state. Nonlocal, EMP, independent online1/warp1 and active radar-outage controls remain native bounds; the ordinary per-owner Rust derived projection is compared only for represented local inputs.',
            'Whole blackout setter50BC90 receives supplied frame196, prior timer/cached totals/dirty bytes and duration30/4/80/0. It writes timer+2A8 from supplied entrySP-8 opaque residue0, for which no represented reader is established. ForceShield/Spy producer lifecycle and caller numeric conversion are excluded.',
        ],
        substitutions=[
            'Existing repair fixture GetCurrentFrame/flash/active-slot allocation and voice-device boundaries are retained. The synthetic374-to382 N00/N02 requests exclude real Anim allocation/lifecycle/IDs and its RNG footprint; unchanged RNG here is not full stock repair parity.',
            'Debug log4068E0, sample/device750920, localized text734E60 and MessageList5D3BA0 are presentation-only interfaces with original calling conventions. Health, timer, Factory rate/cadence, radar and advice decisions are never answered.',
        ],
        entry_points={'building_health_sample':0x440042, 'paid_repair':0x450630,
            'global_factory_loop':0x55B66A, 'house_prefix':0x4F8440,
            'house_power':0x508C30, 'factory_rate':0x4CA6E0,
            'time_to_build':0x6F47A0, 'factory_start':0x4C9EA0,
            'factory_ai':0x4C9B20, 'radar':0x508DF0,
            'later_advice':0x4F8B08, 'blackout_setter':0x50BC90, 'game_options_ctor':0x5FA350,
            'speed_normalize':0x5FB2E0})


def main(argv=None):
    root=Path(__file__).resolve().parents[2]
    sources=SOURCE_PATHS
    native.finish_vectors(generate,Path(__file__).with_suffix('.json'),
        provenance=metadata,argv=argv,source_paths={name:root/name for name in sources})


SOURCE_PATHS = ('tools/native_oracle.py', 'tools/spatial_oracle/__init__.py', 'tools/spatial_oracle/building_body_rules.py', 'tools/spatial_oracle/building_construction.py', 'tools/spatial_oracle/building_repair.py', 'tools/spatial_oracle/building_sale.py', 'tools/spatial_oracle/factory_cadence.py', 'tools/spatial_oracle/harvest_field.py', 'tools/spatial_oracle/infantry_entry_raw.py', 'tools/spatial_oracle/jumpjet_coordinates.py', 'tools/spatial_oracle/jumpjet_entry_discovery.py', 'tools/spatial_oracle/map_queries.py', 'tools/spatial_oracle/refinery_dock.py', 'tools/spatial_oracle/slave_manager.py', 'tools/spatial_oracle/time_to_build.py', 'tools/spatial_oracle/track_destination.py', 'tools/spatial_oracle/unit_entry.py', 'tools/spatial_oracle/unit_scatter_state.py', 'tools/spatial_oracle/unit_source_scatter.py', 'tools/spatial_oracle/walk_head_occupation.py', 'tools/spatial_oracle/house_power_consumers.py')


if __name__ == '__main__':
    main()
