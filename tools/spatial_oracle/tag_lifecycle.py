"""Original live Tag/Trigger construction, dispatch, timers and attachments.

The first49 histories retain their original supplied definitions/world boundaries.
Additive controls execute live cleanup gates, full original stock Type readers,
CellTags and Fill classification, resolved53, and a first stock poll stopped at
its first gameplay-action boundary. Cached lexical inputs, spare storage and
minimal world objects are supplied through existing oracle owners. No full-map
RNG count, complete opening-action execution or Team AI parity is claimed.
"""
from pathlib import Path
import hashlib
import json
import os
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EBP,
    UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EDI, UC_X86_REG_EIP,
    UC_X86_REG_ESI, UC_X86_REG_ESP)
from tools.native_oracle import SCRATCH, RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.aircraft_fire_location import Fixture, SCENARIO, cell
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.team_creation import u32, i32

HOUSE, HOUSE_TYPE, TAG_TYPE = [SCRATCH+n for n in (0x20000, 0x30000, 0x40000)]
TAG_TYPES = [TAG_TYPE+n*0x100 for n in range(8)]
TRIGGER_TYPES = [SCRATCH+0x41000+n*0x100 for n in range(8)]
EVENTS, ACTIONS, CONTROL = [SCRATCH+n for n in (0x43000, 0x46000, 0x49000)]
HEAP, HEAP_END = SCRATCH+0x60000, SCRATCH+0x80000
REGISTRIES = {
    'abstract':0xB0F670, 'object_base':0xB0F618, 'tag':0xB0E720,
    'tag_trigger_base':0xB0F708, 'trigger':0xA8EAE8, 'pending':0xB0F698,
    'logic_tags':0x8B40C8, 'map_tags':0x8B41A8, 'cell_tags':0x880944,
    'object':0xA8E360, 'object_base_b':0xB0F720,
}


class StockPrefix:
    """Compose the existing campaign cached-reader VM, never another loader.

    Full native Type list/readers execute. Sparse cells and minimal Houses are
    supplied world storage: terrain Fill, concrete Techno constructors and their
    RNG are excluded. A stock poll stops at its first gameplay action boundary.
    """

    TYPE_VECTORS = {'team_type':0xA8ECA0, 'script_type':0x8B41C8,
                    'task_force':0xA8E8D0, 'trigger_type':0x8B4178,
                    'tag_type':0xB0E780, 'event':0xB0F1A0, 'action':0xB0E658,
                    'type_base':0xB0F658}

    def __init__(self, difficulty):
        from tools.input_oracle.campaign_start import (CampaignReader, assets_root, install_csf,
                                                       prepare_house_rules)
        from tools.rules_oracle.bridge_child_sound import sections
        from tools.spatial_oracle.building_body_rules import INI, SP
        self.ini, self.sp = INI, SP
        self.m = CampaignReader()
        self.u, self.scenario = self.m.u, self.m.scenario
        self.phase = 'supplied_priors'
        self.events, self.draws, self.dispatches, self.frees, self.resets = [], [], [], [], []
        self.cells, self.objects = {}, []
        m,u = self.m,self.u
        m.invoke(0x6832C0,self.scenario)
        m.seed_streams(31)
        u.mem_write(self.scenario+0x60C,dwords(difficulty))
        u.mem_write(self.scenario+0x11E8,dwords(-1))
        u.mem_write(0xA8E7AC,dwords(2))
        u.mem_write(0xA8ED6B,b'\0')
        u.mem_write(0xA8E9A0,b'\0')
        u.mem_write(0xA8ED5C,b'\0')
        u.mem_map(0,0x1000)
        u.mem_write(0,dwords(0xFFFFFFFF))
        # Reuse the established registry/storage ABI. Spare capacity excludes
        # vector growth; constructors still publish every actual pointer.
        for name,base in (REGISTRIES | self.TYPE_VECTORS).items():
            capacity=8192 if name in ('abstract','type_base') else 2048
            u.mem_write(base,dwords(0x7EB6D4,m.alloc(capacity*4),capacity,1,0,10))
        for base,writer in ((0xB0F670,0x72536D),(0xB0F708,0x72546D),
                            (0xB0E720,0x6E4D7D),(0xA8EAE8,0x4E65FD),
                            (0xB0F618,0x7253ED),(0xB0F698,0x72586D)):
            instruction=bytes(u.mem_read(writer,10))
            assert instruction[:6]==b'\xC7\x05'+dwords(base)
            u.mem_write(base,instruction[6:])
        for base,vt in ((0x880944,0x7E3890),(0x8B41A8,0x7EA5A4),(0x8B40C8,0x7EA5A4)):
            u.mem_write(base,dwords(vt))
        u.mem_write(0xB0C110+0x1C,dwords(0x7EB6D4,m.alloc(16384),2048,1,0,10))
        u.mem_write(0x87F7E8+0x115C,dwords(0x7EB6D4,m.alloc(8192),2048,1,0,10))
        # Original Map lookup5657A0 executes on this existing sparse-table ABI.
        from tools.spatial_oracle.map_queries import TABLE, EMPTY_TABLE, GLOBAL_TABLE
        u.mem_write(TABLE,EMPTY_TABLE)
        u.mem_write(GLOBAL_TABLE,dwords(TABLE))
        self.table=TABLE
        root=Path(os.environ['VERA20K_TAG_LIFECYCLE_ASSETS'])
        raw_map=(assets_root()/'sov01umd.map').read_bytes()
        raw_ai=(root/'aimd.ini').read_bytes()
        raw_eva=(root/'evamd.ini').read_bytes()
        raw_rules=(Path('ini')/'rulesmd.ini').read_bytes()
        self.sources={name:dict(bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest())
                      for name,raw in (('sov01umd.map',raw_map),('aimd.ini',raw_ai),
                                       ('evamd.ini',raw_eva),('rulesmd.ini',raw_rules))}
        self.physical_map,self.physical_ai = sections(raw_map),sections(raw_ai)
        m.ordered_ini({'Basic':self.physical_map['Basic']})
        u.reg_write(UC_X86_REG_ESI,self.scenario)
        u.reg_write(UC_X86_REG_EDI,INI)
        u.reg_write(UC_X86_REG_EAX,m.read32(self.scenario+0x1C70))
        m.region(0x68A13D,0x68A15B,required=(0x5276D0,),context={'phase':'Basic_NewINIFormat'})
        self.format=i32(u,0xA8ED7C)
        rules=sections(raw_rules)
        prepare_house_rules(m,rules)
        csf_names={body['UIName'].upper() for body in rules.values() if 'UIName' in body}
        from tools.storage_oracle.keyboard_bindings import stock_csf
        csf,_=stock_csf()
        install_csf(m,csf_names & csf.keys())
        names=list(rules['Countries'].values())
        for name in self.physical_map.get('Countries',{}).values():
            if name.lower() not in {x.lower() for x in names}:names.append(name)
        countries={}
        for name in names:
            pointer=m.alloc(0x300)
            m.invoke(0x5113F0,pointer,(m.cstring(name),))
            countries[name]=pointer
        for layer in (rules,self.physical_map):
            m.ordered_ini({name:layer[name] for name in names if name in layer})
            for pointer in countries.values():m.invoke(0x511850,pointer,(INI,))
        # Supplied House storage has the actual authored Country binding. No
        # House constructor/difficulty/placement result is certified here.
        self.houses={}
        for name in self.physical_map['Houses'].values():
            pointer=m.alloc(0x17000)
            country=m.invoke(0x5117D0,m.cstring(self.physical_map[name].get('Country','')))
            assert country < len(countries)
            typ=m.read32(m.read32(0xA83C9C)+country*4)
            u.mem_write(pointer+0x34,dwords(typ))
            u.mem_write(pointer+0x38,dwords(0x7EB6D4,m.alloc(8192),2048,1,0,10))
            u.mem_write(m.read32(0xA8022C)+len(self.houses)*4,dwords(pointer))
            self.houses[name]=pointer
        u.mem_write(0xA80238,dwords(len(self.houses)))
        eva_names=list(sections(raw_eva)['DialogList'].values())
        # Original753250 consumes the process EVA name array. Playback and
        # DialogClass/device objects are outside this prefix's scope.
        array=m.alloc(len(eva_names)*4)
        for index,name in enumerate(eva_names):u.mem_write(array+index*4,dwords(m.cstring(name)))
        u.mem_write(0xB1D4A4,dwords(array))
        u.mem_write(0xB1D4B0,dwords(len(eva_names)))
        self.eva_names=eva_names
        self.hook=u.hook_add(UC_HOOK_CODE,self.observe)
        self.id_start=m.read32(self.scenario+0x214)
        self.rng_start=m.rng_state()

    def vector(self,base):
        return [self.m.read32(self.m.read32(base+4)+i*4) for i in range(self.m.read32(base+16))]

    def type_name(self,pointer):
        return self.m.string(pointer+0x24) if pointer else None

    def observe(self,u,pc,size,data):
        m,sp=self.m,u.reg_read(UC_X86_REG_ESP)
        if pc==0x65C837:self.draws.append(dict(phase=self.phase))
        if pc in (0x6F06E0,0x6E5B60,0x726C80):
            self.events.append(dict(phase=self.phase,pc=f'{pc:08x}',
                caller=f'{m.read32(sp):08x}',incoming=m.string(m.read32(sp+4))))
        if pc in (0x6E4DE0,0x725FA0):
            self.events.append(dict(phase=self.phase,pc=f'{pc:08x}',
                type=self.type_name(m.read32(sp+4))))
        if pc==0x6E53A0:
            self.dispatches.append([self.type_name(m.read32(u.reg_read(UC_X86_REG_ECX)+0x24)),
                                    i32(u,sp+4)])
        if pc==0x726400:
            self.resets.append(dict(phase=self.phase,
                type=self.type_name(m.read32(u.reg_read(UC_X86_REG_ECX)+0x24)),
                frame=i32(u,0xA8ED84)))
        if pc==0x5657A0:
            xy=tuple(struct.unpack('<hh',u.mem_read(m.read32(sp+4),4)))
            assert 0<=xy[0]<512 and 0<=xy[1]<512,xy
            if xy not in self.cells:
                pointer=m.alloc(0x200)
                u.mem_write(pointer,dwords(0x7E3890))
                u.mem_write(pointer+0x24,packed(*xy))
                u.mem_write(self.table+(xy[1]*512+xy[0])*4,dwords(pointer))
                self.cells[xy]=pointer

    def list_call(self,entry,boundary,*,fixed=False):
        m,u=self.m,self.u
        u.mem_write(self.sp,dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP,self.sp)
        u.reg_write(UC_X86_REG_ECX,self.ini)
        u.reg_write(UC_X86_REG_EDX,int(fixed))
        begin=entry
        while begin != RET_MAGIC:
            run_checked(u,begin,(boundary,RET_MAGIC),count=2_000_000,
                        context={'phase':self.phase})
            stopped=u.reg_read(UC_X86_REG_EIP)
            if stopped==RET_MAGIC:break
            # Advance the genuine return instruction and resume from the
            # native next PC. No outer-loop register or local is rehydrated.
            next_instruction={0x6F1A8E:0x6F1A8F,0x727684:0x727685,0x6E5FBE:0x6E5FC2}[boundary]
            run_checked(u,stopped,next_instruction,count=10,context={'phase':self.phase})
            begin=u.reg_read(UC_X86_REG_EIP)
        assert u.reg_read(UC_X86_REG_ESP)==self.sp+4

    def definitions(self):
        m,u=self.m,self.u
        tags=[]
        for pointer in self.vector(0xB0E780):
            tags.append(dict(id=self.type_name(pointer),repeat=i32(u,pointer+0x9C),
                trigger=self.type_name(m.read32(pointer+0xA0)),
                flags=m.invoke(0x6E61F0,pointer)&255))
        return tags

    def snapshot(self,phase):
        m,u=self.m,self.u
        live=[]
        for pointer in self.vector(REGISTRIES['tag']):
            trigger=m.read32(pointer+0x28);chain=[]
            while trigger:
                chain.append(dict(type=self.type_name(m.read32(trigger+0x24)),
                    enabled=u.mem_read(trigger+0x44,1)[0],pending=u.mem_read(trigger+0x30,1)[0],
                    start=i32(u,trigger+0x34),duration=i32(u,trigger+0x3C),completed=m.read32(trigger+0x40)))
                trigger=m.read32(trigger+0x28)
                assert len(chain)<=self.m.read32(0x8B4188)
            live.append(dict(type=self.type_name(m.read32(pointer+0x24)),refs=i32(u,pointer+0x2C),
                             pending=u.mem_read(pointer+0x34,1)[0],guard=u.mem_read(pointer+0x35,1)[0],
                             triggers=chain))
        names=lambda base:[self.type_name(m.read32(p+0x24)) for p in self.vector(base)]
        return dict(phase=phase,live_tags=live,logic_tags=names(0x8B40C8),map_tags=names(0x8B41A8),
                    house_tags={name:names(pointer+0x38) for name,pointer in self.houses.items()},
                    cursor=m.read32(self.scenario+0x214)-self.id_start,rng_draws=len(self.draws),
                    scenario_init=i32(u,0xA8E7AC),frame=i32(u,0xA8ED84),
                    logic_cursor=i32(u,0xA83CDC),dispatches=list(self.dispatches))

    def type_orders(self):
        return {name:[self.type_name(p) for p in self.vector(base)]
                for name,base in self.TYPE_VECTORS.items()
                if name in ('team_type','trigger_type','tag_type','script_type','task_force')}

    def selected_trigger(self,identity):
        """Read observed original fields, preserving native linked-node order."""
        m,u=self.m,self.u
        pointer=next(p for p in self.vector(0x8B4178) if self.type_name(p)==identity)
        events=[];node=m.read32(pointer+0xAC)
        while node:
            events.append(dict(kind=i32(u,node+0x2C),value=i32(u,node+0x34)))
            node=m.read32(node+0x28)
        actions=[];node=m.read32(pointer+0xB0)
        while node:
            actions.append(dict(kind=i32(u,node+0x2C),value=i32(u,node+0x90),
                team=self.type_name(m.read32(node+0x30)),tag=self.type_name(m.read32(node+0x4C)),
                trigger=self.type_name(m.read32(node+0x50)),
                parameters=[i32(u,node+off) for off in (0x34,0x38,0x3C,0x40)]))
            node=m.read32(node+0x28)
        return dict(id=identity,index=i32(u,pointer+0x98),flags=list(u.mem_read(pointer+0x9C,5)),
                    owner=self.type_name(m.read32(pointer+0xA4)),linked=self.type_name(m.read32(pointer+0xA8)),
                    events=events,actions=actions)


