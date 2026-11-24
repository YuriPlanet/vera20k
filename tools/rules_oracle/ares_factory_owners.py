"""Bounded installed Ares FactoryOwners qualification, limit and global controls.

Set VERA20K_ARES_DLL and VERA20K_GAMEMD_EXE to local installed binaries.
python -m tools.rules_oracle.ares_factory_owners --check
Inputs are synthetic retained vectors and live membership, not mod resources.
Native suffix statuses3/2 are not whole CanBuild Yes/No results.
Separate skip-tech tail controls supply counter priors; global reads are lexical.
Human/TechLevel branch admission is a separate bounded prefix component.
"""

import argparse
import hashlib
import os
from pathlib import Path

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_ESI

from tools import native_oracle as native
from tools.native_oracle import finish_vectors, provenance
from tools.rules_oracle.ares_prerequisites import ARES_SHA256, identified_bytes
from tools.rules_oracle.ares_country_veterancy import CountryFixture


def qualification_controls(ares):
    game = native.image_bytes()
    p = CountryFixture((ares, game))
    u = p.u
    house, house_ext = p.alloc(0x17000), p.alloc(0x100)
    target, target_ext = p.alloc(0x1800), p.alloc(0x600)
    factory, factory_ext = p.alloc(0x1000), p.alloc(0x200)
    factory_type, factory_type_ext = p.alloc(0x1800), p.alloc(0x600)
    countries = {name: p.alloc(0x200) for name in ('CountryA', 'CountryB')}
    for name, ptr in countries.items():
        u.mem_write(ptr+0x24, name.encode()+b'\0')
    for ptr, size in ((house, 0x17000), (house_ext, 0x100), (target, 0x1800),
                      (target_ext, 0x600), (factory, 0x1000), (factory_ext, 0x200),
                      (factory_type, 0x1800), (factory_type_ext, 0x600)):
        u.mem_write(ptr, bytes(size))
    p.put(house+0x16084, house_ext)
    p.put(house_ext, house)
    p.put(house+0x1D4, 10)
    p.put(target, 0x7E4570)  # actual BuildingType vtable, including WhatAmI
    p.put(target+0x2FC, target_ext)
    p.put(target+0x634, 1)
    p.put(target+0x16F0, -1)
    p.put(factory+0x154, factory_ext)
    p.put(factory+0x21C, house)
    p.put(factory+0x520, factory_type)
    p.put(factory_type+0x2FC, factory_type_ext)
    p.put(0xA8B238, 1)
    u.mem_write(0xA8B263, b'\1')

    visits = []
    watch = {0x100221A0, 0x100221E8, 0x1002221C, 0x10022220,
             0x100222B0, 0x100222F9, 0x1002230B, 0x10022325,
             0x10022255}

    def observe(_u, pc, _size, _data):
        if pc in watch:
            visits.append(f'{pc:08X}')

    u.hook_add(UC_HOOK_CODE, observe)
    cases = [
        dict(name='empty_restrictions_no_factory', allowed=[], forbidden=[], plans=[], factories=[]),
        dict(name='matching_owned_construction_factory', allowed=['CountryA'], forbidden=[], plans=[], factories=['CountryA'], factory_rtti=7),
        dict(name='wrong_factory_country', allowed=['CountryB'], forbidden=[], plans=[], factories=['CountryA'], factory_rtti=7),
        dict(name='has_all_plans_wrong_factory_rtti', allowed=['CountryA'], forbidden=[], plans=[], factories=['CountryA'], factory_rtti=16, has_all_plans=True),
        dict(name='no_all_plans_wrong_factory_rtti', allowed=['CountryA'], forbidden=[], plans=[], factories=['CountryA'], factory_rtti=16),
        dict(name='forbidden_also_allowed_rejects_same_candidate', allowed=['CountryA'], forbidden=['CountryA'], plans=[], factories=['CountryA'], factory_rtti=7, has_all_plans=True),
        dict(name='permanent_plan_without_live_factory', allowed=['CountryA'], forbidden=[], plans=['CountryA'], factories=[]),
        dict(name='lost_live_factory_without_permanent_plan', allowed=['CountryA'], forbidden=[], plans=[], factories=[]),
        dict(name='forbidden_only_null_original_country', allowed=[], forbidden=['CountryA'], plans=[], factories=[None], factory_rtti=7),
        dict(name='nonempty_allowed_null_original_country', allowed=['CountryA'], forbidden=[], plans=[], factories=[None], factory_rtti=7),
    ]
    rows = []
    for values in cases:
        # These are declared std::vector priors; no vector decision is answered.
        for at, names in ((target_ext+0x308, values['allowed']),
                          (target_ext+0x314, values['forbidden']),
                          (house_ext+0x4C, values['plans'])):
            backing = p.alloc(32)
            p.put(at, backing, backing+4*len(names), backing+32)
            p.put(backing, *(countries[name] for name in names))
        live = []
        if values['factories']:
            # A live Building remains present when its OriginalCountry is NULL.
            original_country = values['factories'][0]
            p.put(factory_ext+0x40, 0 if original_country is None else countries[original_country])
            live.append(factory)
        p.vector(house+0x68, live)  # existing DynamicVector input owner
        p.put(factory_type+0xEB8, values.get('factory_rtti', 0))
        u.mem_write(factory_type_ext+0x321, bytes([values.get('has_all_plans', False)]))
        sp = 0x2000F000  # existing Probe call arena
        u.mem_write(sp, bytes(0x80))
        p.put(sp+0x18, house)
        p.put(sp+0x1C, 0x30000000, target)
        u.reg_write(UC_X86_REG_ESP, sp)
        u.reg_write(UC_X86_REG_ESI, target)
        first = len(visits)
        native.run_checked(u, 0x100221A0, 0x30000000,
                           count=100_000, timeout_us=2_000_000)
        p.check_code()
        rows.append(dict(input=values, qualification_status=u.reg_read(UC_X86_REG_EAX),
                         reached=visits[first:]))

    result = dict(
        schema_version=1,
        evidence='Executed original installed Ares qualification suffix with admitted earlier gates; not whole CanBuild or owner lifecycle parity.',
        binaries={'Ares.dll': dict(bytes=len(ares), sha256=hashlib.sha256(ares).hexdigest()),
                  'gamemd.exe': dict(bytes=len(game))},
        owner='tools.rules_oracle.ares_country_veterancy.CountryFixture (existing Probe VM/image/arena/return services)',
        entry='100221A0', stop='30000000 (original ret4)',
        declared_priors=dict(earlier_qualification_gates='already passed',
            target='BuildingType, actual vtable7E4570, TechLevel1, SuperWeapon=-1',
            house='TechLevel10, GameMode1, superweapons enabled',
            lists='Retained Country pointers and currently owned Building list are supplied inputs; no rules/constructor/transfer result is supplied as a golden.'),
        rows=rows,
        bounds=['Country name/vector readers, whole Requirements prefix, full CanBuild limits/prerequisites and FindFactory are not executed.',
                'Owner transfer/loss is represented by explicit current membership; actual capture/death/limbo/snapshot transitions remain instruction-level dependencies.',
                'Existing CountryFixture forbids monitored RNG65C780/65C7E0/65C6D0 and detach7258D0; no such call occurred in these slices.',
                'No timers or playback/rendering/assets are involved; native code bytes remained unchanged.'])
    return result


