"""Bounded original Shrapnel ground-surface repair naval admission/damage.

Research fixture. Cells/actor/owner/list membership are supplied projections,
not a native scenario load or proof that an authored map spawns these ships.
No gameplay callable is replaced in the measured controller/receiver prefix.
Ordinary rows retain constructor IsOnMap=0; two IsOnMap=1 controls stop before
the post-death Mark call. Full Unlimbo, marked-world cleanup and drawing are
outside this fixture. A returned sinking row is not a whole-game lifetime proof.
"""
from pathlib import Path
import hashlib, json, os, struct

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
from unicorn.x86_const import *
from tools.native_oracle import RET_MAGIC, run_checked, finish_vectors, provenance
from tools.rules_oracle.bridge_landing_inputs import Landing, TYPE as RULES, INI, SP, WH, RULES as RULE_BLOCKS
from tools.projectile_oracle.bridge_render_inputs import lexical
from tools.spatial_oracle.map_queries import dwords, packed

HERE=Path(__file__).resolve().parent
ASSETS=Path(os.environ.get('VERA20K_SHRAPNEL_INPUTS', 'target/shrapnel-native-inputs/extract'))
MAP=0x87F7E8

def sha(raw): return hashlib.sha256(raw).hexdigest()
def signed(value): return struct.unpack('<i',dwords(value))[0]

