"""Original BuildingLight numeric regions; no replacement arithmetic/trig."""
import argparse
import json
import struct
from pathlib import Path
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_EBX, UC_X86_REG_FPCW
from tools import native_oracle as native

OUTPUT=Path(__file__).with_name('spotlight.json')
LIGHT,PARENT,RULES=native.SCRATCH,native.SCRATCH+0x1000,native.SCRATCH+0x3000
SP=native.STACK_BASE+native.STACK_SIZE-0x1000

def words(*v): return struct.pack('<'+'I'*len(v),*(x&0xffffffff for x in v))
def ints(u,p,n): return list(struct.unpack('<'+'i'*n,u.mem_read(p,4*n)))
def bits(u,p): return f'{struct.unpack("<Q",u.mem_read(p,8))[0]:016x}'
def vm():
    u=Uc(UC_ARCH_X86,UC_MODE_32);native.load_image(u)
    u.mem_map(native.SCRATCH,native.SCRATCH_SIZE);u.mem_map(native.STACK_BASE,native.STACK_SIZE);u.mem_map(native.RET_MAGIC,0x1000)
    u.reg_write(UC_X86_REG_FPCW,native.NATIVE_FPCW)
    u.mem_write(0x8871e0,words(RULES));u.mem_write(PARENT,words(0x7f5c70));u.mem_write(PARENT+0x90,b'\1')
    u.mem_write(LIGHT,words(0x7e3ad0));u.mem_write(LIGHT+0xe4,words(PARENT))
    u.mem_write(RULES+0x788,words(2000,1000));u.mem_write(RULES+0x790,struct.pack('<ddd',0.014999999664723873,0.0024999999441206455,0.5));u.mem_write(RULES+0x7a8,words(175))
    return u

def ctor(u,p,facing):
    u.mem_write(PARENT+0x9c,words(*p));u.mem_write(PARENT+0x388,words(facing,facing))
    bp=SP+0x100;u.mem_write(bp+8,words(PARENT))
    for r,v in [(UC_X86_REG_EAX,PARENT),(UC_X86_REG_ESP,SP),(UC_X86_REG_EBP,bp),(UC_X86_REG_ESI,LIGHT),(UC_X86_REG_EBX,LIGHT+0xb8)]:u.reg_write(r,v)
    # Original ctor after vector append, before SetLocation and ObjectUnlimbo.
    native.run_checked(u,0x43592a,0x435acd,required_addresses=[0x4c93d0,0x4cacb0,0x4cad00,0x7c5f00])
    endpoint=ints(u,LIGHT+0xc4,3);u.mem_write(LIGHT+0x9c,words(*endpoint))
    return dict(parent=p,facing=facing,pivot=ints(u,LIGHT+0xb8,3),endpoint=endpoint)

def ai(u):
    u.mem_write(SP,words(native.RET_MAGIC));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,LIGHT)
    native.run_checked(u,0x4361d0,native.RET_MAGIC,required_addresses=[0x5f6940])
    return dict(coord=ints(u,LIGHT+0x9c,3),angle_bits=bits(u,LIGHT+0xb0),rate_bits=bits(u,LIGHT+0xd0),direction=bool(u.mem_read(LIGHT+0xd8,1)[0]))

def generate():
    constructors=[]
    for facing in [0,0x3fff,0x4000,0x8000,0xffff]:constructors.append(ctor(vm(),[3200,4096,384],facing))
    motions=[]
    for mode,direction in [(1,False),(1,True),(2,False),(0,False),(0x100,True)]:
        u=vm();c=ctor(u,[3200,4096,384],0x4000);u.mem_write(LIGHT+0xdc,words(mode));u.mem_write(LIGHT+0xd8,bytes([direction]));states=[]
        for frame in range(1,161):
            state=ai(u)
            if frame in (1,2,3,20,40,80,160):states.append(dict(frame=frame,**state))
        motions.append(dict(mode=mode,direction=direction,constructor=c,states=states))
    geometry=[]
    for position in [[4200,4096,384],[4500,4700,384],[3200,6096,384]]:
        u=vm();u.mem_write(PARENT+0x9c,words(3200,4096,384));u.mem_write(LIGHT+0x9c,words(*position));u.mem_write(LIGHT+0xdc,words(1))
        vt=RULES+0x2000;u.mem_write(PARENT,words(vt));u.mem_write(vt+0x48,words(0x5f65a0))
        projected=[]
        def observe(u,pc,*_):
            if pc==0x6d2140:
                sp=u.reg_read(UC_X86_REG_ESP);ret,point,out=ints(u,sp,3)
                projected.append(ints(u,point,3));u.mem_write(out,words(0,0))
                u.reg_write(UC_X86_REG_EAX,1);u.reg_write(UC_X86_REG_ESP,sp+12);u.reg_write(UC_X86_REG_EIP,ret)
        u.hook_add(UC_HOOK_CODE,observe)
        u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,LIGHT)
        native.run_checked(u,0x435dfd,0x436099,count=100000,required_addresses=[0x4cad80,0x5af1a0,0x5afb80])
        assert len(projected)==3
        geometry.append(dict(parent=[3200,4096,384],coord=position,edges=projected[:2],origin=projected[2]))
    return dict(constructors=constructors,motions=motions,draw_geometry=geometry)

def main():
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();data=generate()
    if args.write:
        OUTPUT.write_text(json.dumps(data,indent=2)+'\n')
        meta=native.provenance(scope='BuildingLight ctor numeric region43592A..435ACD and whole AI4361D0 modes0/1/2/invalid; sampled frames through160.',assumptions=['Original Unit7F5C70 parent vtable makes post-movement Building-only Event35 scan ineligible. Parent raw coordinate and snapped FacingClass are supplied inputs. Retail parsed spotlight values are declared inputs; reader fidelity is tested independently in Rust.','Constructor region begins after Object allocation/vector append and stops before native placement. These controls establish numeric endpoints and motion only.'],substitutions=['No instruction patches or replacement arithmetic. Runtime storage and stack/return sentinel are fixture data. Drawing-region controls supply parent virtual coordinates through originalObjectGetCoords5F65A0 via a fixture vtable. Tactical6D2140 is an argument sink for the three computed world coordinates; projection/pixels are excluded. No complete lifecycle, renderer or target acquisition equivalence claim.'],entry_points={'constructor_numeric':0x43592a,'AI':0x4361d0,'beam_geometry':0x435dfd})
        meta['command']='RA2_DIR=/path/to/retail python -m tools.spatial_oracle.spotlight --write'
        OUTPUT.with_suffix('.meta.json').write_text(json.dumps(meta,indent=2)+'\n')
    else: assert data==json.loads(OUTPUT.read_text())
    print('spotlight:5 constructor controls,5 motion traces,3 beam geometries passed')
if __name__=='__main__':main()
