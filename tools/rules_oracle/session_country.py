"""Session Country history on the existing physical Rules oracle owner."""
from pathlib import Path
import json,struct
from tools.native_oracle import configured_gamemd,image_sha256
from tools.rules_oracle.weapon_speed_order import RetailRulesInputs,retail_history
physical=None
fixtures={}
class Inputs:
 def lookup(self,name): return fixtures.get(name.upper(),physical.lookup(name))
def signed(x):return struct.unpack('<i',struct.pack('<I',x))[0]
def snapshot(m):
 result=[]
 for i in range(m.read32(0xA83CA8)):
  p=m.read32(m.read32(0xA83C9C)+i*4)
  result.append(dict(index=i,id=m.string(p+0x24),name=m.string(p+0x64),multiplay=m.u.mem_read(p+0x1A5,1)[0]))
 return result
def ini(m,body):
 name=f'CTRL{len(fixtures)}.INI';fixtures[name]=body.encode('ascii');return m.load_physical_ini(name)[0]
def fixture(root):
 m=retail_history(Inputs());r=m.alloc(0x2000);m.invoke(0x665650,r)
 return m,r,ini(m,root)
def bodies(m,root):
 # Original PrepareSession52E123 invokes each live Country vtable+64.
 index=0
 while index<m.read32(0xA83CA8):
  p=m.read32(m.read32(0xA83C9C)+index*4)
  m.invoke(m.read32(m.read32(p)+0x64),p,(root,));index+=1
def generate():
 global physical
 physical=RetailRulesInputs(configured_gamemd().parent)
 fixtures.clear()
 rows=[]
 root='[Countries]\n0=Alpha\n1=Beta\n[Alpha]\nName=Alias\nMultiplay=yes\n[Orphan]\nName=LaterAlias\nMultiplay=yes\n'
 for token in [None,'','Alpha','Alias','Orphan','<random>','<none>','none','<Player @ A>','<Player @ H>','<player @ a>','012345678901234567890123Long']:
  m,r,rootini=fixture(root)
  phases=[]
  for phase in range(2):
   m.invoke(0x6722F0,r,(rootini,))
   settings=ini(m,'[MultiPlayer]\n'+('' if token is None else 'Side='+token+'\n'))
   rng=m.rng_state()
   result=signed(m.invoke(0x475540,settings,(m.cstring('MultiPlayer'),m.cstring('Side'),0xfffffffd)))
   after=snapshot(m)
   bodies(m,rootini);m.check_code()
   assert rng==m.rng_state()
   phases.append(dict(result=result,allocated=after,refreshed=snapshot(m)))
  rows.append(dict(token=token,phases=phases));m.close()
 m=retail_history(Inputs());r=m.alloc(0x2000);m.invoke(0x665650,r)
 rootini,_=m.load_physical_ini('RULESMD.INI');m.invoke(0x6722F0,r,(rootini,))
 retail=snapshot(m);m.check_code();m.close()
 return dict(native_sha256=image_sha256(),root=root,current=-3,rows=rows,retail_registered=retail)

if __name__=='__main__':
 from tools.native_oracle import finish_vectors,provenance
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=provenance(
  scope='Session Country master/ReadHouseType and subsequent live body history',
  assumptions=['Declared physical root and MultiPlayer sections; original Rules665650, Countries6722F0, whole475540 and live Country virtual+64 reader. Two retained phases per token. Retail root master enumerated through original physical INI loader. Full PrepareSession GUI and destructive reset are not executed.'],
  substitutions=['Existing RetailRulesHistory Windows/file/heap platform transport only; no Country/parser/constructor body replacement.'],
  entry_points={'countries':0x6722F0,'read_house_type':0x475540,'house_ctor':0x5113F0,'house_body':0x511850}))