class Native(Landing):
    def __init__(self, case):
        self.phase='setup';self.calls=[];self.pending=[];self.accesses={};self.writes=[];self.seams=[]
        super().__init__()
        u=self.u;self.case=case
        self.block(0x66571B,0x665725,{UC_X86_REG_ESI:RULES})
        # Native UnitType constructor, spare class registry storage.
        u.mem_write(0xA83CE0,dwords(0x7EB6D4,self.alloc(4096),1024,1,0,10))
        self.typ=self.alloc(0xF00)
        self.invoke(0x7470D0,self.typ,(self.cstring(case.get('type','AEGIS')),))
        self.ctor=self.type_state()
        self.land_names=[self.string(self.read32(0x839D68+i*4)) for i in range(12)]
        self.layers=[]
        self.read_inputs()
        self.actor=self.alloc(0x800);self.loco=self.alloc(0x100)
        self.house=self.alloc(0x17000);self.scenario=self.alloc(0x7000)
        u.mem_write(0xA8B230,dwords(self.scenario))
        self.invoke(0x65C6D0,self.scenario+0x218,(1,))
        self.invoke(0x65C6D0,0x886B88,(1,))
        # Execute Unit's base constructors and field initialization, ending
        # before its COM creation branch. The original Ship ctor is separate.
        u.mem_write(0xB0F720,dwords(0x7EB6D4,self.alloc(4096),1024,1,0,10))
        u.mem_write(SP,dwords(RET_MAGIC,self.typ,0));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,self.actor)
        run_checked(u,0x7353C0,0x7354CE,count=300000)
        self.actor_ctor=dict(flags=self.read32(self.actor+0x14),radio_capacity=self.read32(self.actor+0xE8),
                             vector470_vtable=hex(self.read32(self.actor+0x470)),boundary='0x7354ce')
        self.invoke(0x69EC50,self.loco,())
        assert self.read32(self.loco+4)==0x7F2D8C
        self.imports={self.read32(0x7E11C8):1,self.read32(0x7E11CC):-1}
        u.mem_write(self.loco+0xC,dwords(self.actor));u.mem_write(self.loco+0x14,dwords(1))
        u.mem_write(self.actor,dwords(0x7F5C70));u.mem_write(self.actor+0x6C4,dwords(self.typ))
        assert self.read32(self.actor+0x14)==7
        u.mem_write(self.actor+0x21C,dwords(self.house))
        u.mem_write(self.actor+0x674,dwords(self.loco+4));u.mem_write(self.actor+0x684,b'\xff')
        u.mem_write(self.actor+0x6C,dwords(case.get('health',self.read32(self.typ+0xA0))))
        u.mem_write(self.actor+0x90,bytes([case.get('alive',True)]))
        u.mem_write(self.actor+0x81,b'\0')
        u.mem_write(self.actor+0x74,bytes([case.get('is_on_map',False)]))
        u.mem_write(self.actor+0xAC,dwords(5));u.mem_write(self.actor+0xB4,dwords(-1))
        u.mem_write(self.actor+0x3D5,b'\x01');u.mem_write(self.actor+0x6D8,dwords(-1))
        u.mem_write(self.actor+0x338,dwords(-1));u.mem_write(self.actor+0x5E0,b'\xff')
        u.mem_write(self.house+0x30,dwords(0))
        self.house_type=self.alloc(0x200)
        u.mem_write(self.house+0x34,dwords(self.house_type))
        u.mem_write(self.house_type+0x118,struct.pack('<f',1.0))
        u.mem_write(self.house+0x5394,struct.pack('<f',1.0))
        # Empty native BombList primary vector constructor prefix; no bombs.
        self.block(0x40B550,0x40B57D,{UC_X86_REG_EBX:0})
        # Real coordinate; member-cell placement is independently supplied below.
        current=case.get('current',[115*256+128,59*256+128,208])
        u.mem_write(self.actor+0x9C,dwords(*current))
        u.mem_write(self.loco+0x40,dwords(*case.get('head',[0,0,0])))
        u.mem_write(self.loco+0x58,dwords(case.get('turn_index',-1),case.get('cursor',0)))
        u.mem_write(self.loco+0x60,bytes([case.get('reversed',False)]))
        if case.get('iron_curtain'):
            u.mem_write(self.actor+0x18C,dwords(1000,0,50))
        if case.get('warping_out'):u.mem_write(self.actor+0x270,b'\x01')
        if case.get('immune'):u.mem_write(self.typ+0x233,b'\x01')
        if 'speed_override' in case:u.mem_write(self.typ+0x67C,dwords(case['speed_override']))
        if 'weight_override' in case:u.mem_write(self.typ+0x370,struct.pack('<d',case['weight_override']))
        if case.get('underwater'):u.mem_write(self.typ+0xD69,b'\x01')
        if case.get('organic'):u.mem_write(self.typ+0xD97,b'\x01')
        if case.get('being_warped'):u.mem_write(self.actor+0x271,b'\x01')
        u.mem_write(0xA8ED84,dwords(1000));u.mem_write(0xA8E7AC,dwords(0));u.mem_write(0xA8E9A0,b'\x01')
        u.mem_write(0xA8B230,dwords(self.scenario))
        for p in (0x886B88,self.scenario+0x218,0xABE890):
            self.invoke(0x65C6D0,p,(1,))
        self.rngs={'main':0x886B88,'scenario':self.scenario+0x218,'mapgen':0xABE890}
        self.invoke(0x49F2F0,0,())
        for p in (0xB1CFE8,0xB077F8):u.mem_write(p,dwords(0,0,0))
        for p in (0x89E7C0,0xB07838):u.mem_write(p,dwords(104))
        u.mem_write(MAP+0xF4,dwords(82,82,5,6,71,70))
        table=self.alloc(0x100000);u.mem_write(MAP+0x13C,dwords(table,0x40000))
        self.cells={}
        for x in range(111,120):
            for y in range(57,62):
                p=self.alloc(0x200);self.cells[x,y]=p
                u.mem_write(p,dwords(0x7E4EEC));u.mem_write(p+0x24,packed(x,y))
                u.mem_write(p+0x44,dwords(-1));u.mem_write(p+0x54,dwords(-1,-1))
                land=case.get('land',1) if 114<=x<=116 and y==59 else (1 if 114<=x<=116 and y in (58,60) else 2)
                u.mem_write(p+0xEC,dwords(land));u.mem_write(p+0x116,struct.pack('<h',-1))
                u.mem_write(p+0x11B,b'\x02\0')
                u.mem_write(table+(y*512+x)*4,dwords(p))
        target=tuple(case.get('target',[115,59]))
        self.selected=self.cells[target]
        if 'current_land' in case:
            u.mem_write(self.cells[current[0]//256,current[1]//256]+0xEC,dwords(case['current_land']))
        member=tuple(case.get('member',[115,59]))
        u.mem_write(self.cells[member]+0xE4,dwords(self.actor))
        if member==target:u.mem_write(self.selected+0x124,dwords(0x20))
        if case.get('nonfoot'):u.mem_write(self.actor+0x14,dwords(1))
        self.read_ranges=[('actor',self.actor,0x800),('type',self.typ,0xF00),('house',self.house,0x17000),('loco',self.loco,0x100),('rules',RULES,0x2000),('warhead',self.warhead,0x180)]
        self.u.hook_add(UC_HOOK_MEM_READ,self.read_hook)
        self.u.hook_add(UC_HOOK_MEM_WRITE,self.write_hook)
        self.code_spans=[(0x487A10,0x487C15),(0x73F0A0,0x73FC30),(0x4D9C10,0x4D9FA0),(0x6A3F50,0x6A40BA),(0x737C90,0x7385DE),(0x4D7330,0x4D7550),(0x701900,0x702D40),(0x5F5390,0x5F5850)]
        self.code_before=[bytes(u.mem_read(a,b-a)) for a,b in self.code_spans]
        self.phase='measure'

    def block(self,a,b,regs):
        self.u.reg_write(UC_X86_REG_ESP,SP)
        for reg,val in regs.items():self.u.reg_write(reg,val)
        run_checked(self.u,a,b,count=500000)

    def type_state(self):
        p=self.typ;u=self.u
        return {'name':self.string(p+0x24),'speed':signed(self.read32(p+0x67C)),
                'strength':signed(self.read32(p+0xA0)),'armor':signed(self.read32(p+0x9C)),
                'immune':u.mem_read(p+0x233,1)[0], 'movement_zone':signed(self.read32(p+0x5B4)),
                'weight':struct.unpack('<d',u.mem_read(p+0x370,8))[0],
                'is_train':u.mem_read(p+0xC94,1)[0],'crew':u.mem_read(p+0xCCD,1)[0],
                'naval':u.mem_read(p+0xCCE,1)[0],'underwater':u.mem_read(p+0xD69,1)[0],
                'organic':u.mem_read(p+0xD97,1)[0], 'explodes':u.mem_read(p+0xD15,1)[0],
                'death_frames':signed(self.read32(p+0xE20)), 'restricted_land':signed(self.read32(p+0xDFC))}

    def read_inputs(self):
        u=self.u;t=self.typ
        for file in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','XShrapnel.MAP'):
            path=ASSETS/file
            if not path.exists():
                assert file=='LANGRULE.INI',file
                self.layers.append(dict(file=file,absent=True));continue
            raw=path.read_bytes();sections,lines=lexical(raw,set(self.land_names)|{self.case.get('type','AEGIS'),'CombatDamage','AudioVisual','General','Super'})
            self.make_ini(sections)
            self.invoke(0x674000,0,(INI,))
            for key in ('C4Warhead','ConditionRed','ConditionYellow'):
                section,a,b,_=RULE_BLOCKS[key]
                if section in sections:self.block(a,b,{UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:INI})
            if 'General' in sections:
                self.block(0x66F159,0x66F17F,{UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:INI})
                self.block(0x67198C,0x6719AC,{UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:INI})
            if 'Super' in sections:
                wh=self.read32(RULES+0xFA8)
                assert self.string(wh+0x24)=='Super'
                assert self.invoke(0x75D3A0,wh,(INI,))&255
            if self.case.get('type','AEGIS') in sections:
                # Supplied interior frames, original read/default/store blocks.
                self.block(0x5F94B3,0x5F9516,{UC_X86_REG_EBX:t,UC_X86_REG_ESI:INI,UC_X86_REG_EBP:t+0x24,UC_X86_REG_EAX:u.mem_read(t+0x231,1)[0]})
                for a,b in ((0x7121D1,0x7121EB),(0x712270,0x71228A),(0x7122BE,0x7122D8),(0x7124CF,0x7124F0)):
                    self.block(a,b,{UC_X86_REG_EBP:t,UC_X86_REG_ESI:INI,UC_X86_REG_EBX:t+0x24})
                for a,b in ((0x714A2F,0x714A49),(0x714A63,0x714A7D),(0x714D6D,0x714D8E),(0x715024,0x715045)):
                    self.block(a,b,{UC_X86_REG_EBP:t,UC_X86_REG_EDI:INI,UC_X86_REG_EBX:t+0x24})
                u.mem_write(SP+0x380,dwords(INI))
                # The reader loads [esp+388] after two pushes.
                self.block(0x71605E,0x716090,{UC_X86_REG_EBP:t,UC_X86_REG_EBX:t+0x24})
            self.layers.append(dict(file=file,bytes=len(raw),sha256=sha(raw),sections=sections,source_lines=lines,type_after=self.type_state()))
        self.warhead=self.read32(RULES+0xFA8)
        assert self.string(self.warhead+0x24)=='Super'
        self.inputs={'type':self.type_state(),'c4_name':self.string(self.warhead+0x24),
                     'ship_sinking_weight':struct.unpack('<d',u.mem_read(RULES+0x630,8))[0],
                     'radar_combat_flash_time':signed(self.read32(RULES+0x8C)),
                     'super_infdeath':self.read32(self.warhead+0x120),
                     'land_rows':{name:list(struct.unpack('<8f',u.mem_read(0x89EA40+i*36,32))) for i,name in enumerate(self.land_names)},
                     'ship_vtable':hex(0x7F2D8C),'ship_at_coord':hex(self.read32(0x7F2D8C+0xA0))}

    def hook(self,u,pc,n,d):
        if self.phase=='measure':
            sp=u.reg_read(UC_X86_REG_ESP)
            if pc in getattr(self,'imports',{}):
                p=self.read32(sp+4);v=(self.read32(p)+self.imports[pc])&0xFFFFFFFF
                u.mem_write(p,dwords(v));self.seams.append(dict(pc=hex(pc),kind='OS Interlocked',delta=self.imports[pc]))
                self.ret(v,4);return
            if pc in (0x7C8E17,0x7C8B3D,0x7D140B,0x5B40B0):
                raise AssertionError(('unexpected setup seam during measured original body',hex(pc)))
            if self.pending and pc==self.pending[-1]['return']:
                row=self.pending.pop();row['result']=signed(u.reg_read(UC_X86_REG_EAX));row['health_after']=signed(self.read32(self.actor+0x6C));row.pop('return');self.calls.append(row)
            names={0x487A10:'cell_repair_occupants',0x73F0A0:'unit_can_enter',0x4D9C10:'foot_can_enter',0x55ABF0:'ship_entry_leaf',0x6A3F50:'ship_at_coord',0x737C90:'unit_damage',0x4D7330:'foot_damage',0x701900:'techno_damage',0x5F5390:'object_damage',0x744720:'unit_record_kill',0x702D40:'techno_record_kill',0x4D9720:'foot_detach_all',0x5F5280:'object_detach_all',0x7258D0:'pointer_expired',0x4D5660:'foot_stun',0x738680:'death_explosion',0x4DBDF0:'foot_coordinates',0x65C780:'random_next',0x65C7E0:'random_range'}
            if pc in names:
                row=dict(kind=names[pc],pc=hex(pc),this=hex(u.reg_read(UC_X86_REG_ECX)),health=signed(self.read32(self.actor+0x6C)))
                if pc in (0x73F0A0,0x6A3F50):
                    row['return']=self.read32(sp);row['args']=[self.read32(sp+4+i*4) for i in range(5 if pc==0x73F0A0 else 4)]
                    self.pending.append(dict(row));self.calls.append(dict(row,kind=row['kind']+'_entry'))
                else:self.calls.append(row)
                if pc==0x737C90:
                    args=[self.read32(sp+4+i*4) for i in range(7)]
                    row.update(damage=signed(self.read32(args[0])),health_alias=args[0]==self.actor+0x6C,
                               distance=signed(args[1]),warhead=self.string(args[2]+0x24),attacker=args[3],ignore_defenses=args[4],arg6=args[5],house=args[6])
                if pc in (0x737C90,0x4D7330,0x701900,0x5F5390):
                    self.pending.append(dict(kind=names[pc]+'_return',pc=hex(pc),return_=self.read32(sp)))
                    self.pending[-1]['return']=self.pending[-1].pop('return_')
                if pc in (0x4D9720,0x5F5280):row['all']=self.read32(sp+4)&255
                if pc==0x7258D0:row['all']=u.reg_read(UC_X86_REG_EDX)&255
                if pc in (0x65C7E0,0x65C780):
                    row['rng']=next((name for name,p in self.rngs.items() if p==u.reg_read(UC_X86_REG_ECX)),hex(u.reg_read(UC_X86_REG_ECX)))
                    if pc==0x65C7E0:row['range']=[signed(self.read32(sp+4)),signed(self.read32(sp+8))]
            if pc==self.read32(0x7F5C70+0x28):
                self.calls.append(dict(kind='unit_pointer_expired',pc=hex(pc),expired_is_self=self.read32(sp+4)==self.actor,all=self.read32(sp+8)&255))
        super().hook(u,pc,n,d)

    def read_hook(self,u,_access,address,size,value,_data):
        if self.phase!='measure':return
        for name,p,n in self.read_ranges:
            if p<=address and address+size<=p+n:
                key=(name,address-p,size)
                self.accesses.setdefault(key,bytes(u.mem_read(address,size)).hex())
                break

    def write_hook(self,u,_access,address,size,value,_data):
        if self.phase!='measure':return
        for name,p,n in self.read_ranges:
            if p<=address and address+size<=p+n:
                self.writes.append(dict(region=name,offset=hex(address-p),size=size,value=value,pc=hex(u.reg_read(UC_X86_REG_EIP))));break

    def run(self):
        u=self.u
        before={name:bytes(u.mem_read(p,0x3F4)) for name,p in self.rngs.items()}
        u.mem_write(SP,dwords(RET_MAGIC,0));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,self.selected)
        endpoint=run_checked(u,0x487A10,(RET_MAGIC,self.case.get('stop',0x738680)),count=300000,required_addresses=(0x487A10,))
        assert self.code_before==[bytes(u.mem_read(a,b-a)) for a,b in self.code_spans]
        return dict(endpoint=hex(endpoint),health=signed(self.read32(self.actor+0x6C)),alive=u.mem_read(self.actor+0x90,1)[0],
                    sinking=u.mem_read(self.actor+0x3CD,1)[0],on_bridge=u.mem_read(self.actor+0x8C,1)[0],
                    is_on_map=u.mem_read(self.actor+0x74,1)[0],fpcw=hex(u.reg_read(UC_X86_REG_FPCW)),
                    owner_units_lost=self.read32(self.house+0x5434),ground_member_retained=self.read32(self.cells[tuple(self.case.get('member',[115,59]))]+0xE4)==self.actor,
                    calls=self.calls,writes=self.writes,reads=[dict(region=k[0],offset=hex(k[1]),size=k[2],initial_hex=v) for k,v in sorted(self.accesses.items())],
                    rng={name:dict(before_hex=raw.hex(),after_hex=bytes(u.mem_read(self.rngs[name],0x3F4)).hex(),unchanged=raw==bytes(u.mem_read(self.rngs[name],0x3F4))) for name,raw in before.items()},
                    substitutions=self.seams,code_unchanged=True)

def inputs():
    yield dict(name='resident_water_before',land=2)
    yield dict(name='resident_road_after',land=1)
    yield dict(name='resident_road_iron_curtain',land=1,iron_curtain=True)
    yield dict(name='resident_road_immune_type_counterfactual',land=1,immune=True)
    yield dict(name='resident_road_warping_out',land=1,warping_out=True)
    yield dict(name='resident_road_foot_speed_counterfactual',land=1,speed_override=0)
    for dz in (-105,-104,0,104,105):
        yield dict(name=f'west_water_head_road_dz{dz}',land=1,target=[114,59],member=[113,59],current=[113*256+128,59*256+128,208],head=[114*256+128,59*256+128,208+dz])
    yield dict(name='west_water_null_head',land=1,target=[114,59],member=[113,59],current=[113*256+128,59*256+128,208])
    yield dict(name='west_water_head_road_nonfoot_counterfactual',land=1,target=[114,59],member=[113,59],current=[113*256+128,59*256+128,208],head=[114*256+128,59*256+128,208],nonfoot=True)
    yield dict(name='east_water_head_road',land=1,target=[116,59],member=[117,59],current=[117*256+128,59*256+128,208],head=[116*256+128,59*256+128,208])
    yield dict(name='west_head_not_at_center_call',land=1,target=[115,59],member=[113,59],current=[113*256+128,59*256+128,208],head=[114*256+128,59*256+128,208])
    for gate,kw in [('below_weight',{'weight_override':2.999}),('at_weight',{'weight_override':3.0}),('underwater',{'underwater':True}),('organic',{'organic':True}),('being_warped',{'being_warped':True})]:
        yield dict(name='west_water_'+gate+'_counterfactual',land=1,target=[114,59],member=[113,59],current=[113*256+128,59*256+128,208],head=[114*256+128,59*256+128,208],**kw)
    yield dict(name='resident_road_marked_before_explosion',land=1,is_on_map=True)
    yield dict(name='west_water_marked_before_postlude',land=1,target=[114,59],member=[113,59],current=[113*256+128,59*256+128,208],head=[114*256+128,59*256+128,208],is_on_map=True,stop=0x737F74)

def generate():
    rows=[];m=None
    for case in inputs():
        m=Native(case);rows.append(dict(input=case,output=m.run()))
    return dict(schema_version=1,scope=__doc__,constructor=m.ctor,actor_constructor=m.actor_ctor,retail_inputs=m.inputs,layers=m.layers,cases=rows)

def metadata():
    p=provenance(scope=__doc__,assumptions=[
        'Original UnitType constructor and selected exact reader blocks execute; physical rules/mode/map strings use supplied signed-CRC caches. Full file/archive and scenario loaders excluded.',
        'One supplied AEGIS Unit uses original class/type vtables, Unit/base constructor prefix7353C0..7354CE and original Ship constructor69EC50. Prefix stops before COM locomotor creation/type post-initialization. Actor flags7, current HP from native Strength, Alive1, Limbo0, owner, Guard mission, original Ship owner/refcount and retained head are supplied. No legal route/spawn/full Unlimbo claim.',
        'Default selected cell115,59 and west/east targets114,59/116,59 are level2/raw100-clear. All three x114..116,y59 cells have finalized Road for repaired cases; adjacent x113/117 remain Water. Original repair calls Recalc for all three before any occupants call; a center-only land transition is not represented as stock.',
        'Selected land2 versus1 comes from the independently read physical Road/Water speed rows; Recalc and overlay publication are not executed here. Cells use overlay-1 to isolate finalized-land admission from overlay/classification; stock101/83 membership is not certified.',
        'Road deaths stop at actual Death_Explosion738680 entry. Neighbor Water sinking cases execute full actual Unit/Foot/Techno/Object receiver and return through Cell487A10. They retain constructor IsOnMap0, so no active marked-world proof. Two marked1 controls establish the original Road versus Water decision before post-death Mark; the Water row stops at737F74. Full explosion, full Unlimbo/Logic registration and final lifetime cleanup are excluded.',
        'Original receiver kill-record and three pointer-expiry/self-observer sweeps execute in a returned sinking row, as do real Foot Stun and Ship stop. Registered external observers, team/tag/slave/bunker/parasite/passengers are absent. House owner has supplied index0, cost multipliers1 and empty counts; output loss-counter arithmetic is bounded to that state.',
        'Main886B88, Scenario+218 and MapGenABE890 are seeded with original65C6D0 after constructor/setup draws. Each full0x3F4-byte state is preserved before/after; no draw is observed in any measured controller/receiver case. Native x87 control word0x0E7F is a supplied fixture ambient state, not a captured active-game invariant.',
        'Original Techno receiver writes RadarCombatFlash timer +174=frame1000,+178 padding0,+17C=49 from constructor default21 plus actual General/RadarCombatFlashTime read. All actor/type/house/loco/rules/warhead writes and original PCs are recorded.'],substitutions=[
        'Setup allocator/free/CRT TLS are inherited from Landing. Measured gameplay calls are original; only OS Interlocked increment/decrement arithmetic is supplied when original COM methods reach imports.'
    ],entry_points={'occupants':0x487A10,'unit_can_enter':0x73F0A0,'foot_can_enter':0x4D9C10,'ship_at_coord':0x6A3F50,'unit_damage':0x737C90,'foot_damage':0x4D7330,'techno_damage':0x701900,'object_damage':0x5F5390,'unit_record_kill':0x744720,'self_pointer_expired':0x7446E0,'sink_state_stores':0x737E43})
    m=Native(next(inputs()))
    p['original_slices']=[dict(start=hex(a),end_exclusive=hex(b),hex=raw.hex(),sha256=sha(raw)) for (a,b),raw in zip(m.code_spans,m.code_before)]
    return p

if __name__=='__main__':finish_vectors(generate,HERE/'naval_occupants.json',provenance=metadata)
