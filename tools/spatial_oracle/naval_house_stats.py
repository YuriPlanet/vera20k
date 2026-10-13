"""Original House full Save/Load statistics retention on bounded empty services."""
from naval_occupants import *
from tools import native_oracle as native

HOUSE_VTABLE=0x7EA8A0
FIELDS={'units_lost':0x5434,'buildings_lost':0x5488,'score':0x54E8}
BUILT=[0x55A0,0x55B4,0x55C8,0x55DC]

def snapshot(m,p,*,with_name=False):
 u=m.u
 result=dict(**{k:signed(m.read32(p+off)) for k,off in FIELDS.items()},
  units_killed=list(struct.unpack('<20i',u.mem_read(p+0x53E4,80))),
  buildings_killed=list(struct.unpack('<20i',u.mem_read(p+0x5438,80))),
  built=[dict(total=signed(m.read32(p+off+16)),capacity=m.read32(p+off+8),items=list(struct.unpack('<'+'i'*m.read32(p+off+8),u.mem_read(m.read32(p+off+4),m.read32(p+off+8)*4)))) for off in BUILT])
 if with_name:result['display_name_utf16_hex']=bytes(u.mem_read(p+0x1602A,42)).hex()
 return result

def query(kind,display_name=None):
 m=Native(next(c for c in inputs() if c['name']=='west_water_head_road_dz0'));u=m.u
 names=display_name is not None
 if names:
  guard=[(native.IMAGE_BASE+rva,bytes(u.mem_read(native.IMAGE_BASE+rva,size)))
         for rva,_raw,size,_virtual,flags in native._sections(native.image_bytes())
         if flags&0x20000000 and size]
  source=m.alloc(256);u.mem_write(source,(display_name+'\0').encode('utf-16-le'))
  u.mem_write(m.house+0x1602A,b'\xCD'*42)
  # Ordinary scenario/local-player caller: original bounded UTF16 copy and
  # final terminator. The producer of the supplied input string is excluded.
  m.block(0x688092,0x6880AD,{UC_X86_REG_EBP:m.house,UC_X86_REG_EBX:source})
  writer_name=bytes(u.mem_read(m.house+0x1602A,42)).hex()
 m.phase='setup';m.invoke(0x4F5190,m.house,(0,));m.invoke(0x6CF180,0xB0C110)
 assert m.read32(m.house)==HOUSE_VTABLE
 if kind=='actual_sinking':m.phase='measure';initial=m.run();m.phase='setup'
 else:
  initial=None
  for k,off in FIELDS.items():u.mem_write(m.house+off,dwords({'units_lost':123,'buildings_lost':456,'score':-789}[k]))
  u.mem_write(m.house+0x53E4,dwords(*range(20)))
  u.mem_write(m.house+0x5438,dwords(*range(20,40)))
  for i,off in enumerate(BUILT):
   p=m.alloc(12);u.mem_write(p,dwords(i+1,i+2,i+3));u.mem_write(m.house+off+4,dwords(p,3));u.mem_write(m.house+off+16,dwords(3*i+6))
 before=snapshot(m,m.house,with_name=names);rng_before={k:bytes(u.mem_read(p,0x3F4)).hex() for k,p in m.rngs.items()}
 payload=bytearray();position=0;io=[];events=[]
 stream,table,read_entry,write_entry=m.alloc(16),m.alloc(64),m.alloc(16),m.alloc(16)
 u.mem_write(stream,dwords(table));u.mem_write(table+12,dwords(read_entry,write_entry))
 def stream_hook(u,pc,n,d):
  nonlocal position
  if pc in (read_entry,write_entry):
   receiver,p,size,actual=struct.unpack('<4I',u.mem_read(u.reg_read(UC_X86_REG_ESP)+4,16));assert receiver==stream
   if pc==write_entry:payload.extend(u.mem_read(p,size));mode='write'
   else:
    chunk=payload[position:position+size];assert len(chunk)==size,(position,size,len(payload));u.mem_write(p,bytes(chunk));position+=size;mode='read'
   if actual:u.mem_write(actual,dwords(size))
   io.append(dict(kind=mode,bytes=size,source_return=hex(m.read32(u.reg_read(UC_X86_REG_ESP)))));m.ret(0,16)
  elif pc in (0x504080,0x410320,0x503040,0x410380,0x4F5190,0x49FB70,0x49FBE0,0x6CF240,0x6CF2C0,0x6CF230,0x6CF350):events.append(hex(pc))
 hook=u.hook_add(UC_HOOK_CODE,stream_hook)
 save_result=m.invoke(0x504080,0,(m.house,stream,0));assert signed(save_result)>=0
 size=m.invoke(0x504730,m.house);assert size==0x160B8
 saved=bytes(payload);source=m.house;target=m.alloc(size)
 m.invoke(0x4F5190,target,(0,));u.mem_write(target+0x1C,dwords(0x11223344))
 load_result=m.invoke(0x503040,0,(target,stream));assert signed(load_result)>=0
 assert position==len(payload),(position,len(payload))
 after=snapshot(m,target,with_name=names)
 result=dict(kind=kind,initial_receiver=initial,before=before,after=after,native_size=size,payload_bytes=len(saved),payload_sha256=sha(saved),io=io,events=events,save_result=signed(save_result),load_result=signed(load_result),retained_live_1c=hex(m.read32(target+0x1C)),rng_before=rng_before,rng_after={k:bytes(u.mem_read(p,0x3F4)).hex() for k,p in m.rngs.items()})
 # Retained external HouseType has the same address in this bounded restart;
 # execute the actual registration and conversion rather than patching +34.
 m.invoke(0x6CF2C0,0,(0xB0C110,m.house_type,m.house_type))
 pending_before=dict(requests=m.read32(0xB0C110+0x14),mappings=m.read32(0xB0C110+0x2C))
 m.invoke(0x6CF230,0,(0xB0C110,));u.hook_del(hook)
 result['swizzle']=dict(before=pending_before,house_type_restored=m.read32(target+0x34)==m.house_type,requests_after=m.read32(0xB0C110+0x14),mappings_after=m.read32(0xB0C110+0x2C))
 assert result['swizzle']['house_type_restored']
 if kind=='actual_sinking':
  m.house=target;u.mem_write(m.actor+0x21C,dwords(target));m.phase='measure';m.calls=[]
  # Direct original terminal RecordKill (UnitAI7364FB), before UnInit.
  m.invoke(0x744720,m.actor,(0,));result['terminal_record_loss']=m.read32(target+0x5434);result['terminal_calls']=m.calls
 else:
  probe=m.alloc(size);u.mem_write(probe,bytes(u.mem_read(target,size)))
  m.invoke(0x4F5190,probe,(0,));result['after_noinit_only']=snapshot(m,probe,with_name=names)
  m.block(0x4F5A2A,0x4F5B39,{UC_X86_REG_EBP:probe,UC_X86_REG_EBX:0,UC_X86_REG_ESI:0xFFFFFFFF})
  m.block(0x4F629A,0x4F62B8,{UC_X86_REG_EBP:probe,UC_X86_REG_EBX:0,UC_X86_REG_ESI:0})
  result['after_normal_constructor_statistics_blocks']=snapshot(m,probe,with_name=names)
 result['rng_after_continuation']={k:bytes(u.mem_read(p,0x3F4)).hex() for k,p in m.rngs.items()}
 assert rng_before==result['rng_after']==result['rng_after_continuation']
 assert before==after,(before,after)
 if names:
  assert all(bytes(u.mem_read(at,len(data)))==data for at,data in guard)
  result.update(input_text=display_name,writer_name_utf16_hex=writer_name,original_executable_bytes_unchanged=True)
 return result

