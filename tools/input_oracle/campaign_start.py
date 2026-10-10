"""Original campaign metadata and selected scenario-start initialization.

Composes the existing BulletReader cached INI/allocation/CRT owner. It executes
original campaign constructors/readers, the original RNG initializer with
explicit Windows entropy transport, and bounded Full_Init/House kernels.
Full Windows dialog, file/archive loading and full scenario loading are outside
this corpus. No gameplay callable is replaced by a supplied success result.
"""

import hashlib
import argparse
import os
import struct
from pathlib import Path
from types import SimpleNamespace

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
    UC_X86_REG_ESP,
)

from tools.native_inspect import selected_ranges
from tools.native_oracle import (
    RET_MAGIC, finish_vectors, image_bytes, image_sha256, provenance, run_checked,
)
from tools.projectile_oracle.bridge_render_inputs import BulletReader, lexical
from tools.rules_oracle.bridge_child_sound import Sound, sections
from tools.rmg_oracle.gen_rng_vectors import STRUCT_LEN, seeded_struct
from tools.spatial_oracle.building_body_rules import INI, RULES, SP, dwords
from tools.storage_oracle.keyboard_bindings import stock_csf
from tools.spatial_oracle.infantry_deploy_action import Fixture as AtomicTransportOwner
from tools.input_oracle.fast_scroll import return_from_sink

HERE = Path(__file__).resolve().parent


def assets_root():
    value = os.environ.get("VERA20K_CAMPAIGN_START_ASSETS")
    if not value:
        raise ValueError("Set VERA20K_CAMPAIGN_START_ASSETS to extracted retail campaign INIs/maps")
    return Path(value)


class CampaignReader(BulletReader):
    """Campaign fixture on the shared original reader/allocation owner."""

    def __init__(self):
        self.entropy = None
        self.entropy_events = []
        self.native_events = []
        self.color_read_events = []
        self.pending_color_string = None
        self.side_read_events = []
        self.side_native_events = []
        self.pending_side_string = None
        self.trigger_read_events = []
        self.pending_trigger_string = None
        super().__init__({}, assets_root())
        self.original_code = selected_ranges(image_bytes(), None, None, code_only=True)
        self.scenario = self.read32(0xA8B230)
        self.u.mem_write(0x889F64, b"\0")  # supplied runtime empty-default buffer
        self.u.mem_write(0xA83CF8, dwords(0x7EB6D4, self.alloc(4096), 1024, 1, 0, 10))
        self.u.mem_write(0xA80228, dwords(0x7EB6D4, self.alloc(4096), 1024, 1, 0, 10))
        self.u.mem_write(0xA8B238, dwords(0))
        self.u.mem_write(0xA8ED84, dwords(0))
        # Reuse the existing original CRT's single-thread Windows import seam.
        sinks = (self.alloc(16), self.alloc(16))
        self.atomic = SimpleNamespace(u=self.u, imports=list(sinks), read=self.read32)
        self.u.mem_write(0x7E11C8, dwords(*sinks))
        self.clock_sink = self.alloc(16)
        self.clock_ms = 0
        self.clock_reads = []
        self.u.mem_write(0x7E1530, dwords(self.clock_sink))
        for vector in (0xABF390, 0xA83C98, 0x8B4120, 0xA8E330, 0xA83CB8, 0xB0F6C8):
            self.u.mem_write(vector, dwords(0x7EB6D4, self.alloc(4096), 1024, 1, 0, 10))

    def ordered_ini(self, values):
        # Reuse the established source-order linked/cache transport owner.
        proxy = Sound.__new__(Sound)
        proxy.__dict__ = self.__dict__
        Sound.make_ini(proxy, values)

    def check_code(self):
        for span in self.original_code:
            assert bytes(self.u.mem_read(span["address"], len(span["bytes"]))) == span["bytes"]

    def hook(self, u, pc, size, data):
        if pc == 0x528A10:
            sp = u.reg_read(UC_X86_REG_ESP)
            if self.read32(sp) == 0x474ABF:
                event = dict(default_name=self.string(self.read32(sp + 12)),
                             capacity=self.read32(sp + 20))
                self.color_read_events.append(event)
                self.pending_color_string = event, self.read32(sp + 16)
            if self.read32(sp) in (0x4767E8, 0x475717):
                event = dict(caller=f"{self.read32(sp):08x}",
                             section=self.string(self.read32(sp + 4)),
                             key=self.string(self.read32(sp + 8)),
                             default=self.string(self.read32(sp + 12)),
                             capacity=self.read32(sp + 20))
                self.side_read_events.append(event)
                self.pending_side_string = event, self.read32(sp + 16)
            if self.read32(sp) == 0x727279:
                event = dict(caller="00727279",
                             section=self.string(self.read32(sp + 4)),
                             key=self.string(self.read32(sp + 8)),
                             default=self.string(self.read32(sp + 12)),
                             capacity=self.read32(sp + 20))
                self.trigger_read_events.append(event)
                self.pending_trigger_string = event, self.read32(sp + 16)
        elif pc == 0x474ABF and self.pending_color_string is not None:
            event, buffer = self.pending_color_string
            event.update(buffer=self.string(buffer),
                         returned_count=u.reg_read(UC_X86_REG_EAX))
            self.pending_color_string = None
        elif pc in (0x4767E8, 0x475717) and self.pending_side_string is not None:
            event, buffer = self.pending_side_string
            event.update(buffer=self.string(buffer),
                         returned_count=u.reg_read(UC_X86_REG_EAX))
            self.pending_side_string = None
        elif pc == 0x727279 and self.pending_trigger_string is not None:
            event, buffer = self.pending_trigger_string
            event.update(buffer=self.string(buffer),
                         returned_count=u.reg_read(UC_X86_REG_EAX))
            self.pending_trigger_string = None
        if pc in (0x5113F0, 0x6A4550, 0x5117D0, 0x672440, 0x511850):
            sp = u.reg_read(UC_X86_REG_ESP)
            event = dict(pc=f"{pc:08x}", caller=f"{self.read32(sp):08x}")
            if pc in (0x5113F0, 0x6A4550):
                event.update(family="Country" if pc == 0x5113F0 else "Side",
                             incoming=self.string(self.read32(sp + 4)),
                             id_before=self.read32(self.scenario + 0x214))
            elif pc == 0x5117D0:
                event["token"] = self.string(u.reg_read(UC_X86_REG_ECX))
            elif pc == 0x511850:
                ptr = u.reg_read(UC_X86_REG_ECX)
                event.update(country=self.string(ptr + 0x24),
                             name_before=self.string(ptr + 0x64))
            self.side_native_events.append(event)
        if pc == 0x7C9430:
            # CRT malloc shares the inherited allocator transport; strdup's
            # native strlen, size arithmetic and strcpy still execute.
            super().hook(u, 0x7C8E17, size, data)
            return
        if hasattr(self, "atomic") and pc in self.atomic.imports:
            AtomicTransportOwner.observe(self.atomic, u, pc, size, data)
            return
        if hasattr(self, "clock_sink") and pc == self.clock_sink:
            self.clock_reads.append(self.clock_ms)
            return_from_sink(u, 0, self.clock_ms)
            return
        if self.entropy is not None and pc in (0x52FC73, 0x52FDEE):
            sp = u.reg_read(UC_X86_REG_ESP)
            if pc == 0x52FC73:
                dest = self.read32(sp)
                raw = struct.pack("<8H", *self.entropy["system_time"])
                u.mem_write(dest, raw)
                u.reg_write(UC_X86_REG_ESP, sp + 4)
                self.entropy_events.append(dict(kind="Windows_GetSystemTime", values=list(self.entropy["system_time"])))
            else:
                u.reg_write(UC_X86_REG_EAX, self.entropy["tick_count"])
                self.entropy_events.append(dict(kind="Windows_GetTickCount", value=self.entropy["tick_count"]))
            u.reg_write(UC_X86_REG_EIP, pc + 6)
            return
        if pc in (0x65C6D0, 0x65C780, 0x65C7E0, 0x661C10, 0x4F6EC0, 0x68BCB0, 0x4F9B70):
            sp = u.reg_read(UC_X86_REG_ESP)
            event = dict(pc=f"{pc:08x}", this=u.reg_read(UC_X86_REG_ECX),
                         return_pc=f"{self.read32(sp):08x}")
            if pc in (0x65C6D0, 0x4F6EC0):
                event["argument"] = self.read32(sp + 4)
            if pc == 0x65C7E0:
                event["bounds"] = list(struct.unpack("<2i", u.mem_read(sp + 4, 8)))
            if pc == 0x4F9B70:
                event["target_array_index"] = self.read32(self.read32(sp + 4) + 0x30)
                event["announce"] = self.read32(sp + 8)
            self.native_events.append(event)
        if pc == 0x4F6354:
            self.native_events.append(dict(pc=f"{pc:08x}", event="house_attack_draw_return",
                                          house=u.reg_read(UC_X86_REG_EBP),
                                          value=u.reg_read(UC_X86_REG_EAX)))
        if pc in (0x500B40, 0x500AA4, 0x500B2A):
            ptr = u.reg_read(UC_X86_REG_ECX if pc == 0x500B40 else UC_X86_REG_ESI)
            if pc == 0x500B2A:
                ptr = self.read32(self.read32(0xA8022C) + (self.read32(0xA80238) - 1) * 4)
            self.native_events.append(dict(pc=f"{pc:08x}", event="house_reader_entry" if pc == 0x500B40 else "house_reader_return",
                                          house=ptr, state=house_state(self, ptr)))
        super().hook(u, pc, size, data)

    def seed_streams(self, seed):
        self.invoke(0x65C6D0, self.scenario + 0x218, (seed,))
        self.invoke(0x65C6D0, 0x886B88, (seed,))
        self.invoke(0x65C6D0, 0xABE890, (31,))

    def rng_state(self):
        return {name: bytes(self.u.mem_read(ptr, STRUCT_LEN)).hex() for name, ptr in
                (("scenario", self.scenario + 0x218), ("main", 0x886B88), ("mapgen", 0xABE890))}

    def region(self, begin, end, *, required=(), context=None):
        self.u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(self.u, begin, end, count=2_000_000, required_addresses=required, context=context)

    def invoke_house_list(self, context):
        # Split at the actual outer reader's call/return boundaries. REP-clears
        # inside each MakeAlly rebuild can otherwise exhaust the shared per-run
        # cap on the physical17-House map. No register/local/stack is changed
        # between these contiguous native regions.
        self.u.mem_write(SP, dwords(RET_MAGIC))
        self.u.reg_write(UC_X86_REG_ESP, SP)
        self.u.reg_write(UC_X86_REG_ECX, INI)
        run_checked(self.u, 0x5009B0, 0x500A89, count=2_000_000, context=context)
        begin = 0x500A89
        while begin != RET_MAGIC:
            run_checked(self.u, begin, (0x500AA4, 0x500B2A, RET_MAGIC),
                        count=2_000_000, context=context)
            stopped = self.u.reg_read(UC_X86_REG_EIP)
            if stopped == 0x500AA4:
                run_checked(self.u, stopped, 0x500AE3, count=2_000_000, context=context)
                begin = 0x500AE3
            elif stopped == 0x500B2A:
                run_checked(self.u, stopped, 0x500B2F, count=2_000_000, context=context)
                begin = 0x500B2F
            else:
                begin = stopped

    def begin_scenario_init(self):
        # StartScenario -> ReadScenario -> ReadScenarioINI -> Full_Init.
        # Preserve the actual nested initialization prior before any House
        # admission. These two original kernels increment the process counter;
        # leaving the image's zero value would select live diplomacy admission.
        values = [self.read32(0xA8E7AC)]
        assert values[0] == 0
        self.region(0x68467C, 0x68468A)
        values.append(self.read32(0xA8E7AC))
        self.u.reg_write(UC_X86_REG_EBX, 0)
        self.u.reg_write(UC_X86_REG_ESI, 0)
        self.u.reg_write(UC_X86_REG_ECX, INI)
        self.region(0x686B35, 0x686B55)
        values.append(self.read32(0xA8E7AC))
        assert values == [0, 1, 2]
        return values


