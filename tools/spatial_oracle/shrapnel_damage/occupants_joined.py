"""Shrapnel occupant callbacks joined to original death Anim constructors.

The frozen request-boundary occupants packet remains unchanged. This composition
executes original421EA0/4224D9/4397E0/5F4EC0/424CE0 in the same Occupied VM and
streams, using the existing physical ART reader and Anim/Bouncer state decoder.
No animation AI, later debris contact, audio playback or whole Logic tick runs.
"""
from pathlib import Path
import copy
import json
import struct
import sys

from unicorn.x86_const import (
    UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_EDX,
    UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import _canonical
from tools.rules_oracle.bridge_anim_inputs import Reader
from tools.rules_oracle.bridge_anim_lists import KEYS as ANIM_LIST_READERS
from tools.rules_oracle.bridge_child_sound import sections as sound_sections
from tools.projectile_oracle.bridge_render_inputs import lexical
from tools.spatial_oracle import anim_bouncer_launch as launch
from tools.spatial_oracle.anytown_damage.anytown_occupants import signed
from tools.spatial_oracle.building_body_rules import INI, TYPE as RULES, dwords
from tools.spatial_oracle.shrapnel_repair import packet_io
from . import occupants

HERE = Path(__file__).resolve().parent
OUTPUT = HERE / 'occupants_joined.json.gz'
PROMOTION = HERE / 'occupants_joined_promotion.json'


class Joined(occupants.WoodenOccupied):
    def __init__(self, donor, theater, case, *, bridge_explosions=False):
        self.anim_pending = {}
        self.anim_names = {}
        self.anim_types = {}
        self.constructed = []
        self.asset_loaded = []
        self.asset_ptr = {}
        self.sound_names = []
        self.assets = {p.name.upper(): p.read_bytes()
                       for p in occupants.ASSETS.iterdir() if p.is_file()}
        super().__init__(donor, theater, case, scene=occupants.SCENE)
        self.phase = 'setup'
        u = self.u
        rng_before = {k: bytes(u.mem_read(p, 1012)) for k, p in self.rngs.items()}
        # The old two-member boundary initialized only the per-type item.
        # Initialize the supplied House inventory consistently here; keep v1.
        if hasattr(self, 'second'):
            u.mem_write(self.house + 0x5574, dwords(2))
        # Original Anim registry initializer, shared with ifv_impact setup.
        self.block(0x4E6D60, 0x4E6D96, {})
        # Explicit game-speed4 input to original normalized-rate5FB2E0.
        u.mem_write(0xA8EB60, dwords(4))
        # The constructor reads these General fields before original Unlimbo
        # commits its supplied coordinate. Execute defaults and layered readers.
        self.block(0x666159, 0x666163, {UC_X86_REG_ESI: RULES})
        self.block(0x66723F, 0x667245, {UC_X86_REG_ESI: RULES, UC_X86_REG_EBX: 0})
        self.general_layers = []
        self.bridge_explosion_layers = []
        for filename in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'XShrapnel.MAP'):
            path = occupants.ASSETS / filename
            if not path.is_file():
                assert filename == 'LANGRULE.INI'
                self.general_layers.append(dict(file=filename, absent=True))
                if bridge_explosions:
                    self.bridge_explosion_layers.append(dict(file=filename, absent=True))
                continue
            raw = path.read_bytes()
            general, _ = lexical(raw, {'General'})
            self.make_ini(general)
            if 'General' in general:
                if bridge_explosions:
                    _, begin, end, _ = ANIM_LIST_READERS['BridgeExplosions']
                    self.block(begin, end, {UC_X86_REG_ESI: RULES, UC_X86_REG_EDI: INI})
                self.block(0x66E5D5, 0x66E61D, {UC_X86_REG_ESI: RULES, UC_X86_REG_EDI: INI})
                self.block(0x66F2EE, 0x66F30E, {UC_X86_REG_ESI: RULES, UC_X86_REG_EDI: INI})
            drop = self.read32(RULES + 0x147C)
            self.general_layers.append(dict(file=filename, sha256=occupants.sha(raw),
                flight_level=signed(self.read32(RULES + 0x7B4)),
                drop_zone_anim=self.string(drop + 0x24) if drop else None))
            if bridge_explosions:
                self.bridge_explosion_layers.append(dict(file=filename, sha256=occupants.sha(raw),
                    **self.state('BridgeExplosions')))
        names = self.state('MetallicDebris')['names'] + self.extra_state()['explosion']
        if bridge_explosions:
            names = list(dict.fromkeys(names + self.state('BridgeExplosions')['names']))
        art = (occupants.ASSETS / 'ARTMD.INI').read_bytes()
        sections, _ = lexical(art, set(names))
        reports = {entry['Report'] for entry in sections.values() if 'Report' in entry}
        physical = sound_sections((occupants.ASSETS / 'SOUNDMD.INI').read_bytes())
        selected = {k: v for k, v in physical.items() if k == 'Defaults' or k in reports}
        selected['SoundList'] = {k: v for k, v in physical['SoundList'].items() if v in reports}
        # Extend the existing sound registry through its real owner; GenVehicleDie
        # remains index0, Report indices are fixture-relative native bindings.
        self.make_ini(selected)
        self.invoke(0x7510D0, INI)
        self.sound_reports = {name: self.invoke(0x7514D0, self.cstring(name))
                              for name in sorted(reports)}
        assert all(index != 0xFFFFFFFF for index in self.sound_reports.values())
        self.sound_names_by_index = {0: 'GenVehicleDie'} | {
            index: name for name, index in self.sound_reports.items()}
        self.make_ini(sections)
        self.art = []
        for name in names:
            pointer = self.invoke(0x428B80, self.cstring(name))
            mark = len(self.asset_loaded)
            admitted = self.invoke(0x427D00, pointer, (INI,))
            self.anim_types[pointer] = name
            row = Reader.result(self, name, pointer, admitted)
            row.update(asset_loads=self.asset_loaded[mark:], physical_art_keys=sections.get(name),
                       report_name=self.sound_names_by_index.get(row['report_index']))
            self.art.append(row)
        # D is an actual member of retail MetallicDebris but has no ART section.
        assert all(row['raw_shp_frame_count'] > 0 and row['art_body_read']
                   for row in self.art if row['name'] != 'D')
        assert all(bytes(u.mem_read(self.rngs[k], 1012)) == raw for k, raw in rng_before.items())
        self.art_input = dict(file='ARTMD.INI', bytes=len(art), sha256=occupants.sha(art),
                              rows=self.art, report_bindings=self.sound_reports,
                              constructor_general_layers=self.general_layers)
        if bridge_explosions:
            self.art_input['bridge_explosion_layers'] = self.bridge_explosion_layers
        self.phase = 'measure'

    def vector(self, base):
        items, count = self.read32(base + 4), self.read32(base + 16)
        assert count < 128, (hex(base), count)
        return [self.who(self.read32(items + i * 4)) for i in range(count)]

    def memberships(self):
        # Logic and the five original Display vectors: constructors40CB80 and
        # 4A8630 establish this layout; no membership callback is substituted.
        return dict(logic=self.vector(0x87F778),
                    display=[self.vector(0x8A0360 + i * 24) for i in range(5)],
                    anim_registry=self.vector(0xA8E9A8))

    def anim_snapshot(self, pointer):
        return dict(object=self.who(pointer), anim_type=self.anim_names[pointer],
                    **launch.constructor_state(self.u, pointer))

    def hook(self, u, pc, size, data):
        if pc == 0x5B40B0:
            # Existing unchanged physical archive-byte boundary/identity owner.
            return Reader.hook(self, u, pc, size, data)
        if getattr(self, 'phase', None) == 'measure':
            sp = u.reg_read(UC_X86_REG_ESP)
            this = u.reg_read(UC_X86_REG_ECX)
            if pc in self.anim_pending:
                pointer = self.anim_pending.pop(pc)
                self.add('anim_constructor_return', **self.anim_snapshot(pointer),
                         memberships=self.memberships())
            if pc == launch.CTOR:
                # Deliberately bypass only Occupied's declared request sink.
                # The original instruction at421EA0 and its entire body run.
                self.visit += 1
                args = [self.read32(sp + 4 + i * 4) for i in range(7)]
                name = self.string(args[0] + 0x24)
                assert self.anim_types.get(args[0]) == name, ('unread_anim', name)
                self.symbols[this] = f'anim_{len(self.constructed)}'
                self.constructed.append(this)
                self.anim_names[this] = name
                self.anim_pending[self.read32(sp)] = this
                self.add('anim_constructor', object=self.who(this), anim_type=name,
                         xyz=list(struct.unpack('<3i', u.mem_read(args[1], 12))),
                         delay=signed(args[2]), loops=signed(args[3]), draw_flags=args[4],
                         z_adjust=signed(args[5]), reverse=args[6] & 255,
                         caller=hex(self.read32(sp)), memberships=self.memberships())
                return
            if pc in (launch.INIT, launch.START, launch.UNLIMBO, launch.SUBMIT, 0x55BAA0):
                self.add('native_anim_callee', pc=hex(pc), this=self.who(this),
                         arg0=self.who(self.read32(sp + 4)), memberships=self.memberships())
            if pc == 0x7509E0:
                self.add('named_sound_request', name=self.sound_names_by_index[signed(this)],
                         xyz=list(struct.unpack('<3i', u.mem_read(u.reg_read(UC_X86_REG_EDX), 12))))
        super().hook(u, pc, size, data)

    def snapshot(self):
        return super().snapshot() | dict(
            owner_active_units=signed(self.read32(self.house + 0x5574)),
            native_id_cursor=signed(self.read32(self.scenario + 0x214)),
            memberships=self.memberships(),
            anims=[self.anim_snapshot(pointer) for pointer in self.constructed])


