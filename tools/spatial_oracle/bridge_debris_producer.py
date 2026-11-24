"""Original bridge fallout/constructors on production-exported retail ART inputs.

Native original instructions produce all outputs; input field provenance and
substitutions are recorded in metadata. See companion input JSON and README.
"""
import struct,json
from functools import lru_cache
from pathlib import Path
from unicorn import UC_HOOK_MEM_READ
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_ESI, UC_X86_REG_EBX
from tools.native_oracle import finish_vectors, provenance, run_checked
from tools.spatial_oracle import anim_bouncer_launch as launch
from tools.spatial_oracle.anim_bouncer_launch import dwords, read32, signed

ENTRY, DECK_INIT = 0x47DD70, 0x47B2C0
METAL_ITEMS, EXPLOSION_ITEMS, DIRTY_ITEMS = (launch.MEM + n for n in (0x18000, 0x19000, 0x1A000))
TYPE_STUB, FALSE_STUB = launch.STUB + 0x100, launch.STUB + 0x110
OBJECTS, OBJECT_TYPE, WARHEAD = (launch.MEM + n for n in (0x20000, 0x24000, 0x28000))
METALLIC = [f'DBRIS{i}LG' for i in range(1,10)]+['DBRS10LG']+[f'DBRIS{i}SM' for i in range(1,5)]+['D']
EXPLOSIONS = ['TWLT026','TWLT036','TWLT050','TWLT070']
INPUT_PATH=Path(__file__).with_name('bridge-retail-anim-inputs.json')
@lru_cache(maxsize=1)
def retail_inputs():
 return {row['name']:row for row in json.loads(INPUT_PATH.read_text())['rows']}
def f64(bits):return struct.unpack('<d',struct.pack('<Q',bits))[0]


