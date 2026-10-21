"""Original InfantryType SpeedType constructor and bounded layered reader.

Full5236A0 constructor executes. Original410A60 section admission prefix and
7121D1..7121EB SpeedType read/store execute per supplied lexical INI layer;
the unrelated intervening Object/Techno type reads and full loader do not.
"""
from pathlib import Path
import hashlib,struct
from unicorn import UC_HOOK_MEM_WRITE
from unicorn.x86_const import *
from tools.native_oracle import RET_MAGIC,run_checked,provenance,finish_vectors
from tools.rules_oracle.bridge_anim_inputs import Reader
from tools.projectile_oracle.bridge_render_inputs import lexical,assets_root
from tools.spatial_oracle.building_body_rules import INI,SP,dwords

ROOT=assets_root()

def sha(raw):return hashlib.sha256(raw).hexdigest()
def signed(x):return struct.unpack('<i',struct.pack('<I',x))[0]

class SpeedReader(Reader):
    def __init__(self):
        self.phase='setup';self.read_receipts=[];self.seams=[]
        super().__init__(ROOT,{})
        self.typ=self.alloc(0xF00)
        self.writes=[]
        def record(_u,_access,address,size,value,_data):
            if address<=self.typ+0x67C<address+size:
                self.writes.append(dict(phase=self.phase,pc=hex(self.u.reg_read(UC_X86_REG_EIP)),size=size,value=value))
        self.u.hook_add(UC_HOOK_MEM_WRITE,record)
        self.code_spans=((0x5236A0,0x523977),(0x710AF0,0x7110E6),(0x410A60,0x410A8C),(0x7121D1,0x7121EB),(0x476FC0,0x47701F),(0x48DFF0,0x48E029))
        self.code_before=[bytes(self.u.mem_read(a,b-a))for a,b in self.code_spans]
        self.phase='constructor'
        self.invoke(0x5236A0,self.typ,(self.cstring('ENGINEER'),))
        assert self.string(self.typ+0x24)=='ENGINEER'
        assert self.read32(self.typ)==0x7EB610
        self.constructor=dict(speed_type=self.speed(),field_writes=list(self.writes),seams=list(self.seams))
    def hook(self,u,pc,n,d):
        if pc in (0x7C8E17,0x7C8B3D,0x7D140B,0x5B40B0):self.seams.append(dict(phase=self.phase,pc=hex(pc)))
        if pc==0x476FC0:
            sp=u.reg_read(UC_X86_REG_ESP)
            self.read_receipts.append(dict(reader=hex(pc),ini=hex(u.reg_read(UC_X86_REG_ECX)),section=self.string(self.read32(sp+4)),key=self.string(self.read32(sp+8)),default=signed(self.read32(sp+12))))
        elif pc==0x477002:self.read_receipts[-1]['read_string_value']=self.string(u.reg_read(UC_X86_REG_ECX))
        elif pc==0x477012:self.read_receipts[-1]['read_string_empty_retains_default']=True
        super().hook(u,pc,n,d)
    def speed(self):return signed(self.read32(self.typ+0x67C))
    def read_layer(self,sections,label):
        self.make_ini(sections);self.phase=label;before=self.speed();mark=len(self.writes);self.read_receipts=[];seam_mark=len(self.seams)
        # Original active type-reader admission, before unrelated Name/UIName reads.
        self.u.mem_write(SP,dwords(RET_MAGIC,INI));self.u.reg_write(UC_X86_REG_ESP,SP);self.u.reg_write(UC_X86_REG_ECX,self.typ)
        end=run_checked(self.u,0x410A60,(0x410A8C,0x410B7D),count=200000,required_addresses=(0x526810,))
        admitted=end==0x410A8C
        if admitted:
            self.u.mem_write(SP,dwords(RET_MAGIC));self.u.reg_write(UC_X86_REG_ESP,SP)
            self.u.reg_write(UC_X86_REG_EBP,self.typ);self.u.reg_write(UC_X86_REG_ESI,INI);self.u.reg_write(UC_X86_REG_EBX,self.typ+0x24)
            run_checked(self.u,0x7121D1,0x7121EB,count=200000,required_addresses=(0x476FC0,0x528A10))
            assert self.u.reg_read(UC_X86_REG_ESP)==SP
        assert self.seams[seam_mark:]==[],self.seams[seam_mark:]
        assert self.code_before==[bytes(self.u.mem_read(a,b-a))for a,b in self.code_spans]
        return dict(section_admitted=admitted,section_admission_boundary=hex(end),before=before,after=self.speed(),
                    read_receipts=list(self.read_receipts),field_writes=self.writes[mark:],code_unchanged=True,reader_substitutions=[])

