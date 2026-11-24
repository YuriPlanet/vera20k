"""Original Engineer bridge-hut action branches and object-click composition.

51E49E/51F9F4 are supplied interior frames, not full WhatAction entries.
587410 Boolean and Building GetCoords are explicit seams. Original actor
locality, Building RTTI/undeploy, action arithmetic, Infantry click mapping,
Foot switch and IsControllable execute. Stops before QueueMegaMission body.
Also executes Display4AAE90's normal/minimap action switches from supplied
interior dispatch frames to the cursor virtual+48 boundary.
Additive prerequisites execute full Building/Infantry type constructors,
selected bool field reads, and original Building ART Foundation461225..46125D
with original32-byte reader/table and subsequent one-cell Undeploy gate.
"""
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDI,
                              UC_X86_REG_ESI, UC_X86_REG_EBP, UC_X86_REG_ESP,
                              UC_X86_REG_EIP, UC_X86_REG_EBX)
from tools.native_oracle import (load_image, run_checked, STACK_BASE, STACK_SIZE,
                                SCRATCH, RET_MAGIC, finish_vectors, provenance)
from tools.spatial_oracle.map_queries import dwords
from tools.rules_oracle.bridge_anim_inputs import Reader
from tools.spatial_oracle.building_body_rules import INI as READER_INI, SP as READER_SP

ACTOR, HUT, ACTOR_KIND, HUT_KIND, HOUSE, TARGET_HOUSE = [SCRATCH+n*0x2000 for n in range(1, 7)]
SP = STACK_BASE + STACK_SIZE - 0x1000
INF_VT, BUILDING_VT = 0x7EB058, 0x7E3EBC
QUERY, COORDS, QUEUE = 0x587410, 0x447AC0, 0x6FFBE0
TEXT_BEGIN, TEXT_SIZE = 0x401000, 0x3E0000


class Fixture:
    def __init__(self, case):
        self.case = case
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.u)
        self.u.mem_map(SCRATCH, 0x20000)
        self.u.mem_map(STACK_BASE, STACK_SIZE)
        self.u.mem_map(RET_MAGIC, 0x1000)
        self.trace = []
        self.u.mem_write(ACTOR, dwords(INF_VT))
        self.u.mem_write(ACTOR+0x6C0, dwords(ACTOR_KIND))
        self.u.mem_write(ACTOR+0x21C, dwords(HOUSE))
        self.u.mem_write(ACTOR_KIND+0xEC3, bytes([case['engineer']]))
        self.u.mem_write(ACTOR+0x6A0, dwords(-1))
        self.u.mem_write(ACTOR+0x6A8, dwords(0))
        self.u.mem_write(HUT, dwords(BUILDING_VT))
        self.u.mem_write(HUT+0x520, dwords(HUT_KIND))
        self.u.mem_write(HUT+0x21C, dwords(HOUSE if case['relation']=='self' else TARGET_HOUSE))
        self.u.mem_write(HUT_KIND+0xCCC, bytes([case['repairable']]))
        self.u.mem_write(HUT_KIND+0x16B6, b'\x01')
        self.u.mem_write(HUT_KIND+0x408,dwords(ACTOR_KIND if case.get('undeploy_pointer',False) else 0))
        self.u.mem_write(HUT_KIND+0xEF0,dwords(case.get('foundation_index',0)))
        self.u.mem_write(HOUSE+0x1EC, bytes([case['campaign_human']]))
        self.u.mem_write(HOUSE+0x1ED, bytes([case['campaign_player_control']]))
        # The target owner's alliance bits differ while the hut arm never reads them.
        self.u.mem_write(HOUSE+0x30, dwords(0))
        self.u.mem_write(HOUSE+0x5788, dwords(2 if case['relation']=='allied' else 0))
        self.u.mem_write(TARGET_HOUSE+0x30, dwords(1))
        self.u.mem_write(0xA8B238, dwords(case['mode']))
        self.u.mem_write(0xA83D4C, dwords(HOUSE if case['local'] else TARGET_HOUSE))
        self.apply_gates(case.get('gates',{}))
        assert self.word(INF_VT+0x74)==0x51E3B0
        assert self.word(INF_VT+0xA0)==0x700C40
        assert self.word(INF_VT+0x378)==QUEUE
        assert self.word(BUILDING_VT+0x48)==COORDS
        assert self.word(BUILDING_VT+0x80)==0x457620
        self.original = self.text_hash()

    def apply_gates(self, gates):
        """Supplied native fields; their initialization/producer bodies are excluded."""
        for offset,key in ((0x504,'emp'),(0x6A0,'paralysis_start'),(0x6A8,'paralysis_duration')):
            if key in gates:
                self.u.mem_write(ACTOR+offset,dwords(gates[key]))
        self.u.mem_write(0xA8ED84,dwords(gates.get('frame',0)))
        for offset,key in ((0x270,'warp_out'),(0x271,'warp_in'),(0x1C8,'robot_offline')):
            self.u.mem_write(ACTOR+offset,bytes([gates.get(key,False)]))
        self.u.mem_write(ACTOR_KIND+0xD54,bytes([gates.get('spawned',False)]))
        for offset,key in ((0x2E4,'bunker'),(0x2DC,'slave_owner')):
            self.u.mem_write(ACTOR+offset,dwords(HUT if gates.get(key,False) else 0))
        if 'spawn_slots' in gates:
            manager,vector=SCRATCH+0xE000,SCRATCH+0xF000
            self.u.mem_write(ACTOR+0x2D0,dwords(manager))
            self.u.mem_write(ACTOR_KIND+0xD5C,dwords(gates['spawns_number']))
            self.u.mem_write(manager+0x3C,dwords(vector))
            self.u.mem_write(manager+0x48,dwords(len(gates['spawn_slots'])))
            assert len(gates['spawn_slots'])<=4
            for index,slot in enumerate(gates['spawn_slots']):
                item=SCRATCH+0x10000+index*0x100
                child=SCRATCH+0x11000+index*0x1000
                kind=SCRATCH+0x18000+index*0x1000
                self.u.mem_write(vector+index*4,dwords(item))
                self.u.mem_write(item,dwords(child if slot.get('child',True) else 0,slot['state']))
                self.u.mem_write(child,dwords(INF_VT))
                self.u.mem_write(child+0x6C0,dwords(kind))
                self.u.mem_write(child+0x81,bytes([slot.get('limbo',False)]))
                self.u.mem_write(kind+0xD68,bytes([slot.get('missile_spawn',False)]))

    def word(self, address):
        return struct.unpack('<I', self.u.mem_read(address,4))[0]

    def text_hash(self):
        return hashlib.sha256(bytes(self.u.mem_read(TEXT_BEGIN,TEXT_SIZE))).hexdigest()

    def answer(self, value, cleaned):
        sp=self.u.reg_read(UC_X86_REG_ESP)
        self.u.reg_write(UC_X86_REG_EAX,value & 0xFFFFFFFF)
        self.u.reg_write(UC_X86_REG_EIP,self.word(sp))
        self.u.reg_write(UC_X86_REG_ESP,sp+4+cleaned)

    def hook(self, _u, address, _size, _data):
        sp=self.u.reg_read(UC_X86_REG_ESP)
        if address==QUERY:
            coord=list(struct.unpack('<hh',self.u.mem_read(self.word(sp+4),4)))
            self.trace.append(dict(kind='query_seam',coord=coord,al=self.case['predicate']))
            self.answer(int(self.case['predicate']),4)
        elif address==COORDS:
            assert self.u.reg_read(UC_X86_REG_ECX)==HUT
            dest=self.word(sp+4)
            self.u.mem_write(dest,dwords(*self.case['coords']))
            self.trace.append(dict(kind='getcoords_seam',coords=self.case['coords']))
            self.answer(dest,4)
        elif address==0x50B6F0:
            self.trace.append(dict(kind='original_actor_locality',house='actor'))
            assert self.u.reg_read(UC_X86_REG_ECX)==HOUSE
        elif address in (0x65C780,0x65C7E0,0x4F9A90):
            raise AssertionError(f'Unsupported receiver {address:08X}')