def install_csf(m, names):
    physical, source_hash = stock_csf()
    names = sorted(names)
    records, values, extras = m.alloc(len(names) * 0x28), m.alloc(len(names) * 4), m.alloc(len(names) * 4)
    for i, name in enumerate(names):
        raw = (physical[name] + "\0").encode("utf-16-le")
        ptr = m.alloc(len(raw))
        m.u.mem_write(ptr, raw)
        m.u.mem_write(records + i * 0x28, name.encode("ascii") + b"\0")
        m.u.mem_write(records + i * 0x28 + 0x24, dwords(i))
        m.u.mem_write(values + i * 4, dwords(ptr))
    m.u.mem_write(0xB1CF6C, dwords(len(names)))
    m.u.mem_write(0xB1CF74, dwords(records, values, extras))
    return dict(source_sha256=source_hash, entries={name: physical[name] for name in names})


def campaign_state(m, ptr):
    def signed(off):
        return struct.unpack("<i", m.u.mem_read(ptr + off, 4))[0]
    return dict(id=m.string(ptr + 0x24), cd=signed(0x98), scenario=m.string(ptr + 0x9C),
                final_movie=signed(0x29C), description_utf16_hex=bytes(m.u.mem_read(ptr + 0x2A0, 256)).hex())


def metadata_history():
    m = CampaignReader()
    raw = (assets_root() / "battlemd.ini").read_bytes()
    physical = sections(raw)
    csf = install_csf(m, ["DESC:ALL1", "DESC:SOV1"])
    m.ordered_ini(physical)
    m.invoke(0x46CE10, INI, context={"case": "stock_BATTLEMD_campaign_registry", "input_sha256": hashlib.sha256(raw).hexdigest()})
    count, array = m.read32(0xA83D08), m.read32(0xA83CFC)
    entries = [campaign_state(m, m.read32(array + i * 4)) for i in range(count)]
    lookups = []
    for name in ("all1", "ALL1", "sov1", "tut1", "missing"):
        index = struct.unpack("<i", dwords(m.invoke(0x46CC90, m.cstring(name))))[0]
        lookups.append(dict(request=name, index=index))
    m.check_code()
    return dict(physical_battlemd_sha256=hashlib.sha256(raw).hexdigest(), csf=csf, entries=entries, lookups=lookups)


def metadata_controls():
    m = CampaignReader()
    install_csf(m, ["DESC:ALL1", "DESC:SOV1"])
    layers = [
        ("missing_list", {}),
        ("empty_name", {"Battles": {"0": ""}}),
        ("missing_section", {"Battles": {"0": "Absent"}}),
        ("duplicate_id", {"Battles": {"0": "Mixed", "1": "mIXED"},
                          "Mixed": {"CD": "2", "Scenario": "first.map", "DebugOnly": "yes", "Description": "First"}}),
        ("reload_current_defaults", {"Battles": {"0": "MIXED"}, "Mixed": {"CD": "junk"}}),
        ("read_false_retains", {"Battles": {"0": "Mixed"}}),
        ("replace_empty", {"Battles": {"0": "Mixed"}, "Mixed": {"Scenario": "", "CD": "-1", "DebugOnly": "yes", "Description": ""}}),
        ("long_id", {"Battles": {"0": "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890", "1": "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890"},
                     "ABCDEFGHIJKLMNOPQRSTUVWX": {"Scenario": "long.map", "DebugOnly": "yes", "Description": "Long"}}),
    ]
    rows = []
    for name, values in layers:
        m.ordered_ini(values)
        m.reads.clear()
        m.invoke(0x46CE10, INI, context={"case": name})
        array, count = m.read32(0xA83CFC), m.read32(0xA83D08)
        rows.append(dict(name=name, sections=values, entries=[campaign_state(m, m.read32(array + i * 4)) for i in range(count)], reads=list(m.reads)))
    m.check_code()
    return rows


def movie_names(m):
    return [m.string(m.read32(m.read32(0xABF394) + i * 4)) for i in range(m.read32(0xABF3A0))]


def movie_catalog():
    m = CampaignReader()
    raw = (Path("ini") / "artmd.ini").read_bytes()
    physical = sections(raw)
    physical_movies = {"Movies": physical["Movies"]}
    m.ordered_ini(physical_movies)
    physical_admitted = bool(m.invoke(0x674550, 0, (INI,)) & 255)
    catalog = movie_names(m)
    rows = []
    for default in (-1, 17):
        for value in (None, "", "<none>", "none", "A01_F00e", "a01_f00E", "S01_F00e", "A01_F00e.bik", "unknown", "A" * 140):
            keys = {} if value is None else {"FinalMovie": value}
            m.ordered_ini({"Control": keys})
            result = m.invoke(0x4757D0, INI, (m.cstring("Control"), m.cstring("FinalMovie"), default))
            rows.append(dict(value=value, default=default, result=struct.unpack("<i", dwords(result))[0]))
    controls = []
    for name, values in (("absent", {}), ("empty", {"Movies": {}}),
                         ("sentinels_duplicates", {"Movies": {"0": "<none>", "1": "<NONE>", "2": "none", "3": "NONE", "4": "", "5": "NewName", "6": "newNAME"}}),
                         ("truncate", {"Movies": {"0": "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890", "1": "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567891"}})):
        c = CampaignReader()
        c.ordered_ini(values)
        admitted = bool(c.invoke(0x674550, 0, (INI,)) & 255)
        lookups = []
        for value in (None, "", "<none>", "none", "NONE", "NewName", "newname", "ABCDEFGHIJKLMNOPQRSTUVWXYZ12345"):
            found = c.invoke(0x48DF30, 0 if value is None else c.cstring(value))
            lookups.append(dict(value=value, result=struct.unpack("<i", dwords(found))[0]))
        controls.append(dict(name=name, sections=values, admitted=admitted, catalog=movie_names(c), lookups=lookups))
        c.check_code()
    m.check_code()
    return dict(artmd_sha256=hashlib.sha256(raw).hexdigest(), admitted=physical_admitted, physical_movies=physical_movies, catalog=catalog, reader_cases=rows, controls=controls)


def loading_geometry():
    rows = []
    for mode in (0, 5):
        for width, height in ((640, 480), (800, 600), (1024, 768), (641, 480),
                              (801, 601), (799, 599), (639, 479), (301, 201)):
            m = CampaignReader()
            manager, point = m.alloc(0x40), m.alloc(8)
            m.u.mem_write(manager, b"\xa5" * 0x40)
            m.u.mem_write(0x8A00A4, dwords(width, height))
            m.u.mem_write(0xA8B238, dwords(mode))
            m.invoke(0x552B10, manager)
            m.invoke(0x552BE0, manager, (point,))
            rects = {name: list(struct.unpack("<4i", m.u.mem_read(manager + off, 16)))
                     for name, off in (("title", 0xC), ("body", 0x1C), ("bar", 0x2C))}
            rows.append(dict(mode=mode, width=width, height=height, rects=rects,
                             progress_point=list(struct.unpack("<2i", m.u.mem_read(point, 8)))))
            m.check_code()
    return rows


LOADING_STRINGS = {"LSLoadMessage": 0x362C, "LSLoadBriefing": 0x364B,
                   "LS640BkgdName": 0x367C, "LS800BkgdName": 0x36BC, "LS800BkgdPal": 0x36FC}
LOADING_COORDS = {"LS640BriefLocX": 0x366C, "LS640BriefLocY": 0x3670,
                  "LS800BriefLocX": 0x3674, "LS800BriefLocY": 0x3678}


def mission_loading():
    raw = (assets_root() / "missionmd.ini").read_bytes()
    physical = sections(raw)
    rows = []
    cases = [("stock_all", "ALL01UMD.MAP", physical), ("stock_sov", "SOV01UMD.MAP", physical),
             ("absent", "ALL01UMD.MAP", {}),
             ("section_case", "all01umd.map", physical),
             ("key_case", "ALL01UMD.MAP", {"ALL01UMD.MAP": {"lsloadmessage": "wrong", "ls640brieflocx": "9"}}),
             ("long_strings", "ALL01UMD.MAP", {"ALL01UMD.MAP": {**{key: "X" * 140 for key in LOADING_STRINGS},
                                **dict(zip(LOADING_COORDS, ("-9", "junk", "2147483648", "0x10")))}})]
    for name, filename, values in cases:
        m = CampaignReader()
        m.u.mem_write(m.scenario + 0x125C, filename.encode("ascii") + b"\0")
        for off in LOADING_STRINGS.values():
            m.u.mem_write(m.scenario + off, b"Prior\0")
        for off, value in zip(LOADING_COORDS.values(), (77, -77, 1234, -1234)):
            m.u.mem_write(m.scenario + off, dwords(value))
        m.ordered_ini(values)
        # Full_Init's local CCINI starts at the established frame's ESP+1C.
        m.u.mem_write(SP + 0x1C, bytes(m.u.mem_read(INI, 0x40)))
        m.reads.clear()
        m.region(0x687000, 0x6873AB, required=(0x526B10,), context={"case": name, "filename": filename})
        rows.append(dict(name=name, filename=filename,
                         sections={key: value for key, value in values.items()
                                   if key.lower() == filename.lower()},
                         supplied_section=values.get(filename),
                         strings={key: m.string(m.scenario + off) for key, off in LOADING_STRINGS.items()},
                         coords={key: struct.unpack("<i", m.u.mem_read(m.scenario + off, 4))[0] for key, off in LOADING_COORDS.items()},
                         reads=list(m.reads)))
        m.check_code()
    return dict(missionmd_sha256=hashlib.sha256(raw).hexdigest(), rows=rows)


