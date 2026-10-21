"""Bounded Ares Country initial-veterancy readers, inheritance and building rank.

Set VERA20K_ARES_DLL and VERA20K_GAMEMD_EXE to local enrolled binaries.
python -m tools.rules_oracle.ares_country_veterancy --check
All names and objects below are synthetic; no mod assets/configuration are used.
"""
import argparse
import hashlib
import os
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_ESP, UC_X86_REG_ESI,
)
from tools import native_oracle as native
from tools.native_oracle import finish_vectors, provenance, run_checked
from tools.rules_oracle.ares_prerequisites import ARES_SHA256, Probe, identified_bytes
from tools.input_oracle.fast_scroll import return_from_sink

SCRATCH = 0x20000000
VTABLES = {"infantry":0x7EB610,"unit":0x7F6218,"aircraft":0x7E2868,"building":0x7E4570}


class CountryFixture(Probe):
    """Reuse the public Ares VM; the selected readers run their own CRT leaves.

    Probe remains the image/arena/call owner. Its prerequisite-specific string,
    type-category and reader services are intentionally not installed here;
    individual controls declare their lexical/transport/diagnostic boundaries.
    """
    def __init__(self, images):
        self.input_bytes = images
        super().__init__()
        self.original_code=[(at,bytes(self.u.mem_read(at,size))) for at,size in
            ((0x10001000,0x92000),(0x511850,0x920),(0x7C8D20,0xC0))]

    def check_code(self):
        assert all(bytes(self.u.mem_read(at,len(raw)))==raw for at,raw in self.original_code)

    def hook(self, u, pc, size, data):
        if pc == 0x30000000:
            u.emu_stop()
        elif pc in (0x10076F08, 0x10076F38):
            super().hook(u, pc, size, data)
        elif pc in (0x65C780,0x65C7E0,0x65C6D0,0x7258D0):
            raise RuntimeError(f"Unexpected RNG/detach boundary {pc:08X}")

    def put(self, at, *values):
        for index, value in enumerate(values):
            self.write(at + 4 * index, value)

    def service_return(self, cleanup, value):
        return_from_sink(self.u, cleanup, value)

    def call(self, entry, args=(), ecx=0):
        result=super().call(entry, *args, ecx=ecx)
        self.check_code()
        return result


def country_reader_case(images, name, values, *, register_later=False):
    """Original country-buffer hook, tokenizer, Find loops and vector writes.

    ReadString is an explicit lexical interface. Original CRT comparison runs. The
    three registered type arrays and preallocated prior lists are inputs; no
    FindOrAllocate/constructor or type-registration result is supplied.
    """
    p = CountryFixture(images)
    country, shim, ini = SCRATCH, SCRATCH + 0x1000, SCRATCH + 0x1800
    p.u.mem_write(country + 0x24, b"CHILD\0")
    p.u.mem_write(country + 0x98, b"PARENT\0")
    p.put(shim + 0x18, country)
    p.put(shim + 0xC, ini)
    families = [
        ("VeteranInfantry", 0x14C, "INF_A", 0xA8E34C, 0xA8E358),
        ("VeteranUnits", 0x168, "VEH_A", 0xA83CE4, 0xA83CF0),
        ("VeteranAircraft", 0x184, "AIR_A", 0xA8B21C, 0xA8B228),
    ]
    registered = []
    for index, (_key, offset, known, array_slot, count_slot) in enumerate(families):
        array, data, known_type, prior_type, late_type = (
            SCRATCH + 0x2000 + index * 0x1000 + off
            for off in (0, 0x100, 0x200, 0x400, 0x600))
        for at, text in ((known_type, known), (prior_type, "PRIOR"), (late_type, "LATE")):
            p.u.mem_write(at + 0x24, text.encode() + b"\0")
        p.put(array, known_type, prior_type, late_type)
        p.put(array_slot, array)
        p.put(count_slot, 2)
        p.put(country + offset, 0x7EB6D4, data, 16, 1, 1, 10)
        p.put(data, prior_type)
        registered.append((count_slot, late_type))
    events = []

    def service(uc, address, _size, _data):
        sp = uc.reg_read(UC_X86_REG_ESP)
        if address == 0x528A10:
            section, key, default, target, capacity = [p.read(sp + 4 + i * 4) for i in range(5)]
            key_name = p.string(key)
            value = values.get(key_name)
            raw = (value or "").encode("ascii")[:capacity - 1]
            p.u.mem_write(target, raw + b"\0")
            events.append(dict(service="ReadString", section=p.string(section), key=key_name,
                               default=p.string(default), capacity=capacity, value=value))
            p.service_return(20, len(raw))
        elif address == 0x10061960:
            section, key, token = [p.read(sp + off) for off in (4, 8, 12)]
            events.append(dict(service="INIParseFailed", section=p.string(section),
                               key=p.string(key), token=p.string(token)))
            p.service_return(16, 0)
        elif address == 0x10002940:
            raise RuntimeError("Fixture exhausted its declared vector capacity")

    p.u.hook_add(UC_HOOK_CODE, service)

    def snapshot():
        result = {}
        for key, offset, _known, _array_slot, _count_slot in families:
            vector = country + offset
            count, array = p.read(vector + 0x10), p.read(vector + 4)
            result[key] = [p.string(p.read(array + i * 4) + 0x24) for i in range(count)]
        return result

    resume = p.call(0x1005BE70, (shim,))
    first = snapshot()
    after_registration = None
    second = None
    if register_later:
        for count_slot, _ in registered:
            p.put(count_slot, 3)
        after_registration = snapshot()
        p.call(0x1005BE70, (shim,))
        second = snapshot()
    return dict(name=name,
                initial=dict(country_id="CHILD",parent_country="PARENT",
                    veteran_lists={key:["PRIOR"] for key,*_ in families},
                    registered_type_ids={key:[known,"PRIOR"] for key,_offset,known,_array,_count in families},
                    unregistered_type_ids={key:["LATE"] for key,*_ in families}),
                values=values, native_resume=resume,
                first=first, after_later_registration_without_read=after_registration,
                after_explicit_reread=second,
                registry_counts=[p.read(slot) for slot, _ in registered], events=events)