def action(case, route):
    m=Fixture(case);u=m.u
    # Establish the relation labels with the original receiver in a separate
    # preflight. The measured hut branch must never reach that receiver.
    u.mem_write(SP,dwords(RET_MAGIC,HUT))
    u.reg_write(UC_X86_REG_ESP,SP)
    u.reg_write(UC_X86_REG_ECX,HOUSE)
    run_checked(u,0x4F9A90,RET_MAGIC,count=1000,required_addresses=[0x4F9A90])
    friendly=bool(u.reg_read(UC_X86_REG_EAX)&255)
    assert friendly==(case['relation']!='hostile')
    m.u.hook_add(UC_HOOK_CODE,m.hook)
    u.reg_write(UC_X86_REG_ESP,SP)
    u.reg_write(UC_X86_REG_EDI,ACTOR)
    u.reg_write(UC_X86_REG_ESI,HUT)
    if route=='object':
        begin,fallback,return_offset=0x51E49E,0x51E668,0x38
    else:
        begin,fallback,return_offset=0x51F9F4,0x51FA93,0x1C
        u.reg_write(UC_X86_REG_EBP,HUT_KIND)
    u.mem_write(SP+return_offset,dwords(RET_MAGIC))
    before=bytes(u.mem_read(SCRATCH,0x18000))
    end=run_checked(u,begin,(fallback,RET_MAGIC),count=4000,required_addresses=[begin])
    assert m.text_hash()==m.original
    assert bytes(u.mem_read(SCRATCH,0x18000))==before
    output=dict(boundary=f'{end:08X}',trace=m.trace,text_sha256=m.original,native_relation_is_ally=friendly,
                query_calls=sum(e['kind']=='query_seam' for e in m.trace))
    if end==RET_MAGIC:
        output['action']=u.reg_read(UC_X86_REG_EAX)
        output['stack_delta']=u.reg_read(UC_X86_REG_ESP)-SP
    else:
        output['outcome']='hut_branch_not_admitted; subsequent WhatAction excluded'
    if 'foundation_index' in case:
        index=case['foundation_index']
        output['original_foundation']=dict(index=index,width=m.word(0x8192B8+index*4),
            height=m.word(0x819310+index*4),undeploy_pointer=case['undeploy_pointer'],
            width_hex=bytes(u.mem_read(0x8192B8+index*4,4)).hex(),
            height_hex=bytes(u.mem_read(0x819310+index*4,4)).hex())
    return dict(input=dict(**case,route=route),output=output)


