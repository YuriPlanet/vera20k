"""Original Spotlight64..73 surfaces and DSurface4BBCA0 line controls."""
import argparse
import hashlib
import json
import re
import struct
from pathlib import Path
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP
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
    banks=[];draws=[]
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
        draws.append(type16_draw(m,radius,80 if radius==175 else 89,0 if radius==175 else 1))
    return dict(stage_table_hex=bytes(u.mem_read(0x83358c,90)).hex(),banks=banks,lines=line_controls(m),type16_draws=draws)


def changed_pixels(m):
    """Sparse final packed words from the original surface, not a Python raster."""
    raw=bytes(m.u.mem_read(rally.PIXELS,m.pixel_bytes))
    return [[i%m.surface.size[0],i//m.surface.size[0],pixel[0]]
            for i,pixel in enumerate(struct.iter_unpack('<H',raw))
            if raw[i*2:i*2+2]!=m.before[i*2:i*2+2]]


def type16_draw(m,radius,stage,flags):
    """Whole original constructor/manager/draw using the bank just constructed."""
    u=m.u
    u.mem_write(0xac1678,e.words(0x7eb6d4,m.allocate(64),16,1,0,10))
    pointer=m.allocate(0x18)
    m.call(0x5ff250,pointer,*m.case['source'],16)
    # These are explicit retained stage/flag inputs to the native draw, not a
    # replacement for its lifetime owner or its surface-index calculation.
    u.mem_write(pointer+12,e.words(stage))
    u.mem_write(pointer+20,e.words(flags))
    u.mem_write(0x887314,e.words(rally.SURFACE))
    u.mem_write(0x833588,e.words(2))
    u.mem_write(0xac1838,e.words(0))
    u.mem_write(rally.PIXELS,m.before)
    indices=[]
    def observe(u,pc,_size,_data):
        if pc==0x5ff93f:
            indices.append(e.signed(u.reg_read(UC_X86_REG_ECX)))
    hook=u.hook_add(UC_HOOK_CODE,observe)
    rng=m.trace.states()
    m.call(0x5fffa0)
    u.hook_del(hook)
    assert len(indices)==1,indices
    assert m.trace.states()==rng
    assert bytes(u.mem_read(m.z_pixels,m.pixel_bytes))==m.before_z
    assert bytes(u.mem_read(rally.ALPHA,m.pixel_bytes))==m.before_alpha
    assert bytes(u.mem_read(rally.PIXELS-32,32))==m.guard
    assert bytes(u.mem_read(rally.PIXELS+m.pixel_bytes,32))==m.guard
    assert hashlib.sha256(u.mem_read(e.prism.TEXT_BEGIN,e.prism.TEXT_SIZE)).hexdigest()==m.text_sha256
    return dict(radius=radius,stage=stage,flags=flags,surface_index=indices[0],
                position=e.ints(u,pointer,3),camera=list(m.surface.camera),
                size=list(m.surface.size),clip=list(m.surface.clip),
                background=m.case['background'],old_z=65535,pixels=changed_pixels(m),
                rng_unchanged=True,z_unchanged=True,alpha_unchanged=True,
                guards_unchanged=True,text_unchanged=True)

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
        assert bytes(u.mem_read(m.z_pixels,m.pixel_bytes))==b'\xff\xff'*(m.pixel_bytes//2)
        assert bytes(u.mem_read(rally.ALPHA,m.pixel_bytes))==m.before_alpha
        lines.append(dict(first=first,last=last,z_adjust=z,clip=rect,z_origin_y=origin,visits=list(captured),
                          size=list(m.surface.size),background=m.case['background'],old_z=65535,
                          amount=75,pixels=changed_pixels(m),z_unchanged=True,alpha_unchanged=True))
    return lines

def main():
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate()
    if a.write:
        # One coordinate/word row per line keeps the sparse native raster
        # inspectable without turning two small controls into a huge diff.
        rendered=re.sub(r'\[\s+(-?\d+),\s+(-?\d+),\s+(-?\d+)\s+\]',
                        r'[\1, \2, \3]',json.dumps(data,indent=2))
        OUT.write_text(rendered+'\n')
        meta=n.provenance(scope='Original Initialize5FF420 prologue and complete type16 bank loop5FF5AA..5FF727; all ten masks for two declared radii; whole DSurface4BBCA0 with observed depth-admitted store visits and final RGB565 pixels; two whole type16 Spotlight5FF850 draws through manager5FFFA0.',assumptions=['Rules+7A8 supplied as175 or300. The production retained Rules reader is independently regression tested. All ten native midpoint-circle/downsample bodies execute.','Type16 draw controls supply stages80/89, flags0/1, source coordinates, camera, clip, RGB565 background and direct-color display mode. Original5FF250 constructs/registers the light and original5FF850 chooses the mask; no native Scenario or light lifetime is executed.','Brightening-line Z is prefilledFFFF and writeZ=false; native Z origin fields are observed as input. Native A/Z, guard pixels and executable instructions remain unchanged. The clipped line retains the documented shared clip-owner residual; uncut GPU comparisons consume native final packed pixels.'],substitutions=['Existing LightPixels operator-new/delete/BSurface memory storage transport. No gameplay instructions, circle raster, mask pixels, projection, surface-index arithmetic or table values patched.'],entry_points={'initialize':0x5ff420,'type16_loop':0x5ff5aa,'circle':0x7bb920,'brightening_line':0x4bbca0,'light_ctor':0x5ff250,'light_draw':0x5ff850,'light_manager':0x5fffa0})
        meta['command']='VERA20K_GAMEMD_EXE=/path/to/retail/gamemd.exe python -m tools.procedural_drawing_oracle.spotlight --write'
        OUT.with_suffix('.meta.json').write_text(json.dumps(meta,indent=2)+'\n')
    else:assert data==json.loads(OUT.read_text())
    print('spotlight:20 original masks,4 brightening lines and2 whole type16 draws passed')
if __name__=='__main__':main()
