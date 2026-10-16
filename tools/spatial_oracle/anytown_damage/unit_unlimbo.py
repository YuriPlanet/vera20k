"""Original MTNK placement, authored HIGH caller and whole Building ExitObject controls.

Mission remains the constructor, physical-input and runtime boundary owner.
This fixture changes only declared data inputs and observes original returns.
The naval MTNK control deliberately is not a legal retail Ship production case.
"""
from pathlib import Path
import hashlib
import json
import os
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EBP, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_EFLAGS,
    UC_X86_REG_FPCW,
)

from tools.native_oracle import (
    NATIVE_SHA256, RET_MAGIC, SCRATCH, call as native_call, file_span, finish_vectors, image_bytes, provenance, run_checked,
)
from tools.spatial_oracle.building_body_rules import SP, RULES, dwords
from . import foot_missions
from .navigation_inputs import bind_primary_tiles
from .mission import HERE, Mission, base

REPO = Path(__file__).resolve().parents[3]
GEOMETRY_CALLS = (0x735180, 0x735210, 0x735230, 0x735250, 0x7352F0)
DIRECT_PCS = (
    0x737BA0, 0x4D7170, 0x6F6CA0, 0x5F4EC0, 0x73F0A0, 0x4D9C10,
    0x55ABF0, 0x4D9C60, 0x4DB810, 0x4D3780, 0x747EB0, 0x5F4F42, 0x5F4FB4,
)
FACTORY_PCS = (
    0x443C60, 0x443C81, 0x443C88, 0x444565, 0x444575, 0x44457B,
    0x44F640, 0x44458C, 0x444592, 0x444400, 0x44440C, 0x444412,
    0x444445, 0x44447A, 0x44459F, 0x4445BD, 0x4445C9, 0x4445D6,
    0x4445E3, 0x4445F0, 0x444971, 0x444979, 0x44497E, 0x444EDE,
    0x444EE6, 0x444EEB, 0x737BA0, 0x4D7170, 0x6F6CA0, 0x5F4EC0,
    0x73F0A0, 0x73F34C, 0x73F405, 0x4DB810, 0x4D3780, 0x5F4F1B,
    0x5F4F42, 0x486840, 0x480A30,
)
RADIO_ENTRIES = {
    0x65ACB0: ('contact0_transmit', 1), 0x65AAA0: ('directed_transmit', 2),
    0x65A970: ('transmit', 3), 0x43C2D0: ('building_receive', 3),
    0x6F4AB0: ('techno_receive', 3), 0x65A820: ('radio_receive', 3),
}
RADIO_BOUNDARIES = {
    0x43CD2B, 0x43CDD4, 0x43CDDD, 0x43CE18, 0x6F4C29, 0x6F4C34,
    0x6F4C41, 0x6F4B8D, 0x6F4C50, 0x6F4BAD, 0x73A936, 0x73A939,
    0x73A93D, 0x73A943,
}
RADIO_RETS = {
    0x65ACCF, 0x65ACD4, 0x65AAB7, 0x65A9E5, 0x65A990, 0x43CE21,
    0x43CE12, 0x6F4C4D, 0x6F4C99, 0x6F4BBE, 0x65A8AA, 0x65A883, 0x6F4E52,
}


UNLOAD_PCS = {0x44D880, 0x44DD56, 0x44DE20, 0x44DE4B, 0x44DECD, 0x44DF72,
       0x44DFA1, 0x44DFA4, 0x44E1AD, 0x44E1B8, 0x44E202, 0x44E233,
       0x44E267, 0x44E28E, 0x44E293, 0x4A51F0, 0x4A5240, 0x4A5150,
       0x4A5360, 0x4B0C40, 0x4B0D14, 0x4B0D3F, 0x4D3710,
       0x4B0500, 0x4B0F20, 0x739EC0, 0x73A936, 0x65ACB0,
       0x43CD2B, 0x5B35E0, 0x5B3570, 0x743A50, 0x65A970, 0x65AAA0,
       0x43C2D0, 0x6F4AB0, 0x737430, 0x73A943, 0x73A946, 0x4B0F20,
       0x42C900, 0x42A5B0, 0x73A7D2, 0x73A88B, 0x73A8BF, 0x73A936,
       0x73A98C, 0x73AAC6, 0x73AADB, 0x73AB6C, 0x73AB99, 0x73ABAA,
       0x73ABCE, 0x73ABD6, 0x73ACC2, 0x73ACD1}


class InitialPlacementBoundary(Exception):
    """Retain the constructor result before ordinary Mission placement starts."""


