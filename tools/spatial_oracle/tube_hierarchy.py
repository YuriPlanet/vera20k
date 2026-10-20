"""Original 582D70 high/Tube hierarchy-pair production, with real path walker."""
import json
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP, UC_X86_REG_EBP, UC_X86_REG_EIP, UC_X86_REG_FPCW
from tools.native_oracle import RET_MAGIC, STACK_BASE, STACK_SIZE, NATIVE_SHA256, finish_vectors, load_image, provenance, run_checked
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.bridge_records import MAP, TABLE, CELLS, TUBES, TUBE_TABLE, RECORDS, DUMMY

ENTRY = 0x00582D70
NODES, BUCKETS, ROOT, DATA = 0x00BA0000, 0x00BB0000, 0x00BBF000, 0x00BC0000
DELTAS = [(0,-1),(1,-1),(1,0),(1,1),(0,1),(-1,1),(-1,0),(-1,-1)]


def fixture(name, direction=0, paths=((), ()), **options):
    a = options.pop("a", (5, 5))
    b = options.pop("b", (10, 5))
    sides = [tuple((v + d) & 65535 for v,d in zip(a, DELTAS[(direction + offset) & 7]))
             for offset in (2, -2)]
    tubes = [dict(cell=a, entry=a, exit=b, direction=direction, path=[])]
    for i, cell in enumerate(sides):
        tubes.append(dict(cell=cell, entry=(2, 2), exit=(12, 12), direction=7, path=list(paths[i])))
    return dict(name=name, size=(8, 8), level=0, a=a, b=b, tubes=tubes,
                missing=[], high_tile=None, seed_pairs=[], zone_mode="ordinary", dummy_index=-1, **options)


def cases():
    result = [fixture(f"tube_direction_{d}", d, ((2,2,4), (4,2,2))) for d in range(8)]
    result += [fixture("zero_length_sides"), fixture("marker8_sides", paths=([8], [8,6])),
               fixture("marker8_then_missing", paths=([8,8], [8,0,8])),
               fixture("raw_center_direction_min", -2147483648),
               fixture("raw_center_direction_max", 2147483647)]
    for which in ([1], [2], [1,2]):
        row = fixture("missing_sides_" + "_".join(map(str, which)))
        row["missing"] = which
        result.append(row)
    for level in (1, 2):
        row=fixture(f"level_{level}", paths=([2,2], [2,2])); row["level"]=level; result.append(row)
    for endpoint in ((26,0),(-1,1),(32767,-32768),(32767,32767)):
        result.append(fixture(f"endpoint_{endpoint[0]}_{endpoint[1]}", b=endpoint))
    for mode in ("zero", "equal", "signed_zones"):
        row=fixture(mode);row["zone_mode"]=mode;result.append(row)
    for tile in range(16):
        row=fixture(f"high_offset_{tile}");row["high_tile"]=100+tile;result.append(row)
    for name,a,b in (("negative",(0,0),(2,1)),("padding",(1,1),(3,3))):
        row=fixture("high_native_boundary_"+name,a=a,b=b);row["size"]=(2,2);row["high_tile"]=102;row["zone_mode"]="unique";result.append(row)
    row=fixture("wood_high");row["high_tile"]=206;result.append(row)
    row=fixture("high_packed_side_wrap", a=(32767,1));row["high_tile"]=103;result.append(row)
    row=fixture("first_pair_keeps_flags");row["seed_pairs"]=[(1,1,1)];row["zone_mode"]="equal";result.append(row)
    for raw in (14,15,-2,0x4000000E,0x3FFFFFFF,9,10,11,12,13,0x40000000,0x40000008,-0x40000000,-0x3ffffff8):
        result.append(fixture(f"raw_path_{raw}", paths=([raw,2],[raw,4])))
    for index in (-1,1):
        row=fixture(f"missing_sides_dummy_{index}", paths=([8],[8]));row["missing"]=[1,2];row["dummy_index"]=index;result.append(row)
    for seed in ([],[(2,1,1)],[(1,2,1)]):
        row=fixture("collision_seed_"+str(len(result)),paths=([2]*6,[2]*6));row["zone_mode"]="collision";row["seed_pairs"]=seed;result.append(row)
    return result