class Machine(launch.Machine):
    def __init__(self, seed, fail_alloc=None):
        self.constructed = []
        self.constructed_types = {}
        self.ctor_pending = {}
        self.fail_alloc = fail_alloc
        self.alloc_index = 0
        super().__init__(seed)
        self.uc.hook_add(UC_HOOK_MEM_READ, self.read_hook)

    def read_hook(self, uc, _access, address, size, _value, _data):
        for pool, base, names in [('metallic', METAL_ITEMS, METALLIC),
                                  ('explosion', EXPLOSION_ITEMS, EXPLOSIONS)]:
            if base <= address < base + len(names) * 4:
                index = (address - base) // 4
                self.events.append(dict(call='pool_lookup', pool=pool, index=index,
                    type=names[index], size=size, instruction=f'0x{uc.reg_read(UC_X86_REG_EIP):08X}'))

    def hook(self, uc, address, size, data):
        if address in self.ctor_pending:
            anim = self.ctor_pending.pop(address)
            self.events.append(dict(call='anim_ctor_return', anim=anim-launch.HEAP,
                raw_draw_count=self.advances, rng_state=self.rng(), **launch.anim_state(uc, anim)))
        if address == 0x5657A0:
            self.ret(launch.CELL,4)
            return
        if address == launch.START:
            self.events.append(dict(call='start_native', anim=uc.reg_read(UC_X86_REG_ECX)-launch.HEAP))
            return  # Observe only: native Start executes, unlike base launch harness.
        if address == 0x424F00:
            self.events.append(dict(call='middle_native', anim=uc.reg_read(UC_X86_REG_ECX)-launch.HEAP))
        if address in (TYPE_STUB, FALSE_STUB):
            self.ret(OBJECT_TYPE if address == TYPE_STUB else 0, 0)
            return
        if address == 0x701900:
            sp = uc.reg_read(UC_X86_REG_ESP)
            obj = uc.reg_read(UC_X86_REG_ECX)
            args = struct.unpack('<7I', uc.mem_read(sp + 4, 28))
            self.events.append(dict(call='native_ground_receiver', object_index=(obj-OBJECTS)//0x1000,
                damage_is_health=args[0] == obj + 0x6C, distance=args[1],
                warhead_matches=args[2] == WARHEAD, attacker=args[3], ignore_defenses=args[4],
                arg6=args[5], source_house=args[6], health_before=signed(read32(uc,obj+0x6C))))
        if address == 0x701C1C:
            self.events.append(dict(call='native_immunity_zero_health', instruction='0x00701C1C'))
        if address == launch.NEW:
            self.alloc_index += 1
            if self.fail_alloc == self.alloc_index:
                self.events.append(dict(call='new_failed', allocation=self.alloc_index,
                    size=read32(uc,uc.reg_read(UC_X86_REG_ESP)+4)))
                self.ret(0, 0)
                return
        if address == launch.CTOR:
            anim = uc.reg_read(UC_X86_REG_ECX)
            sp = uc.reg_read(UC_X86_REG_ESP)
            self.constructed.append(anim)
            self.constructed_types[anim] = self.types[read32(uc,sp+4)]
            self.ctor_pending[read32(uc,sp)] = anim
            self.events.append(dict(call='anim_ctor_entry', anim=anim-launch.HEAP,
                raw_draw_count=self.advances, rng_state=self.rng()))
        super().hook(uc, address, size, data)

    def setup(self, case):
        uc = self.uc
        types = []
        for addr,items in ((0x8B4150,launch.MEM+0x1B000),(0xB0F670,launch.MEM+0x1C000)):
            uc.mem_write(addr,dwords(0x7EB6D4,items,64,1,0,10))
        for index,name in enumerate(METALLIC+EXPLOSIONS):
            pointer=launch.TYPES+index*0x400
            self.types[pointer]=name
            name_ptr=pointer+0x380
            uc.mem_write(name_ptr,name.encode()+b'\0')
            uc.mem_write(launch.SP,dwords(launch.STOP,name_ptr))
            uc.reg_write(UC_X86_REG_ESP,launch.SP);uc.reg_write(UC_X86_REG_ECX,pointer)
            run_checked(uc,0x427530,launch.STOP,count=1000000)
            row=retail_inputs()[name]
            if name=='D':
                assert row.get('missing_runtime_config') or not row.get('art_body_read')
            else:
                # Production-resolved ART scalar fields are supplied, not claimed native reads.
                for offset,key in ((0x2B4,'start'),(0x2B8,'loop_start'),(0x2BC,'loop_end'),(0x2C0,'end'),(0x2C4,'loop_count'),(0x2B0,'rate'),(0x334,'damage_radius')):
                    uc.mem_write(pointer+offset,dwords(row[key]))
                for offset,key in ((0x2A8,'damage_f64_bits'),(0x310,'elasticity_f64_bits'),(0x318,'min_z_vel_f64_bits'),(0x328,'max_xy_vel_f64_bits')):
                    uc.mem_write(pointer+offset,struct.pack('<Q',row[key]))
                uc.mem_write(pointer+0x35A,bytes([row['bouncer']]))
                uc.mem_write(pointer+0x362,bytes(['normalized: true' in row['full_config_debug']]))
                # Independently distinguished by bridge_anim_inputs asymmetric controls.
                uc.mem_write(pointer+0x36B,bytes([row['scorch']]))
                uc.mem_write(pointer+0x36D,bytes([row['crater']]))
                # Original image metadata suffix derives Middle=frame_count/2.
                image=launch.MEM+0x1D000+index*0x100
                uc.mem_write(image+6,struct.pack('<h',row['raw_shp_frame_count']))
                uc.mem_write(pointer+0xA4,dwords(image))
                uc.reg_write(UC_X86_REG_ESI,pointer)
                run_checked(uc,0x427C12,0x427C80,count=1000)
                # Current export contains stored RandomRate, not authored pair.
                uc.mem_write(pointer+0x2E4,dwords(*(row['random_rate'] or [0,0])))
                # Symbol targets are supplied bindings, resolved after all types exist.
            frames=row.get('raw_shp_frame_count',0) or 0
            types.append(dict(name=name,frames=frames,middle_frame=signed(read32(uc,pointer+0x298)),
                bouncer=bool(uc.mem_read(pointer+0x35A,1)[0]),
                elasticity=struct.unpack('<d',uc.mem_read(pointer+0x310,8))[0],
                max_xy=struct.unpack('<d',uc.mem_read(pointer+0x328,8))[0],
                min_z=struct.unpack('<d',uc.mem_read(pointer+0x318,8))[0],
                max_z=struct.unpack('<d',uc.mem_read(pointer+0x320,8))[0],
                random_rate=row.get('random_rate'),random_rate_stored=[signed(read32(uc,pointer+x)) for x in (0x2e4,0x2e8)],
                rate=signed(read32(uc,pointer+0x2b0)),loop_count=signed(read32(uc,pointer+0x2c4)),
                end=signed(read32(uc,pointer+0x2c0)),loop_end=signed(read32(uc,pointer+0x2bc)),
                normalized=bool(uc.mem_read(pointer+0x362,1)[0]),source=row))
            if index<len(METALLIC):uc.mem_write(METAL_ITEMS+4*index,dwords(pointer))
            else:uc.mem_write(EXPLOSION_ITEMS+4*(index-len(METALLIC)),dwords(pointer))
        self.type_by_name={name:ptr for ptr,name in self.types.items()}
        for name,ptr in self.type_by_name.items():
            row=retail_inputs()[name]
            if row.get('expire_anim'):uc.mem_write(ptr+0x304,dwords(self.type_by_name[row['expire_anim']]))
        # GameOptions game-speed index4 is an explicit runtime boundary; original
        # SpeedNormalize executes. Delayed sibling Start is outside this producer.
        uc.mem_write(0xA8EB60,dwords(4))
        uc.mem_write(launch.RULES+0x140, dwords(METAL_ITEMS))
        uc.mem_write(launch.RULES+0x14C, dwords(len(METALLIC)))
        uc.mem_write(launch.RULES+0x15C, dwords(EXPLOSION_ITEMS))
        uc.mem_write(launch.RULES+0x168, dwords(case.get('explosion_count',len(EXPLOSIONS))))
        uc.mem_write(launch.CELL+0x24, struct.pack('<hh', *case['cell']))
        uc.mem_write(launch.CELL+0x11B, bytes([case['level'] & 255]))
        uc.mem_write(0xA8ED6B, bytes([case.get('map_editor',0)]))
        uc.mem_write(0x87F8C0, dwords(DIRTY_ITEMS,64))
        uc.mem_write(0x87F8CC, dwords(0))
        # Original initializer, with the established coordinate scalar supplied.
        uc.mem_write(0x89E7C0, dwords(104))
        uc.mem_write(launch.SP,dwords(launch.STOP))
        uc.reg_write(UC_X86_REG_ESP,launch.SP)
        run_checked(uc,DECK_INIT,launch.STOP,required_addresses=(0x47B2DB,))
        assert read32(uc,0x89E7B4)==416
        ground_count=case.get('synthetic_ground_objects',0)
        if ground_count:
            uc.mem_write(launch.RULES+0xFA8,dwords(WARHEAD))
            uc.mem_write(OBJECT_TYPE+0xD37,b'\x01')
            uc.mem_write(WARHEAD+0x177,b'\x01')
            for slot,entry in [(0x84,TYPE_STUB),(0x160,FALSE_STUB),(0x1D4,FALSE_STUB),(0x16C,0x701900)]:
                uc.mem_write(launch.TECHNO_VT+slot,dwords(entry))
            for index in range(ground_count):
                obj=OBJECTS+index*0x1000
                uc.mem_write(obj,dwords(launch.TECHNO_VT))
                uc.mem_write(obj+0x6C,dwords(100+index))
                uc.mem_write(obj+0x30,dwords(obj+0x1000 if index+1<ground_count else 0))
            uc.mem_write(launch.CELL+0xE4,dwords(OBJECTS))
        return types