class UnitUnlimboControls(Mission):
    def __init__(self, case, kind):
        assert kind in ('direct', 'factory')
        self.case, self.kind = case, kind
        self.placement_trace, self.factory_trace = [], []
        self.placement_pre, self.placement_rng_pre, self.after_unlimbo = None, None, None
        self.factory_pending = {}
        self.placement_stage_pre = self.stage_after_unlimbo = self.factory_stage_pre = None
        self.producer = 0
        super().__init__()
        self.unit_type_flags_after_ctor = dict(
            small_visceroid=self.u.mem_read(self.typ + 0xE18, 1)[0],
            large_visceroid=self.u.mem_read(self.typ + 0xE19, 1)[0],
        )
        # These are the original geometry static initializers, in the dependency
        # order already used by astar_hills_route, rather than fitted scalars.
        fields = (0xB1CFD0, 0xB1CFD8, 0xB1CFA0, 0xB1D0B8, 0xB1D0AC)
        before_rng = self.rng_bytes()
        self.unit_geometry_startup = dict(
            before={hex(p): bytes(self.u.mem_read(p, 8)).hex() for p in fields},
            original_calls=[],
        )
        for pc in GEOMETRY_CALLS:
            self.m.invoke(pc, 0)
            self.unit_geometry_startup['original_calls'].append(hex(pc))
        assert self.m.read32(0xB1D0B8) == self.m.read32(0x89E7C0)
        assert self.m.read32(0xB1D0AC) > 0
        assert self.rng_bytes() == before_rng
        self.unit_geometry_startup.update(
            after={hex(p): bytes(self.u.mem_read(p, 8)).hex() for p in fields},
            rng_unchanged=True,
        )

    def rng_bytes(self):
        return {k: bytes(self.u.mem_read(p, 1012)) for k, p in self.resident.rngs.items()}

    def actor(self, pointer=None):
        if pointer is not None:
            retained = self.src
            try:
                self.src = pointer
                return self.actor()
            finally:
                self.src = retained
        if not self.src:
            return None
        return dict(self.state(), on_bridge=self.u.mem_read(self.src + 0x8C, 1)[0],
                    in_playfield=self.u.mem_read(self.src + 0x3D5, 1)[0])

    def direct_post(self):
        m, u = self.m, self.u
        cell = self.resident.ptrs[87, 50]
        return dict(
            actor=self.state(), on_bridge=u.mem_read(self.src + 0x8C, 1)[0],
            stage_and_timer=[m.read32(self.src + off)
                             for off in (0xF8, 0xFC, 0x100, 0x104, 0x108, 0x10C)],
            occupation=[m.read32(cell + off) for off in (0x124, 0x128, 0x54, 0x58)],
            lists=[hex(m.read32(cell + off)) for off in (0xE4, 0xE8)],
        )

    def capture_unlimbo_return(self):
        # Mission invokes this after original737BA0 returns and before its
        # placement assertion or frame1 Attack command; no native input changes.
        self.after_unlimbo = dict(self.direct_post(), actor=self.actor(),
                                  in_playfield=self.u.mem_read(self.src + 0x3D5, 1)[0])
        self.stage_after_unlimbo = foot_missions.FootMissions.stage_clock_snap(self, self.src)

    def observe(self, u, pc, size, data):
        m, sp = self.m, u.reg_read(UC_X86_REG_ESP)
        if pc == 0x737BA0 and self.phase == 'placement':
            if self.kind == 'factory':
                raise InitialPlacementBoundary()
            cell = self.resident.ptrs[87, 50]
            u.mem_write(0xA8E7AC, dwords(self.case['scenario_init_counter']))
            u.mem_write(0xA8B238, dwords(self.case['actual_game_mode']))
            if self.case['bridge']:
                u.mem_write(cell + 0x140, dwords(m.read32(cell + 0x140) | 0x100))
            u.mem_write(cell + 0x124, dwords(self.case['ground_bits'], self.case['deck_bits']))
            u.mem_write(cell + 0x54, dwords(0, 0))
            u.mem_write(self.src + 0x8C, bytes([self.case['caller_on_bridge']]))
            coord = m.read32(sp + 4)
            if self.case['caller_deck_z']:
                u.mem_write(coord + 8, dwords(m.read32(coord + 8) + 4 * m.read32(0xABDE88)))
            self.placement_rng_pre = self.rng_bytes()
            self.placement_stage_pre = foot_missions.FootMissions.stage_clock_snap(self, self.src)
            self.placement_pre = dict(
                actor=self.state(), on_bridge=u.mem_read(self.src + 0x8C, 1)[0],
                requested_xyz=base.xyz(u, coord), cell_flags=m.read32(cell + 0x140),
                level=u.mem_read(cell + 0x11B, 1)[0],
                lists=[hex(m.read32(cell + off)) for off in (0xE4, 0xE8)],
                raw=[m.read32(cell + off) for off in (0x124, 0x128, 0x54, 0x58)],
            )
        if self.phase == 'placement' and pc in DIRECT_PCS:
            row = dict(pc=hex(pc), this=hex(u.reg_read(UC_X86_REG_ECX)))
            if pc == 0x73F0A0:
                row['args'] = [m.read32(sp + 4 + i * 4) for i in range(5)]
            if pc == 0x5F4F42:
                row['can_enter_return'] = u.reg_read(UC_X86_REG_EAX)
            if pc in (0x5F4FB4, 0x4D3780):
                row.update(xyz=base.xyz(u, self.src + 0x9C),
                           on_bridge=u.mem_read(self.src + 0x8C, 1)[0])
            self.placement_trace.append(row)
        if self.phase.startswith('factory'):
            if pc == 0x737BA0:
                self.factory_stage_pre = foot_missions.FootMissions.stage_clock_snap(self, self.src)
            if pc in self.factory_pending:
                row = self.factory_pending.pop(pc)
                row.update(returned_eax=u.reg_read(UC_X86_REG_EAX),
                           returned_al=u.reg_read(UC_X86_REG_EAX) & 255, after=self.actor())
            if pc in FACTORY_PCS:
                row = dict(pc=hex(pc), ecx=hex(u.reg_read(UC_X86_REG_ECX)),
                           counter=m.read32(0xA8E7AC), actual_game_mode=m.read32(0xA8B238),
                           actor=self.actor())
                if pc in (0x737BA0, 0x5F4EC0):
                    row['args'] = [m.read32(sp + 4 + i * 4) for i in range(2)]
                    row['requested_xyz'] = base.xyz(u, row['args'][0])
                    self.factory_pending[m.read32(sp)] = row
                if pc == 0x73F0A0:
                    row['args'] = [m.read32(sp + 4 + i * 4) for i in range(5)]
                    self.factory_pending[m.read32(sp)] = row
                if pc == 0x5F4F42:
                    row['class_entry_result'] = u.reg_read(UC_X86_REG_EAX)
                self.factory_trace.append(row)
        super().observe(u, pc, size, data)

    def run_direct(self):
        setup_failure = None
        try:
            super().setup(placement_observer=self.capture_unlimbo_return)
        except AssertionError as exc:
            assert self.after_unlimbo is not None
            assert self.placement_result & 255 == 0
            setup_failure = str(exc)
        assert self.after_unlimbo is not None
        after_rng = self.rng_bytes()
        rng = {
            k: dict(unchanged=after_rng[k] == before, before_hex=before.hex(),
                    after_hex=after_rng[k].hex(), before_sha256=hashlib.sha256(before).hexdigest(),
                    after_sha256=hashlib.sha256(after_rng[k]).hexdigest())
            for k, before in self.placement_rng_pre.items()
        }
        code_hash = self.text_hash()
        assert not self.pending, self.pending
        return dict(input=self.case, before=self.placement_pre, after=self.direct_post(),
                    after_unlimbo=self.after_unlimbo, returned_al=self.placement_result & 255,
                    trace=self.placement_trace, rng=rng, native_text_sha256=code_hash,
                    expected_setup_assertion=setup_failure,
                    unit_geometry_startup=self.unit_geometry_startup, inherited_inputs=self.inputs,
                    unit_type_flags_after_ctor=self.unit_type_flags_after_ctor,
                    stage_before=self.placement_stage_pre, stage_after=self.stage_after_unlimbo,
                    placement_events=[e for e in self.events if e.get('phase') == 'placement'])

    def retain_before_initial_placement(self):
        """Keep the original constructor result for a declared caller control."""
        assert self.kind == 'factory'
        try:
            super().setup()
        except InitialPlacementBoundary:
            pass
        else:
            raise AssertionError('Initial placement boundary not reached')

    def run_authored(self):
        """Execute the original Unit reader's HIGH arm and virtual Unlimbo call."""
        self.retain_before_initial_placement()
        self.phase = 'authored_caller'
        m, u = self.m, self.u
        case = self.case
        cell = self.resident.ptrs[87, 50]
        flags = m.read32(cell + 0x140)
        u.mem_write(cell + 0x140, dwords(flags | 0x100 if case['has_bridge']
                                      else flags & ~0x100))
        u.mem_write(cell + 0x124, dwords(case['ground_bits'], case['deck_bits']))
        u.mem_write(0xA8E7AC, dwords(case['scope']))
        u.mem_write(SP + 0x3C, dwords(22400, 12928, 0))
        for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_ESI, self.src),
                                (UC_X86_REG_EAX, case['parsed_high'])):
            u.reg_write(register, value)
        drive = m.read32(self.src + 0x674)
        force_slope = m.read32(m.read32(drive) + 0x50)
        selected = {0x7434EC, 0x7434F3, 0x7434FB, 0x578080, 0x47B3A0,
                    0x74350A, 0x743510, 0x7435C6, 0x7435D0, 0x737BA0,
                    0x4D7170, 0x5F4EC0, 0x5F4F1B, 0x73F0A0, 0x5F4F42,
                    force_slope}
        trace = []
        def observe(_u, pc, _size, _data):
            if pc in selected:
                trace.append(dict(pc=hex(pc), ecx=hex(u.reg_read(UC_X86_REG_ECX)),
                                  counter=m.read32(0xA8E7AC), actor=self.actor(),
                                  eax=u.reg_read(UC_X86_REG_EAX)))
        hook = u.hook_add(UC_HOOK_CODE, observe)
        before_rng, before = self.rng_bytes(), self.actor()
        try:
            run_checked(u, 0x7434EC, 0x743514,
                        required_addresses=(0x7434EC, 0x7434F3))
            assert u.reg_read(UC_X86_REG_ESP) == SP
            pre_unlimbo = dict(
                actor=self.actor(), requested_xyz=base.xyz(u, SP + 0x3C),
                counter=m.read32(0xA8E7AC), bridge_height=m.read32(0xB1D0AC),
                level=u.mem_read(cell + 0x11B, 1)[0], cell_flags=m.read32(cell + 0x140),
                # Preserve the external receipt's field name: this slot is
                # ForceSlope, not a second height or activation operation.
                drive_activate_slot50=hex(force_slope))
            u.reg_write(UC_X86_REG_EBP, 0x80)
            run_checked(u, 0x7435C6, 0x7435D6, count=1000000,
                        required_addresses=(0x7435D0, 0x737BA0, 0x5F4EC0))
        finally:
            u.hook_del(hook)
        assert u.reg_read(UC_X86_REG_ESP) == SP
        after_rng = self.rng_bytes()
        assert before_rng == after_rng
        assert m.read32(0xA8E7AC) == case['scope']
        assert not self.pending, self.pending
        return dict(
            input=case, before=before, pre_unlimbo=pre_unlimbo,
            returned_al=u.reg_read(UC_X86_REG_EAX) & 255, after=self.actor(),
            stage_after=foot_missions.FootMissions.stage_clock_snap(self, self.src),
            raw_after=[m.read32(cell + off) for off in (0x124, 0x128, 0x54, 0x58)],
            lists_after=[hex(m.read32(cell + off)) for off in (0xE4, 0xE8)],
            trace=trace, rng={k:dict(before_hex=v.hex(), after_hex=after_rng[k].hex(),
                                   unchanged=v == after_rng[k]) for k,v in before_rng.items()},
            text_sha256=self.text_hash(), inherited_inputs=self.inputs,
            geometry_startup=self.unit_geometry_startup)

    def prepare_factory(self):
        self.retain_before_initial_placement()
        self.phase = 'setup'
        m, u = self.m, self.u
        self.inputs['factory_type_layers'] = []
        u.mem_write(0xA83C68, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
        self.producer_type = m.alloc(0x1800)
        type_ctor_eax = m.invoke(0x45DD90, self.producer_type,
                                (m.cstring(self.case['producer_type']),))
        exit_control = self.case.get('exit_coord_control')
        if exit_control is not None:
            assert exit_control in ('explicit_zero', 'constructor_missing', 'z_only_nonempty')
            self.exit_coordinate_setup = dict(
                control=exit_control, type_ctor_entry='0x0045DD90',
                type_ctor_returned_eax=type_ctor_eax, type_pointer=hex(self.producer_type),
                ctor_exit_coord=base.xyz(u, self.producer_type + 0xEC8),
                ctor_exit_coord_hex=bytes(u.mem_read(self.producer_type + 0xEC8, 12)).hex(),
                empty_comparison_address='0x0089C848',
                empty_comparison_hex=bytes(u.mem_read(0x89C848, 12)).hex(),
                read_passes=[])
        def read_slice(begin, end):
            for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, self.producer_type),
                               (UC_X86_REG_EBX, self.producer_type + 0x24),
                               (UC_X86_REG_ESI, RULES), (UC_X86_REG_EDI, RULES)):
                u.reg_write(reg, value)
            run_checked(u, begin, end, count=300000, required_addresses=(begin,))
            assert u.reg_read(UC_X86_REG_ESP) == SP
        art_raw = (Path(os.environ['VERA20K_SHRAPNEL_INPUTS']) / 'ARTMD.INI').read_bytes()
        art, lines = base.lexical(art_raw, {self.case['producer_type']})
        m.make_ini(art)
        self.inputs['factory_art'] = dict(sha256=hashlib.sha256(art_raw).hexdigest(),
                                         sections=art, lines=lines)
        for name, path in base.layers():
            if not path.exists():
                continue
            sections, lines = base.lexical(path.read_bytes(), {self.case['producer_type']})
            supplied_sections = sections
            if exit_control == 'constructor_missing':
                # This is an independent missing-key input starting from the
                # original constructor, never an omission after retained512.
                supplied_sections = {section: {key: value for key, value in fields.items()
                                               if key != 'ExitCoord'}
                                     for section, fields in sections.items()}
            m.rules_cache(supplied_sections)
            calls = []
            # Native Naval, Weeder, Refinery/WeaponsFactory and ExitCoord reads.
            for begin, end in ((0x714A63, 0x714A7D), (0x4604B2, 0x4604CC),
                               (0x460A38, 0x460A92), (0x460F9C, 0x460FE2)):
                if exit_control is not None and begin == 0x460F9C:
                    before_exit = bytes(u.mem_read(self.producer_type + 0xEC8, 12))
                read_slice(begin, end)
                if exit_control is not None and begin == 0x460F9C:
                    fields = supplied_sections.get(self.case['producer_type'], {})
                    self.exit_coordinate_setup['read_passes'].append(dict(
                        layer=name, entry=hex(begin), stop_before=hex(end),
                        supplied_section=fields, physical_section=sections.get(
                            self.case['producer_type'], {}), missing_key='ExitCoord' not in fields,
                        before_hex=before_exit.hex(),
                        after_hex=bytes(u.mem_read(self.producer_type + 0xEC8, 12)).hex(),
                        after_coord=base.xyz(u, self.producer_type + 0xEC8)))
                calls.append([hex(begin), hex(end)])
            self.inputs['factory_type_layers'].append(dict(
                file=name, sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
                sections=supplied_sections, lines=lines, original_slices=calls))
        if exit_control in ('explicit_zero', 'z_only_nonempty'):
            fields = {'ExitCoord': '0,0,0' if exit_control == 'explicit_zero' else '0,0,1'}
            before_exit = bytes(u.mem_read(self.producer_type + 0xEC8, 12))
            m.rules_cache({self.case['producer_type']: fields})
            read_slice(0x460F9C, 0x460FE2)
            self.exit_coordinate_setup['read_passes'].append(dict(
                layer='supplied_final_control', entry='0x460f9c', stop_before='0x460fe2',
                supplied_section=fields, physical_section=None, missing_key=False,
                before_hex=before_exit.hex(),
                after_hex=bytes(u.mem_read(self.producer_type + 0xEC8, 12)).hex(),
                after_coord=base.xyz(u, self.producer_type + 0xEC8)))
        if exit_control is not None:
            self.exit_coordinate_setup.update(
                final_coord=base.xyz(u, self.producer_type + 0xEC8),
                final_coord_hex=bytes(u.mem_read(self.producer_type + 0xEC8, 12)).hex(),
                limits='Selected physical flag/ART readers execute normally. The '
                       'declared cached ExitCoord key is removed in every layer for '
                       'constructor_missing, or a final original ReadCoord receives '
                       'the explicit lexical control. Complete Rules chronology and '
                       'physical CCINI parsing remain inherited fixture boundaries.')
        for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, self.producer_type),
                           (UC_X86_REG_EBX, self.producer_type + 0x24),
                           (UC_X86_REG_EDI, self.producer_type + 0x1F8)):
            u.reg_write(reg, value)
        run_checked(u, 0x461225, 0x46125D, count=300000, required_addresses=(0x461225,))
        assert u.reg_read(UC_X86_REG_ESP) == SP
        self.producer = m.alloc(0x1000)
        m.invoke(0x45B1C0, 0)
        m.invoke(0x43B740, self.producer, (self.producer_type, 0))
        u.mem_write(self.producer + 0x21C, dwords(self.house))
        u.mem_write(self.producer + 0x14C, dwords(self.house))
        u.mem_write(self.producer + 0x9C, dwords(*self.case['producer_xyz']))
        u.mem_write(self.producer + 0xAC, dwords(5))
        u.mem_write(self.producer + 0xB4, dwords(-1))
        u.mem_write(self.producer + 0x218, dwords(0))
        u.mem_write(0xA8E7AC, dwords(self.case['scope_start']))
        u.mem_write(0xA8B238, dwords(self.case['actual_game_mode']))
        cell = self.resident.ptrs[87, 50]
        u.mem_write(cell + 0x124, dwords(self.case['ground_bits'], self.case['deck_bits']))
        if self.case.get('bridge'):
            u.mem_write(cell + 0x140, dwords(m.read32(cell + 0x140) | 0x100))
        u.mem_write(0xA8E9A0, bytes([self.case.get('scenario_active', True)]))
        self.producer_snapshot = dict(
            ptr=hex(self.producer), vtable=hex(m.read32(self.producer)),
            type_vtable=hex(m.read32(self.producer_type)), type=self.case['producer_type'],
            exit_coord=base.xyz(u, self.producer_type + 0xEC8),
            foundation=m.read32(self.producer_type + 0xEF0),
            flags={hex(off): u.mem_read(self.producer_type + off, 1)[0]
                   for off in (0x16BB, 0x16BC, 0x16BD, 0xCCE)},
            xyz=base.xyz(u, self.producer + 0x9C),
            slots={hex(slot): hex(m.read32(m.read32(self.producer) + slot))
                   for slot in (0x48, 0xB4, 0x184, 0x1E8, 0x278)},
        )
        self.before = self.actor()
        self.phase = 'factory_call'

    def run_factory(self):
        self.prepare_factory()
        m, u = self.m, self.u
        before_rng = self.rng_bytes()
        eax = m.invoke(0x443C60, self.producer, (self.src, 0))
        after_rng = self.rng_bytes()
        assert not self.factory_pending, self.factory_pending
        assert not self.pending, self.pending
        cell = self.resident.ptrs[87, 50]
        return dict(
            input=self.case, failure=None, eax=eax, before=self.before, after=self.actor(),
            counter_after=m.read32(0xA8E7AC), producer=self.producer_snapshot,
            producer_current=base.i32(u, self.producer + 0xAC),
            producer_queued=base.i32(u, self.producer + 0xB4), trace=self.factory_trace,
            rng={k: dict(before_hex=before.hex(), after_hex=after_rng[k].hex(),
                         unchanged=before == after_rng[k]) for k, before in before_rng.items()},
            pending={}, cell_after=self.resident.snapshot(cell),
            plane_after=[m.read32(cell + off) for off in (0xE4, 0xE8, 0x124, 0x128, 0x54, 0x58)],
            events=[e for e in self.events if e.get('phase', '').startswith('factory')],
            inputs=self.inputs, unit_geometry_startup=self.unit_geometry_startup,
            unit_type_flags_after_ctor=self.unit_type_flags_after_ctor,
            stage_before=self.factory_stage_pre,
            stage_after=foot_missions.FootMissions.stage_clock_snap(self, self.src),
            text_sha256=self.text_hash(),
        )

    def visceroid_reader_receipt(self):
        # Independent data controls before any actor construction. The shared
        # physical Rules cache and original CCINI bool reader remain the owners.
        m, u = self.m, self.u
        controls = [
            dict(name='constructor_missing', initial=[0, 0], passes=[{}]),
            dict(name='retained_true_missing', initial=[1, 1], passes=[{}]),
            dict(name='small_yes_large_no', initial=[0, 0],
                 passes=[dict(SmallVisceroid='yes', LargeVisceroid='no')]),
            dict(name='current_defaults_across_layers', initial=[0, 0],
                 passes=[dict(SmallVisceroid='yes', LargeVisceroid='no'), {},
                         dict(SmallVisceroid='no', LargeVisceroid='true')]),
            dict(name='invalid_retains_current', initial=[1, 0],
                 passes=[dict(SmallVisceroid='maybe', LargeVisceroid='maybe')]),
            dict(name='different_case_key_control', initial=[0, 0],
                 passes=[dict(smallvisceroid='yes', largevisceroid='true')]),
        ]
        before_rng = self.rng_bytes()
        rows = []
        for control in controls:
            u.mem_write(self.typ + 0xE18, bytes(control['initial']))
            passes = []
            for fields in control['passes']:
                before = list(u.mem_read(self.typ + 0xE18, 2))
                m.rules_cache({'MTNK': fields})
                for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EDI, self.typ),
                                   (UC_X86_REG_EBP, self.typ + 0x24), (UC_X86_REG_EBX, RULES),
                                   (UC_X86_REG_EAX, u.mem_read(self.typ + 0xE17, 1)[0])):
                    u.reg_write(reg, value)
                run_checked(u, 0x747862, 0x74789C,
                            required_addresses=(0x747862, 0x5295F0))
                assert u.reg_read(UC_X86_REG_ESP) == SP
                passes.append(dict(section='MTNK', fields=fields, before=before,
                                   after=list(u.mem_read(self.typ + 0xE18, 2))))
            rows.append(dict(input=control, passes=passes))
        after_rng = self.rng_bytes()
        assert before_rng == after_rng
        return dict(unit_type_constructor='0x007470D0',
                    constructor_flags=self.unit_type_flags_after_ctor,
                    reader_start='0x00747862', stop_before='0x0074789C',
                    native_keys={hex(p): m.string(p) for p in (0x83CC84, 0x83CC94)},
                    rows=rows,
                    rng={k: dict(before_hex=v.hex(), after_hex=after_rng[k].hex(),
                                 unchanged=v == after_rng[k]) for k, v in before_rng.items()},
                    native_text_sha256=self.text_hash())

    def run_reused_stage(self):
        control = self.case
        self.case = direct_case('ground_clear')
        initial = self.run_direct()
        assert initial['returned_al'] == 1
        self.phase = 'stage_limbo'
        limbo_before = self.actor()
        limbo_stage_before = foot_missions.FootMissions.stage_clock_snap(self, self.src)
        limbo_rng_before = self.rng_bytes()
        limbo_eax = self.m.invoke(0x7440B0, self.src, ())
        limbo_rng_after = self.rng_bytes()
        assert self.u.mem_read(self.src + 0x81, 1)[0] == 1
        limbo = dict(entry='0x007440B0', returned_al=limbo_eax & 255,
                     before=limbo_before, after=self.actor(),
                     stage_before=limbo_stage_before,
                     stage_after=foot_missions.FootMissions.stage_clock_snap(self, self.src),
                     rng={k: dict(before_hex=v.hex(), after_hex=limbo_rng_after[k].hex(),
                                  unchanged=v == limbo_rng_after[k])
                          for k, v in limbo_rng_before.items()})
        self.case = control
        m, u, stage = self.m, self.u, control['poisoned_stage']
        u.mem_write(self.src + 0xF8, dwords(stage['value']))
        u.mem_write(self.src + 0xFC, bytes([stage['changed']]))
        u.mem_write(self.src + 0x100, dwords(stage['start'], stage['aux_raw_u32'],
                                           stage['duration'], stage['rate'], stage['increment']))
        u.mem_write(self.typ + 0xE18, bytes([control['small_visceroid'], control['large_visceroid']]))
        self.frame = control['frame']
        u.mem_write(0xA8ED84, dwords(self.frame))
        before, rng_before = self.actor(), self.rng_bytes()
        rng_before_state = {k: base.sr.rng_state(u, p) for k, p in self.resident.rngs.items()}
        stage_before = foot_missions.FootMissions.stage_clock_snap(self, self.src)
        start, event_start = len(self.placement_trace), len(self.events)
        self.phase = 'placement'
        self.placement_result = m.invoke(0x737BA0, self.src, (self.coord, 0x80))
        self.capture_unlimbo_return()
        rng_after = self.rng_bytes()
        assert not self.pending, self.pending
        return dict(
            input=control, initial_placement=dict(returned_al=initial['returned_al'],
                before=initial['before'], after_unlimbo=initial['after_unlimbo'],
                stage_before=initial['stage_before'], stage_after=initial['stage_after'],
                rng=initial['rng']), limbo=limbo,
            before=before, after=self.actor(), stage_before=stage_before,
            stage_after=self.stage_after_unlimbo, returned_al=self.placement_result & 255,
            rng={k: dict(before_hex=v.hex(), after_hex=rng_after[k].hex(),
                         unchanged=v == rng_after[k]) for k, v in rng_before.items()},
            rng_before=rng_before_state,
            rng_after={k: base.sr.rng_state(u, p) for k, p in self.resident.rngs.items()},
            trace=self.placement_trace[start:], events=self.events[event_start:],
            native_text_sha256=self.text_hash(),
        )

    def radio_endpoint(self, actor):
        m, u = self.m, self.u
        vector, capacity = m.read32(actor + 0xE4), m.read32(actor + 0xE8)
        assert vector and 0 < capacity <= 64
        return dict(
            address=hex(actor), vtable=hex(m.read32(actor)),
            receive_slot_194=hex(m.read32(m.read32(actor) + 0x194)),
            contact0_slot_274=hex(m.read32(m.read32(actor) + 0x274)),
            radio_descriptor_hex=bytes(u.mem_read(actor + 0xE0, 0x10)).hex(),
            contact_capacity=capacity, contacts=[hex(m.read32(vector + 4 * i)) for i in range(capacity)],
            radio_history=[m.read32(actor + off) for off in (0xD4, 0xD8, 0xDC)],
            tether=u.mem_read(actor + 0x418, 1)[0], mission=base.i32(u, actor + 0xAC),
            queued=base.i32(u, actor + 0xB4), handler_state=base.i32(u, actor + 0xBC),
            dispatch=[base.i32(u, actor + off) for off in (0xC8, 0xD0)],
            position=base.xyz(u, actor + 0x9C),
            primary_facing=[m.read32(actor + off) for off in (0x388, 0x38C)],
            turret_facing=[m.read32(actor + off) for off in (0x3A0, 0x3A4)],
            alive=u.mem_read(actor + 0x90, 1)[0], health=base.i32(u, actor + 0x6C),
            limbo=u.mem_read(actor + 0x81, 1)[0], marked=u.mem_read(actor + 0x74, 1)[0],
            in_playfield=u.mem_read(actor + 0x3D5, 1)[0],
        )

    def radio_pair(self):
        return dict(unit=self.radio_endpoint(self.src), producer=self.radio_endpoint(self.producer),
                    actor=self.actor(), scope_counter=self.m.read32(0xA8E7AC),
                    producer_type_flags={name: self.u.mem_read(self.producer_type + off, 1)[0]
                                         for name, off in (('Helipad', 0x16A9), ('UnitRepair', 0x16AB),
                                                           ('WeaponsFactory', 0x16BD))})

    def radio_call(self, label, route='contact0'):
        trace, pending, entries = [], [], dict(RADIO_ENTRIES)
        entries[self.m.read32(self.m.read32(self.src) + 0x194)] = ('unit_receive', 3)
        def observe(u, pc, _size, _data):
            sp = u.reg_read(UC_X86_REG_ESP)
            while pending and pending[-1]['ret'] == pc and pending[-1]['sp'] == sp:
                pending.pop()['row']['returned_eax'] = u.reg_read(UC_X86_REG_EAX)
            if pc in entries:
                kind, count = entries[pc]
                row = dict(kind=kind, pc=hex(pc), this=hex(u.reg_read(UC_X86_REG_ECX)),
                           return_pc=hex(self.m.read32(sp)),
                           args=[self.m.read32(sp + 4 + 4 * i) for i in range(count)])
                trace.append(row)
                pending.append(dict(ret=self.m.read32(sp), sp=sp + 4 + 4 * count, row=row))
            elif pc in RADIO_BOUNDARIES:
                trace.append(dict(kind='original_boundary', pc=hex(pc),
                                  eax=u.reg_read(UC_X86_REG_EAX),
                                  scope_counter=self.m.read32(0xA8E7AC)))
            if pc in RADIO_RETS:
                trace.append(dict(kind='true_ret', pc=hex(pc), eax=u.reg_read(UC_X86_REG_EAX),
                                  return_pc=hex(self.m.read32(sp))))
        self.phase = 'radio_teardown'
        hook = self.u.hook_add(UC_HOOK_CODE, observe)
        before, rng_before, text_before = self.radio_pair(), self.rng_bytes(), self.text_hash()
        cw_before, start = self.u.reg_read(UC_X86_REG_FPCW), len(self.events)
        try:
            if route == 'contact0':
                eax, stop = self.m.invoke(0x65ACB0, self.src, (8,)), RET_MAGIC
            elif route == 'null_sender':
                eax, stop = self.m.invoke(0x43C2D0, self.producer, (0, 8, 0xA8EC30)), RET_MAGIC
            elif route == 'caller_cmp':
                self.u.reg_write(UC_X86_REG_ESP, SP)
                self.u.reg_write(UC_X86_REG_EBP, self.src)
                run_checked(self.u, 0x73A936, 0x73A946, count=100000,
                            required_addresses=(0x73A939, 0x73A93D, 0x73A943, 0x65ACB0,
                                                0x43CD2B, 0x6F4C29, 0x6F4C34, 0x6F4C41))
                eax, stop = self.u.reg_read(UC_X86_REG_EAX), 0x73A946
            else:
                raise AssertionError(route)
        finally:
            self.u.hook_del(hook)
        for frame in pending:
            if frame['ret'] == RET_MAGIC:
                frame['row']['returned_eax'] = eax
        after_rng, after = self.rng_bytes(), self.radio_pair()
        result = dict(
            name=label, route=route, eax=eax, stop_before=hex(stop),
            eflags=self.u.reg_read(UC_X86_REG_EFLAGS), fpcw_before=cw_before,
            fpcw_after=self.u.reg_read(UC_X86_REG_FPCW), before=before, after=after,
            trace=trace, events=self.events[start:],
            rng={k: dict(before_hex=v.hex(), after_hex=after_rng[k].hex(),
                         before_sha256=hashlib.sha256(v).hexdigest(),
                         after_sha256=hashlib.sha256(after_rng[k]).hexdigest(),
                         unchanged=v == after_rng[k]) for k, v in rng_before.items()},
            text_sha256_before=text_before, text_sha256_after=self.text_hash(),
        )
        assert rng_before == after_rng
        assert before['scope_counter'] == after['scope_counter']
        for side in ('unit', 'producer'):
            for field in ('mission', 'queued', 'handler_state', 'dispatch', 'position',
                          'primary_facing', 'turret_facing', 'health', 'alive', 'limbo',
                          'marked', 'in_playfield'):
                assert before[side][field] == after[side][field], (label, side, field)
        return result

    def run_factory_exit_radio(self, route):
        setup = self.run_factory()
        initial = self.radio_pair()
        assert setup['eax'] == 2
        assert initial['producer_type_flags'] == dict(Helipad=0, UnitRepair=0, WeaponsFactory=1)
        assert initial['unit']['contacts'][0] == hex(self.producer)
        assert initial['producer']['contacts'][0] == hex(self.src)
        supplied, preliminary = [], None
        if route == 'absent_contact0':
            pointer = self.m.read32(self.src + 0xE4)
            self.u.mem_write(pointer, dwords(0))
            supplied.append(dict(address=hex(pointer), field='Unit Contacts[0]', value=0,
                                 reason='Explicit caller no-contact boundary; producer retained.'))
            route = 'contact0'
        elif route == 'null_sender_without_contacts':
            preliminary = self.radio_call('clear_before_null_sender')
            assert preliminary['eax'] == 23
            route = 'null_sender'
        primary = self.radio_call(self.case['name'], route)
        if self.case['name'] == 'absent_contact0':
            assert primary['eax'] == 0
            assert primary['after']['producer']['contacts'][0] == hex(self.src)
            assert primary['after']['unit']['tether'] == primary['after']['producer']['tether'] == 1
        else:
            assert primary['eax'] == 23
            assert all(int(v, 16) == 0 for side in ('unit', 'producer')
                       for v in primary['after'][side]['contacts'])
            assert primary['after']['unit']['tether'] == primary['after']['producer']['tether'] == 0
        if route == 'caller_cmp':
            assert primary['eflags'] & 0x40
        row = dict(input=self.case, supplied_post_factory_data=supplied, factory=setup, primary=primary)
        if self.case['name'] == 'healthy_scope0':
            row['already_cleared'] = self.radio_call('already_cleared_contact0')
            assert row['already_cleared']['eax'] == 0
            assert row['already_cleared']['before'] == row['already_cleared']['after']
        if preliminary is not None:
            row['preliminary'] = preliminary
        return row

    @staticmethod
    def rng_pair(before, after):
        return {k: dict(before_hex=v.hex(), after_hex=after[k].hex(), unchanged=v == after[k])
                for k, v in before.items()}

    def factory_tail_state(self):
        m, u = self.m, self.u
        interface = m.read32(self.src + 0x674)
        loco = interface - 4
        foundation = m.read32(self.producer_type + 0xED4)
        return dict(
            product=self.actor(), producer=self.actor(self.producer),
            producer_health=base.i32(u, self.producer + 0x6C),
            producer_constructed_limbo=u.mem_read(self.producer + 0x81, 1)[0],
            producer_tether=u.mem_read(self.producer + 0x418, 1)[0],
            product_tether=u.mem_read(self.src + 0x418, 1)[0],
            producer_archive=hex(m.read32(self.producer + 0x218)),
            radio_pair=self.radio_pair(), navcom=hex(m.read32(self.src + 0x5A4)),
            foot_control_words={hex(off):m.read32(self.src + off) for off in (0x598,0x5A4,0x5AC,0x5BC)},
            door=bytes(u.mem_read(self.producer + 0x350, 0x1C)).hex(),
            deploy_time_bits=bytes(u.mem_read(self.producer_type + 0x3C8, 8)).hex(),
            foundation_pointer=hex(foundation),
            foundation_exit_offset_words=list(struct.unpack('<hh', u.mem_read(foundation + 0x28, 4))),
            drive=dict(interface=hex(interface), vtable=hex(m.read32(interface)),
                       selector=base.i32(u, loco + 0x58), cursor=base.i32(u, loco + 0x5C),
                       reversed=u.mem_read(loco + 0x60, 1)[0], valid=u.mem_read(loco + 0x63, 1)[0],
                       head=base.xyz(u, loco + 0x40), destination=base.xyz(u, loco + 0x34),
                       residual=base.i32(u, loco + 0x4C),
                       target_fraction_bits=bytes(u.mem_read(loco + 0x50, 8)).hex(),
                       applied_fraction_bits=bytes(u.mem_read(self.src + 0x578, 8)).hex()),
            counter=m.read32(0xA8E7AC), actual_game_mode=m.read32(0xA8B238),
            fpcw=u.reg_read(UC_X86_REG_FPCW), text_sha256=self.text_hash())

    def read_factory_unload_inputs(self, *, type_pointer=None, type_name='GAWEAP',
                                   include_unload=True, observe_deploy=False):
        m, u = self.m, self.u
        type_pointer = self.producer_type if type_pointer is None else type_pointer
        self.phase = 'setup'
        result = []
        for name, path in base.layers():
            if not path.exists():
                result.append(dict(file=name, absent=True))
                continue
            raw = path.read_bytes()
            wanted = {type_name, 'Unload'} if include_unload else {type_name}
            sections, lines = base.lexical(raw, wanted)
            m.rules_cache(sections)
            before = bytes(u.mem_read(type_pointer + 0x3C8, 8)).hex()
            for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, type_pointer),
                               (UC_X86_REG_EBX, type_pointer + 0x24), (UC_X86_REG_EDI, RULES)):
                u.reg_write(reg, value)
            reads = []
            def observe_reader(vm, pc, _size, _data):
                if pc == 0x5283D0:
                    sp = vm.reg_read(UC_X86_REG_ESP)
                    reads.append(dict(entry=hex(pc), ini=hex(vm.reg_read(UC_X86_REG_ECX)),
                        section=m.string(m.read32(sp + 4)), key=m.string(m.read32(sp + 8)),
                        default_double_hex=bytes(vm.mem_read(sp + 12, 8)).hex()))
            hook = u.hook_add(UC_HOOK_CODE, observe_reader) if observe_deploy else None
            try:
                run_checked(u, 0x714B77, 0x714B9F, required_addresses=(0x714B94, 0x5283D0))
            finally:
                if hook is not None:
                    u.hook_del(hook)
            assert u.reg_read(UC_X86_REG_ESP) == SP
            if include_unload:
                u.reg_write(UC_X86_REG_ESP, SP)
                u.reg_write(UC_X86_REG_ESI, RULES)
                run_checked(u, 0x679C92, 0x679CAF, required_addresses=(0x5B3760,))
                assert u.reg_read(UC_X86_REG_ESP) == SP
            row = dict(file=name, sha256=hashlib.sha256(raw).hexdigest(), sections=sections,
                       source_lines=lines, deploy_before=before,
                       deploy_after=bytes(u.mem_read(type_pointer + 0x3C8, 8)).hex())
            if observe_deploy:
                assert len(reads) == 1 and reads[0]['section'] == type_name
                assert reads[0]['key'] == 'DeployTime'
                row['deploy_reads'] = reads
            result.append(row)
        return result

    def initialize_foundation_exit_lists(self):
        m, u = self.m, self.u
        before_rng = self.rng_bytes()
        table_before = bytes(u.mem_read(0x89D368, 22 * 0x78))
        before_pointer = m.read32(self.producer_type + 0xED4)
        m.invoke(0x45C300, 0)
        startup_rows = []
        for index in range(22):
            raw = bytes(u.mem_read(0x89D368 + index * 0x78, 0x78))
            pairs = [list(struct.unpack_from('<hh', raw, off)) for off in range(0, 0x78, 4)]
            sentinel = next((i for i, p in enumerate(pairs) if p == [32767, 32767]), None)
            startup_rows.append(dict(id=index, address=hex(0x89D368 + index * 0x78),
                hex=raw.hex(), all_pairs=pairs, terminator_index=sentinel,
                list_before_terminator=pairs[:sentinel] if sentinel is not None else None,
                element10=pairs[10]))
        for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, self.producer_type),
                           (UC_X86_REG_EDI, self.producer_type + 0x1F8)):
            u.reg_write(reg, value)
        run_checked(u, 0x46152C, 0x46157A, required_addresses=(0x46156A, 0x528A10))
        assert u.reg_read(UC_X86_REG_ESP) == SP
        pointer = m.read32(self.producer_type + 0xED4)
        assert pointer == 0x89D368 + m.read32(self.producer_type + 0xEF0) * 0x78
        assert before_rng == self.rng_bytes()
        return dict(original_initializer='0x0045C300', crt_slot='0x008127F0', crt_thunk='0x0045C2F0',
                    original_post_read=['0x0046152C', '0x0046157A'], before_pointer=hex(before_pointer),
                    after_pointer=hex(pointer), foundation_id=m.read32(self.producer_type + 0xEF0),
                    table_before_sha256=hashlib.sha256(table_before).hexdigest(),
                    table_after_sha256=hashlib.sha256(bytes(u.mem_read(0x89D368, 22 * 0x78))).hexdigest(),
                    original_selected_row_hex=bytes(u.mem_read(pointer, 0x78)).hex(),
                    original_startup_rows=startup_rows,
                    rng=self.rng_pair(before_rng, self.rng_bytes()))

    def prepare_factory_continuation_tiles(self):
        """Bind missing crop TMP bodies through the existing physical archive owner.

        This is the same declared cache/relocation boundary as the established
        Resident/Navigation owners. Original IsoTileType construction executes;
        the full native archive/primary tile/INI initializer remains excluded.
        """
        m, u = self.m, self.u
        before_rng = self.rng_bytes()
        theater = base.identity.theater()
        result = bind_primary_tiles(m, theater,
            [row['tile'] for row in self.resident.case['supplied_cells']])
        assert before_rng == self.rng_bytes()
        return dict(result, rng=self.rng_pair(before_rng, self.rng_bytes()))

    def door_progress(self):
        # Observe this pure query through the existing original-function call owner.
        # Transfer the original runtime Door bytes and frame, with no derived input.
        # Its six-byte scratch FSTP avoids this fixture's RET_MAGIC stop callback.
        raw = bytes(self.u.mem_read(self.producer + 0x350, 0x1C))
        frame = bytes(self.u.mem_read(0xA8ED84, 4))
        answer = native_call(0x4A52F0, ecx=SCRATCH, writes={SCRATCH: raw, 0xA8ED84: frame},
                             capture_st0=True, fpcw=self.u.reg_read(UC_X86_REG_FPCW),
                             required_addresses=(0x4A52F0,))
        return dict(binary64_hex=struct.pack('<Q', answer['st0_bits']).hex(), value=answer['st0'])

    def door_controls(self):
        m, u = self.m, self.u
        door = self.producer + 0x350
        retained = bytes(u.mem_read(door, 0x1C))
        retained_frame = m.read32(0xA8ED84)
        before_rng = self.rng_bytes()
        minutes = struct.unpack('<II', u.mem_read(self.producer_type + 0x3C8, 8))
        retained_type_minutes = bytes(u.mem_read(self.producer_type + 0x3C8, 8))
        result = []
        try:
            controls = [(kind, None, None) for kind in
                        ('opening', 'closing', 'reversal_at10', 'zero_opening', 'zero_closing')]
            for label, raw in (('negative', '-.044'), ('overflow_i32', '3000000'),
                               ('overflow_i64', '1e17'), ('nan', 'nan'), ('infinite', 'inf')):
                controls.extend((label + '_' + direction, raw, None) for direction in ('opening', 'closing'))
            for label, value in (('raw_nan', float('nan')), ('raw_infinite', float('inf'))):
                controls.extend((label + '_' + direction, None, struct.pack('<d', value))
                                for direction in ('opening', 'closing'))
            for kind, reader_raw, raw_type in controls:
                u.mem_write(0xA8ED84, dwords(0))
                m.invoke(0x4A50F0, door)
                default = bytes(u.mem_read(door, 0x1C)).hex()
                closing_kind = kind.endswith('closing') and not kind.endswith('opening')
                closing_prerequisite = None
                if closing_kind:
                    m.invoke(0x4A52D0, door)
                    closing_prerequisite = dict(entry='0x004A52D0', before_hex=default,
                                               after_hex=bytes(u.mem_read(door, 0x1C)).hex())
                input_minutes = (0, 0) if kind.startswith('zero_') else minutes
                reader = None
                if reader_raw is not None:
                    m.rules_cache({'GAWEAP': {'DeployTime': reader_raw}})
                    u.mem_write(self.producer_type + 0x3C8, retained_type_minutes)
                    for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, self.producer_type),
                                       (UC_X86_REG_EBX, self.producer_type + 0x24), (UC_X86_REG_EDI, RULES)):
                        u.reg_write(reg, value)
                    scan = []
                    def observe_scan(vm, pc, _size, _data):
                        if pc == 0x528551:
                            pointer = vm.reg_read(UC_X86_REG_ECX)
                            scan.append(dict(local_pointer=hex(pointer),
                                local_before_hex=bytes(vm.mem_read(pointer, 4)).hex()))
                        elif pc == 0x52855D:
                            pointer = vm.reg_read(UC_X86_REG_ESP) + 0x2C
                            assert hex(pointer) == scan[-1]['local_pointer']
                            scan[-1].update(sscanf_returned_eax=vm.reg_read(UC_X86_REG_EAX),
                                local_after_hex=bytes(vm.mem_read(pointer, 4)).hex())
                    scan_hook = u.hook_add(UC_HOOK_CODE, observe_scan)
                    try:
                        run_checked(u, 0x714B77, 0x714B9F,
                            required_addresses=(0x714B94, 0x5283D0, 0x52855D))
                    finally:
                        u.hook_del(scan_hook)
                    assert u.reg_read(UC_X86_REG_ESP) == SP
                    after_reader = bytes(u.mem_read(self.producer_type + 0x3C8, 8))
                    input_minutes = struct.unpack('<II', after_reader)
                    assert len(scan) == 1
                    reader = dict(section='GAWEAP', key='DeployTime', raw=reader_raw,
                                  original_slice=['0x00714B77', '0x00714B9F'],
                                  before_hex=retained_type_minutes.hex(), after_hex=after_reader.hex(),
                                  scan=scan[0], portable_reader_result=scan[0]['sscanf_returned_eax'] == 1,
                                  failed_scan_limit='If the scan assigns no float, original52855D widens unchanged stack bytes. This is a diagnostic of this supplied stack, not a portable INI value; the existing rules parser policy remains separately documented.')
                elif raw_type is not None:
                    input_minutes = struct.unpack('<II', raw_type)
                m.invoke(0x4A5240 if closing_kind else 0x4A51F0,
                         door, input_minutes)
                start = bytes(u.mem_read(door, 0x1C)).hex()
                reversed_at = None
                if kind == 'reversal_at10':
                    u.mem_write(0xA8ED84, dwords(10))
                    reversal_before = bytes(u.mem_read(door, 0x1C)).hex()
                    m.invoke(0x4A5290, door)
                    reversed_at = dict(frame=10, before=reversal_before,
                                       after=bytes(u.mem_read(door, 0x1C)).hex())
                probes = []
                custom = kind.startswith('zero_') or reader_raw is not None or raw_type is not None
                frames = (0, 1) if custom else ((0, 1, 38, 39, 40) if reversed_at is None else (10, 11))
                for frame in frames:
                    u.mem_write(0xA8ED84, dwords(frame))
                    before = bytes(u.mem_read(door, 0x1C)).hex()
                    progress = self.door_progress()
                    due = m.invoke(0x4A5150, door) & 255
                    opening = m.invoke(0x4A5110, door) & 255
                    closing = m.invoke(0x4A5130, door) & 255
                    open_stable = m.invoke(0x4A51B0, door) & 255
                    closed_stable = m.invoke(0x4A51D0, door) & 255
                    probes.append(dict(frame=frame, before=before, after=bytes(u.mem_read(door, 0x1C)).hex(),
                                       progress=progress, due=due, opening=opening, closing=closing,
                                       open_stable=open_stable, closed_stable=closed_stable))
                m.invoke(0x4A5360, door)
                result.append(dict(kind=kind, input_minutes_hex=struct.pack('<II', *input_minutes).hex(),
                                   reader=reader, raw_type_override=raw_type.hex() if raw_type is not None else None,
                                   native_default=default, start=start,
                                   closing_prerequisite=closing_prerequisite,
                                   reversal=reversed_at, probes=probes,
                                   after_finish=bytes(u.mem_read(door, 0x1C)).hex()))
        finally:
            u.mem_write(door, retained)
            u.mem_write(0xA8ED84, dwords(retained_frame))
            u.mem_write(self.producer_type + 0x3C8, retained_type_minutes)
        assert before_rng == self.rng_bytes()
        return dict(rows=result, minutes_hex=struct.pack('<II', *minutes).hex(),
                    rng=self.rng_pair(before_rng, self.rng_bytes()),
                    observation='Original Door bytes and frame are transferred unchanged to native_oracle.call for the pure4A52F0 query. Original ST0 is rounded once to binary64 by that owner\'s six-byte scratch FSTP convention; no original text is changed.')

    def run_factory_unload(self):
        case = self.case
        factory = self.run_factory()
        assert factory['eax'] == 2
        m, u = self.m, self.u
        factory_after = self.factory_tail_state()
        foundation = self.initialize_foundation_exit_lists()
        reader_rng = self.rng_bytes()
        physical_tail_inputs = self.read_factory_unload_inputs()
        assert reader_rng == self.rng_bytes()
        additional_tiles = self.prepare_factory_continuation_tiles()
        timers = self.door_controls()
        native_house_before = u.mem_read(self.house + 0x1EC, 1)[0]
        u.mem_write(self.house + 0x1EC, bytes([case['human_controlled']]))
        before = self.factory_tail_state()
        trace, journal = [], []
        operation = ['setup']
        def observe(vm, pc, _size, _data):
            if pc not in UNLOAD_PCS:
                return
            sp = vm.reg_read(UC_X86_REG_ESP)
            item = dict(pc=hex(pc), operation=operation[0], frame=self.frame,
                        receiver=hex(vm.reg_read(UC_X86_REG_ECX)), state=self.factory_tail_state(),
                        eax=vm.reg_read(UC_X86_REG_EAX), ebx=vm.reg_read(UC_X86_REG_EBX),
                        ebp=hex(vm.reg_read(UC_X86_REG_EBP)), esi=hex(vm.reg_read(UC_X86_REG_ESI)))
            if pc == 0x4B0C40:
                item['args'] = [m.read32(sp + 4 + i * 4) for i in range(5)]
            if pc == 0x5B35E0:
                item['args'] = [m.read32(sp + 4), m.read32(sp + 8)]
            arg_count = {0x65ACB0:1, 0x65A970:3, 0x65AAA0:2,
                         0x43C2D0:3, 0x6F4AB0:3, 0x737430:3, 0x743A50:3}.get(pc)
            if arg_count is not None:
                item['args'] = [m.read32(sp + 4 + i * 4) for i in range(arg_count)]
            trace.append(item)
        hook = u.hook_add(UC_HOOK_CODE, observe)
        failure = None
        before_rng, event_start = self.rng_bytes(), len(self.events)
        consumer_route_boundary = None
        def call(label, entry, receiver, args=()):
            operation[0] = label
            pre, rng0 = self.factory_tail_state(), self.rng_bytes()
            if label == 'original_unit_ai':
                u.mem_write(SP, dwords(RET_MAGIC, *args))
                u.reg_write(UC_X86_REG_ESP, SP)
                u.reg_write(UC_X86_REG_ECX, receiver)
                stop = run_checked(u, entry, (RET_MAGIC, 0x42A5B0), count=2000000,
                                   required_addresses=(entry,))
                answer = u.reg_read(UC_X86_REG_EAX)
            else:
                answer, stop = m.invoke(entry, receiver, args), RET_MAGIC
            post, rng1 = self.factory_tail_state(), self.rng_bytes()
            journal.append(dict(operation=label, frame=self.frame, entry=hex(entry), receiver=hex(receiver),
                                args=list(args), returned_eax=answer if stop == RET_MAGIC else None,
                                stop_before=hex(stop), returned=stop == RET_MAGIC, before=pre, after=post,
                                rng=self.rng_pair(rng0, rng1)))
            return answer, stop
        try:
            self.phase = 'factory_unload'
            call('producer_commence_queued_unload', 0x5B3570, self.producer)
            next_dispatch = 1
            for frame in range(1, 161):
                self.frame = frame
                u.mem_write(0xA8ED84, dwords(frame))
                operation[0] = 'original_techno_door_caller'
                pre, rng0 = self.factory_tail_state(), self.rng_bytes()
                u.reg_write(UC_X86_REG_ESP, SP)
                u.reg_write(UC_X86_REG_ESI, self.producer)
                run_checked(u, 0x6FA5BE, 0x6FA5D6, required_addresses=(0x4A5150,))
                assert u.reg_read(UC_X86_REG_ESP) == SP
                if pre['door'] != self.factory_tail_state()['door']:
                    journal.append(dict(operation=operation[0], frame=frame, before=pre, after=self.factory_tail_state(),
                                        rng=self.rng_pair(rng0, self.rng_bytes())))
                if frame >= next_dispatch and base.i32(u, self.producer + 0xAC) == 16:
                    delay, stop = call('original_mission_unload', 0x44D880, self.producer)
                    assert stop == RET_MAGIC
                    next_dispatch = frame + delay
                if consumer_route_boundary is None:
                    _, stop = call('original_unit_ai', 0x7360C0, self.src)
                    if stop != RET_MAGIC:
                        consumer_route_boundary = dict(frame=frame, stop_before=hex(stop),
                            state=self.factory_tail_state(), rng={k:v.hex() for k,v in self.rng_bytes().items()},
                            boundary='The original consumer has reached its new scatter route request. AStar initialization and all subsequent Unit turns are excluded; no path result or native return is supplied.')
                if consumer_route_boundary is not None and base.i32(u, self.producer + 0xAC) != 16:
                    break
                if (consumer_route_boundary is not None and base.i32(u, self.producer + 0xB4) == 5
                        and bytes(u.mem_read(self.producer + 0x368, 2)) == b'\x00\x00'):
                    break
                if frame > 3 and not u.mem_read(self.producer + 0x418, 1)[0] and base.i32(u, self.producer + 0xBC) >= 4:
                    if frame >= next_dispatch:
                        break
        except Exception as error:
            failure = dict(type=type(error).__name__, message=str(error), operation=operation[0], frame=self.frame,
                           original_tail_pc=[hex(v) for v in self.trace])
        finally:
            u.hook_del(hook)
        assert failure is None, failure
        assert consumer_route_boundary is not None, 'The original post-exit route boundary was not reached'
        result = dict(input=case, factory=factory, factory_after=factory_after, physical_tail_inputs=physical_tail_inputs,
                      foundation_prerequisite=foundation, timer_controls=timers, continuation_tiles=additional_tiles,
                      supplied_house_control=dict(pointer=hex(self.house), offset='0x1EC',
                                                   before=native_house_before, after=case['human_controlled']),
                      before=before, after=self.factory_tail_state(), journal=journal, trace=trace, failure=failure,
                      consumer_route_boundary=consumer_route_boundary, native_events=self.events[event_start:],
                      rng=self.rng_pair(before_rng, self.rng_bytes()), inherited_inputs=self.inputs,
                      text_sha256=self.text_hash())
        print(json.dumps(dict(failure=failure, final_frame=self.frame, states=[
            [j['frame'], j['before']['producer']['status'], j['after']['producer']['status'], j.get('returned_eax')]
            for j in journal if j['operation'] == 'original_mission_unload'],
            final_product=result['after']['product']['position'], final_drive=result['after']['drive'])), flush=True)
        return result

    def run_factory_miner_per_cell(self, selected):
        assert selected in ('Harvester', 'Weeder')
        case = self.case
        factory = self.run_factory()
        assert factory['eax'] == 2
        m, u = self.m, self.u
        before_flags = bytes(u.mem_read(self.typ + 0xE0E, 2)).hex()
        before_default = m.read32(self.typ + 0x398)
        before_reader_rng = self.rng_bytes()
        section = {'Harvester': 'yes' if selected == 'Harvester' else 'no',
                   'Weeder': 'yes' if selected == 'Weeder' else 'no'}
        m.rules_cache({'MTNK': section})
        for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBX, RULES),
                           (UC_X86_REG_EBP, self.typ + 0x24), (UC_X86_REG_EDI, self.typ)):
            u.reg_write(reg, value)
        run_checked(u, 0x74769F, 0x7476D3, required_addresses=(0x7476AE, 0x7476C8))
        assert u.reg_read(UC_X86_REG_ESP) == SP
        run_checked(u, 0x74779D, 0x7477BB, required_addresses=(0x7477B1,))
        assert u.reg_read(UC_X86_REG_ESP) == SP
        assert before_reader_rng == self.rng_bytes()
        reader = dict(section='MTNK', keys=section,
                      original_reader_slice=['0x0074769F', '0x007476D3'],
                      original_post_read_slice=['0x0074779D', '0x007477BB'],
                      before_flags_hex=before_flags,
                      after_flags_hex=bytes(u.mem_read(self.typ + 0xE0E, 2)).hex(),
                      before_default_mission=before_default,
                      after_default_mission=m.read32(self.typ + 0x398),
                      rng=self.rng_pair(before_reader_rng, self.rng_bytes()))
        self.frame = 1
        u.mem_write(0xA8ED84, dwords(1))
        u.mem_write(self.house + 0x1EC, b'\x01')
        self.phase = 'factory_miner_percell'
        before, before_rng, event_start = self.factory_tail_state(), self.rng_bytes(), len(self.events)
        ready = m.read32(m.read32(self.src) + 0x200)
        pcs = {0x739EC0, 0x73A7D2, 0x73A93D, 0x73A943, 0x73A9AE, 0x73A9C2, 0x73AAE6,
               0x5B35E0, 0x73AAF5, 0x73AB6C, 0x73ACC2, ready, 0x73ACC8, 0x73ACD1, 0x5B3570}
        trace = []
        def observe(vm, pc, _size, _data):
            if pc not in pcs:
                return
            sp = vm.reg_read(UC_X86_REG_ESP)
            row = dict(pc=hex(pc), receiver=hex(vm.reg_read(UC_X86_REG_ECX)),
                       eax=vm.reg_read(UC_X86_REG_EAX), ebx=vm.reg_read(UC_X86_REG_EBX),
                       state=self.factory_tail_state())
            if pc == 0x5B35E0:
                row['args'] = [m.read32(sp + 4), m.read32(sp + 8)]
            if pc == 0x739EC0:
                row['args'] = [m.read32(sp + 4)]
            trace.append(row)
        hook = u.hook_add(UC_HOOK_CODE, observe)
        try:
            u.mem_write(SP, dwords(RET_MAGIC, 2))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ECX, self.src)
            stop = run_checked(u, 0x739EC0, (0x73ACD7, RET_MAGIC), count=2000000,
                required_addresses=(0x73A7D2, 0x73A93D, 0x73AAE6, 0x5B35E0, 0x73ACC2))
        finally:
            u.hook_del(hook)
        return dict(name=case['name'], input=case, factory=factory, reader=reader, before=before,
                    after=self.factory_tail_state(), trace=trace, events=self.events[event_start:],
                    rng=self.rng_pair(before_rng, self.rng_bytes()),
                    ready_slot=hex(ready), stop_before=hex(stop), returned=stop == RET_MAGIC,
                    inherited_inputs=self.inputs)

    def unit_move_state(self):
        m, u = self.m, self.u
        nav = m.read32(self.src + 0x5A4)
        cell = next((list(xy) for xy, pointer in self.resident.ptrs.items()
                     if nav == pointer), None)
        return dict(actor=self.actor(), navcom=hex(nav), navcell=cell,
                    unit_bytes={hex(off): u.mem_read(self.src + off, 1)[0]
                                for off in (0x6D2, 0x6E0, 0x6E1, 0x6E2)},
                    door=bytes(u.mem_read(self.src + 0x350, 0x1C)).hex())

    def run_unit_move_guard(self):
        """Original Mission AI dispatch, including Unit's deployment guard."""
        case = self.case
        factory = self.run_factory()
        m, u = self.m, self.u
        reader_rng = self.rng_bytes()
        reader = self.read_factory_unload_inputs(type_pointer=self.typ, type_name='MTNK',
                                                include_unload=False, observe_deploy=True)
        assert reader_rng == self.rng_bytes()
        minutes = bytes(u.mem_read(self.typ + 0x3C8, 8))
        self.frame = case['door_open_frame']
        u.mem_write(0xA8ED84, dwords(self.frame))
        door_before = bytes(u.mem_read(self.src + 0x350, 0x1C)).hex()
        door_rng = self.rng_bytes()
        open_eax = m.invoke(0x4A51F0, self.src + 0x350, struct.unpack('<II', minutes))
        door_open = dict(entry='0x004A51F0', frame=self.frame,
                         deploy_time_double_hex=minutes.hex(), before=door_before,
                         after=bytes(u.mem_read(self.src + 0x350, 0x1C)).hex(),
                         returned_eax=open_eax,
                         rng_pair=self.rng_pair(door_rng, self.rng_bytes()))
        self.frame = case['frame']
        u.mem_write(0xA8ED84, dwords(self.frame))
        u.mem_write(self.src + 0xAC, dwords(case['current_mission']))
        u.mem_write(self.src + 0xB4, dwords(case['queued_mission']))
        u.mem_write(self.src + 0xC8, dwords(0, 0, 0))
        u.mem_write(self.src + 0x6D2, bytes([case['initial_unit_6D2']]))
        u.mem_write(self.src + 0x6E0, bytes(case['flags']))
        navcell = case['navcell']
        nav = self.resident.ptrs[tuple(navcell)] if navcell is not None else 0
        u.mem_write(self.src + 0x5A4, dwords(nav))
        before, rng_before = self.unit_move_state(), self.rng_bytes()
        rng_before_state = {k: base.sr.rng_state(u, p) for k, p in self.resident.rngs.items()}
        fpcw_before = u.reg_read(UC_X86_REG_FPCW)
        move_slot = m.read32(m.read32(self.src) + 0x22C)
        assert move_slot == 0x740A90
        trace, handler_returns = [], []
        pcs = {0x5B3060, 0x740A90, 0x740AA4, 0x740AAE, 0x740AB8, 0x740AEF,
               0x5B35E0, 0x740B03, 0x5B334E, 0x5B3352, 0x5B3358,
               0x5B335F, 0x5B3368, 0x5B336F,
               0x4A5240, 0x4D4200}
        def observe(vm, pc, _size, _data):
            if pc not in pcs:
                return
            sp = vm.reg_read(UC_X86_REG_ESP)
            row = dict(pc=hex(pc), receiver=hex(vm.reg_read(UC_X86_REG_ECX)),
                       eax=vm.reg_read(UC_X86_REG_EAX), state=self.unit_move_state())
            if pc == 0x5B35E0:
                row['args'] = [m.read32(sp + 4), m.read32(sp + 8)]
            if pc == 0x740B03:
                handler_returns.append(vm.reg_read(UC_X86_REG_EAX))
            trace.append(row)
        hook = u.hook_add(UC_HOOK_CODE, observe)
        event_start = len(self.events)
        self.phase = 'unit_move_guard'
        try:
            answer = m.invoke(0x5B3060, self.src)
        finally:
            u.hook_del(hook)
        assert len(handler_returns) == 1, trace
        assert not self.pending and not self.factory_pending
        return dict(input=case, factory=factory,
                    setup=dict(deploy_time_layers=reader, door_open=door_open,
                               move_vtable_slot=hex(move_slot),
                               supplied_dispatch_timer=[0, 0, 0]),
                    entry='0x005B3060', handler_entry='0x00740A90', before=before,
                    after=self.unit_move_state(), handler_return_eax=handler_returns[0],
                    returned_eax=answer, trace=trace, events=self.events[event_start:],
                    rng_pair=self.rng_pair(rng_before, self.rng_bytes()),
                    rng_before=rng_before_state,
                    rng_after={k: base.sr.rng_state(u, p) for k, p in self.resident.rngs.items()},
                    fpcw_before=fpcw_before, fpcw_after=u.reg_read(UC_X86_REG_FPCW),
                    inherited_inputs=self.inputs, text_sha256=self.text_hash())

    def run_factory_busy_redirect(self):
        """Whole original ExitObject recursion under declared House list inputs."""
        case = self.case
        self.prepare_factory()
        self.phase = 'setup'
        continuation_tiles = self.prepare_factory_continuation_tiles()
        m, u = self.m, self.u
        producers = {'source': self.producer}
        types = {'GAWEAP': self.producer_type}
        for spec in case['candidates']:
            name = spec['type']
            if name not in types:
                pointer = m.alloc(0x1800)
                m.invoke(0x45DD90, pointer, (m.cstring(name),))
                types[name] = pointer
            pointer = m.alloc(0x1000)
            m.invoke(0x43B740, pointer, (types[name], 0))
            producers[spec['label']] = pointer
        specs = [dict(label='source', type='GAWEAP', xyz=case['producer_xyz'],
                      current=case['source_current'], queued=case['source_queued'],
                      archive=case['source_archive'],
                      attached=case['source_attachment'] == 'Building')] + case['candidates']
        attachments, attachment_receipts = {}, []
        if any(spec['attached'] for spec in specs):
            prior = bytes(u.mem_read(0xA83E30, 24)).hex()
            assert m.read32(0xA83E40) == 0
            u.mem_write(0xA83E30, dwords(0x7EB6D4, m.alloc(16 * 4), 16, 1, 0, 10))
            factory_registry = dict(address='0x00A83E30', before=prior,
                                    supplied=bytes(u.mem_read(0xA83E30, 24)).hex())
        else:
            factory_registry = None
        for spec in specs:
            pointer = producers[spec['label']]
            u.mem_write(pointer + 0x21C, dwords(self.house))
            u.mem_write(pointer + 0x14C, dwords(self.house))
            u.mem_write(pointer + 0x9C, dwords(*spec['xyz']))
            u.mem_write(pointer + 0xAC, dwords(spec['current']))
            u.mem_write(pointer + 0xB4, dwords(spec['queued']))
            archive = self.resident.ptrs[tuple(spec['archive'])]
            m.invoke(0x70C610, pointer, (archive,))
            if spec['attached']:
                factory = m.alloc(0x74)
                ctor_rng = self.rng_bytes()
                ctor_eax = m.invoke(0x4C98B0, factory)
                ctor = bytes(u.mem_read(factory, 0x74)).hex()
                assert ctor_eax == factory and m.read32(factory) == 0x7E88D0
                writes = {'0x6c': self.house}
                if spec['label'] == 'source':
                    writes.update({'0x24': 54, '0x58': self.src})
                for off, value in writes.items():
                    u.mem_write(factory + int(off, 16), dwords(value))
                u.mem_write(pointer + 0x524, dwords(factory))
                attachments[spec['label']] = factory
                attachment_receipts.append(dict(holder=spec['label'], pointer=hex(factory),
                    entry='0x004C98B0', returned_eax=ctor_eax, after_ctor=ctor,
                    supplied_words={off: hex(value) for off, value in writes.items()},
                    before_exit=bytes(u.mem_read(factory, 0x74)).hex(),
                    rng_pair=self.rng_pair(ctor_rng, self.rng_bytes())))
            else:
                u.mem_write(pointer + 0x524, dwords(0))
        data = m.alloc(max(4, len(case['house_order']) * 4))
        order = [producers[label] for label in case['house_order']]
        u.mem_write(data, dwords(*order))
        prior_house_vector = bytes(u.mem_read(self.house + 0x68, 24)).hex()
        u.mem_write(self.house + 0x68, dwords(0x7EB6D4, data, len(order), 1, len(order), 10))
        house_vector = dict(address=hex(self.house + 0x68), before=prior_house_vector,
                            supplied=bytes(u.mem_read(self.house + 0x68, 24)).hex(),
                            labels=case['house_order'], pointers=[hex(p) for p in order])
        placement_cells_before = {
            str(tuple(xy)): self.resident.snapshot(self.resident.ptrs[tuple(xy)])
            for xy in case['placement_probe_cells']}
        reverse = {pointer: label for label, pointer in producers.items()}
        cells = {pointer: list(xy) for xy, pointer in self.resident.ptrs.items()}
        def state():
            product_archive = m.read32(self.src + 0x218)
            product_nav = m.read32(self.src + 0x5A4)
            return dict(product=self.actor(), product_archive=hex(product_archive),
                product_archive_cell=cells.get(product_archive),
                product_navcom=hex(product_nav), product_navcell=cells.get(product_nav),
                product_stage=foot_missions.FootMissions.stage_clock_snap(self, self.src),
                producers={label: dict(pointer=hex(p), type=hex(m.read32(p + 0x520)),
                    xyz=base.xyz(u, p + 0x9C), current=base.i32(u, p + 0xAC),
                    queued=base.i32(u, p + 0xB4), status=base.i32(u, p + 0xBC),
                    archive=hex(m.read32(p + 0x218)),
                    archive_cell=cells.get(m.read32(p + 0x218)),
                    attachment=hex(m.read32(p + 0x524))) for label, p in producers.items()},
                factory_bytes={label: bytes(u.mem_read(p, 0x74)).hex()
                               for label, p in attachments.items()},
                counter=m.read32(0xA8E7AC), actual_game_mode=m.read32(0xA8B238),
                scenario_active=u.mem_read(0xA8E9A0, 1)[0])
        effective_before = {label: m.invoke(0x5B3040, p) for label, p in producers.items()}
        before, rng_before = state(), self.rng_bytes()
        rng_before_state = {k: base.sr.rng_state(u, p) for k, p in self.resident.rngs.items()}
        trace, recursive_returns = [], []
        pcs = {0x443C60, 0x444492, 0x70C610, 0x4444AA, 0x4444D6, 0x4444F5,
               0x44451F, 0x444525, 0x44452B, 0x444535, 0x444542, 0x444548,
               0x444552, 0x444558, 0x444575, 0x737BA0, 0x5F4EC0, 0x444979,
               0x444EE6, 0x4452C5}
        def observe(vm, pc, _size, _data):
            if pc not in pcs:
                return
            sp, receiver = vm.reg_read(UC_X86_REG_ESP), vm.reg_read(UC_X86_REG_ECX)
            ebp = vm.reg_read(UC_X86_REG_EBP)
            row = dict(pc=hex(pc), receiver=hex(receiver), receiver_label=reverse.get(receiver),
                       eax=vm.reg_read(UC_X86_REG_EAX), ebx=vm.reg_read(UC_X86_REG_EBX),
                       ebp=hex(ebp), candidate_label=reverse.get(ebp), state=state())
            if pc in (0x443C60, 0x737BA0, 0x5F4EC0):
                row['args'] = [m.read32(sp + 4), m.read32(sp + 8)]
            if pc in (0x737BA0, 0x5F4EC0):
                row['requested_xyz'] = base.xyz(u, row['args'][0])
            if pc == 0x70C610:
                row['args'] = [m.read32(sp + 4)]
            if pc == 0x444548:
                recursive_returns.append(vm.reg_read(UC_X86_REG_EAX))
            trace.append(row)
        hook = u.hook_add(UC_HOOK_CODE, observe)
        event_start = len(self.events)
        self.phase = 'factory_busy_redirect'
        try:
            answer = m.invoke(0x443C60, self.producer, (self.src, 0))
        finally:
            u.hook_del(hook)
        after = state()
        effective_after = {label: m.invoke(0x5B3040, p) for label, p in producers.items()}
        assert not self.factory_pending and not self.pending
        receivers = [row['receiver_label'] for row in trace if row['pc'] == '0x443c60']
        assert receivers and receivers[0] == 'source' and len(receivers) <= 2, receivers
        return dict(input=case, entry='0x00443C60', before=before, after=after,
                    returned_eax=answer, recursive_return_eax=recursive_returns,
                    selected_receiver=receivers[1] if len(receivers) == 2 else None,
                    effective_before=effective_before, effective_after=effective_after,
                    setup=dict(producer=self.producer_snapshot, house_vector=house_vector,
                               factory_registry=factory_registry, attachments=attachment_receipts,
                               packed_recursive_cell_argument=m.read32(0x89C818)),
                    continuation_tiles=continuation_tiles,
                    placement_cells_before=placement_cells_before,
                    placement_cells_after={
                        str(tuple(xy)): self.resident.snapshot(self.resident.ptrs[tuple(xy)])
                        for xy in case['placement_probe_cells']},
                    supplied_cells=[r for r in self.resident.case['supplied_cells']
                        if r['coord'] in case['placement_probe_cells']],
                    trace=trace, events=self.events[event_start:],
                    rng_pair=self.rng_pair(rng_before, self.rng_bytes()),
                    rng_before=rng_before_state,
                    rng_after={k: base.sr.rng_state(u, p) for k, p in self.resident.rngs.items()},
                    plane_after={str(xy): [m.read32(p + off) for off in
                        (0xE4, 0xE8, 0x124, 0x128, 0x54, 0x58)]
                        for xy, p in self.resident.ptrs.items()},
                    inherited_inputs=self.inputs, unit_geometry_startup=self.unit_geometry_startup,
                    text_sha256=self.text_hash())

    def run_factory_exit_coordinate(self):
        """Observe the original empty-coordinate arm inside the existing exits."""
        case = self.case
        assert case['route'] in ('delivery', 'busy')
        m, u = self.m, self.u
        queries, centre_calls, query_pending, centre_pending = [], [], [], []
        before, rng_before, rng_before_state, planes_before = None, None, None, None
        raw_planes_before = None
        plane_offsets = (0xE4, 0xE8, 0x124, 0x128, 0x54, 0x58)
        cells = {pointer: list(xy) for xy, pointer in self.resident.ptrs.items()}

        def endpoints():
            archive = m.read32(self.src + 0x218)
            return dict(self.radio_pair(), product_archive=hex(archive),
                        product_archive_cell=cells.get(archive))

        def observe(vm, pc, _size, _data):
            nonlocal before, rng_before, rng_before_state, planes_before, raw_planes_before
            if pc not in pcs or not self.phase.startswith('factory'):
                return
            sp = vm.reg_read(UC_X86_REG_ESP)
            if pc == 0x443C60 and before is None:
                before, rng_before = endpoints(), self.rng_bytes()
                rng_before_state = {k: base.sr.rng_state(vm, p)
                                    for k, p in self.resident.rngs.items()}
                planes_before = {str(tuple(xy)): self.resident.snapshot(
                    self.resident.ptrs[tuple(xy)]) for xy in case['placement_probe_cells']}
                raw_planes_before = {str(tuple(xy)): [m.read32(
                    self.resident.ptrs[tuple(xy)] + off) for off in plane_offsets]
                    for xy in case['placement_probe_cells']}
            elif pc == 0x44F640:
                receiver = vm.reg_read(UC_X86_REG_ECX)
                typ = m.read32(receiver + 0x520)
                row = dict(entry='0x0044F640', receiver=hex(receiver),
                           args=[m.read32(sp + 4), m.read32(sp + 8)],
                           caller_return_pc=hex(m.read32(sp)),
                           supplied_exit_coord=base.xyz(vm, typ + 0xEC8),
                           producer_location=base.xyz(vm, receiver + 0x9C),
                           producer_centre_slot=hex(m.read32(m.read32(receiver) + 0x48)),
                           scope_counter=m.read32(0xA8E7AC), comparison_pcs=[])
                queries.append(row)
                query_pending.append(row)
            elif pc in (0x44F650, 0x44F664, 0x44F674):
                if query_pending:
                    query_pending[-1]['comparison_pcs'].append(hex(pc))
            elif pc in (0x44F678, 0x44F6B5):
                assert query_pending
                query_pending[-1]['selected_arm'] = 'centre' if pc == 0x44F6B5 else 'nonempty'
            elif pc == 0x44F6BC:
                assert query_pending
                receiver = vm.reg_read(UC_X86_REG_ECX)
                query_pending[-1]['centre_virtual_call'] = dict(
                    pc=hex(pc), receiver=hex(receiver),
                    slot='0x48', target=hex(m.read32(m.read32(receiver) + 0x48)),
                    output_argument=hex(m.read32(sp)))
            elif pc == 0x447AC0:
                receiver = vm.reg_read(UC_X86_REG_ECX)
                row = dict(entry='0x00447AC0', receiver=hex(receiver),
                           output_argument=hex(m.read32(sp + 4)),
                           caller_return_pc=hex(m.read32(sp)),
                           producer_location=base.xyz(vm, receiver + 0x9C),
                           type_foundation=m.read32(m.read32(receiver + 0x520) + 0xEF0),
                           within_exit_query=bool(query_pending))
                centre_calls.append(row)
                centre_pending.append(row)
                if query_pending:
                    query_pending[-1]['centre_call_index'] = len(centre_calls) - 1
            elif pc == 0x447B11:
                assert centre_pending
                row = centre_pending.pop()
                row.update(true_ret=hex(pc), returned_eax=vm.reg_read(UC_X86_REG_EAX),
                           output_xyz=base.xyz(vm, int(row['output_argument'], 16)))
            elif pc in (0x44F6B2, 0x44F6DB):
                assert query_pending
                row = query_pending.pop()
                row.update(true_ret=hex(pc), returned_eax=vm.reg_read(UC_X86_REG_EAX),
                           output_xyz=base.xyz(vm, row['args'][0]))
                assert row['returned_eax'] == row['args'][0]

        pcs = {0x443C60, 0x44F640, 0x44F650, 0x44F664, 0x44F674,
               0x44F678, 0x44F6B5, 0x44F6BC, 0x447AC0, 0x447B11,
               0x44F6B2, 0x44F6DB}
        hook = u.hook_add(UC_HOOK_CODE, observe)
        try:
            whole = self.run_factory() if case['route'] == 'delivery' else self.run_factory_busy_redirect()
        finally:
            u.hook_del(hook)
        assert before is not None and queries and not query_pending and not centre_pending
        after, rng_after = endpoints(), self.rng_bytes()
        receiver_labels = {hex(self.producer): 'source'}
        if case['route'] == 'busy':
            receiver_labels.update({row['pointer']: label
                                    for label, row in whole['before']['producers'].items()})
        for row in queries + centre_calls:
            row['receiver_label'] = receiver_labels.get(row['receiver'])

        # Original41BEA0 encodes the placed object's actual physical Cell. Its
        # RET4/output buffer is separate from44F640's RET8 coordinate contract.
        output = m.alloc(8)
        u.mem_write(output, bytes([0xA5]) * 8)
        getter_eax = m.invoke(0x41BEA0, self.src, (output,))
        getter_raw = bytes(u.mem_read(output, 8))
        actual_xy = list(struct.unpack('<hh', getter_raw[:4]))
        actual_pointer = self.resident.ptrs.get(tuple(actual_xy))
        assert getter_eax == output and getter_raw[4:] == bytes([0xA5]) * 4
        assert self.rng_bytes() == rng_after
        assert actual_pointer and str(tuple(actual_xy)) in planes_before, actual_xy
        return dict(
            input=case, entry='0x00443C60', setup=dict(exit_coord=self.exit_coordinate_setup,
                producer=self.producer_snapshot), before=before, after=after,
            whole_exit=whole, coordinate_queries=queries, centre_calls=centre_calls,
            actual_cell=dict(native_entry='0x0041BEA0', returned_eax=getter_eax,
                output_argument=hex(output), raw_output_hex=getter_raw.hex(),
                coord=actual_xy, pointer=hex(actual_pointer),
                before=planes_before[str(tuple(actual_xy))],
                after=self.resident.snapshot(actual_pointer),
                plane_before=raw_planes_before[str(tuple(actual_xy))],
                plane_after=[m.read32(actual_pointer + off) for off in plane_offsets]),
            probe_cells_before=planes_before,
            probe_cells_after={str(tuple(xy)): self.resident.snapshot(
                self.resident.ptrs[tuple(xy)]) for xy in case['placement_probe_cells']},
            rng_pair=self.rng_pair(rng_before, rng_after), rng_before=rng_before_state,
            rng_after={k: base.sr.rng_state(u, p) for k, p in self.resident.rngs.items()},
            unit_geometry_startup=self.unit_geometry_startup,
            inherited_inputs=self.inputs, text_sha256=self.text_hash())

    def text_hash(self):
        digest = hashlib.sha256(bytes(self.u.mem_read(0x401000, 0x3E0000))).hexdigest()
        assert digest == self.resident.code_hash
        return digest


