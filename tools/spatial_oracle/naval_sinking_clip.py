"""Original sinking waterline rectangle and draw-gate evidence.

Unit draw admission executes original projection before the body boundary.
Interior raster locals remain supplied, with the camera sequence consuming the
separate physical AEGIS raster packet. This is not a captured live-process frame
and does not establish the ambient native x87 control word.
"""
from pathlib import Path
import hashlib, json, struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import *
from tools.native_oracle import load_image, run_checked, finish_vectors, provenance, STACK_BASE, STACK_SIZE, SCRATCH, SCRATCH_SIZE, RET_MAGIC

HERE=Path(__file__).resolve().parent
ACTOR=SCRATCH;TACTICAL=SCRATCH+0x1000;RECT=SCRATCH+0x2000;POINT=SCRATCH+0x2100
SP=STACK_BASE+0x80000
SLICES={'unit_clip':(0x73BEA4,0x73BF7B),'techno_clip':(0x70709E,0x70716D),'shadow_gate':(0x73C1D2,0x73C202),'extras_gate':(0x6F5190,0x6F51B0),'ctor':(0x6F2F1E,0x6F2F37)}
SLICES.update(unit_draw_entry=(0x73B0B0,0x73B13C),projection=(0x6D2140,0x6D2272),viewport_store=(0x6D5F60,0x6D5F8D))
def dwords(*x):return struct.pack('<'+'I'*len(x),*(a&0xffffffff for a in x))
def rectangle(u,p):return list(struct.unpack('<4i',u.mem_read(p,16)))

def fixture():
 u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(u)
 u.mem_map(STACK_BASE,STACK_SIZE);u.mem_map(SCRATCH,SCRATCH_SIZE);u.mem_map(RET_MAGIC,0x1000)
 u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EBP,ACTOR);u.reg_write(UC_X86_REG_FPCW,0xE7F)
 u.mem_write(0x887324,dwords(TACTICAL))
 return u

def step(u,c,kind):
 u.mem_write(SP,bytes(0x800));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EBP,ACTOR)
 u.mem_write(ACTOR+0x3CD,bytes([c['sinking']]))
 u.mem_write(TACTICAL+0xB4,dwords(c['camera_world_y']));u.mem_write(0x886FA8,dwords(c['view_width']))
 if kind=='unit':
  u.mem_write(SP+0x1CC,dwords(c['draw_y'],*c['caller_rect']))
  u.mem_write(0xB1CFC4,dwords(c['raster_y']));u.mem_write(0xB1CFCC,dwords(c['raster_height']))
  start,end=0x73BEA4,0x73BF7B;out=SP+0x50
 else:
  u.mem_write(RECT,dwords(*c['caller_rect']));u.mem_write(POINT,dwords(0,c['draw_y']))
  u.mem_write(SP+0x14C,dwords(POINT,RECT));u.mem_write(SP+0x38,dwords(c['raster_y']));u.mem_write(SP+0x48,dwords(c['raster_height']))
  u.mem_write(0x887314,dwords(0x11110000));u.mem_write(0xB1D13C,dwords(0x11110000 if c.get('offscreen_surface') else 0x22220000))
  start,end=0x70709E,0x70716D;out=SP+0x20
 writes=[];calls=[]
 def write(u,a,p,n,v,d):
  if ACTOR<=p<ACTOR+0x800:writes.append(dict(pc=hex(u.reg_read(UC_X86_REG_EIP)),offset=hex(p-ACTOR),size=n,value=v))
 def code(u,p,n,d):
  if p==0x421B60:calls.append(dict(pc=hex(p),caller=rectangle(u,u.reg_read(UC_X86_REG_EDX)),candidate=rectangle(u,struct.unpack('<I',u.mem_read(u.reg_read(UC_X86_REG_ESP)+4,4))[0])))
 h=u.hook_add(UC_HOOK_MEM_WRITE,write);h2=u.hook_add(UC_HOOK_CODE,code)
 before=struct.unpack('<h',u.mem_read(ACTOR+0x3CA,2))[0]
 run_checked(u,start,end,count=20000)
 u.hook_del(h);u.hook_del(h2)
 return dict(waterline_before=before,waterline_after=struct.unpack('<h',u.mem_read(ACTOR+0x3CA,2))[0],clip=rectangle(u,out),calls=calls,writes=writes)

