"""Bounded original selection, repeated fatal damage and raw-load sound tail.

Full scenario/COM stream loading is excluded. Saved-object bytes and original
FootLoad sound-reset tail are composed as explicit interior boundaries.
"""
from tools.spatial_oracle.naval_lifetime_audio import *

def selection_case(sinking):
 m=Native(next(c for c in inputs() if c['name']=='west_water_head_road_dz0'));u=m.u
 if sinking:m.run()
 u.mem_write(0xA83D4C,dwords(m.house));u.mem_write(m.house+0x1EC,b'\1');u.mem_write(m.actor+0x41B,b'\1')
 u.mem_write(0xA8ECB8,dwords(0,m.alloc(64),16,1,0,10))
 before={k:bytes(u.mem_read(p,0x3F4)).hex() for k,p in m.rngs.items()}
 rows=[]
 for name,fn in [('foot_can_select',0x4DFA50),('dynamic_can_select',0x6FC030),('local_selectable',0x6F32D0),('object_select',0x5F4520)]:
  rows.append(dict(name=name,pc=hex(fn),result=m.invoke(fn,m.actor)&255))
 return dict(input=dict(sinking=sinking,local_owner=True,discovered=True,empty_selection=True),calls=rows,selected=u.mem_read(m.actor+0x83,1)[0],selection_count=m.read32(0xA8ECC8),selection_is_self=m.read32(m.read32(0xA8ECBC))==m.actor,rng_before=before,rng_after={k:bytes(u.mem_read(p,0x3F4)).hex() for k,p in m.rngs.items()})

def repeat_fatal():
 m=Native(next(c for c in inputs() if c['name']=='west_water_head_road_dz0'));m.run();u=m.u;m.calls=[];m.writes=[]
 p=m.alloc(4);u.mem_write(p,dwords(1));before={k:bytes(u.mem_read(p,0x3F4)).hex() for k,p in m.rngs.items()}
 result=m.invoke(0x737C90,m.actor,(p,0,m.warhead,0,1,1,0))
 return dict(input=dict(initial_health=1,initial_sinking=1,c4_damage=1,distance=0,attacker=None,ignore_defenses=True,arg6=True,house=None),result=result,health=m.read32(m.actor+0x6C),alive=u.mem_read(m.actor+0x90,1)[0],sinking=u.mem_read(m.actor+0x3CD,1)[0],losses=m.read32(m.house+0x5434),calls=m.calls,writes=m.writes,rng_before=before,rng_after={k:bytes(u.mem_read(p,0x3F4)).hex() for k,p in m.rngs.items()})

def raw_load_sound_reset(m,raw):
 """Existing Unit raw-load/Foot audio-reset boundary shared by sound controls.

 Raw bytes and IStream IO are declared inputs. Native410380, FootLoad's reset
 tail, the no-init Foot ctor and Unit vtable reconstruction perform all writes.
 This deliberately excludes dynamic-vector, COM and pointer-swizzle loading.
 """
 u=m.u
 m.actor=m.alloc(0x1000);u.mem_write(m.actor,raw)
 payload=dwords(0x12345678)+raw;pos=0;reads=[]
 stream,table,read_entry=m.alloc(16),m.alloc(64),m.alloc(16);u.mem_write(stream,dwords(table));u.mem_write(table+0xC,dwords(read_entry))
 def read_stream(u,pc,n,d):
  nonlocal pos
  if pc!=read_entry:return
  receiver,dest,size,actual=struct.unpack('<4I',u.mem_read(u.reg_read(UC_X86_REG_ESP)+4,16));assert receiver==stream
  chunk=payload[pos:pos+size];assert len(chunk)==size,(pos,size,len(payload));u.mem_write(dest,chunk);pos+=size
  if actual:u.mem_write(actual,dwords(size))
  reads.append(dict(bytes=size,destination='actor' if dest==m.actor else 'saved-this token'));m.ret(0,16)
 h=u.hook_add(UC_HOOK_CODE,read_stream)
 u.mem_write(SP,dwords(RET_MAGIC,m.actor,stream));u.reg_write(UC_X86_REG_ESP,SP)
 run_checked(u,0x410380,RET_MAGIC,count=200000)
 u.hook_del(h)
 after_raw=list(bytes(u.mem_read(m.actor+0x3CD,2)))
 # Original FootLoad tail: reset audio handle/active/move countdown. Dynamic
 # vectors, COM loco load and swizzle passes are excluded composition boundary.
 m.block(0x4DB60D,0x4DB624,{UC_X86_REG_ESI:m.actor,UC_X86_REG_EBX:0})
 after_tail=dict(flags=list(bytes(u.mem_read(m.actor+0x3CD,2))),handle_hex=bytes(u.mem_read(m.actor+0x544,16)).hex(),move_sound=u.mem_read(m.actor+0x53C,1)[0],move_countdown=m.read32(m.actor+0x540))
 m.invoke(0x4D3540,m.actor,(0,))
 m.block(0x744521,0x74453B,{UC_X86_REG_ESI:m.actor,UC_X86_REG_EBX:m.actor+4})
 after_noinit=list(bytes(u.mem_read(m.actor+0x3CD,2)))
 return dict(reads=reads,after_raw=after_raw,after_tail=after_tail,after_noinit=after_noinit)

