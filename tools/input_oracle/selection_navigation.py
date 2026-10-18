"""Original N/M/P/T/Health/Y selection and VoiceSelect RNG controls.

Initialized actors/layers are fixture inputs, not a constructor or scenario-load
claim. Search and selection predicates execute unchanged. See the adjacent md.
"""

from copy import deepcopy
from functools import lru_cache
from pathlib import Path
from types import SimpleNamespace
import hashlib
import os
import struct

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDX, UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)

from tools.native_oracle import (
    NATIVE_SHA256, RET_MAGIC, SCRATCH, NativeCallTrace, finish_vectors,
    image_sha256, provenance, run_checked,
)
from tools.input_oracle.fast_scroll import (
    DISPLAY, byte, i32, put8, return_from_sink,
)
from tools.rmg_oracle.gen_rng_vectors import STRUCT_LEN, draws, seeded_struct
from tools.storage_oracle.keyboard_bindings import KeyboardFixture, stock_csf
from tools.projectile_oracle.bridge_render_inputs import BulletReader, lexical
from tools.spatial_oracle.building_body_rules import RULES, SP, dwords

HERE = Path(__file__).resolve().parent
SEARCH = {"next": 0x4AA2B0, "previous": 0x4AA380}
COMMAND = {"next": "NextObject", "previous": "PreviousObject"}
VTABLES = {"infantry": 0x7EB058, "unit": 0x7F5C70, "building": 0x7E3EBC}
MODE_FIELDS = {"repair": 0x11B0, "sell": 0x11B1,
               "power": 0x11B2, "planning": 0x11B3}
ACTOR_BYTES = {"limbo": 0x81, "alive": 0x90, "in_playfield": 0x3D5,
               "discovered": 0x41B, "robot_offline": 0x1C8,
               "docked": 0x418, "locomotor_swap": 0x6AD,
               "virtual_1d4": 0x270, "mission_only": 0x3D4}
SPECS = {
    0x4AA2B0: ("next", 1, 4), 0x4AA380: ("previous", 1, 4),
    0x6F32D0: ("local_gate", 0, 0), 0x6FC030: ("dynamic_gate", 0, 0),
    0x48DC90: ("unselect_all", 0, 0), 0x5F44A0: ("deselect", 0, 0),
    0x6FBFA0: ("select", 0, 0), 0x5F4520: ("object_select", 0, 0),
    0x4AEB30: ("set_follow", 1, 4), 0x4AE290: ("center_selection", 0, 0),
    0x4AC820: ("power_mode", 1, 4), 0x4AC700: ("planning_mode", 2, 8),
    0x4AC8C0: ("repair_mode", 1, 4), 0x4AC660: ("sell_mode", 1, 4),
    0x708EB0: ("voice_select", 0, 0), 0x708D90: ("queue_voice", 1, 4),
    0x65C780: ("rng_raw", 0, 0),
}
SELECTION_SPECS = {
    0x732280: ("combatant_command", 0, 0),
    0x732580: ("alive_owner_gate", 0, 0),
    0x7342C0: ("screen_techno_gate", 0, 0),
    0x7325C0: ("combatant_final_gate", 0, 0),
    0x6DA770: ("unselect_if_not_owned", 0, 0),
    0x6DA740: ("tactical_unselect_all", 0, 0),
    0x731D00: ("reset_selection_mode", 0, 0),
    0x732CA0: ("type_press", 0, 0),
    0x732CC0: ("type_release", 0, 0),
    0x732950: ("type_tap", 0, 0),
    0x733380: ("health_command", 0, 0),
    0x70D150: ("action_timer_start", 0, 0),
    0x70D4A0: ("world_detach", 2, 8),
    0x733160: ("navigation_pointer_expired", 0, 0),
    0x7258D0: ("pointer_expired_dispatch", 0, 0),
    0x5F5280: ("object_detach_all", 1, 4),
    0x5F4D30: ("object_conceal", 0, 0),
    0x4D9720: ("foot_detach_all", 1, 4),
    0x4A9770: ("display_remove", 1, 4),
    0x55BAE0: ("logic_remove", 1, 4),
    0x439150: ("bomb_pointer_expiry", 1, 4),
    0x54E590: ("temporal_pointer_expiry", 1, 4),
    0x6DA560: ("tactical_pointer_expiry", 2, 8),
    0x55B880: ("logic_pointer_expiry", 2, 8),
    0x65ACB0: ("radio_transmit_first", 1, 4),
}
ACTION_TIMER = 0xB0EA80
NAVIGATION_SPECS = {
    0x7336C0: ("veterancy_command", 0, 0),
    0x750030: ("veterancy_rank", 0, 0),
    0x750090: ("set_veteran", 1, 4),
    0x7500E0: ("set_veterancy_percent", 1, 4),
    0x74FF30: ("veterancy_constructor", 0, 0),
    0x7C9BFD: ("crt_atoi", 1, 0),
    0x7335F0: ("navigation_fallback_gate", 0, 0),
    0x732050: ("navigation_selected_snapshot", 0, 0),
    0x731F70: ("navigation_screen_snapshot", 0, 0),
    # Historical P labels are retained in older families. Actual725972 calls
    # KamikazeControlRemove54E590, not a Temporal expiry service.
    0x54E590: ("kamikaze_pointer_expiry", 1, 4),
}


@lru_cache(maxsize=None)
def seed_bytes(seed):
    return seeded_struct(seed)


def actor(identity, kind="unit", **overrides):
    result = dict(id=identity, kind=kind, health=100, limbo=False, alive=True,
                  in_playfield=True, discovered=True, selectable=True,
                  owner="local", slave=False, robot_offline=False, bunker=False,
                  docked=False, cell_building=False, locomotor_swap=False, virtual_1d4=False,
                  mission_only=False, techno_cast=True,
                  coord=[identity * 257, identity * 513, identity * 7],
                  voice_list=[101], queued_voice=-1)
    result.update(overrides)
    return result


