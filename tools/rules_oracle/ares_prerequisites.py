"""Bounded controls executing Ares 3.0 prerequisite parser/check bodies.

Not a game oracle or a proof of full DLL startup/INI byte parsing/CanBuild.
ReadString inputs, existing type registries, ownership and CRT/OS leaves are
explicit fixture boundaries. All Ares tokenization, lookup priority, retained
list writes, group fallbacks and all/any loops execute the original DLL bytes.
"""
from pathlib import Path
import json
import struct
import argparse
import hashlib
from tools.native_oracle import (
    IMAGE_BASE, IMAGE_SIZE, OracleError, map_pe32_image, run_checked,
)

ARES_SHA256 = "93ce890f387a6c1b3240a2dd7b674f25fec5a0e96f0b0d0306796b2edf7b5b83"
FIXTURE = Path(__file__).with_suffix(".json")
# Inclusive entry, exclusive final instruction boundary. The additional caller
# and install spans are read-only evidence, not executed by component controls.
REGIONS = {
    'HouseOwnsAll': (0x10008110, 0x100081d6),
    'HouseOwnsAny': (0x100081e0, 0x100082a8),
    'HouseOwnsSpecific': (0x100082b0, 0x10008362),
    'FindBuildingType': (0x10007e50, 0x10007e97),
    'CanBuild': (0x10021b00, 0x10021c49),
    'Qualification': (0x10021ff0, 0x10022356),
    'SupportingUpgradeUnlimboArm': (0x440871, 0x440a32),
    'SupportingTechnoActiveCounterCall': (0x6f6d85, 0x6f6d94),
    'IsGenericPrerequisite': (0x1003dd00, 0x1003dd60),
    'AircraftDestructorPrereqs': (0x1004e750, 0x1004e77e),
    'FootPutPrereqs': (0x1004e780, 0x1004e7b2),
    'InfantryDestructorPrereqs': (0x1004e7c0, 0x1004e7ee),
    'ChangeOwnershipPrereqs': (0x1004e7f0, 0x1004e83e),
    'DiscoveredByPrereqs': (0x1004e840, 0x1004e882),
    'UnitDestructorPrereqs': (0x1004e890, 0x1004e8be),
    'TypeExtConstructorCachePrefix': (0x1003c710, 0x1003c7ae),
    'LoadTypeExtCachePrefix': (0x1003a370, 0x1003a3f8),
    'SupportingUnitDestructorPrelude': (0x7357bb, 0x73581b),
    'SupportingInfantryDestructorPrelude': (0x517db7, 0x517e33),
    'SupportingAircraftDestructorPrelude': (0x4140b7, 0x414110),
    'SupportingFootPutPreludeAndDiscovery': (0x4d718c, 0x4d7235),
    'SupportingDiscovery': (0x6f4960, 0x6f4a3f),
    'SupportingChangeOwnerTracking': (0x7015d8, 0x7015f2),
}
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP


