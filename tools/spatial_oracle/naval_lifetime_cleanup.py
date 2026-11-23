"""Original marked naval damage postlude and terminal lifetime research.

Supplied marked actor and sparse object/map projection, not full Unlimbo.
Resident current-cell Water02.sno bytes are physical stock Shrapnel metadata.
"""
from naval_occupants import *
from tools.spatial_oracle.shrapnel_repair.retail_inputs import Rules as ResidentRules, theater
from tools.spatial_oracle.shrapnel_repair.shrapnel_repair import physical_map
from tools.native_oracle import require

class Cleanup(Native):
 def __init__(self,case):
  self.lifetime_events=[];self.ordered=[]
  super().__init__(case);self.phase='setup';u=self.u
  self.invoke(0x6D1C20,self.alloc(0x2000))
  self.invoke(0x5F5B90,self.actor,(0,self.alloc(0x80)))
  u.mem_map(0x49000000,0x800000)
  count=165*165;u.mem_write(MAP+0x68,dwords(0x49200000,count,0x49300000));u.mem_write(0x49200000,b'\x07\0\0\0'*count)
  r=ResidentRules();t=theater();_,physical=physical_map();self.resident=[]
  u.mem_write(0xA8ED2C,dwords(0x49000000));u.mem_write(0xA8ED38,dwords(t['count']))
  current=case['current'];c=tuple(x//256 for x in current[:2]);p=self.cells[c];rawcell=physical[c]
  tile=rawcell['tile'];path=ASSETS/t['tiles'][tile];raw=path.read_bytes();tmp=bytearray(raw);data=0x49020000;head=0x49010000
  width,height=struct.unpack_from('<II',tmp)
  for sub in range(width*height):
   offset=struct.unpack_from('<I',tmp,16+sub*4)[0]
   if offset:struct.pack_into('<I',tmp,16+sub*4,data+offset)
  u.mem_write(0x49000000+tile*4,dwords(head));u.mem_write(head,dwords(0x7ECC48));u.mem_write(head+0xA4,dwords(data));u.mem_write(head+0x2C8,dwords(-1));u.mem_write(head+0x2D4,dwords(-1));u.mem_write(head+0x2F0,dwords(1));u.mem_write(data,bytes(tmp))
  self.resident.append(dict(coord=list(c),name=path.name,sha256=sha(raw),raw_cell=rawcell))
  for a,v in t['globals'].items():u.mem_write(a,dwords(v))
  u.mem_write(RULES+0x664,bytes(r.u.mem_read(r.rules+0x664,1)))
  u.mem_write(0xA83D84,dwords(0x49410000))
  for index,op in enumerate(r.overlay_ptrs):
   q=0x49420000+index*0x400;u.mem_write(q,bytes(r.u.mem_read(op,0x2C0)));u.mem_write(0x49410000+index*4,dwords(q))
  u.mem_write(p+0x38,dwords(tile));u.mem_write(p+0x11A,bytes([rawcell['subtile'],rawcell['level']]))
  u.mem_write(0xB0F698,dwords(0x7E91EC,self.alloc(128),32,1,0,10))
  self.read_ranges.append(('current_cell',p,0x144));self.current_cell=p
  # Actual static startup and registration owners, supplied empty world.
  self.invoke(0x40CB80,0);self.invoke(0x4A8630,0)
  require(self.invoke(0x55BAA0,0x87F778,(self.actor,0))&255, 'native 0x55BAA0 returned an unexpected result')
  self.invoke(0x4A9720,0,(self.actor,))
  self.counter=self.alloc(64);u.mem_write(self.counter,dwords(1))
  u.mem_write(self.house+0x5564,dwords(0,self.counter,16,1,1))
  u.mem_write(self.current_cell+0x120,b'\x10')
  u.mem_write(self.actor+0x55C,packed(*c))
  for cell in self.cells.values():u.mem_write(cell+0x122,b'\x10')
  if case.get('moving_input'):
   u.mem_write(self.actor+0x8D,b'\1');u.mem_write(self.actor+0x53C,b'\1')
   u.mem_write(self.loco+0x50,struct.pack('<d',0.75));u.mem_write(self.loco+0x34,dwords(30000,15232,208))
  self.read_ranges.extend([('logic_vector',0x87F778,24),('display_vectors',0x8A0360,120),('deferred_vector',0xB0F698,24),('house_type_counter',self.counter,64),('deferred_items',self.read32(0xB0F69C),128)])
  self.phase='measure'
 def hook(self,u,pc,n,d):
  if pc in (0x7C978A,0x6C8C40):
   assert self.phase=='setup';self.ret(0);return
  if self.phase=='measure':
   names={0x4D3780:'mark',0x5687F0:'map_remove',0x47EA90:'remove_content',0x47D2B0:'recalc',0x4DE5D0:'foot_uninit',0x5F65F0:'object_uninit',0x7440B0:'unit_limbo',0x4DB260:'foot_limbo',0x6F6AC0:'techno_limbo',0x5F4D30:'object_limbo',0x55BAE0:'logic_remove',0x4A9770:'display_remove',0x406060:'release_sound'}
   extra={0x737C90:'unit_damage',0x744720:'record_kill',0x7258D0:'pointer_expired',0x4D9720:'detach_all',0x4D5660:'stun',0x5F44A0:'deselect',0x5F6625:'alive_clear',0x5F4E9E:'limbo_set'}
   if pc in names or pc in extra:
    self.ordered.append(dict(kind=names.get(pc,extra.get(pc)),pc=hex(pc),health=signed(self.read32(self.actor+0x6C)),alive=u.mem_read(self.actor+0x90,1)[0],limbo=u.mem_read(self.actor+0x81,1)[0],mark=u.mem_read(self.actor+0x74,1)[0],logic=self.read32(0x87F788),display=self.read32(0x8A03A0)))
   if pc in names:
    sp=u.reg_read(UC_X86_REG_ESP);self.lifetime_events.append(dict(kind=names[pc],pc=hex(pc),this=hex(u.reg_read(UC_X86_REG_ECX)),arg0=self.read32(sp+4),health=signed(self.read32(self.actor+0x6C)),alive=u.mem_read(self.actor+0x90,1)[0],is_on_map=u.mem_read(self.actor+0x74,1)[0],limbo=u.mem_read(self.actor+0x81,1)[0],ground_head=self.read32(self.current_cell+0xE4)))
  super().hook(u,pc,n,d)
 def marked(self):
  out=self.run();out['lifetime_events']=self.lifetime_events;out['ordered']=self.ordered;out['resident']=self.resident;out['current_cell_land']=signed(self.read32(self.current_cell+0xEC));out['object_next']=self.read32(self.actor+0x30);return out

def snapshot(m):
 u=m.u
 return dict(health=signed(m.read32(m.actor+0x6C)),alive=u.mem_read(m.actor+0x90,1)[0],limbo=u.mem_read(m.actor+0x81,1)[0],marked=u.mem_read(m.actor+0x74,1)[0],sinking=u.mem_read(m.actor+0x3CD,1)[0],seen=u.mem_read(m.actor+0x3CE,1)[0],layer=signed(m.read32(m.actor+0x94)),logic_registered=u.mem_read(m.actor+0x98,1)[0],logic_count=m.read32(0x87F788),display_count=m.read32(0x8A03A0),deferred_count=m.read32(0xB0F6A8),deferred_is_self=m.read32(m.read32(0xB0F69C))==m.actor,ground_head=m.read32(m.current_cell+0xE4),object_next=m.read32(m.actor+0x30),land=m.read32(m.current_cell+0xEC),owner_losses=m.read32(m.house+0x5434),owner_type_count=signed(m.read32(m.counter)),moving_raw=u.mem_read(m.actor+0x8D,1)[0],move_sound=u.mem_read(m.actor+0x53C,1)[0],speed=struct.unpack('<d',u.mem_read(m.loco+0x50,8))[0],destination=list(struct.unpack('<3i',u.mem_read(m.loco+0x34,12))),head=list(struct.unpack('<3i',u.mem_read(m.loco+0x40,12))),coord=list(struct.unpack('<3i',u.mem_read(m.actor+0x9C,12))))

def generate():
 rows=[]
 for side in ('west_water_head_road_dz0','east_water_head_road'):
  for moving in (False,True):
   case=dict(next(c for c in inputs() if c['name']==side),is_on_map=True,moving_input=moving)
   m=Cleanup(case);u=m.u;initial=snapshot(m);damage=m.marked();after_damage=snapshot(m)
   u.mem_write(m.actor+0xA4,dwords(208-396));m.calls=[];m.writes=[];m.lifetime_events=[];m.ordered=[]
   before={k:bytes(u.mem_read(p,0x3F4)).hex() for k,p in m.rngs.items()}
   u.reg_write(UC_X86_REG_ESI,m.actor);u.reg_write(UC_X86_REG_ESP,SP)
   endpoint=run_checked(u,0x7364A1,0x736510,count=300000,required_addresses=(0x744720,0x4DE5D0,0x5F65F0,0x4A9770,0x55BAE0,0x5F6625))
   rows.append(dict(input=case,initial=initial,damage=damage,after_damage=after_damage,terminal=dict(supplied_relative_height=-396,endpoint=hex(endpoint),state=snapshot(m),events=m.lifetime_events,ordered=m.ordered,calls=m.calls,writes=m.writes,rng_before=before,rng_after={k:bytes(u.mem_read(p,0x3F4)).hex() for k,p in m.rngs.items()})))
 return dict(schema=1,cases=rows)

def metadata():
 return provenance(scope=__doc__,assumptions=['Frozen native_occupants fixture and its original AEGIS reader outputs remain the admission/damage base. Selected repaired strip land is supplied, not recomputed by the repair controller here. Only source Water current cell uses physical stock TMP and original47D2B0.', 'Original actual40CB80/4A8630 static vector startup and55BAA0/4A9720 register actor in Logic and display layer2. IsOnMap1, cell membership and owner one-unit counter are supplied; full Unlimbo is not executed. Counter capacity16 avoids resize; no native return is replaced.', 'Foot retained cell55C is supplied physical current cell, sight counters16 are supplied to keep release arithmetic in range, Foot retained sight radius stays constructor0. Stock complete sight reveal or full House bookkeeping initialization not claimed.', 'Terminal stage starts at actual UnitAI7364A1 with physical Z set to ground208-396 and supplied interior ESI/stack; original RecordKill and complete concrete Foot/Unit/Techno/Object UnInit/Limbo execute through736510. Earlier whole UnitAI/FootAI and intervening sinking frames excluded. HP remains1 at final removal, owner loss counter goes1 to2, actor Alive becomes0 and deferred queue retains pointer. Final deferred destructor/free drain excluded.', 'Native selected MPBattle inputs are byte-identical to active MPBattleMD.ini; new resident readers use correct active MPBattleMD path. See separate audio layer identity receipt.','Main/Scenario/MapGen full states bracket damage and terminal stages. FPCW0E7F supplied.'],substitutions=['Inherited setup allocation/free/CRT and measured OS Interlocked only. CRT atexit registration and Tactical wall-clock result0 are setup boundaries. No measured gameplay receiver is replaced.'],entry_points={'damage':0x487A10,'mark':0x4D3780,'remove':0x47EA90,'recalc':0x47D2B0,'terminal':0x7364A1,'uninit':0x4DE5D0,'logic_remove':0x55BAE0,'display_remove':0x4A9770})

if __name__=='__main__':finish_vectors(generate,HERE/'naval_lifetime_cleanup.json',provenance=metadata)
