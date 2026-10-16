"""Native original input readers for the bounded Anytown navigation composition."""
from pathlib import Path
import os,struct,hashlib,json
HERE=Path(__file__).resolve().parent
from . import next_family_native as identity
from .inputs import ASSETS
from tools.spatial_oracle.shrapnel_repair.map_facts import decode_cells
from tools.sidebar_oracle.stock import mix,mix_hash
from tools.spatial_oracle.shrapnel_repair import retail_inputs as ri
from tools.projectile_oracle.bridge_render_inputs import lexical
from tools.rules_oracle.bridge_landing_inputs import Landing
from tools.native_oracle import finish_vectors,provenance,run_checked
from tools.spatial_oracle.building_body_rules import dwords
from unicorn.x86_const import *
sha=lambda raw:hashlib.sha256(raw).hexdigest()
MAPFILE=ASSETS/'XMP03T4.MAP'

def map_inputs(map_file=None):
 raw=Path(map_file or MAPFILE).read_bytes();sections,cells=decode_cells(raw)
 return raw,sections,cells

def extract_tiles(t, *, theater_archive='isotemp.mix', theater_override='isotemmd.mix', tile_suffix='tem', disjoint_theater_additions=False, theater_primary=None, theater_short=None, theater_extra=None):
 root=Path(os.environ['RA2_DIR']);archive=(root/'ra2.mix').read_bytes();iso=mix(archive)[mix_hash(theater_archive)];members=mix(iso);rows=[];data={}
 for i,name in t['tiles'].items():
  entry=mix_hash(name);raw=members.get(entry)
  if raw is not None:data[i]=raw
  rows.append(dict(tile=i,file=name,entry=f'{entry:08X}',bytes=len(raw) if raw else 0,sha256=sha(raw) if raw else None))
 wanted={mix_hash(name) for name in t['tiles'].values()};yr=(root/'ra2md.mix').read_bytes();outer=mix(yr);checks=[]
 # Init_Theater5349C0 registers the long/short theater archives as well as
 # isometric archives. Wood bridge TMPs reside in TEMPERAT.MIX/SNOW.MIX.
 # Do not infer precedence: fail on queried-name overlap and prove each
 # additional physical winner is unique across the registered archives.
 defaults={'tem':('temperat.mix','tem.mix',()),'sno':('snow.mix','sno.mix',('snowmd.mix',))}
 primary,short,extra=defaults[tile_suffix]
 primary=theater_primary or primary;short=theater_short or short
 extra=extra if theater_extra is None else tuple(theater_extra)
 theater_checks=[];selected=set(members)&wanted
 for name in (*extra,primary,short):
  found=[(source,payload) for source,container in [('ra2.mix',mix(archive)),('ra2md.mix',outer)] if (payload:=container.get(mix_hash(name))) is not None]
  assert len(found)<=1,('ambiguous theater archive container',name)
  if not found:theater_checks.append(dict(archive=name,absent=True));continue
  source,payload=found[0];entries=mix(payload);hits=sorted(set(entries)&wanted)
  assert not (set(hits)&selected),('ambiguous registered primary TMP',name)
  for row in rows:
   entry=int(row['entry'],16)
   if entry in hits:
    blob=entries[entry];data[row['tile']]=blob
    row.update(bytes=len(blob),sha256=sha(blob),source=source+'/'+name)
  selected.update(hits)
  theater_checks.append(dict(archive=source+'/'+name,sha256=sha(payload),unique_primary_hits=hits))
 for name in (theater_override,'isogenmd.mix','genermd.mix','localmd.mix','cachemd.mix'):
  raw=outer[mix_hash(name)];entries=mix(raw);hits=sorted(set(entries)&wanted)
  if disjoint_theater_additions and name==theater_override:
   # The stock SNOW additions have no base-name overlap. This admits unique
   # physical files without claiming to execute the native archive resolver.
   assert not (set(hits)&selected),('ambiguous TMP winner',name)
   for row in rows:
    entry=int(row['entry'],16)
    if entry in hits:
     blob=entries[entry];data[row['tile']]=blob
     row.update(bytes=len(blob),sha256=sha(blob),source='ra2md.mix/'+name)
  else:assert not hits,(name,hits)
  checks.append(dict(archive='ra2md.mix/'+name,sha256=sha(raw),**{f'primary_{tile_suffix}_name_hits':hits}))
 for name in ('expandmd01.mix','langmd.mix','language.mix'):
  raw=(root/name).read_bytes();hits=sorted(set(mix(raw))&wanted);assert not hits,(name,hits);checks.append(dict(archive=name,sha256=sha(raw),**{f'primary_{tile_suffix}_name_hits':hits}))
 loose={p.name.upper() for p in root.iterdir() if p.is_file()}&{n.upper() for n in t['tiles'].values()};assert not loose,loose
 return data,dict(archive='ra2.mix/'+theater_archive,outer_sha256=sha(archive),inner_sha256=sha(iso),members=rows,registered_theater_checks=theater_checks,selected_override_checks=checks,**{f'loose_primary_{tile_suffix}_hits':sorted(loose)})