def rng_case(*, override, tick_count, saved=0, replay=0, network=0, mode=0, prior_seed=17, mapgen_disabled=0):
    m = CampaignReader()
    m.seed_streams(prior_seed)
    m.invoke(0x6616B0, 0xA8E7B0)
    m.u.mem_write(0xA8ED94, dwords(prior_seed))
    m.u.mem_write(0xA8ED98, dwords(override))
    m.u.mem_write(0xA8B8B8, dwords(saved))
    m.u.mem_write(0xA8D5F8, dwords(replay))
    m.u.mem_write(0xAA0444, dwords(network))
    m.u.mem_write(0xA8B238, dwords(mode))
    m.u.mem_write(0xABE890, bytes([mapgen_disabled]))
    before = m.rng_state()
    m.entropy = dict(tick_count=tick_count, system_time=(2026, 10, 6, 10, 14, 25, 53, 777))
    m.native_events.clear()
    m.invoke(0x52FC20, 0, context={"case": "campaign_rng", "override": override, "tick_count": tick_count,
                                "saved": saved, "replay": replay, "network": network, "mode": mode})
    seed = m.read32(0xA8ED94)
    after = m.rng_state()
    expected = seeded_struct(seed).hex()
    assert after["scenario"] == after["main"] == expected
    assert after["mapgen"] == before["mapgen"]
    m.check_code()
    return dict(override=override, tick_count=tick_count, saved=saved, replay=replay, network=network,
                mode=mode, prior_seed=prior_seed, mapgen_disabled=mapgen_disabled,
                seed=seed, rng_before=before, rng_after=after,
                entropy_events=m.entropy_events, native_calls=m.native_events)


def difficulty_case(option, basic):
    m = CampaignReader()
    m.seed_streams(31)
    m.u.mem_write(0xA8EB64, dwords(option))
    m.u.mem_write(m.scenario, dwords(0x89ABCDEF))
    m.u.mem_write(0xA8E960, dwords(0xFEDCBA98))
    m.ordered_ini({"Basic": basic} if basic else {})
    before = m.rng_state()
    m.u.reg_write(UC_X86_REG_EBP, INI)
    m.u.reg_write(UC_X86_REG_EBX, 0)
    m.region(0x686B6A, 0x686C48, required=(0x5276D0, 0x5295F0),
             context={"case": "campaign_Full_Init_difficulty", "option": option, "basic": basic})
    state = dict(player_difficulty=struct.unpack("<i", m.u.mem_read(m.scenario + 0x60C, 4))[0],
                 ai_difficulty=struct.unpack("<i", m.u.mem_read(m.scenario + 0x610, 4))[0],
                 scenario_special=m.read32(m.scenario), session_special=m.read32(0xA8E960),
                 init_time=struct.unpack("<i", m.u.mem_read(m.scenario + 0x359C, 4))[0],
                 official=bool(m.u.reg_read(UC_X86_REG_EAX) & 255))
    assert before == m.rng_state()
    m.check_code()
    return dict(option=option, basic=basic, state=state, rng_unchanged=True)


def prepare_house_rules(m, physical):
    """Original Rules constructor and the selected House prerequisites.

    The rest of Rules Process and complete palette generation are excluded.
    The color table preserves physical ordered names; its pixel storage is a
    presentation seam and no RGB output is certified by this fixture.
    """
    rules = m.alloc(0x2000)
    m.u.mem_write(0x8871E0, dwords(rules))
    m.invoke(0x665650, rules)
    # Rules Colors66D3A0 finds/allocates ctor68C710 pairs per ordered color:
    # shadecount1 at66D444, then53 at66D45B. ReadColorString474A90
    # excludes the first scheme; native House indexes refer to this paired array.
    names = [(name, shades) for name in physical["Colors"] for shades in (1, 53)]
    colors = m.alloc(len(names) * 4)
    for i, (name, shades) in enumerate(names):
        color, convert, pixels = m.alloc(0x400), m.alloc(0x200), m.alloc(512)
        m.u.mem_write(color + 0x304, dwords(m.cstring(name)))
        m.u.mem_write(color + 0x30C, dwords(convert))
        m.u.mem_write(color + 0x310, dwords(shades))
        m.u.mem_write(convert + 4, dwords(2))
        m.u.mem_write(convert + 0x174, dwords(pixels))
        m.u.mem_write(colors + i * 4, dwords(color))
    m.u.mem_write(0xB054D4, dwords(colors))
    m.u.mem_write(0xB054E0, dwords(len(names)))
    selected = {key: physical[key] for key in ("General", "AI", "IQ", "Easy", "Normal", "Difficult")}
    m.ordered_ini(selected)
    m.u.reg_write(UC_X86_REG_ESI, rules)
    m.u.reg_write(UC_X86_REG_EDI, INI)
    m.region(0x66FD6E, 0x66FDAE, required=(0x475D70, 0x67A090), context={"case": "retail_TeamDelays"})
    m.u.reg_write(UC_X86_REG_ESI, rules)
    m.u.reg_write(UC_X86_REG_EDI, INI)
    m.region(0x67015B, 0x67019A, required=(0x5276D0,), context={"case": "retail_CampaignMoney"})
    m.u.reg_write(UC_X86_REG_ESI, rules)
    m.u.reg_write(UC_X86_REG_EDI, INI)
    m.region(0x670B12, 0x670B39, required=(0x5283D0,), context={"case": "retail_GameSpeedBias"})
    m.u.reg_write(UC_X86_REG_ESI, rules)
    m.u.reg_write(UC_X86_REG_EDI, INI)
    m.region(0x673977, 0x67399E, required=(0x5283D0,), context={"case": "retail_AI_AttackDelay"})
    m.invoke(0x674240, rules, (INI,))
    for i, name in enumerate(("Easy", "Normal", "Difficult")):
        m.u.reg_write(UC_X86_REG_EDX, rules + 0x1538 + i * 0x50)
        m.invoke(0x66D270, INI, (m.cstring(name),))
    return rules


HOUSE_INTS = {"array_index": 0x30, "difficulty": 0x184, "iq": 0x1D0,
              "tech_level": 0x1D4, "scenario_credits": 0x1DC, "edge": 0x1E0,
              "side": 0x1E8, "balance": 0x30C, "ratio_ai_trigger_team": 0x565C,
              "ratio_aircraft": 0x5660, "ratio_infantry": 0x5664, "ratio_units": 0x5668,
              "initial_color_index": 0x16054, "current_house_clear": 0x56F4}
HOUSE_DOUBLES = {"firepower": 0x188, "groundspeed": 0x190, "airspeed": 0x198,
                 "armor": 0x1A0, "rof": 0x1A8, "cost": 0x1B0,
                 "build_time": 0x1B8, "repair_delay": 0x1C0, "build_delay": 0x1C8}


def house_state(m, p):
    country = m.read32(p + 0x34)
    sw = m.read32(p + 0x258)
    return dict(id=m.read32(p + 0x10), name=m.string(p + 0x15FF4),
                country=m.string(country + 0x24),
                alliance_mask=m.read32(p + 0x5788),
                initial_alliance_mask=m.read32(p + 0x1D8),
                **{key: struct.unpack("<i", m.u.mem_read(p + off, 4))[0]
                   for key, off in HOUSE_INTS.items()},
                doubles={key: bytes(m.u.mem_read(p + off, 8)).hex() for key, off in HOUSE_DOUBLES.items()},
                human=bool(m.u.mem_read(p + 0x1EC, 1)[0]),
                player_control=bool(m.u.mem_read(p + 0x1ED, 1)[0]),
                attack_timer=dict(start=struct.unpack("<i", m.u.mem_read(p + 0x55F0, 4))[0],
                                  duration=struct.unpack("<i", m.u.mem_read(p + 0x55F8, 4))[0],
                                  initial_delay=struct.unpack("<i", m.u.mem_read(p + 0x55FC, 4))[0]),
                team_timer=dict(start=struct.unpack("<i", m.u.mem_read(p + 0x5798, 4))[0],
                                duration=struct.unpack("<i", m.u.mem_read(p + 0x57A0, 4))[0]),
                super_ids=[m.read32(m.read32(sw + i * 4) + 0x10) for i in range(m.read32(p + 0x264))],
                supers=[super_state(m, m.read32(sw + i * 4)) for i in range(m.read32(p + 0x264))])


def super_state(m, p):
    type_ptr = m.read32(p + 0x28)
    return dict(id=m.read32(p + 0x10), type_name=m.string(type_ptr + 0x24),
        type_id=m.read32(type_ptr + 0x10), charge_timer=dict(
        start=struct.unpack("<i", m.u.mem_read(p + 0x30, 4))[0],
        duration=struct.unpack("<i", m.u.mem_read(p + 0x38, 4))[0]),
        granted=bool(m.u.mem_read(p + 0x6C, 1)[0]),
        one_time=bool(m.u.mem_read(p + 0x6D, 1)[0]),
        charged=bool(m.u.mem_read(p + 0x6E, 1)[0]),
        on_hold=bool(m.u.mem_read(p + 0x70, 1)[0]))


