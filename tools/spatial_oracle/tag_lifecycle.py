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


def initialize_registries(u, storage, *, capacity=128, include_object_vtables=False):
    """Supply empty registry storage and install original initializer vtables.

    Callers select the registries they own. In a joined world, omit existing
    Object/Techno registries so live constructor registrations are preserved.
    No Tag construction, attachment, event decision or lifecycle is supplied.
    """
    for name, items in storage.items():
        u.mem_write(REGISTRIES[name] + 4,
                    dwords(items, capacity, 0, 0, capacity))
    for name, vt in (('cell_tags', 0x7E3890), ('map_tags', 0x7EA5A4),
                     ('logic_tags', 0x7EA5A4)):
        if name in storage:
            u.mem_write(REGISTRIES[name], dwords(vt))
    writers = [('abstract', 0x72536D), ('tag_trigger_base', 0x72546D),
               ('tag', 0x6E4D7D), ('trigger', 0x4E65FD),
               ('object_base', 0x7253ED), ('pending', 0x72586D)]
    if include_object_vtables:
        writers += [('object', 0x4E7AFD), ('object_base_b', 0x7252ED)]
    for name, writer in writers:
        if name in storage:
            base = REGISTRIES[name]
            instruction = bytes(u.mem_read(writer, 10))
            assert instruction[:6] == b'\xC7\x05' + dwords(base)
            u.mem_write(base, instruction[6:])


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
    if 'new_ini_format' in case:
        u.mem_write(0xA8ED7C,dwords(case['new_ini_format']))
    u.mem_write(0xA8ED6B, b'\0')
    for base,value in zip((0xA8E9A0,0xA8ED5C),case.get('live_flags',[0,0])):
        u.mem_write(base,bytes([value]))
    u.mem_write(SCENARIO+0x60C, dwords(case.get('difficulty',1)))
    u.mem_write(SCENARIO+0x11E8, dwords(-1))
    scenario_timer = case.get('scenario_timer',False)
    if scenario_timer:
        # Original Mouse constructor5BDA4A installs this primary vtable.
        # Action27's real Tab6D0F00 redraw then reaches4F42F0 through +38;
        # both redraw bodies execute, without a supplied callback result.
        instruction=bytes(u.mem_read(0x5BDA4A,6))
        assert instruction[:2]==b'\xC7\x06'
        u.mem_write(0x87F7E8,instruction[2:])
    initialize_registries(u,
        {name: SCRATCH+0x50000+index*0x400 for index, name in enumerate(REGISTRIES)},
        include_object_vtables=case.get('mixed_deferred_queue', False))
    u.mem_write(0xA8022C, dwords(SCRATCH+0x4F000))
    u.mem_write(0xA80238, dwords(1))
    u.mem_write(SCRATCH+0x4F000, dwords(HOUSE))
    u.mem_write(HOUSE+0x34, dwords(HOUSE_TYPE))
    u.mem_write(HOUSE_TYPE+0xB4, dwords(0,0))
    spy = spy_type = None
    spy_dispatch_states = []
    if case.get('spy_actor',False):
        # Explicit original-layout actor/type priors. The shared lifecycle
        # owner still executes real Tag/Trigger construction and dispatch;
        # these fields do not claim an Infantry/Type/House constructor.
        from tools.spatial_oracle.engineer_repair_admission import INF_VTABLE
        spy,spy_type=SCRATCH+0xC0000,SCRATCH+0xC2000
        u.mem_write(spy,bytes(0x800));u.mem_write(spy,dwords(INF_VTABLE))
        u.mem_write(spy+0x21C,dwords(HOUSE))
        u.mem_write(spy+0x518,dwords(spy_type,HOUSE))
        u.mem_write(spy+0x6C0,dwords(spy_type))
        u.mem_write(spy+0x90,b'\1')
        u.mem_write(spy+0x9C,dwords(50*256,60*256,0))
        u.mem_write(spy_type,bytes(0xED0));u.mem_write(spy_type,dwords(0x7EB610))
        u.mem_write(spy_type+0xDF8,dwords(5));u.mem_write(spy_type+0xEC4,b'\1')
        u.mem_write(0xA83D4C,dwords(HOUSE));u.mem_write(HOUSE+0x30,dwords(20))
        if 'periodic_height' in case:
            from tools.spatial_oracle.object_flight_height import initialize_object_scalars
            initialize_object_scalars(u,lambda address,ecx,args:f.call(address,ecx,list(args)))
            u.mem_write(spy+0x74,b'\1')
            u.mem_write(spy+0x8C,b'\0')
            u.mem_write(spy+0xA4,dwords(case['periodic_height']))
    house_funds = case.get('house_funds')
    observe_object_cleanup = case.get('observe_object_cleanup',False)
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
    if observe_object_cleanup and house_funds is None:
        for pointer in (0x886B88,0xABE890):
            f.call(0x65C6D0,pointer,[case.get('seed',31)])
    if scenario_timer and not observe_object_cleanup and house_funds is None:
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
    object_frees, cleanup_calls, cleanup_observations = [], [], []
    pointer_probe=RET_MAGIC+0x100
    u.mem_write(0x7E115C,dwords(pointer_probe))
    def ticket(pointer):
        if pointer in tags:return ['tag',tags.index(pointer)]
        if pointer in trigger_handles:return ['trigger',trigger_handles.index(pointer)]
        if pointer in objects:return ['object',objects.index(pointer)]
        raise AssertionError(('unknown pending identity',hex(pointer)))
    def observe(_u, pc, _size, _data):
        nonlocal cursor
        if observe_object_cleanup and pc in (0x5F3B80,0x5F6DC0,0x5F5B50,0x725C70,
                                             0x6E5230,0x6E4F60,0x726950,0x65C780,0x65C7E0):
            receiver=u.reg_read(UC_X86_REG_ECX)
            cleanup_calls.append(dict(pc=f'0x{pc:08X}',
                object=objects.index(receiver) if receiver in objects else None))
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
            if observe_object_cleanup and pointer in objects:
                attachment=u32(u,pointer+0x34)
                object_frees.append(dict(object=objects.index(pointer),
                    tag_at_free=tags.index(attachment) if attachment else None))
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
    if scenario_timer:
        ranges.extend(((0x426630,0x42664C),(0x6D0F00,0x6D0F0F),
                       (0x4F42F0,0x4F431E),(0x578AC0,0x578AC7)))
        timer_rng_before={pointer:bytes(u.mem_read(pointer,0x3F4))
                          for pointer in (SCENARIO+0x218,0x886B88,0xABE890)}
    if spy is not None:
        ranges.extend(((0x522640,0x5226BB),(0x5226C0,0x522705),
                       (0x4F9A50,0x4F9AB0),(0x524D60,0x524D67),
                       (0x502D30,0x502D61)))
    if 'periodic_height' in case:
        ranges.extend(((0x4DA7B0,0x4DA806),(0x5F6960,0x5F69B7),
                       (0x41BEA0,0x41BEDD),(0x5F6B90,0x5F6BB2),
                       (0x5F5F40,0x5F5F92)))
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
    if observe_object_cleanup:
        from types import SimpleNamespace
        from tools.input_oracle.campaign_start import CampaignReader
        cleanup_rng_view=SimpleNamespace(u=u,scenario=SCENARIO)
        cleanup_rng_before=CampaignReader.rng_state(cleanup_rng_view)
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
        if observe_object_cleanup:
            # The free recorder retains the allocation bytes. Unlike the
            # historical object_tags view, this observes +34 even after free;
            # a host-filtered None must not be mistaken for a native clear.
            observed=dict(object_tag_words=[tags.index(u32(u,obj+0x34))
                if u32(u,obj+0x34) else None for obj in objects],
                object_frees=list(object_frees),native_cleanup_calls=list(cleanup_calls))
            assert CampaignReader.rng_state(cleanup_rng_view)==cleanup_rng_before
            observed['three_full_rng_states_compared_equal']=True
            cleanup_observations.append(observed)
        if case.get('polling',False):
            base=REGISTRIES['logic_tags']
            state.update(dispatches=list(dispatches),
                         logic_tags=[tags.index(u32(u,u32(u,base+4)+n*4)) for n in range(i32(u,base+16))],
                         scenario_flags=[u.mem_read(SCENARIO+n,1)[0] for n in (0x34AA,0x34A9,0x34AB,0x34BE)])
        if scenario_timer:
            remaining=f.call(0x426630,SCENARIO+0x11E8,[])
            state['scenario_timer']=dict(running=i32(u,SCENARIO+0x11E8)!=-1,
                                        remaining=struct.unpack('<i',dwords(remaining))[0])
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
        elif kind in ('dispatch','dispatch_spy'):
            tag_index,event = args[:2]
            xy = args[2] if len(args)>2 else [0,0]
            force = args[3] if len(args)>3 else 0
            obj = spy if kind=='dispatch_spy' else objects[args[4]] if len(args)>4 else 0
            if kind=='dispatch_spy':assert spy is not None
            returned = f.call(0x6E53A0,tags[tag_index],[event,obj,int.from_bytes(packed(*xy),'little'),force,0])&255
            if kind=='dispatch_spy':
                spy_dispatch_states.append(dict(kind=event,tag=tags[tag_index],
                    cell_tag=u32(u,cell(*xy)+0x3C),tag_pending=u.mem_read(tags[tag_index]+0x34,1)[0]))
        elif kind=='periodic_cell':
            assert spy is not None and 'periodic_height' in case
            u.reg_write(UC_X86_REG_ESI,spy)
            u.reg_write(UC_X86_REG_EBX,0)
            u.reg_write(UC_X86_REG_ESP,f.sp)
            run_checked(u,0x4DA7B0,0x4DA806,count=20000)
            spy_dispatch_states.append(dict(kind=59,tag=tags[0],
                cell_tag=u32(u,cell(50,60)+0x3C),tag_pending=u.mem_read(tags[0]+0x34,1)[0]))
        elif kind=='global' or kind=='local':
            returned = f.call(0x689670 if kind=='global' else 0x689910,SCENARIO,args)&255
        elif kind=='action':
            action_kind,typ = args[:2]
            u.mem_write(CONTROL+0x2C,dwords(action_kind))
            u.mem_write(CONTROL+0x50,dwords(TRIGGER_TYPES[typ]))
            if len(args)>2:
                u.mem_write(CONTROL+0x90,dwords(args[2]))
            returned = f.call(0x6DD8B0,CONTROL,[HOUSE,0,0,0])&255
        elif kind=='mark_tag':
            f.call(0x6E5230,tags[args[0]],[])
        elif kind=='mark_trigger':
            trigger=u32(u,tags[args[0]]+0x28)
            for _ in range(args[1]): trigger=u32(u,trigger+0x28)
            f.call(0x726720,trigger,[])
        elif kind=='drain':
            f.call(0x725C70,0,[])
        elif kind=='dispose_object':
            # Constructor-complete base Object scalar disposal, not a live
            # Techno death or an invented Tag release before the destructor.
            assert observe_object_cleanup and case.get('mixed_deferred_queue',False)
            f.call(0x5F6DC0,objects[args[0]],[1])
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
    if spy is not None:
        result.update(native_dispatch_states=spy_dispatch_states,code_unchanged=True)
    if observe_object_cleanup:
        result.update(full_rng_before=cleanup_rng_before,
            full_rng_after_history=CampaignReader.rng_state(cleanup_rng_view),
            object_tag_raw_history=cleanup_observations,object_frees=list(object_frees))
    if house_funds is not None:
        rng_after={pointer:bytes(u.mem_read(pointer,0x3F4)) for pointer in rng_before}
        assert rng_after==rng_before
        assert not draws
        result.update(rng_before={hex(p):hashlib.sha256(v).hexdigest() for p,v in rng_before.items()},
                      rng_after_history={hex(p):hashlib.sha256(v).hexdigest() for p,v in rng_after.items()})
    if scenario_timer:
        timer_rng_after={pointer:bytes(u.mem_read(pointer,0x3F4)) for pointer in timer_rng_before}
        assert timer_rng_after==timer_rng_before and not draws
        result.update(rng_before={hex(p):hashlib.sha256(v).hexdigest() for p,v in timer_rng_before.items()},
                      rng_after_history={hex(p):hashlib.sha256(v).hexdigest() for p,v in timer_rng_after.items()})
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


def scenario_timer_inputs():
    """Complete Logic countdown polling with real Tag/action callbacks.

    The caller-supplied Action kind/value fields match execute's existing
    definition transport. Original Action27 starts the timer before any poll;
    nothing resets or substitutes its state between the explicit frames.
    """
    rows=[]
    for name,tags,events,actions in (
            ('before_exact_after',1,[[14,0]],[[28,7]]),
            ('two_tags',2,[[14,0]],[[28,7]]),
            ('restart_on_expiry',2,[[14,0]],[[27,4]]),
            ('explicit14',1,[[47,100]],[[28,7]])):
        operations=[['action',27,0,2]]
        operations.extend(['shared',index] for index in range(tags))
        operations.append(['register_logic',*range(tags)])
        for frame in (29,30,31):
            operations.extend((['frame',frame],['poll']))
        rows.append(dict(name='scenario_timer_'+name,scenario_timer=True,
                         repeat=2,polling=True,frame=0,game_mode=0,
                         triggers=[dict(events=events,actions=actions)],operations=operations))
    return rows


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
    rows.extend(execute(case) for case in scenario_timer_inputs())
    return rows