def human_tech_level_controls(ares):
    """Admitted prefix frame; execute the actual human helper and -1 gate only."""
    p = CountryFixture((ares, native.image_bytes()))
    u = p.u
    house, target = p.alloc(0x17000), p.alloc(0x1800)
    visits = []
    stops = {0x10022118, 0x10022325}

    def observe(_u, pc, _size, _data):
        if pc in (0x100220F8, 0x10020AD0, 0x10020ADF, 0x1002210B) or pc in stops:
            visits.append(f'{pc:08X}')

    u.hook_add(UC_HOOK_CODE, observe)
    inputs = [
        dict(name='human_minus_one', session_mode=1, human=True, player_control=False, tech_level=-1),
        dict(name='ai_minus_one', session_mode=1, human=False, player_control=False, tech_level=-1),
        dict(name='nonzero_mode_control_only_minus_one', session_mode=1, human=False, player_control=True, tech_level=-1),
        dict(name='zero_mode_control_only_minus_one', session_mode=0, human=False, player_control=True, tech_level=-1),
        dict(name='human_zero', session_mode=1, human=True, player_control=False, tech_level=0),
        dict(name='human_minus_two', session_mode=1, human=True, player_control=False, tech_level=-2),
    ]
    rows = []
    for values in inputs:
        p.put(0xA8B238, values['session_mode'])
        u.mem_write(house+0x1EC, bytes([values['human'], values['player_control']]))
        p.put(target+0x634, values['tech_level'])
        sp = 0x2000F000
        u.mem_write(sp, bytes(0x80))
        p.put(sp+0x18, house)
        p.put(sp+0x20, target)
        u.reg_write(UC_X86_REG_ESP, sp)
        first = len(visits)
        stop = native.run_checked(u, 0x100220F8, tuple(sorted(stops)),
                                  count=10_000, timeout_us=2_000_000,
                                  required_addresses=(0x10020AD0,), context=values)
        p.check_code()
        reached = visits[first:]
        if reached[-1] != f'{stop:08X}':
            reached.append(f'{stop:08X}')
        assert reached[-1] in ('10022118', '10022325'), reached
        rows.append(dict(input=values, admitted=reached[-1] == '10022118',
                         stop=reached[-1], reached=reached))
    return rows