class Probe:
    def __init__(self):
        ares_bytes, gamemd_bytes = self.input_bytes
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        map_pe32_image(self.u, ares_bytes, image_base=0x10000000,
                       image_size=0xE0000, header_size=0x400)
        map_pe32_image(self.u, gamemd_bytes, image_base=IMAGE_BASE,
                       image_size=IMAGE_SIZE, header_size=0x1000)
        self.u.mem_map(0x20000000, 0x200000)
        self.u.mem_map(0x30000000, 0x1000)
        self.heap = 0x20010000
        self.events = []
        self.reads = {}
        self.owned = {}
        self.type_kinds = {}
        self.groups = []
        self.buildings = []
        self.types = []
        self.crt_thread = None
        self.specific_visits = set()
        self.u.hook_add(UC_HOOK_CODE, self.hook)

    def enable_crt_thread(self):
        """Opt in to an Ares CRT C-locale thread block, not a numeric parser.

        Same boundary as ini_token_readers.Crt for the retail CRT. This image's
        accessor 10085F5C would require Windows GetLastError/TLS initialization.
        Its clients (including the actual sscanf scanner) still execute intact;
        default prerequisite probes keep their previous transport unchanged.
        """
        if self.crt_thread is None:
            self.crt_thread = self.alloc(0x1000)
            # Original CRT's default C-locale initializer; thread locale fields
            # match its constructor10085D5E/locale owner. No token result is set.
            self.write(self.crt_thread + 0x4c, 0x100c02a8)
            self.write(self.crt_thread + 0x48, 0x100c0450)
            self.write(self.crt_thread + 0x350, 1)
            Probe.call(self, 0x10085839)
        return self.crt_thread

    def alloc(self, n=256):
        at = self.heap
        self.heap += (n+15) & ~15
        return at

    def read(self, a):
        return struct.unpack('<I', self.u.mem_read(a, 4))[0]

    def write(self, a, v):
        self.u.mem_write(a, struct.pack('<I', v & 0xffffffff))

    def string(self, a):
        result = bytearray()
        for i in range(4096):
            b = bytes(self.u.mem_read(a+i, 1))
            if b == b'\0':
                return result.decode('ascii')
            result.extend(b)
        raise ValueError('Unbounded string')

    def text(self, s):
        at = self.alloc(len(s)+1)
        self.u.mem_write(at, s.encode()+b'\0')
        return at

    def vector(self, at, values):
        data = self.alloc(512)
        self.write(at+4, data)
        self.write(at+8, 128)
        self.write(at+0x10, len(values))
        for i, v in enumerate(values):
            self.write(data+i*4, v)

    def vector_values(self, at):
        return [struct.unpack('<i', self.u.mem_read(self.read(at+4)+i*4, 4))[0]
                for i in range(self.read(at+0x10))]

    def registry(self, groups=('POWER','FACTORY','BARRACKS','RADAR','TECH','PROC','custom','future','COLLIDE'),
                 buildings=('A','B','COLLIDE')):
        self.groups = []
        for name in groups:
            at = self.alloc(0x50)
            self.u.mem_write(at, name.encode()+b'\0')
            self.vector(at+0x20, [])
            self.vector(at+0x38, [])
            self.groups.append(at)
        array = self.alloc(512)
        for i, value in enumerate(self.groups):
            self.write(array+i*4, value)
        for delta, value in ((0,array),(4,array+len(groups)*4),(8,array+512)):
            self.write(0x100c18d4+delta, value)
        self.buildings = []
        for i, name in enumerate(buildings):
            at = self.alloc(0xf00)
            self.u.mem_write(at+0x24, name.encode()+b'\0')
            self.write(at+0xdf8, i)
            self.buildings.append(at)
        array = self.alloc(4*len(buildings))
        for i, value in enumerate(self.buildings):
            self.write(array+i*4, value)
        self.write(0xa83c6c, array)
        self.write(0xa83c78, len(buildings))
        self.types = list(self.buildings)
        for name in ('TANK','INF'):
            at = self.alloc(0xf00)
            self.u.mem_write(at+0x24, name.encode()+b'\0')
            self.types.append(at)
        vtable=self.alloc(0x80)
        self.write(vtable+0x2c,0x30000200)
        for typ,kind in zip(self.types,(7,7,7,40,16)):
            self.write(typ,vtable)
            self.type_kinds[typ]=kind
        array = self.alloc(4*len(self.types))
        for i, value in enumerate(self.types):
            self.write(array+i*4, value)
        self.write(0xa8eb04, array)
        self.write(0xa8eb10, len(self.types))

    def hook(self, u, pc, size, _):
        if pc in (0x100082b0, 0x100082d3, 0x100082ea, 0x1000830f,
                  0x10008320, 0x10008340):
            self.specific_visits.add(pc)
        if pc == 0x30000000:
            u.emu_stop()
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        ret = self.read(sp)
        args = lambda n: [self.read(sp+4+i*4) for i in range(n)]
        cleanup = 0
        if pc == 0x10085f5c and self.crt_thread is not None:
            result = self.crt_thread
        elif pc == 0x528a10:
            section, key, default, target, capacity = args(5)
            pair = (self.string(section), self.string(key))
            value = self.reads.get(pair, '')
            payload = value.encode()[:capacity-1]
            u.mem_write(target, payload+b'\0')
            self.events.append(dict(service='supplied_ReadString', section=pair[0], key=pair[1], value=value))
            result, cleanup = len(payload), 20
        elif pc == 0x7c8d20:
            # CRT leaf only; preserve all Ares/native caller lookup loops.
            a, b = (self.string(x).lower() for x in args(2))
            result = (a>b)-(a<b)
        elif pc == 0x1007f1d7:
            ptr, limit = args(2)
            value = self.string(ptr).lower().encode()+b'\0'
            assert len(value)<=limit
            u.mem_write(ptr, value)
            result = 0
        elif pc == 0x10003a40:
            target, capacity, count, pattern, value = args(5)
            text = (self.string(pattern) % self.string(value)).encode()[:capacity-1]
            u.mem_write(target, text+b'\0')
            result = len(text)
        elif pc == 0x100039a0:
            target, capacity, source, count = args(4)
            payload=self.string(source).encode()[:capacity-1]
            u.mem_write(target,payload+b'\0')
            result,cleanup=0,16
        elif pc == 0x10076f08:
            size,=args(1)
            result=self.alloc(size)
            self.events.append(dict(service='supplied_operator_new',bytes=size))
        elif pc == 0x10076f38:
            target,size=args(2)
            result=0
            self.events.append(dict(service='supplied_operator_delete',bytes=size))
        elif pc == 0x10061960:
            section, key, token, unused = args(4)
            self.events.append(dict(service='suppressed_DebugINIParseFailed', section=self.string(section), key=self.string(key), token=self.string(token)))
            result, cleanup = 0, 16
        elif pc == 0x30000200:
            # Only the virtual type category is supplied. CountOwnedNow's
            # original switch, index bounds and tracked-counter reads execute.
            typ=u.reg_read(UC_X86_REG_ECX)
            result=self.type_kinds[typ]
        else:
            return
        u.reg_write(UC_X86_REG_EAX, result & 0xffffffff)
        u.reg_write(UC_X86_REG_ESP, sp+4+cleanup)
        u.reg_write(UC_X86_REG_EIP, ret)

    def call(self, fn, *args, ecx=0):
        sp = 0x2000f000
        self.u.mem_write(sp, struct.pack('<'+'I'*(len(args)+1), 0x30000000, *[a & 0xffffffff for a in args]))
        self.u.reg_write(UC_X86_REG_ESP, sp)
        self.u.reg_write(UC_X86_REG_ECX, ecx)
        run_checked(self.u, fn, 0x30000000, count=100000,
                    timeout_us=5000000, context={"scope": "Ares prerequisite component"})
        return self.u.reg_read(UC_X86_REG_EAX)