def stock_registration(difficulty):
    s=StockPrefix(difficulty);m,u=s.m,s.u
    history=[];orders=[]
    for phase,values,fixed in [('fixed_aimd',s.physical_ai,True),('map_teamtypes',s.physical_map,False)]:
        s.phase=phase
        chosen={'TeamTypes':values['TeamTypes']}
        chosen.update({name:values[name] for name in values['TeamTypes'].values() if name in values})
        m.ordered_ini(chosen)
        s.list_call(0x6F19B0,0x6F1A8E,fixed=fixed)
        orders.append(dict(phase=phase,registries=s.type_orders()))
    s.phase='map_triggers'
    m.ordered_ini({name:s.physical_map[name] for name in ('Triggers','Events','Actions','Tags','CellTags')})
    s.list_call(0x7275D0,0x727684)
    orders.append(dict(phase=s.phase,registries=s.type_orders()))
    s.phase='map_tags'
    s.list_call(0x6E5ED0,0x6E5FBE)
    orders.append(dict(phase=s.phase,registries=s.type_orders()))
    definitions=s.definitions()
    selected=[s.selected_trigger(name) for name in ('0B76DF3C','0DD8E18C')]
    s.phase='cell_tags'
    u.reg_write(UC_X86_REG_ESP,s.sp)
    u.reg_write(UC_X86_REG_EDI,s.ini)
    u.reg_write(UC_X86_REG_EBP,0x87F7E8)
    u.reg_write(UC_X86_REG_EBX,0)
    run_checked(u,0x4AD1B3,0x4AD332,count=2_000_000,context={'phase':s.phase})
    history.append(s.snapshot(s.phase))
    s.phase='classification'
    u.reg_write(UC_X86_REG_ESP,s.sp)
    run_checked(u,0x684D47,0x684EC1,count=2_000_000,context={'phase':s.phase})
    history.append(s.snapshot(s.phase))
    # This genuine first poll remains a prefix: no gameplay action is replaced
    # by a successful return. Its incomplete call/guard state is retained.
    s.phase='first_logic_action_boundary'
    u.reg_write(UC_X86_REG_ESP,s.sp)
    u.reg_write(UC_X86_REG_ECX,0x87F778)
    run_checked(u,0x55AFB0,(0x6DD8B0,0x55B205),count=2_000_000,
                context={'phase':s.phase,'difficulty':difficulty})
    stopped=u.reg_read(UC_X86_REG_EIP)
    first_poll=dict(stopped=f'{stopped:08x}',complete=stopped==0x55B205)
    if stopped==0x6DD8B0:
        action=u.reg_read(UC_X86_REG_ECX)
        first_poll.update(action_kind=i32(u,action+0x2C),
                          trigger=s.type_name(m.read32(u.reg_read(UC_X86_REG_EDI)+0x24)))
    history.append(s.snapshot(s.phase))
    # Independent complete native calls on the retained stock instances. The
    # suspended first-poll stack is abandoned; its memory writes stay visible.
    intro_type=next(p for p in s.vector(0x8B4178) if s.type_name(p)=='0DD8E18C')
    intro=next(p for p in s.vector(REGISTRIES['trigger']) if m.read32(p+0x24)==intro_type)
    setup_type=next(p for p in s.vector(0x8B4178) if s.type_name(p)=='0B76DF3C')
    enable=m.read32(setup_type+0xB0)
    while i32(u,enable+0x2C)!=53:enable=m.read32(enable+0x28);assert enable
    assert m.read32(enable+0x50)==intro_type
    observations=[]
    for frame in (0,100):
        u.mem_write(0xA8ED84,dwords(frame));s.phase='disabled_intro_predicate'
        returned=m.invoke(0x7264C0,intro,(13,0,0,0,0))&255
        observations.append(dict(phase=s.phase,frame=frame,returned=returned,
                                 reset_count=len(s.resets)))
        history.append(s.snapshot(s.phase))
    for enable_frame in (137,230):
        u.mem_write(0xA8ED84,dwords(enable_frame));s.phase='resolved53_enable'
        mark=len(s.resets)
        returned=m.invoke(0x6DD8B0,enable,(s.houses['Americans'],0,0,0))&255
        observations.append(dict(phase=s.phase,frame=enable_frame,returned=returned,
                                 reset_targets=s.resets[mark:]))
        history.append(s.snapshot(s.phase))
        for frame in (enable_frame+74,enable_frame+75):
            u.mem_write(0xA8ED84,dwords(frame));s.phase='enabled_intro_predicate'
            returned=m.invoke(0x7264C0,intro,(13,0,0,0,0))&255
            observations.append(dict(phase=s.phase,frame=frame,returned=returned,
                                     reset_count=len(s.resets)))
            history.append(s.snapshot(s.phase))
    m.check_code()
    return dict(input=dict(name=f'stock_sov01_registration_difficulty_{difficulty}',stock_prefix=True,
                           difficulty=difficulty,seed=31,frame=0),sources=s.sources,
                basic_new_ini_format=s.format,type_orders=orders,selected_triggers=selected,
                definitions=definitions,construction=s.events,history=history,
                first_poll=first_poll,observations=observations,resets=s.resets,
                rng_before=s.rng_start,rng_after=m.rng_state(),draws=s.draws,
                excluded=['Terrain Fill and authored concrete objects/attachments; full Rules/country/House numeric prefix.',
                          'Script/TaskForce body readers, physical INI/archive loading, complete stock first Logic/action firing.',
                          'Enable/predicate calls are explicit supplied-frame controls after abandoning the stopped first-poll stack; no stock absolute enable or firing frame.'])


def reference_controls():
    """Original TAction record reader on retained, native-created Type arrays.

    No negative/out-of-range ordinal pointer is executed. The real constructor
    stores ID/name/default fields on unknown long references; no map sorting is
    used to resolve short numeric operands.
    """
    s=StockPrefix(1);m,u=s.m,s.u
    for name in ('First','Second','Third'):
        m.invoke(0x6F06E0,m.alloc(0xF8),(m.cstring(name),))
        m.invoke(0x726C80,m.alloc(0xB4),(m.cstring(name),))
        m.invoke(0x6E5B60,m.alloc(0xA4),(m.cstring(name),))
    s.phase='reference_controls'
    rows=[]
    for parameter,offset,base in ((1,0x30,0xA8ECA0),(5,0x30,0xA8ECA0),
                                  (2,0x50,0x8B4178),(3,0x4C,0xB0E780)):
        for token in ('-1','-1tail','0','01','2','002','second','Missing',
                      'ABCDEFGHIJKLMNOPQRSTUVWXYZ12345','<none>','none'):
            raw=f'1,53,{parameter},{token},0,0,0,0,A'
            pointer=m.alloc(0x94)
            m.invoke(0x6DD000,pointer)
            buffer=m.cstring(raw)
            # Seed the existing original strtok state exactly as72754A does.
            m.invoke(0x7C9CC2,0,(buffer,0x817F70))
            before=[s.type_name(p) for p in s.vector(base)]
            mark=len(s.events)
            m.invoke(0x6DD5B0,pointer)
            target=m.read32(pointer+offset)
            result=None
            if target:
                result=dict(index=s.vector(base).index(target),id=s.type_name(target),
                            name=m.string(target+0x64),id_hex=bytes(u.mem_read(target+0x24,25)).hex())
                if base==0x8B4178:
                    result.update(flags=list(u.mem_read(target+0x9C,5)),
                                  linked=s.type_name(m.read32(target+0xA8)),
                                  event_count=0 if not m.read32(target+0xAC) else None,
                                  action_count=0 if not m.read32(target+0xB0) else None)
                if base==0xB0E780:
                    result.update(repeat=i32(u,target+0x9C),trigger=s.type_name(m.read32(target+0xA0)))
            rows.append(dict(parameter_type=parameter,token=token,raw=raw,
                             before=before,result=result,after=[s.type_name(p) for p in s.vector(base)],
                             constructors=s.events[mark:]))
    team_tags=team_tag_reference_controls(s)
    m.check_code()
    return dict(input=dict(name='original_action_reference_controls',reference_controls=True),
                initial_ids=['First','Second','Third'],rows=rows,team_tag_controls=team_tags)


def team_tag_reference_controls(s):
    """Original6F131E..6F1363 physical Tag field reads and retained pointers.

    Each control supplies an empty TagType registry and a real TeamType
    constructor. Full TeamType parsing and scene teardown are outside this
    field boundary; ReadString528A10 and FindOrAllocate6E6310 run unchanged.
    """
    m,u=s.m,s.u
    long='ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890'
    cases=[('long_absent',[long,None]),('long_empty',[long,'']),
           ('long_repeated',[long,long]),('long_case_repeated',[long,long.lower()]),
           ('long_none',[long,'NoNe']),('keep_bracket_none',['Keep','<NoNe>']),
           ('cold_bracket_none',[None,'<none>']),('capacity128',[None,'K'*128]),
           ('keep_capacity128',['Keep','K'*128]),('keep_absent',['Keep',None]),
           ('keep_empty',['Keep','']),('unknown_fresh',[None,'Unknown'])]
    controls=[]
    for name,values in cases:
        # Explicit independent reader prior. Previously allocated definitions
        # remain valid arena objects but are not members of this supplied list.
        u.mem_write(0xB0E790,dwords(0))
        team=m.alloc(0xF8)
        m.invoke(0x6F06E0,team,(m.cstring('TT'),))
        initial_rng=m.rng_state()
        history=[]
        for value in values:
            s.phase=f'team_tag_{name}'
            m.ordered_ini({'TT':{} if value is None else {'Tag':value}})
            before=[s.type_name(p) for p in s.vector(0xB0E780)]
            mark=len(s.events)
            u.reg_write(UC_X86_REG_ESI,team)
            u.reg_write(UC_X86_REG_EBX,s.ini)
            u.reg_write(UC_X86_REG_EDI,m.cstring('TT'))
            # The preceding reader's +CC value is a supplied, excluded prior.
            u.reg_write(UC_X86_REG_EAX,0)
            m.region(0x6F131E,0x6F1363,required=(0x528A10,),
                     context={'case':name,'value':value})
            pointer=m.read32(team+0xD0)
            result=None if not pointer else dict(index=s.vector(0xB0E780).index(pointer),
                                                id=s.type_name(pointer))
            history.append(dict(value=value,before=before,result=result,
                                after=[s.type_name(p) for p in s.vector(0xB0E780)],
                                constructors=s.events[mark:]))
        assert m.rng_state()==initial_rng
        controls.append(dict(name=name,team_type='TT',values=values,history=history,
                             rng_before=initial_rng,rng_after=m.rng_state()))
    return controls