def country_reader_controls(images):
    return [
        country_reader_case(images, "omitted_retains", {}),
        country_reader_case(images, "empty_retains", dict.fromkeys(
            ("VeteranInfantry", "VeteranUnits", "VeteranAircraft"), "")),
        country_reader_case(images, "none_and_delimiters_clear", {
            "VeteranInfantry": "none", "VeteranUnits": "<none>", "VeteranAircraft": ",,,"}),
        country_reader_case(images, "known_duplicate_unknown_and_space", {
            "VeteranInfantry": "inf_a,INF_A,UNKNOWN", "VeteranUnits": "VEH_A, VEH_A",
            "VeteranAircraft": "UNKNOWN,AIR_A"}),
        country_reader_case(images, "known_after_byte128_is_read", {
            "VeteranInfantry": "A" * 130 + ",INF_A"}),
        country_reader_case(images, "capacity2048_last_complete_token", {"VeteranInfantry": "," * 2041 + "INF_A"}),
        country_reader_case(images, "capacity2048_partial_final_token", {"VeteranInfantry": "," * 2043 + "INF_A"}),
        country_reader_case(images, "late_registration_needs_reread", dict.fromkeys(
            ("VeteranInfantry", "VeteranUnits", "VeteranAircraft"), "LATE"), register_later=True),
    ]