def generate():
    m=SpeedReader();table=[dict(id=i,name=m.string(m.read32(0x81DA58+4*i)))for i in range(8)]
    layers=[]
    for file in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
        path=ROOT/file
        if not path.exists():assert file=='LANGRULE.INI';layers.append(dict(file=file,absent=True));continue
        raw=path.read_bytes();sections,lines=lexical(raw,{'ENGINEER'})
        layers.append(dict(file=file,bytes=len(raw),sha256=sha(raw),engineer_sections=sections,
                           source_lines=lines,**m.read_layer(sections,file)))
    physical=dict(constructor=m.constructor,layers=layers,final_speed_type=m.speed())
    controls=[]
    cases=[('missing_key',{'ENGINEER':{'Other':'yes'}}),('missing_section',{}),
           ('key_wrong_case',{'ENGINEER':{'speedtype':'Track'}}),('section_wrong_case',{'engineer':{'SpeedType':'Track'}})]
    for value in [x['name'] for x in table]+['tRaCk','\t Foot\r\n','bogus','0','1','-1','<none>','None','',' \t ','Foot suffix','F']:
        cases.append(('value_'+repr(value),{'ENGINEER':{'SpeedType':value}}))
    cases.extend([('cap127_cuts_after_Foot',{'ENGINEER':{'SpeedType':' '*123+'Foot'+'Track'}}),
                  ('cap127_truncates_Foot_to_Foo',{'ENGINEER':{'SpeedType':' '*124+'Foot'}})])
    for name,sections in cases:
        c=SpeedReader();controls.append(dict(name=name,supplied_sections=sections,constructor=c.constructor,**c.read_layer(sections,name)))
    assert [x['after'] for x in controls[-2:]]==[0,-1]
    # Explicit layering distinguishes default=current from fixed category fallback.
    c=SpeedReader();stack=[]
    for label,sections in [('explicit_track',{'ENGINEER':{'SpeedType':'Track'}}),('missing_key',{'ENGINEER':{'Other':'yes'}}),
                           ('empty_value',{'ENGINEER':{'SpeedType':''}}),('invalid_value',{'ENGINEER':{'SpeedType':'invalid'}}),
                           ('missing_after_invalid',{'ENGINEER':{'Other':'yes'}}),('explicit_foot',{'ENGINEER':{'SpeedType':'Foot'}})]:
        stack.append(dict(name=label,supplied_sections=sections,**c.read_layer(sections,label)))
    assert physical['constructor']['speed_type']==physical['final_speed_type']==0
    assert [x['after'] for x in stack]==[1,1,1,-1,-1,0]
    return dict(schema_version=1,scope=__doc__,speed_type_table=table,key=m.string(0x844504),
                physical=physical,controls=controls,explicit_layer_controls=stack)


def metadata():
    return provenance(scope=__doc__,entry_points={'infantry_type_constructor':0x5236A0,'techno_type_constructor':0x710AF0,'constructor_speed_write':0x7110E0,
                    'abstract_section_admission':0x410A60,'speed_reader_begin':0x7121D1,'speed_reader_stop':0x7121EB,'read_speed_type':0x476FC0,'read_string':0x528A10,'speed_from_name':0x48DFF0},
                    assumptions=['Full original InfantryType constructor and its Object/Techno base constructors execute against a supplied runtime fixture. Original allocation uses bounded bump storage; registry participation outside selected SpeedType field is not certified.',
                                 'Physical layered files are supplied as signed-CRC INI caches from exact-case lexical section/key/value strings; original INI file/archive loading is not executed. Physical ENGINEER lacks SpeedType; LANGRULE is absent.',
                                 'Original AbstractType section-admission prefix executes and skips absent sections. The later SpeedType field block executes with supplied TechnoType reader frame; unrelated intervening Name/UIName/Object/Techno reads and full reader suffix do not execute.',
                                 'Native ReadSpeedType uses the current field value as default, original128-byte ReadString, original trimming and case-insensitive whole-name parser; native output and exact field stores are retained.',
                                 'Synthetic controls are direct supplied INI cache strings, separate from physical-layer evidence. They cover explicit enum names, wrong-case section/key, empty/whitespace, malformed/numeric values and retained defaults.'],
                    substitutions=['Constructor/setup inherits bounded operator_new7C8E17, operator_delete7C8B3D and CRT TLS accessor7D140B seams. The measured section/SpeedType reader phases assert none is reached.'])


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