def execute(case):
    f = Fixture(dict(seed=case.get('seed',31), include_self=False,
                     game_mode=case.get('game_mode',1)))
    u = f.u
    # CRT dynamic_cast in DetachAll installs an SEH frame at fs:[0]. Supply
    # the empty thread exception chain; its original RTTI body still runs.
    u.mem_map(0,0x1000)
    u.mem_write(0,dwords(0xFFFFFFFF))
    u.mem_write(0xA8ED84, dwords(case.get('frame',123)))
    u.mem_write(0xA8ED6B, b'\0')
    for base,value in zip((0xA8E9A0,0xA8ED5C),case.get('live_flags',[0,0])):
        u.mem_write(base,bytes([value]))
    u.mem_write(SCENARIO+0x60C, dwords(case.get('difficulty',1)))
    u.mem_write(SCENARIO+0x11E8, dwords(-1))
    for index, base in enumerate(REGISTRIES.values()):
        u.mem_write(base+4, dwords(SCRATCH+0x50000+index*0x400,128,0,0,128))
    # Original constructor/static-initializer vtables:565157,4E7F4D,4E7FCD.
    # Detach invokes their real Find bodies even when the vectors are empty.
    u.mem_write(REGISTRIES['cell_tags'],dwords(0x7E3890))
    u.mem_write(REGISTRIES['map_tags'],dwords(0x7EA5A4))
    u.mem_write(REGISTRIES['logic_tags'],dwords(0x7EA5A4))
    for base, writer in ((0xB0F670,0x72536D),(0xB0F708,0x72546D),
                         (0xB0E720,0x6E4D7D),(0xA8EAE8,0x4E65FD),
                         (0xB0F618,0x7253ED),(0xB0F698,0x72586D)):
        instruction=bytes(u.mem_read(writer,10))
        assert instruction[:6]==b'\xC7\x05'+dwords(base)
        u.mem_write(base,instruction[6:])
    if case.get('mixed_deferred_queue',False):
        for base,writer in ((0xA8E360,0x4E7AFD),(0xB0F720,0x7252ED)):
            instruction=bytes(u.mem_read(writer,10))
            assert instruction[:6]==b'\xC7\x05'+dwords(base)
            u.mem_write(base,instruction[6:])
    u.mem_write(0xA8022C, dwords(SCRATCH+0x4F000))
    u.mem_write(0xA80238, dwords(1))
    u.mem_write(SCRATCH+0x4F000, dwords(HOUSE))
    u.mem_write(HOUSE+0x34, dwords(HOUSE_TYPE))
    u.mem_write(HOUSE_TYPE+0xB4, dwords(0,0))
    house_funds = case.get('house_funds')
    if house_funds is not None:
        # Supplied empty-storage House priors, not a House constructor or a
        # silo-initialization result. The actual IHouse money getter executes.
        u.mem_write(HOUSE,dwords(0x7EA8A0))
        assert u32(u,0x7EA8A0+0x28)==0x4FB9B0
        u.mem_write(HOUSE+0x24,dwords(0x7EA834))
        u.mem_write(HOUSE+0x38,dwords(0x7EB6D4,SCRATCH+0x4E000,128,1,0,10))
        u.mem_write(HOUSE+0x2FC,bytes(16))
        u.mem_write(HOUSE+0x30C,dwords(house_funds))
        u.mem_write(HOUSE_TYPE+0x148,struct.pack('<f',1.0))
        from tools.spatial_oracle.house_tag_raise import RAISE_CELL
        u.mem_write(RAISE_CELL,dwords(0))
        for pointer in (0x886B88,0xABE890):
            f.call(0x65C6D0,pointer,[case.get('seed',31)])
    if case.get('active_cleanup',False):
        u.mem_write(HOUSE+0x27C,dwords(case.get('allow_win_count',3)))
        u.mem_write(HOUSE+0x298,dwords(-1,0x11223344,99))
        u.mem_write(0xA83CDC,dwords(case.get('logic_cursor',0)))
        u.mem_write(0xA83DBC,dwords(case.get('map_cursor',0)))
    definitions = case.get('triggers',[dict(events=[[47,0]], actions=[[28,7]])])
    for typ in TAG_TYPES:
        u.mem_write(typ+0x9C, dwords(case.get('repeat',2), TRIGGER_TYPES[0] if definitions else 0))
    event_cursor, action_cursor = EVENTS, ACTIONS
    for index, definition in enumerate(definitions):
        typ = TRIGGER_TYPES[index]
        u.mem_write(typ+0x9C, bytes(definition.get('flags',[1,1,1,1])))
        u.mem_write(typ+0xA4, dwords(HOUSE_TYPE, TRIGGER_TYPES[index+1] if index+1<len(definitions) else 0))
        events = definition.get('events',[])
        actions = definition.get('actions',[])
        u.mem_write(typ+0xAC, dwords(event_cursor if events else 0, action_cursor if actions else 0))
        for n, event in enumerate(events):
            u.mem_write(event_cursor+0x28, dwords(event_cursor+0x60 if n+1<len(events) else 0,event[0],0,event[1]))
            event_cursor += 0x60
        for n, action in enumerate(actions):
            u.mem_write(action_cursor+0x28, dwords(action_cursor+0xA0 if n+1<len(actions) else 0,action[0]))
            u.mem_write(action_cursor+0x90, dwords(action[1]))
            if len(action)>2:
                u.mem_write(action_cursor+0x50, dwords(TRIGGER_TYPES[action[2]]))
            action_cursor += 0xA0
    cursor = HEAP
    allocations, actions_run, draws, calls, tags, objects = [], [], [], [], [], []
    dispatches, trigger_handles, destruct_order = [], [], []
    allocated, freed, allocation_types, frees = set(), set(), {}, []
    pointer_probe=RET_MAGIC+0x100
    u.mem_write(0x7E115C,dwords(pointer_probe))
    def ticket(pointer):
        if pointer in tags:return ['tag',tags.index(pointer)]
        if pointer in trigger_handles:return ['trigger',trigger_handles.index(pointer)]
        if pointer in objects:return ['object',objects.index(pointer)]
        raise AssertionError(('unknown pending identity',hex(pointer)))
    def observe(_u, pc, _size, _data):
        nonlocal cursor
        if pc==pointer_probe:
            # Original CRT typeid probes its real TypeDescriptor through this
            # Windows import. Validate memory before supplying OS success.
            sp=u.reg_read(UC_X86_REG_ESP)
            ret,pointer,length=struct.unpack('<3I',u.mem_read(sp,12))
            u.mem_read(pointer,length)
            u.reg_write(UC_X86_REG_EAX,0)
            u.reg_write(UC_X86_REG_ESP,sp+12)
            u.reg_write(UC_X86_REG_EIP,ret)
            return
        if pc == 0x65C837:
            draws.append(1)
        if pc == 0x6E53A0:
            dispatches.append([tags.index(u.reg_read(UC_X86_REG_ECX)),
                               i32(u,u.reg_read(UC_X86_REG_ESP)+4)])
        if pc in (0x6E4DE0,0x725FA0,0x726400,0x726720,0x7258D0,0x6E4F60,0x726950):
            calls.append(f'{pc:08X}')
        if pc in (0x6E4DE0,0x725FA0):
            allocation_types[u.reg_read(UC_X86_REG_ECX)]='tag' if pc==0x6E4DE0 else 'trigger'
            if pc==0x725FA0:trigger_handles.append(u.reg_read(UC_X86_REG_ECX))
        if pc==0x725D7A and case.get('mixed_deferred_queue',False):
            destruct_order.append(ticket(u.reg_read(UC_X86_REG_ECX)))
        if pc == 0x6DD8B0:
            action = u.reg_read(UC_X86_REG_ECX)
            actions_run.append([i32(u,action+0x2C),i32(u,action+0x90)])
        if pc == 0x7C8B3D:
            sp=u.reg_read(UC_X86_REG_ESP)
            pointer=u32(u,sp+4)
            assert pointer in allocated and pointer not in freed, (hex(pointer),hex(u32(u,sp)),allocated,freed)
            freed.add(pointer)
            frees.append(allocation_types[pointer])
            u.reg_write(UC_X86_REG_ESP,sp+4)
            u.reg_write(UC_X86_REG_EIP,u32(u,sp))
            return
        if pc != 0x7C8E17:
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        size = u32(u,sp+4)
        pointer = cursor
        cursor += (max(size,1)+15)&~15
        assert cursor < HEAP_END
        u.mem_write(pointer,b'\xA5'*max(size,1))
        allocations.append(size)
        allocated.add(pointer)
        u.reg_write(UC_X86_REG_EAX,pointer)
        u.reg_write(UC_X86_REG_ESP,sp+4)
        u.reg_write(UC_X86_REG_EIP,u32(u,sp))
    hook = u.hook_add(UC_HOOK_CODE,observe)
    ranges = [(0x6E4DE0,0x6E5AA0),(0x725FA0,0x726B00),(0x71E940,0x71FA40),
              (0x7258D0,0x725D90),(0x5F3900,0x5F3B40),(0x55AFB0,0x55B205),
              (0x6DD8B0,0x6E45D0),(0x689670,0x689A90)]
    if case.get('mixed_deferred_queue',False):
        ranges.extend(((0x5F3B80,0x5F3E00),(0x5F6690,0x5F66A0),(0x5F6DC0,0x5F6DF0)))
    if house_funds is not None:
        from tools.spatial_oracle.house_tag_raise import RAISE_BEGIN, RAISE_END
        ranges.extend(((RAISE_BEGIN,RAISE_END),(0x684D47,0x684EC1),
                       (0x4F5F99,0x4F5FB1),(0x4FB9B0,0x4FBB50),
                       (0x4F6990,0x4F69CA),(0x4F9790,0x4F99D0),
                       (0x6C9600,0x6C964F),(0x7C5F00,0x7C5F38)))
    code = [bytes(u.mem_read(a,b-a)) for a,b in ranges]
    if house_funds is not None:
        # Execute the genuine House constructor's listener append. This lets
        # the original PointerExpired remove a deleted Tag from House+38.
        u.reg_write(UC_X86_REG_EBP,HOUSE)
        u.reg_write(UC_X86_REG_ESP,f.sp)
        run_checked(u,0x4F5F99,0x4F5FB1)
        assert u.reg_read(UC_X86_REG_ESP)==f.sp
        rng_before={pointer:bytes(u.mem_read(pointer,0x3F4))
                    for pointer in (SCENARIO+0x218,0x886B88,0xABE890)}
    history = []
    def snapshot(returned):
        tag_states = []
        for tag in tags:
            if tag in freed:
                tag_states.append(None)
                continue
            instances, trigger = [], u32(u,tag+0x28)
            while trigger:
                instances.append(dict(type=TRIGGER_TYPES.index(u32(u,trigger+0x24)),
                    enabled=u.mem_read(trigger+0x44,1)[0],pending=u.mem_read(trigger+0x30,1)[0],
                    start=i32(u,trigger+0x34),duration=i32(u,trigger+0x3C),completed=u32(u,trigger+0x40)))
                trigger = u32(u,trigger+0x28)
                assert len(instances)<=len(definitions)
            tag_states.append(dict(refs=i32(u,tag+0x2C),pending=u.mem_read(tag+0x34,1)[0],triggers=instances))
        pending_base = REGISTRIES['pending']
        pending = [u32(u,u32(u,pending_base+4)+n*4) for n in range(i32(u,pending_base+16))]
        state=dict(returned=returned,tags=tag_states,actions=list(actions_run),rng_draws=len(draws),
            pending_count=len(pending),pending_tags=[tags.index(p) for p in pending if p in tags],
            globals=[n for n in range(50) if u.mem_read(SCENARIO+0x1CB0+n*41,1)[0]],
            locals=[n for n in range(100) if u.mem_read(SCENARIO+0x24B2+n*41,1)[0]],
            changed=u.mem_read(SCENARIO+0x34AA,1)[0])
        if case.get('lifecycle',False):
            state.update(frees=list(frees),registry_counts={name:i32(u,base+16) for name,base in REGISTRIES.items()},
                         object_tags=[tags.index(u32(u,obj+0x34)) if obj not in freed and u32(u,obj+0x34) else None for obj in objects])
        if case.get('mixed_deferred_queue',False):
            state.update(pending_tickets=[ticket(pointer) for pointer in pending],
                         destruct_order=list(destruct_order),
                         object_alive=[None if obj in freed else u.mem_read(obj+0x90,1)[0] for obj in objects])
        if case.get('polling',False):
            base=REGISTRIES['logic_tags']
            state.update(dispatches=list(dispatches),
                         logic_tags=[tags.index(u32(u,u32(u,base+4)+n*4)) for n in range(i32(u,base+16))],
                         scenario_flags=[u.mem_read(SCENARIO+n,1)[0] for n in (0x34AA,0x34A9,0x34AB,0x34BE)])
        if case.get('active_cleanup',False):
            state.update(logic_cursor=i32(u,0xA83CDC),map_cursor=i32(u,0xA83DBC),
                         allow_win_count=i32(u,HOUSE+0x27C),
                         allow_win_timer=[i32(u,HOUSE+off) for off in (0x298,0x2A0)])
        if house_funds is not None:
            state['funds']=dict(wallet=i32(u,HOUSE+0x30C),spent=i32(u,HOUSE+0x2DC),
                                house_tag_count=i32(u,HOUSE+0x48),
                                logic_tag_count=i32(u,REGISTRIES['logic_tags']+16))
        return state
    for op in case.get('operations',[['shared'],['dispatch',0,13],['dispatch',0,13]]):
        kind, *args = op
        returned = None
        if kind in ('shared','fresh'):
            tag_type=TAG_TYPES[args[0] if args else 0]
            if kind=='shared':
                tag = f.call(0x6E52A0,tag_type,[])
            else:
                tag = cursor
                cursor += 0x40
                allocated.add(tag)
                u.mem_write(tag,b'\xA5'*0x38)
                f.call(0x6E4DE0,tag,[tag_type])
            if tag not in tags:
                tags.append(tag)
            returned = tags.index(tag)
        elif kind=='frame':
            u.mem_write(0xA8ED84,dwords(args[0]))
        elif kind=='classify_house_tags':
            # Supplied TagType identities, original Fill classification and
            # registration. No hand-authored House/Logic membership list.
            assert house_funds is not None and len(args)<=128
            array=SCRATCH+0x4E800
            u.mem_write(array,dwords(*(TAG_TYPES[n] for n in args)))
            u.mem_write(0xB0E780+4,dwords(array,128,1,len(args),10))
            u.reg_write(UC_X86_REG_ESP,f.sp)
            run_checked(u,0x684D47,0x684EC1)
            assert u.reg_read(UC_X86_REG_ESP)==f.sp
        elif kind in ('spend_money','add_credits'):
            assert house_funds is not None and len(args)==1
            f.call(0x4F9790 if kind=='spend_money' else 0x4F9950,HOUSE,args)
        elif kind=='house_poll':
            assert house_funds is not None
            u.reg_write(UC_X86_REG_ESP,f.sp)
            u.reg_write(UC_X86_REG_ESI,HOUSE)
            run_checked(u,RAISE_BEGIN,RAISE_END)
            assert u.reg_read(UC_X86_REG_ESP)==f.sp
        elif kind=='attach_cell':
            f.call(0x485250,cell(args[1],args[2]),[tags[args[0]]])
        elif kind=='object':
            obj=SCRATCH+0x90000+len(objects)*0x1000
            u.mem_write(obj,b'\xA5'*0x300)
            f.call(0x5F3900,obj,[])
            objects.append(obj)
            if case.get('mixed_deferred_queue',False):
                allocated.add(obj)
                allocation_types[obj]='object'
        elif kind=='object_alive':
            # Supplied readiness transition exercises the original virtual+44.
            # No claim about the caller that makes this object ready is made.
            u.mem_write(objects[args[0]]+0x90,bytes([args[1]]))
        elif kind=='supply_pending':
            # A declared ordered queue prior admits alive entries and duplicate
            # identities across families. Original725C70 and every destructor
            # execute unchanged; enqueue-caller coverage is a separate claim.
            families={'object':objects,'tag':tags,'trigger':trigger_handles}
            pointers=[families[family][index] for family,index in args[0]]
            assert len(pointers)<=128
            base=REGISTRIES['pending']
            u.mem_write(u32(u,base+4),dwords(*pointers))
            u.mem_write(base+16,dwords(len(pointers)))
        elif kind=='attach_object':
            returned=f.call(0x5F5B50,objects[args[0]],[tags[args[1]] if args[1]>=0 else 0])&255
        elif kind=='dispatch':
            tag_index,event = args[:2]
            xy = args[2] if len(args)>2 else [0,0]
            force = args[3] if len(args)>3 else 0
            obj = objects[args[4]] if len(args)>4 else 0
            returned = f.call(0x6E53A0,tags[tag_index],[event,obj,int.from_bytes(packed(*xy),'little'),force,0])&255
        elif kind=='global' or kind=='local':
            returned = f.call(0x689670 if kind=='global' else 0x689910,SCENARIO,args)&255
        elif kind=='action':
            action_kind,typ = args
            u.mem_write(CONTROL+0x2C,dwords(action_kind))
            u.mem_write(CONTROL+0x50,dwords(TRIGGER_TYPES[typ]))
            returned = f.call(0x6DD8B0,CONTROL,[HOUSE,0,0,0])&255
        elif kind=='mark_tag':
            f.call(0x6E5230,tags[args[0]],[])
        elif kind=='mark_trigger':
            trigger=u32(u,tags[args[0]]+0x28)
            for _ in range(args[1]): trigger=u32(u,trigger+0x28)
            f.call(0x726720,trigger,[])
        elif kind=='drain':
            f.call(0x725C70,0,[])
        elif kind=='register_logic':
            base=REGISTRIES['logic_tags']
            assert len(args)<=128
            u.mem_write(u32(u,base+4),dwords(*(tags[n] for n in args)))
            u.mem_write(base+16,dwords(len(args)))
        elif kind=='register_map':
            base=REGISTRIES['map_tags']
            assert len(args)<=128
            u.mem_write(u32(u,base+4),dwords(*(tags[n] for n in args)))
            u.mem_write(base+16,dwords(len(args)))
        elif kind=='scenario_flags':
            assert len(args)==4
            for offset,value in zip((0x34AA,0x34A9,0x34AB,0x34BE),args):
                u.mem_write(SCENARIO+offset,bytes([value]))
        elif kind=='poll':
            # Run the complete original tag-poll prefix, including live vector
            # mutation and dirty-flag clearing. Stop before unrelated Logic AI.
            u.reg_write(UC_X86_REG_ESP,f.sp)
            u.reg_write(UC_X86_REG_ECX,0x87F778)
            run_checked(u,0x55AFB0,0x55B205)
            assert u.reg_read(UC_X86_REG_ESP)==f.sp-0x38
        else:
            raise ValueError(op)
        history.append(snapshot(returned))
    u.hook_del(hook)
    assert code == [bytes(u.mem_read(a,b-a)) for a,b in ranges]
    result=dict(input=case,history=history,allocations=allocations,calls=calls)
    if house_funds is not None:
        rng_after={pointer:bytes(u.mem_read(pointer,0x3F4)) for pointer in rng_before}
        assert rng_after==rng_before
        assert not draws
        result.update(rng_before={hex(p):hashlib.sha256(v).hexdigest() for p,v in rng_before.items()},
                      rng_after_history={hex(p):hashlib.sha256(v).hexdigest() for p,v in rng_after.items()})
    # This terminal continuation probe is deliberately outside saved history.
    result['next_random']=f.call(0x65C780,SCENARIO+0x218,[])
    return result


