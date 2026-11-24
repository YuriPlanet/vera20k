"""Original Tactical6D47FB and Techno705860 airstrike beam, actual4BEAC0 pixels."""
from pathlib import Path
import struct
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_ECX, UC_X86_REG_EAX, UC_X86_REG_ESI, UC_X86_REG_EIP
from tools import native_oracle as n
from tools.procedural_drawing_oracle import rally
from tools.spatial_oracle.bridge_damage_admission import words, SP, MEM, read32, call
from tools.rmg_oracle.gen_rng_vectors import seeded_struct

def ints(u,p,k):
    return list(struct.unpack('<'+'i'*k,u.mem_read(p,k*4)))

def machine(case):
    m=rally.Rally(case);u=m.u
    u.mem_write(rally.VTABLE+0x84,words(0x4C1AB0))
    zsurf,zobj,zbuf=MEM+0x80000,MEM+0x80100,MEM+0x90000
    zbytes=struct.pack('<H',case.get('old_z',65535))*(160*120)
    u.mem_write(zbuf,zbytes)
    u.mem_write(zsurf,words(0x7e2070,160,120,0,2,zbuf,len(zbytes),0))
    u.mem_write(zobj,words(0,0,160,120,0,zsurf,zbuf,zbuf+len(zbytes),len(zbytes),32768,160))
    u.mem_write(0x887644,words(zobj));u.mem_write(0x8a0de8,struct.pack('<2H',0x7bef,0xf7de))
    u.mem_write(0x886b88,seeded_struct(case.get('seed',31)))
    return m,zbuf,zbytes

def execute(case, *, tactical=False):
    m,zbuf,zbytes=machine(case);u=m.u;events=[]
    original=bytes(u.mem_read(0x705860,0x43C))
    def observe(_u,a,_s,_d):
        sp=u.reg_read(UC_X86_REG_ESP)
        if a==0x410964:
            u.reg_write(UC_X86_REG_EAX,case.get('wall_ms',0));u.reg_write(UC_X86_REG_ESP,sp+4);u.reg_write(UC_X86_REG_EIP,read32(u,sp))
        if a==0x65c7e0:
            events.append(dict(kind='rng',receiver=u.reg_read(UC_X86_REG_ECX),args=ints(u,sp+4,2)))
        if a==0x705860:events.append(dict(kind='beam',coords=ints(u,sp+4,6)))
        if a==0x4beac0:
            args=ints(u,sp+4,7);events.append(dict(kind='line',from_xy=ints(u,args[1],2),to_xy=ints(u,args[2],2),rgb=list(u.mem_read(args[3],3)),intensity=args[4],z_adjust=args[5:]))
        if a==0x7ba5e0:
            args=ints(u,sp+4,3);events.append(dict(kind='solid',from_xy=ints(u,args[0],2),to_xy=ints(u,args[1],2),word=args[2]))
    u.hook_add(UC_HOOK_CODE,observe)
    def opaque(key):
        if key not in case: return
        a,b,color=case[key]
        u.mem_write(MEM+0x31000,words(*a,*b))
        call(u,0x7BA5E0,rally.SURFACE,(MEM+0x31000,MEM+0x31008,color))
    opaque('opaque_before')
    if tactical:
        source,target,manager,st,tt=[MEM+x for x in (0xa0000,0xa1000,0xa2000,0xa3000,0xa5000)]
        u.mem_write(source,words(0x7eb058));u.mem_write(source+0x14,words(5));u.mem_write(source+0x9c,words(5248,5248,0));u.mem_write(source+0x6c0,words(st))
        u.mem_write(source+0x294,words(0 if case.get('no_manager') else manager))
        u.mem_write(manager+0x4c,words(source+4 if case.get('borrowed_link') else source))
        u.mem_write(manager+0x50,words(0 if case.get('no_target') else target))
        u.mem_write(manager+0x3d,bytes([case.get('active',0)]))
        u.mem_write(target,words(0x7eb058 if case.get('non_building') else 0x7e3ebc));u.mem_write(target+0x520,words(tt))
        u.mem_write(target+0x9c,words(5504,5248,0));u.mem_write(st+0x898,words(0,80,0,100))
        u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,source)
        n.run_checked(u,0x6d47fb,0x6d48fa,count=2000000)
    else:
        u.mem_write(SP,words(n.RET_MAGIC,*case['source'],*case['target']));u.reg_write(UC_X86_REG_ESP,SP)
        n.run_checked(u,0x705860,n.RET_MAGIC,count=2000000,required_addresses=(0x705860,0x4beac0))
    opaque('opaque_after')
    if bytes(u.mem_read(0x705860,len(original)))!=original: raise n.OracleError("beam code changed")
    if bytes(u.mem_read(zbuf,len(zbytes)))!=zbytes: raise n.OracleError("beam wrote Z")
    if bytes(u.mem_read(rally.ALPHA,len(zbytes)))!=struct.pack('<'+'H'*len(m.alpha),*m.alpha): raise n.OracleError("beam wrote A")
    return dict(input=case,events=events,pixels=m.pixels())