def parse_case(name, value, initial):
    m=Probe();m.registry();vector=m.alloc(24);m.vector(vector,initial)
    if value is not None:
        m.reads[('Unit','Prerequisite')]=value
    m.call(0x100085f0, 0, m.text('Unit'), m.text('Prerequisite'), vector)
    return dict(name=name, supplied_read_result=value, initial=initial,
                result=m.vector_values(vector), events=m.events)


def load_case(name, name_index, reads, initial, initial_alt=(), group_name=None):
    m=Probe();m.registry();group=m.groups[name_index]
    if group_name is not None:
        m.u.mem_write(group,group_name.encode()+b'\0')
    m.vector(group+0x20,initial)
    m.vector(group+0x38,[m.types[i] for i in initial_alt])
    m.reads={tuple(k.split('/')):v for k,v in reads.items()}
    m.call(0x10008470,0,ecx=group)
    alt=[m.string(v+0x24) for v in [x & 0xffffffff for x in m.vector_values(group+0x38)]]
    return dict(name=name, group=m.string(group), reads=reads, initial=initial,
                initial_alternate_names=[m.string(m.types[i]+0x24) for i in initial_alt],
                result=m.vector_values(group+0x20), alternate_names=alt, events=m.events)


def owned_case(name, requirement, members, building_counts, alternates=(), alternate_counts=None, any_=False, alternate_active_counts=None,
               powers_up_host=None, installed_buildings=None):
    m=Probe();m.registry();m.vector(m.groups[6]+0x20,members)
    m.vector(m.groups[6]+0x38,[m.types[i] for i in alternates])
    m.owned={m.types[k]:v for k,v in (alternate_counts or {}).items()}
    house=m.alloc(0x6000);counts=m.alloc(4*len(m.buildings))
    for i,count in enumerate(building_counts):m.write(counts+i*4,count)
    m.write(house+0x5554,counts);m.write(house+0x5558,len(building_counts))
    if powers_up_host is not None:
        m.u.mem_write(m.buildings[1]+0xe88, powers_up_host.encode('ascii')+b'\0')
    if installed_buildings is not None:
        objects = []
        for supplied in installed_buildings:
            building = m.alloc(0x800)
            m.write(building+0x520, m.buildings[supplied['type_index']])
            for slot, index in enumerate(supplied['upgrade_slots']):
                assert slot < 3
                m.write(building+0x5ec+slot*4, 0 if index is None else m.buildings[index])
            m.u.mem_write(building+0x702, bytes((supplied['upgrade_count'] & 255,)))
            objects.append(building)
        m.vector(house+0x68, objects)
    # Ares CountOwnedNow100032F0 uses tracked counters, not the on-map set:
    # Building+5500, Unit+5514, Infantry+5528, Aircraft+553C. Their pointer
    # and capacity members are +4/+8. Actual WhatAmI selects these branches.
    for kind,offset,active in ((7,0x5500,0x5550),(40,0x5514,0x5564),(16,0x5528,0x5578),(1,0x553c,0x558c)):
        typed=[t for t in m.types if m.type_kinds[t]==kind]
        length=max((m.read(t+0xdf8)+1 for t in typed),default=0)
        counter=m.alloc(4*length)
        active_counter=m.alloc(4*length)
        for t in typed:
            index=m.read(t+0xdf8)
            m.write(counter+index*4,m.owned.get(t,0))
            m.write(active_counter+index*4,(alternate_active_counts or {}).get(m.types.index(t),0))
        m.write(house+offset+4,counter);m.write(house+offset+8,length)
        if kind!=7:
            m.write(house+active+4,active_counter);m.write(house+active+8,length)
    req=m.alloc(4*len(requirement))
    for i,v in enumerate(requirement):m.write(req+i*4,v)
    # Actual optimized entry's by-value Iterator is (pointer,count), not DVC.
    result=m.call(0x100081e0 if any_ else 0x10008110,house,req,len(requirement)) & 255
    row=dict(name=name, requirements=requirement, generic_custom_members=members,
                building_counts=building_counts, alternate_names=[m.string(m.types[i]+0x24) for i in alternates],
                alternate_counts={m.string(m.types[k]+0x24):v for k,v in (alternate_counts or {}).items()},
                result=bool(result), events=m.events)
    if alternate_active_counts is not None:
        row['alternate_active_counts']={m.string(m.types[k]+0x24):v for k,v in alternate_active_counts.items()}
    if powers_up_host is not None:
        row['powers_up_host'] = powers_up_host
    if installed_buildings is not None:
        row['installed_buildings'] = installed_buildings
        row['native_specific_visits'] = [hex(pc) for pc in sorted(m.specific_visits)]
    return row