def inputs():
    rows = [dict(name='repeat'),dict(name='empty',triggers=[]),
            dict(name='sharing',operations=[['shared'],['shared'],['fresh'],['shared'],['fresh']]),
            dict(name='pending_shared',operations=[['shared'],['mark_tag',0],['shared'],['dispatch',0,13]]),
            dict(name='reverse_chain',triggers=[dict(events=[[47,0]],actions=[[28,7]]),
                                              dict(events=[[47,0]],actions=[[28,8]])])]
    rows += [dict(name=f'mode_{mode}_refs_{refs}',repeat=mode,
                  operations=[['shared']]+[['attach_cell',0,50+n,60] for n in range(refs)]+
                             [['dispatch',0,13,[50,60]],['dispatch',0,13,[51,60]]])
             for mode in (0,1,2,3) for refs in (0,1,2)]
    rows += [dict(name=f'timer_{event}',triggers=[dict(events=[[event,2]],actions=[[28,7]])],
                  operations=[['shared'],['dispatch',0,13],['frame',153],['dispatch',0,13],
                              ['frame',183],['dispatch',0,51]]) for event in (13,51)]
    rows += [dict(name=f'{kind}_resets_{event}',triggers=[dict(events=[[event,7],[51,6]],actions=[])],
                  operations=[['shared'],['fresh'],['frame',170],[kind,7,1],[kind,7,1],[kind,8,1],
                              [kind,7,0],['action',54,0],[kind,7,1],['action',53,0]])
             for kind, event in (('global',27),('global',28),('local',36),('local',37))]
    rows += [dict(name=f'force_enabled_{enabled}',triggers=[dict(flags=[1,1,1,enabled],events=[[47,999]],actions=[[28,7]])],
                  operations=[['shared'],['fresh'],['action',22,0],['dispatch',0,13],
                              ['action',53,0],['action',22,0]]) for enabled in (0,1)]
    rows += [dict(name=f'difficulty_{difficulty}',difficulty=difficulty,
                  triggers=[dict(flags=[1,0,1,1],events=[[51,7]],actions=[[28,7]])],
                  operations=[['shared'],['action',53,0],['action',22,0]]) for difficulty in (-1,0,1,2,3)]
    rows += [dict(name=f'completion_mode_{mode}',repeat=mode,
                  triggers=[dict(events=[[2,0],[47,10]],actions=[[28,7]])],
                  operations=[['shared'],['dispatch',0,2],['frame',150],['dispatch',0,13],
                              ['dispatch',0,13]]) for mode in (0,2)]
    rows += [dict(name=f'timer_boundary_{value}',frame=0x7FFFFFF0,
                  triggers=[dict(events=[[13,value]],actions=[[28,7]])],
                  operations=[['shared'],['dispatch',0,13],['frame',0x80000010],['dispatch',0,13]])
             for value in (-1,0,2,143165577,2147483647)]
    rows += [dict(name='shared_timer_last_event',
                  triggers=[dict(events=[[51,7],[13,4],[51,3]],actions=[[28,7]])]),
             dict(name='force_predicate_resets_timer',triggers=[dict(events=[[51,7]],actions=[[28,7]])],
                  operations=[['shared'],['dispatch',0,13,[0,0],1],['action',22,0]])]
    rows += [dict(name=f'drain_mode_{mode}',repeat=mode,lifecycle=True,
                  operations=[['shared'],['object'],['attach_object',0,0],['attach_cell',0,50,60],
                              ['dispatch',0,13,[0,0],0,0],['dispatch',0,13,[50,60]],['drain'],['shared']])
             for mode in (0,1,2)]
    rows += [dict(name='explicit_tag_removal',lifecycle=True,
                  triggers=[dict(events=[[51,7]]),dict(events=[[51,3]])],
                  operations=[['shared'],['object'],['attach_object',0,0],['mark_tag',0],['mark_tag',0],
                              ['drain'],['shared']]),
             dict(name='trigger_splice',lifecycle=True,triggers=[{},{}],
                  operations=[['shared'],['mark_trigger',0,1],['mark_trigger',0,1],['drain'],
                              ['mark_trigger',0,0],['drain'],['mark_tag',0],['drain']]),
             dict(name='object_reattachment',lifecycle=True,
                  operations=[['shared'],['fresh'],['object'],['attach_object',0,0],
                              ['attach_object',0,0],['attach_object',0,1],['attach_object',0,-1]])]
    rows += [dict(name=f'logic_poll_repeat_{mode}',repeat=mode,polling=True,lifecycle=True,
                  operations=[['shared',0],['shared',1],['shared',2],['register_logic',0,1,2],
                              ['poll'],['poll'],['drain']]) for mode in (0,2)]
    rows += [dict(name='logic_poll_all_flags',polling=True,triggers=[dict(events=[[47,999]])],
                  operations=[['shared'],['register_logic',0],['scenario_flags',1,1,1,1],['poll'],['poll']]),
             dict(name='logic_poll_random_timer',polling=True,triggers=[dict(events=[[51,7]],actions=[[28,7]])],
                  operations=[['shared'],['register_logic',0],['poll'],['frame',400],['poll'],['poll']])]
    return rows


def active_inputs():
    # Actual destructor gates. These supplied small-world controls execute no
    # AllowWin action: its presence is queried by the original Type getter.
    rows=[]
    for live,started in ((0,0),(0,1),(1,0),(1,1)):
        for cursor in (0,2):
            rows.append(dict(name=f'active_cleanup_{live}_{started}_cursor_{cursor}',
                live_flags=[live,started],game_mode=0,active_cleanup=True,polling=True,lifecycle=True,
                logic_cursor=cursor,map_cursor=cursor,
                triggers=[dict(events=[[8,0],[13,999]],actions=[[15,0]])],
                operations=[['shared',0],['shared',1],['shared',2],
                            ['register_logic',0,1,2],['register_map',0,1,2],
                            ['mark_tag',1],['drain']]))
    for difficulty in (0,1,2):
        rows.append(dict(name=f'active_logic_poll_repeat0_difficulty_{difficulty}',
            live_flags=[1,1],game_mode=0,active_cleanup=True,polling=True,lifecycle=True,difficulty=difficulty,
            repeat=0,operations=[['shared',0],['shared',1],['shared',2],
                                 ['register_logic',0,1,2],['poll'],['drain'],['poll'],['drain']]))
    return rows


def mixed_deferred_inputs():
    setup=[['shared',0],['shared',1],['object'],['object'],['object'],
           ['object_alive',1,0],['object_alive',2,0],['mark_tag',0],['mark_trigger',1,0]]
    return [dict(name='mixed_deferred_'+name,mixed_deferred_queue=True,lifecycle=True,
                 operations=setup+[['supply_pending',tickets],['drain'],
                                   ['object_alive',0,0],['drain']]+
                            ([['mark_tag',1],['drain']] if name=='tag_callback_tail' else []))
            for name,tickets in (
                ('tag_callback_tail',[['object',0],['tag',0],['object',1],['tag',0],
                                      ['trigger',1],['object',2],['object',1],['object',0]]),
                ('trigger_before_tag',[['trigger',0],['object',0],['tag',0],['object',1],
                                       ['trigger',0],['tag',1],['trigger',1],['object',1],
                                       ['object',2],['object',0]]))]


def generate():
    rows=[execute(case) for case in inputs()]
    named={r['input']['name']:r for r in rows}
    assert [h['returned'] for h in named['sharing']['history']]==[0,0,1,0,2]
    assert named['reverse_chain']['history'][1]['actions']==[[28,8],[28,7]]
    assert named['mode_1_refs_2']['history'][-2]['globals']==[]
    assert named['mode_1_refs_2']['history'][-1]['globals']==[7]
    assert named['mode_0_refs_2']['history'][-1]['tags'][0]['refs']==0
    assert named['completion_mode_2']['history'][1]['tags'][0]['triggers'][0]['completed']==1
    assert named['completion_mode_2']['history'][-1]['globals']==[7]
    assert named['completion_mode_0']['history'][-1]['globals']==[]
    for mode in (0,1):
        state=named[f'drain_mode_{mode}']['history'][-2]
        assert state['frees']==['trigger','tag'] and state['pending_count']==0
        assert state['object_tags']==[None] and state['registry_counts']['cell_tags']==0
    state=named['explicit_tag_removal']['history'][-2]
    assert state['frees']==['tag','trigger','trigger'] and state['pending_count']==0
    assert named['trigger_splice']['history'][-1]['registry_counts']['abstract']==0
    assert named['logic_poll_repeat_0']['history'][4]['logic_tags']==[1]
    assert named['logic_poll_repeat_0']['history'][4]['dispatches']==[[0,13],[2,27]]
    assert named['logic_poll_repeat_0']['history'][5]['dispatches']==[[0,13],[2,27],[1,13]]
    assert named['logic_poll_repeat_2']['history'][4]['dispatches']==[[0,13],[1,27],[2,27]]
    assert named['logic_poll_all_flags']['history'][3]['dispatches']==[[0,n] for n in (50,27,28,36,37,45,46,13,51)]
    assert named['logic_poll_all_flags']['history'][3]['scenario_flags']==[0,0,0,0]
    for row in rows:
        # All retained instances must have been initialized by the real ctor.
        for state in row['history']:
            for tag in state['tags']:
                if tag is None: continue
                assert tag['pending'] in (0,1)
                assert all(t['pending'] in (0,1) and t['enabled'] in (0,1) for t in tag['triggers'])
    # Preserve the accepted original49 observations independently of additive
    # coverage. This seal came from their original native corpus, not VERA.
    assert len(rows)==49
    legacy=hashlib.sha256(json.dumps(rows,sort_keys=True,separators=(',',':'),
                                   allow_nan=False).encode('utf-8')).hexdigest()
    assert legacy=='59eb67f4543f243db25eeadf7d7a77412e77708d93d6dd1e743893aa6c1d0899',legacy
    rows.extend(execute(case) for case in active_inputs())
    rows.extend(stock_registration(difficulty) for difficulty in (0,1,2))
    rows.append(reference_controls())
    rows.extend(execute(case) for case in mixed_deferred_inputs())
    return rows