def load_case(seen):
 m=Audio();u=m.u;m.phase='setup'
 u.mem_write(m.actor+0x3CD,bytes([1,seen]));u.mem_write(m.actor+0x53C,b'\1');u.mem_write(m.actor+0x540,dwords(3));u.mem_write(m.actor+0x544,b'\xA5'*16)
 u.mem_write(m.actor+0x3CA,struct.pack('<h',1234))
 size=m.invoke(m.read32(m.read32(m.actor)+0x30),m.actor);assert size==0x8E8
 raw=bytes(u.mem_read(m.actor,0x800))+bytes(size-0x800)
 loaded=raw_load_sound_reset(m,raw)
 m.phase='measure';m.audio_events=[];m.writes=[];m.calls=[]
 before={k:bytes(u.mem_read(p,0x3F4)).hex() for k,p in m.rngs.items()}
 m.block(0x4DABC7,0x4DACDD,{UC_X86_REG_ESI:m.actor,UC_X86_REG_EBX:0,UC_X86_REG_EDI:0xFFFFFFFF})
 return dict(input=dict(saved_sinking=1,saved_seen=seen,saved_sound_handle='a5'*16,saved_move_sound=1,saved_move_countdown=3),**loaded,waterline_after_noinit=struct.unpack('<h',u.mem_read(m.actor+0x3CA,2))[0],after_edge_seen=u.mem_read(m.actor+0x3CE,1)[0],events=m.audio_events,rng_before=before,rng_after={k:bytes(u.mem_read(p,0x3F4)).hex() for k,p in m.rngs.items()})

def generate():return dict(schema=1,selection=[selection_case(False),selection_case(True)],repeat_fatal=repeat_fatal(),load=[load_case(0),load_case(1)])
def metadata():return provenance(scope=__doc__,assumptions=['Original selection gates and ObjectSelect execute on an empty capacity16 selection vector, supplied local human owner/discovery and native type Selectable constructor default. Selection voice wrapper6FBFA0, screen picking and later movement command handling excluded.', 'Repeated C4 uses actual original Unit receiver after initial actual sinking. It is direct receiver evidence, not proof that a particular area effect will still acquire an unmarked hull.', 'Original Unit GetSize returns0x8E8; imported object gets a separate0x1000 buffer, first0x800 bytes from the bounded source actor and zero extension. Original AbstractLoad410380 copies these supplied Unit-size object bytes through a supplied exact IStreamRead callback. Original FootLoad audio-reset tail4DB60D..4DB624 then no-initFoot ctor4D3540 and original Unit vtable reconstruction744521..74453B execute. Intervening vectors, COM and pointer-swizzling load bodies excluded. Fields3CD/3CE survive these measured boundaries; already-seen1 produces no replay from actual sound edge.'],substitutions=['Inherited native setup and audio sound callback boundary from naval_lifetime_audio.', 'IStreamRead supplies retained raw object bytes; original native loader controls read lengths and writes.'],entry_points={'selection':0x5F4520,'repeat_damage':0x737C90,'raw_load':0x410380,'foot_load_sound_reset':0x4DB60D,'foot_noinit':0x4D3540,'unit_noinit_vtables':0x744521})
if __name__=='__main__':finish_vectors(generate,HERE/'naval_lifetime_controls.json',provenance=metadata)