def clicked_object(case, native_action, blocked=False, gate_control=False):
    m=Fixture(case);u=m.u
    if not gate_control:
        u.mem_write(ACTOR+0x504,dwords(1 if blocked else 0))
    def observe(_u,address,size,data):
        if address==0x51E3B0:
            m.trace.append(dict(kind='what_action_seam',action=native_action))
            m.answer(native_action,8)
        elif address in (0x51F1A4,0x51F1C7,0x4D74E0):
            m.trace.append(dict(kind='action_mapping',pc=f'{address:08X}',eax=u.reg_read(UC_X86_REG_EAX)))
        elif gate_control and address in (0x700C40,0x70EFD0,0x4DE770,0x70C5F0,0x6B7D80):
            m.trace.append(dict(kind='original_controllability_receiver',pc=f'{address:08X}'))
        elif address==QUEUE:
            sp=u.reg_read(UC_X86_REG_ESP)
            args=[m.word(sp+4+i*4) for i in range(4)]
            assert args[1:]==[0,HUT,0]
            m.trace.append(dict(kind='queue_boundary',mission=args[0],target=None,destination='hut',extra=0))
        else:
            m.hook(_u,address,size,data)
    u.hook_add(UC_HOOK_CODE,observe)
    u.mem_write(SP,dwords(RET_MAGIC,0,HUT,0))
    u.reg_write(UC_X86_REG_ESP,SP)
    u.reg_write(UC_X86_REG_ECX,ACTOR)
    before=bytes(u.mem_read(SCRATCH,0x20000))
    end=run_checked(u,0x51F190,(QUEUE,RET_MAGIC),count=5000,
                    required_addresses=[0x51F190,0x51E3B0,0x4D74E0])
    # run_checked stops before dispatching observational hooks at a terminal.
    if end==QUEUE:
        observe(u,QUEUE,0,None)
    assert m.text_hash()==m.original
    assert bytes(u.mem_read(SCRATCH,0x20000))==before
    result=dict(input=dict(name=case['name'],native_action=native_action,emp_blocked=blocked),
                output=dict(boundary=f'{end:08X}',trace=m.trace,
                            queue_calls=sum(e['kind']=='queue_boundary' for e in m.trace),
                            returned_al=(u.reg_read(UC_X86_REG_EAX)&255) if end==RET_MAGIC else None,
                            text_sha256=m.original))
    if gate_control:
        result['input'].pop('emp_blocked')
        result['input']['gates']=case['gates']
    return result


def controllability(case):
    m=Fixture(case);u=m.u
    launched=None
    if 'spawn_slots' in case['gates']:
        before_count=bytes(u.mem_read(SCRATCH,0x20000))
        u.mem_write(SP,dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP,SP)
        u.reg_write(UC_X86_REG_ECX,m.word(ACTOR+0x2D0))
        run_checked(u,0x6B7D80,RET_MAGIC,count=5000,required_addresses=[0x6B7D80])
        launched=u.reg_read(UC_X86_REG_EAX)
        assert bytes(u.mem_read(SCRATCH,0x20000))==before_count
    def observe(_u,address,size,data):
        if address in (0x700C40,0x70EFD0,0x4DE770,0x70C5F0,0x6B7D80):
            m.trace.append(dict(kind='original_controllability_receiver',pc=f'{address:08X}'))
        else:
            m.hook(_u,address,size,data)
    u.hook_add(UC_HOOK_CODE,observe)
    u.mem_write(SP,dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP,SP)
    u.reg_write(UC_X86_REG_ECX,ACTOR)
    before=bytes(u.mem_read(SCRATCH,0x20000))
    run_checked(u,0x700C40,RET_MAGIC,count=5000,required_addresses=[0x700C40])
    assert m.text_hash()==m.original
    assert bytes(u.mem_read(SCRATCH,0x20000))==before
    result=dict(input=dict(name=case['name'],gates=case['gates']),
                output=dict(returned_al=u.reg_read(UC_X86_REG_EAX)&255,
                            trace=m.trace,text_sha256=m.original))
    if launched is not None:
        result['output']['original_launched_missiles']=launched
    return result