def metadata():
    value=provenance(
        entry_points={'find_or_create':0x6E52A0,'tag_constructor':0x6E4DE0,
                      'process_event':0x6E53A0,'trigger_predicate':0x7264C0,'spring':0x7265C0,
                      'reset':0x726400,'execute_action':0x6DD8B0,'object_constructor':0x5F3900,
                      'object_attach':0x5F5B50,'deferred_drain':0x725C70,
                      'object_can_delete':0x5F6690,'object_deleting_destructor':0x5F6DC0,
                      'logic_tag_poll_prefix':0x55AFB0,
                      'tag_destructor':0x6E4F60,'trigger_deleting_destructor':0x726950,
                      'fixed_map_teamtypes':0x6F19B0,'map_triggertypes':0x7275D0,
                      'map_tagtypes':0x6E5ED0,'action_record_reader':0x6DD5B0,
                      'team_tag_field_prefix':0x6F131E,'team_tag_find_or_allocate':0x6E6310,
                      'basic_format_prefix':0x68A13D,'cell_tags_prefix':0x4AD1B3,
                      'fill_classification_prefix':0x684D47,'resolved_enable53':0x6E2AF0},
        assumptions=['Original49: resolved definitions, one registered House and initialized Scenario RNG.',
                     'Registries have spare capacity and native initialized vtables; map cells exist. Lifecycle cases construct real base Objects; no Teams are registered.',
                     'Small-world polling cases supply the Logic registration list and run55AFB0..55B205; the scenario countdown is stopped, excluding its expiry/UI callback. Distinct TagTypes may share TriggerTypes.',
                     'Original49 have false A8E9A0/A8ED5C. Eleven additive GameMode0 controls vary both flags and supplied cursors/AllowWin count, executing destructor cursor/House timer gates and mutable polling at difficulty0/1/2.',
                     'Three stock SOV cases use original Scenario/Country constructors, cached ordered physical INI entries, original fixed-AIMD/map TeamType/TriggerType/TagType readers, minimal authored House/Country bindings and sparse cell storage. Complete Rules/House/terrain/concrete Techno construction is excluded.',
                     'Stock classification runs684D47..684EC1 before the later AllowWin loop. The first55AFB0 poll stops before the first6DD8B0 gameplay action; its incomplete guard/stack state is observed, then its stack is abandoned for separate complete resolved53/predicate calls at explicit supplied frames.',
                     'Stock source chronology skips Script/TaskForce body readers; their referenced Type identities are created by original factories. Physical lexical extraction/cache, process EVA name array, CSF/palette priors and Windows thread/clock imports are supplied by existing owners.',
                     'The observed six stock classification RNG draws are relative to an explicitly seeded31 stream after supplied prerequisites; they are not a complete stock launch draw count or absolute intro firing frame.',
                     'Mixed deferred controls supply the ordered pointer queue, Object+90 readiness values and original Object vector vptr literals from4E7AFD/7252ED. Actual725C70 calls every original CanDelete/scalar deleting destructor; Tag destruction appends actual Trigger pointers to the same live queue.',
                     'NativeRead reference controls demonstrate24-byte ctor type-ID copies, distinct from23-data-byte physical INI ReadName buffers. Original live Tag/Trigger constructors retain classID=-1 and do not consume Scenario IDCreate. A8E9A0 is game state; its low byte and process-startedA8ED5C gate Tag destructor cursor/AllowWin effects.',
                     'Twelve TeamType Tag field controls each supply an empty TagType registry and a real TeamType constructor, then execute6F131E..6F1363 through unchanged ReadString and FindOrAllocate. Full TeamType parsing, preceding House+CC result and teardown of earlier independent definition priors are excluded. Absent/empty reads preserve the actual +D0 pointer; repeated long names may construct distinct24-byte IDs; ReadString capacity128 copies127bytes. All supplied RNG streams remain unchanged.'],
        substitutions=['Operator-new7C8E17 supplies bounded poisoned arena storage;7C8B3D records freeing it. Fresh Tag and Object storage is supplied directly to original constructors.',
                       'Windows IsBadReadPtr succeeds only after Unicorn validates the probed memory. Original CRT typeid and gameplay callees/instructions are unchanged.',
                       'Stock native map query executes on supplied sparse Cells; minimal House storage carries real original Country bindings. No gameplay callable receives a substituted success return.'],
        scope='49 unchanged original lifecycle histories,11 additive active-flag/difficulty cleanup/poll histories,3 bounded stock SOV construction/registration/first-action-boundary histories,44 original TAction reference-reader controls,12 physical TeamType Tag reference controls and2 mixed deferred-queue histories. Establishes native registry source/first-reference order, retained TeamType Tag references and buffer/type-ID boundaries, CellTag refcounts, classification, disabled Event13 reset, explicit-frame resolved53 fanout and event predicates, mutable list/cleanup gates, shared Object/Tag/Trigger readiness/duplicate-removal/destructor-tail order. Mixed controls supply the ordered queue prior and an explicit Object readiness transition; original base Object/Tag/Trigger destruction executes. Excludes concrete object factory/placement and authored Object Tag attachments, complete map/Rules/House prefix, Script/TaskForce body reads, Team lifecycle, gameplay actions after the first stock poll boundary, scenario countdown expiry and later Logic AI.',
    )
    value['reproduction']={
        'write':'python -m tools.spatial_oracle.tag_lifecycle --write',
        'check':'python -m tools.spatial_oracle.tag_lifecycle --check',
        'environment':{
            'VERA20K_GAMEMD_EXE':'selected supported original gamemd.exe',
            'VERA20K_CAMPAIGN_START_ASSETS':'directory containing extracted sov01umd.map',
            'VERA20K_TAG_LIFECYCLE_ASSETS':'directory containing extracted aimd.ini and evamd.ini',
            'RA2_DIR':'retail installation for existing stock CSF/MIX owner'},
        'checkout_prerequisite':'ini/rulesmd.ini is the retained retail RULESMD winner',
        'coverage':{'legacy_histories':49,'active_histories':11,'stock_histories':3,
                    'reference_rows':44,'mixed_deferred_histories':2,'stock_difficulties':[0,1,2]},
        'legacy_payload_sha256':'59eb67f4543f243db25eeadf7d7a77412e77708d93d6dd1e743893aa6c1d0899'}
    return value


def source_paths():
    from tools.input_oracle.campaign_start import source_paths as reader_sources
    paths={f'campaign_{name}':path for name,path in reader_sources().items()}
    paths.update(producer=Path(__file__),legacy_fixture=Path(__file__).with_name('aircraft_fire_location.py'),
                 sparse_cells=Path(__file__).with_name('map_queries.py'),
                 team_numeric_observers=Path(__file__).with_name('team_creation.py'))
    return paths


