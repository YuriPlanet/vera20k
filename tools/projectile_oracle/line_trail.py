"""Original LineTrail research. Output is original execution, not a Rust model.

Supplied boundaries: allocator/atexit and IStream transport, prepared tactical
RGB565 surface/Z buffer/camera, admitted Object Unlimbo tail entry. No full game.
"""
import os, json, struct, hashlib
from pathlib import Path
from unicorn import UC_HOOK_MEM_WRITE
from unicorn.x86_const import *
from tools.projectile_oracle.bridge_render_art_state import ArtStateReader
from tools.projectile_oracle.bridge_render_inputs import lexical, assets_root
from tools.spatial_oracle.building_body_rules import SP, INI, RULES, dwords
from tools.native_oracle import image_sha256, run_checked, RET_MAGIC

class TrailMachine(ArtStateReader):
    def __init__(self, detail=2, override=None, pixel=False, width=64, height=96):
        self.calls=[]; self.freed=[]; self.current_visit=0; self.pixel=pixel
        self.cadence_mode=None;self.cadence_calls=[]
        self.stream_data=bytearray();self.stream_position=0
        art,_=lexical((assets_root()/'ARTMD.INI').read_bytes(),{'DRAGON','AAHeatSeeker2'})
        super().__init__(art)
        self.typ=self.construct('AAHeatSeeker2')
        self.type_layers=[]
        for f in ('RULESMD.INI','MPBattleMD.ini','Hills.map'):
            secs,_=lexical((assets_root()/f).read_bytes(),{'AAHeatSeeker2'})
            state=self.read_layer(self.typ,secs)
            state['line_trail']=type_fields(self,self.typ)
            self.type_layers.append(state)
        self.invoke(0x556940,0)
        self.invoke(0x5569a0,0)
        self.u.reg_write(UC_X86_REG_ESP,SP)
        self.u.reg_write(UC_X86_REG_ECX,0xa8eb60)
        run_checked(self.u,0x5fa350,0x5fa377)
        self.options_default=self.read32(0xa8eb78)
        self.u.reg_write(UC_X86_REG_ESP,SP)
        self.u.mem_write(0xa8eb78,dwords(detail))
        self.rules=self.alloc(0x2000)
        self.u.mem_write(0x8871e0,dwords(self.rules))
        self.u.reg_write(UC_X86_REG_ESI,self.rules)
        self.u.reg_write(UC_X86_REG_EBX,0)
        run_checked(self.u,0x66784c,0x66785e)
        self.rules_color=[]
        for f in ('RULESMD.INI','MPBattleMD.ini','Hills.map'):
            secs,_=lexical((assets_root()/f).read_bytes(),{'AudioVisual'})
            self.rules_cache(secs)
            admitted=self.invoke(0x526810,RULES,(self.cstring('AudioVisual'),))&255
            if admitted:
                self.u.reg_write(UC_X86_REG_ESP,SP)
                self.u.reg_write(UC_X86_REG_ESI,self.rules)
                self.u.reg_write(UC_X86_REG_EDI,RULES)
                self.u.reg_write(UC_X86_REG_ECX,SP+0x10)
                run_checked(self.u,0x66b77d,0x66b7a7)
                assert self.u.reg_read(UC_X86_REG_ESP)==SP
            self.rules_color.append(dict(file=f,admitted=bool(admitted),rgb=list(self.u.mem_read(self.rules+0x1863,3))))
        if override is not None:self.u.mem_write(self.rules+0x1863,bytes(override))
        self.u.mem_write(0xa8ed40,dwords(0x7eb6d4,self.alloc(4096),1024,1,0,10))
        self.owner=self.alloc(0x180)
        self.invoke(0x466380,self.owner)
        self.u.mem_write(self.owner+0xac,dwords(self.typ))
        self.tactical=self.alloc(0xe00)
        self.u.mem_write(0x887324,dwords(self.tactical))
        self.u.mem_write(0xb0cd48,struct.pack('<Q',0x3fc25e5374344960))
        self.width,self.height=width,height
        self.u.mem_write(0xb0ce30,dwords(width,height))
        self.u.mem_write(0x886fa0,dwords(0,0,width,height))
        self.pixels=self.alloc(width*height*2)
        self.z=self.alloc(width*height*2)
        self.alpha=self.alloc(width*height*2)
        self.surface=self.alloc(0x40);zs=self.alloc(0x40);zo=self.alloc(0x40)
        aas=self.alloc(0x40);aao=self.alloc(0x40)
        for ptr,buf in [(self.surface,self.pixels),(zs,self.z),(aas,self.alpha)]:
            self.u.mem_write(ptr,dwords(0x7e2070,width,height,0,2,buf,width*height*2,0))
        self.u.mem_write(zo,dwords(0,0,width,height,0,zs,self.z,self.z+width*height*2,width*height*2,32768,width))
        self.u.mem_write(0x887644,dwords(zo))
        self.u.mem_write(aao,dwords(0,0,width,height,0,aas,self.alpha,self.alpha+width*height*2,width*height*2,32768,width))
        self.u.mem_write(0x87e8a4,dwords(aao))
        self.u.mem_write(0x88731c,dwords(self.surface))
        for p,v in [(0x8a0dd0,11),(0x8a0dd4,3),(0x8a0dd8,0),(0x8a0ddc,3),(0x8a0de0,5),(0x8a0de4,2)]:self.u.mem_write(p,dwords(v))
        self.u.mem_write(0x8a0de8,struct.pack('<2H',0x7bef,0xf7de))
        self.reset_surface()
        self.trail=0
    def hook(self,u,a,n,d):
        if self.cadence_mode=='frame':
            if a==0x6d3d10:
                sp=u.reg_read(UC_X86_REG_ESP)
                args=self.ints(sp+4,3)
                self.cadence_calls.append(dict(call='Tactical',surface=args[0],redraw_byte=args[1]&255,render_pass=args[2]))
                self.ret(0,12);return
            if a in (RET_MAGIC+0x310,RET_MAGIC+0x320,RET_MAGIC+0x330,0x5d49a0):
                self.ret(0,{RET_MAGIC+0x310:8,RET_MAGIC+0x320:4,RET_MAGIC+0x330:0,0x5d49a0:0}[a]);return
        if self.cadence_mode=='tactical':
            calls={0x6d9ce0:16,0x6dad60:4,0x6da9d0:4,0x6d5030:4,0x53d850:0,0x6d8db0:4,
                   0x5fffa0:0,0x550240:0,0x4c2830:0}
            calls[self.read32(self.read32(self.surface)+8)]=20
            if a in calls:
                self.cadence_calls.append(hex(a));self.ret(0,calls[a]);return
        if a in (RET_MAGIC+0x100,RET_MAGIC+0x200):
            sp=u.reg_read(UC_X86_REG_ESP)
            owner,pointer,length,out=struct.unpack('<4I',u.mem_read(sp+4,16))
            assert owner==self.stream
            if a==RET_MAGIC+0x200:self.stream_data.extend(u.mem_read(pointer,length))
            else:
                assert self.stream_position+length<=len(self.stream_data)
                u.mem_write(pointer,bytes(self.stream_data[self.stream_position:self.stream_position+length]))
                self.stream_position+=length
            if out:u.mem_write(out,dwords(length))
            self.ret(0,16);return
        if a==0x7c978a:self.ret(0);return
        if a==0x7c8b3d:
            self.freed.append(self.read32(u.reg_read(UC_X86_REG_ESP)+4))
        if a in (0x556b70,0x556c00,0x556b30,0x556ad0):
            self.calls.append(dict(visit=self.current_visit,address=hex(a),trail=u.reg_read(UC_X86_REG_ECX)))
        if a in (0x68bcb0,0x65c640,0x65c660,0x65c780,0x65c7e0):
            self.calls.append(dict(visit=self.current_visit,address=hex(a),kind='scenario_id_or_rng'))
        if a==0x4beac0:
            sp=u.reg_read(UC_X86_REG_ESP)
            args=struct.unpack('<7I',u.mem_read(sp+4,28))
            self.calls.append(dict(visit=self.current_visit,address=hex(a),clip=self.ints(args[0],4),
                from_point=self.ints(args[1],2),to_point=self.ints(args[2],2),rgb=list(u.mem_read(args[3],3)),
                intensity=args[4],z_adjust=self.signed(args[5]),z_adjust_end=self.signed(args[6])))
            if not self.pixel:self.ret(0,28);return
        super().hook(u,a,n,d)
    def signed(self,value):return struct.unpack('<i',dwords(value))[0]
    def ints(self,p,n):return list(struct.unpack('<'+'i'*n,self.u.mem_read(p,n*4)))
    def reset_surface(self,color=0xffff,z=65535,alpha=127):
        n=self.width*self.height
        self.before=struct.pack('<H',color)*n;self.before_z=struct.pack('<H',z)*n
        self.u.mem_write(self.pixels,self.before);self.u.mem_write(self.z,self.before_z)
        if alpha=='mixed':
            # Supplied plane, matching the Prism comparison's mixed input.
            # Original4BEAC0 samples walked X/Y; additive laser4BDF00 instead
            # retains the clipped start X while advancing A only along Y.
            values=[(0,1,63,127,255)[(x//9+y//7)%5]
                    for y in range(self.height) for x in range(self.width)]
            raw=struct.pack('<'+'H'*n,*values)
        else:
            raw=struct.pack('<H',alpha)*n
        self.u.mem_write(self.alpha,raw)
    def pixel_state(self):
        raw=bytes(self.u.mem_read(self.pixels,len(self.before)));zr=bytes(self.u.mem_read(self.z,len(self.before_z)))
        return dict(color_sha256=hashlib.sha256(raw).hexdigest(),z_unchanged=zr==self.before_z,
            changed_pixels=[[i%self.width,i//self.width,x[0]] for i,x in enumerate(struct.iter_unpack('<H',raw)) if raw[i*2:i*2+2]!=self.before[i*2:i*2+2]])
    def produce(self,xyz=(256,256,0),color=None,enabled=True):
        self.u.mem_write(self.owner+0x9c,dwords(*xyz))
        self.u.mem_write(self.typ+0x23a,bytes([enabled]))
        if color is not None:self.u.mem_write(self.typ+0x23b,bytes(color))
        self.u.reg_write(UC_X86_REG_ESI,self.owner)
        self.u.reg_write(UC_X86_REG_ESP,SP)
        run_checked(self.u,0x5f514b,0x5f5210)
        assert self.u.reg_read(UC_X86_REG_ESP)==SP
        self.trail=self.read32(self.owner+0xa8)
        return self.trail_state()
    def trail_state(self):
        p=self.trail
        return dict(owner_trail=self.read32(self.owner+0xa8),registry_count=self.read32(0xabcb88),
            registry=[self.read32(self.read32(0xabcb7c)+i*4) for i in range(self.read32(0xabcb88))],
            trail=None if not p else dict(rgb=list(self.u.mem_read(p,3)),owner=self.read32(p+4),
            decrement=self.signed(self.read32(p+8)),head=self.read32(p+12),
            ring=[dict(xyz=self.ints(p+16+i*16,3),strength=self.signed(self.read32(p+28+i*16))) for i in range(32)]))
    def visit(self,xyz=None,detach=False,frame=1000):
        self.current_visit+=1
        if xyz is not None:self.u.mem_write(self.owner+0x9c,dwords(*xyz))
        if detach:self.invoke(0x556b30,self.trail)
        self.u.mem_write(0xa8ed84,dwords(frame))
        begin=len(self.calls)
        self.invoke(0x556d40,0)
        return dict(visit=self.current_visit,binary_frame=frame,**self.trail_state(),calls=self.calls[begin:],freed=list(self.freed))
    def persistence(self):
        self.stream=self.alloc(0x20);vt=self.alloc(0x40)
        self.u.mem_write(self.stream,dwords(vt));self.u.mem_write(vt+12,dwords(RET_MAGIC+0x100,RET_MAGIC+0x200))
        for off in (4,0x1c):self.u.mem_write(0xb0c110+off,dwords(0x7eb6d4,self.alloc(8192),1024,1,0,1000))
        before=self.trail_state()
        saved=self.invoke(0x410320,0,(self.owner,self.stream,0))
        saved_trail=struct.unpack_from('<I',self.stream_data,4+0xa8)[0]
        # Execute original global teardown before loading the saved object into
        # a fresh native Bullet receiver. This is not the complete game loader.
        self.invoke(0x556df0,0)
        after_clear=self.trail_state()
        loaded_owner=self.alloc(0x180);self.invoke(0x466380,loaded_owner)
        loaded=self.invoke(0x46ae70,0,(loaded_owner,self.stream))
        count=self.read32(0xb0c110+0x14);entries=self.read32(0xb0c110+8)
        return dict(save_result=self.signed(saved),load_result=self.signed(loaded),bytes=len(self.stream_data),
            read_bytes=self.stream_position,saved_trail_pointer=saved_trail,
            before=before,after_clear=after_clear,loaded_owner=loaded_owner,
            loaded_trail_pointer=self.read32(loaded_owner+0xa8),loaded_xyz=self.ints(loaded_owner+0x9c,3),
            postload_registry_count=self.read32(0xabcb88),
            swizzle_requests=[dict(token=self.read32(entries+i*8),field_offset=self.read32(entries+i*8+4)-loaded_owner) for i in range(count)])
    def render_frame_passes(self,gate=0,redraw=0):
        display=self.alloc(0x20);dv=self.alloc(0x80);frame=self.alloc(0x20);fv=self.alloc(0x80)
        self.u.mem_write(display,dwords(dv));self.u.mem_write(dv+0x3c,dwords(RET_MAGIC+0x310,RET_MAGIC+0x310))
        self.u.mem_write(frame,dwords(fv));self.u.mem_write(frame+0xc,dwords(redraw))
        self.u.mem_write(fv+0x40,dwords(RET_MAGIC+0x320,RET_MAGIC+0x330))
        self.u.mem_write(0x887640,dwords(display));self.u.mem_write(0xa9fab0,dwords(gate))
        self.u.mem_write(0x887314,dwords(self.surface))
        for p in (0xb0b519,0xa8ef54,0x887368,0xa8b8b4):self.u.mem_write(p,dwords(0))
        self.cadence_mode='frame';self.cadence_calls=[]
        self.invoke(0x4f4480,frame)
        self.cadence_mode=None
        return dict(gate=gate,redraw=redraw,calls=self.cadence_calls)
    def tactical_composite(self,render_pass):
        self.u.mem_write(0x887314,dwords(self.surface));self.u.mem_write(0x8872fc,dwords(self.surface))
        self.u.reg_write(UC_X86_REG_EBP,self.tactical);self.u.reg_write(UC_X86_REG_EDI,render_pass)
        self.u.reg_write(UC_X86_REG_EBX,self.surface);self.u.reg_write(UC_X86_REG_ESP,SP)
        self.cadence_mode='tactical';self.cadence_calls=[];before=len(self.calls)
        reached=run_checked(self.u,0x6d4582,(0x6d4b3e,0x6d4678))
        self.cadence_mode=None
        return dict(render_pass=render_pass,reached=hex(reached),unrelated_calls=self.cadence_calls,
            trail_calls=self.calls[before:],state=self.trail_state())

def pixel_case(name, delta=(256,0,0), old_z=65535, alpha=127, background=0xffff, camera_offset=(0,0), idle_visits=0, clip=(0,0,64,96), z_origin_y=0):
    m=TrailMachine(pixel=True)
    m.u.mem_write(0x886fa0,dwords(*clip))
    m.u.mem_write(m.read32(0x887644)+4,dwords(z_origin_y))
    m.u.mem_write(m.read32(0x87e8a4)+4,dwords(z_origin_y))
    origin=(2688,5248,1040)
    m.produce(origin)
    point=m.alloc(8)
    m.invoke(0x6d2140,m.tactical,(m.owner+0x9c,point))
    screen=m.ints(point,2)
    camera=[screen[0]-32+camera_offset[0],screen[1]-48+camera_offset[1]]
    m.u.mem_write(m.tactical+0xb0,dwords(*camera))
    m.reset_surface(color=background,z=old_z,alpha=alpha)
    m.visit(origin)
    writes=[]
    def capture(u,access,address,size,value,data):
        if m.pixels<=address<m.pixels+len(m.before):
            writes.append(dict(offset=address-m.pixels,size=size,value=value))
    hook=m.u.hook_add(UC_HOOK_MEM_WRITE,capture)
    result=m.visit([x+y for x,y in zip(origin,delta)])
    for _ in range(idle_visits):
        m.reset_surface(color=background,z=old_z,alpha=alpha)
        result=m.visit()
    m.u.hook_del(hook)
    return dict(name=name,input=dict(origin=origin,delta=delta,old_z=old_z,alpha=alpha,background=background,
        camera_offset=camera_offset,idle_visits=idle_visits,clip=clip,z_origin_y=z_origin_y,reset_background_per_composite=True),camera=camera,
        draw_calls=[c for c in m.calls if c['address']=='0x4beac0'],writes=writes,**m.pixel_state())

def options_controls():
    m=TrailMachine();rows=[]
    for raw in (None,'-1','0','1','2','3','junk','999999'):
        m.u.mem_write(0xa8eb78,dwords(m.options_default))
        m.make_ini({'Options':{} if raw is None else {'DetailLevel':raw}})
        m.u.mem_write(0x8870c0,bytes(m.u.mem_read(INI,0x40)))
        m.u.reg_write(UC_X86_REG_ESP,SP-8);m.u.reg_write(UC_X86_REG_ESI,0xa8eb60)
        run_checked(m.u,0x5fa776,0x5fa7b4)
        assert m.u.reg_read(UC_X86_REG_ESP)==SP-8
        rows.append(dict(raw=raw,result=m.read32(0xa8eb78)))
    return rows

def registry_controls():
    m=TrailMachine();owners=[];trails=[]
    for i in range(3):
        if i:
            m.owner=m.alloc(0x180);m.invoke(0x466380,m.owner)
            m.u.mem_write(m.owner+0xac,dwords(m.typ))
        m.produce((256+16*i,256,104*i))
        owners.append(m.owner);trails.append(m.trail)
    before=len(m.calls);m.invoke(0x556d40,0);first=m.calls[before:]
    for trail in (trails[0],trails[2]):m.invoke(0x556b30,trail)
    after_detach=[dict(owner=o,trail=m.read32(o+0xa8)) for o in owners]
    steps=[]
    for i in range(16):
        before=len(m.calls);m.invoke(0x556d40,0)
        count=m.read32(0xabcb88)
        steps.append(dict(index=i,registry=[m.read32(m.read32(0xabcb7c)+4*k) for k in range(count)],
            calls=m.calls[before:],freed=list(m.freed)))
    return dict(owners=owners,trails=trails,first_visit_calls=first,after_detach=after_detach,steps=steps)

def retained_controls():
    m=TrailMachine(detail=0);start=m.produce((0,0,0));zero=m.visit()
    moved=m.visit((256,256,0))
    m.u.mem_write(0xa8eb78,dwords(2));m.u.mem_write(m.typ+0x23b,bytes((9,8,7)))
    m.u.mem_write(m.rules+0x1863,bytes((6,5,4)))
    changed=m.visit((384,256,0));returned_to_zero=m.visit((0,0,0))
    return dict(start=start,initial_zero=zero,moved=moved,changed_sources=changed,returned_to_zero=returned_to_zero)

def type_fields(m,p):
    return dict(enabled=bool(m.u.mem_read(p+0x23a,1)[0]),color=list(m.u.mem_read(p+0x23b,3)),decrement=m.signed(m.read32(p+0x240)) if hasattr(m,'signed') else struct.unpack('<i',m.u.mem_read(p+0x240,4))[0])

def reader_controls():
    rows=[]
    for values in ({},{'UseLineTrail':'yes','LineTrailColor':'216,216,255','LineTrailColorDecrement':'16'},
        {'UseLineTrail':'no','LineTrailColor':'-1,256,257','LineTrailColorDecrement':'-3'},
        {'UseLineTrail':'garbage','LineTrailColor':'1,2','LineTrailColorDecrement':'garbage'},
        {'UseLineTrail':'yes','LineTrailColor':'1h,2,3','LineTrailColorDecrement':'0'},
        {'UseLineTrail':'yes','LineTrailColor':'1, 2, 3suffix','LineTrailColorDecrement':'25h'},
        *({'LineTrailColor':raw} for raw in ('','   ','junk','1%','1,2%,3','-257,+258,511','1 ,2,3','+4,-5,6'))):
        m=ArtStateReader({'DRAGON':values,'PLAIN':{}})
        typ=m.construct('SHOT');ctor=type_fields(m,typ)
        m.read_layer(typ,{'SHOT':{'Image':'DRAGON'}});first=type_fields(m,typ)
        m.read_layer(typ,{'SHOT':{'Image':'PLAIN'}});omitted=type_fields(m,typ)
        rows.append(dict(input=values,constructor=ctor,first=first,omitted=omitted))
    return rows

def rgb_stack_controls():
    # Malformed scanf leaves outputs uninitialized. These are explicitly
    # supplied caller-stack controls, not retail default values.
    class PoisonArt(ArtStateReader):
        def hook(self,u,a,n,d):
            if a==0x474bab:
                sp=u.reg_read(UC_X86_REG_ESP)
                u.mem_write(sp+8,dwords(0x11111111,0x22222222,0x33333333))
            super().hook(u,a,n,d)
    rows=[]
    for raw in ('1,2','1h,2,3','junk','1 ,2,3','1,2,3'):
        m=PoisonArt({'DRAGON':{'LineTrailColor':raw}})
        typ=m.construct('SHOT');m.read_layer(typ,{'SHOT':{'Image':'DRAGON'}})
        rows.append(dict(raw=raw,supplied_local_dwords=['11111111','22222222','33333333'],result=type_fields(m,typ)))
    return rows

def generate():
    rows=[]
    for detail in (0,1,2):
        for override in (None,[9,0,0]):
            m=TrailMachine(detail=detail,override=override)
            identity=m.read32(m.read32(0xa8b230)+0x214)
            behavior_begin=len(m.calls)
            start=m.produce()
            steps=[m.visit([256+16*i,256,104 if i%2 else 0],frame=1000 if i<4 else 1000+i) for i in range(1,37)]
            steps.extend(m.visit(frame=2000) for _ in range(3))
            steps.append(m.visit(detach=True,frame=2000))
            for _ in range(20):
                if not m.read32(0xabcb88):break
                steps.append(m.visit(frame=2000))
            rows.append(dict(detail=detail,override=override,options_default=m.options_default,
                rules_color=m.rules_color,initial=start,steps=steps,
                identity_before=identity,identity_after=m.read32(m.read32(0xa8b230)+0x214),
                rng_or_id_calls=[c for c in m.calls[behavior_begin:] if c.get('kind')=='scenario_id_or_rng']))
    m=TrailMachine();disabled=m.produce(enabled=False)
    persistence=[]
    for moved in (False,True):
        m=TrailMachine();m.produce();m.visit()
        if moved:m.visit((384,256,104))
        persistence.append(dict(moved=moved,**m.persistence()))
    pixels=[pixel_case('geometry_'+str(i),delta) for i,delta in enumerate([
        (256,0,0),(0,256,0),(256,256,0),(-256,256,0),(-256,-256,0),
        (0,0,208),(256,0,1000),(0,0,-208),(0,0,0)])]
    pixels.extend(pixel_case('depth_'+str(z),old_z=z) for z in (0,32560,32568,32570,32580,32768,65535))
    pixels.extend(pixel_case('alpha_'+str(a),alpha=a) for a in (0,1,64,127,128,255))
    pixels.extend(pixel_case('background_'+str(c),background=c) for c in (0,0x7bef,0xf81f,0x1234))
    pixels.extend(pixel_case('clip_'+str(i),camera_offset=offset) for i,offset in enumerate([
        (-31,0),(32,0),(62,0),(63,0),(0,-47),(0,48),(0,62),(128,128)]))
    pixels.extend(pixel_case('fade_'+str(i),idle_visits=i) for i in (1,7,14,15,16))
    pixels.extend(pixel_case('origin_'+str(i),clip=(4,7,48,72),z_origin_y=origin) for i,origin in enumerate((0,7,21)))
    pixels.extend(pixel_case('alpha_mixed_'+name,delta,alpha='mixed') for name,delta in (
        ('x',(256,0,0)),('y',(256,256,0)),('z',(256,0,1000)),
        ('clipped_reversed',(-768,0,0))))
    cadence=[]
    for render_pass in (0,1,2,3):
        m=TrailMachine();m.produce();cadence.append(m.tactical_composite(render_pass))
    m=TrailMachine();frame_passes=[m.render_frame_passes(gate,redraw) for gate in (0,1) for redraw in (0,2)]
    return dict(native_sha256=image_sha256(),
        source_files={f:hashlib.sha256((assets_root()/f).read_bytes()).hexdigest() for f in ('ARTMD.INI','RULESMD.INI','MPBattleMD.ini','Hills.map','dragon.shp')},
        type_layers=m.type_layers,reader_controls=reader_controls(),rgb_stack_controls=rgb_stack_controls(),rows=rows,disabled=disabled,
        options=options_controls(),persistence=persistence,pixels=pixels,
        tactical_composite=cadence,render_frame_passes=frame_passes,registry=registry_controls(),retained=retained_controls(),
        ordered_pixel_cases=[ordered_pixel_case(n,a) for n,a in ((1,127),(8,127),(128,127),(512,127),(8,64))])

def ordered_pixel_case(count,alpha=127):
    m=TrailMachine(pixel=True)
    origin=(2688,5248,1040);delta=(256,0,0)
    def registry():
        m.u.mem_write(SP,dwords(RET_MAGIC));m.u.reg_write(UC_X86_REG_ESP,SP);m.u.reg_write(UC_X86_REG_ECX,0)
        run_checked(m.u,0x556d40,RET_MAGIC,count=20000000)
    owners=[];colors=[]
    for i in range(count):
        if i:
            m.owner=m.alloc(0x180);m.invoke(0x466380,m.owner)
            m.u.mem_write(m.owner+0xac,dwords(m.typ))
        color=[(216,216,255),(255,64,0),(32,160,255)][i%3]
        m.produce(origin,color=color);owners.append(m.owner);colors.append(color)
    point=m.alloc(8);m.invoke(0x6d2140,m.tactical,(m.owner+0x9c,point))
    screen=m.ints(point,2);camera=[screen[0]-32,screen[1]-48]
    m.u.mem_write(m.tactical+0xb0,dwords(*camera));m.reset_surface(alpha=alpha)
    registry()
    for owner in owners:m.u.mem_write(owner+0x9c,dwords(*[x+y for x,y in zip(origin,delta)]))
    before=len(m.calls);registry()
    return dict(count=count,input=dict(origin=origin,delta=delta,alpha=alpha,background=65535,old_z=65535,colors=colors),
        camera=camera,draw_calls=[c for c in m.calls[before:] if c['address']=='0x4beac0'],**m.pixel_state())

def metadata():
    m=ArtStateReader({})
    spans=[(0x556940,0x556a20),(0x556a20,0x556e83),(0x5f514b,0x5f5210),
        (0x5f5e80,0x5f5f16),(0x474b50,0x474c0b),(0x4beac0,0x4bf645),
        (0x4c1b50,0x4c1b76),(0x4cac40,0x4cacae),(0x6d4582,0x6d4678),
        (0x5fa350,0x5fa377),(0x5fa776,0x5fa7b4)]
    return dict(native_sha256=image_sha256(),
        command='VERA20K_PROJECTILE_RENDER_ASSETS=/path/to/extracted-inputs python -m tools.projectile_oracle.line_trail --check',
        coverage='Selected DRAGON Object producer, LineTrail registry/update/draw/detach and bounded Bullet save/load',
        substitutions=[
            'Physical extracted INI bytes become supplied cached native INI indices; no original archive walk',
            'Physical dragon.shp bytes provided by existing original Object image-loader boundary',
            'Prior Object Unlimbo world admission supplied at5F514B',
            'Allocator/operator-delete/atexit and IStream external services supplied',
            'Prepared original RGB565 BSurface with supplied camera,clip,ZBuffer and ABuffer planes',
            'Cadence controls substitute unrelated Tactical draw families/display virtuals/time services',
            'Malformed RGB scratch-poison controls intentionally supply uninitialized caller-stack locals',
            'Save/load executes one-object path and one-trail global clear, not full scenario restore'],
        limits=[
            'No Rust/GPU/native full-scene comparison claimed by this corpus alone',
            'No outer OS/network scheduling clock or fixed real-time frame rate established',
            'No failed/repeated Unlimbo, full postload reconstruction or all multi-trail ClearAll claim',
            'ZBuffer row seed32768 and RGB565 format globals are supplied established renderer inputs',
            "Four mixed-alpha rows supply (0,1,63,127,255)[(x//9+y//7)%5] per pixel, exercising X/Y/Z-dominant and reversed/clipped4BEAC0 walks. This establishes that consumer's A sample, not the scene ABuffer producer."],
        original_slices=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',
            sha256=hashlib.sha256(bytes(m.u.mem_read(a,b-a))).hexdigest(),
            hex=bytes(m.u.mem_read(a,b-a)).hex()) for a,b in spans],
        dependencies={p:hashlib.sha256(Path(p).read_bytes()).hexdigest() for p in (
            'tools/projectile_oracle/line_trail.py',
            'tools/projectile_oracle/bridge_render_art_state.py',
            'tools/projectile_oracle/bridge_render_inputs.py',
            'tools/spatial_oracle/building_body_rules.py','tools/native_oracle.py')})

if __name__=='__main__':
    import argparse
    parser=argparse.ArgumentParser();parser.add_argument('--check',action='store_true');args=parser.parse_args()
    result=json.loads(json.dumps(generate()));path=Path(__file__).with_suffix('.json')
    meta=json.loads(json.dumps(metadata()));meta_path=path.with_suffix('.meta.json')
    if args.check:
        assert result==json.loads(path.read_text())
        assert meta==json.loads(meta_path.read_text())
        print('PASS original LineTrail producer/update/pixels/detach/persistence controls')
    else:
        path.write_text(json.dumps(result,indent=2)+'\n')
        meta_path.write_text(json.dumps(meta,indent=2)+'\n')
        print(path)