def gate_inputs():
    base=next(inputs())
    controls=[('all_clear',{}),('bunker',dict(bunker=True)),('spawned',dict(spawned=True)),
        ('paralysis_pending',dict(paralysis_start=10,paralysis_duration=20,frame=15)),
        ('paralysis_expired',dict(paralysis_start=10,paralysis_duration=20,frame=30)),
        ('paralysis_stopped_nonzero',dict(paralysis_start=-1,paralysis_duration=-3,frame=0)),
        ('paralysis_future_start',dict(paralysis_start=20,paralysis_duration=10,frame=15)),
        ('warp_out',dict(warp_out=True)),('warp_in',dict(warp_in=True)),
        ('slave_owner',dict(slave_owner=True)),('robot_native_save_control',dict(robot_offline=True)),
        ('emp_native_save_positive',dict(emp=1)),('emp_negative',dict(emp=-1))]
    for count,number in ((0,2),(1,2),(1,1),(2,2),(2,3),(1,-1)):
        controls.append((f'missiles_{count}_of_{number}',dict(spawns_number=number,
            spawn_slots=[dict(state=1) for _ in range(count)])))
    for name,slot in (
        ('ready',dict(state=0)),('child_missile',dict(state=2,missile_spawn=True)),
        ('child_limbo',dict(state=2,missile_spawn=True,limbo=True)),
        ('child_not_missile',dict(state=2)),('child_null',dict(state=2,child=False))):
        controls.append((f'spawn_{name}',dict(spawns_number=2,spawn_slots=[slot])))
    for name,gates in controls:
        yield dict(base,name=name,predicate=True,gates=gates)


def undeploy_inputs():
    base=next(inputs())
    for pointer,index in ((False,0),(True,0),(True,1),(True,2),(True,3)):
        yield dict(base,name=f'undeploy_{pointer}_foundation_{index}',predicate=True,
                   undeploy_pointer=pointer,foundation_index=index)


def inputs():
    base=dict(engineer=True,repairable=True,predicate=True,mode=1,local=True,
              campaign_human=False,campaign_player_control=False,relation='hostile',coords=[2688,2688,1040])
    for relation in ('self','allied','hostile'):
        for predicate in (False,True):
            yield dict(base,name=f'{relation}_{predicate}',relation=relation,predicate=predicate)
    for field,value in [('engineer',False),('repairable',False),('local',False)]:
        yield dict(base,name=f'{field}_false',**{field:value})
    for human,control in ((False,False),(True,False),(False,True),(True,True)):
        yield dict(base,name=f'campaign_{human}_{control}',mode=0,local=False,
                   campaign_human=human,campaign_player_control=control)
    for coords in ([255,256,0],[-255,-256,0],[-257,-511,-104],[-8388608,8388607,0]):
        yield dict(base,name='coords_'+'_'.join(map(str,coords)),coords=coords)


def display_cursor(action_value, minimap):
    """Original action switch; supplied receiver vtable routes +48 to a sink."""
    u=Uc(UC_ARCH_X86,UC_MODE_32)
    load_image(u)
    u.mem_map(SCRATCH,0x10000)
    u.mem_map(STACK_BASE,STACK_SIZE)
    u.mem_map(RET_MAGIC,0x1000)
    receiver,vt,sink=SCRATCH+0x1000,SCRATCH+0x2000,RET_MAGIC+0x100
    u.mem_write(receiver,dwords(vt))
    u.mem_write(vt+0x48,dwords(sink))
    # Both dispatch frames have SUB ESP10 then PUSH EBX/EBP/ESI/EDI
    # already completed. Original argument at old ESP+14 is current ESP+34.
    cursor_argument=int(minimap)
    u.mem_write(SP+0x34,dwords(cursor_argument))
    u.reg_write(UC_X86_REG_ESP,SP)
    u.reg_write(UC_X86_REG_ESI,receiver)
    u.reg_write(UC_X86_REG_EAX,action_value)
    begin=0x4AB366 if minimap else 0x4AB449
    code=bytes(u.mem_read(TEXT_BEGIN,TEXT_SIZE))
    run_checked(u,begin,sink,count=1000,required_addresses=[begin])
    sp=u.reg_read(UC_X86_REG_ESP)
    assert u.reg_read(UC_X86_REG_ECX)==receiver
    assert sp==SP-12
    row,arg=struct.unpack('<II',u.mem_read(sp+4,8))
    assert arg==cursor_argument
    assert bytes(u.mem_read(TEXT_BEGIN,TEXT_SIZE))==code
    descriptor_address=0x82D028+row*28
    raw=bytes(u.mem_read(descriptor_address,28))
    fields=list(struct.unpack('<7i',raw))
    return dict(input=dict(action=action_value,minimap=minimap,cursor_argument=cursor_argument),
                output=dict(entry=f'{begin:08X}',virtual_slot=0x48,cursor_row=row,
                            cursor_argument=arg,return_pc=f'{struct.unpack("<I",u.mem_read(sp,4))[0]:08X}',
                            descriptor_address=f'{descriptor_address:08X}',descriptor_hex=raw.hex(),
                            descriptor_signed_dwords=fields,frame_start=fields[0],
                            frame_count=fields[1],frame_rate=fields[2],
                            text_sha256=hashlib.sha256(code).hexdigest()))


