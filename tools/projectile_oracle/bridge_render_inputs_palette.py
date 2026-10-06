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
  if p==0x7c8e17:
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