def house_case(filename, option, *, seed=31, sections_override=None,
               rule_sections_override=None, scenario_number=1,
               scenario_init_override=None):
    m = CampaignReader()
    m.invoke(0x6832C0, m.scenario)
    scenario_init_history = m.begin_scenario_init()
    if scenario_init_override is not None:
        m.u.mem_write(0xA8E7AC, dwords(scenario_init_override))
    m.u.mem_write(m.scenario + 0x1254, dwords(scenario_number))
    m.seed_streams(seed)
    physical_rules = sections((Path("ini") / "rulesmd.ini").read_bytes())
    for section, values in (rule_sections_override or {}).items():
        physical_rules[section].update(values)
    rules = prepare_house_rules(m, physical_rules)
    physical_map = sections((assets_root() / filename).read_bytes()) if filename else {}
    wanted_sections = {"Basic", "Countries", "Houses"} | set(physical_map.get("Countries", {}).values()) | set(physical_map.get("Houses", {}).values())
    values = {key: value for key, value in physical_map.items() if key in wanted_sections} if sections_override is None else sections_override
    csf_values, _ = stock_csf()
    labels = {keys["UIName"].upper() for layer in (physical_rules, values)
              for keys in layer.values() if "UIName" in keys and keys["UIName"].upper() in csf_values}
    csf = install_csf(m, labels)
    wanted_countries = list(physical_rules["Countries"].values())
    for name in values.get("Countries", {}).values():
        if name.lower() not in {x.lower() for x in wanted_countries}:
            wanted_countries.append(name)
    for name in physical_rules["Sides"]:
        m.invoke(0x6A4550, m.alloc(0xC0), (m.cstring(name),))
    countries = {}
    for name in wanted_countries:
        p = m.alloc(0x300)
        m.invoke(0x5113F0, p, (m.cstring(name),))
        countries[name] = p
    for layer in (physical_rules, values):
        m.ordered_ini({name: layer[name] for name in wanted_countries if name in layer})
        for p in countries.values():
            m.invoke(0x511850, p, (INI,))
    # Native list reader creates the physical stock superweapon type registry.
    m.ordered_ini({"SuperWeaponTypes": physical_rules["SuperWeaponTypes"]})
    m.invoke(0x6725F0, rules, (INI,))
    type_array = m.read32(0xA8E334)
    superweapon_types = [dict(id=m.read32(m.read32(type_array + i * 4) + 0x10),
                             name=m.string(m.read32(type_array + i * 4) + 0x24))
                        for i in range(m.read32(0xA8E340))]
    m.ordered_ini(values)
    m.u.mem_write(0xA8EB64, dwords(option))
    m.u.reg_write(UC_X86_REG_EBP, INI)
    m.u.reg_write(UC_X86_REG_EBX, 0)
    m.region(0x686B6A, 0x686C48)
    before = m.rng_state()
    id_before = m.read32(m.scenario + 0x214)
    m.native_events.clear()
    m.reads.clear()
    m.invoke_house_list({"case": "campaign_House_ReadINI", "filename": filename, "option": option})
    data, count = m.read32(0xA8022C), m.read32(0xA80238)
    houses = [house_state(m, m.read32(data + i * 4)) for i in range(count)]
    after = m.rng_state()
    assert after["main"] == before["main"] and after["mapgen"] == before["mapgen"]
    calls = list(m.native_events)
    m.u.reg_write(UC_X86_REG_ESI, m.scenario)
    m.u.reg_write(UC_X86_REG_EDI, INI)
    m.region(0x68ACAA, 0x68AD2F, required=(0x50C170,),
             context={"case": "campaign_Basic_Player_selection", "filename": filename})
    selected = m.read32(0xA83D4C)
    final_houses = [house_state(m, m.read32(data + i * 4)) for i in range(count)]
    assert after == m.rng_state()
    m.check_code()
    return dict(filename=filename, option=option, seed=seed, sections=values,
                rule_sections_override=rule_sections_override or {}, scenario_number=scenario_number,
                scenario_init_history=scenario_init_history,
                scenario_init=m.read32(0xA8E7AC),
                scenario_init_override=scenario_init_override,
                superweapon_types_section=physical_rules["SuperWeaponTypes"],
                superweapon_types=superweapon_types,
                id_before=id_before, id_after=m.read32(m.scenario + 0x214),
                mission_number=m.read32(m.scenario + 0x1254), csf=csf,
                rules=dict(team_delays=[m.read32(m.read32(rules + 0x115C) + i * 4)
                                       for i in range(m.read32(rules + 0x1168))],
                           ai_attack_delay_bits=bytes(m.u.mem_read(rules + 0x10A8, 8)).hex(),
                           game_speed_bias_bits=bytes(m.u.mem_read(rules + 0x1418, 8)).hex(),
                           easy_money=struct.unpack("<i", m.u.mem_read(rules + 0xDFC, 4))[0],
                           hard_money=struct.unpack("<i", m.u.mem_read(rules + 0xE00, 4))[0]),
                houses=houses, final_houses=final_houses,
                current_house_id=house_state(m, selected)["id"],
                current_house_name=house_state(m, selected)["name"],
                scenario_player_country=m.read32(m.scenario + 0x1C74),
                native_calls=calls, reads=m.reads,
                rng_before=before, rng_after=after)


def progress_rows():
    """Original setter/row/fill math, stopped at the terminal shape submission.

    The W glyph-table prior uses the existing shell_text_lines fixture's native
    font-data layout. Font raster, hidden Surface and converter are excluded.
    Original font selection, glyph lookup and GetTextWidth execute.
    """
    m = CampaignReader()
    meter, font, font_data = m.alloc(0x100), m.alloc(0x100), m.alloc(0x100)
    indices, glyph = m.alloc(0x20000), m.alloc(8)
    m.u.mem_write(font + 4, dwords(font_data))
    m.u.mem_write(font + 0x1C, dwords(17))
    m.u.mem_write(font + 0x2C, dwords(1))
    m.u.mem_write(font_data + 0x14, dwords(8, indices, glyph))
    m.u.mem_write(indices + ord("W") * 2, struct.pack("<H", 1))
    m.u.mem_write(glyph, bytes([8]))
    m.u.mem_write(0x89C4D0, dwords(font))
    raw = (assets_root() / "SPLDBR.SHP").read_bytes()
    shape = m.alloc(len(raw))
    m.u.mem_write(shape, raw)
    scheme, surface, point = m.alloc(0x400), m.alloc(0x100), m.alloc(8)
    m.u.mem_write(0x88730C, dwords(surface))
    m.u.mem_write(meter + 0x48, struct.pack("<d", 100.0))
    m.u.mem_write(meter + 0x54, dwords(shape))
    m.u.mem_write(meter + 0x61, b"\x01")
    m.u.mem_write(meter + 0x78, dwords(-1))
    measured = m.alloc(8)
    m.u.reg_write(UC_X86_REG_EDX, measured + 4)
    m.invoke(0x642E80, measured)
    font_size = list(struct.unpack("<2i", m.u.mem_read(measured, 8)))
    row_size_ptr = m.alloc(8)
    m.invoke(0x642EF0, meter, (row_size_ptr, row_size_ptr + 4, 1, 0))
    row_size = list(struct.unpack("<2i", m.u.mem_read(row_size_ptr, 8)))
    rows = []
    points = ((84, 447), (164, 567), (276, 651), (85, 507), (-74, -129))
    for p in points:
        for percent in range(101):
            m.u.mem_write(SP, dwords(RET_MAGIC, 0) + struct.pack("<d", float(percent)) + dwords(0, 0))
            m.u.reg_write(UC_X86_REG_ESP, SP)
            m.u.reg_write(UC_X86_REG_ECX, meter)
            run_checked(m.u, 0x643C50, (0x643D04, 0x643D50), count=2_000_000,
                        required_addresses=(0x643CA4,), context={"case": "campaign_progress_setter", "percent": percent})
            m.u.mem_write(point, dwords(*p))
            m.u.mem_write(SP, dwords(RET_MAGIC, scheme, m.cstring(""), point, 0, 0))
            m.u.reg_write(UC_X86_REG_ESP, SP)
            m.u.reg_write(UC_X86_REG_ECX, meter)
            run_checked(m.u, 0x643720, 0x643400, count=2_000_000,
                        required_addresses=(0x433ED0, 0x642EF0, 0x69E7E0),
                        context={"case": "campaign_progress_row", "point": p, "percent": percent})
            sp = m.u.reg_read(UC_X86_REG_ESP)
            fill = list(struct.unpack("<2i", m.u.mem_read(sp + 4, 8)))
            ratio = bytes(m.u.mem_read(sp + 12, 8)).hex()
            # Row's native local frame remains in place across the real call.
            row_base = sp + 4 + 24
            row_width = struct.unpack("<i", m.u.mem_read(row_base + 0x18, 4))[0]
            run_checked(m.u, 0x643400, 0x4AED70, count=2_000_000,
                        required_addresses=(0x7C5F00,),
                        context={"case": "campaign_progress_DrawFill", "point": p, "percent": percent})
            sp = m.u.reg_read(UC_X86_REG_ESP)
            args = struct.unpack("<14I", m.u.mem_read(sp + 4, 56))
            rows.append(dict(point=list(p), bar_size=[456, 25], font_size=font_size,
                             percent=percent, row_rect=[*p, row_width, row_size[1]],
                             draw_fill_origin=fill, ratio_bits=ratio,
                             draw_shape_point=list(struct.unpack("<2i", m.u.mem_read(args[2], 8))),
                             clip_rect=list(struct.unpack("<4i", m.u.mem_read(args[3], 16)))))
    m.check_code()
    return dict(shape_sha256=hashlib.sha256(raw).hexdigest(), supplied_font=dict(w_glyph_width=8, spacing=1, cell_height=17), rows=rows)


def basic_start_cases():
    rows = []
    for filename in ("all01umd.map", "sov01umd.map"):
        physical = sections((assets_root() / filename).read_bytes())
        for alternate in (0, 1):
            m = CampaignReader()
            m.ordered_ini({"Movies": sections((Path("ini") / "artmd.ini").read_bytes())["Movies"]})
            m.invoke(0x674550, 0, (INI,))
            m.invoke(0x6832C0, m.scenario)
            m.ordered_ini({name: physical[name] for name in ("Basic", "Waypoints", "SpecialFlags")})
            special_before = m.read32(m.scenario)
            m.invoke(0x6B8CA0, m.scenario, (INI,))
            special = m.read32(m.scenario)
            m.u.reg_write(UC_X86_REG_ESI, m.scenario)
            m.u.reg_write(UC_X86_REG_EDI, INI)
            m.region(0x68A071, 0x68A08F, required=(0x4757D0,))
            action = struct.unpack("<i", m.u.mem_read(m.scenario + 0x1444, 4))[0]
            m.u.reg_write(UC_X86_REG_ESI, m.scenario)
            m.u.reg_write(UC_X86_REG_EDI, INI)
            m.region(0x68A601, 0x68A656, required=(0x5276D0,))
            home, alt = struct.unpack("<2i", m.u.mem_read(m.scenario + 0x20C, 8))
            selected = alt if alternate else home
            # Original wsprintfA's decimal key bytes are the supplied Windows
            # formatting seam. Original ReadInt/divide/remainder still execute.
            m.u.mem_write(SP + 0x10, str(selected).encode("ascii") + b"\0")
            m.u.reg_write(UC_X86_REG_EBX, INI)
            m.u.reg_write(UC_X86_REG_ESI, selected)
            m.u.reg_write(UC_X86_REG_EDI, m.scenario + 0x632 + selected * 4)
            m.region(0x68BDE7, 0x68BE35, required=(0x5276D0,),
                     context={"case": "selected_campaign_Waypoint", "filename": filename, "index": selected})
            cell = list(struct.unpack("<2h", m.u.mem_read(m.scenario + 0x632 + selected * 4, 4)))
            m.u.mem_write(0x8A38E0, bytes([alternate]))
            m.region(0x684C9A, 0x684D2F,
                     context={"case": "campaign_opening_view_cell_center", "filename": filename, "alternate": alternate})
            # The native PUSH at684D25 moved ESP by4 before our stop.
            sp = m.u.reg_read(UC_X86_REG_ESP)
            coord = list(struct.unpack("<3i", m.u.mem_read(sp + 0x14, 12)))
            views = [list(struct.unpack("<2h", m.u.mem_read(m.scenario + off, 4)))
                     for off in (0x348E, 0x3492, 0x3496, 0x349A)]
            rows.append(dict(filename=filename, alternate=alternate,
                             sections={name: physical[name] for name in ("Basic", "Waypoints", "SpecialFlags")},
                             special_before=special_before, special_after=special,
                             action_movie=action, home_cell=home, alt_home_cell=alt,
                             selected_waypoint=selected, cell=cell, views=views,
                             coord_before_ground_height=coord))
            m.check_code()
    return rows