def build_limit_controls(ares):
    """Skip-tech tail with supplied boundary counters and actual held-object leaves."""
    images = ares, native.image_bytes()
    p=CountryFixture(images);u=p.u
    house=p.alloc(0x17000);target=p.alloc(0x1800);other_type=p.alloc(0x1800)
    held=p.alloc(0x900);factory=p.alloc(0x100);factory_array=p.alloc(0x10)
    owned_data=p.alloc(0x10);produced_data=p.alloc(0x10)
    p.put(house+0x1EC,1);p.put(0xA8B238,1)
    p.put(target,0x7EB610);p.put(target+0xDF8,0)
    p.put(other_type,0x7EB610);p.put(other_type+0xDF8,1)
    p.put(held,0x7EB058);p.put(held+0x6C0,target)
    p.put(factory+0x6C,house);p.put(factory+0x58,held)
    p.put(factory_array,factory);p.put(0xA83E34,factory_array)
    # Execute original Counter constructors. Install sized, supplied backing arrays.
    # Correct Counter base+55C8; +55CC is only its backing-array field.
    # The earlier private misbound-counter results are excluded.
    for offset,data in ((0x5528,owned_data),(0x55C8,produced_data)):
     p.call(0x49F9B0,ecx=house+offset)
     p.put(house+offset+4,data,4)
    visited=[]
    def observe(_u,pc,_size,_data):
     if pc in (0x10021B00,0x10021BEB,0x10020AF0,0x10003220,0x100212F0,0x4F83C0,0x4CA160,0x49F9B0):visited.append(f'{pc:08X}')
    u.hook_add(UC_HOOK_CODE,observe)
    inputs=[
     dict(name='int_min_produced_zero',limit=-2147483648,owned=0,produced=0,held=False,include_queued=False),
     dict(name='int_min_produced_one',limit=-2147483648,owned=0,produced=1,held=False,include_queued=False),
     dict(name='int_min_produced_int_max',limit=-2147483648,owned=0,produced=2147483647,held=False,include_queued=False),
     dict(name='int_min_produced_int_min',limit=-2147483648,owned=0,produced=-2147483648,held=False,include_queued=False),
     dict(name='zero_limit_owned_zero',limit=0,owned=0,produced=0,held=False,include_queued=False),
     dict(name='zero_limit_owned_one',limit=0,owned=1,produced=0,held=False,include_queued=True),
     dict(name='negative_one_produced_zero',limit=-1,owned=0,produced=0,held=False,include_queued=False),
     dict(name='negative_one_produced_one',limit=-1,owned=0,produced=1,held=False,include_queued=False),
     dict(name='infantry_over_positive_limit_held_counted',limit=2,owned=3,produced=3,held=True,include_queued=True),
     dict(name='infantry_over_positive_limit_held_ignored',limit=2,owned=3,produced=3,held=True,include_queued=False),
     dict(name='infantry_at_positive_limit_held_counted',limit=2,owned=2,produced=2,held=True,include_queued=True),
     dict(name='infantry_over_positive_limit_wrong_held_type',limit=2,owned=3,produced=3,held=True,wrong_held_type=True,include_queued=True),
    ]
    rows=[]
    for v in inputs:
     p.put(target+0x3B8,v['limit']);p.put(owned_data,v['owned']);p.put(produced_data,v['produced'])
     p.put(0xA83E40,1 if v['held'] else 0)
     p.put(held+0x6C0,other_type if v.get('wrong_held_type') else target)
     start=len(visited)
     answer=p.call(0x10021B00,(house,target,1,int(v['include_queued'])))
     if answer>=0x80000000:answer-=0x100000000
     rows.append(dict(input=dict(v,family='infantry',type_index=0,human_controlled=True,session_mode=1,skip_tech=True),can_build=answer,reached=visited[start:]))
    return rows