def direct_case(name, **values):
    case = dict(name=name, scenario_init_counter=0, actual_game_mode=0, bridge=False,
                ground_bits=0, deck_bits=0, caller_on_bridge=False, caller_deck_z=False)
    return dict(case, **values)


def cases():
    direct = [
        direct_case('ground_clear'), direct_case('ground_busy', ground_bits=0x20),
        direct_case('ground_busy_priority', ground_bits=0x20, scenario_init_counter=1),
        direct_case('ground_busy_actual_mode1', ground_bits=0x20, actual_game_mode=1),
        direct_case('bridge_clear_ground_input', bridge=True),
        direct_case('bridge_deck_busy_ground_input', bridge=True, deck_bits=0x20),
        direct_case('bridge_ground_busy_deck_clear_ground_input', bridge=True, ground_bits=0x20),
        direct_case('bridge_clear_deck_input', bridge=True, caller_on_bridge=True, caller_deck_z=True),
        direct_case('bridge_deck_busy_deck_input', bridge=True, deck_bits=0x20,
                    caller_on_bridge=True, caller_deck_z=True),
    ]
    factory_base = dict(producer_type='GAWEAP', producer_xyz=[21888, 12672, 416],
                        scope_start=0, actual_game_mode=0, ground_bits=0, deck_bits=0)
    factory = [
        dict(factory_base, name='land_clear'),
        dict(factory_base, name='land_busy', ground_bits=32),
        dict(factory_base, name='land_busy_mode1', ground_bits=32, actual_game_mode=1),
        dict(factory_base, name='land_busy_scope3', ground_bits=32, scope_start=3),
        dict(factory_base, name='land_refused_scenario_inactive', scenario_active=False),
        dict(factory_base, name='land_refused_scenario_inactive_scope3',
             scenario_active=False, scope_start=3),
        dict(factory_base, name='land_bridge_ground_pose', bridge=True, ground_bits=32),
        dict(factory_base, name='naval_arm_mtnk_control', producer_type='GAYARD',
             producer_xyz=[21888, 12416, 416]),
    ]
    return direct, factory