def country_extension_case(images, name, *, parent_mode='found', passes=(), failed_base=False, parent_token=None, parent_id='PARENT', phase=2, stage_original_vectors=False):
    """Whole CountryExt reader and parent body with actual Find/CRT.

Uses a generic non-primary-Rules INI pointer, omits other configured fields,
and supplies lexical ReadString values and spare storage. No parent lookup,
copy, list decision or vector write is replaced."""
    p = CountryFixture(images)
    ext, parent_ext, country, parent_country = (SCRATCH + offset for offset in (0, 0x1000, 0x3000, 0x4000))
    child_data, parent_data, ini_ref, ini, array = (SCRATCH + offset for offset in (0x5000, 0x5100, 0x5200, 0x5300, 0x5400))
    known, prior, late = (SCRATCH + off for off in (0x6000, 0x7000, 0x8000))
    p.put(ext, country, 0 if failed_base else phase)
    p.put(country + 188, -1)
    p.put(parent_ext, parent_country, 2)
    p.put(country + 196, ext)
    p.put(parent_country + 196, parent_ext)
    p.u.mem_write(country + 36, b'CHILD\x00')
    p.u.mem_write(parent_country + 36, parent_id.encode() + b'\x00')
    p.u.mem_write(parent_country + 100, b'PARENT_ALIAS\x00')
    p.u.mem_write(country + 152, b'CHILD\x00' if parent_mode == 'self' else b'\x00' if parent_mode == 'empty' else b'PARENT\x00')
    if parent_token is not None:
        p.u.mem_write(country + 152, parent_token.encode() + b'\x00')
    initial_parent_token = p.string(country + 152)
    p.put(ext + 0x15C, child_data, child_data + 4, child_data + 128)
    p.put(parent_ext + 0x15C, parent_data, parent_data + 4, parent_data + 128)
    for at, text in ((known, 'KNOWN'), (prior, 'PRIOR'), (late, 'LATE')):
        p.u.mem_write(at + 36, text.encode() + b'\x00')
    p.put(child_data, prior)
    p.put(parent_data, known)
    p.put(array, known, prior, late)
    p.put(0xA83C6C, array)
    p.put(0xA83C78, 2)
    p.put(ini_ref, ini)
    if stage_original_vectors:
        for index, offset in enumerate((0x14C,0x168,0x184)):
            child_native, parent_native = SCRATCH+0xA000+index*0x100, SCRATCH+0xB000+index*0x100
            p.put(country+offset,0x7EB6D4,child_native,4,1,1,10)
            p.put(parent_country+offset,0x7EB6D4,parent_native,4,1,1,10)
            p.put(child_native,prior)
            p.put(parent_native,known)
    events, snapshots = ([], [])
    current = {}
    found = parent_mode != 'missing'
    countries = SCRATCH + 0x5500
    p.put(countries, country, parent_country)
    p.put(0xA83C9C, countries)
    p.put(0xA83CA8, 2 if found else 1)
    pending_find = None
    initial_codes = [(address, bytes(p.u.mem_read(address, size))) for address, size in ((0x1001C530, 154), (0x1001C5D0, 0x196), (0x1001BDC0, 0x317), (0x10010850, 142), (0x10007E50, 71), (0x10007920, 160), *((0x1001BA90, 80), (0x10003950, 96), (0x7C8D20, 192)))]

    def service(uc, address, _size, _data):
        nonlocal pending_find
        sp = uc.reg_read(UC_X86_REG_ESP)
        if address == 0x528A10:
            section, key, default, target, capacity = [p.read(sp + 4 + i * 4) for i in range(5)]
            key_name = p.string(key)
            value = current.get(key_name)
            raw = (value or '').encode('ascii')[:capacity - 1]
            p.u.mem_write(target, raw + b'\x00')
            events.append(dict(service='ReadString', section=p.string(section), key=key_name, default=p.string(default), capacity=capacity, value=value))
            p.service_return(20, len(raw))
        elif address == 0x1001BA90:
            pending_find = dict(service='OriginalFindHouseType', input=p.string(uc.reg_read(UC_X86_REG_ECX)), registry_count=p.read(0xA83CA8), extension_init_state=p.read(ext + 4))
            events.append(pending_find)
        elif address == 0x1001BDD6:
            pointer = uc.reg_read(UC_X86_REG_EAX)
            pending_find.update(found=bool(pointer), returned_id=p.string(pointer + 36) if pointer else None)
            pending_find = None
        elif address == 0x10061960:
            section, key, token = [p.read(sp + off) for off in (4, 8, 12)]
            events.append(dict(service='INIParseFailed', section=p.string(section), key=p.string(key), token=p.string(token)))
            p.service_return(16, 0)
        elif failed_base and address == 0x526B00:
            events.append(dict(service='IniPreparse'))
            p.service_return(0, 0)
        elif failed_base and address == 0x526810:
            events.append(dict(service='IsPresent', section=p.string(p.read(sp + 4)), result=False))
            p.service_return(4, 0)
        elif address in (0x10002940, 0x10009C60, 0x10076F08):
            raise RuntimeError('Fixture exhausted its declared vector capacity')
    p.u.hook_add(UC_HOOK_CODE, service)
    for index, step in enumerate(passes):
        before_events = len(events)
        current = step.get('values', {})
        if 'parent_found' in step:
            found = step['parent_found']
            p.put(0xA83CA8, 2 if found else 1)
        if step.get('register_parent'):
            p.put(0xA83CA8, 2)
        if 'parent_value' in step:
            p.put(parent_data, {'KNOWN': known, 'PRIOR': prior, 'LATE': late}[step['parent_value']])
        if 'parent_name' in step:
            p.u.mem_write(country + 152, step['parent_name'].encode() + b'\x00')
        if step.get('register_late'):
            p.put(0xA83C78, 3)
        if 'read_parent_values' in step:
            current = step['read_parent_values']
            p.call(0x1001C530, (ini,), parent_ext)
            current = step.get('values', {})
        base_result = None
        if failed_base:
            base_result = p.call(0x511850, (ini,), country) & 255
            frame, shim = (SCRATCH + 0x9000, SCRATCH + 0x9200)
            p.put(frame + 8, ini)
            p.put(shim + 16, frame)
            p.put(shim + 24, country)
            p.call(0x1001D110, (shim,))
        else:
            p.call(0x1001C530, (ini,), ext)
        begin, end = (p.read(ext + 0x15C), p.read(ext + 0x160))
        snapshot = dict(step=index, input=step, native_base_returned_al=base_result, veteran_buildings=[p.string(p.read(at) + 36) for at in range(begin, end, 4)], parent_initialized=p.u.mem_read(ext + 0x1B9, 1)[0], extension_init_state=p.read(ext + 4), registered_building_count=p.read(0xA83C78), events=events[before_events:])
        snapshot['parent_name'] = p.string(country + 152)
        begin, end = (p.read(parent_ext + 0x15C), p.read(parent_ext + 0x160))
        snapshot['parent_veteran_buildings'] = [p.string(p.read(at) + 36) for at in range(begin, end, 4)]
        snapshot['country_registry_count'] = p.read(0xA83CA8)
        if stage_original_vectors:
            snapshot["original_country_vectors"]={}
            snapshot["parent_original_country_vectors"]={}
            for key,offset in (("VeteranInfantry",0x14C),("VeteranUnits",0x168),("VeteranAircraft",0x184)):
                for owner,label in ((country,"original_country_vectors"),(parent_country,"parent_original_country_vectors")):
                    source=p.read(owner+offset+4)
                    snapshot[label][key]=[p.string(p.read(source+i*4)+0x24) for i in range(p.read(owner+offset+0x10))]
        snapshots.append(snapshot)
    assert pending_find is None
    for address, raw in initial_codes:
        assert bytes(p.u.mem_read(address, len(raw))) == raw, hex(address)
    initial=dict(country_id="CHILD",parent_id=parent_id,parent_country=initial_parent_token,
        extension_phase=0 if failed_base else phase,parent_initialized=False,
        veteran_buildings=["PRIOR"],parent_veteran_buildings=["KNOWN"],
        registered_building_ids=["KNOWN","PRIOR"],unregistered_building_ids=["LATE"],
        registered_country_ids=["CHILD",parent_id] if parent_mode != "missing" else ["CHILD"],
        original_country_vectors={key:["PRIOR"] if stage_original_vectors else []
            for key in ("VeteranInfantry","VeteranUnits","VeteranAircraft")},
        parent_original_country_vectors={key:["KNOWN"] if stage_original_vectors else []
            for key in ("VeteranInfantry","VeteranUnits","VeteranAircraft")},
        base_reader_mode="missing_section_suffix" if failed_base else "extension_only")
    result = dict(name=name, initial=initial, parent_mode=parent_mode, steps=snapshots, code_sha256={hex(a): hashlib.sha256(data).hexdigest() for a, data in initial_codes}, boundaries='ReadString and parse diagnostic only; actual Country Find and original default-locale CRT execute; supplied Country registry and extension prior state; all other CountryExt fields omitted; spare capacity; generic non-primary-Rules INI pointer')
    result['initial_parent_token'] = initial_parent_token
    result['supplied_country_ids'] = ['CHILD', parent_id]
    result['supplied_parent_name_alias'] = 'PARENT_ALIAS'
    return result