def initialized_process_fixture():
    uc=Uc(UC_ARCH_X86, UC_MODE_32);load_image(uc)
    uc.mem_map(STACK_BASE,STACK_SIZE);uc.mem_map(RET_MAGIC,0x1000)
    sp=STACK_BASE+STACK_SIZE-0x1000
    capture=json.loads(Path(__file__).with_name("tube_startup_capture.json").read_text())
    captured={int(row["eip"],16):row for row in capture["captures"]}
    assert captured[0x49F0E0]["fpcw"]==captured[0x49F190]["fpcw"]=="0xe7f"
    uc.reg_write(UC_X86_REG_FPCW,int(captured[0x49F0E0]["fpcw"],16))
    for initializer in (0x0049F0E0,0x0049F190,0x0049F2F0,0x0049F2D0,0x0049F280):
        uc.mem_write(sp,dwords(RET_MAGIC));uc.reg_write(UC_X86_REG_ESP,sp)
        run_checked(uc,initializer,RET_MAGIC,count=10000)
    expected=bytes.fromhex(captured[0x6BB9A0]["region_hex"])
    for address,length in ((0x89F680,8),(0x89F688,56),(0x89F6C0,8)):
        offset=address-int(captured[0x6BB9A0]["region_address"],16)
        assert bytes(uc.mem_read(address,length))==expected[offset:offset+length]
    return uc,sp


def execute(case):
    uc,sp=initialized_process_fixture()
    w,h=case["size"];width=w+h;side=width+1;level=case["level"]
    uc.mem_write(MAP+0xF4,dwords(w,h));uc.mem_write(MAP+0x6C,dwords(side*side,NODES))
    uc.mem_write(MAP+0x13C,dwords(TABLE,0x40000));uc.mem_write(TABLE,bytes(0x40000*4))
    uc.mem_write(0x00AA0E28,dwords(100));uc.mem_write(0x00ABAD1C,dwords(200))
    uc.mem_write(DUMMY,bytes(0x200));uc.mem_write(DUMMY+0x24,packed(1234,-2345))
    uc.mem_write(DUMMY+0x38,dwords(65535));uc.mem_write(DUMMY+0x116,struct.pack("<h",case["dummy_index"]))
    rows={tuple(t["cell"]):i for i,t in enumerate(case["tubes"]) if i not in case["missing"]}
    for ordinal,(cell,index) in enumerate(rows.items()):
        x,y=cell;x=x if x<32768 else x-65536;y=y if y<32768 else y-65536
        linear=y*512+x
        if 0<=linear<0x40000:
            ptr=CELLS+ordinal*0x200;uc.mem_write(ptr,bytes(0x200))
            uc.mem_write(ptr+0x24,packed(x,y));uc.mem_write(ptr+0x116,struct.pack("<h",index))
            uc.mem_write(ptr+0x38,dwords(case["high_tile"] if index==0 and case["high_tile"] is not None else 0))
            uc.mem_write(TABLE+linear*4,dwords(ptr))
    uc.mem_write(0x008B413C,dwords(TUBE_TABLE));uc.mem_write(0x008B4148,dwords(len(case["tubes"])))
    for i,tube in enumerate(case["tubes"]):
        ptr=TUBES+i*0x200;uc.mem_write(ptr,bytes(0x200));uc.mem_write(TUBE_TABLE+i*4,dwords(ptr))
        uc.mem_write(ptr+0x24,packed(*tube["entry"])+packed(*tube["exit"])+dwords(tube["direction"]))
        uc.mem_write(ptr+0x30,dwords(*tube["path"]));uc.mem_write(ptr+0x1C0,dwords(len(tube["path"])))
    mode=case["zone_mode"]
    zones=[0 if mode=="zero" else 1 if mode=="equal" else
           [0x7fff,0x8000,0xffff][(x+y)%3] if mode=="signed_zones" else
           1+x+y*width if mode=="unique" else 1+(x+17*y+level*3)%31 for y in range(width) for x in range(width)]
    if mode=="collision":
        for (x,y),zone in [((5,5),1),((10,5),2),((6,5),17),((12,5),18),((4,5),3)]:zones[y*width+x]=zone
    nodes=bytearray(side*side*10)
    for y in range(width):
        for x in range(width):struct.pack_into("<H",nodes,(y*side+x)*10+level*2,zones[y*width+x])
    uc.mem_write(NODES,bytes(nodes));uc.mem_write(MAP+0x80+level*4,dwords(ROOT));uc.mem_write(ROOT,dwords(BUCKETS))
    for i in range(256):uc.mem_write(BUCKETS+i*24,dwords(0,DATA+i*512,32,0,0,0))
    for a,b,flag in case["seed_pairs"]:
        bucket=((a&15)<<4)|(b&15);pair=(a<<16)|b
        uc.mem_write(DATA+bucket*512,dwords(pair,pair,flag));uc.mem_write(BUCKETS+bucket*24+16,dwords(1))
    uc.mem_write(RECORDS,packed(*case["a"])+packed(*case["b"])+dwords(1,1))
    uc.mem_write(sp,dwords(RET_MAGIC,RECORDS,level));uc.reg_write(UC_X86_REG_ESP,sp);uc.reg_write(UC_X86_REG_ECX,MAP)
    run_checked(uc,ENTRY,RET_MAGIC,count=100000,required_addresses=[0x00486750])
    edges=[]
    for i in range(256):
        count=struct.unpack("<I",uc.mem_read(BUCKETS+i*24+16,4))[0]
        if count>32:raise RuntimeError("fixture bucket capacity exceeded")
        for j in range(count):
            a,b,flag=struct.unpack("<III",uc.mem_read(DATA+i*512+j*12,12))
            if a!=b:raise RuntimeError("duplicate packed words differ")
            edges.append([i,a,flag&255])
    return {**case,"width":width,"zones":zones,"edges":edges,
            "dummy_coord":struct.unpack("<hh",uc.mem_read(DUMMY+0x24,4))}