def special_controls():
    rows = []
    for name, values in (("absent", {}), ("wrong_case", {"specialflags": {"TiberiumGrows": "yes"}}),
                         ("keys_wrong_case", {"SpecialFlags": {"tiberiumgrows": "yes"}}),
                         ("grows", {"SpecialFlags": {"TiberiumGrows": "yes"}}),
                         ("spread_no", {"SpecialFlags": {"TiberiumSpreads": "no"}}),
                         ("malformed", {"SpecialFlags": {"TiberiumGrows": "junk", "TiberiumSpreads": "junk"}})):
        m = CampaignReader()
        m.invoke(0x6832C0, m.scenario)
        before = m.read32(m.scenario)
        m.ordered_ini(values)
        m.invoke(0x6B8CA0, m.scenario, (INI,))
        rows.append(dict(name=name, sections=values, before=before, after=m.read32(m.scenario), reads=list(m.reads)))
        m.check_code()
    return rows


def reset_control():
    m = CampaignReader()
    m.invoke(0x6832C0, m.scenario)
    m.u.mem_write(m.scenario, dwords(0x1234C0))
    m.u.mem_write(m.scenario + 0x20C, dwords(14, 14))
    m.u.mem_write(m.scenario + 0x1254, dwords(7))
    m.invoke(0x683610, m.scenario)
    row = dict(prior_special=0x1234C0, prior_home=[14, 14], prior_mission_number=7,
               special=m.read32(m.scenario),
               home=list(struct.unpack("<2i", m.u.mem_read(m.scenario + 0x20C, 8))),
               mission_number=m.read32(m.scenario + 0x1254))
    m.check_code()
    return row


def edge_controls():
    m = CampaignReader()
    rows = []
    for default in (-1, 2):
        for value in (None, "", "South", "south", "North", "West", "East", "2", "unknown", "South" + "X" * 150):
            values = {"Control": {} if value is None else {"Edge": value}}
            m.ordered_ini(values)
            output = m.invoke(0x475980, INI, (m.cstring("Control"), m.cstring("Edge"), default))
            rows.append(dict(sections=values, default=default,
                             result=struct.unpack("<i", dwords(output))[0]))
    m.check_code()
    return rows


def allies_reader_controls():
    m = CampaignReader()
    names = ["A", "B", "C"]
    array = m.read32(0xA8022C)
    for i, name in enumerate(names):
        p = m.alloc(0x16020)
        m.u.mem_write(p + 0x15FF4, (name + "\0").encode("ascii"))
        m.u.mem_write(array + i * 4, dwords(p))
    m.u.mem_write(0xA80238, dwords(len(names)))
    rows = []
    for default in (0, 4):
        for value in (None, "", "B,C", "B, C", " B,C ", "b", "missing",
                      "B,,C", "B\tC", "B;C", "A", "B,B", "X" * 126 + ",B"):
            values = {"Control": {} if value is None else {"Allies": value}}
            m.ordered_ini(values)
            output = m.invoke(0x475260, INI, (m.cstring("Control"), m.cstring("Allies"), default))
            rows.append(dict(sections=values, default=default, result=output))
    values = {"Control": {"allies": "B"}}
    m.ordered_ini(values)
    output = m.invoke(0x475260, INI, (m.cstring("Control"), m.cstring("Allies"), 4))
    rows.append(dict(sections=values, default=4, result=output))
    m.check_code()
    return dict(registry=names, delimiter_hex="2c00", rows=rows)


def color_controls():
    """Full474A90 and Country511850 with valid supplied paired schemes.

    The physical palette-name prior is the same owner as House execution.
    Extra31/32-byte names bound ReadString's copy limit; palette pixels and
    converters remain supplied presentation storage. No invalid pointer input
    is admitted, and the observer records rather than replaces ReadString.
    """
    m = CampaignReader()
    m.invoke(0x6832C0, m.scenario)
    physical = sections((Path("ini") / "rulesmd.ini").read_bytes())
    physical_colors = dict(physical["Colors"])
    synthetic_colors = {"Z" * 31: "0,0,0", "Y" * 32: "0,0,0"}
    physical["Colors"].update(synthetic_colors)
    prepare_house_rules(m, physical)
    install_csf(m, ("DESC:ALL1", "DESC:SOV1"))
    table = m.read32(0xB054D4)
    registry = []
    for index in range(m.read32(0xB054E0)):
        scheme = m.read32(table + index * 4)
        registry.append(dict(index=index, name=m.string(m.read32(scheme + 0x304)),
                             shade_count=m.read32(scheme + 0x310)))
    physical_empty = "[Control]\nName=Control\nColor=\n"
    inputs = [("missing", {"Control": {"Name": "Control"}}, None),
              ("stored_empty", {"Control": {"Name": "Control", "Color": ""}}, None),
              ("physical_empty", sections(physical_empty.encode("ascii")), physical_empty),
              ("unknown", {"Control": {"Color": "unknown"}}, None),
              ("case", {"Control": {"Color": "gOlD"}}, None),
              ("wrong_key_case", {"Control": {"color": "Gold"}}, None),
              ("wrong_section_case", {"control": {"Color": "Gold"}}, None),
              ("name31", {"Control": {"Color": "Z" * 31}}, None),
              ("copy32", {"Control": {"Color": "Z" * 31 + "R"}}, None),
              ("unmatchable_name32", {"Control": {"Color": "Y" * 32}}, None),
              ("numeric", {"Control": {"Color": "3"}}, None)]
    rows = []
    for current in (0, 3):
        for name, values, raw in inputs:
            m.ordered_ini(values)
            start = len(m.color_read_events)
            before_rng = m.rng_state()
            output = m.invoke(0x474A90, INI,
                              (m.cstring("Control"), m.cstring("Color"), current))
            assert before_rng == m.rng_state()
            rows.append(dict(name=name, sections=values, physical_ini=raw,
                             current=current, result=output,
                             read_calls=m.color_read_events[start:]))
            m.check_code()
    country_rows = []
    for current in (0, 3):
        for name, values, raw in [("whole_section_absent", {}, None)] + inputs:
            country = m.alloc(0x300)
            m.invoke(0x5113F0, country, (m.cstring("Control"),))
            constructor_color = m.read32(country + 0xC0)
            setup = {"Control": {"Name": "Control", "Color": "Gold"}} if current == 3 else {}
            if setup:
                m.ordered_ini(setup)
                m.invoke(0x511850, country, (INI,))
            before = m.read32(country + 0xC0)
            assert before == current
            m.ordered_ini(values)
            start = len(m.color_read_events)
            before_rng = m.rng_state()
            admitted = m.invoke(0x511850, country, (INI,))
            assert before_rng == m.rng_state()
            country_rows.append(dict(name=name, constructor_color=constructor_color,
                                     setup_sections=setup, sections=values, physical_ini=raw,
                                     before=before, admitted=bool(admitted & 255),
                                     after=m.read32(country + 0xC0),
                                     read_calls=m.color_read_events[start:]))
            m.check_code()
    return dict(physical_colors=physical_colors, synthetic_colors=synthetic_colors,
                registry=registry, rows=rows, country_rows=country_rows)