def country_parent_find_controls(images):
    """Original ID-only Find, then the same entry inside the existing reader."""
    p = CountryFixture(images)
    country_array, token = SCRATCH, SCRATCH + 0x800
    country_ids = ["PARENT", "none", "<none>", "DUP", "dup"]
    addresses = [SCRATCH + 0x1000 + index * 0x400 for index in range(len(country_ids))]
    p.put(country_array, *addresses)
    p.put(0xA83C9C, country_array)
    p.put(0xA83CA8, len(addresses))
    for address, name in zip(addresses, country_ids):
        p.u.mem_write(address + 0x24, name.encode() + b"\0")
        p.u.mem_write(address + 0x64, b"PARENT_ALIAS\0")
    rows = []
    code = [(at, bytes(p.u.mem_read(at, size))) for at, size in
            ((0x1001BA90, 0x50), (0x10003950, 0x60), (0x7C8D20, 0xC0))]
    for text in ["PARENT", "pArEnT", "PARENT_ALIAS", "none", "NoNe", "<none>", "<NONE>",
                 "Unknown", " PARENT", "PARENT ", "", "DUP", "dup"]:
        p.u.mem_write(token, text.encode() + b"\0")
        pointer = p.call(0x1001BA90, (), token)
        rows.append(dict(token=text, returned_id=p.string(pointer + 0x24) if pointer else None,
                         returned_registry_index=addresses.index(pointer) if pointer else None))
    for at, raw in code:
        assert bytes(p.u.mem_read(at, len(raw))) == raw
    direct = dict(name="actual_parent_find_id_only", registry_ids=country_ids, shared_name="PARENT_ALIAS",
                  rows=rows, substitutions=[], code_sha256={hex(at): hashlib.sha256(raw).hexdigest() for at, raw in code})
    integrated = [
        country_extension_case(images, "actual_find_casefold_inheritance_once", parent_token="pArEnT", passes=(
            {"read_parent_values": {"VeteranBuildings": "KNOWN,PRIOR"}},
            {"read_parent_values": {"VeteranBuildings": "PRIOR"}},
            {"values": {"VeteranBuildings": "PRIOR"}},
        )),
        country_extension_case(images, "actual_find_alias_rejected_consumes_once", parent_token="PARENT_ALIAS", passes=(
            {}, {"parent_name": "PARENT"},
        )),
        country_extension_case(images, "actual_find_none_rejected_even_with_country_id", parent_token="NoNe", parent_id="none", passes=({},)),
        country_extension_case(images, "actual_find_angled_none_rejected_even_with_country_id", parent_token="<NONE>", parent_id="<none>", passes=({},)),
        country_extension_case(images, "actual_find_missing_registration_consumes_once", parent_mode="missing", passes=(
            {}, {"register_parent": True, "read_parent_values": {"VeteranBuildings": "KNOWN,PRIOR"}},
        )),
        country_extension_case(images, "actual_find_self_defers_until_parent_current_read", parent_mode="self", parent_token="cHiLd", passes=(
            {"read_parent_values": {"VeteranBuildings": "KNOWN"}},
            {"parent_name": "PARENT", "read_parent_values": {"VeteranBuildings": "PRIOR,PRIOR"}},
            {"read_parent_values": {"VeteranBuildings": "KNOWN"}},
        )),
        country_extension_case(images, "actual_find_empty_parent_defers", parent_mode="empty", passes=(
            {}, {"parent_name": "PARENT"},
        )),
    ]
    return [direct, *integrated]