class HutTypeReader(Reader):
    """Reuse the original-reader fixture; no physical asset/file seam is admitted."""
    def __init__(self):
        self.phase='setup'
        self.trace=[]
        self.types={}
        # Reader's asset seam is never reached: this nonexistent path supplies
        # no assets and creates no directory. INI caches use its existing owner.
        super().__init__(Path(__file__).parent/'__no_file_io__',{})
        self.original=hashlib.sha256(bytes(self.u.mem_read(TEXT_BEGIN,TEXT_SIZE))).hexdigest()
        self.writes=[]
        self.u.hook_add(UC_HOOK_MEM_WRITE,self.observe_write)

    def hook(self,u,pc,size,data):
        if pc==0x5B40B0:
            raise AssertionError('Physical asset loading is outside the prerequisite boundary')
        if pc in (0x7C8E17,0x7C8B3D,0x7D140B):
            self.trace.append(dict(phase=self.phase,kind='fixture_seam',pc=f'{pc:08X}'))
        elif pc==0x474DA0:
            sp=u.reg_read(UC_X86_REG_ESP)
            self.trace.append(dict(phase=self.phase,kind='foundation_reader',pc=f'{pc:08X}',
                ini=f'{u.reg_read(UC_X86_REG_ECX):08X}',section=self.string(self.read32(sp+4)),
                key=self.string(self.read32(sp+8)),default_index=self.read32(sp+12)))
        elif pc==0x474DC7:
            self.trace.append(dict(phase=self.phase,kind='foundation_string',
                value=self.string(u.reg_read(UC_X86_REG_ESP)+8),capacity=32))
        elif pc==0x5295F0:
            sp=u.reg_read(UC_X86_REG_ESP)
            self.trace.append(dict(phase=self.phase,kind='bool_reader',pc=f'{pc:08X}',
                section=self.string(self.read32(sp+4)),key=self.string(self.read32(sp+8)),
                default=self.read32(sp+12)&255))
        super().hook(u,pc,size,data)

    def observe_write(self,u,access,address,size,value,data):
        if self.phase=='fixture':
            return
        for name,typ in self.types.items():
            fields=[('repairable',0xCCC),('undeploy',0x408)]
            fields+=([('engineer',0xEC3)] if name=='ENGINEER' else
                     [('bridge_repair_hut',0x16B6),('foundation',0xEF0)])
            for field,offset in fields:
                if address<=typ+offset<address+size:
                    self.writes.append(dict(phase=self.phase,type=name,field=field,
                        pc=f'{u.reg_read(UC_X86_REG_EIP):08X}',size=size,value=value))

    def construct(self,name,infantry=False):
        typ=self.alloc(0x1900)
        self.types[name]=typ
        self.phase='constructor_'+name
        self.invoke(0x5236A0 if infantry else 0x45DD90,typ,(self.cstring(name),))
        return typ

    def unchanged(self):
        assert hashlib.sha256(bytes(self.u.mem_read(TEXT_BEGIN,TEXT_SIZE))).hexdigest()==self.original

    def field_block(self,typ,begin,end,registers):
        self.u.mem_write(READER_SP,dwords(RET_MAGIC))
        self.u.reg_write(UC_X86_REG_ESP,READER_SP)
        for register,value in registers.items():
            self.u.reg_write(register,value)
        mark=len(self.trace)
        run_checked(self.u,begin,end,count=200000,required_addresses=(0x5295F0,))
        assert self.u.reg_read(UC_X86_REG_ESP)==READER_SP
        assert not any(row['kind']=='fixture_seam' for row in self.trace[mark:])
        self.unchanged()


