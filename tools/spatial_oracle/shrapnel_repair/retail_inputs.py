"""Original scalar/type readers over declared physical lexical INI caches."""
from pathlib import Path
import hashlib,os,struct
from unicorn.x86_const import *
from tools.rules_oracle.bridge_anim_inputs import Reader
from tools.rules_oracle.bridge_child_sound import Sound
from tools.rules_oracle import theater_general_reader
from tools.rules_oracle.theater_general_reader import TheaterReader,general_text
from tools.projectile_oracle.bridge_render_inputs import lexical
from tools.spatial_oracle.building_body_rules import INI,SP,dwords
from tools.native_oracle import run_checked
HERE=Path(__file__).resolve().parent
ASSETS=Path(os.environ.get('VERA20K_SHRAPNEL_INPUTS', 'target/shrapnel-native-inputs/extract'))

class Rules(Sound):
 hook=Reader.hook
 def __init__(self,*,full_rules_constructor=False):
  raw=(ASSETS/'RULESMD.INI').read_bytes();sec,_=lexical(raw,{'OverlayTypes'})
  Reader.__init__(self,ASSETS,sec)
  self.u.mem_write(0xA83D80,dwords(0x7EB6D4,self.alloc(256*4),256,1,0,10))
  self.block(0x668CE3,0x668D34,{UC_X86_REG_ESI:INI})
  self.overlay_ptrs=[self.read32(self.read32(0xA83D84)+i*4) for i in range(self.read32(0xA83D90))]
  assert len(self.overlay_ptrs)==250
  self.names={i:self.string(p+0x24) for i,p in enumerate(self.overlay_ptrs)}
  self.rules=self.alloc(0x2000)
  if full_rules_constructor:self.invoke(0x665650,self.rules)
  else:self.block(0x665F3B,0x665F41,{UC_X86_REG_ESI:self.rules,UC_X86_REG_EBX:0})
  self.ctor_cliff=self.u.mem_read(self.rules+0x664,1)[0]
  self.land_names=[self.string(self.read32(0x839D68+i*4)) for i in range(12)]
  self.layers=[]
  wanted={self.names[i] for i in range(74,102)}|set(self.land_names)|{'General'}
  for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','XShrapnel.MAP'):
   p=ASSETS/name
   if not p.exists():assert name=='LANGRULE.INI';self.layers.append(dict(file=name,absent=True));continue
   raw=p.read_bytes();sections,lines=lexical(raw,wanted);self.make_ini(sections)
   self.invoke(0x674000,0,(INI,))
   if 'General' in sections:self.block(0x66F1CB,0x66F1EC,{UC_X86_REG_ESI:self.rules,UC_X86_REG_EDI:INI})
   for index in range(74,102):
    if self.names[index] not in sections:continue
    ptr=self.overlay_ptrs[index]
    self.block(0x5FE798,0x5FE8A6,{UC_X86_REG_ESI:ptr,UC_X86_REG_EBX:INI})
    self.block(0x5FE933,0x5FEA09,{UC_X86_REG_ESI:ptr,UC_X86_REG_EBX:INI,UC_X86_REG_EDI:ptr+0x24})
   self.layers.append(dict(file=name,sha256=hashlib.sha256(raw).hexdigest(),cliff=self.u.mem_read(self.rules+0x664,1)[0],sections=sections,source_lines=lines))
 def block(self,a,b,regs):
  self.u.reg_write(UC_X86_REG_ESP,SP)
  for k,v in regs.items():self.u.reg_write(k,v)
  run_checked(self.u,a,b,count=6000000)
 def snapshot(self):
  return dict(constructor_cliff=self.ctor_cliff,cliff=self.u.mem_read(self.rules+0x664,1)[0],overlays=[dict(index=i,name=self.names[i],land=self.read32(p+0x298),no_use_tile_land=self.u.mem_read(p+0x2AC,1)[0],tiberium=self.u.mem_read(p+0x2A9,1)[0],wall=self.u.mem_read(p+0x2A8,1)[0]) for i,p in enumerate(self.overlay_ptrs) if 74<=i<=101],land_table_hex=bytes(self.u.mem_read(0x89EA40,12*36)).hex(),layers=self.layers)

def theater():
 raw=(ASSETS/'SNOWMD.INI').read_bytes()
 # The shared reader builds an unused asset cache at construction. Bind it to
 # the same explicit retail directory; restore its module default afterward.
 previous=theater_general_reader.ROOT;theater_general_reader.ROOT=ASSETS
 try:m=TheaterReader()
 finally:theater_general_reader.ROOT=previous
 initial=m.read(general_text(raw));rows=m.reads
 assert m.asset_loaded==[], 'theater scalar witness must not load graphics'
 # The surrounding loader supplies ordinal and cumulative count. Here each
 # physical TilesInSet scalar is parsed by original5276D0; file/TMP admission
 # remains a declared supplied boundary, not a full545150 loader execution.
 sections,_=lexical(raw,{f'TileSet{i:04d}' for i in range(300)})
 r=Rules();r.make_ini(sections);base=0;tiles={};sets=[]
 for ordinal in range(300):
  name=f'TileSet{ordinal:04d}'
  if name not in sections:break
  count=r.invoke(0x5276D0,INI,(r.cstring(name),r.cstring('TilesInSet'),0))
  assert count<1000
  m.project(ordinal,base)
  filename=sections[name]['FileName']
  for j in range(count):tiles[base+j]=filename+str(j+1).zfill(2)+'.sno'
  sets.append(dict(ordinal=ordinal,base=base,count=count,physical=sections[name]));base+=count
 globals_={int(row.get('resolved_global',row['location']),16):m.read32(int(row.get('resolved_global',row['location']),16)) for row in rows}
 return dict(sha256=hashlib.sha256(raw).hexdigest(),general=initial,globals=globals_,sets=sets,tiles=tiles,count=base)