def planned_case(name,requirement,members,planned,alternates=()):
    m=Probe();m.registry();m.vector(m.groups[6]+0x20,members)
    m.vector(m.groups[6]+0x38,[m.types[i] for i in alternates])
    items=m.alloc(4*len(planned));req=m.alloc(4*len(requirement))
    for i,v in enumerate(planned):m.write(items+i*4,m.buildings[v])
    for i,v in enumerate(requirement):m.write(req+i*4,v)
    result=m.call(0x10008370,items,len(planned),req,len(requirement)) & 255
    return dict(name=name,requirements=requirement,generic_custom_members=members,
                planned_building_indices=planned,alternate_names=[m.string(m.types[i]+0x24) for i in alternates],
                result=bool(result),events=m.events)


def clear_defaults_case():
    m=Probe();m.registry();m.vector(m.groups[6]+0x20,[1]);m.vector(m.groups[6]+0x38,[m.types[3]])
    old=[m.string(g) for g in m.groups]
    m.call(0x10004ca0)
    start,end=m.read(0x100c18d4),m.read(0x100c18d8)
    assert start==end
    m.call(0x10007db0)
    new=[m.read(start+i*4) for i in range((m.read(0x100c18d8)-start)//4)]
    return dict(name='clear_discards_custom_and_defaults_rebuilt',initial_names=old,
                post_clear_count=0,result=[m.string(g) for g in new],
                rebuilt_counts=[[m.read(g+0x30),m.read(g+0x48)] for g in new],events=m.events)


def refresh_case(name, hook, category='unit', registered=True):
    """Original exported lifecycle hook with supplied Syringe register storage.

    Real stock object vtables execute GetTechnoType and WhatAmI. No placement,
    death, discovery or ownership transition is synthesized by this control.
    """
    m=Probe();m.registry()
    typ=m.types[{'unit':3,'infantry':4,'building':0}.get(category,3)]
    if category == 'aircraft':
        typ=m.alloc(0xf00)
        m.u.mem_write(typ+0x24,b'AIR\0')
    extension=m.alloc(0x100)
    m.write(extension,typ);m.write(typ+0x2fc,extension)
    m.vector(m.groups[7]+0x38,[typ] if registered else [])
    old_house=m.alloc(0x400);new_house=m.alloc(0x400)
    obj=m.alloc(0x800);frame=m.alloc(0x40)
    # Values come from the supporting original object vtable slots. They are
    # not caller-supplied WhatAmI/GetTechnoType answers.
    vtable,type_offset={'unit':(0x7f5c70,0x6c4),
                       'infantry':(0x7eb058,0x6c0),
                       'aircraft':(0x7e22a4,0x6c4),
                       'building':(0x7e3ebc,0x520)}[category]
    m.write(obj,vtable);m.write(obj+type_offset,typ);m.write(obj+0x21c,old_house)
    m.write(frame+8,obj);m.write(frame+0xc,obj);m.write(frame+0x10,new_house)
    entry={'put':0x1004e780,'aircraft_dtor':0x1004e750,
           'infantry_dtor':0x1004e7c0,'unit_dtor':0x1004e890,
           'change_owner':0x1004e7f0,'discovered':0x1004e840}[hook]
    m.call(entry,frame)
    byte=lambda at: m.u.mem_read(at,1)[0]
    return dict(name=name,refresh_hook=hook,object_category=category,
                registered_alternate=registered,
                old_house_recheck=bool(byte(old_house+0x1fc)),
                new_house_recheck=bool(byte(new_house+0x1fc)),
                membership_cache=bool(byte(extension+0x68)),
                membership_cache_initialized=bool(byte(extension+0x69)),events=m.events)


def controls():
    rows=[parse_case('missing_retains',None,[1]),parse_case('empty_retains','',[1]),
          parse_case('comma_only_clears',',,,',[1]),parse_case('unknown_clears','NOTFOUND',[1]),
          parse_case('building_before_group_name','COLLIDE',[]),
          parse_case('group_lookup_case_insensitive','cUsToM',[]),
          parse_case('groups_can_forward_reference','future',[]),
          parse_case('duplicates_retained','A,A',[]),
          parse_case('internal_space_is_not_comma_delimiter','A, B',[]),
          load_case('general_then_generic_override',6,{'General/PrerequisiteCustom':'A','GenericPrerequisites/custom':'B'},[2]),
          load_case('empty_generic_retains_general',6,{'General/PrerequisiteCustom':'A','GenericPrerequisites/custom':''},[2]),
          load_case('stock_POWER_uses_same_reader',0,{'General/PrerequisitePower':'A','GenericPrerequisites/POWER':'B'},[2]),
          load_case('alternate_techno_allows_units',6,{'General/PrerequisiteCustomAlternate':'TANK,INF,NOTFOUND'},[0]),
          load_case('alternate_empty_retains',6,{'General/PrerequisiteCustomAlternate':''},[0],(3,)),
          load_case('non_letter_group_prefix_is_preserved',6,{'General/Prerequisite1custom':'A'},[2],group_name='1custom'),
          owned_case('generic_OR_member_owned',[-7],[0,1],[0,1,0]),
          owned_case('generic_none_owned',[-7],[0,1],[0,0,0]),
          owned_case('generic_unit_alternate_owned',[-7],[0,1],[0,0,0],(3,),{3:1}),
          owned_case('generic_unit_alternate_owned_while_off_map',[-7],[0,1],[0,0,0],(3,),{3:1},alternate_active_counts={3:0}),
          owned_case('generic_unit_alternate_active_does_not_substitute_tracking',[-7],[0,1],[0,0,0],(3,),{3:0},alternate_active_counts={3:1}),
          owned_case('generic_unit_alternate_negative_count_is_nonzero',[-7],[0,1],[0,0,0],(3,),{3:-1}),
          owned_case('all_generic_and_specific',[-7,2],[0,1],[1,0,0]),
          owned_case('all_empty_true',[],[],[0,0,0]),
          owned_case('any_empty_false',[],[],[0,0,0],any_=True),
          owned_case('invalid_group_false',[-100],[],[0,0,0]),
          planned_case('planned_generic_OR_building',[-7],[0,1],[1]),
          planned_case('planned_generic_missing',[-7],[0,1],[2]),
          planned_case('planned_generic_does_not_use_alternates',[-7],[],[0],(0,)),
          planned_case('planned_All_requires_specific_and_generic',[-7,2],[0,1],[1,2]),
          clear_defaults_case()]
    # These supplied object shells exercise the original specific-building
    # fallback through both All/Any and direct/group callers. Slot0 and slot2,
    # an older matching host, and an ignored count byte are separate controls.
    host = dict(type_index=0, upgrade_slots=[1,None,None], upgrade_count=1)
    empty = dict(type_index=0, upgrade_slots=[None,None,None], upgrade_count=0)
    third = dict(type_index=0, upgrade_slots=[None,None,1], upgrade_count=0)
    wrong_host = dict(type_index=2, upgrade_slots=[1,None,None], upgrade_count=1)
    wrong_slot = dict(type_index=0, upgrade_slots=[2,None,None], upgrade_count=1)
    rows.extend([
        owned_case('direct_negative_building_count_false',[1],[],[0,-1,0]),
        owned_case('group_negative_building_count_false',[-7],[1],[0,-1,0]),
        owned_case('direct_positive_count_precedes_upgrade_host',[1],[],[0,1,0],powers_up_host='MISSING',installed_buildings=[]),
        owned_case('upgrade_direct_installed',[1],[],[1,0,0],powers_up_host='A',installed_buildings=[host]),
        owned_case('upgrade_group_installed_third_slot_ignores_count',[-7],[1],[1,0,0],powers_up_host='A',installed_buildings=[third]),
        owned_case('upgrade_any_direct_installed',[1],[],[1,0,0],any_=True,powers_up_host='A',installed_buildings=[host]),
        owned_case('upgrade_any_group_installed',[-7],[1],[1,0,0],any_=True,powers_up_host='A',installed_buildings=[host]),
        owned_case('upgrade_older_host_match_survives_newer_empty',[1],[],[2,0,0],powers_up_host='A',installed_buildings=[host,empty]),
        owned_case('upgrade_wrong_host_type_refused',[1],[],[1,0,1],powers_up_host='A',installed_buildings=[wrong_host]),
        owned_case('upgrade_unknown_host_refused',[1],[],[1,0,0],powers_up_host='MISSING',installed_buildings=[host]),
        owned_case('upgrade_zero_active_host_refused',[1],[],[0,0,0],powers_up_host='A',installed_buildings=[host]),
        owned_case('upgrade_negative_active_host_refused',[1],[],[-1,0,0],powers_up_host='A',installed_buildings=[host]),
        owned_case('upgrade_empty_slots_refused',[1],[],[1,0,0],powers_up_host='A',installed_buildings=[empty]),
        owned_case('upgrade_other_slot_type_refused',[1],[],[1,0,0],powers_up_host='A',installed_buildings=[wrong_slot]),
        owned_case('upgrade_host_lookup_case_insensitive',[1],[],[1,0,0],powers_up_host='a',installed_buildings=[host]),
        owned_case('upgrade_negative_required_count_still_checks_install',[1],[],[1,-1,0],powers_up_host='A',installed_buildings=[host]),
    ])
    rows.extend([
        refresh_case('alternate_foot_put_rechecks','put'),
        refresh_case('unrelated_foot_put_does_not_recheck','put',registered=False),
        refresh_case('alternate_aircraft_dtor_rechecks','aircraft_dtor','aircraft'),
        refresh_case('alternate_infantry_dtor_rechecks','infantry_dtor','infantry'),
        refresh_case('alternate_unit_dtor_rechecks','unit_dtor'),
        refresh_case('alternate_change_owner_rechecks_both','change_owner'),
        refresh_case('unrelated_change_owner_does_not_recheck','change_owner',registered=False),
        refresh_case('building_change_owner_excluded','change_owner','building'),
        refresh_case('alternate_discovery_rechecks','discovered'),
        refresh_case('unrelated_discovery_does_not_recheck','discovered',registered=False),
        refresh_case('building_discovery_excluded','discovered','building'),
    ])
    return rows


def identified_bytes(path, expected):
    data = path.read_bytes()
    actual = hashlib.sha256(data).hexdigest()
    if actual != expected:
        raise OracleError(f"Unsupported {path.name} SHA-256 {actual}; expected {expected}")
    return data


def disassemble():
    """Print only the authenticated original body/caller bounds, without running."""
    from capstone import Cs, CS_ARCH_X86, CS_MODE_32
    machine = Probe()
    decoder = Cs(CS_ARCH_X86, CS_MODE_32)
    for name, (start, end) in REGIONS.items():
        print(f'{name} {start:#010x}..{end:#010x} (end exclusive)')
        cursor = start
        for instruction in decoder.disasm(bytes(machine.u.mem_read(start, end-start)), start):
            print(f'{instruction.address:#010x} {instruction.mnemonic} {instruction.op_str}')
            cursor = instruction.address + instruction.size
        if cursor != end:
            raise OracleError(f'Incomplete disassembly of {name}: stopped at {cursor:#x}')
    # Original preferred-base .syhks00 records, including the two discovery
    # sites sharing one exported body. No runtime patching is performed.
    for at in range(0x100c3000,0x100c8a80,16):
        name=machine.string(machine.read(at+8))
        if name.endswith('_Prereqs'):
            print(f'Hook {machine.read(at):#010x} length={machine.read(at+4)} {name}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ares-dll", required=True, type=Path)
    parser.add_argument("--gamemd-exe", required=True, type=Path)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--output", type=Path, help="Write newly executed controls here")
    action.add_argument("--check", action="store_true", help="Compare against the bundled corpus")
    action.add_argument("--disassemble", action="store_true", help="Print bounded original body/caller instructions; no execution or writes")
    args = parser.parse_args()
    Probe.input_bytes = (
        identified_bytes(args.ares_dll, ARES_SHA256),
        args.gamemd_exe.read_bytes(),
    )
    if args.disassemble:
        disassemble()
        return
    payload = dict(schema_version=1, ares_sha256=ARES_SHA256, controls=controls())
    if args.check:
        raw = FIXTURE.read_bytes()
        meta = json.loads(FIXTURE.with_suffix(".meta.json").read_text(encoding="utf8"))
        if hashlib.sha256(raw).hexdigest() != meta["payload_sha256"]:
            raise OracleError("Ares prerequisite corpus does not match its metadata")
        if payload != json.loads(raw):
            raise OracleError("Ares prerequisite execution differs from the bundled corpus")
        print(f"Ares prerequisite controls match: {len(payload['controls'])}")
    else:
        raw = (json.dumps(payload, indent=2) + "\n").encode()
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_bytes(raw)
        meta = json.loads(FIXTURE.with_suffix(".meta.json").read_text(encoding="utf8"))
        meta["payload_sha256"] = hashlib.sha256(raw).hexdigest()
        meta["control_count"] = len(payload["controls"])
        meta["mapping_geometry"]["gamemd"]["base"] = hex(IMAGE_BASE)
        meta["mapping_geometry"]["gamemd"]["size"] = hex(IMAGE_SIZE)
        meta["native_regions_end_exclusive"] = {
            name: [hex(start), hex(end)] for name, (start, end) in REGIONS.items()
        }
        meta["command"] = "python -m tools.rules_oracle.ares_prerequisites --ares-dll <Ares.dll> --gamemd-exe <gamemd.exe> --output <corpus.json>"
        args.output.with_suffix(".meta.json").write_text(json.dumps(meta, indent=2) + "\n", encoding="utf8")
        print(f"Wrote {len(payload['controls'])} Ares prerequisite controls")


if __name__ == '__main__':
    main()