def global_controls(ares):
    """Actual ctor and retained phase2 reads, with the existing lexical transport."""
    images = ares, native.image_bytes()
    p=CountryFixture(images);u=p.u
    ext=p.alloc(0x180);ini=p.alloc(0x100)
    p.call(0x1002A360,(0,),ext)
    ctor=dict(phase=p.read(ext+4),allow_bypass_by_difficulty=list(bytes(u.mem_read(ext+0x14A,3))))
    # Skip unrelated phase0 initializers. Entry really reads current INI in phase2.
    p.put(ext+4,2);current=None;events=[]
    def service(_u,pc,_size,_data):
     if pc==0x528A10:
      sp=u.reg_read(UC_X86_REG_ESP)
      section,key,default,target,capacity=[p.read(sp+4+i*4) for i in range(5)]
      name=p.string(key);value=current if name=='AllowBypassBuildLimit' else None
      raw=(value or '').encode('ascii')[:capacity-1];u.mem_write(target,raw+b'\0')
      events.append(dict(section=p.string(section),key=name,default=p.string(default),capacity=capacity,value=value))
      p.service_return(20,len(raw))
    u.hook_add(UC_HOOK_CODE,service)
    rows=[]
    for name,value in [('omitted',None),('one_yes','yes'),('two_bools','no,yes'),('three_bools','yes,no,yes'),('first_invalid','bad,yes,no'),('partial_valid_invalid','no,bad,yes'),('empty',''),('extra_ignored','no,yes,no,yes')]:
     before=list(bytes(u.mem_read(ext+0x14A,3)));start=len(events);current=value
     p.call(0x1002B370,(ini,),ext)
     rows.append(dict(name=name,supplied_ascii=value,before=before,after=list(bytes(u.mem_read(ext+0x14A,3))),phase_after=p.read(ext+4),read_events=events[start:]))
    p.check_code()
    return dict(initial=ctor, rows=rows)