def foundation_prerequisites():
    probe=HutTypeReader()
    table=[dict(index=i,name=probe.string(probe.read32(0x81B9D8+i*8)),
                stored_index=probe.read32(0x81B9DC+i*8),
                width=probe.read32(0x8192B8+i*4),height=probe.read32(0x819310+i*4))
           for i in range(22)]
    assert [row['stored_index'] for row in table]==list(range(22))
    cases=[dict(name='table_'+str(row['index']),initial_index=0,
                sections={'HUT_IMAGE':{'Foundation':row['name']}}) for row in table]
    cases.extend([
        dict(name='absent_retains_nonzero',initial_index=3,sections={}),
        dict(name='unknown_image_resets_default',initial_index=3,sections={'HUT_IMAGE':{'Foundation':'unknown'}}),
        dict(name='wrong_case_key',initial_index=3,sections={'HUT_IMAGE':{'foundation':'1x1'}}),
        dict(name='wrong_case_section',initial_index=3,sections={'hut_image':{'Foundation':'1x1'}}),
        dict(name='case_insensitive_name',initial_index=0,sections={'HUT_IMAGE':{'Foundation':'2X2'}}),
        dict(name='type_id_nonzero_override',initial_index=0,sections={'HUT_IMAGE':{'Foundation':'1x1'},'CABHUT':{'Foundation':'2x2'}}),
        dict(name='type_id_zero_cannot_override_image',initial_index=0,sections={'HUT_IMAGE':{'Foundation':'2x2'},'CABHUT':{'Foundation':'1x1'}}),
        dict(name='type_id_unknown_cannot_override_image',initial_index=0,sections={'HUT_IMAGE':{'Foundation':'2x2'},'CABHUT':{'Foundation':'unknown'}}),
        dict(name='type_id_absent_retains_image',initial_index=0,sections={'HUT_IMAGE':{'Foundation':'2x2'}}),
        dict(name='image_absent_type_id_nonzero',initial_index=0,sections={'CABHUT':{'Foundation':'2x2'}}),
        dict(name='image_unknown_type_id_nonzero',initial_index=3,sections={'HUT_IMAGE':{'Foundation':'unknown'},'CABHUT':{'Foundation':'3x3'}}),
        dict(name='both_distinct_nonzero_type_wins',initial_index=0,sections={'HUT_IMAGE':{'Foundation':'3x3'},'CABHUT':{'Foundation':'2x2'}}),
        dict(name='type_id_wrong_case_key',initial_index=0,sections={'HUT_IMAGE':{'Foundation':'2x2'},'CABHUT':{'foundation':'3x3'}}),
        dict(name='same_image_and_type_section',initial_index=0,image='CABHUT',sections={'CABHUT':{'Foundation':'2x2'}}),
        dict(name='trim_and_31_byte_content_cap',initial_index=0,sections={'HUT_IMAGE':{'Foundation':' '*28+'2x2'+'3x3'}}),
        dict(name='cap_truncates_name_to_unknown',initial_index=3,sections={'HUT_IMAGE':{'Foundation':' '*29+'2x2'}}),
        dict(name='rules_foundation_is_not_art_input',initial_index=0,
             rules_foundation_poison='3x3',sections={'HUT_IMAGE':{'Foundation':'1x1'}}),
        dict(name='latin1_nbsp_is_not_ascii_trim',initial_index=3,
             sections={'HUT_IMAGE':{'Foundation':'\u00a02x2\u00a0'}}),
    ])
    rows=[]
    for case in cases:
        m=HutTypeReader();typ=m.construct('CABHUT')
        image=case.get('image','HUT_IMAGE')
        m.u.mem_write(typ+0x1F8,image.encode('ascii')+b'\0')
        m.u.mem_write(typ+0xEF0,dwords(case['initial_index']))
        m.make_ini({'CABHUT':{'Foundation':case.get('rules_foundation_poison','2x2')}})
        rules_ini=m.alloc(0x40)
        m.u.mem_write(rules_ini,bytes(m.u.mem_read(READER_INI,0x40)))
        m.make_ini(case['sections']);m.phase='foundation_read';m.trace=[];m.writes=[]
        m.u.mem_write(READER_SP,dwords(RET_MAGIC));m.u.reg_write(UC_X86_REG_ESP,READER_SP)
        m.u.reg_write(UC_X86_REG_EBP,typ);m.u.reg_write(UC_X86_REG_EDI,typ+0x1F8)
        m.u.reg_write(UC_X86_REG_EBX,typ+0x24)
        m.u.reg_write(UC_X86_REG_ESI,rules_ini)
        run_checked(m.u,0x461225,0x46125D,count=200000,required_addresses=(0x474DA0,0x528A10))
        assert m.u.reg_read(UC_X86_REG_ESP)==READER_SP
        assert len([row for row in m.trace if row['kind']=='foundation_reader'])==2
        assert not any(row['kind']=='fixture_seam' for row in m.trace)
        index=m.read32(typ+0xEF0)
        width,height=m.read32(0x8192B8+index*4),m.read32(0x819310+index*4)
        read_trace=list(m.trace);field_writes=list(m.writes)
        for pointer in (False,True):
            m.phase='fixture'
            m.u.mem_write(typ+0x408,dwords(typ if pointer else 0))
            m.phase='one_cell_undeploy_gate'
            gate=m.invoke(0x465D40,typ)
            supplied=dict(name=case['name'],image=image,type_id='CABHUT',art_entries=case['sections'],
                          initial_foundation_id=case['initial_index'],undeploy_pointer=pointer)
            if 'rules_foundation_poison' in case:
                supplied['rules_foundation_poison']=case['rules_foundation_poison']
            rows.append(dict(input=supplied,output=dict(foundation_id=index,
                foundation_name=table[index]['name'],width=width,height=height,
                is_1x1_with_undeploy=bool(gate),returned_eax=gate,
                trace=read_trace,field_writes=field_writes,text_sha256=m.original)))
        m.unchanged()
    raw_tables=[dict(address=f'{address:08X}',bytes=size,
        hex=bytes(probe.u.mem_read(address,size)).hex())
        for address,size in ((0x81B9D8,22*8),(0x8192B8,22*4),(0x819310,22*4))]
    for row in table:
        pointer=probe.read32(0x81B9D8+row['index']*8)
        row['name_address']=f'{pointer:08X}'
        row['name_bytes_hex']=bytes(probe.u.mem_read(pointer,len(row['name'])+1)).hex()
    return dict(table=table,raw_tables=raw_tables,rows=rows)