class SelectionFixture(KeyboardFixture):
    """Reuse original registered commands; supply bounded ordinary object state."""

    def __init__(self, actors, layers, *, campaign=False, other_human=False,
                 other_control=False, other_passive=False, seed=1, voice_enabled=True,
                 selection_commands=False, navigation_commands=False):
        super().__init__()
        self.selection_commands = selection_commands
        self.navigation_commands = navigation_commands
        self.uc.mem_map(RET_MAGIC, 0x1000)
        self.next_free = SCRATCH + 0x10000
        self.actors, self.types, self.identities = {}, {}, {0: None}
        self.input_actors = deepcopy(actors)
        self.houses = {name: self.alloc(0x17000 if selection_commands else 0x6000)
                       for name in ("local", "other")}
        for house in self.houses.values():
            self.put(house + 0x34, self.alloc(0x200))
        self.put(0xA83D4C, self.houses["local"])
        self.put(0xA8B238, int(not campaign))
        put8(self.uc, self.houses["local"] + 0x1EC, True)
        put8(self.uc, self.houses["local"] + 0x1ED, True)
        put8(self.uc, self.houses["other"] + 0x1EC, other_human)
        put8(self.uc, self.houses["other"] + 0x1ED, other_control)
        put8(self.uc, self.get(self.houses["other"] + 0x34) + 0x1A6, other_passive)
        self.put(DISPLAY, 0x7E1964)  # Actual Mouse/Display vtable.
        put8(self.uc, 0xA8ED6B, False)  # Ordinary game, not map editor.
        put8(self.uc, 0xAC4CF4, False)  # No planning-group propagation.
        put8(self.uc, 0xAC4CF6, False)
        put8(self.uc, 0x822CF2, voice_enabled)
        self.put(0xB73550, 1)
        self.put(0xA8ED84, 1000)
        for value in actors:
            pointer, typ = self.alloc(0x1000), self.alloc(0x1000)
            self.actors[value["id"]] = pointer
            self.types[value["id"]] = typ
            self.identities[pointer] = value["id"]
            self.put(pointer, VTABLES[value["kind"]])
            if selection_commands:
                # Actual full710AF0 constructor vtable, supplied as fixture data.
                # These actors/types remain initialized state, not constructed objects.
                self.put(typ, 0x7F4ED8)
            self.put(pointer + (0x6C0 if value["kind"] == "infantry" else 0x6C4), typ)
            if navigation_commands:
                from tools.spatial_oracle.cost_of import VTABLE as COST_VTABLE
                # Original Building GetType459EE0 uses+520. The ordinary
                # P controls excluded Building before this getter.
                if value["kind"] == "building":
                    self.put(pointer + 0x520, typ)
                self.put(typ, COST_VTABLE[value["kind"]])
                self.put(typ + 0x610, value["cost"] & 0xFFFFFFFF)
                self.put(pointer + 0x150, value["veterancy_raw_bits"])
            if value["kind"] == "unit":
                # Unit virtual+37C746C90 regards any +6D8 != -1 as an active
                # control blocker. Ordinary initialized Unit uses -1; zeroed
                # storage would hide ObjectSelect's mission-only refusal.
                self.put(pointer + 0x6D8, 0xFFFFFFFF)
            self.put(pointer + 0x14, 7 if value["techno_cast"] else 0)
            self.put(pointer + 0x6C, value["health"] & 0xFFFFFFFF)
            self.put(pointer + 0x21C, self.houses[value["owner"]])
            self.put(pointer + 0x2DC, self.houses["other"] if value["slave"] else 0)
            self.put(pointer + 0x2E4, self.houses["other"] if value["bunker"] else 0)
            for field, offset in ACTOR_BYTES.items():
                put8(self.uc, pointer + offset, value[field])
            put8(self.uc, pointer + 0x74, value.get("marked", False))
            self.put(pointer + 0x94, value.get("native_layer", 0))
            self.uc.mem_write(pointer + 0x9C, struct.pack("<3i", *value["coord"]))
            self.put(pointer + 0x4F0, value["queued_voice"] & 0xFFFFFFFF)
            put8(self.uc, typ + 0x230, value["selectable"])
            put8(self.uc, typ + 0xDBC, value.get("is_selectable_combatant", False))
            self.put(typ + 0xA0, value.get("strength", 100))
            self.uc.mem_write(typ + 0x24, value.get("type_id", f"TYPE{value['id']}").encode("ascii") + b"\0")
            put8(self.uc, typ + 0xC9C, value.get("positive_primary_damage", True))
            self.set_voice_list(value["id"], value["voice_list"])
            if "enslaved_voice_list" in value:
                self.set_voice_list(value["id"], value["enslaved_voice_list"], 0x430)
        # The dock gate uses original ObjectGetCell5F6960/Map565730 and
        # CellGetBuilding47C520. Only coordinate->cell storage/occupiers are
        # supplied; no building query is replaced by a fixture answer.
        self.put(DISPLAY + 0x13C, 0xC00000)
        self.put(DISPLAY + 0x140, 0x40000)
        put8(self.uc, 0xA8E9A0, True)
        for value in actors:
            if not value["docked"]:
                continue
            cell = self.alloc(0x200)
            self.put(cell, 0x7E4EEC)
            x, y = (coord >> 8 for coord in value["coord"][:2])
            if not (0 <= x < 512 and 0 <= y < 512):
                raise ValueError("Dock control coordinate outside supplied native cell table")
            self.put(0xC00000 + (y * 512 + x) * 4, cell)
            self.uc.mem_write(cell + 0x24, struct.pack("<hh", x, y))
            if value["cell_building"]:
                building = self.alloc(0x1000)
                self.put(building, VTABLES["building"])
                self.put(cell + 0xE4, building)
        self.set_layers(layers)
        # Original static initializer4E7D40 sets this vtable. The vector's
        # capacity/storage are supplied, as in naval_lifetime_controls.py.
        self.selection_items = self.alloc(64 * 4)
        for offset, value in ((0, 0x7E4F64), (4, self.selection_items),
                              (8, 64), (12, 1), (16, 0), (20, 10)):
            self.put(0xA8ECB8 + offset, value)
        self.scenario = self.alloc(0x7000)
        self.put(0xA8B230, self.scenario)
        self.rngs = {"main": 0x886B88, "scenario": self.scenario + 0x218,
                     "mapgen": 0xABE890}
        for pointer in self.rngs.values():
            self.uc.mem_write(pointer, seed_bytes(seed))
        self.seed = seed
        if selection_commands:
            self.csf, self.csf_sha256 = stock_csf()
            self.csf_strings = {}
            self.time_ms = 1000
            self.clock_sink = SCRATCH + 0xFF000
            self.put(0x7E1530, self.clock_sink)  # timeGetTime import transport.
            self.tactical = self.alloc(0x1000)
            self.put(0x887324, self.tactical)
            self.put(self.houses["local"] + 0x16054, 7)
            rules = self.alloc(0x2000)
            self.put(0x8871E0, rules)
            self.uc.mem_write(rules + 0x1700, struct.pack("<dd", 0.5, 0.25))
            self.uc.reg_write(UC_X86_REG_FPCW, 0x0E7F)
            self.uc.mem_write(ACTION_TIMER, struct.pack("<3i", 995, 0, 25))
            self.uc.hook_add(UC_HOOK_MEM_WRITE, self.observe_timer,
                             begin=ACTION_TIMER, end=ACTION_TIMER + 11)
        if navigation_commands:
            from tools.spatial_oracle.cost_of import factors, f32
            from tools.spatial_oracle.infantry_deploy_action import Fixture as AtomicTransportOwner
            ones = factors([f32(1.0)] * 5)
            for house in self.houses.values():
                self.uc.mem_write(self.get(house + 0x34) + 0x114, ones)
                self.uc.mem_write(house + 0x5390, ones)
            if any(value["kind"] == "building" for value in actors):
                # Reuse cost_of's declared unrelated PadAircraft/Dock input
                # shape. Building virtual+AC45ED50 always dereferences the
                # first pad/dock before testing this type's identity.
                pads, dock = self.alloc(8), self.alloc(4)
                unrelated = self.alloc(0x1000)
                self.put(dock, unrelated)
                for index in range(2):
                    pad_type = self.alloc(0x1000)
                    self.put(pads + 4 * index, pad_type)
                    self.put(pad_type, COST_VTABLE["aircraft"])
                    self.put(pad_type + 0x3EC, dock)
                self.put(rules + 0xB5C, pads)
                put8(self.uc, rules + 0x17E8, True)
            # Reuse the existing single-thread OS transport. The native CRT
            # formatter executes its own original instructions, including
            # Interlocked IAT7E11C8/CC; no message/category result is supplied.
            atomic_sinks = (SCRATCH + 0xFF100, SCRATCH + 0xFF120)
            self.put(0x7E11C8, atomic_sinks[0])
            self.put(0x7E11CC, atomic_sinks[1])
            atomic = SimpleNamespace(u=self.uc, imports=list(atomic_sinks),
                                     read=lambda address: i32(self.uc, address))

            def platform_atomic(uc, pc, size, data):
                if pc in atomic_sinks:
                    AtomicTransportOwner.observe(atomic, uc, pc, size, data)

            self.uc.hook_add(UC_HOOK_CODE, platform_atomic)
        self.code_hash = self.text_hash()
        self.reset_observations()
        self.uc.hook_add(UC_HOOK_CODE, self.observe)

    def alloc(self, size):
        result = self.next_free
        self.next_free += (size + 15) & ~15
        if self.next_free >= SCRATCH + 0x100000:
            raise RuntimeError("Selection fixture storage exhausted")
        return result

    def text_hash(self):
        return hashlib.sha256(bytes(self.uc.mem_read(0x401000, 0x3E0000))).hexdigest()

    def set_voice_list(self, identity, values, offset=0x414):
        vector = self.types[identity] + offset
        items = self.alloc(max(4, 4 * len(values)))
        for index, value in enumerate(values):
            self.put(items + index * 4, value & 0xFFFFFFFF)
        self.put(vector + 4, items)
        self.put(vector + 8, len(values))
        self.put(vector + 16, len(values))

    def set_layers(self, layers):
        if len(layers) != 5:
            raise ValueError("Exactly five native layer vectors are required")
        for layer, ids in enumerate(layers):
            vector = 0x8A0360 + layer * 24
            items = self.alloc(max(4, len(ids) * 4))
            for index, identity in enumerate(ids):
                if identity is not None and identity not in self.actors:
                    raise ValueError(f"Layer refers to missing actor {identity}")
                self.put(items + index * 4, self.actors.get(identity, 0))
            self.put(vector + 4, items)
            self.put(vector + 8, len(ids))
            self.put(vector + 16, len(ids))

    def set_selection(self, identities):
        for identity, pointer in self.actors.items():
            put8(self.uc, pointer + 0x83, identity in identities)
        for index, identity in enumerate(identities):
            self.put(self.selection_items + index * 4, self.actors[identity])
        self.put(0xA8ECC8, len(identities))

    def set_display(self, *, follow=None, modes=None, placement=False):
        self.put(DISPLAY + 0x11A0, self.actors.get(follow, 0))
        put8(self.uc, DISPLAY + 0x119C, follow is not None)
        self.put(DISPLAY + 0x11A8, self.alloc(4) if placement else 0)
        for mode, offset in MODE_FIELDS.items():
            put8(self.uc, DISPLAY + offset, (modes or {}).get(mode, False))

    def reset_observations(self):
        self.trace = NativeCallTrace(self.uc, self.get)
        self.call_order, self.center_coords, self.redraws, self.cursor_calls = [], [], [], []
        self.messages, self.message_keys, self.timer_writes = [], [], []

    def observe_timer(self, uc, _access, address, size, value, _data):
        self.timer_writes.append(dict(offset=address - ACTION_TIMER, bytes=size, value=value))

    def observe(self, uc, pc, _size, _data):
        sp = uc.reg_read(UC_X86_REG_ESP)
        self.trace.returned(pc, sp)
        spec = SPECS.get(pc)
        if self.selection_commands:
            spec = SELECTION_SPECS.get(pc, spec)
        if self.navigation_commands:
            spec = NAVIGATION_SPECS.get(pc, spec)
        if spec:
            self.trace.entered(pc, sp, spec)
            self.call_order.append(spec[0])
        if self.selection_commands:
            if pc == 0x7C8B3D:  # Original vector destructor's CRT free boundary.
                return_from_sink(uc, 0)
                return
            if pc == self.clock_sink:
                return_from_sink(uc, 0, self.time_ms)
                return
            if pc == 0x734E60:  # StringTable transport; physical CSF strings.
                key = bytes(uc.mem_read(uc.reg_read(UC_X86_REG_ECX), 128)).split(b"\0")[0].decode("ascii")
                self.message_keys.append(key)
                if key not in self.csf_strings:
                    raw = (self.csf[key.upper()] + "\0").encode("utf-16-le")
                    pointer = self.alloc(len(raw))
                    uc.mem_write(pointer, raw)
                    self.csf_strings[key] = pointer
                return_from_sink(uc, 8, self.csf_strings[key])
                return
            if pc == 0x5D3BA0:  # Message list UI boundary, after original copying/formatting.
                args = [self.get(sp + 4 + index * 4) for index in range(7)]
                pointer = args[2]
                words = []
                while word := bytes(uc.mem_read(pointer + len(words) * 2, 2)):
                    if word == b"\0\0":
                        break
                    words.append(word)
                    if len(words) > 256:
                        raise RuntimeError("Unterminated native message")
                self.messages.append(dict(text=b"".join(words).decode("utf-16-le"),
                                          prefix=args[0], color=args[1], scheme=args[3],
                                          style=args[4], timeout=args[5], silent=args[6]))
                return_from_sink(uc, 28)
                return
        if pc == 0x6D6070:
            coord = self.get(sp + 4)
            self.center_coords.append(list(struct.unpack("<3i", uc.mem_read(coord, 12))))
            self.call_order.append("camera_coord")
            return_from_sink(uc, 4)
        elif pc == 0x4F42F0:
            self.redraws.append(self.get(sp + 4))
            self.call_order.append("redraw")
            return_from_sink(uc, 4)
        elif pc in (0x5BDA80, 0x5BDAA0):
            # Real Mouse virtual+48/+50 entries, declared cursor presentation.
            count = 2 if pc == 0x5BDA80 else 0
            self.cursor_calls.append(dict(pc=hex(pc), args=[self.get(sp + 4 + i * 4) for i in range(count)]))
            return_from_sink(uc, count * 4)

    def invoke(self, entry, receiver, arguments=()):
        self.uc.mem_write(self.stack - 0x1000, bytes(0x1800))
        self.put(self.stack, RET_MAGIC)
        for index, value in enumerate(arguments):
            self.put(self.stack + 4 + index * 4, value & 0xFFFFFFFF)
        self.uc.reg_write(UC_X86_REG_ESP, self.stack)
        self.uc.reg_write(UC_X86_REG_ECX, receiver)
        run_checked(self.uc, entry, RET_MAGIC, count=100000,
                    required_addresses=[entry])
        sp = self.uc.reg_read(UC_X86_REG_ESP)
        self.trace.returned(RET_MAGIC, sp)
        if sp != self.stack + 4 * (len(arguments) + 1):
            raise RuntimeError(f"Unbalanced return at {entry:#x}")
        if self.trace.pending:
            raise RuntimeError(f"Unreturned observed call at {entry:#x}")
        if self.text_hash() != self.code_hash:
            raise RuntimeError("Original .text changed during selection execution")
        return self.uc.reg_read(UC_X86_REG_EAX)

    def run_region(self, entry, stop, *, required=()):
        """Execute an original caller region with explicitly supplied registers."""
        self.uc.reg_write(UC_X86_REG_ESP, self.stack)
        run_checked(self.uc, entry, stop, count=200000,
                    required_addresses=[entry, *required])
        sp = self.uc.reg_read(UC_X86_REG_ESP)
        self.trace.returned(stop, sp)
        if sp != self.stack or self.trace.pending:
            raise RuntimeError(f"Unbalanced caller region at {entry:#x}")
        if self.text_hash() != self.code_hash:
            raise RuntimeError("Original .text changed during caller-region execution")

    def visits(self):
        return [dict(id=self.identities[int(row["ecx"], 16)],
                     gate=row["name"], accepted=bool(row["return_eax"] & 255))
                for row in self.trace.calls if row["name"] in ("local_gate", "dynamic_gate")]

    def rng_bytes(self):
        return {name: bytes(self.uc.mem_read(pointer, STRUCT_LEN))
                for name, pointer in self.rngs.items()}

    def snapshot(self):
        selected = [self.identities[self.get(self.selection_items + 4 * i)]
                    for i in range(self.get(0xA8ECC8))]
        return dict(selected=selected,
                    selected_flags={str(i): byte(self.uc, p + 0x83) for i, p in self.actors.items()},
                    follow=self.identities[self.get(DISPLAY + 0x11A0)],
                    follow_enabled=byte(self.uc, DISPLAY + 0x119C),
                    modes={name: byte(self.uc, DISPLAY + offset) for name, offset in MODE_FIELDS.items()},
                    placement=bool(self.get(DISPLAY + 0x11A8)),
                    selection_mode=self.get(0xB0FE54),
                    selection_across_map=byte(self.uc, 0xB0FE58))

    def observations(self, before):
        after = self.rng_bytes()
        raw_draws = [row["return_eax"] for row in self.trace.calls if row["name"] == "rng_raw"]
        reference, reference_after = draws(before["main"], len(raw_draws))
        if reference != raw_draws or reference_after != after["main"]:
            raise RuntimeError("Voice draws differ from original raw RNG continuation")
        if any(before[key] != after[key] for key in ("scenario", "mapgen")):
            raise RuntimeError("Selection unexpectedly changed deterministic RNG state")
        return dict(visits=self.visits(), call_order=self.call_order,
                    center_coord=self.center_coords[-1] if self.center_coords else None,
                    redraw=self.redraws, cursor_calls=self.cursor_calls,
                    voice_requests=[dict(id=self.identities[int(row["ecx"], 16)],
                                         sound_id=struct.unpack("<i", struct.pack("<I", row["args"][0]))[0])
                                    for row in self.trace.calls if row["name"] == "queue_voice"],
                    main_draws=raw_draws,
                    queued_voices={str(i): i32(self.uc, p + 0x4F0) for i, p in self.actors.items()},
                    rng_unchanged={name: before[name] == state for name, state in after.items()})

    def set_selection_sources(self, screen, map_order):
        if not self.selection_commands:
            raise RuntimeError("Selection sources require command observations")
        self.put(self.tactical + 0xDB0, len(screen))
        for index, identity in enumerate(screen):
            self.put(0xB0CEC8 + 12 * index, self.actors.get(identity, 0))
        items = self.alloc(max(4, len(map_order) * 4))
        for index, identity in enumerate(map_order):
            self.put(items + 4 * index, self.actors.get(identity, 0))
        self.put(0xA8EC7C, items)
        self.put(0xA8EC88, len(map_order))

    def initialize_cleanup_inputs(self, layers, world_size):
        """Run original startup/empty-Radio regions over supplied actor state.

        The shared effect-world owner executes40B540..40B5AB,
        725850..725886 and4E6D60..4E6D96, stopping before CRT atexit.
        This is initialization, not a full constructor or fatal-world witness.
        """
        from tools.projectile_oracle.ifv_impact import initialize_effect_world
        initialize_effect_world(SimpleNamespace(u=self.uc, read32=self.get), {},
                                seed=self.seed, map_size=world_size, clear_terrain=False)
        self.uc.reg_write(UC_X86_REG_ESP, self.stack)
        run_checked(self.uc, 0x4A8630, 0x4A8672, required_addresses=[0x4A8630])
        self.set_layers(layers)
        # Original Tactical ctor6D1D54 supplies this vptr; actual expiry+28
        # 6DA560 executes on the initialized screen records, without a sink.
        self.put(self.tactical, 0x7F4348)
        for pointer in self.actors.values():
            # Only the original Radio65A750 vector allocation/empty-slot
            # region is run. Calling its whole base ctor over an already
            # supplied Unit would incorrectly overwrite the actor's prestate.
            self.uc.reg_write(UC_X86_REG_ESI, pointer)
            self.uc.reg_write(UC_X86_REG_ESP, self.stack)
            run_checked(self.uc, 0x65A758, 0x65A798, required_addresses=[0x65A758])
        if self.text_hash() != self.code_hash:
            raise RuntimeError("Original .text changed during cleanup initialization")

    def cleanup_snapshot(self):
        pointer = self.get(0xB0FE6C)
        retained = [] if not pointer else [
            self.identities[self.get(self.get(pointer + 4) + 4 * index)]
            for index in range(self.get(pointer + 16))]
        return dict(**self.selection_snapshot(), retained_navigation=retained,
                    actor_state={str(identity): dict(
                        alive=bool(byte(self.uc, actor_pointer + 0x90)),
                        selected=bool(byte(self.uc, actor_pointer + 0x83)),
                        limbo=bool(byte(self.uc, actor_pointer + 0x81)),
                        marked=bool(byte(self.uc, actor_pointer + 0x74)),
                        native_layer=i32(self.uc, actor_pointer + 0x94))
                        for identity, actor_pointer in self.actors.items()},
                    layer_order=[[
                        self.identities[self.get(self.get(vector + 4) + 4 * index)]
                        for index in range(self.get(vector + 16))]
                        for vector in range(0x8A0360, 0x8A03D8, 24)],
                    screen_order=[self.identities[self.get(0xB0CEC8 + 12 * index)]
                                  for index in range(self.get(self.tactical + 0xDB0))],
                    map_order=[self.identities[self.get(self.get(0xA8EC7C) + 4 * index)]
                               for index in range(self.get(0xA8EC88))])

    def selection_snapshot(self):
        return dict(**self.snapshot(), across_map=bool(byte(self.uc, 0xB0FE64)),
                    submode=byte(self.uc, 0xB0FE58),
                    action_timer=list(struct.unpack("<3i", self.uc.mem_read(ACTION_TIMER, 12))))

    def navigation_snapshot(self):
        result = self.cleanup_snapshot()
        result.update(health_category=i32(self.uc, 0x845560),
                      veterancy_category=i32(self.uc, 0x845564))
        for identity, state in result["actor_state"].items():
            state["veterancy_raw_bits"] = self.get(self.actors[int(identity)] + 0x150)
        return result

    def selection_observations(self, before):
        names = ("screen_techno_gate", "alive_owner_gate", "dynamic_gate",
                 "combatant_final_gate", "select", "object_select", "deselect")
        calls = [dict(name=row["name"], id=self.identities.get(int(row["ecx"], 16)),
                      accepted=bool(row["return_eax"] & 255))
                 for row in self.trace.calls if row["name"] in names]
        return dict(**self.observations(before), selection_calls=calls,
                    message_keys=self.message_keys, messages=self.messages,
                    timer_writes=self.timer_writes,
                    detach_calls=[name for name in self.call_order
                                  if name in ("world_detach", "abstract_detach",
                                              "object_detach_all", "foot_detach_all")])


def search_case(spec):
    fixture = SelectionFixture(spec["actors"], spec["layers"], **spec.get("house", {}))
    before = fixture.rng_bytes()
    pointer = fixture.invoke(SEARCH[spec["direction"]], DISPLAY,
                             [fixture.actors.get(spec["anchor"], 0)])
    return {**spec, "candidate": fixture.identities[pointer],
            "visits": fixture.visits(),
            "rng_unchanged": before == fixture.rng_bytes()}


def command_history(spec):
    fixture = SelectionFixture(spec["actors"], spec["layers"],
                               voice_enabled=spec.get("voice_enabled", True))
    fixture.set_selection(spec["selected"])
    fixture.set_display(follow=spec.get("follow"), modes=spec.get("modes"),
                        placement=spec.get("placement", False))
    fixture.put(0xB0FE54, 3)
    put8(fixture.uc, 0xB0FE58, True)
    steps = []
    for direction in spec["directions"]:
        fixture.reset_observations()
        before = fixture.rng_bytes()
        obj = fixture.objects[COMMAND[direction]]
        entry = fixture.get(fixture.get(obj) + 0x20)
        fixture.invoke(entry, obj, [0])
        steps.append(dict(direction=direction, **fixture.snapshot(),
                          **fixture.observations(before)))
    return {**spec, "steps": steps}


def voice_history(spec):
    value = actor(1, spec["kind"], voice_list=spec["voice_list"],
                  queued_voice=777, owner=spec.get("owner", "local"),
                  slave=spec.get("slave", False),
                  robot_offline=spec.get("robot_offline", False))
    fixture = SelectionFixture([value], [[], [], [1], [], []],
                               seed=spec["seed"],
                               voice_enabled=spec.get("voice_enabled", True))
    if value["slave"]:
        fixture.set_voice_list(1, spec["voice_list"], 0x430)
    if value["robot_offline"]:
        fixture.set_voice_list(1, spec["voice_list"], 0x44C)
    prefix = {}
    if advance := spec.get("advance_main_raw", 0):
        before_prefix = fixture.rng_bytes()
        fixture.reset_observations()
        for _ in range(advance):
            fixture.invoke(0x65C780, fixture.rngs["main"])
        observed = fixture.observations(before_prefix)
        prefix["main_prefix"] = dict(
            draws=observed["main_draws"],
            before_state_hex=before_prefix["main"].hex(),
            after_state_hex=fixture.rng_bytes()["main"].hex(),
        )
        assert len(observed["main_draws"]) == advance
    initial = fixture.rng_bytes()
    steps = []
    for _ in range(spec.get("calls", 4)):
        before = fixture.rng_bytes()
        fixture.reset_observations()
        fixture.invoke(0x708EB0, fixture.actors[1])
        steps.append(fixture.observations(before))
    after = fixture.rng_bytes()
    continuation, _ = draws(after["main"], 4)
    return {**spec, **prefix, "initial_queued_voice": 777, "steps": steps,
            "rng_before_hex": {name: raw.hex() for name, raw in initial.items()},
            "rng_after_hex": {name: raw.hex() for name, raw in after.items()},
            "main_next_four": continuation}