def dummy_writes():
    uc=Uc(UC_ARCH_X86,UC_MODE_32);load_image(uc)
    uc.mem_map(STACK_BASE,STACK_SIZE)
    uc.mem_write(MAP+0x13C,dwords(TABLE,0x40000));uc.mem_write(TABLE,bytes(0x40000*4))
    uc.mem_write(TABLE+1*4,dwords(CELLS));uc.mem_write(CELLS+0x116,struct.pack("<h",-1))
    uc.mem_write(DUMMY+0x24,packed(1234,-2345));uc.mem_write(DUMMY+0x116,struct.pack("<h",-1))
    sp=STACK_BASE+STACK_SIZE-0x1000;coord_ptr=RECORDS
    writes=[((-1,0),0),((1,0),1),((-511,1),32768),((300,300),65535),((1,-1),65536)]
    transcript=[]
    for coord,ordinal in writes:
        uc.mem_write(coord_ptr,packed(*coord));uc.reg_write(UC_X86_REG_EBP,coord_ptr)
        uc.reg_write(UC_X86_REG_ESP,sp);uc.mem_write(sp+0x10,dwords(ordinal))
        run_checked(uc,0x0072850A,0x00728520,count=1000,required_addresses=[0x005657A0,0x00728519])
        transcript.append(dict(coord=coord,ordinal=ordinal,
            dummy_coord=struct.unpack("<hh",uc.mem_read(DUMMY+0x24,4)),
            dummy_index=struct.unpack("<h",uc.mem_read(DUMMY+0x116,2))[0],
            real_index=struct.unpack("<h",uc.mem_read(CELLS+0x116,2))[0]))
    return transcript