def type_default_prerequisites():
    m=HutTypeReader();hut=m.construct('CABHUT');inf=m.construct('ENGINEER',infantry=True)
    constructors=dict(building=dict(repairable=m.u.mem_read(hut+0xCCC,1)[0],
        bridge_repair_hut=m.u.mem_read(hut+0x16B6,1)[0],foundation_index=m.read32(hut+0xEF0),
        undeploy_pointer=m.read32(hut+0x408)),infantry=dict(repairable=m.u.mem_read(inf+0xCCC,1)[0],
        engineer=m.u.mem_read(inf+0xEC3,1)[0],undeploy_pointer=m.read32(inf+0x408)),
        trace=list(m.trace),field_writes=list(m.writes))
    layers=[('explicit',{'CABHUT':{'Repairable':'no','BridgeRepairHut':'yes'},'ENGINEER':{'Engineer':'yes'}}),
            ('missing_keys',{'CABHUT':{'Other':'yes'},'ENGINEER':{'Other':'yes'}}),
            ('invalid',{'CABHUT':{'Repairable':'x','BridgeRepairHut':'x'},'ENGINEER':{'Engineer':'x'}}),
            ('wrong_case_keys',{'CABHUT':{'repairable':'yes','bridgerepairhut':'no'},'ENGINEER':{'engineer':'no'}}),
            ('wrong_case_sections',{'cabhut':{'Repairable':'yes','BridgeRepairHut':'no'},'engineer':{'Engineer':'no'}}),
            ('reverse',{'CABHUT':{'Repairable':'yes','BridgeRepairHut':'no'},'ENGINEER':{'Engineer':'no'}})]
    rows=[]
    for label,sections in layers:
        m.make_ini(sections);m.phase=label;m.trace=[];m.writes=[]
        m.field_block(hut,0x714A7D,0x714A97,{UC_X86_REG_EBP:hut,UC_X86_REG_EDI:READER_INI,UC_X86_REG_EBX:hut+0x24})
        m.field_block(hut,0x460E86,0x460EA0,{UC_X86_REG_EBP:hut,UC_X86_REG_ESI:READER_INI,UC_X86_REG_EBX:hut+0x24})
        m.field_block(inf,0x52456A,0x52458A,{UC_X86_REG_ESI:inf,UC_X86_REG_EBP:READER_INI,UC_X86_REG_EDI:inf+0x24})
        rows.append(dict(name=label,supplied_sections=sections,repairable=m.u.mem_read(hut+0xCCC,1)[0],
            bridge_repair_hut=m.u.mem_read(hut+0x16B6,1)[0],engineer=m.u.mem_read(inf+0xEC3,1)[0],
            trace=list(m.trace),field_writes=list(m.writes)))
    m.unchanged()
    return dict(constructors=constructors,bool_layer_history=rows,text_sha256=m.original)


def generate():
    actions=[action(case,route) for case in inputs() for route in ('object','cell')]
    clicks=[clicked_object(row['input'],row['output']['action'],blocked)
            for row in actions if row['input']['route']=='object' and 'action' in row['output']
            for blocked in (False,True)]
    cursors=[display_cursor(value,minimap) for value in (29,32) for minimap in (False,True)]
    gates=[dict(native=controllability(case),object_click=clicked_object(case,29,gate_control=True))
           for case in gate_inputs()]
    undeploy=[action(case,route) for case in undeploy_inputs() for route in ('object','cell')]
    foundation=foundation_prerequisites()
    return dict(schema=1,actions=actions,object_clicks=clicks,display_cursors=cursors,
                controllability_controls=gates,undeploy_controls=undeploy,
                foundation_reads=foundation['rows'],foundation_table=foundation['table'],
                foundation_table_bytes=foundation['raw_tables'],
                type_default_prerequisites=type_default_prerequisites())