def selection_history(spec):
    navigation = spec.get("navigation_trace", False)
    fixture = SelectionFixture(spec["actors"], spec["layers"],
                               selection_commands=True,
                               navigation_commands=navigation,
                               voice_enabled=spec.get("voice_enabled", True),
                               seed=spec.get("seed", 1), **spec.get("house", {}))
    cleanup = "retained_navigation" in spec
    if spec.get("setup_cleanup", False):
        fixture.initialize_cleanup_inputs(spec["layers"], spec["cleanup_world_size"])
    fixture.set_selection_sources(spec["screen_order"], spec["map_order"])
    fixture.set_selection(spec["selected"])
    fixture.set_display(follow=spec.get("follow"), modes=spec.get("modes"),
                        placement=spec.get("placement", False))
    fixture.put(0xB0FE54, spec.get("selection_mode", 0))
    put8(fixture.uc, 0xB0FE64, spec.get("across_map", False))
    put8(fixture.uc, 0xB0FE58, spec.get("submode", True))
    fixture.put(0xA8B538, spec.get("command_guard", 0))
    if "action_timer" in spec:
        fixture.uc.mem_write(ACTION_TIMER, struct.pack("<3i", *spec["action_timer"]))
    scope_writes, category_writes = [], []
    if cleanup:
        # Actual732050 constructs the selected snapshot. AssignmentB0FE6C
        # supplies an earlier Health/Y snapshot surviving while P owns mode1.
        if navigation:
            # Also permits explicit duplicate-pointer boundary controls. The
            # actual selected source constructor, not Python, copies them.
            fixture.set_selection(spec["retained_navigation"])
        fixture.put(0xB0FE6C, fixture.invoke(0x732050, 0))
        if navigation:
            fixture.set_selection(spec["selected"])
        if fixture.cleanup_snapshot()["retained_navigation"] != spec["retained_navigation"]:
            raise RuntimeError("Native selected snapshot differs from supplied cleanup prestate")

    if cleanup or navigation:
        def observe_scope(u, _access, address, size, value, _data):
            scope_writes.append(dict(pc=hex(u.reg_read(UC_X86_REG_EIP)),
                                     address=hex(address), bytes=size, value=value))
        fixture.uc.hook_add(UC_HOOK_MEM_WRITE, observe_scope,
                            begin=0xB0FE54, end=0xB0FE67)
    if navigation:
        for key, address in (("health_category", 0x845560), ("veterancy_category", 0x845564)):
            if key in spec:
                fixture.put(address, spec[key] & 0xFFFFFFFF)

        def observe_category(u, _access, address, size, value, _data):
            category_writes.append(dict(pc=hex(u.reg_read(UC_X86_REG_EIP)),
                                        address=hex(address), bytes=size, value=value))

        fixture.uc.hook_add(UC_HOOK_MEM_WRITE, observe_category,
                            begin=0x845560, end=0x845567)
    snapshot = (fixture.navigation_snapshot if navigation else
                fixture.cleanup_snapshot if cleanup else fixture.selection_snapshot)
    voice_writes = []
    if spec.get("enslaved_voice_trace", False):
        native_snapshot = snapshot

        def snapshot():
            result = native_snapshot()
            for identity, state in result["actor_state"].items():
                state["slave_owner_present"] = bool(fixture.get(fixture.actors[int(identity)] + 0x2DC))
                state["queued_voice"] = i32(fixture.uc, fixture.actors[int(identity)] + 0x4F0)
            return result

        def observe_queued_voice(u, _access, address, size, value, identity):
            if address != fixture.actors[identity] + 0x4F0 or size != 4:
                raise RuntimeError("Unexpected native queued-voice write span")
            voice_writes.append(dict(id=identity, pc=hex(u.reg_read(UC_X86_REG_EIP)),
                                     sound_id=struct.unpack("<i", struct.pack("<I", value))[0]))

        for identity, pointer in fixture.actors.items():
            fixture.uc.hook_add(UC_HOOK_MEM_WRITE, observe_queued_voice, identity,
                                begin=pointer + 0x4F0, end=pointer + 0x4F3)
    initial = fixture.rng_bytes()
    steps = []
    for command in spec["commands"]:
        fixture.reset_observations()
        scope_writes.clear()
        category_writes.clear()
        voice_writes.clear()
        before = fixture.rng_bytes()
        prior = snapshot()
        name, key_word = command["command"], command.get("key_word", 0)
        if name == "ordinary_select":
            fixture.invoke(0x6FBFA0, fixture.actors[command["id"]])
        elif name == "ordinary_deselect":
            fixture.invoke(0x5F44A0, fixture.actors[command["id"]])
        elif name == "unselect_all":
            fixture.invoke(0x6DA740, fixture.tactical)
        elif name == "pointer_expiry":
            fixture.invoke(0x733160, fixture.actors[command["id"]])
        elif name == "object_detach_all":
            fixture.invoke(0x5F5280, fixture.actors[command["id"]], [int(command["all"])])
        elif name == "object_conceal":
            fixture.invoke(0x5F4D30, fixture.actors[command["id"]])
        elif name == "free_slave_owner_writer":
            pointer = fixture.actors[command["id"]]
            if byte(fixture.uc, pointer + 0x81):
                raise RuntimeError("Outside FreeSlaves writer requires declared non-Limbo actor")
            fixture.uc.reg_write(UC_X86_REG_ESI, pointer)
            fixture.run_region(0x6B0B6D, 0x6B0B98, required=(0x6B0B73,))
        elif name == "supply_veterancy_raw_bits":
            fixture.put(fixture.actors[command["id"]] + 0x150, command["bits"])
        elif name == "supply_actor_state":
            pointer = fixture.actors[command["id"]]
            for field, value in command["fields"].items():
                if field == "health":
                    fixture.put(pointer + 0x6C, value & 0xFFFFFFFF)
                else:
                    put8(fixture.uc, pointer + ACTOR_BYTES[field], value)
        elif name == "supply_command_guard":
            fixture.put(0xA8B538, command["value"])
        elif name == "reset_selection_mode":
            fixture.invoke(0x731D00, 0)
        elif name == "supply_selection_sources":
            # Post-retirement registry membership is an explicit input seam;
            # mapped storage remains. No destructor/free is claimed here.
            fixture.set_selection_sources(command["screen_order"], command["map_order"])
            fixture.set_layers(command["layers"])
        else:
            obj = fixture.objects[dict(combatant="CombatantSelect", type="TypeSelect",
                                       health="HealthNav", veterancy="VeterancyNav", next="NextObject",
                                       previous="PreviousObject")[name]]
            entry = fixture.get(fixture.get(obj) + 0x20)
            if name == "type":
                fixture.invoke(entry, obj, [key_word & ~0x800])
                fixture.time_ms += 100
                fixture.invoke(entry, obj, [key_word | 0x800])
            else:
                fixture.invoke(entry, obj, [key_word])
        step = {**command, "key_word": key_word, "before": prior,
                **snapshot(), **fixture.selection_observations(before)}
        if cleanup or navigation:
            after = fixture.rng_bytes()
            continuation, _ = draws(after["main"], 4)
            step.update(native_calls=[dict(**row, id=fixture.identities.get(int(row["ecx"], 16)))
                                      for row in fixture.trace.calls],
                        scope_writes=list(scope_writes),
                        rng_before_hex={name: raw.hex() for name, raw in before.items()},
                        rng_after_hex={name: raw.hex() for name, raw in after.items()},
                        main_next_four=continuation)
        if navigation:
            step.update(category_writes=list(category_writes),
                        rank_calls=[dict(id=fixture.identities.get(int(row["ecx"], 16) - 0x150),
                                         result=row["return_eax"])
                                    for row in fixture.trace.calls if row["name"] == "veterancy_rank"])
        if spec.get("enslaved_voice_trace", False):
            step.update(voice_writes=list(voice_writes),
                        accepted_voice_requests=[dict(id=row["id"], sound_id=row["sound_id"])
                                                 for row in voice_writes])
        steps.append(step)
    after = fixture.rng_bytes()
    continuation, _ = draws(after["main"], 4)
    return {**spec, "steps": steps, "csf_sha256": fixture.csf_sha256,
            "rng_before_hex": {name: raw.hex() for name, raw in initial.items()},
            "rng_after_hex": {name: raw.hex() for name, raw in after.items()},
            "main_next_four": continuation}


def navigation_actor(identity, kind="unit", **fields):
    """Explicit additional inputs only; historical actor defaults stay unchanged."""
    return actor(identity, kind, veterancy_raw_bits=fields.pop("veterancy_raw_bits", 0),
                 cost=fields.pop("cost", 200 if kind == "infantry" else 700),
                 is_selectable_combatant=fields.pop("is_selectable_combatant", True),
                 positive_primary_damage=fields.pop("positive_primary_damage", True),
                 **fields)


def navigation_input(name, *, actors=None, screen=(20, 10, 40),
                     map_order=None, selected=(), commands=None, **fields):
    if actors is None:
        actors = [navigation_actor(20, "infantry", type_id="E1"),
                  navigation_actor(10, type_id="MTNK", veterancy_raw_bits=0x3F800000),
                  navigation_actor(40, type_id="MTNK", veterancy_raw_bits=0x40000000)]
    result = dict(id=name, actors=actors, layers=[[], [], [a["id"] for a in actors], [], []],
                  house=dict(campaign=False, other_human=False, other_control=False),
                  seed=1, screen_order=list(screen),
                  map_order=list(map_order if map_order is not None else screen),
                  selected=list(selected), follow=None, modes={name: False for name in MODE_FIELDS},
                  placement=False, selection_mode=0, across_map=True, submode=1,
                  command_guard=0, action_timer=[995, 0, 25], voice_enabled=True,
                  navigation_trace=True, cost_factors_bits=[0x3F800000] * 5,
                  commands=commands or [dict(command="veterancy")])
    result.update(fields)
    return result


def veterancy_inputs():
    y = dict(command="veterancy")
    shift_y = dict(command="veterancy", key_word=0x100)
    for name, commands, fields in (
        ("selected_cycle", [y] * 4, dict(selected=[20, 10, 40], follow=20)),
        ("fresh_shift_clears", [shift_y], dict(selected=[20, 10, 40], follow=20)),
        ("stock_costs_cycle", [y] * 3, dict(selected=[20, 10, 40])),
        ("continuing_shift_mixed", [y, shift_y, shift_y], dict(selected=[20, 10, 40], follow=20)),
        ("fallback_snapshot", [y] * 3, {}),
        ("placement_refuses", [y] * 3, dict(selected=[20, 10, 40], follow=20, placement=True)),
        ("rank_live_recheck", [y, dict(command="supply_veterancy_raw_bits", id=20, bits=0x3F800000), y],
         dict(selected=[20, 10, 40])),
        ("Y_Health_Y_P_Y_T_Y", [dict(command=name) for name in
                              ("veterancy", "health", "veterancy", "combatant", "veterancy", "type", "veterancy")],
         dict(selected=[20, 10, 40])),
        ("deselect_retains_Y_cycle", [y, dict(command="ordinary_deselect", id=40), y],
         dict(selected=[20, 10, 40], follow=20)),
        ("pointer_expired_leaf", [y, dict(command="pointer_expiry", id=10), y],
         dict(selected=[20, 10, 40])),
        ("guarded_fresh_preserves", [y], dict(selected=[20, 10, 40], follow=20, command_guard=1)),
        ("guarded_continuation_preserves", [y, dict(command="supply_command_guard", value=1), shift_y],
         dict(selected=[20, 10, 40], follow=20)),
        ("ordinary_select_resets_then_Y", [y, dict(command="ordinary_select", id=20), y],
         dict(selected=[20, 10, 40])),
        ("unselect_all_resets_then_Y", [y, dict(command="unselect_all"), y],
         dict(selected=[20, 10, 40])),
        ("reset_mode_discards_old_snapshot_on_fresh_Y", [dict(command="reset_selection_mode"), y],
         dict(selected=[40], selection_mode=4, retained_navigation=[20, 10, 40], veterancy_category=1)),
        ("forged_continuation_original_minus_one_category", [y],
         dict(selected=[20, 10, 40], selection_mode=4, retained_navigation=[20, 10, 40])),
        ("voice_disabled", [y] * 3, dict(selected=[20, 10, 40], voice_enabled=False)),
    ):
        yield navigation_input(name, commands=commands, **fields)
    yield navigation_input("empty_rank_does_not_expand_map",
                           actors=[navigation_actor(20), navigation_actor(40, veterancy_raw_bits=0x40000000)],
                           screen=[20], map_order=[40, 20], commands=[y] * 3)
    yield navigation_input("empty_everywhere", actors=[], screen=[], commands=[y, shift_y])
    yield navigation_input("screen_stored_order_without_cost_sort",
                           actors=[navigation_actor(20, "infantry", cost=200, veterancy_raw_bits=0x40000000),
                                   navigation_actor(10, cost=1500, veterancy_raw_bits=0x40000000),
                                   navigation_actor(40, cost=700, veterancy_raw_bits=0x40000000)],
                           screen=[40, 20, 10], map_order=[10, 20, 40], commands=[y])
    for label, house in (
        ("skirmish_other_control", dict(campaign=False, other_human=False, other_control=True)),
        ("campaign_other_control", dict(campaign=True, other_human=False, other_control=True)),
        ("campaign_other_human_only", dict(campaign=True, other_human=True, other_control=False)),
    ):
        yield navigation_input(label,
                               actors=[navigation_actor(20, owner="other", veterancy_raw_bits=0x40000000),
                                       navigation_actor(10, veterancy_raw_bits=0x40000000),
                                       navigation_actor(40, "building", veterancy_raw_bits=0x40000000)],
                               house=house)
    for name, fields in (
        ("slave", dict(slave=True)), ("bunker", dict(bunker=True)),
        ("robot_offline", dict(robot_offline=True)), ("limbo", dict(limbo=True)),
        ("undiscovered", dict(discovered=False)), ("alive_false", dict(alive=False)),
        ("not_selectable", dict(selectable=False)), ("mission_only", dict(mission_only=True)),
        ("health_zero", dict(health=0)), ("not_in_playfield", dict(in_playfield=False)),
        ("no_techno_cast", dict(techno_cast=False)), ("virtual_1d4", dict(virtual_1d4=True)),
    ):
        yield navigation_input("fallback_" + name,
                               actors=[navigation_actor(20, veterancy_raw_bits=0x40000000, **fields)],
                               screen=[20])
    yield navigation_input("selected_building_other_dead",
                           actors=[navigation_actor(20, "building", owner="other", alive=False,
                                                    veterancy_raw_bits=0x40000000)],
                           screen=[], selected=[20])
    yield navigation_input("empty_voice_list", actors=[navigation_actor(20, voice_list=[],
                                                                       veterancy_raw_bits=0x40000000)],
                           screen=[20])
    yield navigation_input("mixed_voice_lists_and_native_draw_order",
                           actors=[navigation_actor(20, "infantry", voice_list=[101, 202, 303],
                                                    veterancy_raw_bits=0x40000000),
                                   navigation_actor(10, voice_list=[101],
                                                    veterancy_raw_bits=0x40000000)],
                           screen=[10, 20], selected=[20, 10])


def enslaved_voice_inputs():
    """Additional list/SlaveOwner inputs, never a slave lifecycle substitute."""
    y = dict(command="veterancy")
    for kind, type_id in (("infantry", "E1"), ("unit", "MTNK")):
        for label, fields in (
            ("empty", dict(voice_list=[101], enslaved_voice_list=[])),
            ("singleton", dict(voice_list=[101], enslaved_voice_list=[202])),
            ("multiple", dict(voice_list=[101], enslaved_voice_list=[202, 303, 404])),
            ("normal_empty", dict(voice_list=[], enslaved_voice_list=[202])),
            ("minus_one_queue_rejection", dict(voice_list=[101], enslaved_voice_list=[-1])),
            ("unenslaved_uses_normal", dict(slave=False, voice_list=[101], enslaved_voice_list=[202])),
        ):
            values = dict(slave=True, type_id=type_id, veterancy_raw_bits=0x40000000)
            values.update(fields)
            yield navigation_input(kind + "_enslaved_" + label,
                                   actors=[navigation_actor(20, kind, **values)], screen=[20],
                                   enslaved_voice_trace=True, commands=[y] * 4)
        for label, voices in (("empty", []), ("nonempty", [202, 303, 404])):
            yield navigation_input(kind + "_freed_" + label + "_then_normal",
                                   actors=[navigation_actor(20, kind, type_id=type_id, slave=True,
                                                            voice_list=[101], enslaved_voice_list=voices,
                                                            veterancy_raw_bits=0x40000000)],
                                   screen=[20], enslaved_voice_trace=True,
                                   commands=[y, dict(command="free_slave_owner_writer", id=20,
                                                     entry="0x6b0b6d", stop="0x6b0b98"),
                                             dict(command="reset_selection_mode"), y])
    for label, fields in (
        ("disabled_select_caller", dict(voice_enabled=False)),
        ("placement_refused", dict(placement=True)),
        ("guarded_no_dispatch", dict(command_guard=1)),
        ("fresh_shift_reselects", dict(selected=[20], commands=[dict(command="veterancy", key_word=0x100)])),
        ("continuing_shift_already_selected", dict(selected=[20], retained_navigation=[20],
                                                   selection_mode=4, veterancy_category=2,
                                                   commands=[dict(command="veterancy", key_word=0x100)])),
        ("seed_zero", dict(seed=0)),
        ("seed_1234", dict(seed=1234)),
    ):
        yield navigation_input("enslaved_" + label,
                               actors=[navigation_actor(20, "infantry", type_id="E1", slave=True,
                                                        voice_list=[101], enslaved_voice_list=[202, 303, 404],
                                                        veterancy_raw_bits=0x40000000)], screen=[20],
                               enslaved_voice_trace=True, **fields)
    for label, house in (
        ("other_owner_outer_rejection", dict(campaign=False, other_human=False, other_control=False)),
        ("passive_other_owner_queue_rejection", dict(campaign=False, other_human=False,
                                                    other_control=False, other_passive=True)),
        ("campaign_other_human_control", dict(campaign=True, other_human=True, other_control=True)),
    ):
        yield navigation_input("enslaved_" + label,
                               actors=[navigation_actor(20, "infantry", type_id="E1", owner="other", slave=True,
                                                        voice_list=[101], enslaved_voice_list=[202],
                                                        veterancy_raw_bits=0x40000000)], screen=[], selected=[20],
                               enslaved_voice_trace=True, house=house)
    yield navigation_input("enslaved_final_select_refusal",
                           actors=[navigation_actor(20, "infantry", type_id="E1", slave=True, selectable=False,
                                                    voice_list=[101], enslaved_voice_list=[202],
                                                    veterancy_raw_bits=0x40000000)], screen=[20],
                           enslaved_voice_trace=True)
    yield navigation_input("enslaved_and_normal_each_added_in_native_order",
                           actors=[navigation_actor(20, "infantry", type_id="E1", slave=True,
                                                    voice_list=[101], enslaved_voice_list=[202, 303, 404],
                                                    veterancy_raw_bits=0x40000000),
                                   navigation_actor(10, type_id="MTNK", voice_list=[501, 502, 503],
                                                    enslaved_voice_list=[202], veterancy_raw_bits=0x40000000)],
                           screen=[20, 10], selected=[10, 20], enslaved_voice_trace=True)