class PowerEventControls:
    """Selected Event/RegisterEvent inputs on the existing FlagsFixture VM.

    Migrates the two executed ALL01 scratch controls into this lifecycle owner.
    No new VM transport, parser, capture model or gameplay callable substitute.
    Minimal House/Actor storage is explicit; a whole capture/world is excluded.
    """
    SPANS = [(0x71E6A0,0x71E7FA),(0x71E940,0x71F216),(0x49FAE0,0x49FB2F),
             (0x502D30,0x502D60),(0x4FCE30,0x4FCE73),(0x6F9DB0,0x6F9DC7),
             (0x7264C0,0x7265BD),(0x71FA30,0x71FA34)]

    def __init__(self,reader_inputs=None,extra_calls=()):
        from tools.native_oracle import NATIVE_FPCW
        from tools.spatial_oracle.trigger_type_flags import FlagsFixture
        from tools.spatial_oracle.infantry_deploy_rules import HEAP, CURSOR, PTD
        from tools.spatial_oracle.engineer_repair_admission import INF_VTABLE
        from unicorn.x86_const import UC_X86_REG_FPCW
        self.fixture = FlagsFixture()
        self.extra_calls=extra_calls
        self.u = u = self.fixture.u
        self.arena = HEAP
        self.event, self.persist, self.timer = HEAP+0x70000, HEAP+0x70100, HEAP+0x70110
        self.houses = [HEAP+0x10000,HEAP+0x30000,HEAP+0x50000]
        self.types = [HEAP+0x81000+i*0x100 for i in range(3)]
        self.house_array, self.actor = HEAP+0x80000, HEAP+0x85000
        self.source = HEAP+0x88000
        self.counters = [HEAP+0x90000+i*0x1000 for i in range(3)]
        u.reg_write(UC_X86_REG_FPCW,NATIVE_FPCW)
        u.mem_write(PTD,bytes(0x74));u.mem_write(CURSOR,dwords(HEAP+0xA0000))
        for i,(house,typ,counter) in enumerate(zip(self.houses,self.types,self.counters)):
            u.mem_write(house,bytes(0x17000))
            u.mem_write(house+0x30,dwords(20+i));u.mem_write(house+0x34,dwords(typ))
            u.mem_write(typ+0xB8,dwords([0,7,0][i]))
            u.mem_write(house+0x5550,dwords(0x7E449C,counter,512,1,0,10))
            u.mem_write(counter,bytes(512*4))
        u.mem_write(0xA8022C,dwords(self.house_array));u.mem_write(0xA80238,dwords(3))
        u.mem_write(self.house_array,dwords(*self.houses))
        u.mem_write(self.actor,bytes(0x800));u.mem_write(self.actor,dwords(INF_VTABLE))
        u.mem_write(self.actor+0x21C,dwords(self.houses[0]))
        u.mem_write(self.source,bytes(0x800));u.mem_write(self.source,dwords(INF_VTABLE))
        u.mem_write(self.source+0x21C,dwords(self.houses[0]))
        u.mem_write(self.timer,dwords(0,0,30));u.mem_write(0xA8ED84,dwords(123))
        u.mem_write(0xA8ED6B,b'\0')
        for i,base in enumerate((0xB0F1A0,0xB0F658,0xB0F670)):
            u.mem_write(base+4,dwords(HEAP+0x1000+i*0x400,128,0,0,128))
        self.fixture.call(0x71E6A0,[],ecx=self.event)
        self.constructor_defaults = {name:self.integer(self.event+offset)
            for name,offset in [('kind',0x2C),('value',0x34),('last_house',0x54)]}
        self.original = [bytes(u.mem_read(a,b-a)) for a,b in self.SPANS]
        self.calls, self.admission_calls, self.predicates, self.registers = [], [], [], []
        u.hook_add(UC_HOOK_CODE,self.observe)
        defaults=(
            '1,32,0,402','1,32,0,7','1,57,0,7','1,58,0,0','1,58,0,7',
            '1,58,0,999','1,30,0,0','1,1,0,0','1,1,0,7','1,1,0,-1')
        self.readers = {raw:self.read_event(raw) for raw in
                        (defaults if reader_inputs is None else reader_inputs)}

    def integer(self,address):
        return struct.unpack('<i',self.u.mem_read(address,4))[0]

    @staticmethod
    def read_event(raw):
        from tools.spatial_oracle.trigger_event_records import execute
        row = execute(raw)
        assert len(row['conditions']) == 1
        return row['conditions'][0]

    def observe(self,vm,pc,size,data):
        if pc in (0x49FAE0,0x49FB60,0x502D30,0x4FCE30,0x6F9DB0,0x6F9DC0,0x65C780,0x65C7E0)+self.extra_calls:
            sp = vm.reg_read(UC_X86_REG_ESP)
            self.calls.append(dict(pc=f'0x{pc:08X}',ecx=vm.reg_read(UC_X86_REG_ECX),
                arg0=struct.unpack('<I',vm.mem_read(sp+4,4))[0]))

    def observe_admission(self,vm,pc,size,data):
        if pc in (0x71E940,0x71FA30,0x726400):
            self.admission_calls.append(dict(pc=hex(pc),ecx=vm.reg_read(UC_X86_REG_ECX)))

    def predicate(self,name,raw,house_index=0,raised=8,actor_index=None,
                  scenario_init=False,counts=(0,0,0),
                  power=((100,200),(300,200),(0,200)),order=(0,1,2),
                  destruction_counts=None,source_index=None,initial_persistence=0):
        # Preserve historical key scenario_init; it denotes A8ED6B, NOT the
        # A8E7AC placement scope. See this selected mode's metadata.
        if raw not in self.readers:
            self.readers[raw] = self.read_event(raw)
        u,p = self.u,self.readers[raw]
        u.mem_write(self.event+0x2C,dwords(p['kind']));u.mem_write(self.event+0x34,dwords(p['value']))
        u.mem_write(self.event+0x54,dwords(0xABCD1234));u.mem_write(self.persist,bytes([initial_persistence]))
        u.mem_write(self.house_array,dwords(*(self.houses[i] for i in order)))
        u.mem_write(0xA8ED6B,bytes([scenario_init]))
        for counter,value in zip(self.counters,counts):
            if 0 <= p['value'] < 512:u.mem_write(counter+p['value']*4,dwords(value))
        for house,(output,drain) in zip(self.houses,power):u.mem_write(house+0x53A4,dwords(output,drain))
        if destruction_counts is not None:
            # CounterClass49FB60 reads its cached signed total at+10.
            # These are supplied priors, not executions of House writers.
            for house,(buildings,units,infantry,aircraft) in zip(self.houses,destruction_counts):
                u.mem_write(house+0x2F0,dwords(buildings))
                for offset,total in ((0x5564,units),(0x5578,infantry),(0x558C,aircraft)):
                    u.mem_write(house+offset+0x10,dwords(total))
        u.mem_write(self.actor+0x21C,dwords(self.houses[actor_index if actor_index is not None else 0]))
        if source_index is not None:
            u.mem_write(self.source+0x21C,dwords(self.houses[source_index]))
        self.calls = []
        result = self.fixture.call(0x71E940,[raised,self.houses[house_index] if house_index is not None else 0,
            self.actor if actor_index is not None else 0,self.timer,self.persist,
            self.source if source_index is not None else 0],ecx=self.event)&255
        self.predicates.append(dict(name=name,raw=raw,read=p,
            inputs=dict(incoming_house=house_index,raised=raised,actor_owner=actor_index,
                        scenario_init=scenario_init,counts=counts,power=power,house_order=order),
            result=result,persistence=u.mem_read(self.persist,1)[0],
            event54=self.integer(self.event+0x54),calls=list(self.calls)))
        if destruction_counts is not None:
            self.predicates[-1]['inputs']['destruction_counts'] = destruction_counts
        if source_index is not None:
            # Default-null callers retain their established ABI and output
            # shape. The source is separate storage, never the actor alias.
            self.predicates[-1]['inputs']['source_owner'] = source_index

    def register_legacy(self,name,raw,trigger_country,actor_owner,counts,power):
        u,p,arena = self.u,self.readers[raw],self.arena
        typ,live,zero = arena+0x87000,arena+0x87200,arena+0x87400
        u.mem_write(zero,dwords(0));u.mem_write(self.event+0x28,dwords(0,p['kind'],0,p['value']))
        u.mem_write(self.event+0x54,dwords(0));u.mem_write(0xA8ED6B,b'\0')
        for i,t in enumerate(self.types):u.mem_write(t+0xB4,dwords([0,7,0][i]))
        u.mem_write(self.house_array,dwords(*self.houses))
        for counter,value in zip(self.counters,counts):u.mem_write(counter+p['value']*4,dwords(value))
        for house,(output,drain) in zip(self.houses,power):u.mem_write(house+0x53A4,dwords(output,drain))
        u.mem_write(typ,bytes(0xB4));u.mem_write(typ+0xA4,dwords(self.types[0 if trigger_country==0 else 1],0,self.event))
        u.mem_write(live,bytes(0x48));u.mem_write(live+0x24,dwords(typ))
        u.mem_write(live+0x34,dwords(0,0,30));u.mem_write(live+0x44,b'\1')
        u.mem_write(self.actor+0x21C,dwords(self.houses[actor_owner or 0]));self.calls=[]
        result=self.fixture.call(0x7264C0,[1 if p['kind']==1 else 8,
            self.actor if actor_owner is not None else 0,0,0,zero],ecx=live)&255
        self.registers.append(dict(name=name,raw=raw,trigger_country=trigger_country,
            actor_owner=actor_owner,counts=counts,power=power,result=result,
            event54=self.integer(self.event+0x54),live_trigger2c=self.integer(live+0x2C),
            live_trigger_completed=self.integer(live+0x40),calls=list(self.calls)))

    def setup_admission(self,events,trigger_country=0,frame=123,timer_start=0,
                        timer_duration=75,completed=0,count=0):
        u,arena = self.u,self.arena
        typ,live,live2,zero = arena+0x87000,arena+0x87200,arena+0x87300,arena+0x87400
        u.mem_write(zero,dwords(0));u.mem_write(0xA8ED84,dwords(frame));u.mem_write(0xA8ED6B,b'\0')
        u.mem_write(self.house_array,dwords(*self.houses));u.mem_write(typ,bytes(0xB4))
        u.mem_write(typ+0xA4,dwords(self.types[0 if trigger_country==0 else 1],0,self.event))
        for i,raw in enumerate(events):
            p=self.readers[raw];address=self.event+i*0x60
            self.fixture.call(0x71E6A0,[],ecx=address)
            u.mem_write(address+0x28,dwords(self.event+(i+1)*0x60 if i+1<len(events) else 0,p['kind'],0,p['value']))
        for counter in self.counters:u.mem_write(counter+402*4,dwords(0))
        u.mem_write(self.counters[0]+402*4,dwords(count))
        for pointer in (live,live2):
            u.mem_write(pointer,bytes(0x48));u.mem_write(pointer+0x24,dwords(typ))
            u.mem_write(pointer+0x34,dwords(timer_start,0,timer_duration))
            u.mem_write(pointer+0x40,dwords(completed));u.mem_write(pointer+0x44,b'\1')

    def register_admission(self,name,raised,repeat=False,actor=None,second_live=False):
        live=self.arena+(0x87300 if second_live else 0x87200)
        self.calls=[]
        if actor is not None:self.u.mem_write(self.actor+0x21C,dwords(self.houses[actor]))
        prior=dict(timer=[self.integer(live+o) for o in (0x34,0x38,0x3C)],
            completed=self.integer(live+0x40),house=self.integer(live+0x2C),event_house=self.integer(self.event+0x54))
        trace_start=len(self.admission_calls)
        result=self.fixture.call(0x7264C0,[raised,self.actor if actor is not None else 0,0,int(repeat),self.arena+0x87400],ecx=live)&255
        self.registers.append(dict(name=name,raised=raised,repeat=repeat,actor_owner=actor,
            prior=prior,result=result,timer=[self.integer(live+o) for o in (0x34,0x38,0x3C)],
            completed=self.integer(live+0x40),house=self.integer(live+0x2C),
            event_house=self.integer(self.event+0x54),calls=list(self.calls),
            admission_calls=list(self.admission_calls[trace_start:])))

    def assert_unchanged(self):
        assert self.original == [bytes(self.u.mem_read(a,b-a)) for a,b in self.SPANS]
        assert not any(call['pc'] in ('0x0065C780','0x0065C7E0')
            for row in self.predicates+self.registers for call in row['calls'])


def power_event_source_paths():
    return dict(producer=Path(__file__),fixture=Path(__file__).with_name('trigger_type_flags.py'),
        reader=Path(__file__).with_name('trigger_event_records.py'),
        arena=Path(__file__).with_name('infantry_deploy_rules.py'),
        native_transport=Path(__file__).parents[1]/'native_oracle.py')


def generate_destroyed_house_events():
    """Selected9/11 controls through the existing HasOccurred fixture.

    House constructors, counter writers and a physical gameplay world are
    excluded. All original predicates, Country lookup and total getters run.
    """
    from tools.native_oracle import image_sha256, NATIVE_FPCW
    m = PowerEventControls()
    m.SPANS = [*m.SPANS,(0x49FB60,0x49FB64)]
    m.original.append(bytes(m.u.mem_read(0x49FB60,4)))
    timer_before = bytes(m.u.mem_read(m.timer,12))
    for kind in (9,11):
        for totals in ((0,0,0,0),(0,1,0,0),(0,0,1,0),(0,-1,1,0),
                       (0,-2,-3,0),(0,0,0,4),(1,0,0,0),(-1,0,0,0)):
            m.predicate(f'destroyed{kind}_totals'+','.join(map(str,totals)),
                f'1,{kind},0,0',house_index=1,
                destruction_counts=(totals,(8,8,8,8),(0,0,0,0)))
        for value in (-1,7,999):
            m.predicate(f'destroyed{kind}_country{value}',f'1,{kind},0,{value}',
                destruction_counts=((0,0,0,0),(0,1,0,0),(0,0,0,0)))
        m.predicate(f'destroyed{kind}_null_incoming_logic_raise',f'1,{kind},0,0',
            house_index=None,raised=13,scenario_init=True,
            destruction_counts=((0,0,0,0),(8,8,8,8),(0,0,0,0)))
        m.predicate(f'destroyed{kind}_first_duplicate_country',f'1,{kind},0,0',
            order=(2,1,0),destruction_counts=((0,0,0,0),(0,0,0,0),(0,1,0,0)))
    m.assert_unchanged()
    return dict(schema_version=1,native_sha256=image_sha256(),native_fpcw=f'0x{NATIVE_FPCW:X}',
        constructor_defaults=m.constructor_defaults,reader_rows=m.readers,
        predicate_controls=m.predicates,code_unchanged=True,zero_rng_calls=True,
        timer_bytes_unchanged=timer_before==bytes(m.u.mem_read(m.timer,12)),
        code_spans=[dict(start=hex(a),end=hex(b),sha256=hashlib.sha256(code).hexdigest())
            for (a,b),code in zip(m.SPANS,m.original)])


def destroyed_house_event_metadata():
    return provenance(
        scope='26 original HasOccurred9/11 predicate controls, including signed independent counters, tracked Building vs active presence, Country/null/duplicate lookup and raised-event contrasts. Not a full ALL01 world or counter lifecycle comparison.',
        assumptions=[
            'Reuse PowerEventControls minimal three-House arena and original counted TEvent reader. House Country bindings0/7/0, tracked Building scalar and CounterClass cached totals are supplied.',
            'Counter getters49FB60, Country lookup502D30 and HasOccurred71E940 execute unchanged; cached totals are not obtained from executed Add/RemoveTracking or Added/RemovedFromGame.',
            'An unchanged timer and Event+54 sentinel bound mutable-field effects. The ALL01 Apocs_Gone/Enter_Tanya tags and their gameplay actions do not run in these scalar controls.'],
        substitutions=['Existing oracle platform/CRT/TLS transport only; no selected gameplay getter or predicate is replaced.'],
        entry_points={'event_constructor':0x71E6A0,'event_counted_read':0x7274DC,
            'has_occurred':0x71E940,'country_house':0x502D30,'counter_total':0x49FB60})


def generate_damage_source_events():
    """Event44 compares source House+30, independently of actor/Country."""
    from tools.native_oracle import image_sha256, NATIVE_FPCW
    m = PowerEventControls()
    # This selected fixture supplies canonical registered House indices0/1/2.
    # Legacy modes keep their existing20/21/22 priors and null source ABI.
    for index,house in enumerate(m.houses):
        m.u.mem_write(house+0x30,dwords(index))
    timer_before = bytes(m.u.mem_read(m.timer,12))
    for source in range(3):
        for value in (0,1,2,7):
            m.predicate(f'source44_source{source}_value{value}',f'1,44,0,{value}',
                raised=44,actor_index=(source+1)%3,source_index=source)
        m.predicate(f'source44_source{source}_value_minus1','1,44,0,-1',
            raised=44,actor_index=(source+1)%3,source_index=source)
    for value in (0,1,2,7,-1):
        m.predicate(f'source44_null_source_value{value}',f'1,44,0,{value}',
            raised=44,actor_index=0)
    m.predicate('source44_wrong_raise','1,44,0,0',raised=6,
        actor_index=1,source_index=0)
    m.predicate('source44_buildup_gate','1,44,0,0',raised=44,
        actor_index=1,source_index=0,scenario_init=True)
    m.predicate('source44_null_incoming_house','1,44,0,2',raised=44,
        actor_index=0,source_index=2,house_index=None)
    m.predicate('source44_null_actor','1,44,0,1',raised=44,
        source_index=1,actor_index=None)
    m.assert_unchanged()
    return dict(schema_version=1,native_sha256=image_sha256(),
        native_fpcw=f'0x{NATIVE_FPCW:X}',house_indices=[0,1,2],country_indices=[0,7,0],
        houses=m.houses,actor=m.actor,source_object=m.source,
        predicate_controls=m.predicates,code_unchanged=True,
        timer_bytes_unchanged=timer_before==bytes(m.u.mem_read(m.timer,12)),
        code_spans=[dict(start=hex(a),end=hex(b),sha256=hashlib.sha256(code).hexdigest())
            for (a,b),code in zip(m.SPANS,m.original)])


def damage_source_event_metadata():
    return provenance(
        scope='24 unchanged original HasOccurred44 controls through71EE57..71EE7C, source House registered index vs Country/actor, nulls, signed-1, wrong raise and buildup. Not full damage receiver, Tag actions or campaign parity.',
        assumptions=[
            'Reuse PowerEventControls/FlagsFixture original-image/platform transport and original counted TEvent reader. Minimal House storage has supplied+30 registered indices0/1/2 and Country bindings0/7/0.',
            'Actor and source are distinct original-layout Techno objects with copied Infantry vtables and independent+21C House pointers. Event44 first reads source+21C then House+30 directly. After a match the original shared tail also calls Country502D30; observed calls remain in the rows and no lookup/getter return is substituted.',
            'Optional source=None retains the original fixture null ABI and omits any new row field; other selectors retain20/21/22 House priors. Null-source rows deliberately keep an actor present to distinguish the two arguments.',
            'Event+54 sentinel and timer bytes are observed. House/Object constructors, source ownership writers, RegisterEvent, Spring, Object damage callers and a physical world do not execute.'],
        substitutions=['Existing FlagsFixture allocator/CRT/TLS transport and explicit object/House storage priors only. No selected predicate instruction, branch or gameplay getter is replaced.'],
        entry_points={'event_constructor':0x71E6A0,'event_counted_read':0x7274DC,
            'has_occurred':0x71E940,'source_house_index_compare':0x71EE57})


