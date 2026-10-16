"""Physical AEGIS native reader, flat Ship draw matrix, raster and clip composition.

The selected-type draw dispatch frame and file IO are explicit boundaries.
No whole Windows renderer, full Unit draw or GPU parity claim is made.
"""
from naval_occupants import *
from tools.voxel_oracle.raster import render_native
from tools.voxel_oracle.lighting import native_camera,native_multiply
import naval_sinking_clip as clip

ART=ASSETS/'ARTMD.INI'
MODE=ASSETS/'MPBattleMD.ini'


def read_art(m):
 m.phase='setup';u=m.u
 ctor=dict(image=m.string(m.typ+0x1F8),voxel=u.mem_read(m.typ+0x236,1)[0])
 layers=[]
 for name,p in [('RULESMD.INI',ASSETS/'RULESMD.INI'),('MPBattleMD.ini',MODE),('XShrapnel.MAP',ASSETS/'XShrapnel.MAP')]:
  sec,lines=lexical(p.read_bytes(),{'AEGIS'});m.make_ini(sec)
  before=m.string(m.typ+0x1F8);default=m.cstring(before)
  m.invoke(0x528A10,INI,(m.typ+0x24,m.cstring('Image'),default,m.typ+0x1F8,25))
  layers.append(dict(name=name,sha256=sha(p.read_bytes()),image_key=sec.get('AEGIS',{}).get('Image'),before=before,after=m.string(m.typ+0x1F8)))
 image=m.string(m.typ+0x1F8);sec,_=lexical(ART.read_bytes(),{image});m.make_ini(sec)
 u.mem_write(0x887180,bytes(u.mem_read(INI,0x40)))
 m.block(0x5F961C,0x5F963A,{UC_X86_REG_EBX:m.typ,UC_X86_REG_ESI:m.typ+0x1F8})
 output=dict(ctor=ctor,layers=layers,art=dict(name=ART.name,sha256=sha(ART.read_bytes()),section=image,keys=sec[image]),voxel_after=u.mem_read(m.typ+0x236,1)[0])
 m.phase='measure';return output


def route(m,present):
 u=m.u;placeholder=m.alloc(16);u.mem_write(m.typ+0xB0,dwords(placeholder if present else 0))
 rect=m.alloc(16);u.mem_write(rect,dwords(0,0,640,480));u.mem_write(SP,bytes(0x200))
 u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,m.actor);u.reg_write(UC_X86_REG_EDI,m.typ);u.reg_write(UC_X86_REG_EBX,rect);u.reg_write(UC_X86_REG_EBP,0)
 end=run_checked(u,0x73D30E,(0x73B470,0x73D361,0x73D395),count=1000)
 return dict(input=dict(selected_type='AEGIS',tube_index=u.mem_read(m.actor+0x684,1)[0],vxl_pointer_present=present),voxel=u.mem_read(m.typ+0x236,1)[0],vtable554=hex(m.read32(m.read32(m.actor)+0x554)),endpoint=hex(end))


def dirty(rect,kind):
 u=clip.fixture();x,y,_,_,w,h=rect
 p=clip.SCRATCH+0x3000;anchor=clip.SCRATCH+0x3100;u.mem_write(anchor,dwords(128,128));u.mem_write(0xB1CFC0,dwords(0,0,0,0))
 if kind=='cached':
  u.mem_write(p,struct.pack('<4h',x,y,w,h));u.reg_write(UC_X86_REG_ESI,p);u.reg_write(UC_X86_REG_ECX,anchor)
  run_checked(u,0x70755D,0x707678,count=1000)
 else:
  u.mem_write(clip.SP+0x38,dwords(*rect));u.mem_write(clip.SP+0x68,dwords(anchor));u.mem_write(clip.SP+0x74,dwords(h,w));u.reg_write(UC_X86_REG_ESI,w)
  run_checked(u,0x7068EE,0x7069F6,count=1000)
 return clip.rectangle(u,0xB1CFC0)