def enslaved_voice_type_histories():
    """Compose current type/INI, source-order sound and native trace owners."""
    from tools.input_oracle.area_guard import sound_name
    from tools.spatial_oracle.anytown_damage.mtnk_attack import sound_inputs
    root = Path(os.environ.get("VERA20K_COMBATANT_INPUTS", "ini"))
    sound_root = Path(os.environ.get("VERA20K_SELECTION_SOUND_INPUTS", "ini"))
    sound_paths = {path.name.upper(): path for path in sound_root.iterdir() if path.is_file()}
    if "SOUNDMD.INI" not in sound_paths:
        raise RuntimeError("Missing fixed SOUNDMD.INI; set VERA20K_SELECTION_SOUND_INPUTS")
    # Existing sound_inputs consumes its native uppercase filename. The
    # selected-byte directory is explicit when filesystem case is sensitive.
    m = BulletReader({}, root)
    registry = sound_inputs(m, sound_root, {},
                            wanted_names={"GISelect", "GenAllVehicleSelect", "SlaveFreedSelect", "SlaveWorkerSelect"})
    m.u.mem_write(0xA8E348, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
    rngs = {"main": 0x886B88, "scenario": m.read32(0xA8B230) + 0x218, "mapgen": 0xABE890}
    for pointer in rngs.values():
        m.u.mem_write(pointer, seed_bytes(1))
    m.u.mem_write(ACTION_TIMER, struct.pack("<3i", 995, 0, 25))
    text_hash = hashlib.sha256(bytes(m.u.mem_read(0x401000, 0x3E0000))).hexdigest()
    active_trace, timer_writes, field_writes = None, [], []
    observed_type = None
    specs = {
        0x710AF0: ("techno_type_constructor", 2, 8),
        0x5236A0: ("infantry_type_constructor", 1, 4),
        0x7470D0: ("unit_type_constructor", 1, 4),
        0x477BE0: ("vector_constructor", 2, 8),
        0x525430: ("sound_list_reader", 9, 36),
        0x528A10: ("string_reader", 5, 20),
        0x751520: ("sound_find_pointer", 0, 0),
        0x7515C0: ("sound_find_index", 0, 0),
        0x478720: ("vector_copy", 1, 4),
        0x477D20: ("vector_destructor", 0, 0),
        0x65C780: ("rng_raw", 0, 0),
        0x7258D0: ("pointer_expired_dispatch", 0, 0),
        0x70D150: ("action_timer_start", 0, 0),
    }

    def observe(u, pc, _size, _data):
        if active_trace is not None:
            sp = u.reg_read(UC_X86_REG_ESP)
            active_trace.returned(pc, sp)
            if pc in specs:
                active_trace.entered(pc, sp, specs[pc])

    def observe_write(u, _access, address, size, value, _data):
        if active_trace is None:
            return
        if address < ACTION_TIMER + 12 and address + size > ACTION_TIMER:
            timer_writes.append(dict(pc=hex(u.reg_read(UC_X86_REG_EIP)),
                                     offset=address - ACTION_TIMER, bytes=size, value=value))
        if observed_type is not None and address < observed_type + 0x44C and address + size > observed_type + 0x414:
            field_writes.append(dict(pc=hex(u.reg_read(UC_X86_REG_EIP)),
                                     offset=address - observed_type, bytes=size, value=value))

    m.u.hook_add(UC_HOOK_CODE, observe)
    m.u.hook_add(UC_HOOK_MEM_WRITE, observe_write)

    def rng_bytes():
        return {name: bytes(m.u.mem_read(pointer, STRUCT_LEN)) for name, pointer in rngs.items()}

    def binding(typ, offset):
        count, data = m.read32(typ + offset + 16), m.read32(typ + offset + 4)
        if count > 128:
            raise RuntimeError("Native sound vector exceeds declared control bound")
        ids = [i32(m.u, data + 4 * index) for index in range(count)]
        return dict(count=count, ids=ids, names=[sound_name(m, index) for index in ids])

    def begin(typ):
        nonlocal active_trace, observed_type
        active_trace, observed_type = NativeCallTrace(m.u, m.read32), typ
        timer_writes.clear()
        field_writes.clear()
        return rng_bytes()

    def observation(before, stop):
        nonlocal active_trace, observed_type
        active_trace.returned(stop, m.u.reg_read(UC_X86_REG_ESP))
        if active_trace.pending:
            raise RuntimeError(f"Unreturned native sound-reader calls: {active_trace.pending}; "
                               f"rows={active_trace.calls}")
        after = rng_bytes()
        if before != after:
            raise RuntimeError("Type voice construction/reading changed an RNG stream")
        if hashlib.sha256(bytes(m.u.mem_read(0x401000, 0x3E0000))).hexdigest() != text_hash:
            raise RuntimeError("Original .text changed during voice input execution")
        continuation, _ = draws(after["main"], 4)
        result = dict(seed=1, rng_before_hex={name: raw.hex() for name, raw in before.items()},
                      rng_after_hex={name: raw.hex() for name, raw in after.items()},
                      main_next_four=continuation, action_timer=list(struct.unpack("<3i", m.u.mem_read(ACTION_TIMER, 12))),
                      timer_writes=list(timer_writes), field_writes=list(field_writes),
                      native_calls=list(active_trace.calls))
        active_trace, observed_type = None, None
        return result

    constructors = []

    def construct(name, entry):
        typ = m.alloc(0x2000)
        m.u.mem_write(typ, b"\xA5" * 0x2000)
        before = begin(typ)
        arguments = [m.cstring(name)] + ([0] if entry == 0x710AF0 else [])
        m.invoke(entry, typ, arguments)
        if m.u.reg_read(UC_X86_REG_ESP) != SP + 4 * (len(arguments) + 1):
            raise RuntimeError("Unbalanced native voice type constructor")
        constructors.append(dict(type_id=name, entry=hex(entry), poison_byte=165,
                                 arguments=[name] + ([0] if entry == 0x710AF0 else []),
                                 normal=binding(typ, 0x414), enslaved=binding(typ, 0x430),
                                 **observation(before, RET_MAGIC)))
        return typ

    def read(typ, sections):
        m.rules_cache(sections)
        normal_before, before = binding(typ, 0x414), binding(typ, 0x430)
        rng_before = begin(typ)
        m.u.mem_write(SP, dwords(RET_MAGIC, RULES))
        m.u.reg_write(UC_X86_REG_ESP, SP)
        m.u.reg_write(UC_X86_REG_ECX, typ)
        stop = run_checked(m.u, 0x410A60, (0x410A8C, 0x410B7D),
                           count=200000, required_addresses=[0x526810])
        admitted = stop == 0x410A8C
        m.reads = []
        if admitted:
            for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, typ),
                                    (UC_X86_REG_EBX, typ + 0x24), (UC_X86_REG_ESI, RULES)):
                m.u.reg_write(register, value)
            stop = run_checked(m.u, 0x712B1D, 0x712BF1,
                               required_addresses=[0x525430, 0x478720, 0x477D20])
            if m.u.reg_read(UC_X86_REG_ESP) != SP:
                raise RuntimeError("Unbalanced normal/enslaved native voice reader blocks")
        return dict(before=before, after=binding(typ, 0x430), normal_before=normal_before,
                    normal_after=binding(typ, 0x414), admitted=admitted,
                    reads=[dict(reader=row["reader"], section=row["section"], key=row["key"], ini="rules")
                           for row in m.reads], **observation(rng_before, stop))

    paths = {path.name.upper(): path for path in root.iterdir() if path.is_file()}
    retail = []
    for name, entry in (("E1", 0x5236A0), ("MTNK", 0x7470D0), ("SLAV", 0x5236A0)):
        typ = construct(name, entry)
        layers = []
        for filename in ("RULESMD.INI", "LANGRULE.INI", "MPBATTLEMD.INI", "XMP03T4.MAP"):
            if filename not in paths:
                if filename != "LANGRULE.INI":
                    raise RuntimeError(f"Missing retail voice layer {filename}")
                layers.append(dict(file=filename, absent=True))
                continue
            raw = paths[filename].read_bytes()
            physical, lines = lexical(raw, {name})
            sections = {name: {key: value for key, value in physical[name].items()
                               if key in ("VoiceSelect", "VoiceSelectEnslaved")}} if name in physical else {}
            layers.append(dict(file=filename, sha256=hashlib.sha256(raw).hexdigest(),
                               sections=sections, source_lines=[line for line in lines
                                   if line["key"] in ("VoiceSelect", "VoiceSelectEnslaved")], **read(typ, sections)))
        retail.append(dict(type_id=name, layers=layers, normal=binding(typ, 0x414), enslaved=binding(typ, 0x430)))
    typ = construct("VOICE_CONTROL", 0x710AF0)
    authored = []
    for label, sections in (
        ("both_registered", {"VOICE_CONTROL": {"VoiceSelect": "GISelect", "VoiceSelectEnslaved": "SlaveWorkerSelect"}}),
        ("missing_key_retains", {"VOICE_CONTROL": {}}),
        ("missing_section_retains", {}),
        ("empty_retains", {"VOICE_CONTROL": {"VoiceSelectEnslaved": ""}}),
        ("whitespace_retains", {"VOICE_CONTROL": {"VoiceSelectEnslaved": "   "}}),
        ("unknown_replaces_empty", {"VOICE_CONTROL": {"VoiceSelectEnslaved": "AbsentEnslavedVoice"}}),
        ("restore", {"VOICE_CONTROL": {"VoiceSelectEnslaved": "SlaveWorkerSelect"}}),
        ("duplicates_case", {"VOICE_CONTROL": {"VoiceSelectEnslaved": "sLaVeWoRkErSeLeCt,SlaveFreedSelect,SlaveWorkerSelect"}}),
        ("internal_spaces_not_trimmed", {"VOICE_CONTROL": {"VoiceSelectEnslaved": "SlaveWorkerSelect, SlaveFreedSelect,SlaveFreedSelect ,sLaVeFrEeDsElEcT"}}),
        ("commas_replaces_empty", {"VOICE_CONTROL": {"VoiceSelectEnslaved": ",,,"}}),
        ("none_unknown_then_registered", {"VOICE_CONTROL": {"VoiceSelectEnslaved": "none,<none>,SlaveWorkerSelect"}}),
        ("wrong_key_case_retains", {"VOICE_CONTROL": {"voiceselectenslaved": "SlaveFreedSelect"}}),
        ("wrong_section_case_retains", {"voice_control": {"VoiceSelectEnslaved": "SlaveFreedSelect"}}),
        ("readstring_127_payload_cuts_suffix", {"VOICE_CONTROL": {"VoiceSelectEnslaved": "X" * 100 + ",SlaveWorkerSelect,GISelectX"}}),
        ("normal_changes_enslaved_retains", {"VOICE_CONTROL": {"VoiceSelect": "SlaveFreedSelect"}}),
    ):
        section = sections.get("VOICE_CONTROL", sections.get("voice_control", {}))
        key = next((key for key in section if key.lower() == "voiceselectenslaved"), None)
        authored.append(dict(name=label, key=key, raw=section.get(key), sections=sections, **read(typ, sections)))
    return dict(sound_registry=registry, constructors=constructors, retail=retail, authored=authored,
                reader_entry="0x712b1d", reader_stop="0x712bf1", enslaved_reader_entry="0x712b87",
                boundary="Full selected type constructors; admitted original normal/enslaved ReadSoundList blocks over supplied INI caches and selected fixed SOUNDMD registry. Whole type readers, physical INI loading and audio playback excluded.")


def health_navigation_inputs():
    h = dict(command="health")
    actors = [navigation_actor(20, "infantry", health=10, type_id="E1"),
              navigation_actor(10, health=40, type_id="MTNK", veterancy_raw_bits=0x3F800000),
              navigation_actor(40, health=100, type_id="MTNK", veterancy_raw_bits=0x40000000)]
    for name, fields in (
        ("health_selected_cycle", dict(selected=[20, 10, 40], commands=[h] * 4)),
        ("health_fresh_shift_clears", dict(selected=[20, 10, 40], commands=[dict(command="health", key_word=0x100)])),
        ("health_continuing_shift", dict(selected=[20, 10, 40],
                                        commands=[h, dict(command="health", key_word=0x100)])),
        ("health_fallback_snapshot", dict(commands=[h] * 3)),
        ("health_guarded_fresh", dict(selected=[20, 10, 40], follow=20, command_guard=1)),
        ("health_guarded_continuation", dict(selected=[20, 10, 40],
                                            commands=[h, dict(command="supply_command_guard", value=1), h])),
        ("health_forged_continuation_original_minus_one", dict(selected=[20, 10, 40],
                                                              selection_mode=3, retained_navigation=[20, 10, 40])),
        ("Health_Y_Health", dict(selected=[20, 10, 40],
                                 commands=[h, dict(command="veterancy"), h])),
    ):
        yield navigation_input(name, actors=actors, commands=fields.pop("commands", [h]), **fields)
    for campaign, human, control in ((False, False, True), (True, False, True), (True, True, False)):
        yield navigation_input(f"health_source_campaign{int(campaign)}_human{int(human)}_control{int(control)}",
                               actors=[navigation_actor(20, owner="other", health=10),
                                       navigation_actor(10, health=10),
                                       navigation_actor(40, "building", health=10)],
                               house=dict(campaign=campaign, other_human=human, other_control=control),
                               commands=[h])
    yield navigation_input("health_price_and_equal_cost_stored_order",
                           actors=[navigation_actor(20, "infantry", health=10, cost=200),
                                   navigation_actor(10, health=10, cost=700),
                                   navigation_actor(40, health=10, cost=700)],
                           screen=[40, 20, 10], selected=[40, 20, 10], commands=[h])
    yield navigation_input("health_empty_snapshot_retains_across_map", actors=[], screen=[], commands=[h])
    yield navigation_input("health_nonempty_without_category_retains_across_map",
                           actors=[navigation_actor(20, health=100)], screen=[20], commands=[h])


def veterancy_cleanup_inputs():
    y = dict(command="veterancy")
    actors = [navigation_actor(20, "infantry", type_id="E1", health=10, native_layer=2),
              navigation_actor(40, type_id="MTNK", health=40, native_layer=2, veterancy_raw_bits=0x3F800000),
              navigation_actor(50, type_id="MTNK", health=100, native_layer=2, veterancy_raw_bits=0x40000000)]
    for name, commands, fields in (
        ("deselect_without_expiry_retains_snapshot", [dict(command="ordinary_deselect", id=20), y, y], {}),
        ("alivefalse_deselect_without_expiry_retains_snapshot",
         [dict(command="supply_actor_state", id=20, fields=dict(alive=False)),
          dict(command="ordinary_deselect", id=20), y, y], {}),
        ("direct_pointer_expiry_stable_order", [dict(command="pointer_expiry", id=20), y, y], {}),
        ("registry_absence_after_actual_expiry",
         [dict(command="ordinary_deselect", id=20), dict(command="pointer_expiry", id=20),
          dict(command="supply_selection_sources", screen_order=[40], map_order=[50, 40],
               layers=[[], [], [40, 50], [], []]), y], {}),
        ("initialized_whole_detach_alive_preserving", [dict(command="object_detach_all", id=20, all=True), y],
         dict(setup_cleanup=True)),
        ("initialized_alivefalse_whole_detach",
         [dict(command="supply_actor_state", id=20, fields=dict(alive=False)),
          dict(command="object_detach_all", id=20, all=True), y], dict(setup_cleanup=True)),
        ("initialized_whole_conceal", [dict(command="object_conceal", id=20), y], dict(setup_cleanup=True)),
        ("no_match_pointer_expiry", [dict(command="pointer_expiry", id=50), y],
         dict(selected=[20, 40], retained_navigation=[20, 40])),
        ("repeated_pointer_expiry", [dict(command="pointer_expiry", id=20)] * 2 + [y], {}),
        ("supplied_duplicate_retained_snapshot_repeated_expiry",
         [dict(command="pointer_expiry", id=20)] * 3 + [y],
         dict(retained_navigation=[20, 40, 20, 50])),
        ("all_pointers_expired_empty_snapshot",
         [dict(command="pointer_expiry", id=identity) for identity in (20, 40, 50)] + [y], {}),
        ("Health_snapshot_expiry_then_Health_Y",
         [dict(command="pointer_expiry", id=20), dict(command="health"), y],
         dict(selection_mode=3, health_category=0)),
        ("Health_alivefalse_without_expiry",
         [dict(command="supply_actor_state", id=20, fields=dict(alive=False)),
          dict(command="ordinary_deselect", id=20), dict(command="health"), dict(command="health")],
         dict(selection_mode=3, health_category=0)),
    ):
        defaults = dict(selected=[20, 40, 50], follow=20, selection_mode=4,
                        veterancy_category=0, retained_navigation=[20, 40, 50],
                        setup_cleanup=False, cleanup_world_size=[64, 64])
        defaults.update(fields)
        yield navigation_input(name, actors=actors, screen=[20, 40, 50], map_order=[50, 40, 20],
                               commands=commands, **defaults)


def native_rng_bounds(fixture, before):
    after = fixture.rng_bytes()
    continuation, _ = draws(after["main"], 4)
    return dict(rng_before_hex={name: raw.hex() for name, raw in before.items()},
                rng_after_hex={name: raw.hex() for name, raw in after.items()},
                main_next_four=continuation, timer_writes=list(fixture.timer_writes))