def bind_primary_tiles(m, theater, selected_tiles, *, preserve_abstract_registry=False):
    """Shared physical TMP cache transport with original IsoTileType constructors.

    The caller supplies an initialized primary array/header and sparse loop
    indexes. Original TMP readers/Recalc remain native; archive enumeration and
    in-memory TMP offset relocation are the existing fixture boundary.
    """
    u = m.u
    physical, receipt = extract_tiles(theater)
    table, count = m.read32(0xA8ED2C), m.read32(0xA8ED38)
    retained_header = bytes(u.mem_read(0xA8ED28, 24))
    u.mem_write(0xA8ED28, dwords(0x7EB6D4, table, count, 1, count, 10))
    # Original constructor also retains every primary/secondary tile type.
    # The existing Reader remains the sole allocator in this composed VM.
    if not preserve_abstract_registry:
        u.mem_write(0xB0F670, dwords(0x7EB6D4, m.alloc(4096 * 4), 4096, 1, 0, 10))
    rows = []
    for tile in sorted(set(selected_tiles)):
        if tile >= count or m.read32(table + tile * 4):
            continue
        raw = physical.get(tile)
        assert raw is not None, (tile, theater['tiles'][tile])
        head, data = m.alloc(0x400), m.alloc(len(raw))
        name = theater['tiles'][tile]
        # Supply the sparse original primary-array loop index as caller state;
        # the constructor itself installs its vtable/defaults and this entry.
        u.mem_write(0xA8ED38, dwords(tile))
        args = (tile, -65, 0, m.cstring(name[:-4]), 0)
        returned = m.invoke(0x5447C0, head, args)
        assert returned == head and m.read32(table + tile * 4) == head
        tmp = bytearray(raw)
        width, height = struct.unpack_from('<II', tmp)
        for sub in range(width * height):
            offset = struct.unpack_from('<I', tmp, 16 + sub * 4)[0]
            if offset:
                struct.pack_into('<I', tmp, 16 + sub * 4, data + offset)
        u.mem_write(data, bytes(tmp))
        u.mem_write(head + 0xA4, dwords(data))
        u.mem_write(head + 0x2E4, dwords(width & 255, height & 255))
        rows.append(dict(tile=tile, file=name, raw_sha256=hashlib.sha256(raw).hexdigest(),
                         raw_bytes=len(raw), original_ctor='0x005447C0', ctor_args=list(args),
                         head=hex(head), vtable=hex(m.read32(head)), data=hex(data),
                         original_defaults_hex=bytes(u.mem_read(head + 0x2C0, 0x3C)).hex(),
                         relocated_sha256=hashlib.sha256(bytes(tmp)).hexdigest()))
    u.mem_write(0xA8ED28, retained_header)
    assert m.read32(0xA8ED38) == count
    return dict(archive_inputs=receipt, added_rows=rows,
        boundary='Physical primary TMP cache supplied with original constructor heads and native default animation/shadow fields. The original archive loader, all tile INI properties and rendering variants are excluded. Existing crop Cell inputs are unchanged; later original Recalc owns land/slope/zone updates.')

