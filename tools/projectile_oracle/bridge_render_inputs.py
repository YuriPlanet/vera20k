"""Research: original BulletType constructor/full rules/art reader on physical lexical inputs.
Physical file/archive loading is supplied. No VERA-interpreted scalar input.
"""
import argparse,hashlib,json,struct,os
from pathlib import Path
from unicorn.x86_const import *
from tools.native_oracle import image_sha256,run_checked
from tools.rules_oracle.bridge_anim_inputs import Reader
from tools.spatial_oracle.building_body_rules import INI,RULES,SP,dwords

def assets_root():
    return Path(os.environ.get('VERA20K_PROJECTILE_RENDER_ASSETS',str(Path(os.environ.get('CARGO_TARGET_DIR','target'))/'asset/bridge-projectile-inputs/extract')))
ROOT=assets_root()
BOOLS={'shadow':0x29a,'arcing':0x29b,'inviso':0x29e,'inverse_rotates':0x2a1,
       'anim_palette':0x2a8,'firers_palette':0x2a9,'flat':0x2f7,'voxel':0x236,
       'aa':0x2a4,'ag':0x2a5,'theater':0x22c,'new_theater':0x237}

def lexical(raw, wanted):
    out={};cur=None;lines=[]
    for n,line in enumerate(raw.decode('latin1').splitlines(),1):
        line=line.split(';',1)[0].strip()
        if line.startswith('[') and ']' in line:
            name=line[1:line.index(']')];cur=None
            if name in wanted:
                assert name not in out,name
                cur={};out[name]=cur
        elif cur is not None and '=' in line:
            key,val=map(str.strip,line.split('=',1))
            if key and val:
                assert key not in cur,(name,key)
                cur[key]=val;lines.append(dict(line=n,section=name,key=key,value=val))
    return out,lines

class BulletReader(Reader):
    def __init__(self,art,root=None):
        self.reads=[]
        super().__init__(root or assets_root(),art)
        self.u.mem_write(0xa83c80,dwords(0x7eb6d4,self.alloc(4096),1024,1,0,10))
        scenario=self.alloc(0x7000)
        self.u.mem_write(0xa8b230,dwords(scenario))
        self.u.mem_write(scenario+0x1258,dwords(0))
        # Color is absent on selected Cannon. Supply the unrelated preexisting
        # color registry entry needed by original474A90's default dereference.
        color=self.alloc(0x400);colors=self.alloc(4)
        self.u.mem_write(color+0x304,dwords(self.cstring('FIXTURE_UNUSED_COLOR')))
        self.u.mem_write(colors,dwords(color))
        self.u.mem_write(0xb054d4,dwords(colors));self.u.mem_write(0xb054e0,dwords(1))
    def hook(self,u,p,n,d):
        if p in (0x528a10,0x5295f0,0x5276d0):
            sp=u.reg_read(UC_X86_REG_ESP)
            self.reads.append(dict(reader=hex(p),ini=hex(u.reg_read(UC_X86_REG_ECX)),
                section=self.string(self.read32(sp+4)),key=self.string(self.read32(sp+8))))
        super().hook(u,p,n,d)
    def rules_cache(self,sections):
        art=bytes(self.u.mem_read(INI,0x40))
        self.make_ini(sections)
        self.u.mem_write(RULES,bytes(self.u.mem_read(INI,0x40)))
        self.u.mem_write(INI,art)
    def construct(self,name):
        p=self.alloc(0x400);self.invoke(0x46bbc0,p,(self.cstring(name),));return p
    def bullet_state(self,p):
        image=self.read32(p+0xa4)
        return dict(image=self.string(p+0x1f8),image_present=bool(image),
            canvas=list(struct.unpack('<2h',self.u.mem_read(image+2,4))) if image else None,
            frame_count=struct.unpack('<h',self.u.mem_read(image+6,2))[0] if image else 0,
            image_buffer25=bytes(self.u.mem_read(p+0x1f8,25)).hex(),
            anim_low=self.u.mem_read(p+0x2f4,1)[0],anim_high=self.u.mem_read(p+0x2f5,1)[0],anim_rate=self.u.mem_read(p+0x2f6,1)[0],
            **{name:bool(self.u.mem_read(p+off,1)[0]) for name,off in BOOLS.items()})
    def read_layer(self,p,sections):
        self.rules_cache(sections);self.reads=[];self.asset_loaded=[]
        admitted=self.invoke(0x46bee0,p,(RULES,))&255
        return dict(admitted=bool(admitted),state=self.bullet_state(p),reads=self.reads,asset_loads=self.asset_loaded)