def veterancy_rank_cases():
    rows = []
    for bits in (0x00000000, 0x80000000, 0x3F7FFFFF, 0x3F800000,
                 0x3F800001, 0x3FFFFFFF, 0x40000000, 0x40000001,
                 0x7F7FFFFF, 0xBF800000, 0x7F800000, 0xFF800000,
                 0x7FC00000, 0xFFC00000, 0x00000001, 0x80000001):
        fixture = SelectionFixture([navigation_actor(20)], [[], [], [20], [], []],
                                   selection_commands=True, navigation_commands=True)
        fixture.put(fixture.actors[20] + 0x150, bits)
        fixture.reset_observations()
        before = fixture.rng_bytes()
        rank = fixture.invoke(0x750030, fixture.actors[20] + 0x150)
        rows.append(dict(id=f"raw_{bits:08x}", raw_bits=bits, rank=rank,
                         seed=1, fpcw=0x0E7F, entry="0x750030",
                         native_calls=list(fixture.trace.calls), **native_rng_bounds(fixture, before)))
    return rows


def map_veterancy_seed_cases():
    """Original token-present branches and percent setter, before Unlimbo.

    The pointer is an explicit already-tokenized input. Empty string is a
    branch-boundary control, not a possible original strtok token. Aircraft
    executes its own caller slice over the shared+150 field; no flight object
    constructor, virtual or Unlimbo is executed.
    """
    rows = []
    tokens = ["0", "1", "2", "99", "100", "101", "150", "199", "200", "201",
              "250", "-1", "-100", "65535", "2147483647", "2147483648",
              "4294967295", "-2147483648", "-2147483649", "", "garbage",
              "  +100tail", "100.9", "1e2", "-0", "0x64", "999999999999999999999999"]
    for kind, field_index, entry, stop in (
        ("unit", 8, 0x743495, 0x7434AE),
        ("infantry", 9, 0x51FD54, 0x51FD6D),
        ("aircraft", 8, 0x41B308, 0x41B321),
    ):
        for index, (token, initial) in enumerate([(None, 0), (None, 0x3F800000),
                                                (None, 0x3EAAAAAB)] + [(token, 0) for token in tokens]):
            storage_kind = "infantry" if kind == "infantry" else "unit"
            fixture = SelectionFixture([navigation_actor(20, storage_kind)], [[], [], [20], [], []],
                                       selection_commands=True, navigation_commands=True)
            pointer = fixture.actors[20]
            fixture.put(pointer + 0x150, initial)
            token_pointer = 0
            if token is not None:
                raw = token.encode("ascii") + b"\0"
                token_pointer = fixture.alloc(len(raw))
                fixture.uc.mem_write(token_pointer, raw)
            fixture.uc.reg_write(UC_X86_REG_EAX, token_pointer)
            fixture.uc.reg_write(UC_X86_REG_ESI, pointer)
            fixture.uc.reg_write(UC_X86_REG_EDI, pointer)
            fixture.reset_observations()
            before = fixture.rng_bytes()
            fixture.run_region(entry, stop,
                               required=() if token is None else (0x7C9BFD, 0x7500E0))
            parsed = next((struct.unpack("<i", struct.pack("<I", row["args"][0]))[0]
                           for row in fixture.trace.calls if row["name"] == "set_veterancy_percent"), None)
            raw_after = fixture.get(pointer + 0x150)
            rank = fixture.invoke(0x750030, pointer + 0x150)
            rows.append(dict(id=f"{kind}_token_{index:02d}", kind=kind, field_index=field_index,
                             token=token, entry=hex(entry), stop=hex(stop),
                             raw_bits_before=initial, parsed_percent=parsed,
                             raw_bits_after=raw_after, rank=rank, seed=1, fpcw=0x0E7F,
                             token_transport="supplied already-tokenized pointer",
                             native_calls=list(fixture.trace.calls), **native_rng_bounds(fixture, before)))
    return rows


def veterancy_initialization_cases():
    """Original defaults and rank gates; named bounded regions, not whole spawn."""
    rows = []

    def fresh(kind="unit"):
        return SelectionFixture([navigation_actor(20, kind)], [[], [], [20], [], []],
                                selection_commands=True, navigation_commands=True)

    def country(fixture, name):
        fixture.uc.mem_write(0xA83C98, dwords(0x7EB6D4, fixture.alloc(4096), 1024, 1, 0, 10))
        pointer, text = fixture.alloc(0x2000), fixture.alloc(32)
        fixture.uc.mem_write(pointer, b"\xa5" * 0x2000)
        fixture.uc.mem_write(text, name.encode("ascii") + b"\0")
        fixture.invoke(0x5113F0, pointer, [text])
        return pointer

    def lists(fixture, pointer):
        return {key: dict(data_is_null=fixture.get(pointer + offset + 4) == 0,
                          count=fixture.get(pointer + offset + 16),
                          capacity=fixture.get(pointer + offset + 8),
                          capacity_increment=fixture.get(pointer + offset + 20))
                for key, offset in (("VeteranInfantry", 0x14C), ("VeteranUnits", 0x168),
                                    ("VeteranAircraft", 0x184))}

    def house_flags(fixture):
        pointer = fixture.houses["local"]
        fixture.uc.mem_write(pointer + 0x2BC, b"\xa5" * 5)
        fixture.uc.reg_write(UC_X86_REG_EBX, 0xA5A5A5A5)
        fixture.run_region(0x4F54B2, 0x4F54B4)  # Actual constructor XOR EBX.
        fixture.uc.reg_write(UC_X86_REG_EBP, pointer)
        fixture.run_region(0x4F5855, 0x4F5873)
        return dict(barracks_infiltrated=bool(byte(fixture.uc, pointer + 0x2BF)),
                    war_factory_infiltrated=bool(byte(fixture.uc, pointer + 0x2C0)),
                    adjacent_infiltration_bytes=list(fixture.uc.mem_read(pointer + 0x2BC, 5)))

    for name in ("Americans", "YuriCountry"):
        fixture = fresh()
        fixture.reset_observations()
        before = fixture.rng_bytes()
        pointer = country(fixture, name)
        rows.append(dict(id="country_constructor_" + name, case_kind="country_constructor",
                         entry="0x5113f0", type_id=name, poison_byte=165,
                         lists=lists(fixture, pointer), seed=1,
                         native_calls=list(fixture.trace.calls), **native_rng_bounds(fixture, before)))
    fixture = fresh()
    fixture.reset_observations()
    before = fixture.rng_bytes()
    flags = house_flags(fixture)
    rows.append(dict(id="house_constructor_infiltration_writers", case_kind="house_initialization",
                     regions=[["0x4f54b2", "0x4f54b4"], ["0x4f5855", "0x4f5873"]],
                     poison_byte=165, **flags, seed=1,
                     native_calls=list(fixture.trace.calls), **native_rng_bounds(fixture, before)))
    for kind in ("unit", "infantry"):
        fixture = fresh(kind)
        pointer = fixture.actors[20]
        fixture.put(pointer + 0x150, 0x3F800000)
        fixture.uc.reg_write(UC_X86_REG_ESI, pointer)
        fixture.uc.reg_write(UC_X86_REG_EBX, 0)
        fixture.reset_observations()
        before = fixture.rng_bytes()
        fixture.run_region(0x6F2BDC, 0x6F2BED, required=(0x74FF30,))
        rank = fixture.invoke(0x750030, pointer + 0x150)
        rows.append(dict(id="techno_raw_constructor_" + kind, case_kind="raw_constructor",
                         kind=kind, entry="0x6f2bdc", stop="0x6f2bed",
                         raw_bits_before=0x3F800000, raw_bits_after=fixture.get(pointer + 0x150),
                         rank=rank, seed=1, native_calls=list(fixture.trace.calls),
                         **native_rng_bounds(fixture, before)))
    for kind in ("unit", "infantry"):
        cases = [("fresh_no_list_no_infiltration", "empty", False, True, False),
                 ("country_match_even_untrainable", "match", False, False, False),
                 ("country_nonmatch", "nonmatch", False, True, False),
                 ("infiltrated_trainable", "empty", True, True, False),
                 ("infiltrated_untrainable", "empty", True, False, False)]
        if kind == "unit":
            cases.append(("infiltrated_trainable_naval", "empty", True, True, True))
        for name, vector_input, infiltrated, trainable, naval in cases:
            fixture = fresh(kind)
            pointer, typ = fixture.actors[20], fixture.types[20]
            c = country(fixture, "Americans")
            fixture.put(fixture.houses["local"] + 0x34, c)
            house_flags(fixture)
            offset = 0x168 if kind == "unit" else 0x14C
            if vector_input != "empty":
                items = fixture.alloc(4)
                fixture.put(items, typ if vector_input == "match" else fixture.alloc(0x1000))
                fixture.put(c + offset + 4, items)
                fixture.put(c + offset + 8, 1)
                fixture.put(c + offset + 16, 1)
            put8(fixture.uc, fixture.houses["local"] + (0x2C0 if kind == "unit" else 0x2BF), infiltrated)
            put8(fixture.uc, typ + 0xC8E, trainable)
            put8(fixture.uc, typ + 0xCCE, naval)
            fixture.uc.reg_write(UC_X86_REG_ESI, pointer)
            fixture.uc.reg_write(UC_X86_REG_EDI, typ)
            fixture.reset_observations()
            before = fixture.rng_bytes()
            entry, stop = (0x735604, 0x735678) if kind == "unit" else (0x517CE7, 0x517D51)
            fixture.run_region(entry, stop)
            raw = fixture.get(pointer + 0x150)
            rank = fixture.invoke(0x750030, pointer + 0x150)
            rows.append(dict(id=kind + "_" + name, case_kind="constructor_rank_gate", kind=kind,
                             entry=hex(entry), stop=hex(stop), country_list_input=vector_input,
                             infiltrated=infiltrated, trainable=trainable, naval=naval,
                             raw_bits_before=0, raw_bits_after=raw, rank=rank, seed=1,
                             native_calls=list(fixture.trace.calls), **native_rng_bounds(fixture, before)))
    # Full Country constructors plus actual three list-reader blocks on the
    # physical lexical layers. This reuses the existing INI/TLS/heap authority;
    # physical file loading and the remainder of CountryReadINI are excluded.
    root = Path(os.environ.get("VERA20K_COMBATANT_INPUTS", str(HERE.parent.parent / "target/asset/combatant-inputs/extract")))
    paths = {path.name.upper(): path for path in root.iterdir() if path.is_file()}
    keys = ("VeteranInfantry", "VeteranUnits", "VeteranAircraft")
    for name in ("Americans", "YuriCountry"):
        m = BulletReader({}, root)
        m.u.mem_write(0xA83C98, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
        pointer = m.alloc(0x2000)
        m.u.mem_write(pointer, b"\xa5" * 0x2000)
        m.invoke(0x5113F0, pointer, (m.cstring(name),))
        rngs = {"main": 0x886B88, "scenario": m.read32(0xA8B230) + 0x218, "mapgen": 0xABE890}
        for rng in rngs.values():
            m.u.mem_write(rng, seed_bytes(1))
        text_hash = hashlib.sha256(bytes(m.u.mem_read(0x401000, 0x3E0000))).hexdigest()
        m.u.mem_write(ACTION_TIMER, struct.pack("<3i", 995, 0, 25))
        m.u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
        layers = []
        for filename in ("RULESMD.INI", "LANGRULE.INI", "MPBATTLEMD.INI", "XMP03T4.MAP"):
            if filename not in paths:
                if filename != "LANGRULE.INI":
                    raise RuntimeError("Missing physical country list layer " + filename)
                layers.append(dict(file=filename, absent=True))
                continue
            raw = paths[filename].read_bytes()
            physical, _ = lexical(raw, {name})
            sections = {name: {key: value for key, value in physical[name].items() if key in keys}} if name in physical else {}
            m.rules_cache(sections)
            m.reads = []
            before = {key: bytes(m.u.mem_read(p, STRUCT_LEN)) for key, p in rngs.items()}
            for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBX, pointer),
                                    (UC_X86_REG_ESI, RULES), (UC_X86_REG_EDI, pointer + 0x24)):
                m.u.reg_write(register, value)
            run_checked(m.u, 0x511D16, 0x51208C, count=200000, required_addresses=(0x528A10,))
            if m.u.reg_read(UC_X86_REG_ESP) != SP:
                raise RuntimeError("Unbalanced original country-list reader")
            after = {key: bytes(m.u.mem_read(p, STRUCT_LEN)) for key, p in rngs.items()}
            continuation, _ = draws(after["main"], 4)
            layers.append(dict(file=filename, sha256=hashlib.sha256(raw).hexdigest(), sections=sections,
                               reads=[row for row in m.reads if row["reader"] == "0x528a10"],
                               lists={key: dict(data_is_null=m.read32(pointer + offset + 4) == 0,
                                               count=m.read32(pointer + offset + 16))
                                      for key, offset in zip(keys, (0x14C, 0x168, 0x184))},
                               default_string_hex=bytes(m.u.mem_read(0x889F64, 1)).hex(),
                               rng_before_hex={key: state.hex() for key, state in before.items()},
                               rng_after_hex={key: state.hex() for key, state in after.items()},
                               main_next_four=continuation, timer_after=list(struct.unpack("<3i", m.u.mem_read(ACTION_TIMER, 12)))))
        if text_hash != hashlib.sha256(bytes(m.u.mem_read(0x401000, 0x3E0000))).hexdigest():
            raise RuntimeError("Original .text changed during country-list read")
        rows.append(dict(id="retail_country_list_reader_" + name, case_kind="country_list_reader",
                         type_id=name, entry="0x511d16", stop="0x51208c", layers=layers))
    return rows


def combatant_cleanup_inputs():
    """Six completed cleanup-to-P controls; initialized memory, not fatal AI."""
    for name, alive, setup, commands in (
        ("direct_deselect_then_P", True, False,
         [dict(command="ordinary_deselect", id=20), dict(command="combatant")]),
        ("alivefalse_deselect_expiry_then_P", False, False,
         [dict(command="ordinary_deselect", id=20), dict(command="pointer_expiry", id=20),
          dict(command="combatant")]),
        ("absent_registry_after_deselect_expiry_then_P", False, False,
         [dict(command="ordinary_deselect", id=20), dict(command="pointer_expiry", id=20),
          dict(command="supply_selection_sources", screen_order=[40], map_order=[50, 40],
               layers=[[], [], [40, 50], [], []]), dict(command="combatant")]),
        ("initialized_object_detach_all_true_then_P", True, True,
         [dict(command="object_detach_all", id=20, all=True), dict(command="combatant")]),
        ("initialized_alivefalse_object_detach_all_then_P", False, True,
         [dict(command="object_detach_all", id=20, all=True), dict(command="combatant")]),
        ("initialized_object_conceal_then_P", True, True,
         [dict(command="object_conceal", id=20), dict(command="combatant")]),
    ):
        actors = [actor(20, alive=alive, is_selectable_combatant=True,
                        positive_primary_damage=True, marked=False, native_layer=2),
                  actor(40, "infantry", is_selectable_combatant=True,
                        positive_primary_damage=True, marked=False, native_layer=2),
                  actor(50, is_selectable_combatant=True,
                        positive_primary_damage=True, marked=False, native_layer=2)]
        yield dict(id=name, actors=actors, layers=[[], [], [20, 40, 50], [], []],
                   house=dict(campaign=False, other_human=False, other_control=False),
                   seed=1, screen_order=[40], map_order=[50, 40, 20], selected=[20],
                   follow=20, modes={name: False for name in MODE_FIELDS}, placement=False,
                   selection_mode=1, across_map=True, submode=1, retained_navigation=[20],
                   action_timer=[995, 0, 25], voice_enabled=True, setup_cleanup=setup,
                   cleanup_world_size=[64, 64], commands=commands)