def cases():
 base=dict(sinking=1,camera_world_y=1000,view_width=640,draw_y=200,raster_y=100,raster_height=40,caller_rect=[0,0,640,480])
 states=[('first',0,{}),('repeat',1212,{'draw_y':250}),('camera_down',1212,{'camera_world_y':1100}),('camera_above',1212,{'camera_world_y':500}),('fully_clipped',1212,{'camera_world_y':1300}),('partial_caller',1212,{'caller_rect':[20,190,100,100]}),('caller_below',1212,{'caller_rect':[20,230,100,100]}),('flag0_no_cache',0,{'sinking':0}),('flag0_retained',1212,{'sinking':0}),('negative_cache',-1,{}),('signed_wrap',0,{'camera_world_y':32760}),('raw255',0,{'sinking':255})]
 rows=[]
 for kind in ('unit','techno'):
  for name,cache,extra in states:
   c=dict(base,**extra)
   if kind=='techno':c['raster_y']-=128
   u=fixture();u.mem_write(ACTOR+0x3CA,struct.pack('<h',cache));r=step(u,c,kind)
   rows.append(dict(name=kind+'_'+name,input=dict(kind=kind,waterline=cache,**c),output=r))
  if kind=='techno':
   for cache in (0,1212):
    c=dict(base,offscreen_surface=True);c['raster_y']-=128;u=fixture();u.mem_write(ACTOR+0x3CA,struct.pack('<h',cache))
    rows.append(dict(name=f'techno_offscreen_{cache}',input=dict(kind=kind,waterline=cache,**c),output=step(u,c,kind)))
 # Retained object across genuine successive original calls, with camera change.
 for kind in ('unit','techno'):
  u=fixture();cs=[]
  for extra in ({},{'draw_y':250},{'camera_world_y':1100,'draw_y':250},{'camera_world_y':1300,'draw_y':250}):
   c=dict(base,**extra)
   if kind=='techno':c['raster_y']-=128
   cs.append(dict(input=c,output=step(u,c,kind)))
  rows.append(dict(name=kind+'_retained_sequence',input=dict(kind=kind,waterline=0),sequence=cs))
 return rows

def gates():
 rows=[]
 for flag in (0,1,255):
  u=fixture();u.mem_write(ACTOR+0x3CD,bytes([flag]));u.mem_write(ACTOR+0x6D3,b'\1')
  end=run_checked(u,0x73C1D2,(0x73C202,0x73C5C9),count=1000)
  shadow=dict(endpoint=hex(end),shadow_tail_entered=end==0x73C202,flag6d3=u.mem_read(ACTOR+0x6D3,1)[0])
  u=fixture();u.mem_write(ACTOR+0x3CD,bytes([flag]));u.reg_write(UC_X86_REG_ECX,ACTOR);u.mem_write(SP,dwords(RET_MAGIC,RECT,POINT))
  end=run_checked(u,0x6F5190,(0x6F51B0,RET_MAGIC),count=1000)
  rows.append(dict(input=dict(sinking=flag),shadow=shadow,extras=dict(endpoint=hex(end),extras_body_entered=end==0x6F51B0)))
 return rows

def unit_admission(u,xyz,camera,viewport):
 """Run the original Unit/Object entry with its real vtable and projection.

 The selected Unit body itself is an endpoint, not a substituted call.
 Full scenario/Display iteration, redraw production and raster work excluded.
 """
 u.mem_write(ACTOR,dwords(0x7F5C70));u.mem_write(ACTOR+0x9C,dwords(*xyz))
 u.mem_write(ACTOR+0x80,b'\x01\x00');u.mem_write(ACTOR+0x418,b'\0')
 u.mem_write(TACTICAL+0xB0,dwords(*camera))
 u.mem_write(0xB0CD48,struct.pack('<Q',0x3FC25E5374344960))
 u.mem_write(0x886FA0,dwords(0,0,*viewport));u.mem_write(RECT,dwords(0,0,*viewport))
 # Both Set_View_Dimensions4A89B8 and Scenario Full_Init687620 pass886FA0
 # to this retained viewport writer. Stop before its camera-clamp work.
 u.mem_write(SP,dwords(RET_MAGIC,0x886FA0));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,TACTICAL)
 run_checked(u,0x6D5F60,0x6D5F8D,count=1000)
 written=rectangle(u,0xB0CE28)
 u.mem_write(SP,dwords(RET_MAGIC,RECT,0,0));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,ACTOR)
 projected=[]
 def observe(u,pc,n,d):
  if pc==0x6D221E:
   p=u.reg_read(UC_X86_REG_EAX);projected.extend(struct.unpack('<2i',u.mem_read(p,8)))
 h=u.hook_add(UC_HOOK_CODE,observe)
 end=run_checked(u,0x73B0B0,(RET_MAGIC,0x73CEC0),count=10000,required_addresses=(0x5F4B10,0x6D2140))
 u.hook_del(h)
 out=dict(admitted=end==0x73CEC0,endpoint=hex(end),projected=projected,retained_viewport=written)
 if out['admitted']:
  sp=u.reg_read(UC_X86_REG_ESP);point,rect=struct.unpack('<2I',u.mem_read(sp+4,8))
  out.update(body_anchor=list(struct.unpack('<2i',u.mem_read(point,8))),caller_rect=rectangle(u,rect))
 return out