def path_gate_cases():
    result=[]
    stops={0x0042CB36:"reject",0x0042CB46:"precheck",0x0042CB8B:"ordinary"}
    for a,b in ((1,65535),(2,2),(2,3)):
        for allow in (0,1):
            uc=Uc(UC_ARCH_X86,UC_MODE_32);load_image(uc);uc.mem_map(STACK_BASE,STACK_SIZE)
            sp=STACK_BASE+STACK_SIZE-0x1000;uc.reg_write(UC_X86_REG_ESP,sp)
            uc.mem_write(sp+0x1C,dwords(a));uc.mem_write(sp+0x3C,dwords(b));uc.mem_write(sp+0x4C,bytes([allow]))
            run_checked(uc,0x0042CB22,tuple(stops),count=100,required_addresses=[0x0042CB2A])
            result.append(dict(a=a,b=b,allow=allow,action=stops[uc.reg_read(UC_X86_REG_EIP)]))
    return result


def constant_walks():
    result=[]
    for token in (14,15,-2,0x4000000E,0x3FFFFFFF):
        uc,sp=initialized_process_fixture();scratch=STACK_BASE+0x1000
        uc.mem_write(scratch,packed(10,20));uc.mem_write(scratch+8,dwords(token))
        uc.mem_write(sp,dwords(RET_MAGIC,1,scratch+8));uc.reg_write(UC_X86_REG_ESP,sp)
        uc.reg_write(UC_X86_REG_ECX,scratch+16);uc.reg_write(UC_X86_REG_EDX,scratch)
        run_checked(uc,0x00429780,RET_MAGIC,count=1000)
        result.append(dict(token=token,start=[10,20],end=struct.unpack("<hh",uc.mem_read(scratch+16,4))))
    return result