def combatant_inputs():
    def value(identity, kind="unit", **overrides):
        return actor(identity, kind, is_selectable_combatant=overrides.pop("is_selectable_combatant", True),
                     positive_primary_damage=overrides.pop("positive_primary_damage", True),
                     **overrides)

    def row(name, actors=None, screen=(20, 10), map_order=(40, 10, 20),
            selected=(), commands=None, **fields):
        actors = actors if actors is not None else [value(20, "infantry"), value(10), value(40)]
        return dict(id=name, actors=actors, layers=[[], [], [a["id"] for a in actors], [], []],
                    screen_order=list(screen), map_order=list(map_order),
                    selected=list(selected), commands=commands or [dict(command="combatant")],
                    **fields)

    yield row("screen_then_map_then_repeat", commands=[dict(command="combatant") for _ in range(3)])
    yield row("screen_reverse_map_stored_order", screen=(10, 20), map_order=(20, 40, 10),
              commands=[dict(command="combatant") for _ in range(2)])
    yield row("already_all_screen_selected_expands_same_call", selected=(10, 20), follow=20)
    yield row("empty_screen_expands_same_call", screen=())
    yield row("empty_everywhere", actors=[], screen=(), map_order=())
    yield row("noncombatant_screen_expands_to_map", actors=[value(20, is_selectable_combatant=False), value(40)],
              screen=(20,), map_order=(40, 20))
    yield row("screen_null_and_nontechno", actors=[value(20), value(10, techno_cast=False), value(40)],
              screen=(None, 10, 20))
    yield row("map_alive_gate_accepts_nontechno", actors=[value(20, techno_cast=False), value(40)],
              screen=(), map_order=(20, 40))
    for shift in (False, True):
        yield row(f"shift{int(shift)}_retains_noncombatant", actors=[value(20, "infantry"), value(10),
                  value(40, is_selectable_combatant=False)], selected=(40,), follow=40,
                  commands=[dict(command="combatant", key_word=0x100 if shift else 0)])
        yield row(f"shift{int(shift)}_all_screen_selected_expands", selected=(10, 20), follow=20,
                  commands=[dict(command="combatant", key_word=0x100 if shift else 0)])
        yield row(f"placement_refusal_shift{int(shift)}", selected=(40,), follow=40, placement=True,
                  modes={"sell": True},
                  commands=[dict(command="combatant", key_word=0x100 if shift else 0)])
    for name, fields in (
        ("building", dict(kind="building")),
        ("noncombatant", dict(is_selectable_combatant=False)),
        ("alive_false", dict(alive=False)), ("other_owner", dict(owner="other")),
        ("health_zero_alive_true", dict(health=0)),
        ("limbo", dict(limbo=True)), ("not_in_playfield", dict(in_playfield=False)),
        ("undiscovered", dict(discovered=False)), ("not_selectable", dict(selectable=False)),
        ("slave", dict(slave=True)), ("robot_offline", dict(robot_offline=True)),
        ("bunker", dict(bunker=True)), ("locomotor_swap", dict(locomotor_swap=True)),
        ("docked_empty_cell", dict(docked=True)),
        ("docked_building", dict(docked=True, cell_building=True)),
        ("virtual_1d4", dict(virtual_1d4=True)), ("mission_only", dict(mission_only=True)),
    ):
        for kind in ("infantry", "unit") if "kind" not in fields else (fields["kind"],):
            yield row(f"{kind}_{name}_distinguishes_escalation_and_final_select",
                      actors=[value(20, kind, **{k: v for k, v in fields.items() if k != "kind"}), value(40)],
                      screen=(20,), map_order=(40, 20), selected=(40,), follow=40)
    yield row("native_prepend_and_append", actors=[value(20, "infantry"), value(10, positive_primary_damage=False),
              value(40), value(30, positive_primary_damage=False)], screen=(20, 10, 40, 30),
              map_order=(30, 10, 20, 40), selected=(10,))
    yield row("ordinary_selection_resets_mode_then_P_screen", screen=(20,),
              actors=[value(20, "infantry"), value(10), value(40), value(30, is_selectable_combatant=False)],
              commands=[dict(command="combatant"), dict(command="combatant"),
                        dict(command="ordinary_select", id=30), dict(command="unselect_all"),
                        dict(command="combatant")])
    yield row("P_T_P_health_P", actors=[value(20, "infantry", type_id="E1"), value(10, type_id="MTNK"),
              value(40, type_id="MTNK")], screen=(20, 10),
              commands=[dict(command="combatant"), dict(command="combatant"),
                        dict(command="type"), dict(command="combatant"),
                        dict(command="health"), dict(command="combatant")])
    yield row("health_P_T_P", commands=[dict(command="health"), dict(command="combatant"),
              dict(command="type"), dict(command="combatant")])
    yield row("T_map_P_restarts_screen", actors=[value(20, "infantry", type_id="E1"),
              value(10, type_id="MTNK"), value(40, type_id="MTNK")], selected=(20,),
              commands=[dict(command="type"), dict(command="combatant"),
                        dict(command="type"), dict(command="combatant")])
    yield row("health_empty_snapshot_preserves_map_byte", actors=[], screen=(), map_order=(),
              selection_mode=1, across_map=True,
              commands=[dict(command="health"), dict(command="combatant")])
    for campaign, human, control in ((False, False, False), (False, True, False),
                                     (True, False, True), (True, True, False)):
        yield row(f"owner_mode{int(campaign)}_human{int(human)}_control{int(control)}",
                  actors=[value(20, "infantry", owner="other"), value(40)],
                  screen=(20,), map_order=(40, 20), selected=(20,), follow=20,
                  selection_mode=1, across_map=True,
                  house=dict(campaign=campaign, other_human=human, other_control=control))
    for campaign, human in ((False, False), (True, True)):
        yield row(f"shift_P_lone_nonlocal_campaign{int(campaign)}_human{int(human)}",
                  actors=[value(20, "infantry", owner="other"), value(40)],
                  screen=(20,), map_order=(40, 20), selected=(20,), follow=20,
                  selection_mode=1, across_map=True,
                  house=dict(campaign=campaign, other_human=human, other_control=False),
                  commands=[dict(command="combatant", key_word=0x100)])
    yield row("ordinary_deselect_retains_P_map_scope", selected=(20,), follow=20,
              selection_mode=1, across_map=True,
              commands=[dict(command="ordinary_deselect", id=20), dict(command="combatant")])
    yield row("voice_disabled", voice_enabled=False)
    for seed in (0, 1234):
        yield row(f"voice_seed{seed}", seed=seed)
    yield row("inactive_action_timer_preserved", action_timer=[-1, 0, 0],
              commands=[dict(command="combatant"), dict(command="combatant")])
    yield row("voice_empty_and_multiple", actors=[value(20, "infantry", voice_list=[]),
              value(10, voice_list=[101, 202, 303]), value(40)])
    yield row("voice_minus_one_already_spent_draw", actors=[value(20, voice_list=[-1]), value(40)],
              screen=(20,), map_order=(40, 20))
    yield row("guarded_P_preserves_all", selected=(20,), follow=20, selection_mode=3,
              across_map=True, command_guard=1, modes={name: True for name in MODE_FIELDS})
    for mode in MODE_FIELDS:
        yield row(f"P_preserves_{mode}_mode", modes={mode: True}, selected=(40,), follow=40)


def combatant_type_histories():
    """Compose the existing type/INI fixture; do not substitute field results."""
    root = Path(os.environ.get("VERA20K_COMBATANT_INPUTS", "ini"))
    files = ("RULESMD.INI", "LANGRULE.INI", "MPBATTLEMD.INI", "XMP03T4.MAP")
    # Filename case reflects the native source identities while allowing the
    # existing extraction owner to preserve either ordinary filesystem spelling.
    paths = {path.name.upper(): path for path in root.iterdir() if path.is_file()}
    for name in files:
        if name != "LANGRULE.INI" and name not in paths:
            raise RuntimeError(f"Missing physical combatant type layer {name}; set VERA20K_COMBATANT_INPUTS")
    m = BulletReader({}, root)
    m.u.mem_write(0xA8E348, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
    text_hash = hashlib.sha256(bytes(m.u.mem_read(0x401000, 0x3E0000))).hexdigest()
    constructors = []

    def construct(name, entry):
        typ = m.alloc(0x2000)
        m.u.mem_write(typ, b"\xa5" * 0x2000)
        writes = []
        def observe(u, _access, address, size, value, _data):
            if address <= typ + 0xDBC < address + size:
                writes.append(dict(pc=hex(u.reg_read(UC_X86_REG_EIP)),
                                   offset=address-typ, bytes=size, value=value))
        hook = m.u.hook_add(UC_HOOK_MEM_WRITE, observe)
        try:
            m.invoke(entry, typ, (m.cstring(name),))
        finally:
            m.u.hook_del(hook)
        constructors.append(dict(type_id=name, entry=hex(entry), poison_byte=165,
                                 value_before=165, value_after=byte(m.u, typ+0xDBC),
                                 vtable=hex(m.read32(typ)), field_writes=writes))
        return typ

    def read(typ, sections):
        m.rules_cache(sections)
        before = bool(byte(m.u, typ+0xDBC))
        m.u.mem_write(SP, dwords(RET_MAGIC, RULES))
        m.u.reg_write(UC_X86_REG_ESP, SP)
        m.u.reg_write(UC_X86_REG_ECX, typ)
        stop = run_checked(m.u, 0x410A60, (0x410A8C, 0x410B7D),
                           count=200000, required_addresses=[0x526810])
        admitted = stop == 0x410A8C
        m.reads = []
        if admitted:
            for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, typ),
                                    (UC_X86_REG_EBX, typ+0x24), (UC_X86_REG_ESI, RULES)):
                m.u.reg_write(register, value)
            run_checked(m.u, 0x71574E, 0x71576F,
                        count=200000, required_addresses=[0x524EC0, 0x5295F0])
            if m.u.reg_read(UC_X86_REG_ESP) != SP:
                raise RuntimeError("Unbalanced native combatant bool-reader block")
        return dict(before=before, admitted=admitted, after=bool(byte(m.u, typ+0xDBC)),
                    reads=[dict(reader=row["reader"], section=row["section"], key=row["key"],
                                ini="rules") for row in m.reads if row["reader"] == "0x5295f0"])

    retail = []
    for name, entry in (("E1", 0x5236A0), ("E2", 0x5236A0), ("ENGINEER", 0x5236A0),
                        ("MTNK", 0x7470D0), ("HARV", 0x7470D0), ("CMIN", 0x7470D0),
                        ("ROBO", 0x7470D0), ("GACNST", 0x45DD90)):
        typ = construct(name, entry)
        layers = []
        for filename in files:
            if filename not in paths:
                layers.append(dict(file=filename, absent=True))
                continue
            raw = paths[filename].read_bytes()
            physical, _ = lexical(raw, {name})
            sections = {name: {key: value for key, value in physical[name].items()
                               if key == "IsSelectableCombatant"}} if name in physical else {}
            layers.append(dict(file=filename, sha256=hashlib.sha256(raw).hexdigest(),
                               raw_value=sections.get(name, {}).get("IsSelectableCombatant"),
                               sections=sections, **read(typ, sections)))
        retail.append(dict(type_id=name, constructor=False, layers=layers,
                           final=bool(byte(m.u, typ+0xDBC))))
    authored = []
    typ = construct("AUTHORED", 0x710AF0)
    for label, sections in (
        ("yes", {"AUTHORED": {"IsSelectableCombatant": "yes"}}),
        ("missing_key_retains_true", {"AUTHORED": {}}),
        ("missing_section_retains_true", {}),
        ("empty_retains_true", {"AUTHORED": {"IsSelectableCombatant": ""}}),
        ("wrong_key_case_retains_true", {"AUTHORED": {"isselectablecombatant": "no"}}),
        ("wrong_section_case_retains_true", {"authored": {"IsSelectableCombatant": "no"}}),
        ("numeric2_retains_true", {"AUTHORED": {"IsSelectableCombatant": "2"}}),
        ("malformed_retains_true", {"AUTHORED": {"IsSelectableCombatant": "garbage"}}),
        ("no", {"AUTHORED": {"IsSelectableCombatant": "no"}}),
        ("missing_key_retains_false", {"AUTHORED": {}}),
        ("empty_retains_false", {"AUTHORED": {"IsSelectableCombatant": ""}}),
        ("numeric2_retains_false", {"AUTHORED": {"IsSelectableCombatant": "2"}}),
        ("mixed_case_true", {"AUTHORED": {"IsSelectableCombatant": "TrUe"}}),
        ("numeric0_false", {"AUTHORED": {"IsSelectableCombatant": "0"}}),
        ("numeric1_true", {"AUTHORED": {"IsSelectableCombatant": "1"}}),
        ("false", {"AUTHORED": {"IsSelectableCombatant": "false"}}),
    ):
        authored.append(dict(id=label, sections=sections, **read(typ, sections)))
    if text_hash != hashlib.sha256(bytes(m.u.mem_read(0x401000, 0x3E0000))).hexdigest():
        raise RuntimeError("Original .text changed during combatant type execution")
    return dict(key=m.string(0x843414), field_offset=0xDBC, constructors=constructors,
                retail_histories=retail, authored_history=authored)


def search_inputs():
    actors = [actor(i, "infantry" if i in (10, 20, 70) else "unit")
              for i in (90, 80, 40, 10, 30, 20, 60, 70)]
    layers = [[90], [80], [40, None, 10], [30, 20], [60]]
    for direction in SEARCH:
        for anchor in (None, 40, 10, 30, 20, 60, 70):
            yield dict(id=f"layer_order_{direction}_anchor_{anchor}", actors=actors,
                       layers=layers, anchor=anchor, direction=direction)
        for name, overrides in (
            ("health_zero", {"health": 0}), ("health_negative", {"health": -1}),
            ("limbo", {"limbo": True}), ("not_in_playfield", {"in_playfield": False}),
            ("undiscovered", {"discovered": False}), ("not_selectable", {"selectable": False}),
            ("other_owner", {"owner": "other"}), ("slave", {"slave": True}),
            ("robot_offline", {"robot_offline": True}), ("bunker", {"bunker": True}),
            ("locomotor_swap", {"locomotor_swap": True}), ("dead_bit_only", {"alive": False}),
            ("no_techno_cast", {"techno_cast": False}),
            ("docked_empty_cell", {"docked": True}),
            ("docked_building", {"docked": True, "cell_building": True}),
            ("building", {"kind": "building"}),
        ):
            for kind in ("infantry", "unit") if name != "building" else ("building",):
                changed = [actor(40), actor(10, kind, **{k: v for k, v in overrides.items() if k != "kind"}), actor(20)]
                yield dict(id=f"{kind}_{name}_{direction}", actors=changed,
                           layers=[[], [], [40, 10, 20], [], []],
                           anchor=10, direction=direction)
        for name, supplied_layers, supplied_actors in (
            ("empty", [[], [], [], [], []], []),
            ("ignored_layers_only", [[90], [80], [], [], []], actors),
            ("all_ineligible", [[], [], [40, None, 10], [], []],
             [actor(40, health=0), actor(10, "infantry", locomotor_swap=True)]),
            ("singleton", [[], [], [40], [], []], [actor(40)]),
            ("empty_ground", [[], [], [], [30], [60]], actors),
        ):
            yield dict(id=f"{name}_{direction}", actors=supplied_actors,
                       layers=supplied_layers, anchor=40 if name == "singleton" else None,
                       direction=direction)
        for campaign, human, controlled in ((False, True, True), (True, False, False),
                                            (True, True, False), (True, False, True)):
            yield dict(id=f"owner_mode_{int(campaign)}_human{int(human)}_control{int(controlled)}_{direction}",
                       actors=[actor(1, "infantry", owner="other")],
                       layers=[[], [], [1], [], []], anchor=None, direction=direction,
                       house=dict(campaign=campaign, other_human=human, other_control=controlled))


def command_inputs():
    actors = [actor(20, "infantry"), actor(10), actor(30, "infantry")]
    layers = [[], [], [20, 10, 30], [], []]
    yield dict(id="consecutive_next_previous", actors=actors, layers=layers,
               selected=[20], follow=20,
               directions=["next", "next", "next", "previous", "previous", "previous"])
    yield dict(id="multiple_selection_first_anchor", actors=actors, layers=layers,
               selected=[10, 20], follow=20, directions=["next", "previous"])
    yield dict(id="empty_selection_then_repeat", actors=actors, layers=layers,
               selected=[], directions=["previous", "next", "next"])
    yield dict(id="singleton_follow_reselect", actors=[actor(20, "infantry")],
               layers=[[], [], [20], [], []], selected=[20], follow=20,
               directions=["next", "previous"])
    for mode in MODE_FIELDS:
        yield dict(id=f"cancel_{mode}", actors=actors, layers=layers, selected=[20],
                   modes={mode: True}, directions=["next"])
        yield dict(id=f"placement_preserves_{mode}_refuses_selection", actors=actors,
                   layers=layers, selected=[20], follow=20, modes={mode: True},
                   placement=True, directions=["next"])
    yield dict(id="placement_refuses_selection", actors=actors, layers=layers,
               selected=[20], follow=20, placement=True, directions=["next", "previous"])
    yield dict(id="no_candidate_preserves_selection_and_follow", actors=[actor(20, "infantry", health=0)],
               layers=[[], [], [20], [], []], selected=[20], follow=20,
               modes={"repair": True}, directions=["next", "previous"])
    for field in ("mission_only", "virtual_1d4"):
        for kind in ("unit", "infantry"):
            yield dict(id=f"{kind}_search_accepts_select_refuses_{field}",
                       actors=[actor(20, "infantry"), actor(10, kind, **{field: True})],
                       layers=[[], [], [20, 10], [], []], selected=[20], follow=20,
                       directions=["next"])
    yield dict(id="voice_disabled", actors=actors, layers=layers, selected=[20],
               voice_enabled=False, directions=["next"])
    yield dict(id="voice_empty", actors=[actor(20, "infantry", voice_list=[])],
               layers=[[], [], [20], [], []], selected=[], directions=["next"])


def voice_inputs():
    for kind in ("infantry", "unit"):
        for seed in (0, 1, 1234):
            for name, values in (("empty", []), ("single", [101]),
                                 ("multiple", [101, 202, 303])):
                yield dict(id=f"{kind}_{name}_seed{seed}", kind=kind, seed=seed,
                           voice_list=values, calls=4)
        for name, fields in (("disabled_queue", {"voice_enabled": False}),
                             ("other_owner_queue", {"owner": "other"}),
                             ("enslaved_list", {"slave": True}),
                             ("robot_offline_list", {"robot_offline": True})):
            yield dict(id=f"{kind}_{name}", kind=kind, seed=1,
                       voice_list=[101, 202, 303], calls=4, **fields)
        yield dict(id=f"{kind}_minus_one_sound", kind=kind, seed=1,
                   voice_list=[-1], calls=2)
    yield dict(id="unit_multiple_seed1_after248_main_raw", kind="unit", seed=1,
               voice_list=[101, 202, 303], calls=4, advance_main_raw=248)


