"""Original Spotlight64..73 surfaces and DSurface4BBCA0 line controls."""
import argparse
import hashlib
import json
import struct
from pathlib import Path
from unicorn import UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_ESP
from tools import native_oracle as n
from tools.procedural_drawing_oracle import electric_bolt as e, rally

OUT=Path(__file__).with_name('spotlight.json')
def fnv(data):
    h=0xcbf29ce484222325
    for b in data:h=((h^b)*0x100000001b3)&0xffffffffffffffff
    return f'{h:016x}'
def machine():
    m=e.LightPixels(dict(name='building_spotlight',source=[3038,3038,0],target=[3600,2700,0],camera=[-60,250],fps=60,detail=2,background=rally.BACKGROUND))
    m.u.mem_map(0x26000000,0x1000000);m.light_cursor=0x26000000
    return m

def generate():
    banks=[]
    for radius in [175,300]:
        m=machine();u=m.u;rules=m.allocate(0x2000);u.mem_write(0x8871e0,e.words(rules));u.mem_write(rules+0x7a8,e.words(radius))
        u.mem_write(e.SP,e.words(n.RET_MAGIC));u.reg_write(UC_X86_REG_ESP,e.SP)
        n.run_checked(u,0x5ff420,0x5ff444,count=10000)
        n.run_checked(u,0x5ff5aa,0x5ff727,count=40000000,timeout_us=180000000,required_addresses=[0x7bb920,0x5ff6f2])
        masks=[]
        for index in range(64,74):
            surface=e.u32(u,0xac1698+index*4);raw=bytes(u.mem_read(e.u32(u,surface+0x14),256*128))
            masks.append(dict(index=index,fnv64=fnv(raw),nonzero=sum(x!=0 for x in raw),peak=max(raw)))
        banks.append(dict(radius=radius,masks=masks))
    return dict(stage_table_hex=bytes(u.mem_read(0x83358c,90)).hex(),banks=banks,lines=line_controls(m))

def line_controls(m=None):
    if m is None:m=machine()
    u=m.u
    lines=[];captured=[]
    def store(u,access,address,size,value,user):
        if rally.PIXELS<=address<rally.PIXELS+m.pixel_bytes:
            assert size==2
            offset=(address-rally.PIXELS)//2
            sp=u.reg_read(UC_X86_REG_ESP)
            z=struct.unpack('<H',u.mem_read(sp+0x8c,2))[0]
            captured.append([offset%m.surface.size[0],offset//m.surface.size[0],z])
    u.hook_add(UC_HOOK_MEM_WRITE,store)
    for first,last,z in [([20,20],[40,30],[-100,-50]),([40,30],[20,20],[-50,-100]),([-5,20],[20,30],[-100,-80]),([30,20],[32,40],[-100,-90])]:
        u.mem_write(rally.PIXELS,m.before);u.mem_write(m.z_pixels,b'\xff\xff'*(m.pixel_bytes//2));captured.clear()
        clip=m.allocate(16);a=m.allocate(8);b=m.allocate(8)
        rect=[0,0,*m.surface.size];u.mem_write(clip,e.words(*rect));u.mem_write(a,e.words(*first));u.mem_write(b,e.words(*last))
        m.call(0x4bbca0,rally.SURFACE,clip,a,b,75,*z,0)
        zptr=e.u32(u,0x887644)
        origin=((e.ints(u,zptr+4,1)[0]+struct.unpack('<H',u.mem_read(zptr+0x24,2))[0])&0xffff)-0x8000
        lines.append(dict(first=first,last=last,z_adjust=z,clip=rect,z_origin_y=origin,visits=list(captured)))
    return lines

def main():
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate()
    if a.write:
        OUT.write_text(json.dumps(data,indent=2)+'\n')
        meta=n.provenance(scope='Original Initialize5FF420 prologue and complete type16 bank loop5FF5AA..5FF727; all ten masks for two declared radii; whole DSurface4BBCA0 with observed depth-admitted store visits.',assumptions=['Rules+7A8 supplied as175 or300. The production retained Rules reader is independently regression tested. All ten native midpoint-circle/downsample bodies execute.','Only mask64..73 outputs and original90-byte stage table are retained; no full Windows startup or GPU equivalence claim.'],substitutions=['Existing LightPixels operator-new/delete/BSurface memory storage transport. No gameplay instructions, circle raster, mask pixels or table values patched.'],entry_points={'initialize':0x5ff420,'type16_loop':0x5ff5aa,'circle':0x7bb920,'brightening_line':0x4bbca0})
        meta['command']='RA2_DIR=/path/to/retail python -m tools.procedural_drawing_oracle.spotlight --write'
        OUT.with_suffix('.meta.json').write_text(json.dumps(meta,indent=2)+'\n')
    else:assert data==json.loads(OUT.read_text())
    print('spotlight masks:20 original surfaces passed')
if __name__=='__main__':main()