def cameo_country_case(images,name,category,*,member=False,trainable=False,
                       undeploys=False,unit_list_self=False,unit_list_undeploy_target=False,alt_art=True):
    p=CountryFixture(images)
    extension,typ,country,country_ext,house_ext,house=(SCRATCH+off for off in (0,0x1000,0x2000,0x2400,0x3000,0x4000))
    other=SCRATCH+0x9000
    p.put(extension,typ)
    p.put(typ,VTABLES[category])
    p.put(typ+0x2FC,extension)
    p.put(typ+0x710,1 if alt_art else 0)
    p.put(extension+0x24C,0)
    p.u.mem_write(typ+0xC8E,bytes([trainable]))
    p.put(typ+0x408,other if undeploys else 0)
    p.put(house+0x34,country)
    p.put(house+0x16084,house_ext)
    p.put(country+0xC4,country_ext)
    # Both original House spy flags and all three HouseExt spy flags remain0.
    for index,(kind,ptr,count) in enumerate((
        ("infantry",0x150,0x15C),("unit",0x16C,0x178),("aircraft",0x188,0x194))):
        data=SCRATCH+0xA000+index*0x100
        values=[typ] if (kind==category and member) or (kind=="unit" and unit_list_self) else []
        if kind=="unit" and unit_list_undeploy_target:
            values.append(other)
        p.put(country+ptr,data)
        p.put(country+count,len(values))
        p.put(data,*values)
    data=SCRATCH+0xB000
    p.put(country_ext+0x15C,data,data+(4 if member and category=="building" else 0),data+4)
    p.put(data,typ)
    eligible=bool(p.call(0x1003D800,(house,),extension)&255)
    row=dict(name=name,category=category,member=member,trainable=trainable,undeploys=undeploys,
        unit_list_self=unit_list_self,unit_list_undeploy_target=unit_list_undeploy_target,
        alt_art=alt_art,eligible=eligible)
    if unit_list_self and category=="building":
        row["input_limit"]="Building self pointer supplied in UnitType-only list; proves compared pointer but not ordinary INI-produced reachability"
    return row


