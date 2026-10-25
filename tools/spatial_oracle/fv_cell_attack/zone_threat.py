"""Original585F40 sampling and42C290 positive-threat costs on supplied graphs.

Companion to the existing zone_cost --threat CLI. Reuses its original FV,
physical map, graph constructors and heap owner. House grid and coefficient
controls are supplied prestates, not lifecycle or full-AStar evidence.
"""
import hashlib, struct
from pathlib import Path
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_EBP, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESI
from tools.native_oracle import provenance
from . import zone_cost, pursuit
from tools.spatial_oracle.building_body_rules import dwords


def generate():
 q=pursuit.Continuation(dict(name='zone_threat_setup',stage='healthy'))
 try:
  q.plane();q.construct(True);q.pathfinder()
  producer_writes=[];seed=None
  def producer(u,pc,size,data):
   nonlocal seed
   sp=u.reg_read(UC_X86_REG_ESP)
   signed=lambda n:struct.unpack('<h',struct.pack('<H',n&65535))[0]
   if pc==0x582288:seed=[signed(u.reg_read(UC_X86_REG_EBP)),signed(q.m.read32(sp+0x10))]
   if pc in (0x5822AB,0x584A78):
    level=q.m.read32(sp+(0x80 if pc==0x5822AB else 0x1C))
    p=u.reg_read(UC_X86_REG_ECX);base=q.m.read32(0x87F878+level*24)
    producer_writes.append(dict(pc=f'{pc:08x}',level=level,zone=(p-base)//36,
        seed=seed if pc==0x5822AB else[signed(u.reg_read(UC_X86_REG_ESI)),signed(u.reg_read(UC_X86_REG_EBP))],
        index=u.reg_read(UC_X86_REG_EAX if pc==0x5822AB else UC_X86_REG_EDX)))
  q.u.hook_add(UC_HOOK_CODE,producer);q.graphs()
  q.m.invoke(0x585EE0,0) # Original CRT slot813AB8, required four corner offsets.
  corners=list(struct.unpack('<8h',q.u.mem_read(0xABD460,16)))
  samples=list(struct.unpack('<32i',q.u.mem_read(0x82A984,128)))
  producer_indices=[]
  for level in range(3):
   base=q.m.read32(0x87F878+level*24)
   producer_indices.append([q.m.read32(base+i*36+32)for i in range(len(q.built_graphs[level]['records']))])
  point=q.m.alloc(4);q.u.mem_write(point,struct.pack('<hh',87,49))
  before_rng={k:pursuit.proof.state(q.m,v)for k,v in q.resident.rngs.items()}
  q.m.invoke(0x584550,0x87F7E8,(point,))
  assert before_rng=={k:pursuit.proof.state(q.m,v)for k,v in q.resident.rngs.items()}
  q.checkpoint();house=q.m.read32(q.src+0x21C);events=[];reads=[];pending={}
  def observe(u,pc,size,data):
   sp=u.reg_read(UC_X86_REG_ESP)
   if pc in pending:
    for e in pending.pop(pc):e['returned']=struct.unpack('<i',dwords(u.reg_read(UC_X86_REG_EAX)))[0]
   if pc==0x585F40:
    args=list(struct.unpack('<4I',u.mem_read(sp+4,16)));assert args[0]==house
    e=dict(level=args[1],source=args[2],target=args[3],caller=f'{q.m.read32(sp):08x}')
    events.append(e);pending.setdefault(q.m.read32(sp),[]).append(e)
  def read(u,access,address,size,value,data):
   pc=u.reg_read(UC_X86_REG_EIP)
   if 0x585F40<=pc<=0x5862B2 and house+0x57E4<=address<house+0x57E4+16900*4:
    reads.append(dict(pc=f'{pc:08x}',index=(address-house-0x57E4)//4,
                      value=struct.unpack('<i',u.mem_read(address,4))[0]))
  q.u.hook_add(UC_HOOK_CODE,observe);q.u.hook_add(UC_HOOK_MEM_READ,read)
  helpers=[];prechecks=[]
  def begin(default=0,overrides=()):
   q.reset();events.clear();reads.clear();pending.clear()
   grid=[default]*16900
   for index,value in overrides:grid[index]=value
   q.u.mem_write(house+0x57E4,dwords(*grid));q.phase='control'
   return dict(default=default,overrides=[list(p)for p in overrides])
  def guards():
   return ({k:pursuit.proof.state(q.m,v)for k,v in q.resident.rngs.items()},
           bytes(q.u.mem_read(q.src,0x1000)),hashlib.sha256(q.u.mem_read(0x401000,0x3E0000)).hexdigest())
  def checked(before):
   assert guards()==before;assert not pending,pending
  directions=[(0,-8),(8,-8),(8,0),(8,8),(0,8),(-8,8),(-8,0),(-8,-8)]
  controls=[('level0_ignores_grid',0,131,132,9),('level1_destination_index',1,131,132,9),
            ('level2_same_block_signed_average',2,132,132,-3),
            ('level2_same_block_add_wrap',2,132,132,0x7FFFFFFF)]
  for direction,(dx,dy)in enumerate(directions):
   controls.append((f'level2_direction{direction}',2,131+10+130*10,
                    131+(40+dx)//4+130*((40+dy)//4),None))
  for name,level,source,target,default in controls:
   grid=begin(default if default is not None else 0,
              [(i,(i*13)%97-20)for i in range(130*8,130*14)]if default is None else [])
   base=q.m.read32(0x87F878+level*24)
   q.u.mem_write(base+36+32,dwords(source));q.u.mem_write(base+72+32,dwords(target))
   before=guards();answer=q.m.invoke(0x585F40,0x87F7E8,(house,level,1,2))
   events[-1]['returned']=struct.unpack('<i',dwords(answer))[0];pending.clear();checked(before)
   helpers.append(dict(name=name,level=level,source_index=source,target_index=target,
                       grid=grid,returned=struct.unpack('<i',dwords(answer))[0],events=events.copy(),reads=reads.copy()))
  controls=[(level,bits,False)for level in (1,2)for bits in
    ('0000000000000000','3ee4f8b588e368f0','3ee4f8b588e368f2','3fdfffffffffffff','3fe0000000000000','3ff0000000000000')]
  controls += [(level,'3ff0000000000000',True)for level in (1,2)]
  for level,bits,reverse in controls:
    grid=begin(0,[(133,1 if reverse else 9),(134,9 if reverse else 1),(135,0)])
    graphs=[]
    for l in range(3):
     rs=zone_cost.records([0]*4,{1:[[2,1],[3,0]],2:[[4,0]],3:[[4,0]]})
     for i,r in enumerate(rs):r.update(parent=0 if l==2 else i,threat_index=131+i)
     graphs.append(rs)
    if level==1:
     for r in graphs[1]:r['parent']=1
     graphs[2]=[dict(zone_type=7,parent=0,edges=[],threat_index=0),dict(zone_type=0,parent=0,edges=[],threat_index=131)]
    zone_cost.install_graph(q,graphs[0],1,4,level_records=graphs)
    if level==1:
     plane=q.m.read32(0x87F858)
     for x,y in ((87,49),(87,54)):q.u.mem_write(plane+(y*q.side+x)*10+4,struct.pack('<H',1))
    q.u.mem_write(q.src+0x530,struct.pack('<Q',int(bits,16)));q.u.mem_write(q.src+0x5D4,dwords(0))
    points=q.m.alloc(8);q.u.mem_write(points,struct.pack('<4h',87,49,87,54))
    before=guards();answer=q.m.invoke(0x42C290,0x87E8B8,(points,points+4,0,q.src));q.flow_pending.clear();checked(before)
    counts=[q.m.read32(0x87E8B8+0xC74+i*4)for i in range(3)]
    q.flow[-1].update(returned_al=answer&255,paths=[list(struct.unpack('<'+'H'*n,q.u.mem_read(0x87E8B8+0xBC+i*1000,n*2)))for i,n in enumerate(counts)])
    prechecks.append(dict(name=f'level{level}_coefficient_{bits}'+('_reverse_grid'if reverse else''),level=level,coefficient_bits=bits,
                          grid=grid,graphs=graphs,source_id=1,target_id=4,returned_al=answer&255,
                          flow=q.flow,hierarchy=q.hierarchy,events=events.copy(),reads=reads.copy()))
    print('PASS precheck',level,bits,answer&255,len(events),flush=True)
  return dict(schema=1,corners=corners,sample_pairs=samples,
              producer_indices=producer_indices,producer_writes=producer_writes,helpers=helpers,prechecks=prechecks,
              rng_unchanged=True,actor_unchanged=True,text_unchanged=True)
 finally:q.close()


def metadata():
 return provenance(scope=__doc__,entry_points={'precheck':0x42C290,'estimate':0x585F40,'corner_startup':0x585EE0},
  assumptions=['Original constructed FV/House, full hierarchy constructors, native heap and PC53/chop startup come from zone_cost/pursuit. Graph/index/coefficient and padded House grid values are explicit inputs.',
               '585EE0 is original CRT slot813AB8. All8 direction samples, level0/1, signed average/add wrap, coefficient threshold and integer truncation controls execute original instructions.',
               'Query/read order, original return values, heap-pop float stores and all3 RNG/actor/text guards are captured. No full-AStar, House lifecycle or stock route equivalence is claimed.'],
  substitutions=['Existing pursuit OS/allocation seams only; no threat,cost,route,heap,float or RNG return is substituted.'])