def generate():
    return dict(schema=1,
                search_cases=[search_case(spec) for spec in search_inputs()],
                command_histories=[command_history(spec) for spec in command_inputs()],
                voice_histories=[voice_history(spec) for spec in voice_inputs()],
                combatant_histories=[selection_history(spec) for spec in combatant_inputs()],
                combatant_cleanup_histories=[selection_history(spec)
                                            for spec in combatant_cleanup_inputs()],
                combatant_type_histories=combatant_type_histories(),
                veterancy_histories=[selection_history(spec) for spec in veterancy_inputs()],
                health_navigation_histories=[selection_history(spec) for spec in health_navigation_inputs()],
                veterancy_cleanup_histories=[selection_history(spec) for spec in veterancy_cleanup_inputs()],
                enslaved_voice_histories=[selection_history(spec) for spec in enslaved_voice_inputs()],
                enslaved_voice_type_histories=enslaved_voice_type_histories(),
                veterancy_rank_cases=veterancy_rank_cases(),
                map_veterancy_seed_cases=map_veterancy_seed_cases(),
                veterancy_initialization_cases=veterancy_initialization_cases())


def metadata():
    result = provenance(scope=__doc__, assumptions=[
        "Actors, types, owners, layers and selection storage are initialized fixture inputs, not original constructor/scenario-load proof. No retail E1/MTNK INI inputs or actual airborne/layer producer are claimed.",
        "Real Unit/Infantry vtables and candidate/selection bodies execute; a Building vtable supplies the RTTI6 exclusion and dock occupier controls only. Tag is NULL, disguise byte1D8 is zero, planning propagationAC4CF4 is disabled and selected-control-group metadata is empty.",
        "Unit6D8 is supplied -1 for the ordinary control branch. Object270 is virtual1D4 warp refusal; Foot6AD is the separate locomotor-swap control. These initialized bits do not prove their lifecycle writers.",
        "Layer vectors are supplied at8A0360; original820030 direction table is read by both search bodies. Layer0/1 exclusion, nulls, absent/ineligible anchors and retained sequence outputs are native observations, not a Python expected scan.",
        "Selection vectorA8ECB8 uses actual static-initializer4E7D40 vtable7E4F64 with supplied capacity64. Consecutive command calls retain the original immediate selection array; no Rust command-delay model is supplied.",
        "Dock controls supply native map-table cells and occupiers; original5F6960/565730/47C520 execute. Building lifecycle, collision and aircraft motion are excluded.",
        "Historical selection/voice families use fixture-relative numeric sound IDs and exclude native sound lookup/type readers. Appended enslaved_voice_type_histories separately execute those selected readers against a physical fixed SOUNDMD subset. OS audio playback/device samples remain excluded. Direct VoiceSelect disabled/other-owner controls establish receiver ordering, not reachability through the Select wrapper.",
        "Main886B88, Scenario+218 and MapGenABE890 are initialized with original65C6D0 via sharedgen_rng_vectors.seeded_struct. Complete3F4-byte states bracket voice histories; actual raw draw outputs/continuation are independently replayed with the same original65C780 helper. Scenario and MapGen must remain byte-identical.",
        "The appended unit_multiple_seed1_after248_main_raw history executes 248 original65C780 calls on the same Main886B88 before four708EB0 calls. Prefix draws and full before/after state are observed; no reseeding or cursor replacement occurs between prefix and voices. This supplies an earlier-consumer boundary, not execution of terrain loading, MoveSound, Gattling or death consumers.",
        "Cursor-mode controls execute original power/planning/repair/sell bodies. Planning's saved cursor pointer11BC is null, so its nonempty cursor restoration callback and active planning-group propagation are outside these controls.",
        "Camera4AE290 executes through its singleton/empty reduction; final rendering and full camera bounds/projection are outside the declared6D6070 boundary. Original.text integrity is checked after each invocation.",
        "The appended CombatantSelect histories execute registered5367F0 over original732280, supplied Tactical screen recordsB0CEC8/count+DB0 and retained native Techno vectorA8EC7C/countA8EC88. Screen7342C0, Alive/owner732580, dynamic13C and final7325C0/Select14C execute unchanged. Map/display membership producers, selected Tag actions, disguise and robot power/voice lifecycle are not claimed.",
        "P mode1 and T mode2 share the single across-map byteB0FE64; Health mode3 and the empty-snapshot mode0 retain that byte. The next differing command performs its own native mode-based scope reset. The prior selection_across_map output keeps its historical B0FE58 submode meaning; appended across_map and submode expose both actual bytes separately.",
        "Real ObjectSelect5F4520 prepends positive-PrimaryC9C objects and appends others. Scan/Select/voice order and final CurrentObjects order are separately observed. InvertedShift is passed by the registered P/Health wrappers; direct ordinary_select invokes the existing TechnoSelect body, not a mouse gesture.",
        "The two Shift+P lone-nonlocal histories start mode1/maptrue, selected/follow20. Original6DA770 removes20 even with Shift, leaves map scope, clears follow and selects40 only; campaign human1EC/control1EDfalse and noncampaign controls both require the same final CurrentObjects membership.",
        "The direct ordinary_deselect step invokes original5F44A0 on local20 from selected/follow20, mode1/maptrue. Deselect clears selected membership/follow but retains mode1 and B0FE64; following P therefore starts across-map. OrdinarySelect and UnselectAll remain distinct mode-reset witnesses.",
        "P success invokes VoiceSelect for every eligible newly selected object while822CF2 remains enabled. Shift's already-selected refusals consume no draw. Empty and-minus1 voice lists and global disabled controls distinguish gate/admission. Every step checks original Main draw continuation and byte-identical Scenario/MapGen; full RNG bytes and next4 original Main draws bracket each new history.",
        "P armed power/planning/repair/sell modes, placement, follow and all12B0EA80 action-timer bytes are observed. Timer-write hooks and known action-timer/Detach entries stay empty in this untagged P path. OrdinarySelect and UnselectAll histories are reached original selection-reset witnesses, not a full native scenario replacement or save/load execution.",
        "Full original InfantryType5236A0, UnitType7470D0, BuildingType45DD90 and TechnoType710AF0 constructors overwrite A5-poisoned+DBC with0 at71164B. Physical RULESMD, optional LANGRULE, MPBATTLEMD and XMP03T4 layers are supplied through existing BulletReader lexical/cache owners; original410A60 section admission and71574E..71576F current-default Bool read/store execute. Full unrelated type reader/Rules scenario chronology and physical file IO are not claimed.",
        "Appended type fields use original524EC0 string accessor, exact IsSelectableCombatant843414 and ReadBool5295F0. Physical layer hashes and authored missing/empty/wrongcase/malformed/numeric retained-default histories are native outputs. All original.text bytes stay unchanged.",
        "The six combatant_cleanup_histories retain mode1/B0FE64true across actual Deselect5F44A0, direct pointer-expiry733160 and bounded whole ObjectDetach5F5280(true)/ObjectConceal5F4D30, then execute registered5367F0. Present Alivefalse/selectedfalse and supplied registry omission are separate explicit inputs. Subsequent P includes offscreen50 despite available unselected screen40. Rust pending membership has no native flag counterpart.",
        "Cleanup retainedB0FE6C input is constructed by original732050 from selected20 then assigned as supplied prior Health/Y state while P owns mode1. Full per-step RNG bytes/next4, actor state, actual layer/screen/map membership, scope/action-timer writes and original native nested calls are recorded; no Python mutation/comparator/RNG algorithm supplies reference outputs.",
        "Whole cleanup controls reuse ifv_impact.initialize_effect_world's original startup regions40B540..40B5AB,725850..725886,4E6D60..4E6D96 with supplied64x64 dimensions and seed1. Original4A8630..4A8672 initializes display vectors and65A758..65A798 allocates empty Radio slots over supplied actors. Tactical ctor6D1D54 vptr7F4348 is supplied; actual6DA560 executes. CRT atexit registrations are outside setup regions.",
        "Cleanup services/listeners, Bomb/Kamikaze/Temporal/Team, Tags/spawner and radio contacts are empty. Units are unmarked, Type234false excludes Conceal LogicRemove, actual DisplayRemove executes. Alivefalse is prestate, not an executed death writer; absent membership is supplied, mapped object storage remains. Full fatal AI, UnInit/Limbo/Mark, destructor/free/deferred-drain and nonempty ancillary cleanup are not demonstrated.",
        "Original MainTick55D360 calls GScreenInput4F4320 at55D8AB and ProcessCommand55DEE0 at55D8B4 before Logic55AFB0 at55DC9E. Registered Execute+20 at55E015 reaches P5367F0/732280 and synchronous Select+14C at7324A4. Infantry vtable7EB058+DC/+14C/+150 original bytes are4D9720/6FBFA0/5F44A0; registeredPslot7EB9AC is5367F0. This is original body/caller/data evidence, not a whole fatal-frame execution or an EventClass phase claim.",
        "Appended Y/Health families opt into additional trace labels; historical search/NM/voice/P/type/cleanup observations remain unchanged. Actor+150 raw bits, type+610 cost, actual cost vtables and explicit all-one country/plant factors are inputs. The selected Building control additionally supplies cost_of's unrelated PadAircraft/Dock data; no Aircraft runtime/producer is executed.",
        "Original registered VeterancyNav5369F0 calls7336C0 with inverted Shift. Mode4 and Health mode3 share retained B0FE6C; original file bytes845560/845564 initialize both categories to signed-1. Fresh navigation replaces the snapshot, clears selection even with Shift and starts category0. Continuing navigation samples current raw rank750030 or Health5F5DD0 without rechecking Alive/local source admission.",
        "New histories record full per-step three RNG states, original Main next4, timer/scope/category writes, native calls, retained order and actual actor/layer/screen/map state. Supplied raw-rank, Alive, guard and registry changes are explicit inputs, not original writers. The selected snapshot may contain other-house/dead/Building actors; screen fallback7335F0 checks Alive, native local owner and non-Building.",
        "Original733160 searches backward and stable-erases at most one exact pointer. Duplicate-pointer controls are explicit supplied snapshot boundaries constructed by original732050; no duplicate CurrentObjects producer is claimed. Whole Detach/Conceal reuse the existing initialized empty-service/unmarked boundaries. Actual7258D0 visits represented listeners, Bomb, Kamikaze54E590, Houses,733160,Tactical,Logic in that order. The old P trace's temporal_pointer_expiry label at54E590 is historical; new opt-in traces correctly name Kamikaze.",
        "Map rank goldens execute Unit743495..7434AE, Infantry51FD54..51FD6D and Aircraft41B308..41B321 original token-present/NULL branches, CRT atoi7C9BFD, percent setter7500E0 and rank750030. The pointer is supplied after original strtok's token boundary; empty string is helper-only. Unit/Infantry tokens are field8/9, Aircraft field8. These regions precede Unlimbo7435D0/51FE52/41B39B; whole map readers, Aircraft constructors and flight behavior are excluded.",
        "7500E0 FILD loads signed i32, FMUL uses original binary64 bytes7B14AE47E17A843F at7E3808, and FSTP writes raw binary32 under native FPCW0E7F (53-bit precision/chop). No clamp or rank quantization occurs. NULL leaves raw unchanged; raw0 is the ordinary constructor state, raw1.0 and1/3 are explicit preservation controls.",
        "Initialization rows execute full CountryType5113F0 on A5 storage (empty VeteranInfantry14C/Units168/Aircraft184), Techno caller6F2BDC..6F2BED/74FF30 (raw0), original House XOR4F54B2 and flag writers4F5855..4F5873 (Barracks2BF/WarFactory2C0 false), plus actual Unit735604..735678 and Infantry517CE7..517D51 rank gates. These are bounded initialization/gate executions, not full House/Unit/Infantry construction.",
        "Selected Americans/YuriCountry physical layers execute Country list-reader511D16..51208C via the existing lexical/cache/INI owner: exact keys VeteranInfantry/Units/Aircraft and runtime empty default889F64. Native missing reads retain constructor-empty lists. Whole CountryReadINI, authored list parsing, spy infiltration production/lifecycle and SpecialFlags.InitialVeteran processing are not executed by these rows.",
        "Appended enslaved_voice_histories supply explicit normal+414/enslaved+430 lists and SlaveOwner+2DC presence, then execute registered Y and original6FBFA0/708EB0/708D90. Empty enslaved lists return before any draw instead of falling back to normal. Nonempty lists spend exactly one Main raw draw per newly selected actor, including singleton and supplied-1 slots. Actual708DB5 writes distinguish accepted queue requests from attempted requests; disabled/admission/Shift/other-house/passive-house controls preserve original caller gating.",
        "The freed-slave controls execute actual FreeSlaves6B0B6D..6B0B98: outside-Limbo input, clearSlaveOwner+2DC at6B0B73, then stop before owner-change/reset/cheer. A subsequent original731D00/Y selection reaches the normal list on the same Main continuation. Whole FreeSlaves manager enumeration, death, house transfer, missions and cheers are not claimed. Offline Robot voice/lifecycle remains held.",
        "Four full selected type constructors on A5 storage initialize normal+414 and enslaved+430 vectors empty. Original admitted712B1D..712BF1 executes normalVoiceSelect then exactVoiceSelectEnslaved8442A0 through ReadSoundList525430, current-vector default, ReadString128, comma-only strtok817F70, fixed sound pointer/index751520/7515C0. Missing/empty reads retain; nonempty reads replace, skip unresolved names and retain duplicates. Source-order selected SOUNDMD registration7510D0 and actual retail E1/MTNK/SLAV RULESMD/optionalLANGRULE/MPBATTLEMD/XMP03T4 layers execute via existing cache/transport owners; whole TechnoType reader/scenario loading/audio playback is excluded.",
    ], substitutions=[
        "Inherited KeyboardFixture allocator7C8E17 returns bounded scratch storage during original registration and the empty planning-slot allocation. Registration stops before533D20 INI file loading. No allocator failure/CRT exit registration claims.",
        "Mouse cursor presentation5BDA80/5BDAA0, Tactical camera application6D6070 and redraw4F42F0 record calls then return.",
        "Appended command vectors reuse bounded allocator transport7C8E17; destructor CRT free7C8B3D returns without reclaiming fixture storage. Gameplay/vector/destructor/selection bodies execute original instructions.",
        "Appended T edges use supplied timeGetTime via original import7E1530 and100ms tap duration. StringTable734E60 returns actual UTF16 strings from physical langmd.mix/ra2md.csf via existing stock_csf; native wcscpy/swprintf execute. Final MessageList5D3BA0 records seven original arguments/text then returns; no GUI paint/timer scheduling is claimed.",
        "Appended type constructor/reader setup inherits existing BulletReader bounded heap, CRT TLS and asset-platform seams. Selected IsSelectableCombatant field readers invoke original INI/string/Bool code; no scalar/gate/field result is substituted.",
        "New navigation's original CRT feedback formatter reuses infantry_deploy_action's single-thread InterlockedIncrement/Decrement OS transport at IAT7E11C8/CC. No formatter, rank, candidate, cost, selection, expiry or RNG return is substituted.",
        "Selected fixed SOUNDMD registration reuses anytown_damage.mtnk_attack.sound_inputs and bridge_child_sound.Sound.make_ini source-order links. Native audio registry/type/readers execute; AudioIndex sample-name lookups return local sample indexes. Enslaved type list ids0..3 are fixture-relative and native-retained names identify the sounds. Physical INI file loading, full catalog and playback are outside this boundary.",
    ], entry_points={"next": SEARCH["next"], "previous": SEARCH["previous"],
                     "execute_next": 0x536610, "execute_previous": 0x536A80,
                     "candidate": 0x6F32D0, "dynamic_candidate": 0x6FC030,
                     "select": 0x6FBFA0, "deselect": 0x5F44A0,
                     "voice_select": 0x708EB0, "queue_voice": 0x708D90,
                     "enslaved_voice_reader": 0x712B87, "normal_voice_reader": 0x712B1D,
                     "sound_list_reader": 0x525430, "sound_registry": 0x7510D0,
                     "sound_find_pointer": 0x751520, "sound_find_index": 0x7515C0,
                     "free_slaves": 0x6B0AE0, "free_slave_owner_writer": 0x6B0B73,
                     "rng_raw": 0x65C780, "rng_seed": 0x65C6D0,
                     "execute_combatant": 0x5367F0, "combatant": 0x732280,
                     "alive_owner": 0x732580, "screen_techno": 0x7342C0,
                     "combatant_final": 0x7325C0, "unselect_if_not_owned": 0x6DA770,
                     "execute_type": 0x5368B0, "type_tap": 0x732950,
                     "execute_health": 0x536950, "health": 0x733380,
                     "execute_veterancy": 0x5369F0, "veterancy": 0x7336C0,
                     "veterancy_rank": 0x750030, "veterancy_percent": 0x7500E0,
                     "veterancy_constructor": 0x74FF30, "crt_atoi": 0x7C9BFD,
                     "navigation_fallback": 0x7335F0, "navigation_selected_snapshot": 0x732050,
                     "navigation_screen_snapshot": 0x731F70,
                     "country_constructor": 0x5113F0, "country_reader": 0x511850,
                     "country_list_reader": 0x511D16, "house_constructor": 0x4F54A0,
                     "unit_constructor_rank_gate": 0x735604,
                     "infantry_constructor_rank_gate": 0x517CE7,
                     "map_unit_rank_branch": 0x743495, "map_infantry_rank_branch": 0x51FD54,
                     "map_aircraft_rank_branch": 0x41B308,
                     "reset_mode": 0x731D00, "techno_type_constructor": 0x710AF0,
                     "infantry_type_constructor": 0x5236A0,
                     "unit_type_constructor": 0x7470D0,
                     "building_type_constructor": 0x45DD90,
                     "section_admission": 0x410A60,
                     "combatant_type_reader": 0x71574E, "bool_reader": 0x5295F0,
                     "navigation_pointer_expired": 0x733160,
                     "pointer_expired_dispatch": 0x7258D0,
                     "object_detach_all": 0x5F5280, "object_conceal": 0x5F4D30,
                     "foot_detach_all": 0x4D9720, "display_remove": 0x4A9770,
                     "main_tick": 0x55D360, "screen_input": 0x4F4320,
                     "process_command": 0x55DEE0, "logic": 0x55AFB0})
    result["command"] = "python -m tools.input_oracle.selection_navigation --check"
    result["coverage_counts"] = dict(search_cases=94, command_histories=20,
                                    command_calls=31, voice_histories=29,
                                    voice_calls=112, prefix_raw_calls=248)
    inputs = list(combatant_inputs())
    result["coverage_counts"].update(
        combatant_histories=len(inputs),
        combatant_steps=sum(len(spec["commands"]) for spec in inputs),
        combatant_registered_execute_calls=sum(
            2 if command["command"] == "type" else command["command"] not in ("ordinary_select", "ordinary_deselect", "unselect_all")
            for spec in inputs for command in spec["commands"]),
        combatant_type_constructors=9, combatant_retail_type_histories=8,
        combatant_authored_reader_steps=16)
    cleanup_inputs = list(combatant_cleanup_inputs())
    result["coverage_counts"].update(
        combatant_cleanup_histories=len(cleanup_inputs),
        combatant_cleanup_steps=sum(len(spec["commands"]) for spec in cleanup_inputs),
        combatant_cleanup_registered_execute_calls=sum(
            command["command"] == "combatant"
            for spec in cleanup_inputs for command in spec["commands"]),
        combatant_cleanup_direct_boundaries=sum(
            command["command"] in ("ordinary_deselect", "pointer_expiry",
                                   "object_detach_all", "object_conceal")
            for spec in cleanup_inputs for command in spec["commands"]))
    result["command"] = ("VERA20K_COMBATANT_INPUTS=/path/to/extracted/selection/layers "
                         "python -m tools.input_oracle.selection_navigation --check")
    registered = {"combatant", "type", "health", "veterancy", "next", "previous"}
    direct = {"ordinary_select", "ordinary_deselect", "unselect_all", "pointer_expiry",
              "object_detach_all", "object_conceal", "reset_selection_mode", "free_slave_owner_writer"}
    for family, factory in (("veterancy", veterancy_inputs), ("health_navigation", health_navigation_inputs),
                            ("veterancy_cleanup", veterancy_cleanup_inputs),
                            ("enslaved_voice", enslaved_voice_inputs)):
        inputs = list(factory())
        commands = [command["command"] for spec in inputs for command in spec["commands"]]
        result["coverage_counts"].update({
            family + "_histories": len(inputs),
            family + "_steps": len(commands),
            family + "_registered_execute_calls": sum(2 if name == "type" else 1 for name in commands if name in registered),
            family + "_direct_boundaries": sum(name in direct for name in commands),
            family + "_supplied_changes": sum(name.startswith("supply_") for name in commands),
        })
    result["coverage_counts"].update(veterancy_rank_cases=16, map_veterancy_seed_cases=90,
                                    map_percent_setter_calls=81, veterancy_initialization_cases=18,
                                    country_constructor_rows=2, constructor_rank_gate_rows=11,
                                    retail_country_list_histories=2, retail_country_list_passes=6)
    result["coverage_counts"].update(enslaved_voice_type_constructors=4,
                                    enslaved_voice_authored_reader_steps=15,
                                    enslaved_voice_retail_type_histories=3,
                                    enslaved_voice_retail_layer_passes=9,
                                    enslaved_voice_registered_sound_types=4)
    result["command"] = ("VERA20K_COMBATANT_INPUTS=/path/to/extracted/selection/layers "
                         "VERA20K_SELECTION_SOUND_INPUTS=/path/to/extracted/fixed/sound "
                         "python -m tools.input_oracle.selection_navigation --check")
    result["csf_sha256"] = stock_csf()[1]
    return result


