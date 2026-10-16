"""Original selected Init_Game palette loads, expansion and full Convert constructors.
Archive IO, an RGB565 BSurface and its initialized format globals are supplied.
"""
import importlib.util,json,struct,hashlib,argparse
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
from tools.native_oracle import run_checked,image_sha256
from tools.spatial_oracle.building_body_rules import SP,dwords
from tools.projectile_oracle import bridge_render_inputs as module

class PaletteReader(module.BulletReader):
 def __init__(self,art):
  super().__init__(art);self.u.mem_map(0x24400000,0x3c00000)
 def alloc(self,n):
  out=self.cursor;self.cursor+=(n+15)&~15;assert self.cursor<0x28000000;return out
 def hook(self,u,p,n,d):
  if p in (0x7c8e17,0x7c9430):
   self.ret(self.alloc(self.read32(u.reg_read(UC_X86_REG_ESP)+4)));return
  super().hook(u,p,n,d)

def initialize(m):
 u=m.u;surface=m.alloc(0x40);vtable=m.alloc(0x100)
 u.mem_write(surface,dwords(vtable));u.mem_write(surface+0x10,dwords(2))
 u.mem_write(vtable+0x70,dwords(0x411630));u.mem_write(0x887308,dwords(surface))
 u.mem_write(0x89ecf8,dwords(0x7eb6d4,m.alloc(4096),1024,1,0,10))
 # Native startup's empty DynamicVector registries, supplied with spare capacity.
 # Original intensity420140 populates and subsequently reuses this cache.
 u.mem_write(0x88a080,dwords(0x7eb6d4,m.alloc(4096),1024,1,0,10))
 for addr,value in((0x8a0dd0,11),(0x8a0dd4,3),(0x8a0dd8,0),(0x8a0ddc,3),(0x8a0de0,5),(0x8a0de4,2)):
  u.mem_write(addr,dwords(value))
 u.mem_write(0x8a0de8,struct.pack('<2H',0x7bef,0xf7de))
 u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EBX,0)
 # Full original calls, not injected Convert fields or shade tables.
 run_checked(u,0x52be61,0x52bfce,count=100000000,timeout_us=120000000,
  required_addresses=(0x52be6d,0x52bf26,0x48e740,0x48ebf0,0x4bbb00))
 assert u.reg_read(UC_X86_REG_ESP)==SP
 return m

def color_scheme_palette(name='LightGold'):
 """Native selected temperate palette expansion and whole ColorScheme ctor.

 The selected theater/name and spare registry are supplied component inputs;
 no SHP Convert or reconstructed table initializes the voxel Convert.
 """
 from tools.procedural_drawing_oracle.house_color import parse_native_color
 root=module.assets_root();raw=(root/'RULESMD.INI').read_bytes()
 sections,_=module.lexical(raw,{'Colors'});token=sections['Colors'][name]
 hsv=parse_native_color(name,token)
 m=initialize(PaletteReader({}));u=m.u
 source=m.alloc(768);unit=(root/'UNITTEM.PAL').read_bytes();assert len(unit)==768
 u.mem_write(source,unit);u.reg_write(UC_X86_REG_ESI,source)
 u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EDI,0)
 run_checked(u,0x534cd3,0x534d31,required_addresses=(0x6260d0,))
 # Same paired1/53 order as66D444/66D45B, for this selected name only.
 u.mem_write(0xb054d0,dwords(0x7eb6d4,m.alloc(4096),1024,1,0,10))
 color=m.alloc(3);u.mem_write(color,hsv)
 for shades in (1,53):
  scheme=m.alloc(0x33c)
  m.invoke(0x68c710,scheme,(m.cstring(name),color,0x886380,0x885780,shades,1),
           timeout_us=120000000)
 assert m.read32(0xb054e0)==2
 scheme=m.read32(m.read32(0xb054d4)+4);convert=m.read32(scheme+0x30c)
 table=m.read32(convert+0x170);shades=m.read32(convert+0x16c)
 metadata=dict(name=name,lexical=token,hsv=list(hsv),color_index=1,shade_count=shades,
  rulesmd_sha256=hashlib.sha256(raw).hexdigest(),
  palette=dict(name='UNITTEM.PAL',bytes=len(unit),sha256=hashlib.sha256(unit).hexdigest()),
  expanded_palette_sha256=hashlib.sha256(bytes(u.mem_read(0x886380,768))).hexdigest(),
  table_sha256=hashlib.sha256(bytes(u.mem_read(table,shades*512))).hexdigest())
 # Only actual initialized output storage crosses into the existing raster VM.
 globals_=[(p,bytes(u.mem_read(p,n))) for p,n in
  ((0x8a0dd0,28),(0xb054d0,24),(0x88a080,24),(0x89ecf8,24))]
 return (bytes(u.mem_read(0x24000000,m.cursor-0x24000000)),globals_),metadata

def generate():
 m=initialize(PaletteReader({}));u=m.u;rows=[]
 for name,global_address in [('anim.pal',0x87f6c0),('palette.pal',0x87f6c4)]:
  p=m.read32(global_address);table=m.read32(p+0x170);count=m.read32(p+0x16c)
  rows.append(dict(name=name,global_address=hex(global_address),bytes_per_pixel=m.read32(p+4),shade_count=count,
   middle_row=(m.read32(p+0x174)-table)//512,mask_half=m.read32(p+0x180),mask_quarter=m.read32(p+0x184),
   table_sha256=hashlib.sha256(bytes(u.mem_read(table,count*512))).hexdigest(),
   body_plain_vtable=hex(m.read32(m.read32(p+0x88))),body_rle_vtable=hex(m.read32(m.read32(p+0x8c))),
   convert_fields=[hex(m.read32(p+i)) for i in range(0,0x170,4)]))
 return dict(native_sha256=image_sha256(),loads=m.asset_loaded,rows=rows)

if __name__=='__main__':
 a=argparse.ArgumentParser();a.add_argument('--check',type=Path);args=a.parse_args();result=generate()
 if args.check:assert result==json.loads(args.check.read_text());print('PASS original palette startup and full Convert construction')
 else:print(json.dumps(result,indent=2))