def generate_entered_cell_events():
    """Original1/59 predicates plus the bounded cached-Tag arrival slice."""
    from tools.native_oracle import image_sha256, NATIVE_FPCW
    from tools.spatial_oracle.building_body_rules import SP
    m = PowerEventControls()
    for kind in (1,59):
        for actor in (0,1,2,None):
            m.predicate(f'entered{kind}_actor{actor}',f'1,{kind},0,0',raised=kind,actor_index=actor)
        m.predicate(f'entered{kind}_wildcard',f'1,{kind},0,-1',raised=kind,actor_index=1)
        m.predicate(f'entered{kind}_wrong_raise',f'1,{kind},0,0',raised=59 if kind==1 else 1,actor_index=0)
        m.predicate(f'entered{kind}_buildup',f'1,{kind},0,0',raised=kind,actor_index=0,scenario_init=True)
        m.predicate(f'entered{kind}_null_incoming',f'1,{kind},0,0',raised=kind,actor_index=0,house_index=None)
    m.assert_unchanged()
    u = m.u
    selected_cell,tag = m.arena+0x8A000,m.arena+0x8B000
    rows = []
    active = {}
    def observe(vm,pc,size,data):
        if pc == 0x5657A0 and active:
            sp = vm.reg_read(UC_X86_REG_ESP)
            pointer = struct.unpack('<I',vm.mem_read(sp+4,4))[0]
            active['lookups'].append(list(struct.unpack('<hh',vm.mem_read(pointer,4))))
            vm.reg_write(UC_X86_REG_EAX,selected_cell)
            vm.reg_write(UC_X86_REG_ESP,sp+8)
            vm.reg_write(UC_X86_REG_EIP,struct.unpack('<I',vm.mem_read(sp,4))[0])
        elif pc == 0x6E53A0 and active:
            sp = vm.reg_read(UC_X86_REG_ESP)
            kind,actor,xy,force,source = struct.unpack('<IIIII',vm.mem_read(sp+4,20))
            active['calls'].append(dict(kind=kind,tag=vm.reg_read(UC_X86_REG_ECX),
                actor=actor,cell=list(struct.unpack('<hh',dwords(xy))),force=force,source=source))
            assert vm.reg_read(UC_X86_REG_ECX)==tag
            if kind == 1:
                if active['mutation']=='detach_after1':vm.mem_write(selected_cell+0x3C,dwords(0))
                elif active['mutation']=='move_after1':vm.mem_write(m.actor+0x9C,dwords(8*256,9*256,0))
                elif active['mutation']=='clear_alive_after1':vm.mem_write(m.actor+0x90,b'\0')
            vm.reg_write(UC_X86_REG_EAX,1)
            vm.reg_write(UC_X86_REG_ESP,sp+24)
            vm.reg_write(UC_X86_REG_EIP,struct.unpack('<I',vm.mem_read(sp,4))[0])
    u.hook_add(UC_HOOK_CODE,observe)
    body_before = bytes(u.mem_read(0x4D8978,0x4D8A14-0x4D8978))
    getter_before = bytes(u.mem_read(0x41BEA0,0x2C))
    for name,flags,on_bridge,has_tag,mutation in (
        ('ordinary',0,False,True,'none'),('no_tag',0,False,False,'none'),
        ('bridge_ground',0x100,False,True,'none'),('bridge_on',0x100,True,True,'none'),
        ('alternate_bridge_ground',0x400,False,True,'none'),
        ('cached_after_detach',0,False,True,'detach_after1'),
        ('reread_cell_after_move',0,False,True,'move_after1'),
        ('no_alive_gate_between_raises',0,False,True,'clear_alive_after1')):
        u.mem_write(selected_cell,bytes(0x180))
        u.mem_write(selected_cell+0x140,dwords(flags))
        u.mem_write(selected_cell+0x3C,dwords(tag if has_tag else 0))
        u.mem_write(m.actor+0x9C,dwords(3*256,4*256,0))
        u.mem_write(m.actor+0x8C,bytes([on_bridge]));u.mem_write(m.actor+0x90,b'\1')
        u.reg_write(UC_X86_REG_ESI,m.actor);u.reg_write(UC_X86_REG_ESP,SP)
        active.clear();active.update(mutation=mutation,calls=[],lookups=[])
        stop=run_checked(u,0x4D8978,(0x4D8A14,0x4D8A70),count=1000)
        rows.append(dict(name=name,inputs=dict(flags=flags,on_bridge=on_bridge,tag=has_tag,mutation=mutation),
            calls=list(active['calls']),lookups=list(active['lookups']),stop=hex(stop)))
    active.clear()
    assert body_before==bytes(u.mem_read(0x4D8978,len(body_before)))
    assert getter_before==bytes(u.mem_read(0x41BEA0,len(getter_before)))
    return dict(schema_version=1,native_sha256=image_sha256(),native_fpcw=f'0x{NATIVE_FPCW:X}',
        predicate_controls=m.predicates,arrival_controls=rows,actor=m.actor,tag=tag,houses=m.houses,
        code_unchanged=True,arrival_body_sha256=hashlib.sha256(body_before).hexdigest())


def entered_cell_event_metadata():
    return provenance(
        scope='16 original1/59 HasOccurred predicates and8 original Foot4D8978..4D8A14 arrival-slice controls. Excludes full PerCell, original Tag action bodies, line/zone loops and Fly reachability.',
        assumptions=[
            'Predicate controls reuse the same minimal original House/actor/TEvent-reader fixture as power controls, including duplicate Country identity and null incoming House.',
            'Arrival controls supply original-layout Infantry actor, copied vtable and Cell storage, flags and initial Location. Original virtual GetMapCoords41BEA0 executes.',
            'Map5657A0 is a supplied selected-Cell callout; Tag6E53A0 is a recorded sink. Named callbacks mutate Cell attachment, actor Location or alive; only original caller caching/argument rereads are demonstrated.',
            'The slice begins after range stop and ends before MCV53/54 or Map24/25/26. Native timers/RNG, prefix producers, world load and overall mission progression do not execute.'],
        substitutions=['Existing FlagsFixture transport. Explicit recorded Map pointer and Tag callback sinks; no original GetMapCoords getter or selected caller instruction is replaced.'],
        entry_points={'has_occurred':0x71E940,'entered_special':0x71F1BD,
            'foot_cell_tag_slice':0x4D8978,'map_coords':0x41BEA0,'map_callout':0x5657A0,'tag_callout':0x6E53A0})


def generate_line_events():
    """Original marker writer and Foot line loops on declared sparse cells."""
    from tools.native_oracle import image_sha256
    from tools.spatial_oracle.building_body_rules import SP
    from tools.spatial_oracle.map_queries import TABLE, EMPTY_TABLE
    m = PowerEventControls()
    u, arena = m.u, m.arena
    positions = [(x,y) for y in range(1,5) for x in range(1,5)]
    addresses = {xy: arena+0xB0000+index*0x200 for index,xy in enumerate(positions)}
    original_spans = [(0x568CDB,0x568DCC),(0x4D8A70,0x4D8B6A),
                      (0x6E5300,0x6E5339),(0x6E6250,0x6E62A8),
                      (0x726F50,0x726FA1),(0x5657A0,0x5657D0),(0x41BEA0,0x41BECC)]
    original = [bytes(u.mem_read(a,b-a)) for a,b in original_spans]
    active = {}
    def observe(vm,pc,size,data):
        if pc != 0x6E53A0 or not active:
            return
        sp = vm.reg_read(UC_X86_REG_ESP)
        kind,actor,xy,force,source = struct.unpack('<IIIII',vm.mem_read(sp+4,20))
        active['calls'].append(dict(kind=kind, tag=active['tag_ids'][vm.reg_read(UC_X86_REG_ECX)],
            cell=list(struct.unpack('<hh',dwords(xy))),actor=actor,force=force,source=source))
        if len(active['calls']) == 1:
            if active['mutation'] == 'move_after_first':
                vm.mem_write(m.actor+0x9C,dwords(8*256,9*256,0))
            elif active['mutation'] == 'detach_later':
                vm.mem_write(addresses[(3,3)]+0x3C,dwords(0))
        vm.reg_write(UC_X86_REG_EAX,1)
        vm.reg_write(UC_X86_REG_ESP,sp+24)
        vm.reg_write(UC_X86_REG_EIP,struct.unpack('<I',vm.mem_read(sp,4))[0])
    hook = u.hook_add(UC_HOOK_CODE,observe)
    rows = []
    try:
        for name,definitions,mutation in (
            ('no_markers',[], 'none'),
            ('vertical',[((3,2),[26])], 'none'),
            ('two_vertical_live_order',[((3,2),[26]),((3,3),[26])], 'none'),
            ('linked_vertical',[((3,2),[8,26])], 'none'),
            ('horizontal',[((1,4),[25]),((2,4),[25])], 'none'),
            ('horizontal_precedes_vertical',[((2,4),[25,26])], 'none'),
            ('mixed_last_cell_vertical_gate',[((1,4),[25]),((4,2),[26]),((3,2),[26])], 'none'),
            ('reread_actor_cell',[((3,2),[26]),((3,3),[26])], 'move_after_first'),
            ('reread_later_attachment',[((3,2),[26]),((3,3),[26])], 'detach_later')):
            table = bytearray(EMPTY_TABLE)
            for xy,address in addresses.items():
                struct.pack_into('<I',table,(xy[1]*512+xy[0])*4,address)
                u.mem_write(address,bytes(0x180))
                u.mem_write(address+0x24,packed(*xy))
                u.mem_write(address+0x140,dwords(4))
            u.mem_write(TABLE,bytes(table))
            u.mem_write(0x87F7E8+0x13C,dwords(TABLE,0x40000))
            u.mem_write(0x87F924,dwords(TABLE))
            u.mem_write(0x87F7E8+0x124,dwords(1,1,4,4))
            tags = {}
            for index,(xy,kinds) in enumerate(definitions):
                tag = arena+0xC0000+index*0x400
                tag_type,first = tag+0x100,tag+0x200
                u.mem_write(tag,bytes(0x400))
                u.mem_write(tag+0x24,dwords(tag_type))
                u.mem_write(tag_type+0xA0,dwords(first))
                # Each supplied linked TriggerType has one real-layout Event.
                for node,kind in enumerate(kinds):
                    typ = first+node*0x100
                    event = typ+0xB8
                    u.mem_write(typ+0xA8,dwords(first+(node+1)*0x100 if node+1<len(kinds) else 0,event))
                    u.mem_write(event+0x28,dwords(0,kind))
                u.mem_write(addresses[xy]+0x3C,dwords(tag))
                tags[tag] = index
            # Execute the original line subset once per retained source Cell;
            # line bit outcomes are native output, never hand-calculated.
            for xy in positions:
                address = addresses[xy]
                tag = struct.unpack('<I',u.mem_read(address+0x3C,4))[0]
                if not tag:
                    continue
                u.reg_write(UC_X86_REG_ESI,address)
                u.reg_write(UC_X86_REG_EDI,0x87F7E8)
                u.reg_write(UC_X86_REG_EBP,0x200000)
                u.reg_write(UC_X86_REG_EAX,tag)
                u.reg_write(UC_X86_REG_ESP,SP)
                run_checked(u,0x568CDB,0x568DCC,count=5000)
            marked = [m.integer(addresses[xy]+0x140) for xy in positions]
            u.mem_write(m.actor+0x9C,dwords(3*256,4*256,0))
            u.reg_write(UC_X86_REG_ESI,m.actor)
            u.reg_write(UC_X86_REG_EBP,addresses[(3,4)])
            u.reg_write(UC_X86_REG_EBX,4)
            u.reg_write(UC_X86_REG_ESP,SP)
            u.mem_write(SP+0x1C,dwords(3))
            u.mem_write(SP+0x14,dwords(addresses[(3,4)]))
            active.clear();active.update(tag_ids=tags,calls=[],mutation=mutation)
            run_checked(u,0x4D8A70,0x4D8B6A,count=5000)
            rows.append(dict(name=name,input=dict(cells=positions,actor_cell=[3,4],
                definitions=[dict(cell=xy,events=kinds) for xy,kinds in definitions],mutation=mutation),
                line_flags=marked,calls=list(active['calls'])))
            active.clear()
    finally:
        u.hook_del(hook)
    assert original == [bytes(u.mem_read(a,b-a)) for a,b in original_spans]
    return dict(schema_version=1,native_sha256=image_sha256(),controls=rows,
        original_code_unchanged=True,
        code_spans=[dict(start=hex(a),end=hex(b),sha256=hashlib.sha256(code).hexdigest())
                    for (a,b),code in zip(original_spans,original)])


