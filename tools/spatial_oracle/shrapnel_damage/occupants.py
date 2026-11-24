"""Original repaired Shrapnel wooden damage, MTNK occupants and Cell Detach.

The shared Occupied owner executes all actor/Drive/admission/damage/lifetime
bodies. This adapter supplies physical scene coordinates and observes original
low-controller/list ordering. Native Unlimbo/orders/retained-head production,
outer damage admission, complete effect playback and navigation are excluded.
"""
from pathlib import Path
import json,sys
from unicorn.x86_const import UC_X86_REG_EAX,UC_X86_REG_EBP,UC_X86_REG_ESI,UC_X86_REG_EIP
from tools.native_oracle import provenance,_canonical
from tools.spatial_oracle.anytown_damage.anytown_occupants import Occupied,sha,signed
from tools.spatial_oracle.anytown_damage.publication import publication_projection
from tools.spatial_oracle.shrapnel_repair.shrapnel_repair import (
    Rules,theater,input_case,ResidentRepair,ASSETS)
from tools.spatial_oracle.shrapnel_repair import packet_io

HERE=Path(__file__).resolve().parent
OUTPUT=HERE/'occupants.json.gz'
PROMOTION=HERE/'occupants_promotion.json'
SCENE=dict(impact_cell=[115,59],ground_z=208,map_path=ASSETS/'XShrapnel.MAP',
    area_entry=0x48A25A,area_stop=0x48A26A,
    span=[(x,y) for y in range(54,65) for x in range(114,117)])


class WoodenOccupied(Occupied):
    def hook(self,u,pc,n,data):
        if self.phase=='measure':
            if pc==0x47DD70:
                raise AssertionError('ordinary wooden controller reached structural fallout')
            if pc in (0x57BAA0,0x57C2B0,0x57E2A0,0x57B990,0x57C870):
                self.add('wooden_controller',pc=hex(pc))
            if pc==0x48A25F:
                self.add('driver_return',al=u.reg_read(UC_X86_REG_EAX)&255)
            if pc==0x487A30:
                p=u.reg_read(UC_X86_REG_ESI)
                self.add('resident_cached_next',actor=self.who(p),
                    next=self.who(u.reg_read(UC_X86_REG_EBP)))
            if pc==0x487BD6:
                p=u.reg_read(UC_X86_REG_ESI)
                self.add('neighbor_live_next',actor=self.who(p),
                    next=self.who(self.read32(p+0x30)),health=signed(self.read32(p+0x6C)))
        super().hook(u,pc,n,data)

    def write_hook(self,u,access,address,size,value,data):
        super().write_hook(u,access,address,size,value,data)
        if self.phase!='measure':
            return
        for coord,p in self.cells.items():
            if address==p+0x44:
                self.add('bridge_overlay_write',coord=list(coord),overlay=signed(value),pc=hex(u.reg_read(UC_X86_REG_EIP)))
                break
            if address==p+0xEC:
                self.add('cell_land_write',coord=list(coord),land=signed(value))
                break


def generate():
    r=Rules();t=theater();case=input_case([117,56],t)
    donor=ResidentRepair(case,r,t);repair=donor.run()
    repaired=[donor.snapshot(donor.ptrs[x,y]) for y in range(54,65) for x in range(114,117)]
    assert [donor.snapshot(donor.ptrs[115,y])['overlay'] for y in (58,59,60)]==[84,84,85]
    rows=[]
    for c in [
        dict(name='resident_center',current_cell=[115,59]),
        dict(name='north_head_center',current_cell=[115,58],head_cell=[115,59]),
        dict(name='north_stationary',current_cell=[115,58],target_impact=True),
        dict(name='north_restore_replacement',current_cell=[115,58],target_impact=True,restore_target=[115,57]),
        dict(name='north_restore_same',current_cell=[115,58],target_impact=True,restore_target=[115,59]),
        dict(name='resident_two_members',current_cell=[115,59],second_member=True),
        dict(name='north_two_heads',current_cell=[115,58],head_cell=[115,59],second_member=True),
    ]:
        print('native occupants',c['name'],flush=True)
        m=WoodenOccupied(donor,t,c,scene=SCENE)
        result=m.run()
        rows.append(dict(input=c,initializers=m.initializers,native_inputs=m.inputs,
            extra_layers=m.extra_layers,result=result))
    return dict(schema=1,native_inputs=r.snapshot(),theater=t,input=case,
        initial_recalc=donor.initial_recalc,repair_prefix=repair,repaired_healthy=repaired,
        assets=donor.assets,base_reader_layers=m.layers,sound=m.sound_input,
        scene={k:(v.name if isinstance(v,Path) else v)for k,v in SCENE.items()},
        text_section_sha256=m.code_hash,cases=rows)


def metadata():
    return provenance(scope=__doc__,assumptions=[
        'Physical XShrapnel MAP/SNOW TMP and original overlay/theater/land readers come from the existing Shrapnel ResidentRepair owner. Actual570050/57F200/57FBC0 repair and all nine47D2B0 calls establish the retained healthy input; initial crop and zone storage are explicit supplied boundaries.',
        'Measured original area-low continuation48A25A..48A26A calls actual57BAA0/57C2B0/57E2A0,487A10 and conditional70D4A0. Entry is after the admitted Scenario strength roll; no projectile/fire/outer damage gate is claimed.',
        'Shared anytown_occupants.Occupied executes unchanged native MTNK/Drive type/construction, path admission, C4/Super receiver, death cleanup, list removal and target restoration bodies. Actor/House/world membership, mission, frame1000 and retained incoming head are supplied active-state projections.',
        'Physical layered RULESMD, absentLANGRULE, MPBattleMD and XShrapnel readers execute. The sound registry contains GenVehicleDie at relative index0; original sound reader executes, sample IO is supplied.',
        'Main,Scenario,MapGen are original65C6D0 seeded0 after setup. Full raw states bracket both visits. Original RNG calls/results, timers, detach, cached resident Next and live neighbor Next are observed. Native .text checksum must remain unchanged.',
    ],substitutions=[
        'Shared successful bounded heap/CRT/OS seams and signed-CRC INI lexical caches; no gameplay callback return is substituted within admission, receiver or cleanup.',
        'Animation421EA0 and sound7509E0 are request boundaries; spawned effect playback/lifetime and their later RNG remain excluded.',
        'Connectivity56C510/hierarchy586990 and display/radar extents are recorded return boundaries. No native whole scenario load or production actor admission/order/head creation is claimed.',
    ],entry_points=dict(area_low_continuation=0x48A25A,driver=0x57BAA0,root=0x57C2B0,
        sibling=0x57E2A0,recalc=0x47D2B0,occupants=0x487A10,unit_can_enter=0x73F0A0,
        drive_at_coord=0x4B4920,unit_damage=0x737C90,detach=0x70D4A0))


if __name__=='__main__':
    result=generate()
    if '--write' in sys.argv and not PROMOTION.exists():
        raw=_canonical(json.loads(_canonical(publication_projection(result))))
        PROMOTION.write_text(json.dumps(dict(results={OUTPUT.name.removesuffix('.gz'):
            dict(published_payload_sha256=sha(raw))}),indent=2)+'\n')
    packet_io.finish_vectors(result,OUTPUT,provenance=metadata,
        promotion_path=PROMOTION,projection=publication_projection)