def generate():return dict(schema=1,cases=[query('actual_sinking'),query('mixed_counters')],name_cases=[query('mixed_counters',name) for name in ('Halvor','Ångström Игрок','ABCDEFGHIJKLMNOPQRSTUV')])
def metadata():return dict(provenance(scope=__doc__,assumptions=['Actual original repair-neighbor sinking produces UnitsLost1 in the first row. Full House504080Save,410320AbstractSave,503040Load,410380AbstractLoad and no-init4F5190 execute; empty object/vector/COM services are supplied. Mixed inline statistics and four valid capacity3 Counter arrays are supplied controls, not native production of those values.','Original Swizzle constructor/registration/conversion run. The retained external HouseType has an explicit identity mapping, and the actor owner link is rebound to the restored House; complete scenario/object reload is excluded. A direct original Unit RecordKill after restore proves loss1→2, not the full restored AI cadence.','A copied mixed House executes no-init4F5190 separately, demonstrating inline retention and reset built Counter owners; full House Load restores those owners from appended Counter streams. Normal constructor statistics blocks4F5A2A..4F5B39 and4F629A..4F62B8 execute with explicit EBP=this,EBX0,ESI-1/0 caller registers, proving zero initialization only for those blocks.','Name cases execute original ordinary local-player writer688092..6880AD with supplied UTF16 input, then the same full House storage/noinit/swizzle owner. All42bytes at+1602A are retained; ASCII, non-ASCII and overlong inputs expose zero-fill and the20-code-unit terminator. This is not every House name writer or a complete saved scenario.','Native combined score54E8 is preserved as raw signed bits. This is not a new proof of the Rust split score or aggregate per-house/category representation. All three complete RNG states are unchanged.'],substitutions=['IStream Write/Read retain exactly the bytes and lengths requested by original methods; allocator returns successful bump storage/free is no-op. Original code and House vtable remain unchanged.'],entry_points={'house_save':0x504080,'house_load':0x503040,'noinit':0x4F5190,'abstract_save':0x410320,'abstract_load':0x410380,'get_size':0x504730,'counter_save':0x49FB70,'counter_load':0x49FBE0,'record_kill':0x744720,'swizzle_ctor':0x6CF180,'swizzle_register':0x6CF2C0,'swizzle_convert':0x6CF230,'constructor_losses_score_counters':0x4F5A2A,'constructor_kill_tables':0x4F629A,'local_player_name_writer':0x688092}), command='PYTHONPATH=. python tools/spatial_oracle/naval_house_stats.py --check')
if __name__=='__main__':finish_vectors(generate,HERE/'naval_house_stats.json',provenance=metadata)