def stage_cases():
    return [
        direct_case(f'reused_small{int(small)}_large{int(large)}_'
                    f'{"accepted" if bits == 0 else "refused"}',
                    ground_bits=bits, small_visceroid=small, large_visceroid=large, frame=123,
                    poisoned_stage=dict(value=77, changed=1, start=-7,
                                        aux_raw_u32=0x13579BDF, duration=19, rate=-3, increment=5))
        for small in (False, True) for large in (False, True) for bits in (0, 32)
    ]


def radio_cases():
    factory_base = cases()[1][0]
    return [(dict(factory_base, name=name, scope_start=scope), route)
            for name, scope, route in (
                ('healthy_scope0', 0, 'contact0'), ('raised_scope1', 1, 'contact0'),
                ('nested_scope3', 3, 'contact0'), ('native_caller_cmp', 0, 'caller_cmp'),
                ('absent_contact0', 0, 'absent_contact0'),
                ('null_sender_with_contact0', 0, 'null_sender'),
                ('null_sender_without_contacts', 0, 'null_sender_without_contacts'),
            )]


def authored_cases():
    cases = [dict(name=f'high{high}_bridge{int(bridge)}_scope2_raw_busy',
                  parsed_high=high, has_bridge=bridge, scope=2,
                  ground_bits=32, deck_bits=32)
             for high in (0, 1) for bridge in (False, True)]
    cases.extend([
        dict(name='high0_bridge0_scope0_raw_busy', parsed_high=0, has_bridge=False,
             scope=0, ground_bits=32, deck_bits=0),
        dict(name='high1_bridge1_scope0_raw_busy', parsed_high=1, has_bridge=True,
             scope=0, ground_bits=0, deck_bits=32),
    ])
    return cases