def side_controls():
    """Original ordered Country/Side kernels and their complete readers.

    Reuses this fixture's reader/cache/palette/allocator owners. The prefix
    executes original Countries registration668C8B..668CDB, full672440, then
    the original first Country ReadTypeData loop679A10..679A3A. Other Process
    families are excluded. Direct4767C0 controls stop before672440 could use
    a special negative index as a Country pointer; no such pointer is supplied.
    """
    physical = sections((Path("ini") / "rulesmd.ini").read_bytes())

    def ordered_sections(values):
        # JSON object order is not preserved by every consumer. Record the
        # exact supplied cache input order separately, including stored-empty
        # entries, without changing the dictionaries executed below.
        return [dict(name=name, entries=[[key, value] for key, value in entries.items()])
                for name, entries in values.items()]

    def fixture():
        m = CampaignReader()
        m.invoke(0x6832C0, m.scenario)
        m.seed_streams(31)
        rules = prepare_house_rules(m, physical)
        install_csf(m, ("DESC:ALL1", "DESC:SOV1"))
        return m, rules

    def signed(value):
        return struct.unpack("<i", dwords(value))[0]

    def vector(m, address):
        count = m.read32(address + 0x10)
        return list(struct.unpack(f"<{count}i", m.u.mem_read(m.read32(address + 4), count * 4))) if count else []

    def snapshot(m):
        countries, sides = [], []
        for index in range(m.read32(0xA83CA8)):
            ptr = m.read32(m.read32(0xA83C9C) + index * 4)
            countries.append(dict(index=index, id=m.string(ptr + 0x24),
                                  name=m.string(ptr + 0x64), native_id=m.read32(ptr + 0x10),
                                  color=m.read32(ptr + 0xC0), side=signed(m.read32(ptr + 0xBC))))
        for index in range(m.read32(0x8B4130)):
            ptr = m.read32(m.read32(0x8B4124) + index * 4)
            sides.append(dict(index=index, id=m.string(ptr + 0x24),
                              native_id=m.read32(ptr + 0x10), members=vector(m, ptr + 0x98)))
        return dict(id_cursor=m.read32(m.scenario + 0x214), countries=countries, sides=sides)

    def read_pass(m, rules, values):
        before, rng = snapshot(m), m.rng_state()
        event_start, read_start = len(m.side_native_events), len(m.side_read_events)
        m.ordered_ini(values)
        m.u.reg_write(UC_X86_REG_ESI, INI)
        m.u.reg_write(UC_X86_REG_EDI, rules)
        m.region(0x668C8B, 0x668CDB, context={"case": "Side_Countries_registration", "sections": values})
        registered = snapshot(m)
        admitted = m.invoke(0x672440, rules, (INI,), context={"case": "Side_membership", "sections": values})
        membership = snapshot(m)
        m.u.mem_write(SP, dwords(RET_MAGIC, INI))
        m.region(0x679A10, 0x679A3A, context={"case": "Side_Country_ReadTypeData", "sections": values})
        after = snapshot(m)
        assert rng == m.rng_state()
        m.check_code()
        return dict(sections=values, ordered_sections=ordered_sections(values),
                    before=before, registered=registered,
                    side_reader_admitted=bool(admitted & 255), membership=membership, after=after,
                    rng_unchanged=True, native_calls=m.side_native_events[event_start:],
                    read_calls=m.side_read_events[read_start:])

    prefix = "012345678901234567890123"
    histories = [
        ("orphan_and_late_side", [
            {"Countries": {"0": "Alpha"}, "Sides": {"GDI": "Alpha,Beta"},
             "Alpha": {"Name": "Alpha"}, "Beta": {"Side": "NewSide"}},
            {"Countries": {"1": "Beta"}, "Sides": {"GDI": "Alpha,Beta"},
             "Beta": {"Side": "NewSide"}}]),
        ("ordered_expansion_and_retention", [
            {"Countries": {"0": "Alpha", "1": "Beta", "2": "Gamma"},
             "Sides": {"First": "Alpha,Beta,Beta", "Later": "First,Gamma",
                       "EarlierRef": "Future", "Future": "Beta", "Self": "Alpha"}},
            {"Sides": {"Self": "Self,First", "First": "Gamma", "Later": "First,Self,Future"}},
            {"Sides": {"Self": "", "First": ",,,", "Later": "unknown, Alpha ,Beta,missing", "Future": ""}},
            {"Other": {"Key": "value"}},
            {"sides": {"First": "Alpha"}, "Alpha": {"side": "Unreached"}}]),
        ("aliases_at_read_time", [
            {"Countries": {"0": "Alpha", "1": "Beta"}, "Sides": {"GDI": "oldAlias"},
             "Alpha": {"Name": "oldAlias"}, "Beta": {"Name": "Alpha"}},
            {"Sides": {"GDI": "oldAlias,Alpha,newAlias"}, "Alpha": {"Name": "newAlias"}},
            {"Sides": {"GDI": "oldAlias,newAlias,Alpha"}, "Beta": {"Name": "Beta"}}]),
        ("late_override_retains_actual_vectors", [
            {"Countries": {"0": "Alpha", "1": "Beta", "2": "Gamma"},
             "Sides": {"A": "Alpha,Beta", "B": "Gamma"}, "Alpha": {"Side": "Late"}},
            {"Sides": {"A": ""}, "Alpha": {"Name": "Alpha"}},
            {"Alpha": {"Side": "Other"}, "Beta": {"Side": "Other"}},
            {"Sides": {"Copy": "Late,Other,A"}}]),
        ("raw_membership_key_and_stored_id", [
            {"Countries": {"0": "Alpha", "1": "Beta"},
             "Sides": {prefix + "A": "Alpha", prefix + "B": "Beta"}},
            {"Sides": {prefix: "Alpha", prefix + "A": "Beta"}}]),
        ("side_keys_keep_none_spellings", [
            {"Sides": {"none": "Americans", "<NONE>": "British", "NONE": "French"}},
            {"Countries": {"0": "Americans", "1": "British", "2": "French"},
             "Sides": {"none": "Americans", "<NONE>": "British", "NONE": "French"}},
            {"Sides": {"Copy": "<none>,none"}}]),
    ]
    history_rows = []
    for name, passes in histories:
        m, rules = fixture()
        history_rows.append(dict(name=name, initial=snapshot(m),
                                 passes=[read_pass(m, rules, values) for values in passes]))

    lookup_rows = []
    lookup_setup = {"Countries": {"0": "Alpha", "1": "Beta", "2": "Gamma"},
                    "Alpha": {"Name": "sharedAlias"}, "Beta": {"Name": "Alpha"},
                    "Gamma": {"Name": "sharedAlias"}}
    for populated in (False, True):
        m, rules = fixture()
        if populated:
            read_pass(m, rules, lookup_setup)
        for token in ("", "Alpha", "aLpHa", "Beta", "sharedAlias", "Alpha ",
                      "<random>", "<RaNdOm>", "<none>", "none", "missing"):
            before, rng = snapshot(m), m.rng_state()
            output = m.invoke(0x5117D0, m.cstring(token))
            assert before == snapshot(m) and rng == m.rng_state()
            lookup_rows.append(dict(setup_sections=lookup_setup if populated else {},
                                    ordered_setup_sections=ordered_sections(lookup_setup if populated else {}),
                                    token=token, before=before, result=signed(output), rng_unchanged=True))
            m.check_code()

    list_rows = []
    list_setup = {"Countries": {"0": "Alpha", "1": "Beta"},
                  "Sides": {"Prior": "Alpha,Beta,Beta", "Other": "Beta"}}
    physical_empty = "[Control]\nMembers=\n"
    list_inputs = [("missing", {}, None), ("stored_empty", {"Control": {"Members": ""}}, None),
                   ("physical_empty", sections(physical_empty.encode("ascii")), physical_empty),
                   ("commas", {"Control": {"Members": ",,,"}}, None),
                   ("duplicates_and_unknown", {"Control": {"Members": "Beta,unknown,Alpha,Beta"}}, None),
                   ("existing_side_and_self", {"Control": {"Members": "Other,Prior"}}, None),
                   ("token_spaces", {"Control": {"Members": " Alpha ,Beta, Beta"}}, None),
                   ("wrong_key_case", {"Control": {"members": "Alpha"}}, None),
                   ("wrong_section_case", {"control": {"Members": "Alpha"}}, None),
                   ("buffer128", {"Control": {"Members": "unknown," * 15 + "Alpha,Beta"}}, None),
                   ("random_special", {"Control": {"Members": "<random>,<RaNdOm>,<none>,none"}}, None)]
    for populated in (False, True):
        m, rules = fixture()
        if populated:
            read_pass(m, rules, list_setup)
        for name, values, raw in list_inputs:
            # The by-value default is the actual Prior Side vector. Reuse
            # original477B60 for its base copy; the extra count/growth fields
            # are transported from that original vector, as6724E8..6724F2.
            default = m.alloc(0x1C)
            if populated:
                side = m.read32(m.read32(0x8B4124))
                m.invoke(0x477B60, default, (side + 0x98,))
                m.u.mem_write(default + 0x10, bytes(m.u.mem_read(side + 0xA8, 8)))
            else:
                m.invoke(0x477BE0, default, (0, 0))
                m.u.mem_write(default + 0x14, dwords(10))
            before, rng = snapshot(m), m.rng_state()
            prior = vector(m, default)
            m.ordered_ini(values)
            read_start = len(m.side_read_events)
            output = m.alloc(0x1C)
            m.invoke(0x4767C0, INI,
                     (output, m.cstring("Control"), m.cstring("Members"),
                      *struct.unpack("<7I", m.u.mem_read(default, 0x1C))))
            assert before == snapshot(m) and rng == m.rng_state()
            list_rows.append(dict(name=name, setup_sections=list_setup if populated else {},
                                  ordered_setup_sections=ordered_sections(list_setup if populated else {}),
                                  sections=values, ordered_sections=ordered_sections(values),
                                  physical_ini=raw, before=before, default=prior,
                                  result=vector(m, output), read_calls=m.side_read_events[read_start:],
                                  rng_unchanged=True))
            m.check_code()

    side_reader_rows, scenario_reader_rows = [], []
    scalar_inputs = [("missing", {}), ("stored_empty", {"Control": {"Value": ""}}),
                     ("known", {"Control": {"Value": "Prior"}}),
                     ("case", {"Control": {"Value": "pRiOr"}}),
                     ("unknown", {"Control": {"Value": "NewSide"}}),
                     ("none", {"Control": {"Value": "<none>"}}),
                     ("wrong_key_case", {"Control": {"value": "NewSide"}}),
                     ("copy128", {"Control": {"Value": "S" * 128}})]
    for current in (-1, 0):
        for name, values in scalar_inputs:
            m, rules = fixture()
            read_pass(m, rules, list_setup)
            before, rng = snapshot(m), m.rng_state()
            m.ordered_ini(values)
            read_start, event_start = len(m.side_read_events), len(m.side_native_events)
            output = m.invoke(0x4756F0, INI,
                              (m.cstring("Control"), m.cstring("Value"), current & 0xFFFFFFFF))
            assert rng == m.rng_state()
            side_reader_rows.append(dict(name=name, setup_sections=list_setup,
                                         ordered_setup_sections=ordered_sections(list_setup),
                                         sections=values, ordered_sections=ordered_sections(values),
                                         current=current, result=signed(output), before=before,
                                         after=snapshot(m), native_calls=m.side_native_events[event_start:],
                                         read_calls=m.side_read_events[read_start:], rng_unchanged=True))
            m.check_code()
    for token in (None, "", "Alpha", "sharedAlias", "<random>", "<RaNdOm>",
                  "<none>", "none", "<Player @ A>", "<player @ a>", "unknown"):
        m, rules = fixture()
        read_pass(m, rules, lookup_setup)
        before, rng = snapshot(m), m.rng_state()
        values = {"Control": {"Country": token}} if token is not None else {}
        m.ordered_ini(values)
        event_start = len(m.side_native_events)
        output = m.invoke(0x475540, INI,
                          (m.cstring("Control"), m.cstring("Country"), 0xFFFFFFFF))
        assert rng == m.rng_state()
        scenario_reader_rows.append(dict(setup_sections=lookup_setup,
                                         ordered_setup_sections=ordered_sections(lookup_setup),
                                         sections=values, ordered_sections=ordered_sections(values),
                                         current=-1, result=signed(output), before=before,
                                         after=snapshot(m), native_calls=m.side_native_events[event_start:],
                                         rng_unchanged=True))
        m.check_code()

    trigger_type_reader_rows = []
    trigger_id = "Control"
    trigger_physical_empty = "[Triggers]\nControl=\n"
    trigger_inputs = [
        ("missing_section", {}, None),
        ("missing_key", {"Triggers": {"Other": "Alpha"}}, None),
        ("stored_empty", {"Triggers": {trigger_id: ""}}, None),
        ("physical_empty", sections(trigger_physical_empty.encode("ascii")), trigger_physical_empty),
        ("wrong_section_case", {"triggers": {trigger_id: "Alpha"}}, None),
        ("wrong_key_case", {"Triggers": {"control": "Alpha"}}, None),
        ("none_literal", {"Triggers": {trigger_id: "<none>"}}, None),
        ("none_upper", {"Triggers": {trigger_id: "<NONE>"}}, None),
        ("none_mixed", {"Triggers": {trigger_id: "<NoNe>"}}, None),
        ("none_outer_spaces", {"Triggers": {trigger_id: "   <NoNe>   "}}, None),
        ("none_leading_delimiters", {"Triggers": {trigger_id: ",,<none>,ignored"}}, None),
        ("none_token_trailing_space", {"Triggers": {trigger_id: "<none> ,ignored"}}, None),
        ("none_word", {"Triggers": {trigger_id: "none"}}, None),
        ("id", {"Triggers": {trigger_id: "Beta"}}, None),
        ("id_case", {"Triggers": {trigger_id: "bEtA"}}, None),
        ("alias", {"Triggers": {trigger_id: "sharedAlias"}}, None),
        ("id_before_later_alias", {"Triggers": {trigger_id: "Alpha"}}, None),
        ("first_token_only", {"Triggers": {trigger_id: "Beta,unknown,<none>"}}, None),
        ("copy512", {"Triggers": {trigger_id: "Alpha," + "x" * 520}}, None),
        ("unknown", {"Triggers": {trigger_id: "missing"}}, None),
        ("random", {"Triggers": {trigger_id: "<random>"}}, None),
        ("random_case", {"Triggers": {trigger_id: "<RaNdOm>"}}, None),
    ]
    trigger_cases = [(name, [lookup_setup], values, raw) for name, values, raw in trigger_inputs]
    trigger_cases.extend([
        ("retained_new_alias", [lookup_setup, {"Alpha": {"Name": "newAlias"}}],
         {"Triggers": {trigger_id: "newAlias"}}, None),
        ("retained_old_alias_resolves_later_country", [lookup_setup, {"Alpha": {"Name": "newAlias"}}],
         {"Triggers": {trigger_id: "sharedAlias"}}, None),
        ("retained_removed_alias", [lookup_setup, {"Alpha": {"Name": "newAlias"},
                                                    "Gamma": {"Name": "otherAlias"}}],
         {"Triggers": {trigger_id: "sharedAlias"}}, None),
    ])
    for name, passes, values, raw in trigger_cases:
        m, rules = fixture()
        setup_passes = [read_pass(m, rules, value) for value in passes]
        before, rng = snapshot(m), m.rng_state()
        array = m.read32(0xA83C9C)
        country_pointers = [m.read32(array + index * 4) for index in range(m.read32(0xA83CA8))]
        prior_owner_index = 1
        # Only the stored Type ID and a valid prior owner pointer are supplied.
        # The original ReadINI prologue, cache clear, ReadString512, strtok,
        # literal comparison, Country scan and +A4 write execute unchanged.
        trigger = m.alloc(0xB4)
        m.u.mem_write(trigger, bytes(0xB4))
        m.u.mem_write(trigger + 0x24, trigger_id.encode("ascii") + b"\0")
        m.u.mem_write(trigger + 0xA4, dwords(country_pointers[prior_owner_index]))
        m.ordered_ini(values)
        read_start, event_start = len(m.trigger_read_events), len(m.side_native_events)
        m.u.mem_write(SP, dwords(RET_MAGIC, INI))
        m.u.reg_write(UC_X86_REG_ECX, trigger)
        context = {"case": "TriggerType_country_owner_prefix", "name": name, "sections": values}
        m.region(0x727240, (0x7272BB, 0x7272D1, 0x7275B4), context=context)
        boundary = m.u.reg_read(UC_X86_REG_EIP)
        first_token = None if boundary == 0x7275B4 else m.string(m.u.reg_read(UC_X86_REG_ESI))
        lookup_result = signed(m.u.reg_read(UC_X86_REG_EAX)) if boundary == 0x7272BB else None
        if boundary == 0x7272BB and 0 <= lookup_result < len(country_pointers):
            # Resume the contiguous native region without altering stack,
            # registers or locals. Never execute a negative array index.
            run_checked(m.u, boundary, 0x7272D1, count=2_000_000,
                        required_addresses=(0x7272CB,), context=context)
            boundary = m.u.reg_read(UC_X86_REG_EIP)
        binding_executed = boundary == 0x7272D1
        owner_index_after = country_pointers.index(m.read32(trigger + 0xA4))
        outcome = ("bound_country" if binding_executed else
                   "missing_entry" if boundary == 0x7275B4 else "excluded_negative_index")
        assert before == snapshot(m) and rng == m.rng_state()
        if not binding_executed:
            assert owner_index_after == prior_owner_index
        if outcome == "excluded_negative_index":
            assert lookup_result in (-1, -2)
        trigger_type_reader_rows.append(dict(
            name=name, setup_passes=setup_passes, sections=values,
            ordered_sections=ordered_sections(values), physical_ini=raw, trigger_id=trigger_id,
            prior_owner_index=prior_owner_index, before=before, after=snapshot(m),
            first_token=first_token, lookup_result=lookup_result, outcome=outcome,
            boundary=f"{boundary:08x}", binding_executed=binding_executed,
            owner_index_after=owner_index_after,
            resolved_index=owner_index_after if binding_executed else None,
            read_calls=m.trigger_read_events[read_start:],
            native_calls=m.side_native_events[event_start:], rng_unchanged=True))
        m.check_code()
    return dict(palette_sections={"Colors": physical["Colors"]},
                ordered_palette_sections=ordered_sections({"Colors": physical["Colors"]}), histories=history_rows,
                lookup_rows=lookup_rows, list_rows=list_rows, side_reader_rows=side_reader_rows,
                scenario_reader_rows=scenario_reader_rows, trigger_type_reader_rows=trigger_type_reader_rows,
                excluded="Full672440 and TriggerType727240 negative Country-index binding and out-of-range/default pointers; TriggerType constructor and parsing after7272D1; complete Rules Process and full scenario loading.")