def generate():
    source = packet_io.read_result(HERE / 'occupants.json.gz')
    source_hash = occupants.sha(_canonical(source))
    expected = json.loads((HERE / 'occupants_promotion.json').read_bytes())
    assert source_hash == expected['results']['occupants.json']['published_payload_sha256']
    rules, theater = occupants.Rules(), occupants.theater()
    donor = occupants.ResidentRepair(occupants.input_case([117, 56], theater), rules, theater)
    donor.run()
    rows = []
    for old_case in source['cases']:
        case = old_case['input']
        print('native joined occupants', case['name'], flush=True)
        machine = Joined(donor, theater, case)
        result = machine.run()
        assert not machine.anim_pending
        rows.append(dict(input=case, initializers=machine.initializers,
                         native_inputs=machine.inputs, art_input=machine.art_input,
                         result=result))
    return dict(schema=1, request_boundary_payload_sha256=source_hash,
                text_section_sha256=machine.code_hash, scene=source['scene'], cases=rows)


def publication_projection(data):
    result = copy.deepcopy(data)
    for case in result['cases']:
        for row in case['art_input']['rows']:
            row['physical_art_keys_sha256'] = occupants.sha(_canonical(row.pop('physical_art_keys')))
    return result


def metadata():
    result = occupants.metadata()
    result['scope'] = __doc__
    result['assumptions'] += [
        'All seven supplied actor/head/mission cases and the native repaired crop are unchanged from the hash-pinned request-boundary packet. Joined two-member setup also supplies House+5574 total2 alongside its per-type item2; this is not a replay of original actor admission.',
        'Original AnimType factories and427D00 read all native MetallicDebris/MTNK Explosion names from physical ARTMD and full original SHPs. Original7510D0 extends the selected sound registry with physical Report entries; indices are fixture-relative. No Rust values initialize types.',
        'Original Anim array initializer4E6D60..4E6D96 executes; game-speed index4 is supplied. Actual Anim constructor421EA0, Bouncer4224D9..422648, BounceInit4397E0, Unlimbo5F4EC0, Logic55BAA0, Display4A9720 and Start424CE0 execute in the same VM and original RNG streams.',
        'Native requested names, coordinates, timer/state bytes, ordered Logic/Display/Anim registry memberships and full Main/Scenario/MapGen states are observed. Constructor-only identity offsets reflect selected type setup, not whole ScenarioLoad identity parity.',
    ]
    result['substitutions'] = [
        'Inherited physical lexical caches, exact unchanged archive SHP bytes, bump allocator and CRT/OS seams. Actor/House/list/head input and crop/connectivity/hierarchy/display-extent boundaries remain the request-boundary packet.',
        'Audio sample lookup returns declared fixture-local indices. Original Report binding and Start requests execute, but7509E0 stops at the sound request boundary; playback, audio RNG and device mixing remain excluded.',
        'No AnimAI, later Bouncer flight/contact/damage/smudge/expiry, deferred-delete drain or complete Logic scheduler runs. Outputs end when the already-admitted area-low caller returns.',
    ]
    result['entry_points'].update(anim_type_read=0x427D00, sound_read=0x7510D0,
        anim_constructor=launch.CTOR, bouncer=0x4224D9, bounce_init=launch.INIT,
        unlimbo=launch.UNLIMBO, anim_start=launch.START)
    return result


if __name__ == '__main__':
    result = generate()
    if '--write' in sys.argv and not PROMOTION.exists():
        raw = _canonical(json.loads(_canonical(publication_projection(result))))
        PROMOTION.write_text(json.dumps(dict(results={OUTPUT.name.removesuffix('.gz'):
            dict(published_payload_sha256=occupants.sha(raw))}), indent=2) + '\n')
    packet_io.finish_vectors(result, OUTPUT, provenance=metadata,
        promotion_path=PROMOTION, projection=publication_projection)
