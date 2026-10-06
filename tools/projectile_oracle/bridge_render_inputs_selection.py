"""Original Techno/Weapon constructors, Primary block and full Weapon reader.
The Techno reader prefix/admission is supplied. The Weapon reader executes with
an empty sound registry; its allocated AP warhead retains constructor fields.
"""
import importlib.util,json,struct,argparse
from pathlib import Path
from unicorn.x86_const import *
from tools.native_oracle import run_checked,image_sha256
from tools.spatial_oracle.building_body_rules import SP,RULES,dwords
from tools.projectile_oracle import bridge_render_inputs as mod

def initialize_weapon(m):
 """Fresh selected105mm/Rules owner in a BulletReader-compatible machine.
 Return weapon, actual native allocated/read BulletType, Rules object.
 Sound registry is empty; AP is ctor-only; no combat outcome claim.
 """
 u=m.u;u.mem_write(0x887568,dwords(0x7eb6d4,m.alloc(4096),1024,1,0,10))
 weapon=m.invoke(0x772fa0,m.cstring('105mm'));rules_object=m.alloc(0x2000)
 u.reg_write(UC_X86_REG_ESI,rules_object);run_checked(u,0x6674d6,0x6674e0)
 for filename in ('RULESMD.INI','MPBattleMD.ini','Hills.map'):
  sections,_=mod.lexical((mod.assets_root()/filename).read_bytes(),{'105mm','Cannon','AudioVisual'})
  m.rules_cache(sections);m.invoke(0x772080,weapon,(RULES,))
  bullet=m.read32(weapon+0xa0);m.invoke(0x46bee0,bullet,(RULES,))
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,rules_object);u.reg_write(UC_X86_REG_EDI,RULES)
  run_checked(u,0x66b3c4,0x66b3e4)
 return weapon,bullet,rules_object

def generate():
 m=mod.BulletReader({});u=m.u
 for a in (0x887568,0xa8eb00):u.mem_write(a,dwords(0x7eb6d4,m.alloc(4096),1024,1,0,10))
 techno=m.alloc(0xE00);m.invoke(0x710af0,techno,(m.cstring('MTNK'),))
 ctor=dict(primary=m.read32(techno+0x898),secondary=m.read32(techno+0x8b4));rows=[]
 rules_object=m.alloc(0x2000);u.reg_write(UC_X86_REG_ESI,rules_object)
 run_checked(u,0x6674d6,0x6674e0)
 ctor['gravity']=m.read32(rules_object+0x16b8)
 for filename in ('RULESMD.INI','MPBattleMD.ini','Hills.map'):
  sections,_=mod.lexical((mod.ROOT/filename).read_bytes(),{'MTNK','105mm','Cannon','AudioVisual'})
  m.rules_cache(sections);m.reads=[]
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EBP,techno)
  u.reg_write(UC_X86_REG_EBX,techno+0x24);u.reg_write(UC_X86_REG_ESI,RULES)
  run_checked(u,0x7129ab,0x712a1d,required_addresses=(0x528a10,))
  assert u.reg_read(UC_X86_REG_ESP)==SP
  weapon=m.read32(techno+0x898)
  assert weapon
  admitted=bool(m.invoke(0x772080,weapon,(RULES,))&255)
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,rules_object);u.reg_write(UC_X86_REG_EDI,RULES)
  # Constructor-established retained default, original ReadAudioVisual block.
  run_checked(u,0x66b3c4,0x66b3e4,required_addresses=(0x5276d0,))
  bullet=m.read32(weapon+0xa0)
  rows.append(dict(file=filename,primary=m.string(weapon+0x24),secondary=m.read32(techno+0x8b4),
   projectile=m.string(bullet+0x24),weapon_reader_admitted=admitted,
   weapon_speed=m.read32(weapon+0xa8),lobber=bool(u.mem_read(weapon+0x12e,1)[0]),
   gravity=m.read32(rules_object+0x16b8),native_reads=m.reads))
 return dict(native_sha256=image_sha256(),ctor=ctor,rows=rows)

if __name__=='__main__':
 a=argparse.ArgumentParser();a.add_argument('--check',type=Path);args=a.parse_args();result=generate()
 if args.check:assert result==json.loads(args.check.read_text());print('PASS original MTNK Primary and 105mm Projectile lookup')
 else:print(json.dumps(result,indent=2))