def generate():
 m=Native(next(c for c in inputs() if c['name']=='west_water_head_road_dz0'));reader=read_art(m)
 routes=[route(m,True),route(m,False)]
 # Original fatal receiver produces sinking on the same retained object.
 m.u.mem_write(m.typ+0xB0,dwords(0));m.run();u=m.u
 camera=native_camera();out=m.alloc(48);key=m.alloc(4);p=ASSETS
 files={n:(p/n).read_bytes() for n in ('aegis.vxl','aegis.hva','voxels.vpl')}
 rows=[]
 for facing in (0,8,16,24):
  u.mem_write(m.actor+0x388,dwords(facing*2048,facing*2048));u.mem_write(m.actor+0x398,dwords(0));u.mem_write(m.actor+0x328,struct.pack('<2f',0,0));u.mem_write(key,dwords(0))
  before={k:sha(bytes(u.mem_read(p,0x3F4))) for k,p in m.rngs.items()}
  m.invoke(0x69F670,0,(m.loco+4,out,key));ship=bytes(u.mem_read(out,48));matrix=native_multiply(camera,ship)
  r=render_native(files['aegis.vxl'],files['aegis.hva'],files['voxels.vpl'],matrix)
  cold,cached=dirty(r['rect'],'cold'),dirty(r['rect'],'cached');assert cold==cached
  frame=dict(sinking=1,camera_world_y=1000,view_width=640,draw_y=200,raster_y=cold[1],raster_height=cold[3],caller_rect=[0,0,640,480])
  cu=clip.fixture();first=clip.step(cu,frame,'unit');second=clip.step(cu,frame,'unit')
  pixels=bytearray(65536)
  for i,v in r['pixels']:pixels[i]=v
  rows.append(dict(input=dict(facing_step32=facing,raw_facing=facing*2048,sinking=u.mem_read(m.actor+0x3CD,1)[0],rocking_f32_bits=[0,0],ship_slope=u.mem_read(m.loco+0x1C,4).hex(),frame=0),ship_matrix_bits=list(struct.unpack('<12I',ship)),ship_cache_key=m.read32(key),draw_matrix_bits=r['input_matrix_bits'],native_rect=r['rect'],cold_dirty=cold,cached_dirty=cached,first_clip=first,second_clip=second,pixels_sha256=sha(pixels),nonzero_pixels=len(r['pixels']),write_counts=r['write_counts'],boxes=r['boxes'],raster_params=r['raster_params'],rng_hash_before=before,rng_hash_after={k:sha(bytes(u.mem_read(p,0x3F4))) for k,p in m.rngs.items()}))
 return dict(schema=1,reader=reader,routes=routes,files={n:dict(bytes=len(raw),sha256=sha(raw)) for n,raw in files.items()},camera_bits=list(struct.unpack('<12I',camera)),cases=rows)


def metadata():
 return provenance(scope=__doc__,assumptions=['Original UnitType7470D0 constructor; supplied lexical INI indexes over physical RULESMD, MPBattleMD, map; original Image ReadString528A10 caller arguments (ObjectType5F933B) and original ART Voxel block5F961C..5F963A. Missing LANGRULE has no layer. Active MPBattleMD is byte-identical to original packet MPBattle.INI.', 'Original selected-type dispatch73D30E..73D359 reaches actual Unit+55473B470 when native Voxel and supplied resident VXL pointer presence are true; a missing-pointer control bypasses. Preceding disguise/type selection and actual asset pointer installation into this UnitType are excluded.', 'Actual original Cell487A10→Unit damage produces sinking on retained ordinary AEGIS; full ShipDrawMatrix69F670 runs for explicit stable facing0/8/16/24, constructor flat slope, no rocking and completed timer. Native startup camera and native matrix multiply compose output.', 'Existing repository render_native executes full original VXL/HVA/VPL loads, HVA scaling, box transformation, native rectangle production, sorting and raster. Original code consumes all physical file bytes. Source crop offsets and destination-relative bounds stay separate. FPCW0E7F supplied; ambient original process setting not inferred from this fixture.', 'Native cold and cached dirty-rectangle blocks7068EE..7069F6 and70755D..707678 execute with empty accumulator and original raster-derived destination bounds placed at128,128 cached-surface anchor. These outputs feed the unchanged original Unit clip slice twice. No full composited Unit draw dispatch, RGB palette, surface blit or GPU claim.'],substitutions=['Inherited object setup and original receiver boundaries from naval_occupants; INI lexical cache boundaries.', 'File vtable open/read/seek/close expose immutable retail VXL/HVA/VPL bytes; operator new returns distinct mapped storage in repository raster helper. No raster gameplay/math result substituted.', 'Selected-type frame, VXL-pointer presence and retained renderer locals are supplied interior composition inputs.'],entry_points={'art_voxel_reader':0x5F961C,'selected_draw_route':0x73D30E,'unit_body':0x73B470,'ship_matrix':0x69F670,'vxl_load':0x755DB0,'hva_load':0x5BD5C0,'raster':0x754510,'cold_dirty':0x7068EE,'cached_dirty':0x70755D,'unit_clip':0x73BEA4})