def metadata():
    return provenance(scope=__doc__,entry_points={'object_interior':0x51E49E,'cell_interior':0x51F9F4,
        'locality':0x50B6F0,'query_seam':QUERY,'getcoords_seam':COORDS,'object_clicked':0x51F190,
        'foot_clicked':0x4D74E0,'is_controllable':0x700C40,'queue_boundary':QUEUE,
        'one_cell_undeploy':0x465D40,'foundation_widths':0x8192B8,'foundation_heights':0x819310,
        'building_type_constructor':0x45DD90,'infantry_type_constructor':0x5236A0,
        'techno_type_constructor':0x710AF0,'building_foundation_begin':0x461225,
        'building_foundation_stop':0x46125D,'read_foundation':0x474DA0,
        'foundation_name_table':0x81B9D8,'read_bool':0x5295F0,'read_string':0x528A10,
        'display_normal_dispatch':0x4AB449,'display_minimap_dispatch':0x4AB366,
        'cursor_descriptor_table':0x82D028},
        assumptions=['Supplied already-established interior WhatAction frames; original Infantry/Building vtables, type/house fields and linked target. Upstream base WhatAction, modifiers, target discovery, object construction and native mode loading excluded.',
                     'Target relation changes pointer/actor alliance mask+5788; original4F9A90 preflight independently establishes self/allied/hostile labels. The measured hut arm is stopped before its later alliance receiver. Campaign and noncampaign actor locality execute original50B6F0.',
                     'Original IsControllable700C40 executes. The prior click rows use supplied zero flags/links, timer start=-1/duration=0 and EMP0/1. Additive controls supply bunker, Spawned, paralysis timer, both warp bytes, SlaveOwner, native-save Robot/EMP bytes and SpawnManager slots; original6B7D80 counts slots using original child vtables. Constructors, timer/link producers, native save load and repair suffix are excluded.',
                     'Positive Infantry EMP/Robot controls are supplied native-save states, not established ordinary retail producers: Techno6F3112/6F2C9A initialize zero; EMPulseApply4C575E accepts RTTI1/2, not Infantry RTTI15(523340). TechnoLoad70BF50 reaches rawIStreamread410380 and InfantrySize5232F0=6F0; that can retain either field. Robot70FD62 House callers walk ALL Technos but require type-pointer equality to PowersUnit+40C; original713300 resolves that pointer through7480D0 UnitType allocator (7471BF vtable7F6218, RTTI UnitTypeClass at845988). The other active caller is UnitPerCell739F59. Generic50E1C0 has no incoming static references; that alone does not establish unreachability. Initialization and load observations are instruction evidence, not executed constructor/load comparisons.',
                     'Undeploy controls supply BuildingType+408 pointer and+EF0 foundation index0..3. Original465D40 reads original width8192B8/height819310 tables; only nonnull pointer with1x1 shortcircuits the object hut branch. Cell branch has no corresponding gate.',
                     'Additive foundation_reads execute original461225..46125D with supplied interior BuildingType frame, original474DA0/528A10/_stricmp and original22-name/index/width/height tables. Initial+EF0 is explicitly supplied; original first read uses current index as default, second uses the first result as default, first is stored unconditionally and second only if nonzero. Missing sections/keys retain defaults; malformed present names resolve0. Both reads use fixedARTINI887180, first Image+1F8 and then typeID+24. A separate supplied rulesINI with a Foundation value is present in ESI and is not consulted. Effective Image bytes, full Object/Techno/Building reader prefixes/suffixes, physical file loader, ART/source selection and persistence are excluded. Both null/non-null Undeploy pointer fixtures exercise original465D40 after the field read; no pointer factory/lifecycle claim.',
                     'Additive type_default_prerequisites execute full original Building45DD90 and Infantry5236A0 constructors with Object/Techno bases through the existing Reader fixture. Six sequential selected field blocks execute unchanged ReadBool5295F0/current-default stores for BuildingRepairable+CCC, BuildingBridgeRepairHut+16B6 and InfantryEngineer+EC3. Their supplied exact-case INI caches are synthetic; no physical retail-layer or full reader/Rules sweep comparison is claimed. InfantryRepairable=false is captured as constructor evidence only; this target-hut chain consumes BuildingRepairable.',
                     'Display interior switches receive EAX action29/32, ESI supplied receiver and stack+34 argument0/1. Original normal/minimap switch tables execute; selected 28-byte cursor descriptors come unchanged from retail image82D028.'],
        substitutions=['587410 returns an explicitly supplied Boolean. This is caller/action evidence; bridge geometry/query parity belongs to the separate complete587410 oracle.',
                       'Building GetCoords447AC0 returns suppliedXYZ; original signed truncation/packed conversion executes. Foundation coordinate producer is excluded.',
                       'Object ClickedAction51F190 receives its WhatAction call result from the prior native branch fixture as an explicit cross-fixture seam. No code bytes patched.',
                       'QueueMegaMission6FFBE0 is an observed terminal boundary; mission/body/network enqueue, Walk, Engineer consumption, repair effects and audio/render are not executed.',
                       'Rejected branch cases stop before original fallback; no claim about the fallback final action. All text hashes and unchanged fixture bytes checked.',
                       'Display receiver vtable+48 points to an observed terminal sink. Cursor setter/clock/SHP drawing and upstream Display action production do not execute; only original switch selection and immutable selected descriptor bytes are established.',
                       'Additive type prerequisites reuse Reader/Lists bounded operator_new7C8E17, no-op operator_delete7C8B3D and supplied CRT TLS7D140B setup seams; no other function is substituted. Original constructors may reach the allocator, recorded in constructor trace. Measured Foundation/bool reader phases assert no seam is reached and original full text hash is unchanged. Physical asset loading5B40B0 is rejected.'])


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata,
                   source_paths={'caller':Path(__file__),
                                 'native_oracle':Path(__file__).parents[1]/'native_oracle.py',
                                 'map_queries':Path(__file__).with_name('map_queries.py'),
                                 'reader_fixture':Path(__file__).parents[1]/'rules_oracle'/'bridge_anim_inputs.py',
                                 'reader_base':Path(__file__).parents[1]/'rules_oracle'/'bridge_anim_lists.py',
                                 'reader_memory':Path(__file__).with_name('building_body_rules.py'),
                                 'reader_crc':Path(__file__).parents[1]/'projectile_oracle'/'flat_art.py'})