def get_zone_cases():
    fixtures=[
        dict(name="ordinary_raw_ffff",query=[5,5],flags=0,check=1,raw=65535,real=True,records=[]),
        dict(name="missing_structural_raw_ffff",query=[5,5],flags=256,check=1,raw=65535,real=True,records=[]),
        dict(name="structural_check_disabled",query=[5,5],flags=256,check=0,raw=65535,real=True,records=[]),
        dict(name="missing_structural_raw2",query=[5,5],flags=256,check=1,raw=2,real=True,records=[]),
        dict(name="dummy_structural_missing",query=[-1,0],flags=256,check=1,raw=2,real=False,records=[]),
        dict(name="dummy_ordinary",query=[-1,0],flags=0,check=1,raw=2,real=False,records=[]),
        dict(name="signed_tolerance_matches",query=[-1,1],flags=256,check=1,raw=2,real=True,records=[([0,0],[0,3],True,0)]),
        dict(name="signed_tolerance_rejects",query=[-1,1],flags=256,check=1,raw=2,real=True,records=[([1,0],[1,3],True,0)]),
        dict(name="reversed_span_rejects",query=[0,1],flags=256,check=1,raw=2,real=True,records=[([0,3],[0,0],True,0)]),
        dict(name="tube_kind_not_bridge",query=[0,1],flags=256,check=1,raw=2,real=True,records=[([0,0],[0,3],True,1)]),
    ]
    for name,tile,land in (("high",106,0),("wood",206,0),("rock",106,3),("ordinary",0,0),("boundary",None,0)):
        fixtures.append(dict(name="inactive_east_"+name,query=[5,5],flags=256,check=1,raw=2,real=True,
            records=[([2,5],[8,5],False,0)],extra=[] if tile is None else [([6,5],tile,land,0)]))
    fixtures.append(dict(name="inactive_south_high",query=[5,5],flags=256,check=1,raw=2,real=True,
        records=[([5,2],[5,8],False,0)],extra=[([5,6],106,0,0)]))
    fixtures.append(dict(name="inactive_signed_alias_cell_coord",query=[-1,1],flags=256,check=1,raw=2,real=True,
        records=[([0,0],[0,3],False,0)],extra=[([511,1],106,0,0)]))
    results=[]
    for row in fixtures:
        uc,sp=initialized_process_fixture()
        uc.mem_write(MAP+0xF4,dwords(8,8));uc.mem_write(MAP+0x68,dwords(NODES,17*17))
        uc.mem_write(NODES,b"\x07\x00\x01\x00"*(17*17));uc.mem_write(MAP+0x18,dwords(DATA))
        uc.mem_write(DATA,struct.pack("<HHH",0,row["raw"],3))
        uc.mem_write(MAP+0x13C,dwords(TABLE,0x40000));uc.mem_write(TABLE,bytes(0x40000*4))
        uc.mem_write(DUMMY,bytes(0x200));uc.mem_write(DUMMY+0x24,packed(1234,-2345))
        x,y=row["query"];linear=y*512+x
        target=CELLS if row["real"] else DUMMY
        uc.mem_write(target+0x140,dwords(row["flags"]))
        if row["real"]:
            uc.mem_write(TABLE+linear*4,dwords(CELLS));uc.mem_write(CELLS+0x24,packed(linear%512,linear//512))
        uc.mem_write(DUMMY+0x38,dwords(65535))
        uc.mem_write(0x00AA0E28,dwords(100));uc.mem_write(0x00ABAD1C,dwords(200))
        for i,(coord,tile,land,flags) in enumerate(row.get("extra",[])):
            ptr=CELLS+(i+1)*0x200;uc.mem_write(ptr,bytes(0x200));uc.mem_write(ptr+0x24,packed(*coord))
            uc.mem_write(ptr+0x38,dwords(tile));uc.mem_write(ptr+0xEC,dwords(land));uc.mem_write(ptr+0x140,dwords(flags))
            uc.mem_write(TABLE+(coord[1]*512+coord[0])*4,dwords(ptr))
        uc.mem_write(MAP+0x54,dwords(RECORDS));uc.mem_write(MAP+0x60,dwords(len(row["records"])))
        for i,(a,b,active,kind) in enumerate(row["records"]):
            uc.mem_write(RECORDS+i*16,packed(*a)+packed(*b)+dwords(int(active),kind))
            if not active:
                index=max(0,min(17*17-1,b[1]*17+b[0]));uc.mem_write(NODES+index*4+2,struct.pack("<H",2))
        query=STACK_BASE+0x1000;uc.mem_write(query,packed(x,y));uc.mem_write(sp,dwords(RET_MAGIC,query,0,row["check"]))
        uc.reg_write(UC_X86_REG_ESP,sp);uc.reg_write(UC_X86_REG_ECX,MAP)
        run_checked(uc,0x0056D230,RET_MAGIC,count=10000)
        results.append({**row,"result":uc.reg_read(UC_X86_REG_EAX),"dummy_coord":struct.unpack("<hh",uc.mem_read(DUMMY+0x24,4))})
    return results


if __name__=="__main__":
    finish_vectors(lambda:{"cases":[execute(case) for case in cases()],"dummy_writes":dummy_writes(),"path_gate":path_gate_cases(),"constant_walks":constant_walks(),"get_zone":get_zone_cases()},Path(__file__).with_suffix(".json"),
        provenance=lambda:provenance(scope="Original582D70 full high/Tube helper and429780 path walk into temporary hierarchy buckets",
            assumptions=["Supplied cell/tube/zone state, original direction initializer executes",
                         "Native sideW+H+1 with zero hierarchy padding; all three levels sampled",
                         "Adequate preallocated temporary buckets; allocation failures excluded",
                         "No code patches or substituted returns; no full floodfill or final edge allocation",
                         "42CB22 gate stops at reject, precheck-call preparation or ordinary continuation; raw labels/allowHS supplied",
                         "Path0..8, adjacent zero slots9..13, captured initialized constants14/15/-2/-1 address aliases sampled; arbitrary raw data domain unproved",
                         "Original constant initializers run at hardware-captured FPCW0xE7F and bytes are checked against tube_startup_capture.json"],
            substitutions=[],entry_points={"register":ENTRY,"walk":0x00429780,"append":0x0058AF80,"last_pair":0x00589E20,"raw_binding_tail":0x0072850A,"raw_zone_equality_gate":0x0042CB22,"path_get_zone":0x0056D230}))