def ground_generate(*,drone=False):
 from tools.voxel_oracle.barrel_pitch import native_arm,native_offset_scale,STARTUP_FPCW
 from tools.native_oracle import NATIVE_SHA256
 root=Path(os.environ.get('VERA20K_SHRAPNEL_INPUTS',str(ASSETS)))
 model='GTNK' if drone else 'DEST'
 names=(model+'.VXL',model+'.HVA','VOXELS.VPL')
 if drone:names+=('GTNKTUR.VXL','GTNKTUR.HVA','GTNKBARL.VXL','GTNKBARL.HVA')
 files={name:(root/name).read_bytes() for name in names}
 cases=[dict(name='flat',angle_bits=[0,0],facing=0),
  dict(name='below_double_tilt_gate',angle_bits=[0x3ba3d70a,0],facing=0),
  dict(name='above_double_tilt_gate',angle_bits=[0x3ba3d70b,0],facing=0),
  dict(name='positive_roll',angle_bits=[0x3f490fdb,0],facing=0,raster=True),
  dict(name='negative_roll',angle_bits=[0xbf490fdb,0],facing=0,raster=True),
  dict(name='both_angles_facing5',angle_bits=[0x3e800000,0xbe000000],facing=5<<11),
  dict(name='retained_slope',angle_bits=[0x3f490fdb,0],facing=13<<11,slope=2),
  dict(name='slope_transition',angle_bits=[0xbf490fdb,0],facing=24<<11,slope=2,previous_slope=1,
       frame=6,slope_timer=dict(start=0,duration=12,total=12))]
 if drone:
  for case in cases:
   if case['name']=='positive_roll':case['angle_bits']=[0x3E800000,0]
   if case['name']=='negative_roll':case['angle_bits']=[0xBE800000,0]
   if case['name']=='both_angles_facing5':case['gun']=dict(turret_step=6,barrel_raw=0x3800,turret_offset=50)
 camera=native_camera();scale=native_offset_scale(STARTUP_FPCW);rows=[]
 for case in cases:
  case=dict(dict(frame=0,slope=0,previous_slope=0,slope_timer=dict(start=0,duration=0,total=0)),**case)
  pose=dict(case);timer=case['slope_timer'];pose['loco_words']=[(0x1c,[case['slope'],case['previous_slope'],timer['start'],0,timer['duration'],timer['total']])]
  r=render_native(files[model+'.VXL'],files[model+'.HVA'],files['VOXELS.VPL'],**{'drive_pose' if drone else 'ship_pose':pose})
  kind='drive' if drone else 'ship'
  row=dict(input=case,**{kind:r[kind]},body_draw_bits=r['input_matrix_bits'])
  if case.get('raster'):row['rasters']=dict(body=dict(rect=r['rect'],pixels=r['pixels']))
  # This is an explicit shared gun-component control; retail DEST has no
  # turret/barrel assets. No physical DEST gun raster is claimed or invented.
  if drone or case['name'] in ('positive_roll','negative_roll'):
   gun=case.get('gun',dict(turret_step=4,barrel_raw=0,turret_offset=0)) if drone else dict(turret_step=6,barrel_raw=0x3800,turret_offset=50)
   turret,barrel=native_arm(scale,camera,gun['turret_offset'],case['facing']>>11,
      gun['turret_step'],gun['barrel_raw'],locomotor_matrix=struct.pack('<12I',*r[kind]['matrix_bits']))
   row['gun_component']=dict(input=gun,turret_draw_bits=list(struct.unpack('<12I',turret)),barrel_draw_bits=list(struct.unpack('<12I',barrel)))
   if drone and case.get('raster'):
    for part,base,matrix in [('turret','GTNKTUR',turret),('barrel','GTNKBARL',barrel)]:
     child=render_native(files[base+'.VXL'],files[base+'.HVA'],files['VOXELS.VPL'],matrix)
     row['rasters'][part]=dict(rect=child['rect'],pixels=child['pixels'])
  rows.append(row)
 return dict(schema=1,native_sha256=NATIVE_SHA256,files={name:dict(bytes=len(raw),sha256=sha(raw)) for name,raw in files.items()},
  camera_bits=list(struct.unpack('<12I',camera)),offset_scale_bits=struct.unpack('<Q',scale)[0],cases=rows)


def squid_generate():
 return ground_generate()


def drive_generate():
 return ground_generate(drone=True)