def house_controls():
    rows = []
    for name, option, credits, controlled, tech in (
        ("negative_easy", 0, "-1", "yes", None), ("negative_ai", 0, "-1", "no", None),
        ("hard_clamp", 2, "1", "yes", None), ("overflow", 0, "21474837", "yes", None),
        ("missing_tech", 1, "16", "yes", None), ("malformed_tech", 1, "16", "yes", "junk"),
        ("hex_tech", 1, "16", "yes", "10h"), ("prefix_hex_tech", 1, "16", "yes", "0x10")):
        house = {"Country": "Americans", "Credits": credits, "PlayerControl": controlled}
        if tech is not None:
            house["TechLevel"] = tech
        row = house_case(None, option, sections_override={"Basic": {"Player": "Control"},
                         "Houses": {"0": "Control"}, "Control": house},
                         rule_sections_override={"General": {"CampaignMoneyDeltaEasy": "5000", "CampaignMoneyDeltaHard": "-5000"}},
                         scenario_number=7)
        row["name"] = name
        rows.append(row)
    row = house_case(None, 2, sections_override={"Basic": {"Player": "missing"}})
    row["name"] = "missing_houses_fallback"
    rows.append(row)
    row = house_case(None, 1, sections_override={"Basic": {"Player": "A"},
        "Houses": {"0": "A", "1": "B", "2": "C"},
        "A": {"Country": "Americans", "Allies": "B"},
        "B": {"Country": "Americans"}, "C": {"Country": "Americans"}})
    row["name"] = "one_way_allies"
    rows.append(row)
    row = house_case(None, 1, sections_override={"Basic": {"Player": "A"},
        "Houses": {"0": "A", "1": "B", "2": "C"},
        "A": {"Country": "Americans", "Allies": "B,C"},
        "B": {"Country": "Americans"}, "C": {"Country": "Americans"}},
        scenario_init_override=0)
    row["name"] = "zero_counter_live_admission"
    rows.append(row)
    return rows


def generate_houses():
    return dict(schema_version=1, native_sha256=image_sha256(),
                stock_rows=[house_case(name, option) for name in ("all01umd.map", "sov01umd.map") for option in range(3)],
                controls=house_controls(), edge_controls=edge_controls(),
                allies_reader_controls=allies_reader_controls(), color_controls=color_controls(),
                side_controls=side_controls())


def generate():
    return dict(schema_version=1, native_sha256=image_sha256(), metadata=metadata_history(),
                metadata_controls=metadata_controls(), movies=movie_catalog(),
                loading_geometry=loading_geometry(), mission_loading=mission_loading(),
                progress_rows=progress_rows(), basic_start=basic_start_cases(),
                special_controls=special_controls(), reset_control=reset_control(),
                rng_cases=[rng_case(override=override, tick_count=tick) for override, tick in
                           ((0, 0), (0, 1), (0, 31), (1, 99), (0x12345678, 99))] +
                          [rng_case(override=99, tick_count=23, **flag) for flag in
                           ({"saved": 1}, {"replay": 2}, {"network": 1}, {"mode": 3})] +
                          [rng_case(override=99, tick_count=23, mapgen_disabled=1)],
                difficulty_cases=[difficulty_case(option, basic) for option in range(5)
                                  for basic in ({}, {"InitTime": "-1", "Official": "yes"},
                                                {"InitTime": "junk", "Official": "junk"})])