def factory_unload_case():
    return dict(cases()[1][0], name='gaweap_mtnk_human_no_rally_initialized_tail',
                human_controlled=1)


def factory_miner_per_cell_cases():
    return [(selected, dict(cases()[1][0], name='primary_reason2_' + selected.lower(),
                            human_controlled=1)) for selected in ('Harvester', 'Weeder')]


def unit_move_guard_cases():
    return [dict(cases()[1][0], name=f'deploy_flags_{"".join(map(str, flags))}_'
                    f'nav_{"null" if navcell is None else "cell"}',
                 flags=list(flags), navcell=navcell, frame=200, door_open_frame=190,
                 current_mission=2, queued_mission=1, initial_unit_6D2=0)
            for flags in ((1, 0, 0), (0, 1, 0), (0, 0, 1), (1, 1, 1))
            for navcell in (None, [88, 50])]


def factory_busy_redirect_cases():
    base_case = dict(cases()[1][0], source_current=16, source_queued=-1,
                     source_archive=[87, 50], source_attachment='House',
                     placement_probe_cells=[[87, 50], [86, 50], [87, 49]])
    a = dict(label='candidate_a', type='GAWEAP', xyz=[21632, 12672, 416],
             current=5, queued=-1, archive=[86, 49], attached=False)
    b = dict(label='candidate_b', type='GAWEAP', xyz=[21888, 12416, 416],
             current=5, queued=-1, archive=[88, 49], attached=False)
    common = dict(candidates=[a, b], house_order=['source', 'candidate_b', 'candidate_a'])
    eligibility = [
        dict(a, label='attached_same_type', archive=[84, 50], attached=True),
        dict(b, label='other_type', type='NAWEAP', archive=[85, 50]),
        dict(a, label='move_same_type', xyz=[21376, 12672, 416],
             archive=[86, 50], current=2),
        dict(a, label='none_queued_guard', current=-1, queued=5),
    ]
    return [
        dict(base_case, **common, name='player_house_order'),
        dict(base_case, **common, name='building_house_order', source_attachment='Building'),
        dict(base_case, name='eligibility_effective_guard', candidates=eligibility,
             house_order=['source', 'attached_same_type', 'other_type',
                          'move_same_type', 'none_queued_guard']),
        dict(base_case, **common, name='player_first_refused', scenario_active=False),
        dict(base_case, **common, name='building_first_refused', scenario_active=False,
             source_attachment='Building'),
        dict(base_case, name='no_alternate', candidates=[], house_order=['source']),
    ]