def house_cheer_inputs():
    """Authored priors; admission and queue outputs are never calculated here."""
    def case(name, *, last=-100, rate=300, locked=0, count=0, read=0,
             write=0, house=7, frames=(), operations=None):
        return dict(name=name, input=dict(last_frame=last,
            maximum_cheer_rate=rate, input_locked=locked, queue_count=count,
            queue_read_index=read, queue_write_index=write, house_index=house),
            operations=operations or [dict(kind="press", current_frame=frame)
                                      for frame in frames])

    return [
        case("stock_initial_boundary", frames=(0, 200, 201, 501, 502)),
        case("positive_boundary", last=100, frames=(399, 400, 401)),
        case("zero_rate", last=100, rate=0, frames=(100, 101)),
        case("negative_rate", last=100, rate=-1, frames=(99, 100)),
        case("add_overflow_max_plus_one", last=2147483647, rate=1,
             frames=(0,)),
        case("add_overflow_min_minus_one", last=-2147483648, rate=-1,
             frames=(0, 2147483647)),
        case("wrapped_sum_equality", last=2147483647, rate=1,
             frames=(-2147483648, -2147483647)),
        case("current_frame_is_signed", frames=(-2147483648,)),
        case("locked_preserves_prior", last=100, locked=1,
             operations=[dict(kind="press", current_frame=401),
                         dict(kind="press", current_frame=401, input_locked=0)]),
        case("any_nonzero_lock", last=100, locked=4294967295, frames=(401,)),
        case("last_free_slot_wrap", count=127, write=127, frames=(201,)),
        case("empty_ring_wrap", read=127, write=127, frames=(201, 502), house=29),
        case("full_queue_consumes_opportunity", count=128, read=37, write=37,
             operations=[dict(kind="press", current_frame=201),
                         dict(kind="press", current_frame=202, queue_count=0),
                         dict(kind="press", current_frame=501),
                         dict(kind="press", current_frame=502)]),
        case("scene_reset_stock_rate", last=123456, read=12, write=12,
             operations=[dict(kind="reset", current_frame=123456),
                         dict(kind="press", current_frame=200),
                         dict(kind="press", current_frame=201)]),
        case("constructor_default_rate", last=-100, rate=-987654321,
             operations=[dict(kind="constructor_rate", current_frame=0),
                         dict(kind="press", current_frame=-99),
                         dict(kind="press", current_frame=-98)]),
    ]


def house_cheer_case(spec):
    """Reuse the registered keyboard VM; execute the selected producer unchanged."""
    m = SelectionFixture([], [[], [], [], [], []], seed=31)
    u, prior = m.uc, spec["input"]
    rules, clock = m.alloc(0x2000), m.alloc(16)
    m.put(0x8871E0, rules)
    m.put(0x7E1530, clock)
    m.put(m.houses["local"] + 0x30, prior["house_index"])
    m.put(0x84555C, prior["last_frame"] & 0xFFFFFFFF)
    m.put(rules + 0x580, prior["maximum_cheer_rate"] & 0xFFFFFFFF)
    m.put(0xA8B538, prior["input_locked"])
    for address, field in ((0xA802C8, "queue_count"),
                           (0xA802CC, "queue_read_index"),
                           (0xA802D0, "queue_write_index")):
        m.put(address, prior[field])
    # Existing records are supplied opaque occupied slots. Only actual new
    # writes are decoded, never their unused/padding bytes or prior payload.
    u.mem_write(0xA802D4, bytes(128 * 0x6F))
    u.mem_write(0xA83A54, bytes(128 * 4))
    command = m.objects["AllToCheer"]
    execute = m.get(m.get(command) + 0x20)
    assert execute == 0x536C00
    constructor_calls, imports, last_writes, queue_slots = [], [], [], set()
    rng_requests, detach = [], []

    def observe(vm, pc, _size, _data):
        sp = vm.reg_read(UC_X86_REG_ESP)
        if pc == 0x4C66C0:
            constructor_calls.append(dict(house_index=i32(vm, sp + 4),
                                          opcode=m.get(sp + 8)))
        elif pc == clock:
            imports.append(dict(service="timeGetTime", result=100000))
            return_from_sink(vm, 0, 100000)
        elif pc == 0x4068E0:
            # Existing event-constructor diagnostic transport, before its
            # actual opcode/House/frame stores; not a gameplay answer.
            return_from_sink(vm, 0)
        elif pc in (0x65C780, 0x65C7E0):
            rng_requests.append(f"{pc:08X}")
        elif pc in (0x70D4A0, 0x7258D0):
            detach.append(f"{pc:08X}")

    def observe_write(vm, _access, address, size, value, _data):
        if address == 0x84555C:
            last_writes.append(i32_value(value))
        elif 0xA802D4 <= address < 0xA802D4 + 128 * 0x6F:
            first = (address - 0xA802D4) // 0x6F
            last = (address + size - 1 - 0xA802D4) // 0x6F
            queue_slots.update(range(first, last + 1))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_WRITE, observe_write,
               begin=0x84555C, end=0x84555F)
    u.hook_add(UC_HOOK_MEM_WRITE, observe_write,
               begin=0xA802D4, end=0xA802D4 + 128 * 0x6F - 1)
    before = m.rng_bytes()
    phases = []
    for operation in spec["operations"]:
        m.put(0xA8ED84, operation["current_frame"] & 0xFFFFFFFF)
        if "input_locked" in operation:
            m.put(0xA8B538, operation["input_locked"])
        if "queue_count" in operation:
            # Explicit scheduler boundary: prior records have been drained.
            # This does not emulate that drain or change native queue indexes.
            m.put(0xA802C8, operation["queue_count"])
        constructor_calls.clear()
        imports.clear()
        last_writes.clear()
        queue_slots.clear()
        kind = operation["kind"]
        if kind == "press":
            m.invoke(execute, command, (0,))
        elif kind == "reset":
            m.invoke(0x734210, 0)
        elif kind == "constructor_rate":
            u.reg_write(UC_X86_REG_ESI, rules)
            m.run_region(0x665D92, 0x665D9C, required=(0x665D92,))
        else:
            raise ValueError(kind)
        events = []
        for slot in sorted(queue_slots):
            pointer = 0xA802D4 + slot * 0x6F
            events.append(dict(opcode=u.mem_read(pointer, 1)[0],
                house_index=struct.unpack("<b", u.mem_read(pointer + 2, 1))[0],
                frame=i32(u, pointer + 3),
                queued_at_ms=m.get(0xA83A54 + slot * 4)))
        phases.append(dict(input=operation, expected=dict(
            last_frame=i32(u, 0x84555C),
            maximum_cheer_rate=i32(u, rules + 0x580),
            queue_count=m.get(0xA802C8),
            queue_read_index=m.get(0xA802CC),
            queue_write_index=m.get(0xA802D0),
            new_events=events, constructor_calls=list(constructor_calls),
            last_frame_writes=list(last_writes), os_imports=list(imports))))
    assert m.rng_bytes() == before and not rng_requests and not detach
    assert m.text_hash() == m.code_hash
    return dict(name=spec["name"], input=prior, phases=phases)


def i32_value(value):
    return struct.unpack("<i", struct.pack("<I", value & 0xFFFFFFFF))[0]


def house_cheer_generate():
    assert image_sha256() == NATIVE_SHA256
    return dict(schema_version=1, source="unicorn/gamemd.exe",
        native_sha256=image_sha256(),
        cases=[house_cheer_case(spec) for spec in house_cheer_inputs()])


def house_cheer_metadata():
    return provenance(
        scope="Registered AllToCheer536C00 -> whole730F30, actual Event4C66C0 and inlined OutList append, reset734210 and Rules default store665D92",
        assumptions=[
            "Existing SelectionFixture/KeyboardFixture owns the VM, original532150 command registration, scratch/stack allocation, empty actors/layers, native call invocation and original .text immutability checks. No additional VM or admission/queue implementation is supplied.",
            "The selected current House index, Rules+580, signed last/current frames, Session+300 input lock and capacity128 OutList count/read/write indexes are explicit priors. Existing occupied records are opaque; only newly written event opcode/House/frame fields are observed. The constructor does not initialize byte+1 or arbitrary payload, so those bytes are not goldens.",
            "Every press invokes actual registered Execute536C00 with argument0 and executes whole730F30, including real4C66C0 and the inlined111-byte OutList copy, timestamp and index/count writes when admitted. This handler does not call the separate6521C0 enqueue body.",
            "The full-queue history explicitly supplies a drained queue_count0 before its second press, retaining both indexes. That boundary is not execution of local/network scheduling. Equality/rearm, signed wrapping ADD, signed current-frame CMP, nonzero lock and full-buffer rate consumption are original outputs.",
            "Reset controls execute whole734210, including both OS clock calls and the actual last-frame=-100 store. The constructor-default control executes only the original665D92 ten-byte store with supplied ESI=Rules; it is not whole Rules construction/loading. The stock300 input is independently retail-derived, not invented as a constructor default.",
            "Main/Scenario/MapGen buffers are initialized with the existing seed31 transport and asserted byte-identical through each history; no65C780/65C7E0 or detach70D4A0/7258D0 calls occur in these producer/reset controls. This does not cover downstream House/Infantry/audio RNG or timers.",
            "No Windows key delivery/loader, full keyboard configuration, event dispatch/House scan, Infantry DoAction, rendering, sound/device, saved-game or whole-frame scheduling is executed here. Existing Steam infantry_cheer controls retain their separate provenance and payload."],
        substitutions=[
            "Existing original-registration malloc7C8E17 transport supplies bounded zero-filled scratch storage; registry execution stops before533D20 file I/O.",
            "The declared timeGetTime import7E1530 returns100000 milliseconds using the existing return_from_sink OS transport. These values stamp OutList transport and independent reset click clocks, not the simulation frame or admission decision.",
            "Event constructor diagnostic4068E0 is an observed platform log boundary returning0 with cdecl caller cleanup; original Event, producer, reset and queue stores execute unchanged. No selected simulated-input, event/header, last-frame, gate, arithmetic or queue return is substituted."],
        entry_points=dict(registry=0x532150, registry_stop=0x533D20,
            registered_execute=0x536C00, command_handler=0x730F30,
            event_constructor=0x4C66C0, queue_copy=0x730F99,
            queue_full_branch=0x730F7B, last_frame_store=0x730FD3,
            scene_input_reset=0x734210, constructor_rate_store=0x665D92))


if __name__ == "__main__":
    import sys
    if "--house-cheer" in sys.argv[1:]:
        args = list(sys.argv[1:])
        args.remove("--house-cheer")
        finish_vectors(house_cheer_generate, HERE / "house_cheer.json",
            provenance=house_cheer_metadata, argv=args,
            source_paths={
                "selection_navigation": Path(__file__),
                "native_oracle": HERE.parent / "native_oracle.py",
                "keyboard_bindings": HERE.parent / "storage_oracle/keyboard_bindings.py",
                "fast_scroll": HERE / "fast_scroll.py",
                "rng": HERE.parent / "rmg_oracle/gen_rng_vectors.py",
            })
        sys.exit(0)
    finish_vectors(generate, HERE / "selection_navigation.json", provenance=metadata,
                   source_paths={
                       "selection_navigation": Path(__file__),
                       "native_oracle": HERE.parent / "native_oracle.py",
                       "keyboard_bindings": HERE.parent / "storage_oracle/keyboard_bindings.py",
                       "fast_scroll": HERE / "fast_scroll.py",
                       "rng": HERE.parent / "rmg_oracle/gen_rng_vectors.py",
                       "stock": HERE.parent / "sidebar_oracle/stock.py",
                       "type_fixture": HERE.parent / "projectile_oracle/bridge_render_inputs.py",
                       "ini_cache": HERE.parent / "rules_oracle/bridge_anim_inputs.py",
                       "heap": HERE.parent / "rules_oracle/bridge_anim_lists.py",
                       "ini_fixture": HERE.parent / "spatial_oracle/building_body_rules.py",
                       "key_crc": HERE.parent / "projectile_oracle/flat_art.py",
                       "cleanup_startup": HERE.parent / "projectile_oracle/ifv_impact.py",
                       "cost_owner": HERE.parent / "spatial_oracle/cost_of.py",
                       "atomic_transport_owner": HERE.parent / "spatial_oracle/infantry_deploy_action.py",
                       "sound_input_owner": HERE.parent / "spatial_oracle/anytown_damage/mtnk_attack.py",
                       "sound_source_order_owner": HERE.parent / "rules_oracle/bridge_child_sound.py",
                       "sound_name_owner": HERE / "area_guard.py",
                   })