def load_retail_type(root=None):
    root=root or assets_root()
    art,_=lexical((root/'ARTMD.INI').read_bytes(),{'Cannon','120MM'})
    m=BulletReader(art,root);p=m.construct('Cannon')
    for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
        if not (root/name).exists():
            assert name=='LANGRULE.INI';continue
        sections,_=lexical((root/name).read_bytes(),{'Cannon'})
        m.read_layer(p,sections)
    return m,p

def controls():
    cases=[('defaults',{'Image':'120MM'},{}),
        ('flags',{'Image':'120MM','Shadow':'no','Inviso':'yes','FirersPalette':'yes'},
                  {'Rotates':'yes','Flat':'yes','AnimPalette':'yes','AnimLow':'257','AnimHigh':'-1','AnimRate':'258'}),
        ('strict_art',{'Image':'120MM'},{'Image':'REDIRECT','Rotates':'no','Flat':'no'}),
        ('no_image',{'Arcing':'yes'},{}),
        ('buffer25',{'Image':'abcdefghijklmnopqrstuvwxTAIL'},{}),
        ('exact_key',{'image':'120MM'},{}),
        ('art_case',{'Image':'120MM'},{'rotates':'yes','flat':'yes','animpalette':'yes'})]
    rows=[]
    for name,rules,art in cases:
        m=BulletReader({'120MM':art,'Cannon':{'Rotates':'yes','Flat':'yes'}})
        p=m.construct('Cannon');first=m.read_layer(p,{'Cannon':rules})
        second=m.read_layer(p,{'Cannon':{'Arcing':'no'}}) if name=='flags' else None
        rows.append(dict(name=name,rules=rules,art=art,first=first,second=second))
    # Original constructor zeroes the animation byte; the actual ordinary type
    # bypasses velocity-facing selection, so no trajectory values are supplied.
    m,p=load_retail_type();u=m.u
    u.mem_write(0xa8ed40,dwords(0x7eb6d4,m.alloc(4096),1024,1,0,10))
    bullet=m.alloc(0x180);m.invoke(0x466380,bullet)
    u.mem_write(bullet+0xac,dwords(p))
    frame=m.invoke(0x468000,bullet);layer=m.invoke(0x468b90,bullet)
    filenames=[]
    for theater in range(6):
        for key in ('Theater','NewTheater'):
            m=BulletReader({'GTTEST':{key:'yes'}});p=m.construct('Cannon')
            m.u.mem_write(m.read32(0xa8b230)+0x1258,dwords(theater))
            row=m.read_layer(p,{'Cannon':{'Image':'GTTEST'}})
            filenames.append(dict(theater=theater,key=key,loads=row['asset_loads'],state=row['state']))
    return dict(rows=rows,filenames=filenames,ordinary_bullet=dict(frame=frame,layer=layer,anim_frame=u.mem_read(bullet+0x12c,1)[0],countdown=u.mem_read(bullet+0x12d,1)[0]))

def generate():
    art_raw=(ROOT/'ARTMD.INI').read_bytes();art,art_lines=lexical(art_raw,{'Cannon','120MM'})
    m=BulletReader(art);p=m.construct('Cannon');initial=m.bullet_state(p);layers=[]
    for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
        path=ROOT/name
        if not path.exists():
            assert name=='LANGRULE.INI';layers.append(dict(file=name,absent=True));continue
        raw=path.read_bytes();sections,lines=lexical(raw,{'Cannon','MTNK','105mm','120mm','120mmE'})
        layers.append(dict(file=name,bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest(),raw_sections=sections,source_lines=lines,**m.read_layer(p,sections)))
    from tools.projectile_oracle.bridge_render_inputs_selection import generate as selection
    shp=(ROOT/'120mm.shp').read_bytes()
    physical_image=dict(bytes=len(shp),sha256=hashlib.sha256(shp).hexdigest(),hex=shp.hex())
    palettes=[]
    for name in ('palette.pal','anim.pal'):
        raw=(ROOT/name).read_bytes()
        palettes.append(dict(name=name,sha256=hashlib.sha256(raw).hexdigest(),bytes=len(raw),
            colors=[dict(index=i,raw6=list(raw[i*3:i*3+3])) for i in sorted(set(shp[32:]))]))
    return dict(native_sha256=image_sha256(),constructor=initial,art=dict(bytes=len(art_raw),sha256=hashlib.sha256(art_raw).hexdigest(),raw_sections=art,source_lines=art_lines),layers=layers,controls=controls(),ordinary_selection=selection(),physical_image=physical_image,palettes=palettes)

if __name__=='__main__':
    a=argparse.ArgumentParser();a.add_argument('--check',type=Path);args=a.parse_args()
    result=generate()
    if args.check:
        assert result==json.loads(args.check.read_text());print('PASS original full BulletType reader')
    else:print(json.dumps(result,indent=2))
