"""Original42C927..42CA43 endpoint preparation, then supplied allowHS gate."""
import struct
from pathlib import Path
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_EBP
from tools.native_oracle import STACK_BASE, finish_vectors, provenance, run_checked
from tools.spatial_oracle.tube_hierarchy import initialized_process_fixture, MAP, TABLE, CELLS, DUMMY, NODES, DATA, RECORDS
from tools.spatial_oracle.map_queries import dwords, packed


def cases():
    def row(name,start=(4,5),goal=(5,5),cells=None,records=None,**kw):
        return dict(name=name,start=start,goal=goal,cells=cells if cells is not None else
            [((4,5),0,0,0),((5,5),0,0,0)],records=records or [],start_bridge=False,
            bounds=[8,0,0,8,8],dummy_flags=0,**kw)
    result=[]
    for allow in (False,True):
        result.append(row("ordinary_"+str(allow),allow=allow))
        result.append(row("initial_source_miss_"+str(allow),start=(-1,0),allow=allow))
        result.append(row("both_retained_dummy_"+str(allow),start=(-1,0),goal=(-2,0),allow=allow))
        result.append(row("source_dummy_changed_by_goal_query_"+str(allow),start=(-1,0),
            cells=[((5,5),256,0,0)],records=[((2,5),(8,5),False)],allow=allow))
    for allow in (False,True):
        c=row("source_projection_changes_retained_goal_"+str(allow),start=(5,5),goal=(-1,0),
            cells=[((5,5),256,0,0)],records=[((5,4),(5,8),False)],allow=allow)
        c["start_bridge"]=True;result.append(c)
    for name,flags,record in (("active_lane",256,((0,5),(8,5),True)),
          ("active_signed_distance",256,((0,-32768),(0,3),True)),
          ("active_lane_word_wrap",2304,((0,-1),(3,32767),True))):
        q=(1,0) if "signed" in name else (1,0) if "wrap" in name else (5,5)
        result.append(row(name,goal=q,cells=[((4,5),0,0,0),(q,flags,0,0)],records=[record],allow=False))
    for flags in (256,2304):
        for rock in (False,True):
            q=(5,5);positive=(5,6) if flags==256 else (6,5);negative=(5,4) if flags==256 else (4,5)
            result.append(row(f"no_record_{flags}_rock{rock}",cells=[((4,5),0,0,0),(q,flags,0,0),
                (positive,0,106,3 if rock else 0),(negative,0,206,0)],allow=True))
    result.append(row("active_lane_adjusted_distance",goal=(1,2),cells=[((4,5),0,0,0),((1,2),256,0,0)],
        records=[((3,1),(3,4),True)],allow=False))
    result.append(row("inactive_alias_stored_coord",goal=(-1,1),cells=[((4,5),0,0,0),((511,0),256,0,0),((511,1),0,106,0)],
        records=[((0,0),(0,3),False),((511,0),(511,3),False)],allow=False))
    return result


def execute(case):
    uc,sp=initialized_process_fixture()
    uc.mem_write(0xABD480,packed(0,0)) # original5618B0 sole constructor stores
    uc.mem_write(MAP+0xF4,dwords(8,8));uc.mem_write(MAP+0xFC,dwords(*case["bounds"][1:]))
    uc.mem_write(MAP+0x68,dwords(NODES,17*17));uc.mem_write(NODES,b"\x07\x00\x01\x00"*(17*17))
    uc.mem_write(MAP+0x18,dwords(DATA));uc.mem_write(DATA,struct.pack("<HH",0,2))
    uc.mem_write(MAP+0x13C,dwords(TABLE,0x40000));uc.mem_write(TABLE,bytes(0x40000*4))
    uc.mem_write(0xAA0E28,dwords(100));uc.mem_write(0xABAD1C,dwords(200))
    uc.mem_write(DUMMY,bytes(0x200));uc.mem_write(DUMMY+0x24,packed(1234,-2345))
    uc.mem_write(DUMMY+0x38,dwords(65535));uc.mem_write(DUMMY+0x140,dwords(case["dummy_flags"]))
    for i,(c,flags,tile,land) in enumerate(case["cells"]):
        x,y=c;ptr=CELLS+i*0x200
        uc.mem_write(ptr,bytes(0x200));uc.mem_write(ptr+0x24,packed(x,y));uc.mem_write(ptr+0x140,dwords(flags))
        uc.mem_write(ptr+0x38,dwords(tile));uc.mem_write(ptr+0xEC,dwords(land));uc.mem_write(TABLE+(y*512+x)*4,dwords(ptr))
    uc.mem_write(MAP+0x54,dwords(RECORDS));uc.mem_write(MAP+0x60,dwords(len(case["records"])))
    for i,(a,b,active) in enumerate(case["records"]):uc.mem_write(RECORDS+i*16,packed(*a)+packed(*b)+dwords(int(active),0))
    source=STACK_BASE+0x1000;goal=source+4;mover=DATA+0x100;astar=DATA+0x300
    uc.mem_write(source,packed(*case["start"]));uc.mem_write(goal,packed(*case["goal"]))
    uc.mem_write(mover,bytes(0x100));uc.mem_write(mover+0x8C,bytes([case["start_bridge"]]))
    uc.mem_write(sp+0x34,dwords(source,goal,mover));uc.mem_write(sp+0x48,dwords(0,0))
    uc.reg_write(UC_X86_REG_ESP,sp);uc.reg_write(UC_X86_REG_EBP,astar)
    run_checked(uc,0x42C927,0x42CA43,count=100000)
    labels=struct.unpack("<I",uc.mem_read(sp+0x1C,4))[0],struct.unpack("<I",uc.mem_read(sp+0x3C,4))[0]
    hs=struct.unpack("<hh",uc.mem_read(sp+0x14,4));hg=struct.unpack("<hh",uc.mem_read(sp+0x10,4))
    query_dummy=struct.unpack("<hh",uc.mem_read(DUMMY+0x24,4))
    # Type/mover virtual predicates and Infantry Jumpjet precheck-row override
    # are outside this comparison. Supply only their final allow predicate.
    run_checked(uc,0x42CAEE if case["allow"] else 0x42CB1D,0x42CB22,count=10000)
    return dict(**case,labels=labels,hierarchy_start=hs,hierarchy_goal=hg,
        query_dummy=query_dummy,dummy_coord=struct.unpack("<hh",uc.mem_read(DUMMY+0x24,4)),
        endpoints_inside=bool(uc.mem_read(sp+0x4C,1)[0]))

if __name__=="__main__":
    finish_vectors(lambda:[execute(c) for c in cases()],Path(__file__).with_suffix(".json"),
        provenance=lambda:provenance(scope="Original42C927..42CA43 lookups,queries,retained projections then conditional mode1 endpoints",
            assumptions=["Explicit movement row0, supplied mover onBridge and final allowHS predicate",
                "No typevirtual predicates, Team waypoint authority, Infantry precheck row override, cell AStar or retry loop",
                "Supplied real-cell/record/raw-row state; constructor sentinel(0,0) from5618B0",
                "Terminating projections with valid fallback exits; record[-1]/cyclic process domains excluded"],
            substitutions=[],entry_points={"prepare":0x42C927,"prepared_stop":0x42CA43,"allowed_membership":0x42CAEE,"denied_membership":0x42CB1D,"gate_stop":0x42CB22}))