def owned_count_controls(ares):
    images = (ares, native.image_bytes())
    p=CountryFixture(images);u=p.u
    house=p.alloc(0x17000);building=p.alloc(0x1800);unit=p.alloc(0x1800);upgrade=p.alloc(0x1800);host=p.alloc(0x1800)
    bc=p.alloc(0x10);uc=p.alloc(0x10);b1=p.alloc(0x800);b2=p.alloc(0x800);array=p.alloc(0x10)
    for target,vt in ((building,0x7E4570),(unit,0x7F6218),(upgrade,0x7E4570),(host,0x7E4570)):
     p.put(target,vt);p.put(target+0xDF8,0)
    for offset,data in ((0x5500,bc),(0x5514,uc)):
     p.call(0x49F9B0,ecx=house+offset);p.put(house+offset+4,data,4)
    visits=[]
    def observe(_u,pc,_size,_data):
     if pc in (0x10020AF0,0x10020B6C,0x10007E50,0x10020B90,0x10020BA7):visits.append(f'{pc:08X}')
    u.hook_add(UC_HOOK_CODE,observe)
    p.put(bc,2147483647);p.put(uc,1);p.put(building+0x408,unit)
    raw=p.call(0x10020AF0,(house,building));signed=raw if raw<0x80000000 else raw-0x100000000
    rows=[dict(name='building_undeploy_counter_wrap',input=dict(building_tracked=2147483647,unit_tracked=1,building_undeploys_into_unit=True,powers_up_building='',type_indices=0),count_owned=signed,raw=raw,reached=list(visits))]
    p.u.mem_write(upgrade+0xE88,b'HOST_A\0');p.u.mem_write(host+0x24,b'HOST_A\0')
    p.put(array,host);p.put(0xA83C6C,array);p.put(0xA83C78,1)
    p.put(b1+0x520,host);p.put(b2+0x520,host)
    p.put(b1+0x5EC,upgrade,0,upgrade);p.put(b2+0x5EC,0,upgrade,0)
    p.vector(house+0x68,[b1,b2]);p.put(bc,777)
    start=len(visits);raw=p.call(0x10020AF0,(house,upgrade))
    rows.append(dict(name='upgrade_counts_all_matching_host_slots',input=dict(upgrade_tracked_same_index=777,powers_up_building='HOST_A',registered_building_ids=['HOST_A'],host_types=['HOST_A','HOST_A'],slots=[['UPGRADE',None,'UPGRADE'],[None,'UPGRADE',None]],active_counts='All zero; no Active host-count gate in this count path'),count_owned=raw,reached=visits[start:]))
    p.check_code()
    return rows


def generate(ares):
    result = qualification_controls(ares)
    result["build_limit_rows"] = build_limit_controls(ares)
    result["global_controls"] = global_controls(ares)
    result["owned_count_rows"] = owned_count_controls(ares)
    result["human_tech_level_rows"] = human_tech_level_controls(ares)
    return result