def cameo_country_controls(images):
    rows=[]
    for category in ("infantry","unit","aircraft","building"):
        rows.append(cameo_country_case(images,category+"_country_miss",category))
        rows.append(cameo_country_case(images,category+"_country_match_untrainable",category,member=True))
    rows.extend([
        cameo_country_case(images,"missing_alternate_art_rejects_country_match","infantry",member=True,alt_art=False),
        cameo_country_case(images,"undeploy_building_ignores_building_list","building",member=True,undeploys=True),
        cameo_country_case(images,"undeploy_building_ignores_target_in_unit_list","building",undeploys=True,unit_list_undeploy_target=True),
        cameo_country_case(images,"undeploy_building_compares_self_in_unit_list","building",undeploys=True,unit_list_self=True),
    ])
    return rows

def constructor_control(images):
    p = CountryFixture(images)
    ext, country = SCRATCH, SCRATCH + 0x3000
    p.u.mem_write(ext, b"\xA5" * 0x200)
    p.u.reg_write(UC_X86_REG_ESI, ext)
    p.u.reg_write(UC_X86_REG_EAX, country)
    # Exact constructor literal-store span, not the later GUI defaults helper.
    run_checked(p.u, 0x1001B85F, 0x1001B8DA)
    return dict(name="country_extension_constructor_literal_defaults",
        init_state=p.read(ext+4), veteran_building_header=[p.read(ext+off) for off in (0x15C,0x160,0x164)],
        parent_initialized=p.u.mem_read(ext+0x1B9,1)[0],
        limit="Constructor literal stores only; complete construction and remaining extension fields excluded")


def building_creation_case(images, name, *, members=(), trainable=False, initial_bits=0, owner=True):
    p = CountryFixture(images)
    obj, house_ext, country, country_ext, typ, shim, house, data, other = (
        SCRATCH + off for off in (0x1000,0x2000,0x3000,0x3400,0x4000,0x5000,0x6000,0x8000,0x9000))
    p.put(obj+0x21C, house if owner else 0)
    p.put(obj+0x520, typ)
    p.put(obj+0x150, initial_bits)
    p.put(house+0x34, country)
    p.put(house+0x16084, house_ext)
    p.put(country+0xC4, country_ext)
    p.put(country_ext+0x15C, data, data+4*len(members), data+64)
    for index, label in enumerate(members):
        p.put(data+4*index, typ if label == "MATCH" else other)
    p.u.mem_write(typ+0xC8E, bytes([trainable]))
    p.u.mem_write(house_ext+0x4A, b"\0")
    p.put(shim+0xC, obj)
    calls=[]
    def observe(u, pc, size, data_):
        if pc == 0x10020750:
            sp=u.reg_read(UC_X86_REG_ESP)
            calls.append(dict(service="Academy", object_is_subject=p.read(sp+4)==obj,
                              category=p.read(sp+8), veterancy_bits=p.read(obj+0x150)))
            p.service_return(8,0)
        elif pc in (0x65C780,0x65C7E0,0x65C6D0,0x7258D0):
            raise RuntimeError(f"Unexpected RNG/detach boundary {pc:08X}")
    original=bytes(p.u.mem_read(0x100236D0,0x75))
    p.u.hook_add(UC_HOOK_CODE,observe)
    result=p.call(0x100236D0,(shim,))
    assert bytes(p.u.mem_read(0x100236D0,0x75))==original
    return dict(name=name,members=list(members),trainable=trainable,owner=owner,
        initial_bits=initial_bits, veterancy_bits=p.read(obj+0x150),returned=result,calls=calls,
        limit="Country-membership branch only; spy flagfalse and Academy is an observed sink")