def execute(case):
    m=Machine(case['seed'],case.get('fail_allocation'))
    uc=m.uc
    types=m.setup(case)
    m.events.clear();m.advances=0
    before=m.rng()
    uc.mem_write(launch.SP,dwords(launch.STOP))
    uc.reg_write(UC_X86_REG_ESP,launch.SP)
    uc.reg_write(UC_X86_REG_ECX,launch.CELL)
    run_checked(uc,ENTRY,launch.STOP,count=2_000_000,required_addresses=(0x47E036,))
    assert uc.reg_read(UC_X86_REG_ESP)==launch.SP+4
    assert not m.pending and not m.ctor_pending
    return dict(input=case,events=m.events,raw_draw_count=m.advances,
        rng_before=before,rng_after=m.rng(),deck_offset=read32(uc,0x89E7B4),
        dirty_count=read32(uc,0x87F8CC),dirty_cell=list(struct.unpack('<hh',uc.mem_read(DIRTY_ITEMS,4))),
        ground_health_after=[signed(read32(uc,OBJECTS+i*0x1000+0x6C)) for i in range(case.get('synthetic_ground_objects',0))],
        anims=[dict(type=m.constructed_types[a],**launch.anim_state(uc,a)) for a in m.constructed]),types

def generate():
    rows=[]
    for seed in range(1,257):
        row,types=execute(dict(seed=seed,cell=[12,9],level=4))
        rows.append(row)
    # Signed source level/coordinate boundaries; no dependence on bridge flags.
    for cell,level in [([0,0],0),([-1,-2],-1),([32767,-32768],-128),([-300,1500],127)]:
        row,_=execute(dict(seed=1,cell=cell,level=level));rows.append(row)
    for variant in [dict(map_editor=1),dict(explosion_count=0),dict(explosion_count=-1),
                    dict(fail_allocation=1),dict(fail_allocation=2),dict(synthetic_ground_objects=2)]:
        row,_=execute(dict(seed=1,cell=[12,9],level=4,**variant));rows.append(row)
    selected=sorted({e['index'] for r in rows for e in r['events'] if e['call']=='pool_lookup' and e['pool']=='metallic'})
    assert selected==list(range(15)),selected
    return dict(retail_anim_types=types,metallic_pool=METALLIC,
        explosion_pool=EXPLOSIONS,selected_metallic_indices=selected,rows=rows)