def inputs():
    base=dict(source=[5248,5248,100],target=[5504,5248,150],seed=31,camera=[-50,560],clip=[0,0,160,120],old_z=65535,alpha='clear')
    yield dict(base,name='base')
    for seed in (1,3,4,15,34,80):yield dict(base,name=f'seed{seed}',seed=seed)
    yield dict(base,name='reverse',source=base['target'],target=base['source'])
    yield dict(base,name='lower',target=[5504,5248,0])
    yield dict(base,name='vertical',target=[5504,5504,150])
    yield dict(base,name='steep_z',target=[5504,5248,416],camera=[-50,540])
    yield dict(base,name='cross_clip',camera=[0,560])
    yield dict(base,name='interior_clip',clip=[20,20,100,80])
    yield dict(base,name='alpha0',alpha='black')
    yield dict(base,name='mixed_alpha',alpha='mixed')
    yield dict(base,name='occluded',old_z=0)
    yield dict(base,name='coincident',target=base['source'])
    yield dict(base,name='opaque_overlap',opaque_before=[[45,42],[85,42],0x07E0],opaque_after=[[65,37],[65,55],0x001F])

def vectors():
    base=dict(camera=[-50,560],clip=[0,0,160,120])
    gates=[dict(name='no_manager',no_manager=True),dict(name='borrowed',borrowed_link=True),dict(name='no_target',no_target=True),dict(name='non_building',non_building=True),dict(name='inactive',active=0),dict(name='active',active=1)]
    gates += [dict(name=f'clock{ms}',wall_ms=ms) for ms in (0,128,256,1024,0xFFFFFF80)]
    return dict(size=[160,120],background_word=rally.BACKGROUND,cases=[execute(c) for c in inputs()],tactical=[execute(dict(base,**c),tactical=True) for c in gates])

def metadata():
    return n.provenance(scope='Original Tactical6D47FB airstrike branch,705860 and4BEAC0 RGB565 pixels', assumptions=['Prepared source Infantry/Building manager fields,primary FLH80,0,100 and source pose; no complete Scenario/lifecycle replay.','Direct cases supply explicit endpoints,160x120 RGB565 destination,ABuffer andZBuffer; original line color RNG,geometry,clipping and pixel bodies execute.'],substitutions=['Existing Rally memory-surface methods replace DirectDraw storage/locking; IsDSurface uses native4C1AB0. No gameplay .text patches.','Windows timeGetTime supplies the explicit wall_ms value. Main RNG is seeded using the existing native-fixture seed owner.'],entry_points={'tactical':0x6D47FB,'beam':0x705860,'line':0x4BEAC0})


if __name__=='__main__':
    n.finish_vectors(vectors,Path(__file__).with_suffix('.json'),provenance=metadata)