def factory_exit_coordinate_cases():
    delivery = dict(cases()[1][0], route='delivery',
                    placement_probe_cells=[[87, 50], [87, 49], [85, 49], [86, 50]])
    busy = dict(factory_busy_redirect_cases()[1], route='busy',
                placement_probe_cells=[[87, 50], [87, 49], [85, 49], [86, 50]])
    return [
        dict(delivery, name='delivery_explicit_zero', exit_coord_control='explicit_zero'),
        dict(delivery, name='delivery_constructor_missing', exit_coord_control='constructor_missing'),
        dict(busy, name='busy_explicit_zero', exit_coord_control='explicit_zero'),
        dict(busy, name='busy_constructor_missing', exit_coord_control='constructor_missing'),
        dict(delivery, name='delivery_z_only_nonempty', exit_coord_control='z_only_nonempty'),
    ]


def generate_factory_exit_coordinate_rows():
    rows = []
    for case in factory_exit_coordinate_cases():
        row = UnitUnlimboControls(case, 'factory').run_factory_exit_coordinate()
        rows.append(row)
        print(f'exitcoord {case["name"]}: original arms'
              f'{[query["selected_arm"] for query in row["coordinate_queries"]]}', flush=True)
    return rows


def generate():
    direct, factory = cases()
    result = dict(schema_version=1, native_sha256=NATIVE_SHA256,
                  visceroid_reader_receipt=UnitUnlimboControls(
                      direct_case('visceroid_reader'), 'direct').visceroid_reader_receipt(),
                  direct_rows=[], factory_rows=[], stage_rows=[], factory_exit_radio_rows=[],
                  authored_rows=[], factory_unload_rows=[], factory_miner_per_cell_rows=[],
                  unit_move_guard_rows=[], factory_busy_redirect_rows=[],
                  factory_exit_coordinate_rows=[])
    for case in direct:
        row = UnitUnlimboControls(case, 'direct').run_direct()
        result['direct_rows'].append(row)
        print(f'direct {case["name"]}: original AL{row["returned_al"]}', flush=True)
    for case in factory:
        row = UnitUnlimboControls(case, 'factory').run_factory()
        result['factory_rows'].append(row)
        print(f'factory {case["name"]}: original EAX{row["eax"]}', flush=True)
    for case in stage_cases():
        row = UnitUnlimboControls(case, 'direct').run_reused_stage()
        result['stage_rows'].append(row)
        print(f'stage {case["name"]}: original AL{row["returned_al"]}', flush=True)
    for case, route in radio_cases():
        row = UnitUnlimboControls(case, 'factory').run_factory_exit_radio(route)
        result['factory_exit_radio_rows'].append(row)
        print(f'radio {case["name"]}: original reply{row["primary"]["eax"]}', flush=True)
    for case in authored_cases():
        row = UnitUnlimboControls(case, 'factory').run_authored()
        result['authored_rows'].append(row)
        print(f'authored {case["name"]}: original AL{row["returned_al"]}', flush=True)
    row = UnitUnlimboControls(factory_unload_case(), 'factory').run_factory_unload()
    result['factory_unload_rows'].append(row)
    for selected, case in factory_miner_per_cell_cases():
        row = UnitUnlimboControls(case, 'factory').run_factory_miner_per_cell(selected)
        result['factory_miner_per_cell_rows'].append(row)
        print(f'miner {case["name"]}: original mission{row["after"]["product"]["mission"]}', flush=True)
    for case in unit_move_guard_cases():
        row = UnitUnlimboControls(case, 'factory').run_unit_move_guard()
        result['unit_move_guard_rows'].append(row)
        print(f'guard {case["name"]}: original handler EAX{row["handler_return_eax"]}', flush=True)
    for case in factory_busy_redirect_cases():
        row = UnitUnlimboControls(case, 'factory').run_factory_busy_redirect()
        result['factory_busy_redirect_rows'].append(row)
        print(f'busy {case["name"]}: original receiver{row["selected_receiver"]} '
              f'EAX{row["returned_eax"]}', flush=True)
    result['factory_exit_coordinate_rows'] = generate_factory_exit_coordinate_rows()
    return result