def line_event_metadata():
    return provenance(
        scope='Nine original Map568CDB line-marker subsets and Foot4D8A70 horizontal/vertical scanning controls on declared4x4 cells. Not full InitCellAttributes, original Tag bodies, Fly ingress or a complete campaign.',
        assumptions=[
            'Reuse PowerEventControls/FlagsFixture original-image/platform transport, actual Infantry vtable and GetMapCoords41BEA0. Native Map5657A0 packed lookup executes on the supplied512-stride sparse pointer table.',
            'Minimal Tag/TagType/linked TriggerType/Event node priors expose original6E5300/6E5320/6E6250/6E6280/726F50/726F80 queries. No enabled/difficulty or event predicate substitution is introduced.',
            'Source Cell coordinates and initial flag4 are supplied. The writer begins after terrain-animation latch clearing and ends before tiberium/Recalc; source iteration is explicitly declared. Final line bits are original output.',
            'Arrival begins after direct1/59 and MCV interlude. Initial actor cell3,4, source Cell and saved stack/register priors match that slice. Each Tag request is recorded; named sinks move the actor or clear a later Cell attachment.',
            'No original Tag actions, full PerCell prefix/tail, zone24, rendering, native RNG/timer schedule or ordinary Fly reachability is claimed.'],
        substitutions=['Recorded Tag6E53A0 sink returns1 and supplies only the named synchronous mutations. Existing platform/CRT transport remains; marker writer, classifiers, Map lookup, GetMapCoords and caller loops execute unchanged.'],
        entry_points={'line_marker':0x568CDB,'foot_line_slice':0x4D8A70,
            'vertical_getter':0x6E5300,'horizontal_getter':0x6E5320,
            'packed_map_lookup':0x5657A0,'map_coords':0x41BEA0,'tag_callout':0x6E53A0})


def funds_event_history(kind,repeat,with_elapsed=False):
    """Compose existing original Fill, Logic, House and lifecycle owners."""
    if with_elapsed:
        initial,threshold=1501,1500
        steps=[(0,None),(3744,['spend_money',1]),(3750,None),(3752,None),
               (7504,['add_credits',1]),(7512,['spend_money',1])]
    elif kind==52:
        initial,threshold=70000,10000
        steps=[(0,None),(7,['spend_money',60000]),(8,None),
               (16,['add_credits',1]),(24,['spend_money',1]),(32,None)]
    else:
        initial,threshold=9999,10000
        steps=[(0,None),(7,['add_credits',1]),(8,None),
               (16,['spend_money',1]),(24,['add_credits',1]),(32,None)]
    operations=[['shared'],['classify_house_tags',0]]
    row_indices=[]
    for frame,mutation in steps:
        operations.append(['frame',frame])
        if mutation:operations.append(mutation)
        if with_elapsed:operations.append(['poll'])
        operations.append(['house_poll'])
        row_indices.append(len(operations)-1)
    if repeat==0:
        operations.extend([['drain'],['frame',40],['house_poll']])
    case=dict(name=f'funds{kind}_repeat{repeat}'+('_elapsed13' if with_elapsed else ''),
              repeat=repeat,frame=0,game_mode=0,lifecycle=True,active_cleanup=True,
              live_flags=[1,1],house_funds=initial,
              triggers=[dict(events=[[kind,threshold]]+([[13,250]] if with_elapsed else []),
                             actions=[[28,7]])],operations=operations)
    receipt=execute(case)
    rows=[]
    for (frame,mutation),index in zip(steps,row_indices):
        state=receipt['history'][index]
        tag=state['tags'][0]
        trigger=tag['triggers'][0]
        rows.append(dict(frame=frame,mutation=mutation,actions=len(state['actions']),
            **state['funds'],tag_pending=tag['pending'],trigger_pending=trigger['pending'],
            timer_start=trigger['start'],timer_duration=trigger['duration'],
            completed=trigger['completed'],pending_count=state['pending_count']))
    final=receipt['history'][-1]
    cleanup=dict(**final['funds'],pending_count=final['pending_count'],
                 free_order=final['frees'],tags_remaining=sum(t is not None for t in final['tags']))
    return dict(name=case['name'],kind=kind,repeat=repeat,initial=initial,threshold=threshold,
                with_elapsed=with_elapsed,steps=rows,cleanup=cleanup,
                rng_before=receipt['rng_before'],rng_after_history=receipt['rng_after_history'])


def generate_funds_events():
    """Signed funds predicates and five original production-caller histories."""
    from tools.native_oracle import image_sha256, NATIVE_FPCW
    m=PowerEventControls(reader_inputs=(),extra_calls=(0x4F6990,0x6C9600,0x7C5F00))
    u=m.u
    for house,typ in zip(m.houses,m.types):
        u.mem_write(house+0x24,dwords(0x7EA834))
        u.mem_write(typ+0x148,struct.pack('<f',1.0))
    # The existing FlagsFixture owns this arena/transport. Supply one Scenario
    # state for three genuine original RNG constructors; no draw is simulated.
    scenario=m.arena+0xB0000
    u.mem_write(0xA8B230,dwords(scenario))
    streams=(scenario+0x218,0x886B88,0xABE890)
    for pointer in streams:m.fixture.call(0x65C6D0,[31],ecx=pointer)
    rng_before={p:bytes(u.mem_read(p,0x3F4)) for p in streams}
    spans=[(0x4F6990,0x4F69CA),(0x6C9600,0x6C964F),(0x7C5F00,0x7C5F38),
           (0x71F950,0x71FA40)]
    code=[bytes(u.mem_read(a,b-a)) for a,b in spans]
    predicates,classifiers=[],[]

    def probe(name,kind,balance,threshold,house=0,raised=13,buildup=False,persistence=0):
        for pointer in m.houses:u.mem_write(pointer+0x30C,dwords(0))
        if house is not None:u.mem_write(m.houses[house]+0x30C,dwords(balance))
        before=list(struct.unpack('<iii',u.mem_read(m.timer,12)))
        m.predicate(name,f'1,{kind},0,{threshold}',house_index=house,raised=raised,
                    actor_index=2,scenario_init=buildup,initial_persistence=persistence)
        row=m.predicates[-1]
        after=list(struct.unpack('<iii',u.mem_read(m.timer,12)))
        assert after==before and row['persistence']==persistence
        assert row['event54']==struct.unpack('<i',dwords(0xABCD1234))[0]
        predicates.append(dict(name=name,kind=kind,balance=balance,threshold=threshold,
            incoming_house=house,raised=raised,scenario_init=buildup,
            initial_persistence=persistence,result=row['result'],persistence=row['persistence'],
            event54=row['event54'],timer_before=before,timer_after=after,
            calls=[call['pc'] for call in row['calls']]))

    pairs=[(699,700),(700,700),(701,700),(-51,-50),(-50,-50),(-49,-50),(0,0),
           (2147483647,-1),(-2147483648,0),(-2147483648,2147483647),
           (2147483647,-2147483648)]
    for kind in (12,52):
        for index,(balance,threshold) in enumerate(pairs):
            probe(f'funds{kind}_signed{index}',kind,balance,threshold)
        for flag in (False,True):
            probe(f'funds{kind}_wrong_raise_buildup{int(flag)}',kind,700,700,
                  raised=99,buildup=flag)
        probe(f'funds{kind}_null_house',kind,0,0,house=None)
        probe(f'funds{kind}_retained_persistence',kind,10000,10000,persistence=1)
        probe(f'funds{kind}_incoming_not_country',kind,8,7,house=1)
        u.mem_write(m.event+0x2C,dwords(kind))
        classifiers.append(dict(kind=kind,
            state_a=m.fixture.call(0x71F950,[],ecx=m.event)&255,
            state_b=m.fixture.call(0x71F9C0,[],ecx=m.event)&255))
    m.assert_unchanged()
    assert code==[bytes(u.mem_read(a,b-a)) for a,b in spans]
    rng_after={p:bytes(u.mem_read(p,0x3F4)) for p in streams}
    assert rng_before==rng_after
    histories=[funds_event_history(kind,repeat) for kind in (12,52) for repeat in (0,2)]
    histories.append(funds_event_history(52,2,with_elapsed=True))
    return dict(schema_version=1,native_sha256=image_sha256(),native_fpcw=f'0x{NATIVE_FPCW:X}',
        predicate_controls=predicates,classifiers=classifiers,histories=histories,
        code_unchanged=True,
        predicate_rng_before={hex(p):hashlib.sha256(v).hexdigest() for p,v in rng_before.items()},
        predicate_rng_after={hex(p):hashlib.sha256(v).hexdigest() for p,v in rng_after.items()})


def funds_event_metadata():
    return provenance(
        scope='32 original funds12/52 predicate controls, four StateA/StateB calls, four pure House-poll histories and one mixed funds52/elapsed13 Logic-before-House history. Not whole economy, world, campaign or nonempty-silo parity.',
        assumptions=[
            'PowerEventControls and execute reuse their existing native-image/platform owners. The selected scalar mode omits unrelated preloaded power readers; actual counted TEvent Read, HasOccurred71E940, AvailableMoney4F6990, Storage6C9600 and ftol7C5F00 execute unchanged.',
            'Minimal House+2FC storage is zero and HouseType IncomeMult is1.0; signed wallet+30C is the supplied balance. Each scalar control clears all wallets then sets only its selected incoming House. Country bindings0/7/0 and a distinct Country0 actor distinguish wallet identity from threshold and actor. Neither nonempty storage nor FillSilos initialization is claimed.',
            'Histories supply one resolved TagType/TriggerType/event/action chain and run real Tag/Trigger constructors. Original Fill684D47..684EC1 registers the actual Tag in House and, for mixed13, Logic vectors. Original House constructor listener append4F5F99..4F5FB1 and actual primary vtable PointerExpired4FB9B0 admit native Tag detach notifications. No complete House or physical map construction is claimed.',
            'Original AddCredits4F9950/SpendMoney4F9790 cross thresholds. House4F8F87..4F8FBE executes original Tag6E53A0/Register7264C0/predicate/Spring/Action28 SetGlobal7. Pure events are nonlatching and repeat2 refires at each still-true eighth-frame poll. Repeat0 queues Trigger and zero-ref Tag and immediately removes House membership through the original notification path. Later real725C70 frees trigger then tag and clears the pending queue; a final House poll sees no tag.',
            'The mixed native event order52/1500 then13/250 models the ALL04 counted-reader linked order. Original Logic55AFB0..55B205 runs before the selected House slice; intervening world/House AI is excluded. It fires at non-eighth frame3750 and rearms the original elapsed timer; later successful7512 rearms again. This is a composed caller comparison, not a complete-frame schedule.',
            'Three genuine RNG constructors seed31. Complete0x3F4-byte Scenario/Main/Map states are compared unchanged across predicates and each history. The reused execute owner draws one terminal Scenario continuation only after the recorded history; it is outside these equality checks. Funds predicates write no timer, Event+54, persistence or detach. RegisterEvent may call ResetTimers on repeat2; actual timers and deferred pending/free order remain recorded.',
            'Event12/52 thresholds are literal signed INI values, with inclusive >=/<=. Both accept unrelated raised kind and A8ED6B true, and null incoming House succeeds. Boundaries include INT_MIN/MAX, equality, negatives, persistence initially1 and a nonzero incoming House whose Country matches the threshold; retained Event+54 sentinel and timer remain unchanged.'],
        substitutions=['Existing allocator/CRT/TLS and validated Windows pointer-probe transports only. Minimal storage, vector capacities and resolved definition priors are declared. No selected gameplay predicate, money getter/mutation, Tag action, classification, poll or cleanup body is replaced.'],
        entry_points={'has_occurred':0x71E940,'funds12_compare':0x71EF01,'funds52_compare':0x71EF20,
            'available_money':0x4F6990,'storage_total':0x6C9600,'ftol':0x7C5F00,
            'state_a':0x71F950,'state_b':0x71F9C0,'classify':0x684D47,
            'house_tag_listener':0x4F5F99,'house_poll':0x4F8F87,'logic_poll':0x55AFB0,
            'add_credits':0x4F9950,'spend_money':0x4F9790,'register_event':0x7264C0,
            'action_execute':0x6DD8B0,'deferred_drain':0x725C70,'house_pointer_expired':0x4FB9B0})


if __name__=='__main__':
    import sys
    args=sys.argv[1:]
    if '--funds-events' in args:
        args.remove('--funds-events')
        finish_vectors(generate_funds_events,Path(__file__).with_name('trigger_funds_events.json'),
                       provenance=funds_event_metadata,source_paths=power_event_source_paths(),argv=args)
    elif '--damage-source-events' in args:
        args.remove('--damage-source-events')
        finish_vectors(generate_damage_source_events,Path(__file__).with_name('trigger_damage_source_events.json'),
                       provenance=damage_source_event_metadata,source_paths=power_event_source_paths(),argv=args)
    elif '--line-events' in args:
        args.remove('--line-events')
        finish_vectors(generate_line_events,Path(__file__).with_name('trigger_line_events.json'),
                       provenance=line_event_metadata,source_paths=power_event_source_paths(),argv=args)
    elif '--entered-cell-events' in args:
        args.remove('--entered-cell-events')
        finish_vectors(generate_entered_cell_events,Path(__file__).with_name('trigger_entered_cell_events.json'),
                       provenance=entered_cell_event_metadata,source_paths=power_event_source_paths(),argv=args)
    elif '--destroyed-house-events' in args:
        args.remove('--destroyed-house-events')
        finish_vectors(generate_destroyed_house_events,Path(__file__).with_name('trigger_destroyed_house_events.json'),
                       provenance=destroyed_house_event_metadata,source_paths=power_event_source_paths(),argv=args)
    else:
        finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata,
                       source_paths=source_paths())
