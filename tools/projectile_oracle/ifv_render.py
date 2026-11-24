"""Original DRAGON frame selection and physical shape drawing for IFV flight.
Shared launch is independently rerun; shape buffers/camera remain supplied.
"""
import hashlib,json,math,struct
from pathlib import Path
from tools.native_oracle import finish_vectors,provenance
from tools.projectile_oracle import ifv_launch,guided_step,bridge_render_shape as shape
from tools.projectile_oracle.bridge_render_inputs import assets_root
from tools.projectile_oracle.bridge_render_inputs_palette import PaletteReader,initialize
from tools.spatial_oracle.building_body_rules import dwords

def generate():
 flight=ifv_launch.generate()
 m,b,cells,initial=guided_step.create();u=m.u;t=m.read32(b+0xac);native_type=m.bullet_state(t)
 raw=(assets_root()/'dragon.shp').read_bytes();assert native_type['frame_count']==32
 pm=initialize(PaletteReader({}));palette=(bytes(pm.u.mem_read(0x24000000,pm.cursor-0x24000000)),[(p,bytes(pm.u.mem_read(p,n))) for p,n in ((0x87f6c0,8),(0x8a0dd0,28))])
 frames=[];draws=[]
 def capture(name,position,velocity,binary_frame,flags,*,kind='flight',extra=None):
  u.mem_write(b+0x9c,dwords(*position));u.mem_write(b+0xe8,struct.pack('<3Q',*[int(x,16) for x in velocity['bits']]));u.mem_write(0xa8ed84,dwords(binary_frame));frame=m.invoke(0x468000,b)
  spec=dict(name=name,xyz=position,mapped_cell=[position[0]//256,position[1]//256],velocity=velocity['value'],level=6,flags=flags,on_bridge=0,depth_baseline=32768,old_z=65535,body_screen=[32,24]);spec.update(extra or {})
  result=shape.execute(spec,native_type,raw,palette)
  assert all(x['frame']==frame for x in result['draws']),(name,frame,result['draws'])
  frames.append(dict(name=name,kind=kind,binary_frame=binary_frame,position=position,velocity=velocity,native_frame=frame,draw_index=len(draws)))
  draws.append(result)
 for i,case in enumerate(flight['cases']):
  supplied=case['supplied'];band={tuple(x) for x in supplied.get('bridge_band',[])};live=set(band)
  if supplied.get('live_bridge_cell'):live.add(tuple(supplied['live_bridge_cell']))
  states=[dict(frame=0,**case['launch'])]+case['frames']
  for state in states:
   f=state['frame']
   for change in supplied.get('between_frame_flag_changes',[]):
    if f==change['frame']:
     if change['live']:live.update(band)
     else:live.difference_update(band)
   position=state['position'];flags=256 if (position[0]//256,position[1]//256) in live else 0
   capture(f'case{i}_frame{f}',position,state['velocity'],f,flags)
 directions=[]
 for i in range(32):
  # Supplied direction-center inputs; selected frames and all colors are native.
  angle=math.tau*i/32;values=[10*math.cos(angle),10*math.sin(angle),0.]
  velocity=dict(value=values,bits=[f'{v:016x}' for v in struct.unpack('<3Q',struct.pack('<3d',*values))])
  capture(f'direction{i}',[2688,5248,800],velocity,1,0,kind='direction')
  directions.append(frames[-1]['native_frame'])
 assert len(set(directions))==32,directions
 v=dict(value=[10.,0.,0.],bits=[f'{v:016x}' for v in struct.unpack('<3Q',struct.pack('<3d',10.,0.,0.))])
 for name,extra in [('corner',dict(body_screen=[0,0])),('bottom',dict(body_screen=[63,95])),('outside',dict(body_screen=[65,16])),('dirty',dict(dirty=[9,11,32,40]))]:capture(name,[2688,5248,1041],v,1,256,kind='clip',extra=extra)
 return dict(flight_source_sha256=hashlib.sha256(json.dumps(flight,sort_keys=True,separators=(',',':')).encode()).hexdigest(),type_inputs=initial,selected_native_type=native_type,shp_sha256=hashlib.sha256(raw).hexdigest(),palette_loads=pm.asset_loaded,surface=dict(width=shape.WIDTH,height=shape.HEIGHT,bytes_per_pixel=2,circular_depth_baseline=32768,a_value=127),frames=frames,draws=draws)

def metadata():
 return provenance(scope='Original DRAGON frame getter and full physical shape drawing for125 independently rerun IFV flight states,5launch states,32direction centers and4clipping controls',assumptions=[
  'ifv_launch.generate independently reexecutes original physical selected launch/flight; its explicit source/world admission, flatmap/structural flags and wholeProcess boundaries remain. Retained poststep XYZ/velocity feed frame/draw without Rust expectations. Impact endpoint is pre-retirement, not a Display scheduling claim.',
  'Original Bullet constructor and native physical AAHeatSeeker2 readers provide image32frames24x16, inverseRotatesfalse, Shadowfalse. Original468000 uses4CAE30 atan2 and7C5F00 conversion. Direction centers are suppliedbinary64 inputs, not native movement-producer outputs.',
  'Full original ObjectRender5F4B10/BulletDraw468090/shape4AED70/clip/rowwalker/leaves execute using unchanged physicalDRAGON bytes and independently initialized native palette Convert tables. Prepared64x96RGB565/circularA/Z buffers, backgroundFFFF,depth32768/oldFFFF,A127 and explicit bodypoint/camera remain fixture boundaries.',
  'Native game x87PC53/truncate0E7F. No host atan2/fitted pixels, native Display traversal, GPU or whole-scene parity claim. Shared bridge_render_shape covers physical120MM geometry/depth; this addition covers selectedDRAGON rotation and image pixels.',
 ],substitutions=['Inherited native input/launch/palette transport boundaries. No frame/draw/clip/shape selector or pixel instruction replaced.'],entry_points={'get_anim_frame':0x468000,'atan2':0x4cae30,'object_render':0x5f4b10,'bullet_draw':0x468090,'shape':0x4aed70,'body_leaf':0x494b60})

if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