def metadata():
    result = provenance(
        scope='Nine original MTNK Unit/Foot/Techno/Object Unlimbo controls, eight whole '
              'Building443C60 controls, eight original Unit Limbo/reused Stage controls, '
              'six selected UnitType bool-reader histories and seven factory Contact0 '
              'radio8 fixtures (nine radio calls), six original Unit reader HIGH/caller '
              'controls, one original GAWEAP/MTNK Unload/ForceTrack/primary clearance '
              'continuation with nineteen Door controls and all twenty-two Foundation '
              'exit-list startup rows, two contained Harvester/Weeder primary PerCell '
              'controls, eight whole MissionAI Unit Move deployment-guard controls and '
              'six whole busy-factory same-type redirection '
              'controls, plus five empty/default/nonempty ExitCoord delivery/redirection '
              'controls, with physical layered MTNK and selected '
              'producer inputs. Immediate Unit return is distinct from the inherited later command.',
        entry_points=dict(unit_ctor=0x7353C0, unit_unlimbo=0x737BA0, foot_unlimbo=0x4D7170,
                          techno_unlimbo=0x6F6CA0, object_unlimbo=0x5F4EC0,
                          object_scope_gate=0x5F4F1B, unit_can_enter=0x73F0A0,
                          unit_scope_gate=0x73F34C, unit_crate_mode_gate=0x73F405,
                          factory_exit_object=0x443C60, produced_playfield_prewrite=0x443C81,
                          factory_scope_increment=0x444575, factory_unlimbo_call=0x44458C,
                          factory_scope_success=0x444979, factory_scope_failure=0x444EE6,
                          naval_unlimbo_call=0x44440C, factory_exit_coords=0x44F640,
                          building_centre_coords=0x447AC0, object_physical_cell=0x41BEA0,
                          factory_empty_coordinate=0x89C848,
                          cell_ground_coords=0x480A30, building_type_ctor=0x45DD90,
                          building_ctor=0x43B740, foundation_startup=0x45B1C0,
                          unit_limbo=0x7440B0, unit_stage_tail=0x737BF5,
                          unit_type_ctor=0x7470D0, unit_visceroid_reader=0x747862,
                          bool_reader=0x5295F0, unit_stage_rng=0x737C47,
                          contact0_transmit=0x65ACB0, directed_transmit=0x65AAA0,
                          transmit=0x65A970, building_receive=0x43C2D0,
                          building_request8=0x43CD2B, techno_receive=0x6F4AB0,
                          techno_request8=0x6F4C29, techno_untether=0x6F4B8D,
                          techno_break=0x6F4C50, radio_receive=0x65A820,
                          unit_radio_caller=0x73A936, unit_reply_cmp=0x73A943,
                          authored_unit_reader=0x743270, authored_high_start=0x7434EC,
                          authored_high_stop_before=0x743514,
                          authored_unlimbo_start=0x7435C6,
                          authored_unlimbo_stop_before=0x7435D6,
                          map_ground_height=0x578080, unit_type_coord_clamp=0x747EB0,
                          drive_force_slope_slot50=0x4B04D0, drive_force_slope=0x4AFB40,
                          ship_force_slope=0x69F250,
                          read_scenario_scope_raise=0x684685,
                          full_init_scope_raise=0x686B4F,
                          full_init_unit_reader_call=0x687AA7,
                          rmg_scope_raise=0x598A9A, rmg_scope_unwind=0x59934A,
                          startup_scope_clear=0x68691C, startup_scope_restore=0x686945,
                          factory_mission_unload=0x44D880, force_track=0x4B0C40,
                          foot_set_speed=0x4D3710, factory_unit_ai=0x7360C0,
                          techno_door_caller=0x6FA5BE, door_ctor=0x4A50F0,
                          door_progress=0x4A52F0, door_finish=0x4A5360,
                          deploy_time_reader=0x714B77,
                          rules_mission_control_reader=0x679C92,
                          foundation_exit_startup=0x45C300,
                          foundation_exit_post_read=0x46152C,
                          unit_per_cell=0x739EC0, unit_harvester_weeder_reader=0x74769F,
                          unit_miner_default_post_read=0x74779D,
                          mission_ai=0x5B3060, unit_move=0x740A90,
                          unit_move_guard_queue=0x740AEF, unit_move_return=0x740B03,
                          mission_move_timer=0x5B3358, effective_mission=0x5B3040,
                          factory_ctor=0x4C98B0, busy_factory_archive=0x444492,
                          busy_factory_transfer=0x444525,
                          busy_factory_recursive_call=0x444542,
                          busy_factory_recursive_return=0x444548,
                          busy_factory_restore=0x444552),
        assumptions=[
            'Mission is the sole constructor, physical crop and native callback owner. '
            'Original geometry735180/735210/735230/735250/7352F0 executes first; retained '
            'scalars and all three full1012-byte RNG objects are recorded. No code or '
            'original vtable is modified; original .text retains its measured hash.',
            'Direct inputs supply independent A8E7AC scope and A8B238 actual GameMode, '
            'raw ground/deck planes, HasBridge and caller OnBridge/Z. after_unlimbo is '
            'captured immediately after737BA0 returns; after preserves the historical '
            'whole Mission.setup output, including the frame1 Attack command if admitted.',
            'Factory inputs retain the fresh limbo Unit before the ordinary Mission '
            'placement. Original BuildingType/Building constructors, selected layered '
            'Naval/Weeder/Refinery/WeaponsFactory/ExitCoord reads and original ART '
            'Foundation read run. Producer coordinates, Guard mission and archive0 '
            'are supplied. Whole original443C60 and its native callbacks then execute.',
            'Land controls measure produced+3D5 and the counter0->1->0 or3->4->3 '
            'through Unlimbo, Mark, radio and producer QueueUnload. Inactive Scenario '
            'controls measure original refusal and scope unwind. Counter scope is '
            'distinct from actual GameMode; it is not a session-mode alias.',
            'Original Object5F3993..5F39C0 copies Location+9C/+A0/+A4 from '
            'AC1380/AC1384/AC1388. Static initializer5F38A0 writes all three '
            'default-coordinate words zero and CRT slot814204 points to it. '
            'Those original spans/caller prefixes are saved as static receipts; '
            'complete CRT registration traversal is not executed by this fixture. '
            'The original complete Unit constructor retains Location[0,0,0] '
            'before placement, as recorded in the direct and factory rows.',
            'Stage decoding composes the existing FootMissions.stage_clock_snap '
            'owner. Original UnitType ctor74714F/747155 writes E18/E19=false; '
            'reader747862..74789C uses literal SmallVisceroid/LargeVisceroid and '
            'current-byte defaults through5295F0. Six independent reader controls '
            'retain full RNG and native strings, including later missing keys.',
            'Reused Stage rows perform original first placement and frame1 '
            'Attack command, then complete Unit7440B0 Limbo. Poisoned retained '
            'Stage, four type-flag combinations, frame123 and raw0/32 are explicit '
            'second-call inputs. Full737BA0 is measured. Either flag selects '
            'ScenarioRanged(0,29); original Stage writes are captured, including '
            'retained FC changed/110 increment and raw104 stack padding.',
            'Factory radio rows execute whole443C60 before original Contact0 '
            'Transmit8. Actual Unit/Building contacts and tethers are measured '
            'through nested19/3 sends, original returns and scopes0/1/3. Original '
            '73A936..73A946 caller slice stops after CMP EAX,23; no radio result '
            'or RNG is substituted. Full three RNG objects and .text are unchanged.',
            'Authored controls retain the fresh native Unit, then execute original '
            '7434EC..743514 HIGH preparation and7435C6..7435D6 virtual+D8 caller. '
            'Parsed HIGH writes OnBridge directly. True adds initialized native '
            'B1D0AC to Map578080 ground Z without HasBridge/bridge-walkable admission. '
            'Scope2 admits raw-busy cells; scope0 controls measure actual refusal. '
            'All three full RNG objects and .text are unchanged.',
            'Static original caller spans establish nested ReadScenario/FullInit '
            'scope through authored readers, the distinct live RMG Generate bracket, '
            'and temporary scope0 around PostMapInit starting-unit callbacks. '
            'FullInit6878E6 is early failure unwind, not an ordinary decrement '
            'before Unit loading. The fixture does not execute the full Scenario.',
            'Original Drive slot50/4B04D0 samples the Cell slope and calls '
            'slot7C/4AFB40. That helper and Ship69F250 write only slope/cache/timer '
            'state, not Location or SetHeight. UnitType747EB0 owns max(inputZ, '
            'Map578080 groundZ), before this ForceSlope callback.',
            'The additive common human no-rally factory row composes the same '
            'successful443C60 owner with original Foundation45C300 CRT startup, '
            'all22 raw30-pair rows and original46152C..46157A ED4 pointer binding. '
            'Original GAWEAP Foundation5x3 selects row17 and element10=[5,1]. '
            'The pointer slice runs after the inherited factory call, before '
            'Unload; full producer loading chronology remains excluded.',
            'Layered physical GAWEAP DeployTime and Rules Unload MissionControl '
            'reads execute through original714B77 and679C92. The shared native '
            'Techno+350 Door runs through original constructor/state/timer '
            'leaves, including signed/overflow/zero/reversal and typed IEEE '
            'controls. Each closing control records original4A52D0 ForceOpen '
            'before Close. Constructor first8/aux+C are undefined raw bytes. '
            'Finish clears active and preserves direction/timer words. '
            'Literal INI nan/inf fail sscanf; assigned-count and unchanged '
            'local bytes mark stack diagnostics, not portable reader outputs.',
            'Missing primary TMPs in the inherited crop are supplied through '
            'the existing navigation_inputs.extract_tiles owner. Original '
            'IsoTileType5447C0 constructs each head at the supplied sparse '
            'primary-loop index. Actual TMP relocation and array/cache headers '
            'are explicit boundaries; physical crop Cell inputs are unchanged '
            'before later original Recalc. No terrain/gameplay return is replaced.',
            'Original Commence starts queued producer Unload16. The original '
            'Techno door caller runs each supplied frame; whole Unload44D880 '
            'dispatch follows measured returned delays. Whole original UnitAI '
            'runs after each producer step until its new scatter route request. '
            'It executes ForceTrack66/head, FootSetSpeed0.5, original PerCell '
            'reason2 primary radio8/FootStop/Scatter, Ready and Commence. '
            'All3 full RNG objects and native events are retained per leaf.',
            'At the new post-exit scatter request, the Unit call stops before '
            'original AStar42A5B0 with no supplied path result or native return. '
            'Its marked0 is suspended-turn state after Unmark, not a completed '
            'Unit turn. No later Unit turn runs. Producer-only original Door '
            'and Unload leaves continue with actual cleared reciprocal contacts '
            'until close completion and queued Guard5; whole BuildingAI/Logic '
            'and subsequent navigation remain excluded.',
            'The two contained miner controls execute original UnitType '
            '74769F..7476D3 Harvester/Weeder current-default bool reads and '
            '74779D..7477BB default-Mission postread on custom MTNK inputs, '
            'after actual443C60. Whole739EC0 starts at reason2 and stops '
            'before73ACD7 after actual QueueHarvest[10,1], UnitReady744270 '
            'and Commence. This establishes that primary branch and its '
            'RNG/mission effects, not the movement trigger or whole harvesting.',
            'Eight additive guard rows reuse the original successful factory '
            'placement, read physical MTNK DeployTime through the existing '
            '714B77..714B9F owner and record actual ReadDouble arguments/current '
            'defaults. Original Unit Door4A51F0 opens at190 from those double '
            'bits. Supplied Move/current2, Attack/queued1, due dispatch timer, '
            'flags6E0/6E1/6E2 and NULL or real Cell88,50 NavCom precede whole '
            'MissionAI5B3060 at200. Byte6D2 begins0. Actual nested740B03 EAX '
            'and dispatcher timer writes, Door/Nav/flag snapshots and all3 '
            'RNG objects are observations, not selected expected returns.',
            'Six additive busy-factory rows reuse original43B740 constructors '
            'with shared exact GAWEAP type identity and a physical NAWEAP '
            'constructor identity for the different-type control. Caller '
            'supplies distinct real-Cell archives, producer poses/current/queued '
            'missions and House+68 list order. Original5B3040 records effective '
            'missions, including currentNone/queuedGuard. Typed nonNULL '
            'attachments execute originalFactory4C98B0 after a valid empty '
            'A83E30 registry header; owner/completion/held-object words are '
            'declared inputs. Whole443C60 owns archive-before-busy, selection, '
            'temporary+524 attachment moves, recursive original+100 call and '
            'restoration. selected_receiver is observed at the second443C60 '
            'entry, with actual recursive/whole returns and full RNG retained.',
            'Five additive ExitCoord rows compose the same whole factory '
            'delivery/redirection owners. Original BuildingType45DD90 defaults '
            'and every460F9C..460FE2 ReadCoord pass are recorded. Independent '
            'constructor_missing inputs remove only ExitCoord from supplied '
            'cached sections before any such read; explicit zero and z-only '
            'inputs run one final original ReadCoord. Whole443C60 records '
            'actual44F640 component comparisons, its selected arm, original '
            'producer virtual+48/447AC0 calls and returned coordinate buffers. '
            'After exit, original41BEA0 encodes the Unit physical Cell; its '
            'actual resident before/after planes, archive/radio/mission/scope '
            'and full three RNG objects are retained.',
        ],
        substitutions=[
            'Full Scenario, House/map load and producer visual asset loading are '
            'excluded. A supplied human House binds after null-House constructors '
            'with selected valid counters. Producer is not installed into foundation '
            'content lists. Complete factory controller/paid queue scheduling, other '
            'objects and global match phases are outside these controls.',
            'HasBridge and caller deck state are supplied controls, not evidence of '
            'full authored bridge materialization or bridge placement lifetime. '
            'Authored controls supply parsed HIGH EAX, centered XY/Z0 stack input, '
            'ESI actor, EBP facing128, raw planes, HasBridge and scope0/2; complete '
            'Unit line tokenization/atoi and House/type lookup are excluded. '
            'The GAYARD/MTNK '
            'row is deliberately invalid retail naval production: it establishes '
            'the original no-bracket branch control, not Ship/FNPC naval completion.',
            'Inherited physical source-order INI caches, successful heap/file '
            'mappings, OS ASCII/CLSID/COM activation into original Drive, OleRun, '
            'Interlocked/SEH/CRT/wall-clock setup, visual/radar/sound recording '
            'boundaries and cropped map/zone storage remain explicit. Original '
            'connectivity executes; whole-map hierarchy586990 remains inherited. '
            'Original admission, Unlimbo, Mark, radio and queue returns are not substituted.',
            'Native corpus publication is independent of Rust. These bounded '
            'controls do not certify navigation continuation, whole native match, '
            'complete BuildingType loading, complete RNG/audio or bridge parity.',
            'Reused Stage inputs poison an actually reused object but do not '
            'claim a native producer for arbitrary poisoned field values or '
            'MTNK visceroid flags. Reader controls supply lexical cached sections; '
            'full native physical CCINI parsing and Rules chronology are excluded.',
            'Radio NULL-sender and absent-Unit-contact controls are adversarial '
            'boundaries. Original65A970 NULL target resolves Contacts[0] when '
            'present; this does not certify valid reciprocal lifetime for the '
            'absent-contact input. Helipad/UnitRepair distance/repair/docking, '
            'The standalone radio rows exclude BuildingUnload and earlier/full '
            'UnitPerCell. The additive continuation/miner rows have their own '
            'bounded coverage; whole navigation/harvest/slave/rally/hunt and '
            'the producer occupied-footprint secondary73AB6C branch remain '
            'excluded. The common successful first Scatter publishes NavCom, '
            'so secondary is skipped. Raw+5AC is not treated as a direct count.',
            'The additive producer remains unplaced/limbo/health0; complete '
            'positive-health admission/readiness, House initialization and '
            'BuildingAI scheduling are excluded. HumanControlled1 is supplied. '
            'The pure Door progress query uses native_oracle.call with unchanged '
            'Door bytes/frame and its scratch FSTP binary64 observation, never '
            'an original .text patch. Original primary TMP constructor defaults '
            'for animation/shadow are retained; complete archive/Tile INI/variant '
            'loading is excluded.',
            'Guard inputs start after supplied mission/flag/Nav and actual '
            'Door-opening setup. They establish the direct Unit Move guard and '
            'MissionAI timer boundary, not producer deployment or whole UnitAI '
            'flag lifetime. Byte6D2 is initially0; no final-host miner-clear '
            'claim is made. Busy producer poses/House vector/mission/archive '
            'and typed Factory owner/completion inputs are supplied after '
            'original constructors. Full paid queues, House holder production, '
            'producer footprint/readiness/loading and Factory scheduling are '
            'excluded. ScenarioActive0 refusal is an executed native global '
            'boundary distinct from the Rust adversarial cell_marked refusal; '
            'comparison of selection/archive/returns/restoration/RNG does not '
            'certify equivalence of those refusal causes.',
            'ExitCoord control caches are declared synthetic data boundaries '
            'over otherwise retained physical type/ART inputs. Physical '
            'RULESMD ExitCoord is not claimed absent: its source fields are '
            'saved separately from the supplied missing-key sections. These '
            'rows cover admitted whole factory exits and first-alternate '
            'redirection, not arbitrary pad translation, non-WeaponsFactory '
            'exit-cell selection or full producer placement/navigation.',
        ],
    )
    result['promotion_sources'] = {
        'direct_probe_sha256': '7e0a5eaa8a89164fa5739c65117205a18649977e9dfa57a9702929aa6c23c1e6',
        'direct_results_sha256': '58be18cc77fb440d8fac8357daad7253b6dfd02bf46d391198eb2343dc89a210',
        'factory_probe_sha256': '63d888881f2a39135e54990a3958e5e3f07c7f1a0dcfd5e2c8ee831e1f36c258',
        'factory_results_sha256': '5287929bd37031a380faca689a94c0f58a0d52a2d080c423d12dd6de93c0d390',
        'factory_radio8_payload_sha256': '96f4f67894e7a358495bc1456e8ddffa207d36beb41f5e3d93f2972fb6704103',
        'factory_radio8_probe_sha256': '863e27c45e6749791d6cfad143a00dbc4c9cd49bc65e686156d711d6799fd06e',
        'factory_radio8_results_sha256': '443a058f1f8e928d70b28297aeeccabea84a3e0b897da42fa1759ee1212974fd',
        'authored_caller_probe_sha256': 'c93b3b32bfaf50e09aee21060803bea90456094aac0c30368ef76468838d9e48',
        'authored_caller_results_sha256': '705dfe7ac278a1ef0a75471a5d9fe4682b11a8de137e8e56dd1f4e02e82ea543',
        'authored_caller_payload_sha256': '24a70d5a8ed2a0ae2c300bd01c92a2053862b36382d986db69e835a966aec88b',
        'factory_unload_probe_sha256': '5bad3a5a00fcf1860e8cbec922f5be9db71b485e12b8d656a5a25405b99470b3',
        'factory_unload_results_sha256': '728721a95c042ec0bc0958ec6e64e3ddfbce059c44767c1311aa58f24b15299f',
        'factory_unload_payload_sha256': '763620ea453ea78f9dd76f117a67c28bd55ac8d2dd5c6b71d7f82999ec741740',
        'factory_miner_percell_probe_sha256': '998273f65a1a55443255099a0302a79643e47985255c31c13100ec35c7a9ac0c',
        'factory_miner_percell_results_sha256': '22fa0c2d236e9dda5ae60a5f0741866e727b27792394425f6f6b284d06c8d482',
        'factory_miner_percell_payload_sha256': 'd76df53d41ec9b18daf4d5848037697b33e82b7faa627f45b336801e690fa9ca',
    }
    result['constructor_location_caller_chain'] = dict(
        unit_to_foot='0x007353CE', infantry_to_foot='0x00517A5B',
        aircraft_to_foot='0x00413D2B', building_to_techno='0x0043B74B',
        foot_to_techno='0x004D31EA', techno_to_radio='0x006F2B46',
        radio_to_mission='0x0065A753', mission_to_object='0x005B2DA3',
        terrain_to_object='0x0071BB99', terrain_default_to_object='0x0071BDF9',
        terrain_distinction='Ordinary71BB90 later calls71D000 Unlimbo from its '
                            'supplied Cell; its complete constructor is not a held-limbo proof.',
    )
    result['native_spans'] = {}
    binary = image_bytes()
    for name, address, length in (
        ('factory_exit_object', 0x443C60, 0x1A48), ('factory_exit_coords', 0x44F640, 0x9C),
        ('factory_exit_coords_empty_tail', 0x44F6B5, 0x29),
        ('building_centre_coords', 0x447AC0, 0x54),
        ('object_physical_cell', 0x41BEA0, 0x3D),
        ('building_centre_vtable_slot', 0x7E3F04, 4),
        ('building_exit_coords_vtable_slot', 0x7E3F70, 4),
        ('cell_ground_coords', 0x480A30, 0x4E), ('unit_scope_and_mode', 0x73F34C, 0xD4),
        ('infantry_scope', 0x51C13A, 0xA0), ('producer_naval_read', 0x714A63, 0x1A),
        ('producer_weeder_read', 0x4604B2, 0x1A),
        ('producer_refinery_factory_read', 0x460A38, 0x5A),
        ('producer_exit_coord_read', 0x460F9C, 0x46),
        ('producer_foundation_read', 0x461225, 0x38),
        ('default_coord_initializer', 0x5F38A0, 0x12),
        ('default_coord_crt_slot', 0x814204, 4),
        ('object_constructor_location', 0x5F3993, 0x30),
        ('mission_constructor_prefix', 0x5B2DA0, 8),
        ('radio_constructor_prefix', 0x65A750, 8),
        ('techno_constructor_prefix', 0x6F2B40, 0x0B),
        ('foot_constructor_prefix', 0x4D31E0, 0x0F),
        ('unit_constructor_prefix', 0x7353C0, 0x13),
        ('infantry_constructor_prefix', 0x517A50, 0x10),
        ('aircraft_constructor_prefix', 0x413D20, 0x10),
        ('building_constructor_prefix', 0x43B740, 0x10),
        ('terrain_constructor_prefix', 0x71BB90, 0x0E),
        ('terrain_default_constructor_prefix', 0x71BDF0, 0x0E),
        ('unit_unlimbo', 0x737BA0, 0xE8),
        ('unit_visceroid_ctor_defaults', 0x7470D0, 0xC0),
        ('unit_visceroid_reader', 0x747862, 0x3A),
        ('small_visceroid_key', 0x83CC84, 15), ('large_visceroid_key', 0x83CC94, 15),
        ('contact0_transmit', 0x65ACB0, 0x27), ('directed_transmit', 0x65AAA0, 0x1A),
        ('radio_transmit', 0x65A970, 0x105), ('building_radio_dispatch', 0x43C2D0, 0x28),
        ('building_radio8', 0x43CD2B, 0xF9), ('techno_radio8', 0x6F4C29, 0x27),
        ('techno_untether', 0x6F4B8D, 0x34), ('techno_break_link', 0x6F4C50, 0x4C),
        ('unit_radio_reply_gate', 0x73A936, 0x1A),
        ('building_radio_table', 0x43CE60, 0x3B), ('techno_radio_table', 0x6F4E5C, 0x49),
        ('authored_unit_reader', 0x743270, 0x467),
        ('map_ground_height', 0x578080, 0x77), ('unit_type_coord_clamp', 0x747EB0, 0x61),
        ('read_scenario_raise', 0x68467C, 0x0E),
        ('read_scenario_dispatch', 0x68495B, 0x75),
        ('read_scenario_failure_unwind', 0x684A5F, 0x12),
        ('read_scenario_success_unwind', 0x684B58, 0x14),
        ('full_init_raise', 0x686B35, 0x20), ('full_init_failure', 0x6878C3, 0x61),
        ('full_init_unit_call', 0x687A9B, 0x11), ('full_init_unwind', 0x687C2B, 0x1F),
        ('post_map_startup_scope', 0x686905, 0x46),
        ('rmg_scope_raise', 0x598A84, 0x1C), ('rmg_scope_unwind', 0x59933C, 0x1A),
        ('starting_mcv', 0x5D7030, 0xAE), ('house_start_coords', 0x50DF30, 0xA4),
        ('starting_unlimbo_first_pose', 0x688F3B, 0x69),
        ('starting_unlimbo_fallback_pose', 0x68921A, 0x6F),
        ('drive_force_slope_slot50', 0x4B04D0, 0x25),
        ('drive_force_slope_slot7c', 0x7E7F2C, 4),
        ('drive_force_slope', 0x4AFB40, 0x35), ('ship_force_slope', 0x69F250, 0x35),
        ('techno_unlimbo_object_call_and_success_membership', 0x6F6CA0, 0x64),
        ('techno_ctor_door', 0x6F2ED3, 0x0B), ('techno_ai_door', 0x6FA5BE, 0x18),
        ('door_ctor', 0x4A50F0, 0x1A), ('door_states', 0x4A5110, 0x30),
        ('door_due', 0x4A5150, 0x5C), ('door_stable', 0x4A51B0, 0x29),
        ('door_open', 0x4A51F0, 0x49), ('door_close', 0x4A5240, 0x49),
        ('door_reverse_progress_finish', 0x4A5290, 0xF6),
        ('deploy_time_ctor', 0x710D06, 0x0C), ('deploy_time_reader', 0x714B77, 0x28),
        ('deploy_read_double', 0x5283D0, 0x1D2),
        ('foundation_exit_crt_thunk', 0x45C2F0, 5), ('foundation_exit_crt_slot', 0x8127F0, 4),
        ('foundation_exit_initializer', 0x45C300, 0x1A14),
        ('foundation_exit_ctor_pointer', 0x45DEDC, 0x0D),
        ('foundation_exit_post_read', 0x46152C, 0x4E),
        ('building_unload', 0x44D880, 0xB09), ('building_unload_jump_table', 0x44E38C, 0x14),
        ('building_unload_vtable_slot', 0x7E40F8, 4), ('force_track', 0x4B0C40, 0x100),
        ('unit_door_closed', 0x744180, 0x30), ('unit_move_door', 0x740A90, 0x74),
        ('mission_dispatch', 0x5B3060, 0x486), ('effective_mission', 0x5B3040, 0x12),
        ('unit_move_vtable_slot', 0x7F5E9C, 4),
        ('mission_move_table_word', 0x5B34F0, 4),
        ('factory_ctor', 0x4C98B0, 0x153),
        ('factory_archive_busy_redirect', 0x444492, 0xE3),
        ('building_exit_vtable_slot', 0x7E3FBC, 4),
        ('unit_per_cell_factory_exit', 0x73A7D2, 0x505),
        ('isotile_ctor', 0x5447C0, 0x240), ('recalc_missing_tmp_call', 0x47D57B, 0x51),
        ('unit_scatter_prefix', 0x743A50, 0x195), ('producer_guard_queue', 0x44D6A0, 0x46),
        ('unit_harvester_weeder_reader', 0x74769F, 0x34),
        ('unit_miner_default_post_read', 0x74779D, 0x1E),
        ('harvester_key', 0x83D4CC, 10), ('weeder_key', 0x81AC50, 7),
    ):
        offset, raw = file_span(binary, address, length)
        if name == 'default_coord_crt_slot':
            assert int.from_bytes(raw, 'little') == 0x5F38A0
        if name == 'unit_move_vtable_slot':
            assert int.from_bytes(raw, 'little') == 0x740A90
        if name == 'mission_move_table_word':
            assert int.from_bytes(raw, 'little') == 0x5B334E
        if name == 'building_exit_vtable_slot':
            assert int.from_bytes(raw, 'little') == 0x443C60
        if name == 'building_centre_vtable_slot':
            assert int.from_bytes(raw, 'little') == 0x447AC0
        if name == 'building_exit_coords_vtable_slot':
            assert int.from_bytes(raw, 'little') == 0x44F640
        result['native_spans'][name] = dict(address=f'0x{address:08X}', file_offset=offset,
                                           length=length, sha256=hashlib.sha256(raw).hexdigest(),
                                           hex=raw.hex())
    return result


def source_paths():
    # Reuse the existing Mission's transitive inventory, preserving its historical
    # receipt. New outputs bind current source hashes without republishing old ones.
    paths = foot_missions.source_paths()
    # The native run executes no Rust; the Rust consumers' own tests compare them.
    for relative in ('tools/spatial_oracle/anytown_damage/unit_unlimbo.py',
                     'tools/spatial_oracle/anytown_damage/navigation_inputs.py'):
        paths[relative] = REPO / relative
    return paths


def publish(argv=None):
    finish_vectors(generate, HERE / 'unit_unlimbo.json', provenance=metadata,
                   source_paths=source_paths(), argv=argv)