def metadata():
    return provenance(
        scope="Ten installed Ares FactoryOwners qualification suffix controls at100221A0 (statuses3/2), twelve human skip-tech CanBuild10021B00 build-limit tail controls (answers0/1/-1), eight retained GlobalControls reads, two CountOwned conversion/installed-upgrade controls, and six human-predicate/TechLevel=-1 branch controls; not complete ordinary CanBuild or production history.",
        assumptions=[
            "Earlier Requirements gates are declared passed; supplied target uses actual BuildingType vtable7E4570, TechLevel1 and SuperWeapon=-1. House TechLevel10, GameMode1 and enabled superweapons are declared inputs.",
            "Retained allowed/forbidden Country pointer vectors, House permanent plans and current owned Building membership are synthetic priors. Country constructors, INI/list readers, parent binding and producer/loss transitions are not executed.",
            "The actual native candidate traversal, same-candidate allowed/forbidden tests, permanent-plan path, live Factory RTTI/HasAllPlans checks and remaining admitted suffix execute. Actual target WhatAmI runs from the original vtable.",
            "Two qualification controls retain a live Building with NULL OriginalCountry, separately with forbidden-only and nonempty allowed restrictions. NULL is an explicit supplied pointer prior, not a constructor/lifecycle result.",
            "Human/TechLevel controls enter100220F8 after earlier gates have passed and execute actual10020AD0 with supplied GameMode and House bytes+1EC/+1ED. They stop before10022118 (branch admitted) or10022325 (branch rejected); admitted is not a full Requirements or CanBuild result.",
            "CountOwned controls execute10020AF0 and the real Building Find with supplied signed counters/current host list/all three upgrade slots. Conversion lifecycle, upgrade installation, VehicleThief state and generated production histories are not executed.",
            "Build-limit controls supply signed owned/produced counters and held-object membership as boundary priors, not gameplay histories; skip-tech=true and human=true bypass qualification, rich FindFactory and AI global controls.",
            "Build-limit controls execute Counter49F9B0 construction, CountProduced10003220, CountOwned10020AF0, held-object100212F0/4F83C0/4CA160 and actual InfantryType/object leaves. Produced Counter base is House+55C8, backing+55CC. Incorrect prior misbound-counter results are excluded.",
            "GlobalControls executes constructor1002A360 then eight retained phase2 reads1002B370 with actual tokenizer1002A180 and bool10076CC0; only ReadString528A10 lexical transport is supplied. Reverse successful prefix writes difficulty slots2/1/0; phase0 unrelated registry initializers and installed Process caller are not executed.",
            "CountryFixture rejects monitored RNG65C6D0/65C780/65C7E0 and detach7258D0; selected Ares code bytes are checked unchanged. Complete ordinary CanBuild, prerequisites, FindFactory, allocation failure, saves, UI and production output parity are excluded.",
        ], substitutions=[
            "Existing ares_country_veterancy.CountryFixture owns the Probe VM, image mapping, heap, return arena and code checks; no new VM, parser or queue is introduced.",
            "Supplied heap object shells, preallocated std::vector backing, existing DynamicVector input helper and synthetic entry ABI after earlier admission. Existing Probe allocation/free services remain inherited boundaries.",
            "Four-entry counter backing attaches after native construction; Factory owner/held fields and global Factory array are supplied. Count/type/get-held decisions execute without answer stubs.",
            "GlobalControls ReadString supplies ASCII values with the actual requested default/capacity; the native parser, constructor and retained writes execute unchanged. Other GlobalControls keys are omitted.",
        ], entry_points=dict(qualification_suffix=0x100221A0,
            permanent_candidates=0x100221E8, live_candidates=0x100222B0,
            live_factory_rtti=0x100222F9, live_has_all_plans=0x1002230B,
            refused_tail=0x10022325, can_build=0x10021B00,
            build_limit_tail=0x10021BEB, count_produced=0x10003220,
            count_owned=0x10020AF0, held_type_count=0x100212F0,
            factory_scan=0x004F83C0, get_held_object=0x004CA160,
            counter_constructor=0x0049F9B0, rules_ext_constructor=0x1002A360,
            global_read=0x1002B370, global_bool_prefix=0x1002A180,
            ares_bool=0x10076CC0, human_tech_gate=0x100220F8,
            human_predicate=0x10020AD0, human_tech_admitted=0x10022118))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ares-dll", type=Path, default=os.environ.get("VERA20K_ARES_DLL"))
    parser.add_argument("--gamemd-exe", type=Path)
    args, remaining = parser.parse_known_args()
    if args.ares_dll is None:
        parser.error("Set VERA20K_ARES_DLL or supply --ares-dll")
    if args.gamemd_exe:
        os.environ["VERA20K_GAMEMD_EXE"] = str(args.gamemd_exe)
    ares = identified_bytes(args.ares_dll, ARES_SHA256)
    finish_vectors(lambda: generate(ares), Path(__file__).with_suffix(".json"),
        provenance=metadata, argv=remaining,
        source_paths={"producer": Path(__file__), "runner": Path(native.__file__),
            "vm_owner": Path(__file__).with_name("ares_prerequisites.py"),
            "fixture_owner": Path(__file__).with_name("ares_country_veterancy.py")})


if __name__ == "__main__":
    main()