def drive_metadata():
 return provenance(scope='Eight physical GTNK fullDrive4AFF60 matrix controls, full gun-component matrices and two all-part indexed rasters',assumptions=[
  'Existing physical raster VM/loader/postpass owns GTNK/GTNKTUR/GTNKBARL VXL/HVA and VOXELS.VPL. Original4AF4E0 initializes Drive empty Coord; full4AF540 and4AFF60 execute. Actual loaded model header validity and native first-section half-sizes are retained. No supplied model-validity or draw-matrix return.',
  'Frame, raw body facing, two binary32 angles and retained slope/timer words are explicit numeric component inputs. Adjacent f32 values bracket the original double0.005 gate. Opposite0.25 rolls, two axes, stable slope2 and active1-to2 half transition are covered. These are not captured gameplay poses or a whole locomotor history.',
  'Every row calls existing barrel_pitch.native_arm full73BA4C..73BC26 with the original Drive matrix and supplied turret step/pitch/offset. Default stockGTNK offset0 and explicit nonzero50/pitch3800 control remain distinct. Two rows retain native body/turret/barrel indexed rasters from actual physical parts; no RGB/composited-scene or shadow-pixel claim.',
  'Existing native_slope startup, camera, multiply, loader-zero CRT handler and strict uninitialized-read guard are shared unchanged. All historical Squid/Ship controls retain their original bytes.',
 ],substitutions=['Inherited immutable requested-file vtable I/O and successful allocations. No Drive/FLH/tilt/trig/ftol/matrix/raster instruction or result is replaced.'],entry_points=dict(drive_ctor=0x4AF540,drive_empty=0x4AF4E0,drive_matrix=0x4AFF60,half_sizes=0x7160B4,gun_arm=0x73BA4C,raster=0x754510))


def squid_metadata():
 return provenance(scope='Eight physical DEST original69F670 matrices, original loaded-VXL half-size postpass, two physical hull rasters and two explicit shared gun-component controls',assumptions=[
  'Existing raster VM loads actual DEST VXL/HVA/VPL via original755DB0/5BD5C0/753B70. Original7160B4..716123 derives type half-sizes from first native-loaded section. Original6F9E10 tests actual model header byte0 through Unit type+B0; physical model validity and first section dimensions are recorded.',
  'Actual Ship69EC50 constructor and original69EBF0 empty-Coord startup run. Frame0 or6, facing/angles/slope/timer are explicit component inputs. Full69F670 preserves native cached versus dynamic gate, table trig/ftol, retained slope interpolation and native multiply grouping. No full native scenario/unit asset-binding/locomotor process claim.',
  'Only positive/negative roll controls retain physical hull packed rasters. Existing original loader/raster, camera and matrix owners are reused. Retail DEST Turret=no and derived DESTTUR/BARL assets absent; two gun_component rows explicitly supply offset50/turretstep6/pitch3800 to the existing full73BA4C..73BC26 owner using the original Ship matrix. Those are shared numerical controls, not physical DEST gun rendering.',
  'Original69EBF0 initializes cold Ship empty Coord globals. Frame and actor/type/facing links are supplied component inputs, not fabricated gameplay callable results. Sideways angle input is exact binary32; conversion to simulation fixed point is an explicitly separate consumer boundary.',
  'Shared native_slope startup runs original754980 yaw and all original slope/quaternion initializers on this same VM. The21 historical slope matrices remain unchanged. CRT math-error-handler slotB7819C is supplied as its PE loader-zero virtual-tail prior (original setter7C8F96); the strict uninitialized-read guard remains active. No new native shadow execution or shadow-pixel claim.',
 ],substitutions=['Inherited immutable requested-file vtable IO, successful bump allocation and original startup camera transport. No Ship DrawMatrix/IsVoxel/postpass/trig/ftol/multiply/gun-arm/raster return or instruction is replaced.'],entry_points=dict(ship_ctor=0x69ec50,ship_empty_startup=0x69ebf0,half_sizes=0x7160b4,is_voxel=0x6f9e10,ship_matrix=0x69f670,gun_arm=0x73ba4c,raster=0x754510))


if __name__=='__main__':
 import sys
 if '--drone-tilt' in sys.argv:
  finish_vectors(drive_generate,HERE/'drive_draw_drone_tilt.json',provenance=drive_metadata,argv=[a for a in sys.argv[1:] if a!='--drone-tilt'])
 elif '--squid-tilt' in sys.argv:
  finish_vectors(squid_generate,HERE/'naval_draw_squid_tilt.json',provenance=squid_metadata,argv=[a for a in sys.argv[1:] if a!='--squid-tilt'])
 else:finish_vectors(generate,HERE/'naval_draw_bounds.json',provenance=metadata)