def metadata(*, houses=False):
    entry_points = {"campaign_list": 0x46CE10, "campaign_ctor": 0x46CB60,
        "campaign_read": 0x46CCD0, "campaign_index": 0x46CC90,
        "movies_read": 0x674550, "read_movie": 0x4757D0, "movie_from_name": 0x48DF30,
        "rng_init": 0x52FC20, "rng_seed": 0x65C6D0, "random_straw_ctor": 0x6616B0,
        "scenario_ctor": 0x6832C0, "scenario_defaults": 0x683610,
        "difficulty_prefix": 0x686B6A, "mission_loading_reader": 0x687000,
        "loading_layout": 0x552B10, "progress_point": 0x552BE0,
        "progress_font_size": 0x642E80, "progress_row_size": 0x642EF0,
        "progress_setter": 0x643C50, "progress_row": 0x643720, "progress_fill": 0x643400,
        "basic_action": 0x68A071, "basic_home_cells": 0x68A601,
        "waypoint_reader_kernel": 0x68BDE7, "opening_view_kernel": 0x684C9A,
        "special_flags_read": 0x6B8CA0}
    assumptions = [
        "Original constructors, readers, numeric and RNG instructions execute without patching; all executable PE sections are compared byte-for-byte after every case.",
        "The inherited INI owners supply lexical CRC and source-order cache storage. OS file/archive admission, physical INI parsing, dialogs and full scenario/world loading are excluded. Synthetic stored-empty controls explicitly use cache projection, not physical parsing.",
        "Campaign GameMode0, retained Scenario storage and native-seeded Main/Scenario/MapGen states are fixture priors. Full ClearScene, complete Rules Process, type-allocation counts, Resize, Fill, object publication and first live frame are not executed here.",
        "The ordinary campaign difficulty selector exposes0..2. Full_Init prefix-only direct inputs3/4 do not certify later out-of-range difficulty table reads.",
        "RNG rows execute the entire52FC20 initializer plus original RandomStraw constructor/seed/get. GetSystemTime receives the supplied8-word SYSTEMTIME and GetTickCount the supplied32-bit value; the complete MapGen raw state, including disabled byte, is retained.",
        "Loading layout rows cover mode0/5, even, odd and negative-centered surfaces. Progress rows cover every integer0..100 over physical456x25 SPLDBR at five supplied meter points; native font select, W glyph lookup, width, row-height, ratio and clip math execute. They stop at terminal CC_DrawShape without supplying its return or claiming raster output.",
        "Basic.Action, HomeCell/AltHomeCell, SpecialFlags and selected Waypoint readers execute. Opening view stops before world ground-height lookup and Radar center; no projected camera/raster output is claimed."
    ]
    substitutions = [
        "Inherited BulletReader bounded allocation/delete and CRT TLS transport; runtime empty-default889F64 and spare native vector storage supplied.",
        "Existing original CRT OS atomic import and millisecond clock transport; Windows entropy imports supplied only at their original call sites.",
        "Initialized selected retail CSF table storage supplied through existing physical CSF decoder; original string-table lookup executes.",
        "Progress font prior is W width8, spacing1 and cell height17 using existing shell_text_lines native layout; hidden Surface/converter pointers are supplied terminal presentation storage.",
        "Windows wsprintfA selected decimal Waypoint key bytes supplied; original ReadInt, quotient/remainder and cell-center arithmetic execute."
    ]
    if houses:
        entry_points = {"scenario_ctor": 0x6832C0, "rules_ctor": 0x665650,
            "read_team_delays": 0x66FD6E, "read_campaign_money": 0x67015B,
            "read_game_speed_bias": 0x670B12, "read_ai_attack_delay": 0x673977,
            "read_iq": 0x674240, "read_difficulty": 0x66D270,
            "country_ctor": 0x5113F0, "country_read": 0x511850,
            "side_ctor": 0x6A4550, "superweapon_types": 0x6725F0,
            "difficulty_prefix": 0x686B6A, "house_list": 0x5009B0,
            "house_ctor": 0x4F54A0, "house_read_scenario": 0x500B40,
            "set_difficulty": 0x4F6EC0, "super_ctor": 0x6CAF90,
            "current_house_kernel": 0x68ACAA, "read_edge": 0x475980,
            "read_scenario_init": 0x68467C, "full_init_nesting": 0x686B35,
            "read_houses_list": 0x475260, "house_from_name": 0x50C170,
            "make_ally": 0x4F9B70, "can_ally": 0x501540}
        entry_points["read_color"] = 0x474A90
        entry_points.update(country_registration_kernel=0x668C8B, read_sides=0x672440,
                            read_country_list=0x4767C0, find_country_index=0x5117D0,
                            read_side=0x4756F0, country_read_type_data=0x679A10,
                            read_house_type=0x475540, special_house_token=0x510FB0,
                            special_house_index=0x510F60, create_id=0x410230,
                            next_id=0x68BCB0, trigger_type_owner_prefix=0x727240,
                            trigger_type_owner_write=0x7272CB)
        assumptions = assumptions[:3] + [
            "Six physical first-map cases cover ALL/SOV and ordinary selector options0..2. Selected native Rules prerequisites, registered Country/Side/SuperWeaponType readers and the whole original House list/read/SetDifficulty bodies execute before original Basic.Player selection.",
            "House list execution is split at actual outer call/return boundaries solely to satisfy the shared instruction cap. Registers, locals and stack are retained between contiguous regions; no gameplay callback returns are substituted.",
            "Raw Main and MapGen states must remain unchanged. Entire Scenario RNG before/after storage and native Range call/return order, ctor timers, team timers, House/Super IDs, all nine stored difficulty scalars and post-current-house fields are observed.",
            "IDs begin at the actual Scenario constructor prior. Complete Rules Process allocation prefix and Map Resize are excluded; these rows establish only the one House/Super allocation segment and its delta.",
            "Credits controls cover signed wrapping times100, Easy/Hard money addition and signed clamp only for PlayerControl at read time, later Basic.Player force, TechLevel current mission default/malformed/hex and no-Houses registration fallback.",
            "Original ReadScenario and Full_Init nesting-counter kernels establish0->1->2 before the House pass. Native current and initial alliance masks and MakeAlly caller arguments are observed under this initialization prior. A distinct direct counter-zero control characterizes live admission and is not a stock startup prior.",
            "A three-House one-way authored list distinguishes directional map admission from reciprocal alliance construction. ReadHousesList475260 controls execute the full original128-byte reader, comma-only strtok and exact-case House-name resolver against an explicitly supplied A/B/C registry.",
            "ReadEdge controls cover absent/stored-empty, case-insensitive named edges, numeric/unknown/long strings and caller defaults-1/2.",
            "ReadColor474A90 controls use valid current scheme indexes0/3, missing versus stored-empty and physical-empty cache inputs, named case, exact key/section case, unknown/numeric values and31/32-byte copy boundaries. Full Country511850 controls retain constructor0 or a native-read Gold prior3 and expose whole-section admission. Original ReadString arguments, count and resulting buffer are observed without substituting its return.",
            "Side controls execute original Country-registration668C8B..668CDB, full672440, then the original679A10..679A3A Country ReadTypeData loop. Six retained histories observe immediate perSide reads, prior-name aliases, missing/stored-empty list retention, duplicate and unknown tokens, existingSide/self expansion, raw key versus stored24-byte ID, and later4756F0 Side creation. Other Rules Process families are excluded; observed native IDs belong only to this bounded prefix.",
            "Direct5117D0 and4767C0 controls retain signed special-2 for the case-insensitive literal<random>. Full672440 subsequently dereferences that index as a Country pointer, so negative binding and out-of-range/default pointers are excluded, never normalized to an ordinary index. ReadHouseType475540 controls stop at the reader result and include exact-case<Player @ A> routing and unknown-Country construction; later House pointer admission is excluded. No Side/Country control changes any RNG stream.",
            "TriggerType727240 owner-prefix controls use actual retained Country setup passes and exact Triggers entry lookup. Valid first tokens execute the original owner-pointer write7272CB and stop before7272D1 processes the next token. The caller's case-insensitive<none> literal chooses Country vector0 before5117D0; ordinary IDs/aliases use the full scan. Missing/count-zero entries stop at7275B4 with the prior pointer unchanged. Unknown-1/random-2 stop BEFORE array-pointer dereference7272BB; no safe native binding or complete TriggerType/action execution is claimed."
        ]
        substitutions = substitutions[:3] + [
            "Palette table storage follows original66D3A0 paired ColorScheme ctor order (shade counts1/53) and physical ordered Colors names. Isolated Color controls append explicitly supplied31/32-byte names to that same table. Pixel and converter storage are supplied; full palette generation and RGB output are excluded.",
            "Standalone ReadHousesList controls supply only the three stored House names and registry membership; full House constructors are executed in stock and House controls, not in those isolated name-reader controls.",
            "Direct4767C0 uses a by-value default copied from an actually constructed/read Side vector through original477B60. Count/growth fields are transported unchanged from that vector as the original6724E8..6724F2 caller does. The empty prior uses original477BE0. No lookup, constructor, membership or ID return is supplied.",
            "TriggerType owner-prefix controls supply only bounded Type storage, its stored ID and a valid prior Country pointer. The constructor, remaining Type fields and later parsing are excluded. Original INI cache clear526B00, ReadString512 arguments/return, comma tokenization and Country-pointer selection execute without supplied gameplay returns."
        ]
    suffix = " --houses" if houses else ""
    inputs = ("all01umd.map", "sov01umd.map") if houses else ("battlemd.ini", "missionmd.ini", "all01umd.map", "sov01umd.map", "SPLDBR.SHP")
    paths = {name: assets_root() / name for name in inputs}
    paths.update({name: Path("ini") / name for name in ("rulesmd.ini", "artmd.ini")})
    result = provenance(scope=__doc__, entry_points=entry_points,
                        assumptions=assumptions, substitutions=substitutions)
    result["input_sha256"] = {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in paths.items()}
    result["csf_sha256"] = stock_csf()[1]
    result["reproduce_command"] = ("RA2_DIR='<retail-directory-containing-original-gamemd.exe>' "
        "VERA20K_CAMPAIGN_START_ASSETS='<extracted-retail-campaign-directory>' "
        "python -m tools.input_oracle.campaign_start" + suffix + " --check")
    return result


def source_paths():
    paths = {"producer": Path(__file__), "native_runner": HERE.parent / "native_oracle.py",
        "native_inspect": HERE.parent / "native_inspect.py",
        "reader": HERE.parent / "projectile_oracle/bridge_render_inputs.py",
        "reader_base": HERE.parent / "rules_oracle/bridge_anim_inputs.py",
        "reader_lists": HERE.parent / "rules_oracle/bridge_anim_lists.py",
        "fixture_base": HERE.parent / "spatial_oracle/building_body_rules.py",
        "source_order": HERE.parent / "rules_oracle/bridge_child_sound.py",
        "rng": HERE.parent / "rmg_oracle/gen_rng_vectors.py",
        "csf": HERE.parent / "storage_oracle/keyboard_bindings.py",
        "csf_mix": HERE.parent / "sidebar_oracle/stock.py",
        "crc": HERE.parent / "projectile_oracle/flat_art.py",
        "atomic_import": HERE.parent / "spatial_oracle/infantry_deploy_action.py",
        "clock_import": HERE / "fast_scroll.py",
        "font_prior_reference": HERE.parent / "storage_oracle/shell_text_lines.py"}
    return paths


if __name__ == "__main__":
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--houses", action="store_true")
    args, remaining = parser.parse_known_args()
    finish_vectors(generate_houses if args.houses else generate,
                   HERE / ("campaign_start_houses.json" if args.houses else "campaign_start.json"),
                   provenance=lambda: metadata(houses=args.houses), argv=remaining,
                   source_paths=source_paths())