def admission_cases():
 # Native projection of this declared coordinate is1620,2565. Vary camera
 # inputs only; all expected admission/anchor values are original outputs.
 xyz=[29056,15232,208];viewport=[640,480]
 cameras=[(1620-x,2365) for x in(-361,-360,-359,760,761,840,999,1000,1001)]
 cameras += [(1320,2565-y) for y in(-181,-180,-179,659,660,661)]
 rows=[]
 for camera in cameras:
  rows.append(dict(input=dict(xyz=xyz,camera=camera,viewport=viewport),output=unit_admission(fixture(),xyz,camera,viewport)))
 # Full dispatch admission followed by the original clip slice, using actual
 # cached AEGIS bounds. Z208->8 is an explicit pair of retained coordinates;
 # the independent UnitAI packet establishes the five-lepton reached cadence.
 raster=json.loads((HERE/'naval_draw_bounds.json').read_text())['cases'][0]['cached_dirty']
 u=fixture();sequence=[]
 for xyz,camera in [([29056,15232,208],[780,2365]),([29056,15232,8],[780,2365]),([29056,15232,8],[1320,2365])]:
  entry=unit_admission(u,xyz,camera,viewport);assert entry['admitted']
  c=dict(sinking=1,camera_world_y=camera[1],view_width=viewport[0],draw_y=entry['body_anchor'][1],raster_y=raster[1],raster_height=raster[3],caller_rect=entry['caller_rect'])
  sequence.append(dict(input=dict(xyz=xyz,camera=camera,viewport=viewport),entry=entry,clip_input=c,output=step(u,c,'unit')))
 return dict(cases=rows,aegis_cached_dirty=raster,sequence=sequence)

def generate():
 u=fixture();u.mem_write(ACTOR+0x3CA,b'\xA5'*5);u.reg_write(UC_X86_REG_ESI,ACTOR);u.reg_write(UC_X86_REG_EBX,0)
 run_checked(u,0x6F2F1E,0x6F2F37,count=1000)
 return dict(schema=2,ctor=dict(supplied_bx=0,waterline=struct.unpack('<h',u.mem_read(ACTOR+0x3CA,2))[0],byte3cc=u.mem_read(ACTOR+0x3CC,1)[0],sinking=u.mem_read(ACTOR+0x3CD,1)[0],seen=u.mem_read(ACTOR+0x3CE,1)[0]),cases=cases(),gates=gates(),admission=admission_cases())

def metadata():
 u=fixture()
 p=provenance(scope=__doc__,assumptions=['Original renderer frame locals are supplied: caller clip, draw anchor Y, raster bounds Y/height, Tactical+B4 world camera Y, view width and current/offscreen surface identity. No values derive from Rust.', 'Original Unit73BEA4..73BF7B cached composite and Techno70709E..70716D generic piece rectangles run through native421B60. Unit capture includes -128 because its cached raster uses a128px center; generic inputs use corresponding shifted raster bounds. First capture leaves the caller clip unchanged.', 'Constructor field stores6F2F1E..6F2F37 execute with the enclosing constructor zero-register invariant supplied. Raw-load/no-init persistence is separately executed in naval_lifetime_controls.', 'Shadow gate73C1D2..73C202/73C5C9 and full early DrawExtras6F5190 return for nonzero sinking execute. Sinking0 stops before the remaining ordinary tails. Raw255 is a supplied legacy-byte control, not an active producer claim.', 'All executed instructions in this packet are integer arithmetic, stores and branches. FPCW0E7F is supplied and immaterial to these slices. No RNG calls, timer writes or detach calls are present in these executed slices; preceding raster production excluded.'],substitutions=['No calls are replaced; native421B60 rectangle intersection executes. Projection, raster creation, full render dispatcher and surface blits are outside the measured boundaries.'],entry_points={k:v[0] for k,v in SLICES.items()}|{'intersection':0x421B60})
 p['original_slices']={k:dict(start=hex(a),end=hex(b),bytes=bytes(u.mem_read(a,b-a)).hex()) for k,(a,b) in SLICES.items()}
 p['assumptions'][-1]='The original clip/gate slices are integer-only. Added Unit73B0B0->Object5F4B10->CoordsToClient2 6D2140 executes native projection and ftol under supplied FPCW0E7F and startup scalar bits3FC25E5374344960; supplied XYZ/camera and redraw-ready Unit with original vtable7F5C70. No RNG, timer or detach calls occur in these measured draw boundaries.'
 p['assumptions'].append('Native viewport store6D5F60..6D5F8D executes with supplied tactical rect886FA0, matching original caller4A89B8 and Scenario687620. Full camera clamping/window creation excluded. The retained camera sequence consumes facing-zero cached_dirty from the separately executable physical AEGIS raster packet; no Rust raster inputs.')
 p['substitutions']=['No calls or executable instructions are replaced. Unit body entry73CEC0 is a stop boundary; the separately measured clip slice consumes its anchor/caller rectangle plus the physical raster bounds. Full Display iteration, intervening sink ticks, whole Unit raster dispatch and surface blits are outside this packet.']
 p['aegis_raster_packet_sha256']=hashlib.sha256((HERE/'naval_draw_bounds.json').read_bytes()).hexdigest()
 return p

if __name__=='__main__':finish_vectors(generate,HERE/'naval_sinking_clip.json',provenance=metadata)