def extension_stream(images):
    from tools.spatial_oracle import native_id_snapshot as transport_owner
    p=CountryFixture(images)
    u=p.u
    transport=transport_owner.ScenarioFixture.__new__(transport_owner.ScenarioFixture)
    transport.u,transport.data,transport.position=u,bytearray(),0
    read_callback,write_callback=0x30000400,0x30000500
    p.put(transport_owner.STREAM,transport_owner.VTABLE)
    p.put(transport_owner.VTABLE+12,read_callback,write_callback)
    ext,data,holder=SCRATCH,SCRATCH+0x1000,SCRATCH+0x2000
    tokens=[0x11223344,0x55667788,0x11223344]
    events=[]
    def observe(vm,pc,size,data_):
        sp=u.reg_read(UC_X86_REG_ESP)
        if pc in (read_callback,write_callback):
            events.append(dict(operation="read" if pc==read_callback else "write",size=p.read(sp+12)))
            logical=transport_owner.READ if pc==read_callback else transport_owner.WRITE
            transport_owner.ScenarioFixture.hook(transport,vm,logical,size,data_)
        elif pc==0x1006C3E0:
            target=p.read(sp+4)
            events.append(dict(operation="register_pointer_swizzle",vector_index=(target-data)//4,raw=p.read(target)))
            p.service_return(4,1)
        elif pc in (0x10002940,0x10009C60):
            raise RuntimeError("Fixture exhausted declared vector capacity")
    u.hook_add(UC_HOOK_CODE,observe)
    def flag_slice(begin,end):
        u.reg_write(UC_X86_REG_EBX,ext)
        u.reg_write(UC_X86_REG_EDI,holder)
        u.reg_write(UC_X86_REG_ESP,0x2000F000)
        run_checked(u,begin,end)
        assert u.reg_read(UC_X86_REG_ESP)==0x2000F000
    rows=[]
    for flag in (0,1):
        before=len(events)
        p.put(data,*tokens)
        p.put(ext+0x15C,data,data+12,data+64)
        u.mem_write(ext+0x1B9,bytes([flag]))
        output=p.alloc(16)
        p.call(0x1006B4E0,(32,),output)
        p.put(holder,output)
        p.call(0x1000F0F0,(holder,),ext+0x15C)
        flag_slice(0x1001B378,0x1001B388)
        payload=bytes(u.mem_read(p.read(output),p.read(output+4)-p.read(output)))
        transport.data,transport.position=bytearray(),0
        writer=p.call(0x1006B7D0,(transport_owner.STREAM,),output)
        p.put(ext+0x160,data)
        u.mem_write(data,bytes(12))
        u.mem_write(ext+0x1B9,bytes([1-flag]))
        input_buffer=p.alloc(16)
        p.call(0x1006B5C0,(transport_owner.STREAM,),input_buffer)
        p.put(holder,input_buffer)
        p.call(0x1000DAA0,(holder,),ext+0x15C)
        flag_slice(0x1001B1C3,0x1001B1D3)
        rows.append(dict(name="veteran_buildings_and_one_shot_stream",input_flag=flag,pointer_tokens=tokens,
            payload_hex=payload.hex(),framed_stream_hex=bytes(transport.data).hex(),writer_al=writer&255,
            loaded_pointer_tokens=[p.read(at) for at in range(p.read(ext+0x15C),p.read(ext+0x160),4)],
            loaded_parent_initialized=u.mem_read(ext+0x1B9,1)[0],stream_read_bytes=transport.position,
            buffer_consumed=p.read(input_buffer+12),events=events[before:]))
    return rows



def installed_hook_records(images):
    p=CountryFixture(images)
    names={"HouseTypeClass_LoadFromINI","HouseTypeClass_LoadFromINI_Buffer_CountryVeteran","BuildingClass_Init_Academy"}
    return [dict(site=f"{p.read(at):08X}",length=p.read(at+4),name=p.string(p.read(at+8)))
            for at in range(0x100C3000,0x100C8A80,16) if p.string(p.read(at+8)) in names]


def generate(ares):
    images = ares, native.image_bytes()
    find = country_parent_find_controls(images)
    return dict(schema_version=1,
        installed_hook_records=installed_hook_records(images),
        constructor=constructor_control(images),
        cameo_country=cameo_country_controls(images),
        building_creation=[
            building_creation_case(images,"empty_list_retains_rank"),
            building_creation_case(images,"nonmatching_list_retains_rank",members=("OTHER",),initial_bits=0x3F000000),
            building_creation_case(images,"matching_untrainable_is_veteran",members=("MATCH",)),
            building_creation_case(images,"matching_trainable_is_veteran",members=("OTHER","MATCH"),trainable=True),
            building_creation_case(images,"duplicates_do_not_add_rank",members=("MATCH","MATCH")),
            building_creation_case(images,"matching_replaces_prior_rank",members=("MATCH",),initial_bits=0x40000000),
            building_creation_case(images,"missing_owner_retains_rank",members=("MATCH",),owner=False),
        ],
        extension_stream=extension_stream(images),
        country_lists=country_reader_controls(images),
        parent_find=find[0], extension_histories=find[1:] + [
            country_extension_case(images,"extension_inheritance_keeps_original_three_vectors",
                stage_original_vectors=True,passes=({},)),
            country_extension_case(images, "missing_section_suffix_consumes_parent_once",
                failed_base=True,
                passes=({}, {"parent_value": "PRIOR"})),
            country_extension_case(images, "retained_override_clear_and_late_registration",
                passes=(
                    {},
                    {"parent_value": "PRIOR", "values": {"VeteranBuildings": ""}},
                    {"values": {"VeteranBuildings": "known,KNOWN,UNKNOWN, KNOWN"}},
                    {"values": {"VeteranBuildings": "none"}},
                    {"values": {"VeteranBuildings": "LATE"}},
                    {"register_late": True},
                    {"values": {"VeteranBuildings": "LATE"}},
                    {"values": {"VeteranBuildings": ",,,"}},
                )),
            country_extension_case(images,"first_init_self_preserves_retained_list",parent_mode="self",phase=0,passes=({},)),
            country_extension_case(images,"negative_phase_skips_common",phase=-1,passes=({},)),
            country_extension_case(images,"phase_above2_skips_common",phase=3,passes=({},)),
            country_extension_case(images,"building_capacity2048_boundary",passes=(
                {"values":{"VeteranBuildings":","*2041+"KNOWN"}},
                {"values":{"VeteranBuildings":","*2043+"KNOWN"}},
            )),
        ])


def metadata():
    return provenance(
        scope="Ares Country three-list replacement, retained VeteranBuildings/one-shot inheritance, country-only initial building rank and alternate-cameo eligibility, and bounded list/flag persistence.",
        assumptions=[
            "Supplied synthetic Country/type registries, stored IDs, retained vectors, extension phase and spare capacities; no type construction or physical INI ingestion is certified.",
            "Actual Ares Country Find, sentinel handling, original CRT comparisons, parent copy, tokenizer, list Find and vector writes execute. Common CountryExt reader uses a non-primary-Rules INI pointer; GUI-specific reader is excluded.",
            "Original Country511850 failure plus installed suffix body are connected by a synthetic ABI frame; no whole executable hook installer or startup schedule is executed.",
            "Country reader supplied lexical values include empty strings; physical parser omission of empty/whitespace lines belongs to the production INI owner.",
            "Constructor control executes literal stores1001B85F..1001B8DA only. Reader phase0/negative/>2 controls operate on explicit prior values, not fully constructed game objects.",
            "Building creation executes100236D0 with spy flagfalse; Academy10020750 is an observed sink. Alternate-cameo controls execute1003D800 with all spy flagsfalse and a supplied alternate SHP field. A positive Building self pointer in a UnitType-only list is a structural diagnostic, not normal INI-produced reachability.",
            "Persistence executes actual ByteBuffer helpers, complete vector save/load helpers and isolated parent-flag slices, reusing ScenarioFixture IStream. Swizzle registration is recorded without relocation, and whole extension/save framing, failure paths and game startup remain excluded.",
            "Selected controls reject observed RNG entries65C6D0/65C780/65C7E0 and detach7258D0; they establish no draw/detach on the bounded selected paths, not full initialization timing or unrelated effects.",
        ], substitutions=[
            "Public ares_prerequisites.Probe owns native image mapping/arena/call execution; prerequisite-specific services are disabled for these reader controls.",
            "ReadString lexical-value transport, parse diagnostics, missing-section preparse/IsPresent, and Academy callback boundaries; no parent decision or membership result is supplied.",
            "Existing Probe new/delete backing is retained only for ByteBuffer storage. ScenarioFixture.hook supplies exact in-memory IStream bytes; pointer-swizzle registration returns success without resolving pointers.",
        ], entry_points=dict(country_lists=0x1005BE70, country_base=0x511850,
            extension_suffix=0x1001D110, extension_read=0x1001C530,
            extension_common=0x1001C5D0, parent_copy=0x1001BDC0,
            parent_find=0x1001BA90, building_list=0x10010850,
            building_creation=0x100236D0,cameo_eligibility=0x1003D800,
            vector_save=0x1000F0F0,vector_load=0x1000DAA0,
            flag_save=0x1001B378,flag_load=0x1001B1C3,
            buffer_save=0x1006B7D0,buffer_load=0x1006B5C0))


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
            "stream_owner": Path("tools/spatial_oracle/native_id_snapshot.py"),
            "return_owner": Path("tools/input_oracle/fast_scroll.py")})


if __name__ == "__main__":
    main()