def metadata():
    value=provenance(
        entry_points={'find_or_create':0x6E52A0,'tag_constructor':0x6E4DE0,
                      'process_event':0x6E53A0,'trigger_predicate':0x7264C0,'spring':0x7265C0,
                      'reset':0x726400,'execute_action':0x6DD8B0,'object_constructor':0x5F3900,
                      'object_attach':0x5F5B50,'deferred_drain':0x725C70,
                      'object_can_delete':0x5F6690,'object_deleting_destructor':0x5F6DC0,
                      'logic_tag_poll_prefix':0x55AFB0,
                      'scenario_timer_expiry':0x55B17D,'timer_remaining':0x426630,
                      'timer_action_redraw':0x6D0F00,'redraw':0x4F42F0,
                      'tag_destructor':0x6E4F60,'trigger_deleting_destructor':0x726950,
                      'fixed_map_teamtypes':0x6F19B0,'map_triggertypes':0x7275D0,
                      'map_tagtypes':0x6E5ED0,'action_record_reader':0x6DD5B0,
                      'team_tag_field_prefix':0x6F131E,'team_tag_find_or_allocate':0x6E6310,
                      'basic_format_prefix':0x68A13D,'cell_tags_prefix':0x4AD1B3,
                      'fill_classification_prefix':0x684D47,'resolved_enable53':0x6E2AF0},
        assumptions=['Original49: resolved definitions, one registered House and initialized Scenario RNG.',
                     'Registries have spare capacity and native initialized vtables; map cells exist. Lifecycle cases construct real base Objects; no Teams are registered.',
                     'Small-world polling cases supply the Logic registration list and run55AFB0..55B205; the scenario countdown is stopped, excluding its expiry/UI callback. Distinct TagTypes may share TriggerTypes.',
                     'Four scenario_timer controls additionally execute Action27 before polling frames29/30/31, starting duration30 at frame0. They retain real Tag/Trigger constructors, predicates, callbacks, mutable Logic traversal, explicit14 after unsuccessful13/51 and post-loop expiry55B17D..55B1D8. Two repeat2 Tags may share one Type; a callback Action27/4 restarts the clock at frame30 and the tail preserves the new duration60. No timer state is supplied between polls. These cases do not establish whole-SOV01 timing, repeat0 traversal behavior or rendered pixels.',
                     'scenario_timer controls supply only the original Mouse primary vptr from constructor instruction5BDA4A so Action27 Tab6D0F00 can call original4F42F0 through slot+38. The allocated screen fields and absent Tactical pointer remain fixture priors; the real redraw flag/level and epoch stores execute. Full Mouse/Scenario construction and display drawing are excluded. Their history scenario_timer fields observe original426630 remaining and the native start!=-1 display gate; all three RNG buffers stay unchanged through the histories.',
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
        scope='49 unchanged original lifecycle histories,11 additive active-flag/difficulty cleanup/poll histories,3 bounded stock SOV construction/registration/first-action-boundary histories,44 original TAction reference-reader controls,12 physical TeamType Tag reference controls,2 mixed deferred-queue histories and4 Scenario countdown completion histories. Establishes native registry source/first-reference order, retained TeamType Tag references and buffer/type-ID boundaries, CellTag refcounts, classification, disabled Event13 reset, explicit-frame resolved53 fanout and event predicates, mutable list/cleanup gates, shared Object/Tag/Trigger readiness/duplicate-removal/destructor-tail order. Countdown controls establish real Action27 initialization, post-loop expiry, explicit14 fallback and callback restart retention. Mixed controls supply the ordered queue prior and an explicit Object readiness transition; original base Object/Tag/Trigger destruction executes. Excludes concrete object factory/placement and authored Object Tag attachments, complete map/Rules/House prefix, Script/TaskForce body reads, Team lifecycle, gameplay actions after the first stock poll boundary and later Logic AI.',
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
                    'reference_rows':44,'mixed_deferred_histories':2,'scenario_timer_histories':4,
                    'stock_difficulties':[0,1,2]},
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
    from tools.native_oracle import NATIVE_FPCW
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
    return dict(schema_version=1,native_fpcw=f'0x{NATIVE_FPCW:X}',
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


def generate_building_loss_events():
    """Event10 uses the selected Country's signed tracked-building scalar."""
    from tools.native_oracle import NATIVE_FPCW
    m=PowerEventControls()
    m.SPANS=[*m.SPANS,(0x71F950,0x71FA40)]
    m.original.append(bytes(m.u.mem_read(0x71F950,0xF0)))
    controls=[]

    def probe(name,buildings,value=0,house=0,actor=None,raised=99,
              order=(0,1,2),buildup=False,persistence=0):
        before=list(struct.unpack('<iii',m.u.mem_read(m.timer,12)))
        m.predicate(name,f'1,10,0,{value}',house_index=house,actor_index=actor,
                    raised=raised,order=order,scenario_init=buildup,
                    destruction_counts=tuple((count,0,0,0) for count in buildings),
                    initial_persistence=persistence)
        row=m.predicates[-1]
        after=list(struct.unpack('<iii',m.u.mem_read(m.timer,12)))
        assert after==before and row['persistence']==persistence
        assert row['event54']==struct.unpack('<i',dwords(0xABCD1234))[0]
        controls.append(dict(name=name,value=value,tracked_buildings=list(buildings),
            house_order=list(order),incoming_house=house,actor_owner=actor,raised=raised,
            scenario_init=buildup,initial_persistence=persistence,result=row['result'],
            persistence=row['persistence'],event54=row['event54'],
            timer_before=before,timer_after=after))

    for count in (-2147483648,-1,0,1,2147483647):
        probe(f'tracked_buildings_{count}',(count,5,7),raised=8)
    for order in ((0,1,2),(2,1,0)):
        probe(f'first_country_order_{order}',(0,0,7),house=1,actor=1,order=order)
    for value in (-1,999):
        probe(f'no_country_{value}',(7,5,9),value=value)
    for incoming in (0,None):
        for buildup in (False,True):
            probe(f'input_house_{incoming}_bypass_{buildup}',(0,5,7),
                  house=incoming,buildup=buildup)
    probe('retained_persistence_one',(0,5,7),house=1,raised=8,persistence=1)
    classification=dict(state_a=m.fixture.call(0x71F950,[],ecx=m.event)&255,
                        state_b=m.fixture.call(0x71F9C0,[],ecx=m.event)&255)
    m.assert_unchanged()
    return dict(schema_version=1,native_fpcw=f'0x{NATIVE_FPCW:X}',
                country_indices=[0,7,0],controls=controls,classification=classification,
                code_unchanged=True,zero_rng_calls=True)


def building_loss_event_metadata():
    return provenance(
        scope='Fourteen original HasOccurred10 controls and two StateA/StateB calls. Covers the tracked-building predicate and first-Country resolution, not a complete House destruction or campaign mechanism.',
        assumptions=[
            'Reuse PowerEventControls/FlagsFixture native image, cached counted TEvent reader and minimal three-House arena. Country bindings0/7/0, registered order, signed House+2F0 tracked-building counts and actor/incoming House identities are supplied priors. No House or Building constructor, tracking writer, Limbo, UnInit or destructor runs here.',
            'Original HasOccurred71E940 executes Country lookup502D30 with Event+34 and Event10 branch71F115..71F12E. It tests the first matched House tracked+2F0 <=0 signed; absent Country returns true. The declared INT_MIN/-1/0/1/INT_MAX and reversed duplicate-Country controls distinguish signed scalar and registration-order semantics.',
            'Wrong raised99, distinct actor/incoming House1, null incoming House and both A8ED6B values are executed. Event+54 starts0xABCD1234 and persistence starts0 or1; both and the supplied timer remain unchanged. StateA71F950 returns0 and StateB71F9C0 returns1. Existing observers see neither RNG draw entry; this predicate has no timer write or detach.',
            'The corpus deliberately omits unrelated power reader rows and active/per-type counters. Existing house_tracking.py evidence owns Add/RemoveTracking behavior; existing house_tag_raise.py and tag_lifecycle.py evidence own polling, registration and cleanup. These scalar controls do not extend their declared coverage or certify a whole ALL04 rescue sequence.'],
        substitutions=['Existing FlagsFixture allocator/CRT/TLS transports and explicit House/actor storage priors only. Original counted TEvent reader, predicate, Country lookup and StateA/StateB instructions run unchanged; no selected gameplay return is supplied.'],
        entry_points={'event_constructor':0x71E6A0,'event_counted_read':0x7274DC,
            'has_occurred':0x71E940,'country_house':0x502D30,'building_compare':0x71F115,
            'state_a':0x71F950,'state_b':0x71F9C0})


def generate_damage_source_events():
    """Event44 compares source House+30, independently of actor/Country."""
    from tools.native_oracle import NATIVE_FPCW
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
    return dict(schema_version=1,
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


def generate_entered_cell_events(*, spy=False):
    """Original1/59 predicates and shared optional53/54 cached-Tag slice."""
    from tools.native_oracle import NATIVE_FPCW
    from tools.spatial_oracle.building_body_rules import SP
    m = PowerEventControls(reader_inputs=() if spy else None)
    for kind in (() if spy else (1,59)):
        for actor in (0,1,2,None):
            m.predicate(f'entered{kind}_actor{actor}',f'1,{kind},0,0',raised=kind,actor_index=actor)
        m.predicate(f'entered{kind}_wildcard',f'1,{kind},0,-1',raised=kind,actor_index=1)
        m.predicate(f'entered{kind}_wrong_raise',f'1,{kind},0,0',raised=59 if kind==1 else 1,actor_index=0)
        m.predicate(f'entered{kind}_buildup',f'1,{kind},0,0',raised=kind,actor_index=0,scenario_init=True)
        m.predicate(f'entered{kind}_null_incoming',f'1,{kind},0,0',raised=kind,actor_index=0,house_index=None)
    m.assert_unchanged()
    u = m.u
    selected_cell,tag = m.arena+0x8A000,m.arena+0x8B000
    agent_type,other_type=m.arena+0x8C000,m.arena+0x8D000
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
            if spy:
                if kind==59 and active['mutation']=='enable_agent_after59':
                    vm.mem_write(agent_type+0xEC4,b'\1')
                if kind==53:
                    if active['mutation']=='clear_agent_after53':vm.mem_write(agent_type+0xEC4,b'\0')
                    elif active['mutation']=='replace_type_after53':vm.mem_write(m.actor+0x6C0,dwords(other_type))
                    elif active['mutation']=='clear_alive_after53':vm.mem_write(m.actor+0x90,b'\0')
                    elif active['mutation']=='move_after53':vm.mem_write(m.actor+0x9C,dwords(8*256,9*256,0))
                    elif active['mutation']=='detach_after53':vm.mem_write(selected_cell+0x3C,dwords(0))
            vm.reg_write(UC_X86_REG_EAX,1)
            vm.reg_write(UC_X86_REG_ESP,sp+24)
            vm.reg_write(UC_X86_REG_EIP,struct.unpack('<I',vm.mem_read(sp,4))[0])
    u.hook_add(UC_HOOK_CODE,observe)
    end=0x4D8A70 if spy else 0x4D8A14
    body_before = bytes(u.mem_read(0x4D8978,end-0x4D8978))
    getter_before = bytes(u.mem_read(0x41BEA0,0x2C))
    cases=(
        ('ordinary',0,False,True,'none'),('no_tag',0,False,False,'none'),
        ('bridge_ground',0x100,False,True,'none'),('bridge_on',0x100,True,True,'none'),
        ('alternate_bridge_ground',0x400,False,True,'none'),
        ('cached_after_detach',0,False,True,'detach_after1'),
        ('reread_cell_after_move',0,False,True,'move_after1'),
        ('no_alive_gate_between_raises',0,False,True,'clear_alive_after1'))
    if spy:
        cases=[(mutation,0,False,True,mutation) for mutation in (
            'none','non_agent','enable_agent_after59','clear_agent_after53',
            'replace_type_after53','clear_alive_after53','move_after53','detach_after53')]
    for name,flags,on_bridge,has_tag,mutation in cases:
        u.mem_write(selected_cell,bytes(0x180))
        u.mem_write(selected_cell+0x140,dwords(flags))
        u.mem_write(selected_cell+0x3C,dwords(tag if has_tag else 0))
        u.mem_write(m.actor+0x9C,dwords(3*256,4*256,0))
        u.mem_write(m.actor+0x8C,bytes([on_bridge]));u.mem_write(m.actor+0x90,b'\1')
        if spy:
            u.mem_write(agent_type,bytes(0xED0));u.mem_write(other_type,bytes(0xED0))
            u.mem_write(agent_type+0xEC4,bytes([mutation not in ('non_agent','enable_agent_after59')]))
            u.mem_write(m.actor+0x6C0,dwords(agent_type))
        u.reg_write(UC_X86_REG_ESI,m.actor);u.reg_write(UC_X86_REG_ESP,SP)
        active.clear();active.update(mutation=mutation,calls=[],lookups=[])
        stop=run_checked(u,0x4D8978,(end,0x4D8A70),count=1000)
        rows.append(dict(name=name,inputs=dict(flags=flags,on_bridge=on_bridge,tag=has_tag,mutation=mutation),
            calls=list(active['calls']),lookups=list(active['lookups']),stop=hex(stop)))
    active.clear()
    assert body_before==bytes(u.mem_read(0x4D8978,len(body_before)))
    assert getter_before==bytes(u.mem_read(0x41BEA0,len(getter_before)))
    return dict(schema_version=1,native_fpcw=f'0x{NATIVE_FPCW:X}',
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


def generate_spy_entry_events():
    """Original53 identity predicates, cached53/54 caller and linked lifetime.

    Infantry PerCell4D8A14 checks Agent once. HasOccurred71ECF5 resolves
    Country502D30 and forced DisplayOwner5226C0, not the actor's real House.
    The existing lifecycle owner independently runs real53/54 Tag actions.
    """
    from tools.native_oracle import NATIVE_FPCW
    m=PowerEventControls(reader_inputs=(),extra_calls=(0x5226C0,0x4F9A50))
    spans=((0x5226C0,0x522705),(0x4F9A50,0x4F9AB0),(0x71F950,0x71FA40))
    m.SPANS=[*m.SPANS,*spans]
    m.original.extend(bytes(m.u.mem_read(a,b-a)) for a,b in spans)
    # value, worn House, PlayerPtr; supplied House Country bindings are0/7/0.
    cases=[
        ('worn_first_country0',0,0,0,{}),('worn_other_country7',7,1,0,{}),
        ('wrong_worn_house',0,1,0,{}),('same_country_different_house',0,2,0,{}),
        ('duplicate_country_reordered',0,2,0,dict(order=(2,1,0))),
        ('fallback_player',7,None,1,{}),('fallback_player_wrong_duplicate',0,None,2,{}),
        ('forced_ignores_allied_owner',7,1,0,dict(disguised=True)),
        ('forced_ignores_raw_disguised',7,1,0,{}),
        ('null_display_house',0,None,None,{}),('minus_one_not_wildcard',-1,0,0,{}),
        ('unregistered_country',999,0,0,{}),('wrong_raise',0,0,0,dict(raised=54)),
        ('map_editor_gate',0,0,0,dict(scenario_init=True)),
        ('null_actor',0,0,0,dict(actor_index=None)),
        ('initial_persistence_survives_mismatch',0,1,0,dict(initial_persistence=1)),
        ('incoming_house_irrelevant',0,0,0,dict(house_index=1))]
    for name,value,worn,player,options in cases:
        options=dict(options);disguised=options.pop('disguised',False)
        m.u.mem_write(m.actor+0x51C,dwords(m.houses[worn] if worn is not None else 0))
        m.u.mem_write(0xA83D4C,dwords(m.houses[player] if player is not None else 0))
        m.u.mem_write(m.actor+0x1D8,bytes([disguised]))
        before=bytes(m.u.mem_read(m.timer,12))
        m.predicate(name,f'1,53,0,{value}',**dict(
            dict(house_index=None,raised=53,actor_index=0),**options))
        row=m.predicates[-1]
        row.update(display_house=worn,current_player=player,raw_disguised=disguised,
            initial_persistence=options.get('initial_persistence',0),
            forced_getter_calls=[dict(pc=call['pc'],forced=call['arg0'])
                for call in row['calls'] if call['pc']=='0x005226C0'],
            timer_unchanged=before==bytes(m.u.mem_read(m.timer,12)))
        assert row['timer_unchanged'] and row['event54']==struct.unpack('<i',dwords(0xABCD1234))[0]
    classification=[]
    for kind in (53,54):
        m.u.mem_write(m.event+0x2C,dwords(kind))
        classification.append(dict(kind=kind,state_a=m.fixture.call(0x71F950,[],ecx=m.event)&255,
                                   state_b=m.fixture.call(0x71F9C0,[],ecx=m.event)&255))
    m.assert_unchanged()
    arrivals=generate_entered_cell_events(spy=True)
    linked=execute(dict(name='repeat0_linked_spy53_then54_same_cached_tag',repeat=0,
        lifecycle=True,spy_actor=True,
        triggers=[dict(events=[[53,0]],actions=[[56,7]]),
                  dict(events=[[54,5]],actions=[[56,8]])],
        operations=[['shared'],['attach_cell',0,50,60],
                    ['dispatch_spy',0,53,[50,60]],['dispatch_spy',0,54,[50,60]],
                    ['dispatch_spy',0,53,[50,60]]]))
    linked['checkpoints']=dict(after53=2,after54=3,after53again=4)
    assert linked['history'][2]['locals']==[7] and linked['history'][3]['locals']==[7,8]
    assert linked['history'][4]['actions']==linked['history'][3]['actions']
    assert all(row['cell_tag']==0 and row['tag_pending']==0 for row in linked['native_dispatch_states'])
    return dict(schema_version=1,native_fpcw=f'0x{NATIVE_FPCW:X}',
        predicate_controls=m.predicates,classification=classification,
        arrival_controls=arrivals['arrival_controls'],linked_history=linked,
        actor=m.actor,tag=arrivals['tag'],houses=m.houses,code_unchanged=True)


def spy_entry_event_metadata():
    return provenance(
        scope='17 unchanged original53 predicates,53/54 StateA/B,8 Foot4D8978..4D8A70 caller-slice controls and one real repeat0 linked53/54 Tag/action history. Not full PerCell, Spy construction/death, a retail-authored53 map or whole-game parity.',
        assumptions=[
            'PowerEventControls/FlagsFixture executes the original counted TEvent reader, HasOccurred71E940, first-Country House502D30, forced Infantry DisplayOwner5226C0 and IsAllied4F9A50. Supplied House+30 identities20/21/22 and Country bindings0/7/0 distinguish House identity from Country identity; their constructors and ownership/disguise writers do not execute.',
            '53 requires matching raise, actor, value!=-1 and first matching registered Country House. GetDisplayOwner(1) reads retained worn House even with raw Disguised false, otherwise PlayerPtr; it is called twice for nonnull results. Match sets caller persistence1; failure preserves it. Event+54 sentinel and timer bytes stay unchanged. This leaf contains no RNG or detach; no general zero-timer/RNG/detach claim extends to Tag actions.',
            'Caller controls reuse the entered-cell slice owner. Real GetMapCoords41BEA0 and4D8A14 Agent/Infantry admission execute; supplied Map5657A0 and recorded Tag6E53A0 sinks apply named changes. After53, clearing Agent/Alive, replacing Type or detaching CellTag does not suppress54; moving changes54 Location. These sinks establish caller rereads, not the behavior of actual mutation actions.',
            'The independent linked history reuses execute: original Tag/Trigger constructors, real53/54 predicates and Infantry forced identity/type/index getters, Register/Spring, Action56 SetLocal, ReplaceTag, pointer-expiry and deferred enqueue execute. Resolved definitions and an unregistered original-layout Spy/Type/House are supplied priors. The same caller-supplied Tag receives53 then54 then53; it is not a Foot execution or a physical Infantry constructor.',
            'With repeat0,53 sets local7 and marks only its Trigger pending. CellTag is immediately0 and Tag refs0 but Tag.pending stays0, so the cached Tag still dispatches54 to the other Trigger, setting local8. Both Triggers then pending; a further53 executes no action. Queue counts2 then4 include the duplicated Tag ticket; no queue drain, TAction70 immediate free/UAF or general destruction parity is claimed. Existing next_random is a terminal continuation probe outside history.'],
        substitutions=['Existing FlagsFixture/execute platform allocation/CRT transport and explicit actor/type/House/resolved-definition priors. Arrival slice only: recorded Map and Tag callout sinks with named field mutations. No selected predicate, forced getter, Tag lifecycle instruction, action or RNG return is replaced.'],
        entry_points=dict(has_occurred=0x71E940,spy_house_compare=0x71ECF5,
            spy_type_compare=0x71ED59,house_country=0x502D30,display_owner=0x5226C0,
            disguise_type=0x522640,infantry_type_index=0x524D60,house_allied=0x4F9A50,
            foot_slice=0x4D8978,agent_admission=0x4D8A14,raise53=0x4D8A4B,raise54=0x4D8A6B,
            map_coords=0x41BEA0,tag_process=0x6E53A0,register=0x7264C0,spring=0x7265C0,
            trigger_destroy=0x726720,pointer_expiry=0x7258D0,set_local=0x689910))


def spy_entry_event_source_paths():
    return dict(power_event_source_paths(),
        lifecycle_fixture=Path(__file__).with_name('aircraft_fire_location.py'),
        infantry_layout=Path(__file__).with_name('engineer_repair_admission.py'))


def generate_periodic_cell_events():
    """Original periodic caller/getters and composed live Tag histories.

    Shared platform/memory/CRT and Tag lifecycle remain the existing owners.
    Flight, physical location and type/Rules identities are supplied priors;
    no actor construction, AI prefix or retail rendering is executed here.
    """
    from tools.spatial_oracle.aircraft_fire_location import OWNER, TYPE
    from tools.spatial_oracle.object_flight_height import initialize_object_scalars
    f=Fixture(dict(game_mode=5));u=f.u
    initialize_object_scalars(u,lambda address,ecx,args:f.call(address,ecx,list(args)))
    u.mem_write(0xA8ED7C,dwords(4))
    rules,rocket,tag=SCRATCH+0x40000,SCRATCH+0x44000,SCRATCH+0x50000
    u.mem_write(rules,bytes(0x2000));u.mem_write(0x8871E0,dwords(rules))
    f.call(0x661EC0,rocket,[])
    cases=[]
    for cls,vt in (('unit',0x7F5C70),('infantry',0x7EB058),('aircraft',0x7E22A4)):
        for frame,z,marked,tagged in ((15,500,1,1),(16,207,1,1),(16,208,1,1),
                                       (16,500,0,1),(16,500,1,0),(32,500,1,1)):
            cases.append(dict(name=f'{cls}_f{frame}_z{z}_mark{marked}_tag{tagged}',
                              cls=cls,vt=vt,frame=frame,z=z,marked=marked,tagged=tagged))
    def unit(name,**kw):
        return dict(dict(name=name,cls='unit',vt=0x7F5C70,frame=16,z=208,marked=1,tagged=1),**kw)
    cases += [unit(f'signed_frame{frame}',frame=frame,z=500) for frame in (0,-16,-1,-2147483648)]
    cases += [unit('bridge_flags500_no_onbridge_high',cellflags=0x500),
              unit('onbridge_relative207',z=623,on_bridge=1),
              unit('onbridge_relative208',z=624,on_bridge=1),
              unit('physical_world_alias',xy=[526*256+128,11*256+128],cell_xy=[14,12]),
              unit('health0_alive0_limbo1_slice_only',z=500,health=0,alive=0,limbo=1)]
    for slot in (0x4E0,0x514):
        for phase in (2,3,5,6):
            cases.append(dict(name=f'rocket_rule{slot:x}_phase{phase}',cls='aircraft',
                vt=0x7E22A4,frame=16,z=0,marked=0,tagged=1,rocket_rule=slot,rocket_phase=phase))
    spans=((0x4DA7B0,0x4DA806),(0x4DE620,0x4DE625),(0x5F6B90,0x5F6BB2),
           (0x41B920,0x41B973),(0x5F6960,0x5F69B7),(0x41BEA0,0x41BEDD),
           (0x5F5F40,0x5F5F92),(0x661F90,0x661FA0))
    original=[bytes(u.mem_read(a,b-a)) for a,b in spans]
    watch={0x4DE620,0x5F6B90,0x41B920,0x5F5F40,0x578080,
           0x5F6960,0x565730,0x41BEA0,0x661F90,0x6E53A0}
    calls=[]
    def observe(uc,pc,size,data):
        if pc in watch:calls.append(hex(pc))
        assert pc not in (0x65C780,0x65C7E0,0x65C837),('unexpected RNG',hex(pc))
    hook=u.hook_add(UC_HOOK_CODE,observe)
    caller=[]
    for case in cases:
        x,y=case.get('xy',[15*256+128,15*256+128]);cx,cy=case.get('cell_xy',[15,15])
        for ptr in (cell(15,15),cell(14,12)):
            u.mem_write(ptr+0x3C,dwords(0));u.mem_write(ptr+0x140,dwords(0))
        u.mem_write(cell(cx,cy)+0x3C,dwords(tag if case['tagged'] else 0))
        u.mem_write(cell(cx,cy)+0x140,dwords(case.get('cellflags',0)))
        u.mem_write(OWNER,dwords(case['vt']));u.mem_write(OWNER+0x9C,dwords(x,y,case['z']))
        for off,value in ((0x74,case['marked']),(0x8C,case.get('on_bridge',0)),
                          (0x90,case.get('alive',1)),(0x81,case.get('limbo',0))):
            u.mem_write(OWNER+off,bytes([value]))
        u.mem_write(OWNER+0x6C,dwords(case.get('health',100)))
        u.mem_write(rules+0x4E0,dwords(0));u.mem_write(rules+0x514,dwords(0))
        if 'rocket_rule' in case:
            u.mem_write(rules+case['rocket_rule'],dwords(TYPE))
            u.mem_write(OWNER+0x674,dwords(rocket+4));u.mem_write(rocket+0x40,dwords(case['rocket_phase']))
        u.mem_write(0xA8ED84,dwords(case['frame']))
        u.reg_write(UC_X86_REG_ESI,OWNER);u.reg_write(UC_X86_REG_EBX,0);u.reg_write(UC_X86_REG_ESP,f.sp)
        calls.clear()
        stop=run_checked(u,0x4DA7B0,(0x4DA806,0x6E53A0),count=2000)
        row=dict(input=case,raised=stop==0x6E53A0,calls=list(calls))
        if row['raised']:
            sp=u.reg_read(UC_X86_REG_ESP)
            kind,actor,xy,force,source=struct.unpack('<5I',u.mem_read(sp+4,20))
            row['dispatch']=dict(kind=kind,cell=list(struct.unpack('<2h',dwords(xy))),force=force,source=source)
        caller.append(row)
    u.hook_del(hook)
    assert original==[bytes(u.mem_read(a,b-a)) for a,b in spans]
    histories=[]
    for name,frame,height,repeat,events,operations in (
        ('periodic_f15_z208_repeat0',15,208,0,[[59,-1]],[['periodic_cell'],['drain']]),
        ('periodic_f16_z207_repeat0',16,207,0,[[59,-1]],[['periodic_cell'],['drain']]),
        ('periodic_f16_z208_repeat0',16,208,0,[[59,-1]],[['periodic_cell'],['drain']]),
        ('periodic_f16_z208_repeat2',16,208,2,[[59,-1]],[['periodic_cell'],['drain']]),
        ('repeat2_same_binary_frame',16,208,2,[[59,-1]],
         [['periodic_cell'],['periodic_cell'],['frame',17],['periodic_cell'],['frame',32],['periodic_cell']]),
        ('repeat0_detach_before_second_query',16,208,0,[[59,-1]],
         [['periodic_cell'],['periodic_cell'],['drain']]),
        ('tag_without59_can_match_level47',16,208,2,[[47,0]],[['periodic_cell']]),
        ('entered1_is_not_raised',16,208,2,[[1,-1]],[['periodic_cell']]),
    ):
        result=execute(dict(name=name,frame=frame,repeat=repeat,periodic_height=height,
            game_mode=5,new_ini_format=4,lifecycle=True,polling=True,spy_actor=True,
            observe_object_cleanup=True,triggers=[dict(events=events,actions=[[28,7]])],
            operations=[['shared'],['attach_cell',0,50,60],*operations]))
        histories.append(result)
    return dict(schema_version=1,session_mode=5,new_ini_format=4,
                caller_controls=caller,tag_histories=histories,code_unchanged=True)


def periodic_cell_event_metadata():
    return provenance(
        scope='35 original Foot4DA7B0..4DA806/getter controls and eight complete periodic-to-Tag/action/cleanup histories. Not full AI, natural flight or rendered retail parity.',
        assumptions=[
            'Actual Unit/Infantry/Aircraft vtables, Object scalar initializers, GetHeight, GetOccupiedCell double lookup, GetMapCoords and designated Rocket IsMovingNow execute. Actor/type/Rules/terrain and marked/height/phase are explicit supplied priors. The 35 caller controls stop at Tag entry.',
            'Eight histories reuse the existing execute lifecycle owner with its original-layout Infantry prior and resolved definitions. Original periodic caller, Tag/Trigger constructors, Register/HasOccurred59,47,1/Spring, Action28 global7, immediate detach and deferred deletion execute. Full three-stream state is compared before/after each saved history; next_random is a terminal probe outside history.',
            'SessionMode A8B238 is explicitly5 (Skirmish); NewINIFormat A8ED7C is independently4. No existing control default is changed.',
            'Frame15 and relative207 reject; marked relative208 admits. Repeated same-frame visits dispatch again for repeat2; repeat0 immediately clears Cell+3C and queues the actual Trigger/Tag before drain. A Tag containing47 but no59 still runs;1 alone does not match. No selected action writes a countdown timer; ResetTimers is still called for a successful59 persistence flag.'],
        substitutions=['Existing Fixture/execute allocation and CRT transport; supplied actor/world/flight priors. No selected caller/getter, Tag lifecycle, event predicate, action or RNG return is replaced.'],
        entry_points=dict(foot_periodic=0x4DA7B0,high_flying=0x5F6B90,aircraft_high=0x41B920,
            occupied_cell=0x5F6960,map_coords=0x41BEA0,tag=0x6E53A0,
            register=0x7264C0,reset_timers=0x726400,action=0x6DD8B0,deferred_drain=0x725C70))


def periodic_cell_event_source_paths():
    return dict(spy_entry_event_source_paths(),
        object_scalars=Path(__file__).with_name('object_flight_height.py'))


def generate_line_events():
    """Original marker writer and Foot line loops on declared sparse cells."""
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
    return dict(schema_version=1,controls=rows,
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
    from tools.native_oracle import NATIVE_FPCW
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
    return dict(schema_version=1,native_fpcw=f'0x{NATIVE_FPCW:X}',
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


def generate_loss_threshold_events():
    """Original15/16 compare the incoming House's signed loss counters."""
    from tools.native_oracle import NATIVE_FPCW
    from tools.sidebar_oracle.stock import mix, mix_hash
    from tools.rules_oracle.bridge_child_sound import sections
    m=PowerEventControls(reader_inputs=())
    m.SPANS=[*m.SPANS,(0x71F950,0x71FA40)]
    m.original.append(bytes(m.u.mem_read(0x71F950,0xF0)))
    controls,classifiers=[],[]

    def probe(kind,name,value,counters,house=0,raised=8,buildup=False,persistence=0):
        for pointer,(buildings,units) in zip(m.houses,counters):
            m.u.mem_write(pointer+0x5488,dwords(buildings))
            m.u.mem_write(pointer+0x5434,dwords(units))
        before=list(struct.unpack('<iii',m.u.mem_read(m.timer,12)))
        m.predicate(f'loss{kind}_{name}',f'1,{kind},0,{value}',house_index=house,
                    raised=raised,scenario_init=buildup,initial_persistence=persistence)
        row=m.predicates[-1]
        after=list(struct.unpack('<iii',m.u.mem_read(m.timer,12)))
        assert before==after and row['persistence']==persistence
        assert row['event54']==struct.unpack('<i',dwords(0xABCD1234))[0]
        controls.append(dict(name=row['name'],kind=kind,value=value,loss_counters=counters,
            incoming_house=house,raised=raised,scenario_init=buildup,
            initial_persistence=persistence,result=row['result'],persistence=row['persistence'],
            event54=row['event54'],timer_before=before,timer_after=after))

    for kind in (15,16):
        probe(kind,'all_houses_zero_losses_threshold0',0,((0,0),(0,0),(0,0)))
        for count,value in ((0,0),(0,1),(5,6),(6,6),(7,6),(-1,-1),(-1,0),
                            (-2147483648,0),(2147483647,-1),(-2147483648,-2147483648)):
            pair=(count,1000) if kind==15 else (1000,count)
            probe(kind,f'counter{count}_threshold{value}',value,(pair,(100,100),(0,0)))
        pair=(6,1000) if kind==15 else (1000,6)
        probe(kind,'threshold_is_not_country',7,(pair,(100,100),(0,0)))
        probe(kind,'incoming_house1',6,((0,0),(6,6),(0,0)),house=1)
        probe(kind,'null_incoming',2147483647,((0,0),(0,0),(0,0)),house=None,raised=99)
        for buildup in (False,True):
            probe(kind,f'wrong_raise_bypass{buildup}',6,((6,6),(0,0),(0,0)),
                  raised=99,buildup=buildup)
        probe(kind,'retained_persistence_one',6,((6,6),(0,0),(0,0)),persistence=1)
        classifiers.append(dict(kind=kind,state_a=m.fixture.call(0x71F950,[],ecx=m.event)&255,
                                state_b=m.fixture.call(0x71F9C0,[],ecx=m.event)&255))
    m.assert_unchanged()

    # Reuse the retail MIX and lexical cache owners. Only the counted TEvent
    # reader is executed here; Team/TaskForce/Script sections remain raw data.
    archive=(Path(os.environ['RA2_DIR'])/'mapsmd03.mix').read_bytes()
    raw_map=mix(archive)[mix_hash('ALL04DMD.MAP')]
    definitions=sections(raw_map)
    routes=[]
    for trigger_id,tag_id,team_id in (('093B41CC','093B571C','0997621C'),
                                      ('093B522C','093B50BC','08D1D32C')):
        team=definitions[team_id]
        task_force_id,script_id=team['TaskForce'],team['Script']
        raw=definitions['Events'][trigger_id]
        routes.append(dict(map='ALL04DMD.MAP',map_sha256=hashlib.sha256(raw_map).hexdigest(),
            archive='mapsmd03.mix',archive_sha256=hashlib.sha256(archive).hexdigest(),
            trigger_id=trigger_id,trigger=definitions['Triggers'][trigger_id],
            tag_id=tag_id,tag=definitions['Tags'][tag_id],raw_event=raw,native_read=m.read_event(raw),
            actions=definitions['Actions'][trigger_id],team_id=team_id,team=team,
            task_force_id=task_force_id,task_force=definitions[task_force_id],
            script_id=script_id,script=definitions[script_id]))
    return dict(schema_version=1,native_fpcw=f'0x{NATIVE_FPCW:X}',
                country_indices=[0,7,0],controls=controls,classification=classifiers,
                retail_routes=routes,code_unchanged=True,zero_rng_calls=True)


def loss_threshold_event_metadata():
    return provenance(
        scope='Thirty-four original HasOccurred15/16 controls, four StateA/StateB calls and two original counted Event reads from actual retail ALL04. Loss counters are supplied priors; not RecordKill, House lifecycle, reinforcement execution or whole-campaign parity.',
        assumptions=[
            'Reuse PowerEventControls/FlagsFixture and its existing counted TEvent reader. Three original-layout Houses have Country bindings0/7/0. House+5488 Building losses and+5434 other-object losses are supplied signed bit patterns; the other counter is1000 in boundary controls to distinguish the selected field.',
            'Original HasOccurred71E940 executes15 at71F03F and16 at71F058. Both compare their incoming House counter >= Event+34 signed. Controls cover equality, negatives, INT_MIN/MAX, incoming House1, absent incoming House, wrong raised99 and both A8ED6B values. A separate Country7 House has larger losses when threshold7 must still fail for incoming House0.',
            'Event+54 sentinel0xABCD1234, supplied timer and initial persistence0/1 remain unchanged. StateA71F950 returns0 and StateB71F9C0 returns1. The existing native observer asserts no65C780/65C7E0 draw entry. No trigger timer write or detach belongs to these predicates; shared RegisterEvent/Tag repeat/reset/cleanup are outside this scalar execution.',
            'Existing retail MIX extraction supplies the actual mapsmd03.mix member ALL04DMD.MAP; hashes identify archive and member bytes. Existing lexical sections supply Events cache strings, then original ReadString/count/constructor/Read71F4E0/strtok/atoi parse each selected record. Original physical file loading and complete INI layer ordering are not emulated.',
            'The two retained routes are retail Americans easy own-loss reinforcements: Event16/6 on093B41CC and Event15/4 on093B522C. Trigger, Tag, action, Team, TaskForce and Script sections are included as raw retail data for production-reader integration. Their references and ground reinforcement execution are not native-executed here; the counted Event reader outputs alone establish the parsed event kinds/values.',
            'Counter initialization and writers remain the existing MatchStatistics/RecordKill owners. Existing engineer_capture_accounting evidence covers its declared NULL callback/capture controls on its recorded binary. This new corpus does not execute RecordKill, sale, sinking, capture, House construction, loss wrapping or death timing, and adds no new lifecycle claim.'],
        substitutions=['Existing FlagsFixture allocator/CRT/TLS transports and explicit House loss-counter priors only. Original HasOccurred/Country lookup/StateA/StateB and counted Event reader instructions run unchanged; no selected gameplay return is supplied.'],
        entry_points={'event_constructor':0x71E6A0,'event_counted_read':0x7274DC,
            'has_occurred':0x71E940,'building_loss_compare':0x71F03F,'unit_loss_compare':0x71F058,
            'country_house':0x502D30,'state_a':0x71F950,'state_b':0x71F9C0})




def naval_land_retail_inputs():
    """Existing Country/type reader owners, with selected ALL05 retail bytes."""
    from tools.input_oracle.campaign_start import CampaignReader, prepare_house_rules, assets_root
    from tools.sidebar_oracle.stock import mix, mix_hash
    from tools.rules_oracle.bridge_child_sound import sections
    from tools.spatial_oracle.building_body_rules import INI
    root=Path(os.environ['RA2_DIR'])
    raw_map=mix((root/'mapsmd03.mix').read_bytes())[mix_hash('ALL05UMD.MAP')]
    raw_rules=Path('ini/rulesmd.ini').read_bytes()
    physical_map,physical_rules=sections(raw_map),sections(raw_rules)
    # The inherited owner eagerly reads its asset directory. This selected
    # prefix needs no assets; require an empty directory rather than silently
    # reading a whole retail installation or introducing supplied image data.
    assets=assets_root()
    if not assets.is_dir() or next(assets.iterdir(),None) is not None:
        raise ValueError('Set VERA20K_CAMPAIGN_START_ASSETS to an empty directory for --naval-land-events')
    m=CampaignReader();m.invoke(0x6832C0,m.scenario)
    rules=prepare_house_rules(m,physical_rules)
    registries=[]
    for label,values in (('RULESMD.INI',physical_rules),('ALL05UMD.MAP',physical_map)):
        m.ordered_ini({'Countries':values.get('Countries',{})})
        m.u.reg_write(UC_X86_REG_ESI,INI);m.u.reg_write(UC_X86_REG_EDI,rules)
        m.region(0x668C8B,0x668CDB)
        registries.append(dict(layer=label,entries=[dict(index=i,
            name=m.string(m.read32(m.read32(0xA83C9C)+i*4)+0x24))
            for i in range(m.read32(0xA83CA8))]))
    # This is the shared base TechnoType constructor and field-block boundary
    # used by ifv_select_anim, not a full UnitType or Rules Process replay.
    m.u.mem_write(0xA8EB00,dwords(0x7EB6D4,m.alloc(4096),1024,1,0,10))
    typ=m.alloc(0xE00);m.invoke(0x710AF0,typ,(m.cstring('BSUB'),))
    def fields():
        return dict(naval=m.u.mem_read(typ+0xCCE,1)[0],
                    passengers=struct.unpack('<i',m.u.mem_read(typ+0x5E0,4))[0])
    readers=[dict(layer='TechnoType_constructor710AF0',fields=fields())]
    for label,values in (('RULESMD.INI',physical_rules),('ALL05UMD.MAP',physical_map)):
        body=values.get('BSUB',{})
        m.ordered_ini({'BSUB':body})
        for begin,end in ((0x714A63,0x714A7D),(0x714B35,0x714B56)):
            m.u.reg_write(UC_X86_REG_EBP,typ);m.u.reg_write(UC_X86_REG_EBX,typ+0x24)
            m.u.reg_write(UC_X86_REG_EDI,INI);m.region(begin,end)
        readers.append(dict(layer=label,section_present='BSUB' in values,
                            naval_key=body.get('Naval'),passengers_key=body.get('Passengers'),
                            fields=fields()))
    m.check_code()
    identity,tag_id='0E5FA1BC','0E5FB56C'
    raw=physical_map['Events'][identity]
    return dict(map='ALL05UMD.MAP',map_sha256=hashlib.sha256(raw_map).hexdigest(),
        rules_sha256=hashlib.sha256(raw_rules).hexdigest(),trigger_id=identity,
        raw_event=raw,native_read=PowerEventControls.read_event(raw),
        trigger=physical_map['Triggers'][identity],tag_id=tag_id,tag=physical_map['Tags'][tag_id],
        actions=physical_map['Actions'][identity],selected_house_name='BadGuy2 House',
        selected_house=physical_map['BadGuy2 House'],initial_bsub_entry_id='38',
        initial_bsub_entry=physical_map['Units']['38'],authored_health=184,
        country_registries=registries,type_field_readers=readers)


def generate_naval_land_events():
    """Signed tracked-count predicates; keep the existing tracking oracle owner."""
    from tools.native_oracle import NATIVE_FPCW, first_difference
    from tools.spatial_oracle import house_tracking
    m=PowerEventControls(reader_inputs=())
    m.SPANS=[*m.SPANS,(0x71F950,0x71FA40)]
    m.original.append(bytes(m.u.mem_read(0x71F950,0xF0)))
    controls,classifiers=[],[]

    def probe(kind,name,counts,value=0,house=1,actor=1,raised=8,
              order=(0,1,2),buildup=False,persistence=0):
        for pointer,(vehicles,navy,infantry) in zip(m.houses,counts):
            m.u.mem_write(pointer+0x2E8,dwords(vehicles,navy,0,infantry))
        before=list(struct.unpack('<iii',m.u.mem_read(m.timer,12)))
        m.predicate(f'naval_land{kind}_{name}',f'1,{kind},0,{value}',
                    house_index=house,actor_index=actor,raised=raised,order=order,
                    scenario_init=buildup,initial_persistence=persistence)
        row=m.predicates[-1]
        after=list(struct.unpack('<iii',m.u.mem_read(m.timer,12)))
        assert after==before and row['persistence']==persistence
        assert row['event54']==struct.unpack('<i',dwords(0xABCD1234))[0]
        controls.append(dict(name=row['name'],kind=kind,value=value,tracked_counts=counts,
            house_order=order,incoming_house=house,actor_owner=actor,raised=raised,
            scenario_init=buildup,initial_persistence=persistence,result=row['result'],
            persistence=row['persistence'],event54=row['event54'],timer_before=before,timer_after=after))

    for navy in (-2147483648,-1,0,1,2147483647):
        probe(55,f'navy{navy}',((3,navy,9),(7,7,7),(4,2,0)))
    for index,selected in enumerate(((0,0,0),(1,0,0),(0,0,1),(5,5,0),(6,5,0),(4,5,0),
                                     (0,0,-1),(0,0,-2147483648),(0,0,2147483647),
                                     (-2147483648,1,0),(2147483647,-1,0),
                                     (-2147483648,2147483647,0),(2147483647,-2147483648,0),
                                     (-1,0,0))):
        probe(56,f'signed_sub_infantry{index}',(selected,(7,7,7),(4,2,0)))
    for kind in (55,56):
        probe(kind,'all_houses_zero',((0,0,0),)*3,house=0,actor=None)
        for order in ((0,1,2),(2,1,0)):
            probe(kind,f'first_country_order{order}',((0,0,0),(7,7,7),(4,2,0)),order=order)
        for value in (-1,999):
            probe(kind,f'absent_country{value}',((2,1,1),)*3,value=value)
        probe(kind,'country7',((7,7,7),(0,0,0),(7,7,7)),value=7,house=0,actor=2)
        probe(kind,'null_incoming_selected_positive',((2,1,0),(0,0,0),(4,2,0)),house=None)
        for buildup in (False,True):
            probe(kind,f'wrong_raise_buildup{int(buildup)}',((0,0,0),(7,7,7),(4,2,0)),
                  raised=99,buildup=buildup)
        probe(kind,'retained_persistence_one',((0,0,0),(7,7,7),(4,2,0)),persistence=1)
        classifiers.append(dict(kind=kind,state_a=m.fixture.call(0x71F950,[],ecx=m.event)&255,
                                state_b=m.fixture.call(0x71F9C0,[],ecx=m.event)&255))
    m.assert_unchanged()
    # Re-execute the sole tracking producer, compare its existing complete
    # corpus, and record actual replay identity separately from old provenance.
    actual=house_tracking.generate()
    saved_path=Path(house_tracking.__file__).with_suffix('.json')
    saved=json.loads(saved_path.read_text(encoding='utf-8'))
    difference=first_difference(saved,actual)
    assert difference is None,difference
    digest=hashlib.sha256(json.dumps(actual,sort_keys=True,separators=(',',':'),
                                   allow_nan=False).encode('utf-8')).hexdigest()
    old_meta=json.loads(saved_path.with_suffix('.meta.json').read_text(encoding='utf-8'))
    replay=dict(cases=len(actual),payload_matches_saved=True,payload_sha256=digest)
    return dict(schema_version=1,native_fpcw=f'0x{NATIVE_FPCW:X}',
                country_indices=[0,7,0],controls=controls,classification=classifiers,
                tracking_replay=replay,retail=naval_land_retail_inputs(),
                code_unchanged=True,zero_predicate_rng_calls=True)


def naval_land_event_metadata():
    result=provenance(
        scope='39 original HasOccurred55/56 predicates, four StateA/StateB calls, replay of the existing36 tracking controls, selected retail ALL05 Event/Country and BSUB Naval/Passengers readers. Not full House/object construction, resource search, reinforcement execution or campaign parity.',
        assumptions=[
            'Reuse PowerEventControls/FlagsFixture original image, counted TEvent reader and minimal three-House arena with Country bindings0/7/0. Supplied tracked+2E8/+2EC/+2F4 priors are vehicles/navy/infantry. Original Country502D30 and HasOccurred71E940 execute; incoming House, actor, raised kind and A8ED6B do not select these predicates\' House.',
            '55 at71F159 tests signed navy<=0.56 at71F16F subtracts navy from vehicles with32-bit wrapping, TEST/JG, then independently tests tracked infantry<=0. INT_MIN/MAX, overflow, independent infantry signs, first duplicate Country order, absent Country, null incoming House, unrelated actor/raised flag and initial persistence0/1 are executed. Event+54 and timer remain unchanged; StateA/B are0/1 and existing observers assert no predicate65C780/65C7E0 entry. No own timer write or detach belongs to these leaves.',
            'house_tracking.generate is the existing sole Add/Remove fixture. Its complete36-case replay on the actual selected image must match every saved payload field. The old sidecar names the other supported1cdd image: actual replay identity and matching canonical payload hash are separate fields here, not a rewrite of that older provenance. Existing supplied virtuals, per-type Counter and owned-type sinks remain; this is not a concrete constructor/destructor lifecycle replay.',
            'Original tracking increases/decreases+2EC only in Unit RTTI1, after Insignificant/DontScore admission, when Naval+CCE is nonzero AND signed Passengers+5E0 is exactly0. Naval transports with any nonzero capacity remain vehicles without navy contribution. Production integration must retain the existing tracking construction, ChangeOwner and actual final-deletion owners rather than active count, factory/naval statistics or a map scan.',
            'Actual mapsmd03.mix member ALL05UMD.MAP and extracted retail ini/rulesmd.ini bytes are SHA-identified. Lexical cached entries feed original counted TEvent and Country registration668C8B..668CDB; rules followed by map registration yields Country17 BadGuy2. The retained BadGuy2 House BSUB Units entry is raw data; authored health184 is not executed actual HP. Trigger0E5FA1BC starts enabled and one-shot;53/21/53 activates existing black-eagle support consumers. Later AIProduction74 and whole-level behavior are outside this evidence.',
            'Existing CampaignReader and shared TechnoType710AF0 constructor establish Naval=false/Passengers0. Original field blocks714A63..714A7D and714B35..714B56 execute on physical RULESMD BSUB then the absent map section: Naval=yes ->1, absent Passengers retains0; the map retains1/0. Readers pass current fields as defaults, exact-case keys, ReadBool5295F0/ReadInt5276D0 without clamps. The absent-section row deliberately executes selected readers; it does not claim whole ReadINI section admission, full UnitType construction, all Rules Process passes, physical loading or exhaustive LANGRULE/scenario-INI absence. Campaign has no mode layer in this selected input.',
            'CampaignReader is supplied an existing empty asset directory through VERA20K_CAMPAIGN_START_ASSETS; its inherited constructor preloads that directory, and these selected Country/type controls request no asset files. Registries have declared spare capacity. No original Tag/House polling/action execution is added here; the Rust production regressions and existing lifecycle evidence own their separate coverage.'],
        substitutions=['Existing FlagsFixture/CampaignReader allocator/CRT/TLS/INI-cache transports and explicit scalar/vector/House priors; existing house_tracking virtual/per-type sinks retain their published boundary. No selected event, Country lookup, field reader or tracking-body return is replaced.'],
        entry_points={'has_occurred':0x71E940,'navy_predicate':0x71F159,'land_predicate':0x71F16F,
            'country_house':0x502D30,'state_a':0x71F950,'state_b':0x71F9C0,
            'add_tracking':0x4FF700,'remove_tracking':0x4FF550,'country_registration':0x668C8B,
            'techno_type_constructor':0x710AF0,'naval_reader':0x714A63,'passengers_reader':0x714B35})
    result['environment']={'RA2_DIR':'retail installation containing gamemd.exe and mapsmd03.mix',
        'VERA20K_CAMPAIGN_START_ASSETS':'existing empty directory; selected Country/type prefix needs no assets',
        'checkout_prerequisite':'ini/rulesmd.ini is the selected extracted retail RULESMD'}
    return result


def naval_land_event_source_paths():
    paths=power_event_source_paths()
    paths.update(tracking=Path(__file__).with_name('house_tracking.py'),
                 country_reader=Path(__file__).parents[1]/'input_oracle'/'campaign_start.py',
                 retail_mix=Path(__file__).parents[1]/'sidebar_oracle'/'stock.py',
                 retail_lexical=Path(__file__).parents[1]/'rules_oracle'/'bridge_child_sound.py')
    return paths




def no_factories_predicates():
    """Event17 on explicit tracked-count/list priors, using the shared VM."""
    from types import SimpleNamespace
    from tools.input_oracle.campaign_start import CampaignReader
    m=PowerEventControls(reader_inputs=())
    u=m.u
    for start,end in ((0x71F950,0x71FA40),(0x65C6D0,0x65C88D)):
        m.SPANS=[*m.SPANS,(start,end)]
        m.original.append(bytes(u.mem_read(start,end-start)))
    scenario=m.arena+0xB0000
    u.mem_write(0xA8B230,dwords(scenario))
    for pointer in (scenario+0x218,0x886B88,0xABE890):
        m.fixture.call(0x65C6D0,[31],ecx=pointer)
    rng_view=SimpleNamespace(u=u,scenario=scenario)
    before_rng=CampaignReader.rng_state(rng_view)

    def building(factory,*,limbo=False,health=100,ai_alive=True):
        return dict(factory_rtti=factory,limbo=limbo,health=health,ai_alive=ai_alive)

    def house(tracked,*buildings,count=None):
        return dict(tracked_buildings=tracked,buildings=list(buildings),
                    list_count=len(buildings) if count is None else count)

    empty=house(0)
    cases=[
        dict(name='all_houses_zero_tracked_empty_lists',houses=(empty,empty,empty)),
        dict(name='active_factory_incoming_not_event_country',
             houses=(house(1,building(16)),empty,empty),value=7),
        dict(name='limbo_factory_does_not_block',
             houses=(house(1,building(16,limbo=True)),empty,empty)),
        dict(name='null_and_no_factory_do_not_block',
             houses=(house(2,None,building(0)),empty,empty)),
        dict(name='zero_tracked_skips_present_factory',
             houses=(house(0,building(16)),empty,empty)),
        dict(name='negative_tracked_skips_present_factory',
             houses=(house(-1,building(16)),empty,empty)),
        dict(name='negative_factory_and_zero_health_still_block',
             houses=(house(1,building(-1,health=0,ai_alive=False)),empty,empty)),
        dict(name='incoming_not_country_actor_raised_or_buildup',
             houses=(house(1,building(16)),house(1,building(0)),empty),
             incoming=1,raised=99,actor=0,buildup=True,persistence=1),
        dict(name='null_incoming_generic_success',
             houses=(house(1,building(16)),empty,empty),incoming=None),
        dict(name='positive_tracked_empty_list',houses=(house(1),empty,empty)),
        dict(name='negative_vector_count',
             houses=(house(1,building(16),count=-1),empty,empty)),
        dict(name='normal_factory_health0',
             houses=(house(1,building(16,health=0)),empty,empty)),
        dict(name='normal_factory_ai_alive_false',
             houses=(house(1,building(16,ai_alive=False)),empty,empty)),
        dict(name='normal_factory_health_negative',
             houses=(house(1,building(16,health=-1)),empty,empty)),
        dict(name='intmin_tracked_with_factory',
             houses=(house(-2147483648,building(16)),empty,empty)),
        dict(name='intmax_tracked_with_factory',
             houses=(house(2147483647,building(16)),empty,empty)),
        dict(name='last_list_entry_factory',houses=(house(3,building(0),
             building(16,limbo=True),building(16)),empty,empty)),
        dict(name='object_class_unit_is_nonzero_factory',
             houses=(house(1,building(1)),empty,empty),raised=99),
        dict(name='object_class_infantry_is_nonzero_factory',
             houses=(house(1,building(15)),empty,empty)),
    ]
    controls=[]
    for case in cases:
        ordinal=0
        for index,(pointer,prior) in enumerate(zip(m.houses,case['houses'])):
            items=m.arena+0xC0000+index*0x1000
            pointers=[]
            for item in prior['buildings']:
                if item is None:
                    pointers.append(0)
                    continue
                obj=m.arena+0xD0000+ordinal*0x1000
                typ=m.arena+0xE0000+ordinal*0x1000
                ordinal+=1
                assert ordinal<16
                u.mem_write(obj,bytes(0x600));u.mem_write(typ,bytes(0x1000))
                u.mem_write(obj+0x81,bytes([item['limbo']]))
                u.mem_write(obj+0x6C,dwords(item['health']))
                u.mem_write(obj+0x90,bytes([item['ai_alive']]))
                u.mem_write(obj+0x520,dwords(typ))
                u.mem_write(typ+0xEB8,dwords(item['factory_rtti']))
                pointers.append(obj)
            if pointers:u.mem_write(items,dwords(*pointers))
            u.mem_write(pointer+0x6C,dwords(items))
            u.mem_write(pointer+0x78,dwords(prior['list_count']))
            u.mem_write(pointer+0x2F0,dwords(prior['tracked_buildings']))
        timer_before=list(struct.unpack('<iii',u.mem_read(m.timer,12)))
        persistence=case.get('persistence',0)
        m.predicate(case['name'],f"1,17,0,{case.get('value',0)}",
            house_index=case.get('incoming',0),raised=case.get('raised',8),
            actor_index=case.get('actor'),scenario_init=case.get('buildup',False),
            initial_persistence=persistence)
        row=m.predicates[-1]
        timer_after=list(struct.unpack('<iii',u.mem_read(m.timer,12)))
        assert timer_after==timer_before and row['persistence']==persistence
        assert row['event54']==struct.unpack('<i',dwords(0xABCD1234))[0]
        controls.append(dict(name=case['name'],kind=17,value=case.get('value',0),
            house_priors=case['houses'],house_order=[0,1,2],
            incoming_house=case.get('incoming',0),actor_owner=case.get('actor'),
            raised=case.get('raised',8),scenario_init=case.get('buildup',False),
            initial_persistence=persistence,result=row['result'],persistence=row['persistence'],
            event54=row['event54'],timer_before=timer_before,timer_after=timer_after,
            calls=[call['pc'] for call in row['calls']]))
    classification=dict(kind=17,state_a=m.fixture.call(0x71F950,[],ecx=m.event)&255,
                        state_b=m.fixture.call(0x71F9C0,[],ecx=m.event)&255)
    m.assert_unchanged()
    after_rng=CampaignReader.rng_state(rng_view)
    assert before_rng==after_rng
    return dict(country_indices=[0,7,0],controls=controls,classification=classification,
        reader_rows=m.readers,constructor_defaults=m.constructor_defaults,code_unchanged=True,
        rng_before={k:hashlib.sha256(bytes.fromhex(v)).hexdigest() for k,v in before_rng.items()},
        rng_after={k:hashlib.sha256(bytes.fromhex(v)).hexdigest() for k,v in after_rng.items()})


def no_factories_action9_controls():
    """Original dispatcher/FindHouse owner; only declared House storage grows."""
    from types import SimpleNamespace
    from unicorn import UC_HOOK_MEM_WRITE
    from tools.input_oracle.campaign_start import CampaignReader
    from tools.spatial_oracle.trigger_action_values import AllChangeHouseFixture, ACTION, SCENARIO
    f=AllChangeHouseFixture()
    u=f.u
    f.houses=[f.allocate(0x17000) for _ in range(3)]
    for pointer,typ,country in zip(f.houses,f.house_types,(19,7,19)):
        u.mem_write(pointer,bytes(0x17000))
        u.mem_write(pointer+0x34,dwords(typ));u.mem_write(typ+0xB8,dwords(country))
    f.ranges.append((0x6E45E0,0x6E462B))
    f.original.append(bytes(u.mem_read(0x6E45E0,0x4B)))
    writes=[]

    def observe_write(vm,_access,address,size,value,_data):
        for index,pointer in enumerate(f.houses):
            if pointer<=address<pointer+0x17000:
                writes.append(dict(house=index,offset=hex(address-pointer),size=size,
                                   value=value,pc=hex(vm.reg_read(UC_X86_REG_EIP))))

    u.hook_add(UC_HOOK_MEM_WRITE,observe_write)
    cases=[dict(name='retail_country19_first',selector=19),
        dict(name='duplicate_country_reversed',selector=19,order=(2,1,0)),
        dict(name='country7_not_incoming_or_trigger_house',selector=7),
        dict(name='retained_trigger_house_8997',selector=0x2325),
        dict(name='retained_trigger_house_null',selector=0x2325,trigger_owner=None),
        dict(name='null_live_trigger_valid_country',selector=19,null_trigger=True),
        dict(name='missing_country999',selector=999),dict(name='minus_one',selector=-1),
        dict(name='player_at_first_slot',selector=4475),
        dict(name='player_at_unmapped_slot',selector=4475,slots=(-1,)*8),
        dict(name='null_incoming_valid_country',selector=19,incoming=None),
        dict(name='already_mode4_still_writes',selector=19,modes=(4,3,5))]
    rows=[]
    rng_view=SimpleNamespace(u=u,scenario=SCENARIO)
    for case in cases:
        f.selectors,f.rng_calls=[],[]
        order=case.get('order',(0,1,2))
        u.mem_write(f.house_array,dwords(*(f.houses[i] for i in order)))
        slots=case.get('slots',(1,0,-1,-1,-1,-1,-1,-1))
        u.mem_write(SCENARIO+0x1180,dwords(*slots))
        u.mem_write(f.live,bytes(0x48))
        owner=case.get('trigger_owner',2)
        u.mem_write(f.live+0x2C,dwords(f.houses[owner] if owner is not None else 0))
        for i,pointer in enumerate(f.houses):
            u.mem_write(pointer+0x249,b'\x01');u.mem_write(pointer+0x24C,dwords(5))
            u.mem_write(pointer+0x250,dwords(case.get('modes',(-7,3,5))[i]))
            u.mem_write(pointer+0x5634,dwords(41+i,777+i,108+i))
        read=f.read_action(f"9,0,{case['selector']},0,0,0,0,A")
        before=[bytes(u.mem_read(p,0x17000)) for p in f.houses]
        rng_before=CampaignReader.rng_state(rng_view)
        writes.clear()
        incoming=case.get('incoming',1)
        result=f.call(0x6DD8B0,[f.houses[incoming] if incoming is not None else 0,
            0,0 if case.get('null_trigger') else f.live,0],ecx=ACTION)&255
        measured_writes=list(writes)
        after=[bytes(u.mem_read(p,0x17000)) for p in f.houses]
        rng_after=CampaignReader.rng_state(rng_view)
        assert rng_before==rng_after and not f.rng_calls
        assert all(w['offset']=='0x250' and w['size']==4 and w['value']==4
                   for w in measured_writes)
        changed=[[j for j,(a,b) in enumerate(zip(left,right)) if a!=b]
                 for left,right in zip(before,after)]
        assert all(all(0x250<=offset<0x254 for offset in offsets) for offsets in changed)
        rows.append(dict(name=case['name'],selector=case['selector'],native_read=read,
            country_indices=[19,7,19],house_order=order,player_at_slots=slots,
            incoming_house=incoming,trigger_owner=owner,live_trigger=not case.get('null_trigger',False),
            result=result,selector_calls=list(f.selectors),mode_writes=measured_writes,
            modes_before=[struct.unpack_from('<i',raw,0x250)[0] for raw in before],
            modes_after=[struct.unpack_from('<i',raw,0x250)[0] for raw in after],
            strategy_timers_before=[list(struct.unpack_from('<iii',raw,0x5634)) for raw in before],
            strategy_timers_after=[list(struct.unpack_from('<iii',raw,0x5634)) for raw in after],
            changed_house_byte_offsets=changed,
            rng_before={k:hashlib.sha256(bytes.fromhex(v)).hexdigest() for k,v in rng_before.items()},
            rng_after={k:hashlib.sha256(bytes.fromhex(v)).hexdigest() for k,v in rng_after.items()}))
    f.check_code()
    return dict(controls=rows,code_unchanged=True,all_three_rng_states_unchanged=True)


def no_factories_factory_readers(raw_rules,raw_map):
    """Read the original RTTI table and the existing BuildingType field owner."""
    from tools.spatial_oracle.engineer_bridge_cursor_caller import HutTypeReader, READER_INI, READER_SP
    from tools.rules_oracle.bridge_child_sound import sections
    m=HutTypeReader()
    u=m.u
    table=[dict(name=m.string(m.read32(address)),value=m.read32(address+4))
           for address in range(0x816EE0,0x817130,8)]
    types={name:m.construct(name) for name in ('GAPILE','GAPILL')}
    defaults={name:m.read32(pointer+0xEB8) for name,pointer in types.items()}
    reads,rows=[],[]

    def observe(vm,pc,_size,_data):
        sp=vm.reg_read(UC_X86_REG_ESP)
        if pc==0x474FF0:
            reads.append(dict(entry=hex(pc),section=m.string(m.read32(sp+4)),
                key=m.string(m.read32(sp+8)),default=m.read32(sp+12)))
        elif pc==0x528A10:
            reads.append(dict(entry=hex(pc),section=m.string(m.read32(sp+4)),
                key=m.string(m.read32(sp+8)),default_string=m.string(m.read32(sp+12)),
                capacity=m.read32(sp+20)))
        elif pc==0x40DCE0:
            reads.append(dict(entry=hex(pc),enum_input=m.string(vm.reg_read(UC_X86_REG_ECX))))

    u.hook_add(UC_HOOK_CODE,observe)

    def field(name,label,values,prior=None):
        typ=types[name]
        if prior is not None:u.mem_write(typ+0xEB8,dwords(prior))
        before=m.read32(typ+0xEB8)
        m.make_ini(values)
        first=len(reads)
        u.mem_write(READER_SP,dwords(RET_MAGIC))
        for register,value in ((UC_X86_REG_ESP,READER_SP),(UC_X86_REG_EBP,typ),
            (UC_X86_REG_EBX,typ+0x24),(UC_X86_REG_ESI,READER_INI),(UC_X86_REG_EDI,typ+0x1F8)):
            u.reg_write(register,value)
        run_checked(u,0x46051A,0x46054B,count=200000,required_addresses=(0x474FF0,0x40DCE0))
        assert u.reg_read(UC_X86_REG_ESP)==READER_SP-16
        rows.append(dict(type=name,name=label,supplied_prior=prior,raw_cache=values,
            before=before,after=m.read32(typ+0xEB8),actual_read_calls=reads[first:]))

    for name in types:
        for label,values in (('RULESMD.INI',sections(raw_rules)),('SOV02SMD.MAP',sections(raw_map))):
            # Only this exact-key field block runs; unrelated Type fields are
            # intentionally not admitted as a whole ReadINI claim.
            cache={name:{'Factory':values[name]['Factory']}} if 'Factory' in values.get(name,{}) else {}
            field(name,label,cache)
    for label,cache,prior in (
        ('absent_retains_prior',{},16),
        ('wrong_case_key',{'GAPILE':{'factory':'UnitType'}},16),
        ('wrong_case_section',{'gapile':{'Factory':'UnitType'}},16),
        ('case_insensitive_enum',{'GAPILE':{'Factory':'iNfAnTrYtYpE'}},0),
        ('object_class_unit',{'GAPILE':{'Factory':'Unit'}},0),
        ('absent_retains_object_class_unit',{},None),
        ('object_class_infantry',{'GAPILE':{'Factory':'Infantry'}},0),
        ('absent_retains_object_class_infantry',{},None),
        ('unknown_enum',{'GAPILE':{'Factory':'NoSuchClass'}},16),
        ('explicit_none',{'GAPILE':{'Factory':'<none>'}},16),
        ('stored_empty_value',{'GAPILE':{'Factory':''}},16),
        ('enum_spaces',{'GAPILE':{'Factory':' InfantryType '}},0),
        ('enum_over_32bytes',{'GAPILE':{'Factory':'InfantryType'+'x'*35}},16)):
        field('GAPILE',label,cache,prior)
    m.unchanged()
    def dispatch_table(selector_address,jump_address,default):
        # These are the actual paired executable tables, not another RTTI
        # parser or a reimplementation of the selected native switch.
        selectors=bytes(u.mem_read(selector_address,40))
        jumps=list(struct.unpack('<5I',u.mem_read(jump_address,20)))
        return dict(selector_bytes_hex=selectors.hex(),jump_targets=jumps,
            aliases=[dict(rtti=index+1,entry=jumps[selector]) for index,selector in enumerate(selectors)
                     if jumps[selector]!=default],default_entry=default)

    # Preserve Suggest's existing schema/output while sharing its data reader
    # with the separate accounting consumers. None executes a production AI.
    suggest=dispatch_table(0x4FBE0C,0x4FBDF8,0x4FBDF3)
    counters=[]
    slot_names={0x5378:'aircraft',0x537C:'infantry',0x5380:'vehicle',
                0x5384:'building',0x5388:'naval'}
    for name,entry,selector,jump,default,leas in (
        ('increment',0x4FFA50,0x4FFAF0,0x4FFADC,0x4FFAD6,
         ((0x4FFAB2,0x4FFAAA),(0x4FFABA,0x4FFABA),(0x4FFACA,0x4FFACA),(0x4FFAC2,0x4FFAC2))),
        ('recount',0x4FF980,0x4FFA20,0x4FFA0C,0x4FFA06,
         ((0x4FF9E2,0x4FF9DA),(0x4FF9EA,0x4FF9EA),(0x4FF9FA,0x4FF9FA),(0x4FF9F2,0x4FF9F2))),
        ('get_count',0x500910,0x50097C,0x500968,0x500960,
         ((0x500939,0x500931),(0x500941,0x500941),(0x500951,0x500951),(0x500949,0x500949)))):
        counter=dispatch_table(selector,jump,default)
        assert counter['jump_targets'][-1]==default
        leaves={}
        for target,pair in zip(counter['jump_targets'],leas):
            offsets=[]
            for pc in pair:
                instruction=bytes(u.mem_read(pc,6))
                # LEA EAX,[EDI/ECX+disp32]: the original House counter field.
                assert instruction[:2] in (b'\x8D\x87',b'\x8D\x81')
                offsets.append(struct.unpack('<I',instruction[2:])[0])
            assert all(offset in slot_names for offset in offsets)
            leaves[target]=dict(slot_offsets_by_naval=offsets,
                role=slot_names[offsets[0]] if offsets[0]==offsets[1] else 'vehicle_or_naval')
        for alias in counter['aliases']:
            alias.update(leaves[alias['entry']])
        counters.append(dict(name=name,entry=entry,selector_address=selector,jump_address=jump,
            **counter,jump_bytes_hex=bytes(u.mem_read(jump,20)).hex(),
            slot_lea_instructions=[dict(address=pc,hex=bytes(u.mem_read(pc,6)).hex())
                for pc in sorted({pc for pair in leas for pc in pair})]))
    return dict(rtti_name_table=table,constructor_defaults=defaults,controls=rows,
                suggest_dispatch=suggest,counter_dispatch=counters,code_unchanged=True)


def no_factories_retail_readers(raw_rules,raw_map):
    """Country/Trigger binding and selected action reads, never a world load."""
    from tools.input_oracle.campaign_start import CampaignReader, prepare_house_rules
    from tools.rules_oracle.bridge_child_sound import sections
    from tools.spatial_oracle.building_body_rules import INI, SP
    from tools.spatial_oracle.trigger_action_values import ActionFixture, ACTION
    from tools.spatial_oracle.trigger_type_flags import FlagsFixture
    rules,values=sections(raw_rules),sections(raw_map)
    m=CampaignReader()
    m.invoke(0x6832C0,m.scenario)
    rules_pointer=prepare_house_rules(m,rules)
    m.seed_streams(31)
    rng_before=m.rng_state()
    registrations=[]
    for layer,entries in (('RULESMD.INI',rules),('SOV02SMD.MAP',values)):
        m.ordered_ini({'Countries':entries.get('Countries',{})})
        m.u.reg_write(UC_X86_REG_ESI,INI);m.u.reg_write(UC_X86_REG_EDI,rules_pointer)
        m.region(0x668C8B,0x668CDB)
        registrations.append(dict(layer=layer,raw_countries=entries.get('Countries',{}),
            entries=[dict(index=i,name=m.string(m.read32(m.read32(0xA83C9C)+i*4)+0x24))
                     for i in range(m.read32(0xA83CA8))]))
    country_pointers=[m.read32(m.read32(0xA83C9C)+i*4) for i in range(m.read32(0xA83CA8))]
    m.ordered_ini({'German1 House':values['German1 House']})
    house_country=m.invoke(0x475540,INI,(m.cstring('German1 House'),m.cstring('Country'),0xFFFFFFFF))
    action_reader=ActionFixture()
    trigger_ids=('043B06CC','043A9BBC')
    for identity in trigger_ids:
        name=action_reader.allocate(len(identity)+1)
        action_reader.u.mem_write(name,identity.encode('ascii')+b'\0')
        action_reader.call(0x726C80,[name],ecx=action_reader.allocate(0xB4))
    routes=[]
    flags=FlagsFixture()
    for identity in trigger_ids:
        raw_trigger=values['Triggers'][identity]
        trigger=m.alloc(0xB4)
        m.u.mem_write(trigger,bytes(0xB4))
        m.u.mem_write(trigger+0x24,identity.encode('ascii')+b'\0')
        m.u.mem_write(trigger+0xA4,dwords(country_pointers[0]))
        m.ordered_ini({'Triggers':{identity:raw_trigger}})
        read_start=len(m.trigger_read_events)
        m.u.mem_write(SP,dwords(RET_MAGIC,INI));m.u.reg_write(UC_X86_REG_ECX,trigger)
        m.region(0x727240,0x7272D1,required=(0x5117D0,0x7272CB))
        owner_index=country_pointers.index(m.read32(trigger+0xA4))
        raw_action=values['Actions'][identity]
        assert raw_action.startswith('1,')
        action=action_reader.read_action(raw_action[2:],ini_capacity=512)
        reference=action_reader.read32(ACTION+0x50)
        routes.append(dict(trigger_id=identity,trigger=raw_trigger,
            native_owner_country=owner_index,country_read_calls=m.trigger_read_events[read_start:],
            native_flags=flags.execute(raw_trigger,[1,1,1,1,0]),
            raw_event=values['Events'][identity],native_event=PowerEventControls.read_event(values['Events'][identity]),
            raw_actions=raw_action,native_action=action,
            native_action_trigger_reference=action_reader.string(reference+0x24) if reference else None,
            tags={tag:body for tag,body in values['Tags'].items() if body.endswith(','+identity)}))
    m.check_code();action_reader.check_code()
    assert rng_before==m.rng_state()
    return dict(map='SOV02SMD.MAP',rulesmd_sha256=hashlib.sha256(raw_rules).hexdigest(),
        map_sha256=hashlib.sha256(raw_map).hexdigest(),country_registration=registrations,
        house_name='German1 House',house=values['German1 House'],native_house_country=house_country,
        initial_structures={key:body for key,body in values['Structures'].items()
                            if body.split(',')[0]=='German1 House'},
        initial_infantry={key:body for key,body in values['Infantry'].items()
                         if body.split(',')[0]=='German1 House'},
        raw_rules_iq=rules['IQ'],raw_map_iq=values.get('IQ'),routes=routes,
        code_unchanged=True,country_trigger_rng_unchanged=True)


def no_factories_strategy_controls():
    """Compose the existing Strategy/Update/Sale/Hunt owners, not a new AI VM."""
    import inspect
    from tools import ai_strategy_oracle as strategy
    from tools import superweapon_oracle as sw
    saved=json.loads(Path(strategy.__file__).with_suffix('.json').read_text(encoding='utf-8'))
    saved_meta=json.loads(Path(strategy.__file__).with_suffix('.meta.json').read_text(encoding='utf-8'))
    comparisons=[]

    def replay(section,fn,row):
        actual=fn(**{key:row[key] for key in inspect.signature(fn).parameters if key in row})
        assert actual==row
        comparisons.append(dict(section=section,matched_all_fields=True,actual=actual))

    replay('strategy',strategy.strategy_row,next(r for r in saved['strategy']
           if r['label']=='urgency_plain_only_mode4_game0'))
    replay('schedule',strategy.schedule_row,next(r for r in saved['schedule']
           if r['start']==100 and r['delay']==108 and r['frame']==208))
    replay('fire_sale',strategy.fire_sale_row,next(r for r in saved['fire_sale']
           if r['label']=='sells_each_live_building'))
    replay('all_to_hunt',strategy.all_to_hunt_row,next(r for r in saved['all_to_hunt']
           if r['label']=='mixed_computer'))
    actual=strategy.strategy_row('campaign_mode4_original_rng',game_mode=0,mode=4,
        iq=5,iq_superweapons=4,buildings=(strategy.PLAIN,strategy.PLAIN),
        peers=[strategy.peer() for _ in range(4)],native_rng_seed=31)
    delay=actual['delay']
    assert actual['native_rng']['before']['main']==actual['native_rng']['after']['main']
    assert actual['native_rng']['before']['mapgen']==actual['native_rng']['after']['mapgen']
    timers=[strategy.schedule_row(start=1000,delay=delay,frame=frame,game_mode=0,answer=delay)
            for frame in (1000+delay-1,1000+delay,1000+delay+1)]
    ordinary_hunt=strategy.all_to_hunt_row('ordinary_foot_no_garrison',technos=[
        strategy.techno(),strategy.techno(team=True),strategy.techno(owner=False),
        strategy.techno(down=False),strategy.techno(limbo=True)])
    saved_sw=json.loads(Path(sw.__file__).with_suffix('.json').read_text(encoding='utf-8'))
    replay('uncharged_superweapons',sw.try_fire_row,next(row for row in saved_sw['ai_try_fire']
           if row['kinds'] and not any(charged for kind,charged in row['kinds'])))
    return dict(existing_payload_replays=comparisons,
        genuine_rng_strategy=actual,update_cadence_cases_are_independent=True,
        update_cadence_from_native_delay=timers,ordinary_foot_hunt=ordinary_hunt)


def generate_no_factories_events():
    from tools.native_oracle import NATIVE_FPCW
    from tools.sidebar_oracle.stock import mix, mix_hash
    archive=(Path(os.environ['RA2_DIR'])/'mapsmd03.mix').read_bytes()
    raw_map=mix(archive)[mix_hash('SOV02SMD.MAP')]
    raw_rules=Path('ini/rulesmd.ini').read_bytes()
    return dict(schema_version=1,native_fpcw=f'0x{NATIVE_FPCW:X}',
        predicates=no_factories_predicates(),action9=no_factories_action9_controls(),
        factory=no_factories_factory_readers(raw_rules,raw_map),
        retail=dict(archive='mapsmd03.mix',archive_sha256=hashlib.sha256(archive).hexdigest(),
                    **no_factories_retail_readers(raw_rules,raw_map)),
        strategy=no_factories_strategy_controls())


def no_factories_event_metadata():
    result=provenance(
        scope='19 original Event17 predicate controls, 12 complete Action9 dispatch controls, 74 native Factory RTTI names, 17 original Factory field reads, three original Factory accounting dispatch tables and selected SOV02 Country/Trigger/action bindings; existing Strategy/Update/Sale/Hunt payload replays plus one genuine-RNG mode4 control. Independent bounded owners, not a complete native SOV02 or House lifecycle execution.',
        assumptions=[
            'PowerEventControls/FlagsFixture owns Event17 execution. Supplied House Country0/7/0, signed tracked+2F0 and Building vector+68 priors are independent. Original17 scans the incoming House, succeeds for NULL or signed tracked<=0, skips null/limbo list entries and rejects any nonzero Factory raw RTTI. Health and AI-alive priors do not gate it. Negative vector count is an abnormal supplied boundary, not a normal Rust Vec state.',
            'HasOccurred71E940/StateA71F950/StateB71F9C0 execute unchanged. Event+54 sentinel0xABCD1234, persistence initially0/1 and supplied timer remain unchanged; wrong raised kind and A8ED6B true are included. Three full original seed31 RNG states are compared unchanged. The selected predicate contains no timer or detach producer; no tracking writer, House/Building constructor, Unlimbo/UnInit or list teardown is newly certified.',
            'AllChangeHouseFixture reuses ActionFixture and the original6E45E0 House selector. Twelve controls run real TAction constructor/record read/dispatch with Country19/7/19 order, retained Trigger House8997, Player@4475 slots, missing/NULL targets and an already-mode4 target. Full supplied House byte comparisons and actual write observations permit only mode+250=4; strategy timers remain unchanged and all three full RNG states are identical. No Tag firing or immediate sale/Hunt occurs in this Action9 boundary.',
            'Factory table816EE0..817130 is actual executable data, not a host enum algorithm. Whole original BuildingType45DD90 constructors produce default0; selected46051A..46054B passes exact section/key Factory and current raw RTTI through ReadFactory474FF0, ReadString32 and40DCE0. Enum names are case-insensitive; raw Unit1/Infantry15 must stay distinct from UnitType40/InfantryType16. Suggest_New_Object4FBD80 two-level selector4FBE0C/jump4FBDF8 bytes preserve the eight native object/type aliases as static data, not an executed producer chain. Cached empty values are not a physical empty-line admission claim. Only the retained RULESMD and selected map field passes execute, not whole ReadINI, supplemental language/mode/campaign layer closure or asset loading.',
            'Factory accounting dispatch data is read from this same original-image owner: Increment4FFA50 selector4FFAF0/jump4FFADC, Recount4FF980 selector4FFA20/jump4FFA0C and GetFactoryCount500910 selector50097C/jump500968. Their eight raw class aliases1/40,2/3,6/7,15/16 and default target are preserved with actual LEA instruction bytes and House field offsets for Naval=false/true. The shared table extractor also owns the retained Suggest table; there is no second VM, RTTI parser or counter algorithm. These are original byte/control-flow goldens for class-to-slot dispatch only; no new execution of count increments/decrements, wrapping, destruction timing or primary grouping is claimed.',
            'Existing MIX/lexical transports supply actual SOV02SMD.MAP and retained retail ini/rulesmd.ini. CampaignReader executes Countries668C8B..668CDB in Rules/map order, House Country475540 and Trigger727240..7272D1 binding. Germany1 becomes Country19; German1 House is its actual House label. Original counted Event and Action record readers bind17/0 -> Action53 target043A9BBC ->13/45 ->Action9 Country19. Tags and authored Structures/Infantry remain raw retail data; authored health256 is not actual HP. No live native Tag enable/House poll or whole map load is executed here.',
            'Existing Strategy owner is reused, not copied. Four selected old Strategy/Update/Sale/Hunt payloads and one uncharged-Superweapon payload are compared field-for-field; their historical sidecars are retained, not reidentified or called a successful old CLI check. Existing recorded SellBack, Team removal, QueueMission, release/virtual and Superweapon boundaries remain intact.',
            'The new strategy_row native_rng_seed=31 control executes unchanged Strategy4FD500 and genuine original RandomRanged/Seed; three full states, actual raw-word stores and ranged result are retained. Campaign mode4 uses supplied plain-building/peer/money priors, IQ5 and IQ-SuperWeapons4; TryFireSW/FireSale/Hunt remain recorded dependency boundaries. The three Update controls each start a fresh VM with start1000 and the native-returned delay; they are independent boundary probes, not a sequential timer history. Ordinary Foot Hunt is bounded to supplied nongarrison objects. No actual sale destruction, garrison release, full AI/world schedule or total-frame RNG equivalence is certified.'],
        substitutions=['Existing VM/image/heap/CRT/TLS/cached-INI transports only for Event/Action/Factory/Country readers; no selected predicate or Action9 return is supplied. Strategy dependency callbacks and the existing Update Strategy return remain recorded supplied seams; genuine-RNG mode replaces only the old answer-injection interface, never original RNG instructions or returns.'],
        entry_points=dict(has_occurred=0x71E940,no_factories=0x71EF3F,state_a=0x71F950,
            state_b=0x71F9C0,action_dispatch=0x6DD8B0,action9=0x6DEAD6,
            action9_mode_write=0x6DEAFF,find_house=0x6E45E0,building_type_constructor=0x45DD90,
            factory_field=0x46051A,read_factory=0x474FF0,abstract_type_from_name=0x40DCE0,
            increment_factory_count=0x4FFA50,recount_factory=0x4FF980,get_factory_count=0x500910,
            country_registration=0x668C8B,house_country=0x475540,trigger_country=0x727240,
            strategy=0x4FD500,strategy_update=0x4F8FBE,fire_sale=0x5013A0,
            all_to_hunt=0x501400,random_seed=0x65C6D0,random_ranged=0x65C7E0))
    result['environment']={
        'RA2_DIR':'retail installation containing mapsmd03.mix and a supported original gamemd.exe',
        'VERA20K_CAMPAIGN_START_ASSETS':'an existing EMPTY directory for CampaignReader transport; do not point it at the game installation',
        'checkout_prerequisite':'ini/rulesmd.ini is the retained retail RULESMD winner',
        'python':'Python with the repository native-oracle dependencies; local execution used Python3.13'}
    result['reused_evidence']={
        'house_tracking':'tools/spatial_oracle/house_tracking.json and its recorded fixture/coverage; not replayed or extended by this mode',
        'building_lists':'existing House/base ownership and native Unlimbo/PointerExpired/ChangeOwner instruction evidence; not a new lifecycle execution',
        'strategy':'tools/ai_strategy_oracle.json; selected payloads replayed, original sidecar retained',
        'rng':'tools/rmg_oracle/gen_rng_vectors.py original Seed/Ranged constants and raw-store observation boundary'}
    return result


def no_factories_event_source_paths():
    root=Path(__file__).parents[1]
    return power_event_source_paths() | {
        'action_owner':Path(__file__).with_name('trigger_action_values.py'),
        'factory_reader':Path(__file__).with_name('engineer_bridge_cursor_caller.py'),
        'campaign_reader':root/'input_oracle/campaign_start.py',
        'strategy_owner':root/'ai_strategy_oracle.py',
        'strategy_transport':root/'ai_base_building_oracle.py',
        'superweapon_owner':root/'superweapon_oracle.py',
        'rng_owner':root/'rmg_oracle/gen_rng_vectors.py',
        'mix_transport':root/'sidebar_oracle/stock.py'}




def build_type_record_controls():
    """Whole Record_Last_Built on the existing reader/Counter/observer owners."""
    from types import SimpleNamespace
    from tools.input_oracle.campaign_start import CampaignReader
    from tools.spatial_oracle.engineer_capture_accounting import AccountingObservation
    from tools.spatial_oracle.building_body_rules import INI
    from tools.sidebar_oracle.stock import mix, mix_hash
    from tools.rules_oracle.bridge_child_sound import sections
    from tools.native_oracle import image_bytes
    from tools.native_inspect import selected_ranges, decode_ranges, instruction_row
    m=CampaignReader();u=m.u
    m.invoke(0x6832C0,m.scenario)
    rules=m.alloc(0x2000);u.mem_write(0x8871E0,dwords(rules));m.invoke(0x665650,rules)
    m.seed_streams(31)
    country=m.alloc(0x300);m.invoke(0x5113F0,country,(m.cstring('Control'),))
    house=m.alloc(0x17000);before=m.rng_state();mark=len(m.native_events)
    m.invoke(0x4F54A0,house,(country,))
    slots={'building':0x26C,'infantry':0x270,'vehicle':0x274,'aircraft':0x278}
    constructor=dict(entry='0x004F54A0',
        input=dict(rng_seed=31,country_ctor_name='Control',rules_constructor=True),
        last_built={name:struct.unpack('<i',u.mem_read(house+offset,4))[0]
                    for name,offset in slots.items()},
        rng_full_before=before,rng_full_after=m.rng_state(),
        native_events=list(m.native_events[mark:]))
    # Read only the actual stock prefix needed for ALL07's literal index7.
    # Keep Reader.invoke's original2M limit; no complete catalog is attempted.
    raw_rules=(Path('ini')/'rulesmd.ini').read_bytes()
    physical=sections(raw_rules)
    first_eight=dict(list(physical['BuildingTypes'].items())[:8])
    assert len(first_eight)==8
    u.mem_write(0xA83C68,dwords(0x7EB6D4,m.alloc(64*4),64,1,0,10))
    prefix_before_count=m.read32(0xA83C78)
    m.ordered_ini({'BuildingTypes':first_eight});m.invoke(0x672660,rules,(INI,))
    array,count=m.read32(0xA83C6C),m.read32(0xA83C78)
    registered=[dict(index=i,name=m.string(m.read32(array+4*i)+0x24),
        stored_index=m.read32(m.read32(array+4*i)+0xDF8)) for i in range(count)]
    assert count==8
    prefix=dict(entry='0x00672660',prior_count=prefix_before_count,
        physical_rules_sha256=hashlib.sha256(raw_rules).hexdigest(),
        physical_prefix=first_eight,registered=registered)
    raw_archive=(Path(os.environ['RA2_DIR'])/'mapsmd03.mix').read_bytes()
    raw_map=mix(raw_archive)[mix_hash('ALL07SMD.MAP')]
    values=sections(raw_map);identity,tag_id='0C028AFC','0C029C8C'
    from tools.spatial_oracle.trigger_event_records import execute as read_events
    retail=dict(map='ALL07SMD.MAP',map_sha256=hashlib.sha256(raw_map).hexdigest(),
        archive='mapsmd03.mix',archive_sha256=hashlib.sha256(raw_archive).hexdigest(),
        trigger_id=identity,trigger=values['Triggers'][identity],
        tag_id=tag_id,tag=values['Tags'][tag_id],raw_event=values['Events'][identity],
        native_event=read_events(values['Events'][identity]),actions=values['Actions'][identity],
        selected_index7=registered[7],map_building_types=values.get('BuildingTypes',{}),
        langrule_present=(Path('ini')/'langrule.ini').exists())
    # Use real stock prefixes for all selected classes. Standalone MTNK/ORCA
    # constructors would assign fixture-local0, unlike their retail indices.
    # APACHE is the native AircraftTypes first entry but has no physical section
    # (and no Rust ObjectType); the first7 native entries reach PDPLANE/index6.
    # Concrete actors below remain supplied layout priors, not constructors.
    types={'building':m.read32(array+7*4)}
    stock_type_prefixes={}
    for category,entry,section,registry,prefix_count in (
        ('vehicle',0x672360,'VehicleTypes',0xA83CE0,1),
        ('infantry',0x672280,'InfantryTypes',0xA8E348,1),
        ('aircraft',0x6723D0,'AircraftTypes',0xA8B218,7)):
        u.mem_write(registry,dwords(0x7EB6D4,m.alloc(64*4),64,1,0,10))
        physical_prefix=dict(list(physical[section].items())[:prefix_count])
        assert len(physical_prefix)==prefix_count
        prior_count=m.read32(registry+16)
        m.ordered_ini({section:physical_prefix});m.invoke(entry,rules,(INI,))
        actual_array,count=m.read32(registry+4),m.read32(registry+16)
        assert count==prefix_count
        actual=[]
        for index in range(count):
            pointer=m.read32(actual_array+index*4)
            actual.append(dict(index=index,name=m.string(pointer+0x24),
                array_index=m.invoke(m.read32(m.read32(pointer)+0x40),pointer),
                stored_index=m.read32(pointer+0xDF8)))
        types[category]=m.read32(actual_array+(count-1)*4)
        assert actual[-1]['name'] in physical
        stock_type_prefixes[category]=dict(entry=f'0x{entry:08X}',section=section,
            prior_count=prior_count,physical_prefix=physical_prefix,registered=actual,
            selected_index=actual[-1]['array_index'],selected_name=actual[-1]['name'])
    type_defaults={category:dict(name=m.string(p+0x24),vtable=f'0x{m.read32(p):08X}',
        array_index=m.invoke(m.read32(m.read32(p)+0x40),p),
        dont_score=u.mem_read(p+0xC9F,1)[0],
        create_sound=struct.unpack('<i',u.mem_read(p+0x534,4))[0]) for category,p in types.items()}
    assert all(0<=row['array_index']<8 for row in type_defaults.values())
    sound_defaults={hex(offset):struct.unpack('<i',u.mem_read(rules+offset,4))[0]
                    for offset in (0x178,0x17C,0x180)}
    for offset in (0x178,0x17C,0x180):u.mem_write(rules+offset,dwords(-1))
    for pointer in types.values():u.mem_write(pointer+0x534,dwords(-1))
    counter_offsets={'building':0x55A0,'vehicle':0x55B4,'infantry':0x55C8,'aircraft':0x55DC}
    counter_slots={category:m.alloc(8*4) for category in counter_offsets}
    actors,virtuals={},{}
    image=image_bytes()
    for category,vtable,type_offset in (
        ('building',0x7E3EBC,0x520),('vehicle',0x7F5C70,0x6C4),
        ('infantry',0x7EB058,0x6C0),('aircraft',0x7E22A4,0x6C4)):
        actor=m.alloc(0x1000);u.mem_write(actor,bytes(0x1000))
        u.mem_write(actor,dwords(vtable));u.mem_write(actor+type_offset,dwords(types[category]))
        u.mem_write(actor+0x21C,dwords(house));actors[category]=actor
        targets={label:m.read32(vtable+offset) for label,offset in (('what_am_i',0x2C),('get_type',0x84))}
        targets['type_array_index']=m.read32(m.read32(types[category])+0x40)
        assert m.invoke(targets['get_type'],actor)==types[category]
        virtuals[category]=dict(vtable=f'0x{vtable:08X}',supplied_type_offset=hex(type_offset),
            original_what_am_i=m.invoke(targets['what_am_i'],actor),
            entries={label:hex(pc) for label,pc in targets.items()},
            code=[dict(entry=hex(pc),**decode_ranges(selected_ranges(image,pc,24,code_only=False),
                  lambda ins:[instruction_row(ins)])) for pc in targets.values()])
    # Adapt only the reader's buffer/check API to the single accounting observer.
    # No copied native writer, Counter, allocator, RNG or callee-result seam.
    def unchanged():
        m.check_code()
        return True
    observed,draws,advances,events=[],[],[],[]
    facade=SimpleNamespace(u=u,read32=m.read32,rng=m.rng_state,
        trace=observed,events=events,draws=draws,advances=advances,pending=[],code_unchanged=unchanged)
    watched_pcs=(0x4FB6B0,0x49FA00,0x7509E0,0x7258D0,0x65C780,0x65C7E0,0x661C10)
    def observe(_u,pc,size,data):
        if pc in watched_pcs:
            row=dict(pc=f'0x{pc:08X}',receiver=u.reg_read(UC_X86_REG_ECX))
            observed.append(row)
            if pc in (0x65C780,0x65C7E0,0x661C10):draws.append(row)
            if pc==0x65C780:advances.append(row)
            if pc==0x7258D0:events.append(row)
    hook=u.hook_add(UC_HOOK_CODE,observe)
    def state():
        return dict(last_built={category:struct.unpack('<i',u.mem_read(house+offset,4))[0]
                               for category,offset in slots.items()},
            built_counters={category:dict(total=m.read32(house+offset+0x10),
                items=list(struct.unpack('<8I',u.mem_read(counter_slots[category],32))))
                for category,offset in counter_offsets.items()},
            notification246=u.mem_read(house+0x246,1)[0],recheck1fc=u.mem_read(house+0x1FC,1)[0],
            notification_timer=list(struct.unpack('<3i',u.mem_read(house+0x57A4,12))))
    controls=[]
    for category in ('building','vehicle','infantry','aircraft'):
        for dont_score in (False,True):
            for offset in slots.values():u.mem_write(house+offset,dwords(-1))
            for label,offset in counter_offsets.items():
                u.mem_write(house+offset+4,dwords(counter_slots[label],8))
                u.mem_write(house+offset+0x10,dwords(0));u.mem_write(counter_slots[label],bytes(32))
            u.mem_write(house+0x246,b'\0');u.mem_write(house+0x1FC,b'\0')
            u.mem_write(house+0x57A4,dwords(29,17,90))
            u.mem_write(types[category]+0xC9F,bytes([dont_score]));m.seed_streams(31)
            before=state();observation=AccountingObservation(facade,{'owner':house})
            for offset in (*slots.values(),0x57A4,0x57A8,0x57AC):observation.watched[house+offset]='owner'
            mark=len(observed)
            m.invoke(0x4FB6B0,house,(actors[category],))
            after=state()
            required=(0x4FB6B0,) if dont_score else (0x4FB6B0,0x49FA00,0x49FA47,0x49FA50)
            receipt=observation.finish(required)
            assert receipt['rng_before']==receipt['rng_after'] and not receipt['native_draws']
            assert after['notification_timer']==before['notification_timer']
            assert not any(row['pc'] in ('0x007509E0','0x007258D0') for row in observed[mark:])
            controls.append(dict(category=category,dont_score=dont_score,
                native_type_index=type_defaults[category]['array_index'],before=before,after=after,
                observation=receipt))
    u.hook_del(hook);m.check_code()
    record=dict(prefix=prefix,stock_type_prefixes=stock_type_prefixes,
        type_constructor_defaults=type_defaults,
        rules_constructor_create_sound_defaults=sound_defaults,virtuals=virtuals,controls=controls)
    return constructor,record,retail


def generate_build_type_events():
    """19..22: native last-built predicates, writers and delayed latch consumer."""
    from types import SimpleNamespace
    from tools.native_oracle import image_bytes, file_span, NATIVE_FPCW
    from tools.input_oracle.campaign_start import CampaignReader
    from tools.spatial_oracle.trigger_action_values import discovery_building_override
    m=PowerEventControls(reader_inputs=(),extra_calls=(0x7258D0,))
    m.SPANS=list(m.SPANS)
    for start,end in ((0x71F950,0x71FA40),(0x65C6D0,0x65C88D)):
        m.SPANS.append((start,end));m.original.append(bytes(m.u.mem_read(start,end-start)))
    scenario=m.arena+0xC0000
    m.u.mem_write(0xA8B230,dwords(scenario))
    for pointer in (scenario+0x218,0x886B88,0xABE890):
        m.fixture.call(0x65C6D0,[31],ecx=pointer)
    view=SimpleNamespace(u=m.u,scenario=scenario)
    before=CampaignReader.rng_state(view)
    controls=[]
    slots={'building':0x26C,'infantry':0x270,'vehicle':0x274,'aircraft':0x278}
    for kind,offset in ((19,0x26C),(20,0x274),(21,0x270),(22,0x278)):
        for name,value,last,house,raised,flag,persistence in (
            ('match',7,7,0,8,False,0),('miss',7,8,0,8,False,0),
            ('negative_match',-1,-1,0,8,False,0),
            ('match_wrong_raise_editor',7,7,0,99,True,0),
            ('incoming_house1_not_first_country',7,7,1,99,False,0),
            ('null_incoming',7,8,None,99,True,0),
            ('miss_retained_one',7,8,0,8,False,1)):
            for pointer in m.houses:m.u.mem_write(pointer+0x26C,dwords(101,102,103,104))
            if house is not None:m.u.mem_write(m.houses[house]+offset,dwords(last))
            priors=[{category:m.integer(pointer+field) for category,field in slots.items()}
                    for pointer in m.houses]
            timer_before=list(struct.unpack('<3i',m.u.mem_read(m.timer,12)))
            m.predicate(f'event{kind}_{name}',f'1,{kind},0,{value}',house_index=house,
                actor_index=2,raised=raised,scenario_init=flag,initial_persistence=persistence)
            row=m.predicates[-1]
            timer_after=list(struct.unpack('<3i',m.u.mem_read(m.timer,12)))
            assert timer_before==timer_after
            assert row['event54']==struct.unpack('<i',dwords(0xABCD1234))[0]
            assert not any(call['pc']=='0x007258D0' for call in row['calls'])
            controls.append(dict(name=row['name'],kind=kind,value=value,last_built=priors,
                incoming_house=house,actor_owner=2,raised=raised,scenario_init=flag,
                initial_persistence=persistence,result=row['result'],persistence=row['persistence'],
                event54=row['event54'],timer_before=timer_before,timer_after=timer_after,
                calls=row['calls']))
    classifiers=[]
    for kind in (19,20,21,22):
        m.u.mem_write(m.event+0x2C,dwords(kind))
        classifiers.append(dict(kind=kind,state_a=m.fixture.call(0x71F950,[],ecx=m.event)&255,
            state_b=m.fixture.call(0x71F9C0,[],ecx=m.event)&255))
    after=CampaignReader.rng_state(view)
    assert before==after
    predicate_rng=dict(full_seed31=before,full_after=after,unchanged=True)
    # INI ALL07 order13,19 becomes this19,13 push-front list. Reuse the
    # existing Register owner; changing House state cannot undo its latch.
    raw='1,13,0,10';m.readers[raw]=m.read_event(raw)
    m.setup_admission(['1,19,0,7',raw],frame=0,timer_start=0,timer_duration=150)
    m.u.hook_add(UC_HOOK_CODE,m.observe_admission)
    for frame,value,name in ((0,7,'build_matches_before_timer'),
                              (149,8,'type_changed_elapsed149'),(150,8,'type_changed_elapsed150')):
        m.u.mem_write(m.houses[0]+0x26C,dwords(value));m.u.mem_write(0xA8ED84,dwords(frame))
        m.register_admission(name,8,repeat=False)
        m.registers[-1].update(frame=frame,last_built_value=value)
    assert CampaignReader.rng_state(view)==before
    assert not any(call['pc']=='0x007258D0' for row in m.registers for call in row['calls'])
    m.assert_unchanged()
    constructor,record,retail=build_type_record_controls()
    discovery=discovery_building_override()
    discovery['writer_bytes']=dict(start='0x0044D5D0',end_exclusive='0x0044D621',
        hex=file_span(image_bytes(),0x44D5D0,0x51)[1].hex())
    return dict(schema_version=1,native_fpcw=f'0x{NATIVE_FPCW:X}',
        country_indices=[0,7,0],controls=controls,classification=classifiers,
        predicate_rng=predicate_rng,register_history=m.registers,
        constructor=constructor,discovery=discovery,record_last_built=record,retail=retail,
        code_unchanged=True)


def build_type_event_metadata():
    result=provenance(
        scope='28 original HasOccurred19..22 controls, eight StateA/StateB calls, three original Register latch steps, real House/Type constructors, existing Building discovery replay, eight whole Record_Last_Built calls and actual ALL07 binding through the first8 stock BuildingTypes entries. Not entire production/placement, full type registry, audio, complete House/Logic/Spring/EVA or campaign parity.',
        assumptions=[
            'PowerEventControls/FlagsFixture owns the unchanged HasOccurred/reader/Register VM. Three supplied Houses have Country0/7/0, independent last-built priors and actor2.19/20/21/22 read incoming House+26C/+274/+270/+278, not Event.value Country; null incoming succeeds without persistence mutation. Signed bit-pattern equality, negative-1, mismatch, wrong raised99/A8ED6B, distinct incoming House and initial persistence0/1 execute. Event+54 and timer remain unchanged; successful nonnull matches set persistence1.',
            'Original StateA71F950 andStateB71F9C0 execute for each of four kinds: four rows/eight calls. Their1/1 result makes successful19 eligible for existing Register completion bits even on a nonrepeating Tag. Actual19/7 then13/10 Register history at0/149/150 records completion1 before the elapsed condition succeeds; changing last-built7 to8 does not undo the latch. Timer[0,0,150] stays unchanged. The existing House8-frame Tag classification/poll owner is not reimplemented or full-world executed here.',
            'Three native65C6D0 constructors seed31; full0x3F4-byte Scenario/Main/MapGen before/after hex is saved for predicates/classifiers and every Record control. Predicate/Register instructions draw no observed RNG or detach. Full House4F54A0 initialization is separate and DOES consume Scenario RNG; constructor before/after and actual events are retained, not claimed zero-draw.',
            'CampaignReader executes existing Scenario6832C0, Rules665650, Country5113F0 and House4F54A0 constructors. Four last-built slots initialize-1. Physical RULESMD first8 BuildingTypes cache entries feed real ReadBuildingTypes672660/constructors under the original2M cap and spare vector capacity. Actual index7 and TypeDF8 are GAWEAP/7. Whole cold/reset catalog, ReadTypeData/ART, discovery-appended types and full physical INI layers remain excluded. Real ALL07 map/archive hashes, Trigger0C028AFC, Tag0C029C8C, counted Events and raw Action21 EVA string bind this ordinary route; Spring/EVA execution remains a Rust production consumer check.',
            'Record inputs use real stock prefixes: GAWEAP/index7 from first8 BuildingTypes, AMCV/index0 from first VehicleTypes via672360, E1/index0 from first InfantryTypes via672280, and PDPLANE/index6 from first7 AircraftTypes via6723D0. Original list readers/constructors/ArrayIndex getters execute and save every registration. Aircraft first entry APACHE has no physical object section; selecting PDPLANE retains its real prefix index instead of treating it as0. This corrects the earlier standalone MTNK/ORCA fixture-local0 inputs; new results are a changed input set, not claimed identical prior controls. Actual instance vtables, type-pointer offsets and owner layouts remain supplied priors, not full Actor constructors/Unlimbo/tracking/PLACE. Original WhatAmI/GetTechnoType/TypeArrayIndex virtuals execute. Original House4FB6B0 executes for each DontScore0/1 through existing AccountingObservation. Last-built writes occur for both flags; built Counter49FA00 is skipped only by DontScore. Capacity8, zero-item/total Counter priors exclude allocation/growth.',
            'Type CreateSound and Rules+178/+17C/+180 are explicitly-1; actual constructor defaults are retained separately. No native sound/detach entry occurs in these eight Record bodies, and supplied notification timer[29,17,90] stays unchanged while246/1FC set1. Future House90-frame rearm, audible output/audio RNG and full TechTree effects are outside these scalar calls.',
            'Existing trigger_action_values.discovery_building_override runs actual44D5D0 after shared6F4960 admission, with synthetic TypeDF8=73, priorHouse26C=99 and an already-openedBuilding6E4=1 Grand_Opening fast return. Init2 owner entry preserves99, first current-player discovery atinit0 writes73/246, repeat discovery retains it; exact44D5D0..44D621 bytes are saved. This helper retains its existing three-stream SHA observations, not a new full-state replay claim for that subgraph. Current-player/owner/placement priors, sparse map and existing supplied unrelated calls retain that owner\'s boundaries; no complete first opening, arbitrary discovery or whole-world lifecycle claim.',
            'Record_Last_Built4FB6B0 and Building44D601 are distinct native writers of the same House slot. Production must use one private House state owner, not count-derived last type, presentation visibility or a new direct build-event dispatcher. No existing reader/VM/Counter/allocator/RNG algorithm is duplicated.'],
        substitutions=['Inherited FlagsFixture/CampaignReader/DiscoveryFixture CRT, heap, TLS, platform and cached-INI transports. Explicit House/Actor/vector/counter/timer/type/mission priors retain stated limits. No HasOccurred, Register, last-built writer, Counter increment, virtual type getter or selected RNG return is substituted. Existing discovery helper retains its declared presentation/first-opening seams.'],
        entry_points=dict(has_occurred=0x71E940,building_predicate=0x71EFEB,
            vehicle_predicate=0x71F003,infantry_predicate=0x71F01D,aircraft_predicate=0x71F037,
            state_a=0x71F950,state_b=0x71F9C0,register=0x7264C0,
            house_constructor=0x4F54A0,record_last_built=0x4FB6B0,counter_increment=0x49FA00,
            building_discovery=0x44D5D0,discovery_last_built_store=0x44D601,
            building_types_reader=0x672660,vehicle_types_reader=0x672360,
            infantry_types_reader=0x672280,aircraft_types_reader=0x6723D0,
            random_seed=0x65C6D0))
    result['environment']=dict(RA2_DIR='retail installation containing gamemd.exe and mapsmd03.mix',
        VERA20K_CAMPAIGN_START_ASSETS='existing empty directory; selected constructor/list controls request no assets',
        checkout_prerequisite='ini/rulesmd.ini is the extracted retail RULESMD')
    return result


def build_type_event_source_paths():
    from tools.spatial_oracle.trigger_action_values import SOURCE_PATHS
    paths=source_paths()|power_event_source_paths()
    paths.update({f'discovery_{name}':path for name,path in SOURCE_PATHS.items()})
    paths.update(accounting=Path(__file__).with_name('engineer_capture_accounting.py'),
        native_inspection=Path(__file__).parents[1]/'native_inspect.py',
        retail_mix=Path(__file__).parents[1]/'sidebar_oracle'/'stock.py',
        retail_lexical=Path(__file__).parents[1]/'rules_oracle'/'bridge_child_sound.py')
    return paths




def damage_event_bytes():
    """Pinned original bodies/calls, not a whole-world reachability claim."""
    from tools.native_oracle import image_bytes,file_span
    from tools.native_inspect import selected_ranges,decode_ranges,instruction_row
    image=image_bytes();blocks={}
    for start,end in ((0x5F5390,0x5F584D),(0x71E940,0x71F216),(0x71F950,0x71FA40),
                      (0x701DF8,0x701F94),(0x711EB0,0x711EB7),(0x45ED50,0x45EDCF)):
        raw=file_span(image,start,end-start)[1]
        blocks[f'{start:08X}..{end:08X}']=dict(hex=raw.hex(),sha256=hashlib.sha256(raw).hexdigest())
    callers=[]
    for name,site,target in (
        ('building_to_techno',0x442425,0x701900),('unit_to_foot',0x737D52,0x4D7330),
        ('infantry_to_foot',0x518042,0x4D7330),('aircraft_to_foot',0x4165EC,0x4D7330),
        ('foot_to_techno',0x4D742C,0x701900),('techno_to_object',0x701DF8,0x5F5390)):
        instruction=decode_ranges(selected_ranges(image,site,5,code_only=False),
                                  lambda ins:[instruction_row(ins)])['matches']
        assert len(instruction)==1 and instruction[0]['mnemonic']=='call'
        assert int(instruction[0]['operands'],16)==target
        callers.append(dict(name=name,site=f'0x{site:08X}',target=f'0x{target:08X}',
                            instruction=instruction[0]))
    vtables={}
    for category,vt in (('building',0x7E3EBC),('unit',0x7F5C70),
                        ('infantry',0x7EB058),('aircraft',0x7E22A4)):
        raw=file_span(image,vt+0x16C,4)[1]
        vtables[category]=dict(vtable=f'0x{vt:08X}',slot='0x16C',hex=raw.hex(),
                               entry=f'0x{int.from_bytes(raw,"little"):08X}')
    # Post-Object701E12 obtains the live Type for virtual GetCost(+AC),
    # while701E32/38 independently obtain its current Strength. These are
    # static bytes/dispatch evidence, not a new anger-arithmetic execution.
    type_cost_getters={}
    for category,vt in (('building',0x7E4570),('unit',0x7F6218),
                        ('infantry',0x7EB610),('aircraft',0x7E2868)):
        raw=file_span(image,vt+0xAC,4)[1]
        type_cost_getters[category]=dict(vtable=f'0x{vt:08X}',slot='0xAC',hex=raw.hex(),
            entry=f'0x{int.from_bytes(raw,"little"):08X}')
    return dict(blocks=blocks,callers=callers,concrete_receivers=vtables,
                type_cost_getters=type_cost_getters)


def damage_event_retail_routes():
    """Actual selected map bindings through existing MIX/cache/counted reader."""
    from tools.sidebar_oracle.stock import mix,mix_hash
    from tools.rules_oracle.bridge_child_sound import sections
    from tools.spatial_oracle.trigger_event_records import execute as read_events
    archive=(Path(os.environ['RA2_DIR'])/'mapsmd03.mix').read_bytes();members=mix(archive)
    routes=[]
    for name,identity in (('ALL05UMD.MAP','077835AC'),('ALL06UMD.MAP','077B8E1C')):
        raw=members[mix_hash(name)];values=sections(raw)
        # Include both directions of the authored linked-list references.
        # Tag48BC38C reaches43 through08F91C4C, not a direct Event43 Tag.
        linked={identity};previous=set()
        while linked!=previous:
            previous=set(linked)
            for key,definition in values['Triggers'].items():
                successor=definition.split(',')[1]
                if key in linked and successor in values['Triggers']:linked.add(successor)
                if successor in linked:linked.add(key)
        triggers={key:dict(trigger=values['Triggers'][key],raw_event=values['Events'][key],
            native_event=read_events(values['Events'][key]),actions=values['Actions'][key])
            for key in sorted(linked)}
        tags={key:definition for key,definition in values['Tags'].items()
              if any(trigger in definition.split(',') for trigger in linked)}
        structures={key:definition for key,definition in values['Structures'].items()
                    if any(tag in definition.split(',') for tag in tags)}
        row=dict(map=name,map_sha256=hashlib.sha256(raw).hexdigest(),
            archive='mapsmd03.mix',archive_sha256=hashlib.sha256(archive).hexdigest(),
            trigger_id=identity,trigger=values['Triggers'][identity],
            raw_event=values['Events'][identity],native_event=read_events(values['Events'][identity]),
            actions=values['Actions'][identity],linked_triggers=triggers,tags=tags,structures=structures)
        if name=='ALL05UMD.MAP':
            team_id='0771D13C';team=values[team_id]
            row.update(team_id=team_id,team=team,task_force_id=team['TaskForce'],
                task_force=values[team['TaskForce']],script_id=team['Script'],script=values[team['Script']])
        routes.append(row)
    return routes


def damage_event_object_destruction_controls():
    """Base Object disposal boundary; original bytes decide Tag retention."""
    setup=[['shared'],['object'],['attach_object',0,0]]
    cases=[dict(name='base_object_last_tag_reference',operations=setup+
        [['object_alive',0,0],['supply_pending',[['object',0]]],['drain'],['drain']]),
        dict(name='base_objects_share_tag',operations=[['shared'],['object'],['object'],
            ['attach_object',0,0],['attach_object',1,0],['object_alive',0,0],
            ['supply_pending',[['object',0]]],['drain'],['object_alive',1,0],
            ['supply_pending',[['object',1]]],['drain'],['drain']]),
        dict(name='constructor_complete_tagged_scalar_disposal',operations=setup+
            [['dispose_object',0],['drain'],['drain']])]
    rows=[];rng=None
    for case in cases:
        row=execute(dict(case,mixed_deferred_queue=True,lifecycle=True,repeat=2,
            observe_object_cleanup=True))
        before=row.pop('full_rng_before');after=row.pop('full_rng_after_history')
        assert before==after
        if rng is None:rng=before
        assert before==rng
        row.update(rng_before_ref='object_destruction_rng.full_seed31',
            rng_after_ref='object_destruction_rng.full_seed31')
        rows.append(row)
    return rows,dict(full_seed31=rng,full_after=rng,unchanged=True)


def generate_damage_events():
    """Combined38..43 evidence; reuse receiver, reader and Register owners."""
    from types import SimpleNamespace
    from tools.native_oracle import NATIVE_FPCW
    from tools.input_oracle.campaign_start import CampaignReader
    from tools.spatial_oracle.object_health import (damage_event_receiver_controls,
                                                  damage_event_techno_anger_controls)
    receiver,mutations,receiver_rng=damage_event_receiver_controls()
    anger,anger_rng=damage_event_techno_anger_controls()
    assert anger_rng==receiver_rng
    destruction,destruction_rng=damage_event_object_destruction_controls()
    m=PowerEventControls(reader_inputs=(),extra_calls=(0x7258D0,0x6E4F60))
    m.SPANS=[*m.SPANS,(0x71F950,0x71FA40),(0x65C6D0,0x65C88D)]
    m.original.extend(bytes(m.u.mem_read(start,end-start)) for start,end in m.SPANS[-2:])
    scenario=m.arena+0xC0000;m.u.mem_write(0xA8B230,dwords(scenario))
    for ptr in (scenario+0x218,0x886B88,0xABE890):m.fixture.call(0x65C6D0,[31],ecx=ptr)
    rng_view=SimpleNamespace(u=m.u,scenario=scenario);before=CampaignReader.rng_state(rng_view)
    controls=[];classification=[]
    for kind in range(38,44):
        raw=f'1,{kind},0,2147483647'
        for name,raised,flag,persistence,house,actor,source in (
            ('matching',kind,False,0,0,None,None),('wrong_raise',8,False,0,0,1,1),
            ('bypass',kind,True,0,0,1,1),('null_house',kind,False,0,None,None,None),
            ('initial1_wrong_raise',8,False,1,None,None,None)):
            timer_before=list(struct.unpack('<3i',m.u.mem_read(m.timer,12)))
            # The shared predicate supplies this same sentinel at entry;
            # retain its actual prior field value alongside the output.
            m.u.mem_write(m.event+0x54,dwords(0xABCD1234));event54_before=m.integer(m.event+0x54)
            m.predicate(f'{kind}_{name}',raw,house_index=house,raised=raised,
                actor_index=actor,source_index=source,scenario_init=flag,initial_persistence=persistence)
            native=m.predicates[-1];timer_after=list(struct.unpack('<3i',m.u.mem_read(m.timer,12)))
            assert timer_before==timer_after and native['persistence']==persistence
            assert native['event54']==struct.unpack('<i',dwords(0xABCD1234))[0]
            assert CampaignReader.rng_state(rng_view)==before
            assert not any(row['pc'] in ('0x007258D0','0x006E4F60') for row in native['calls'])
            controls.append(dict(name=native['name'],kind=kind,value=native['read']['value'],
                raw=raw,read=native['read'],incoming_house=house,actor_owner=actor,source_owner=source,
                raised=raised,scenario_init=flag,initial_persistence=persistence,
                result=native['result'],persistence=native['persistence'],
                event54_before=event54_before,event54=native['event54'],
                timer_before=timer_before,timer_after=timer_after,calls=native['calls']))
        m.u.mem_write(m.event+0x2C,dwords(kind))
        classification.append(dict(kind=kind,state_a=m.fixture.call(0x71F950,[],ecx=m.event)&255,
                                   state_b=m.fixture.call(0x71F9C0,[],ecx=m.event)&255))
    m.u.hook_add(UC_HOOK_CODE,m.observe_admission)
    raw='1,38,0,0';m.readers[raw]=m.read_event(raw)
    m.setup_admission([raw],timer_start=90,timer_duration=75)
    for name,raised in (('first',38),('later_wrong',8),('same_again',38)):
        m.register_admission(name,raised)
        assert CampaignReader.rng_state(rng_view)==before
    m.assert_unchanged()
    assert not any(call['pc'] in ('0x007258D0','0x006E4F60')
                   for row in m.registers for call in row['calls'])
    return dict(schema_version=1,native_fpcw=f'0x{NATIVE_FPCW:X}',
        receiver_controls=receiver,mutation_controls=mutations,receiver_rng=receiver_rng,
        techno_anger_controls=anger,
        object_destruction_controls=destruction,object_destruction_rng=destruction_rng,
        controls=controls,classification=classification,register_history=m.registers,
        predicate_rng=dict(full_seed31=before,full_after=CampaignReader.rng_state(rng_view),unchanged=True),
        retail_routes=damage_event_retail_routes(),bytes=damage_event_bytes(),code_unchanged=True)


def damage_event_metadata():
    from tools.spatial_oracle.object_health import OBJ,TYPE,SOURCE,TAG,DamageEventsObservation
    result=provenance(
        scope='Original Object damage Tag38..43:18 ordinary and23 declared callee-mutation receiver controls;eight post-Object Techno anger arithmetic fragments;three base Object destruction/Tag-retention histories;30 HasOccurred,12 StateA/B calls,three single-event Register steps and actual ALL05/ALL06 bindings. No real damage Tag/action chain, full Techno/concrete receiver, production Team or whole-campaign parity.',
        assumptions=[
            'The existing object_health.Fixture owns whole unchanged Object5F5390, signed HP/packet and masked PC53/chop arithmetic, copied Unit/Building RTTI/type getters, Changed/Kill/Destroy and Tag6E53A0 recording sinks. Receiver controls supply Strength/HP, ConditionRed=.25, Alive, source/tag storage, ignoreDefenses1/preventEscape1, distance0 and null warhead/sourceHouse. The packet is separate, not aliased Health. No armor/kernel, concrete receiver or supplied callee body is certified.',
            'All23 mutation rows preserve their names, explicit return-PC/write inputs, original ordered callee returns and live/saved HP observations. Host writes after actual Tag/Kill return and before resumed original instructions provide HP/Alive/attachment/Strength priors. They do not execute a real Tag action, RecordKill, Destroy or detach algorithm; existing kill_clear_alive/destroy_clear_alive sinks retain their prior definitions. Original WhatAmI/type getters and receiver instructions are not supplied.',
            'Three original65C6D0 seed31 states are saved in receiver_rng.full_seed31 and full_after; each of41 receiver rows compares all full states before/after to the shared reference, without repeating6KiB per row. Receiver source pointer0 meansNULL and the published SOURCE address means the supplied source object; it is not a House projection. Observed65C780/65C7E0/7258D0/6E4F60 entries are absent in these Object bodies; inherited callback sinks have no such work. This is not a zero-RNG/timer/detach claim for real actions or complete death.',
            'PowerEventControls/FlagsFixture and the original counted reader execute six kinds with matching/wrong raises, A8ED6B true, null incoming House/actor/source, arbitrary valueINT_MAX and initial persistence0/1. The historical scenario_init key denotes A8ED6B, not placement A8E7AC. Each timer12bytes, Event54 sentinel and persistence stay unchanged, and each full3-stream state is checked. GetStateA/GetStateB execute six rows/twelve calls; both return1. The separate three Register single-event controls do not demonstrate partial multi-event latching; completed stays0 and timer[90,0,75] stays unchanged.',
            'Static original bytes pin whole Object/HasOccurred/classifiers, four real receiver vtable entries and six direct class/base callers. They establish shared instruction routes and boundaries, not exhaustive reachability or execution of all four concrete receivers. Body arithmetic/ordering claims rely on the saved native outputs, not Ghidra annotations or handwritten expected values.',
            'Additional static bytes701DF8..701F94 pin the post-Object consumer:701E12 obtains current Type for GetCost,701E23 reads source Owner,701E32/38 re-read Type/Strength,701E4A calls Type vtable+AC,701E61 reads victim Owner and701E68 immediately calls existing UpdateAngerNodes504790. This precedes result4 CausesDelayKill/PostMortem and result5 early return; sourceHouse alone does not replace the Attacker pointer gate. Type vtable+AC data and complete getter bytes pin ordinary TechnoType711EB0 and BuildingType45ED50 (existing type_cost/building_actual_cost authority). Those byte rows are static control-flow evidence; the separate techno_anger_controls below execute the ordinary cost arithmetic. No real House update, PostMortem timer, audio/Uncloak or whole-Techno execution is claimed, and all earlier receiver/predicate numeric controls remain unchanged.',
            'Separately,eight techno_anger_controls execute actual701DFD..701E68 through existing Object Fixture: EAX Object result, ESI receiver, EBX independent packet, stack+D4 source pointer, source/victim Owner pointers and UnitType fields are supplied fragment priors. Actual Unit vt+84 getters, Type vt+AC GetCost711EB0 and originalMath::ftol7C5F00 execute; Python supplies no arithmetic output. Stop at real504790 entry before its House body, recording actual ECX, stackdelta/sourceHouse, returnPC and packet. Rows cover ordinary51/100/700, liveStrength110, negativeheal, zero packet/Strength, nonintegral and wide signed dwords under0E7F. Full three-stream states compare to shared receiver_rng before/after each row. Supplied radar/reveal timers atObject+174/+1E0 remain unchanged; sticky x87 status is observed, not claimed unchanged. These fragments establish the arithmetic/call arguments only, not actual Tag mutation->wholeTechno->House execution, alternate Building GetCost calculation, all numeric inputs, PostMortem/audio or full world. Existing ai_strategy_oracle owns separate504790 node-update execution.',
            'Three object_destruction_controls reuse the single execute lifecycle owner: original Object5F3900 constructor and Attach5F5B50, supplied Object+90/pending queue for two full725C70 histories and actual5F6DC0 scalar disposal for a constructor-complete object. Actual full Object5F3B80 destructor does not clearObject+34 or decrementTag+2C. Tag refs remain1 for one attachment and2 after two shared objects are freed; no Tag/Trigger ticket or release is invented. Original allocator/free imports retain bytes for observation: object_tag_words and tag_at_free report actual+34 including after release, while legacy object_tags filters freed objects toNone. These rows are a base-Object boundary, not full Techno/Building destruction, native death or a mandate to retain dead Rust index entries. Repeat2 Tag definitions and original Trigger timers are observed; no damage action or Tag event is executed. Full three-stream seed31 states compare at every saved step through shared CampaignReader.rng_state, and the terminal next_random probe remains outside history/RNG-after. No general zero-timer/detach claim extends past these executed bodies.',
            'Retail mapsmd03.mix bytes and member hashes identify ALL05UMD and ALL06UMD. Existing MIX/lexical cache owners supply actual Trigger/Tag/Structures/actions and ALL05 Team/TaskForce/Script; original counted Event reader executes every retained linked Trigger event. Raw reference extraction is not full native Trigger/Team/Structure construction, scenario admission, Rules/ART layers or physical loader execution. ALL05 first-damage41 creates the existing defensive Team, it does not necessarily create fresh soldiers. ALL06 repeating Parliament Tag enters43 through its linked chain and invokes54/32/53/112, a real nested-damage risk whose world execution is outside this corpus.'],
        substitutions=['Inherited Object Fixture Tag/Changed/Kill/Destroy callout sinks; named controlled field mutations at genuine call-return seams. Existing FlagsFixture CRT/TLS/allocation and supplied House/actor/trigger priors. No selected original predicate, classification, Register, receiver, type getter or RNG return is replaced.'],
        entry_points=dict(object_receive=0x5F5390,tag_callout=0x6E53A0,has_occurred=0x71E940,
            state_a=0x71F950,state_b=0x71F9C0,register=0x7264C0,random_seed=0x65C6D0,
            tag_before_kill_guard=0x5F5752,exact_zero=0x5F5765,
            post_object_anger=0x701E68,update_anger_nodes=0x504790,
            techno_type_get_cost=0x711EB0,building_type_get_cost=0x45ED50))
    result['receiver_supplied_addresses']=dict(object=f'0x{OBJ:08X}',type=f'0x{TYPE:08X}',
        source=f'0x{SOURCE:08X}',tag=f'0x{TAG:08X}',source_null=0)
    result['tag_return_pcs']={f'0x{pc:08X}':kind for pc,kind in DamageEventsObservation.TAG_RETURNS.items()}
    result['environment']=dict(RA2_DIR='retail installation containing gamemd.exe and mapsmd03.mix',
        interpreter='Python compatible with installed Unicorn; local replay uses Python3.13',
        asset_directory='No CampaignReader VM is constructed or eager asset directory loaded by this mode')
    return result


def damage_event_source_paths():
    paths=power_event_source_paths()
    paths.update(object_health=Path(__file__).with_name('object_health.py'),
        rng_state_reader=Path(__file__).parents[1]/'input_oracle'/'campaign_start.py',
        native_inspection=Path(__file__).parents[1]/'native_inspect.py',
        retail_mix=Path(__file__).parents[1]/'sidebar_oracle'/'stock.py',
        retail_lexical=Path(__file__).parents[1]/'rules_oracle'/'bridge_child_sound.py')
    return paths


if __name__=='__main__':
    import sys
    args=sys.argv[1:]
    if '--periodic-cell-events' in args:
        args.remove('--periodic-cell-events')
        finish_vectors(generate_periodic_cell_events,Path(__file__).with_name('trigger_periodic_cell_events.json'),
                       provenance=periodic_cell_event_metadata,source_paths=periodic_cell_event_source_paths(),argv=args)
    elif '--spy-entry-events' in args:
        args.remove('--spy-entry-events')
        finish_vectors(generate_spy_entry_events,Path(__file__).with_name('trigger_spy_entry_events.json'),
                       provenance=spy_entry_event_metadata,source_paths=spy_entry_event_source_paths(),argv=args)
    elif '--damage-events' in args:
        args.remove('--damage-events')
        finish_vectors(generate_damage_events,Path(__file__).with_name('trigger_damage_events.json'),
                       provenance=damage_event_metadata,source_paths=damage_event_source_paths(),argv=args)
    elif '--build-type-events' in args:
        args.remove('--build-type-events')
        finish_vectors(generate_build_type_events,Path(__file__).with_name('trigger_build_type_events.json'),
                       provenance=build_type_event_metadata,source_paths=build_type_event_source_paths(),argv=args)
    elif '--no-factories-events' in args:
        args.remove('--no-factories-events')
        finish_vectors(generate_no_factories_events,Path(__file__).with_name('trigger_no_factories_events.json'),
                       provenance=no_factories_event_metadata,source_paths=no_factories_event_source_paths(),argv=args)
    elif '--naval-land-events' in args:
        args.remove('--naval-land-events')
        finish_vectors(generate_naval_land_events,Path(__file__).with_name('trigger_naval_land_events.json'),
                       provenance=naval_land_event_metadata,source_paths=naval_land_event_source_paths(),argv=args)
    elif '--loss-threshold-events' in args:
        args.remove('--loss-threshold-events')
        finish_vectors(generate_loss_threshold_events,Path(__file__).with_name('trigger_loss_threshold_events.json'),
                       provenance=loss_threshold_event_metadata,source_paths=power_event_source_paths(),argv=args)
    elif '--funds-events' in args:
        args.remove('--funds-events')
        finish_vectors(generate_funds_events,Path(__file__).with_name('trigger_funds_events.json'),
                       provenance=funds_event_metadata,source_paths=power_event_source_paths(),argv=args)
    elif '--building-loss-events' in args:
        args.remove('--building-loss-events')
        finish_vectors(generate_building_loss_events,Path(__file__).with_name('trigger_building_loss_events.json'),
                       provenance=building_loss_event_metadata,source_paths=power_event_source_paths(),argv=args)
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