class Inputs(ri.Rules):
 def __init__(self,t,*,map_file=None,theater_file='TEMPERATMD.INI',damage_overlays=range(205,233),tiberium_timer_readers=False):
  self.tiberium_timer_readers=tiberium_timer_readers;self.tiberium_timer_layers=[]
  self.map_file=Path(map_file or MAPFILE);self.theater_file=theater_file;self.damage_overlays=tuple(damage_overlays)
  super().__init__();self.receipts=[];raw,sections,cells=map_inputs(self.map_file);self.physical=cells;self.map_sections=sections
  self.invoke(0x71D580,0)
  self.u.mem_write(0xA8B230,ri.dwords(self.alloc(0x1300)))
  self.u.mem_write(0x8871E0,ri.dwords(self.rules))
  self.block(0x666DF8,0x666E02,{UC_X86_REG_ESI:self.rules})
  names=sorted({v for k,v,line in sections['Terrain']});self.terrain_ptrs={}
  self.u.mem_write(0xA8E318,ri.dwords(0x7EB6D4,self.alloc(1024*4),1024,1,0,10))
  for name in names:
   p=self.alloc(0x400);self.invoke(0x71DA80,p,(self.cstring(name),));self.terrain_ptrs[name]=p
  art,_=lexical((ri.ASSETS/'ARTMD.INI').read_bytes(),set(names));self.make_ini(art);art_ini=bytes(self.u.mem_read(ri.INI,0x40))
  self.building_ptrs={}
  for n in sorted({v.split(',')[1] for k,v,line in sections['Structures']}):
   p=self.alloc(0x1800);self.block(0x45E145,0x45E151,{UC_X86_REG_ESI:p,UC_X86_REG_EBX:0});self.building_ptrs[n]=p
  tibsec,_=lexical((ri.ASSETS/'RULESMD.INI').read_bytes(),{'Tiberiums'});self.tiberium_ptrs={}
  self.u.mem_write(0xB0F4E8,ri.dwords(0x7EB6D4,self.alloc(256),64,1,0,10))
  for name in tibsec['Tiberiums'].values():
   p=self.alloc(0x200);self.invoke(0x7216C0,p,(self.cstring(name),));self.tiberium_ptrs[name]=p
  self.u.mem_write(0xA83CE0,ri.dwords(0x7EB6D4,self.alloc(1024),256,1,0,10));self.mtnk=self.alloc(0xF00);self.invoke(0x7470D0,self.mtnk,(self.cstring('MTNK'),))
  self.terrain_layers=[]
  for name,path in [('RULESMD.INI',ri.ASSETS/'RULESMD.INI'),('LANGRULE.INI',ri.ASSETS/'LANGRULE.INI'),('MPBattleMD.ini',ri.ASSETS/'MPBattleMD.ini'),(self.map_file.name,self.map_file)]:
   if not path.exists():assert name=='LANGRULE.INI';self.receipts.append(dict(file=name,absent=True));continue
   raw=path.read_bytes();lex,_=lexical(raw,set(self.names.values())|set(self.land_names)|{'General'}|set(names)|set(self.tiberium_ptrs)|{'MTNK'}|{v.split(',')[1] for k,v,line in sections['Structures']});self.make_ini(lex)
   rules_ini=self.alloc(0x40);self.u.mem_write(rules_ini,bytes(self.u.mem_read(ri.INI,0x40)));self.u.mem_write(ri.INI,art_ini)
   self.invoke(0x674000,0,(rules_ini,))
   if 'General' in lex:
    self.block(0x66F1CB,0x66F1EC,{UC_X86_REG_ESI:self.rules,UC_X86_REG_EDI:rules_ini})
    self.block(0x671DD2,0x671DF1,{UC_X86_REG_ESI:self.rules,UC_X86_REG_EDI:rules_ini})
   overlays=[]
   for n,p in self.building_ptrs.items():
    if n in lex:self.block(0x460AA0,0x460AE0,{UC_X86_REG_EBP:p,UC_X86_REG_ESI:rules_ini,UC_X86_REG_EBX:self.cstring(n),UC_X86_REG_EAX:0})
   for i,p in enumerate(self.overlay_ptrs):
    if self.invoke(0x5FE770,p,(rules_ini,))&255:overlays.append(i)
   for n,p in self.tiberium_ptrs.items():
    if not self.invoke(0x526810,rules_ini,(p+0x24,)):continue
    if tiberium_timer_readers:self.block(0x721A78,0x721AFA,{UC_X86_REG_ESI:p,UC_X86_REG_EBX:rules_ini})
    self.block(0x721AFA,0x721B12,{UC_X86_REG_EAX:self.read32(p+0xB8),UC_X86_REG_ECX:rules_ini,UC_X86_REG_EBX:rules_ini,UC_X86_REG_ESI:p,UC_X86_REG_EDI:p+0x24})
    self.u.reg_write(UC_X86_REG_ESP,ri.SP);self.u.reg_write(UC_X86_REG_EBX,rules_ini);self.u.reg_write(UC_X86_REG_ESI,p);self.u.reg_write(UC_X86_REG_EDI,p+0x24);run_checked(self.u,0x721C3F,(0x721C7B,0x721CDC),count=100000)
   if 'MTNK' in lex:
    p=self.mtnk;self.block(0x5F94B3,0x5F9516,{UC_X86_REG_EBX:p,UC_X86_REG_ESI:rules_ini,UC_X86_REG_EBP:p+0x24,UC_X86_REG_EAX:self.u.mem_read(p+0x231,1)[0]})
    # Original 712452..712473 reads ThreatAvoidanceCoefficient with the
    # retained Type+2F0 double as default; Foot Unlimbo copies it to Foot+530.
    for a,b in [(0x7121D1,0x7121EB),(0x712270,0x71228A),(0x7122BE,0x7122D8),(0x712452,0x712473),(0x714CC8,0x714CE9)]:self.block(a,b,{UC_X86_REG_EBP:p,UC_X86_REG_ESI:rules_ini,UC_X86_REG_EDI:rules_ini,UC_X86_REG_EBX:p+0x24})
    # Original MovementZone reader loads the INI argument from its retained
    # outer frame after two pushes. Keep its native default/store+5B4 owner.
    self.u.mem_write(ri.SP+0x380,ri.dwords(rules_ini));self.block(0x71605E,0x716090,{UC_X86_REG_EBP:p,UC_X86_REG_EBX:p+0x24})
   self.block(0x7476D3,0x747711,{UC_X86_REG_EDI:self.mtnk,UC_X86_REG_EBX:rules_ini,UC_X86_REG_EBP:self.mtnk+0x24})
   terrain=[]
   for n,p in self.terrain_ptrs.items():
    if self.invoke(0x71DEA0,p,(rules_ini,))&255:terrain.append(n)
   self.receipts.append(dict(file=name,sha256=sha(raw),bytes=len(raw),overlay_admitted=overlays,terrain_admitted=terrain))
   if tiberium_timer_readers:self.tiberium_timer_layers.append(dict(file=name,sha256=sha(raw),types=self.tiberium_queue_inputs()))
  assert all(self.string(p+0x1F8)==n for n,p in self.terrain_ptrs.items()), 'ART alias cache requires expansion'
  self.theater=t
  allnames={f'TileSet{i:04d}' for i in range(300)}
  raw=(ri.ASSETS/self.theater_file).read_bytes();baselex,_=lexical(raw,allnames);allnames|={v.get('SetName','No Name') for v in baselex.values()};full,_=lexical(raw,allnames);self.make_ini(full);self.tile_properties=[]
  for row in t['sets']:
   sec=f"TileSet{row['ordinal']:04d}";sn=self.cstring(sec);lex=full[sec];setname=lex.get('SetName','No Name');shadow=self.invoke(0x5295F0,ri.INI,(sn,self.cstring('ShadowCaster'),0))&255;count=self.invoke(0x5276D0,ri.INI,(sn,self.cstring('ShadowTiles'),0)) if shadow else 0
   last=self.invoke(0x5276D0,ri.INI,(sn,self.cstring('LastTilesInSet'),-1));assert last in (0xFFFFFFFF,row['count'])
   animations={}
   for j in range(row['count']):
    key=f'Tile{j+1:02d}Anim';buffer=self.alloc(128);length=self.invoke(0x528A10,ri.INI,(self.cstring(setname),self.cstring(key),self.cstring(''),buffer,128))
    if length:
     aname=self.string(buffer);ap=self.invoke(0x428B80,self.cstring(aname));index=self.invoke(self.read32(self.read32(ap)+0x40),ap);values={}
     for suffix,default in [('XOffset',0),('YOffset',0),('AttachesTo',-1),('ZAdjust',0)]:
      value=self.invoke(0x5276D0,ri.INI,(self.cstring(setname),self.cstring(f'Tile{j+1:02d}{suffix}'),default));values[suffix]=struct.unpack('<i',ri.dwords(value))[0]
     animations[j]=dict(name=aname,index=index,values=values)
   properties=dict(ordinal=row['ordinal'],base=row['base'],count=row['count'],setname=setname,shadow=shadow,shadow_tiles=count,last_tiles_in_set=last,animations=animations)
   if tiberium_timer_readers:
    # Original546124 zeroesEBX;546187..54619B reads this exact Boolean,
    # then54646D/546AB5 publishes the saved value to IsoTileType+306.
    properties['allow_tiberium']=self.invoke(0x5295F0,ri.INI,(sn,self.cstring('AllowTiberium'),0))&255
   self.tile_properties.append(properties)
  assert all(not self.u.mem_read(p+0x16BF,1)[0] and not self.u.mem_read(p+0x16C0,1)[0] for p in self.building_ptrs.values())
 def tiberium_queue_inputs(self):
  """Selected original constructor/readers; no host scalar conversion."""
  signed=lambda p:struct.unpack('<i',self.u.mem_read(p,4))[0]
  bits=lambda p:f"{struct.unpack('<Q',self.u.mem_read(p,8))[0]:016x}"
  return [dict(name=n,type_id=self.read32(p+0x98),growth=signed(p+0xA8),spread=signed(p+0x9C),growth_percentage_bits=bits(p+0xB0),spread_percentage_bits=bits(p+0xA0),value=signed(p+0xB8),base_overlay=self.read32(self.read32(p+0xE0)+0x294),max_density=self.read32(p+0xE4),images=self.read32(p+0xE8),extra=self.read32(p+0xEC),growth_timer=list(struct.unpack('<3i',self.u.mem_read(p+0x11C,12))),spread_timer=list(struct.unpack('<3i',self.u.mem_read(p+0x100,12)))) for n,p in self.tiberium_ptrs.items()]
 def read_map_tiberium_gates(self):
  """Original pre-init SpecialFlags and Basic growth on this reader VM.

  A temporary Scenario-sized buffer with declared zero flags supplies member
  storage; this is not Scenario construction or full scenario loading.
  """
  raw=self.map_file.read_bytes();lex,_=lexical(raw,{'Basic','SpecialFlags'});self.make_ini(lex)
  receiver=self.alloc(0x4000);self.u.mem_write(receiver,dwords(0))
  self.invoke(0x6B8CA0,receiver,(ri.INI,))
  self.block(0x683848,0x68384F,{UC_X86_REG_EBP:receiver})
  self.block(0x68A571,0x68A58F,{UC_X86_REG_ESI:receiver,UC_X86_REG_EDI:ri.INI})
  return dict(flags=self.read32(receiver),basic_growth_enabled=bool(self.u.mem_read(receiver+0x34A6,1)[0]),prior_flags=0,basic_default_store='00683848',basic_reader='0068A571..0068A58F',special_flags_reader='006B8CA0',map_sha256=sha(raw),basic_value=lex.get('Basic',{}).get('TiberiumGrowthEnabled'),special_values=lex.get('SpecialFlags',{}))
 def read_map_theater(self):
  # Full_Init687631..68764F executes the exact Map/Theater read, original
  # 475870 -> 528A10 -> 48DBE0 lookup and Scenario+1258 store. The surrounding
  # Scenario allocation and lexical INI cache remain declared host boundaries.
  raw=self.map_file.read_bytes();lex,_=lexical(raw,{'Map'});self.make_ini(lex)
  scenario=self.read32(0xA8B230);before=self.read32(scenario+0x1258)
  self.block(0x687631,0x68764F,{UC_X86_REG_EBP:ri.INI,UC_X86_REG_EBX:0})
  return dict(map_sha256=sha(raw),section='Map',key='Theater',default=0,
              source_value=lex.get('Map',{}).get('Theater'),before=before,
              value=self.read32(scenario+0x1258),reader='0x475870',
              lookup='0x48DBE0',caller='0x687631..0x68764F',field='Scenario+0x1258')
 def snapshot(self):
  used=sorted({c['overlay'] for c in self.physical.values() if c['overlay'] is not None}|set(self.damage_overlays))
  terrains={}
  for n,p in self.terrain_ptrs.items():
   row=Landing.terrainstate(self,p);f=self.read32(p+0x2B8);cells=[]
   for i in range(10):
    pair=list(struct.unpack('<hh',self.u.mem_read(f+i*4,4)))
    if pair==[32767,32767]:break
    cells.append(pair)
   else:raise AssertionError(('unterminated Terrain occupy list',n))
   row.update(image=self.string(p+0x1F8),occupy_list=cells,map_occupied=self.u.mem_read(p+0x235,1)[0]);terrains[n]=row
  return dict(art_sha256=sha((ri.ASSETS/'ARTMD.INI').read_bytes()),bootstrap_layers=[dict(file=v['file'],absent=v.get('absent',False),sha256=v.get('sha256'),sections=sorted(v.get('sections',{}))) for v in self.layers],mtnk=dict(strength=self.read32(self.mtnk+0xA0),speed_type=self.read32(self.mtnk+0x67C),movement_zone=self.read32(self.mtnk+0x5B4),crusher=self.u.mem_read(self.mtnk+0xD28,1)[0]),tiberiums={n:dict(index=self.read32(p+0x98),value=self.read32(p+0xB8),base_overlay=self.read32(self.read32(p+0xE0)+0x294),images=self.read32(p+0xE8),extra=self.read32(p+0xEC)) for n,p in self.tiberium_ptrs.items()},building_zone_gate_bytes={n:bytes(self.u.mem_read(p+0x16BF,2)).hex() for n,p in self.building_ptrs.items()},tile_properties=self.tile_properties,layers=self.receipts,cliff=self.u.mem_read(self.rules+0x664,1)[0],land_table_hex=bytes(self.u.mem_read(0x89EA40,432)).hex(),overlays=[dict(index=i,name=self.names[i],land=self.read32(self.overlay_ptrs[i]+0x298),crushable=self.u.mem_read(self.overlay_ptrs[i]+0x22D,1)[0],wall=self.u.mem_read(self.overlay_ptrs[i]+0x2A8,1)[0],tiberium=self.u.mem_read(self.overlay_ptrs[i]+0x2A9,1)[0],no_use_tile_land=self.u.mem_read(self.overlay_ptrs[i]+0x2AC,1)[0],rubble=self.u.mem_read(self.overlay_ptrs[i]+0x2B4,1)[0],rock=self.u.mem_read(self.overlay_ptrs[i]+0x2B5,1)[0]) for i in used],terrains=terrains)