def death_loop(case):
    m=Machine(case['seed']);u=m.uc;types=m.setup(dict(seed=case['seed'],cell=[12,9],level=4))
    m.events.clear();m.advances=0;before=m.rng()
    u.mem_write(launch.TECHNO,dwords(launch.TECHNO_VT));u.mem_write(launch.TECHNO_VT+0x48,dwords(launch.TECHNO_COORDS));m.techno_coord=case['coord']
    u.reg_write(UC_X86_REG_ESP,launch.SP-0x400);u.reg_write(UC_X86_REG_ESI,launch.TECHNO);u.reg_write(UC_X86_REG_EBX,case['pieces'])
    run_checked(u,launch.LOOP_BEGIN,launch.LOOP_END,count=5000000)
    return dict(input=case,events=m.events,raw_draw_count=m.advances,rng_before=before,rng_after=m.rng(),anims=[dict(type=m.constructed_types[a],**launch.anim_state(u,a)) for a in m.constructed])

def all_cases():
    data=generate()
    data['death_loop']=[death_loop(dict(seed=seed,pieces=pieces,coord=[3000,2500,104])) for seed in (1,5,31,1554098974) for pieces in (1,2,3)]
    return data

def metadata():
 return provenance(scope='Original complete bridge fallout47DD70, actual15-entry native-read MetallicDebris and4-entry BridgeExplosions pools, all15 selected by originalScenario draws, full AnimClass constructors and immediate Start. Actual production-resolved ART and image frame metadata supplied; native AnimType constructor defaults for D. Separate original deathloop support rows. No flight/landing claim.',assumptions=[
  'Input source bridge-retail-anim-inputs.json: production headlessHills load on PR552 release, AssetManager+RuleSet ArtRegistry after binding. Retained input file accompanies harness. Fourteen DBRIS images15frames; TWLT frames17,17,17,26; absentD section/image represented by original427530 defaults. Existing production missingD is intentionally not copied as behavior.',
  'All injected ART scalars/references/flags/rates and physical SHP counts are independently established by rules_oracle/bridge_anim_inputs.py original full427530/427D00/427B50 execution; that corpus asserts these production exports. This producer still supplies those established fields at its declared boundary.',
  'Original427530 executes for all19types with populated native registries; ART scalar fields and referencedExpireAnim pointers supplied afterward. Original427C12..427C80 executes suppliedimageframecount metadata arithmetic. Stored RandomRate suppliedfromproduction (no second900/x); constructor draws originalRNG, including no-advance equal1..1.',
  'Middle7/8/13 computed by originalimage metadata suffix. Image pixel payloads absent; no rendering claim. Normalized flags supplied; game speed index4 atA8EB60, verified constructor422209 passes this address to original5FB2E0.',
  'Only instant metallic Start executes; sibling explosion delay positive so Report/smudge behavior occurs later and is not claimed. Their sound indices remain ctor-1 boundaries; no effect on captured producer. DBRIS1LG trailerSMOKEY2 omitted from suppliedpointer binding, outside producer; no flight claim.',
  'Map celllookup controlled; ground/deck empty exceptexplicit synthetic Techno radiation-immunity row. Original47B2C0 computes416from104. Declaredallocationfailures preserved. x87 FPCW0x0E7F.',
 ],substitutions=['MapGetCell565730/5657A0 return suppliedCELL; ObjectMark5F5850 and DisplaySubmit4A9720 return1.','operatornew7C8E17 bump allocation or declared failure; synthetic ground-only type accessor/false-test boundary stubs.','Input ART fields, imageframecount, registry sparecapacity and referencebindings are fixturestate, notnativeINI/asset-loader execution. No native instruction code patches.'],entry_points={'bridge_fallout':ENTRY,'anim_type_ctor':0x427530,'image_metadata':0x427C12,'anim_ctor':launch.CTOR,'anim_start':launch.START,'bounce_init':launch.INIT,'death_loop':launch.LOOP_BEGIN,'scenario_seed':launch.SEED})
if __name__=='__main__':finish_vectors(all_cases,Path(__file__).with_suffix('.json'),provenance=metadata)
