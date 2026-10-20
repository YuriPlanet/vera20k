"""Execute D7 OK validation, including original CRT decimal/hex parsing.

The original NetworkOptionsDialog callback 560480 receives WM_COMMAND/IDOK.
Only window transport, string lookup, message-box presentation, firewall reset
and file/INI output are substituted. atoi, strtok and sscanf execute unchanged;
their original arithmetic decides validation and partially committed fields.
"""

from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_ECX,
    UC_X86_REG_ESI, UC_X86_REG_ESP,
)

from tools.native_oracle import OracleError, finish_vectors, provenance, run_checked
from tools.rmg_oracle.gen_rng_vectors import STRUCT_LEN, seeded_struct
from tools.spatial_oracle.building_body_rules import (
    Fixture as IniFixture, ENTRY, INDEX, INI, RAW, SECTION, SP, TYPE,
)
from tools.storage_oracle.shell_relayout import Image, Machine, PARENT


CALLBACK = 0x560480
SOCKET, CARD, DESTINATION = 0xA8EBAA, 0xA8EBB4, 0xA8EBB8
SEND_DELAY, PORT = 0xA8E180, 0xA8E254
INITIAL = dict(socket=17, card=1, destination="AA.BB.CC.DD", send_delay=False, port=0)
INITIALIZE_BEGIN, INITIALIZE_END = 0x698161, 0x69822D
MAIN_RNG, PROCESS_INI, INITIALIZED = 0x886B88, 0x8870C0, 0xB07788


class NetworkOptionsMachine(Machine):
    """D7-specific input and output boundaries on the existing shell VM."""

    def __init__(self, inputs, initial):
        super().__init__(0xD7, 800, 600)
        self.inputs = inputs
        self.events = []
        self.result = self.alloc(4)
        self.w32(self.result, 0)
        self.thread = self.alloc(0x100)
        self.native_visits = dict(atoi=0, strtok=0, sscanf=0)
        self.uc.mem_write(SOCKET, struct.pack("<H", initial["socket"]))
        self.w32(CARD, initial["card"])
        self.uc.mem_write(DESTINATION, initial["destination"].encode("ascii") + b"\0")
        self.uc.mem_write(SEND_DELAY, bytes([initial["send_delay"]]))
        self.w32(PORT, initial["port"])
        handlers = {
            "GetWindowLongA": self.get_window_long,
            "SendMessageA": self.send_message,
            "SendDlgItemMessageA": self.send_dlg_message,
        }
        for address, name in Image(self.uc).imports().items():
            if name in handlers:
                self.w32(address, self.stub(handlers[name]))
        self.uc.hook_add(UC_HOOK_CODE, self.boundary)

    def state(self):
        return dict(
            socket=struct.unpack("<H", self.uc.mem_read(SOCKET, 2))[0],
            card=struct.unpack("<i", self.uc.mem_read(CARD, 4))[0],
            destination=Image(self.uc).cstr(DESTINATION),
            send_delay=bool(self.uc.mem_read(SEND_DELAY, 1)[0]),
            port=struct.unpack("<i", self.uc.mem_read(PORT, 4))[0],
        )

    def get_window_long(self):
        if (self.arg(0), self.arg(1)) != (PARENT, 8):
            raise OracleError("D7 requested unexpected window data")
        self.ret(8, self.result)

    def send_message(self):
        hwnd, message, size, target = (self.arg(i) for i in range(4))
        control = self.windows[hwnd]["id"]
        if message == 0x4B5 and control in (0x5D1, 0x5D3, 0x5D7):
            key = {0x5D1: "socket", 0x5D3: "destination", 0x5D7: "port"}[control]
            value = self.inputs[key].encode("ascii")
            # Cases fit the callback's requested buffer: avoid pretending this
            # fixture proves the owner-draw edit's independent truncation policy.
            if len(value) >= size:
                raise OracleError(f"{key} input exceeds supplied read buffer")
            self.uc.mem_write(target, value + b"\0")
            self.ret(16, len(value))
        elif message == 0x147 and control == 0x5D4:
            self.ret(16, self.inputs["card"])
        else:
            raise OracleError(f"unexpected D7 control/message {control:#x}/{message:#x}")

    def send_dlg_message(self):
        if tuple(self.arg(i) for i in range(5)) != (PARENT, 0x762, 0xF0, 0, 0):
            raise OracleError("unexpected D7 checkbox read")
        self.ret(20, int(self.inputs["send_delay"]))

    def boundary(self, uc, address, _size, _data):
        if address == 0x622B50:  # No common-handler consumption of WM_COMMAND.
            self.ret(8, 0)
        elif address == 0x7D140B:  # Same _getptd boundary as ini_token_readers.
            self.ret(0, self.thread)
        elif address in (0x7C9BFD, 0x7C9CC2, 0x7CA530):
            self.native_visits[{0x7C9BFD: "atoi", 0x7C9CC2: "strtok", 0x7CA530: "sscanf"}[address]] += 1
        elif address == 0x734E60:
            # Return the original ASCII key as an opaque message identifier.
            self.ret(8, uc.reg_read(UC_X86_REG_ECX))
        elif address == 0x5D3490:
            self.events.append(dict(kind="message", key=Image(uc).cstr(self.arg(0)), state=self.state()))
            self.ret(20, 0)
        elif address == 0x5D8FC0:
            self.events.append(dict(kind="reset_firewall"))
            self.ret(0)
        elif address == 0x65CA80:
            if Image(uc).cstr(self.arg(0)) != "RA2MD.INI":
                raise OracleError("unexpected network settings filename")
            self.ret(4, uc.reg_read(UC_X86_REG_ECX))
        elif address in (0x529560, 0x5275C0):
            self.events.append(dict(
                kind="write_ini", section=Image(uc).cstr(self.arg(0)),
                key=Image(uc).cstr(self.arg(1)),
                value=bool(self.arg(2) & 0xFF) if address == 0x529560 else struct.unpack("<i", struct.pack("<I", self.arg(2)))[0],
            ))
            self.ret(12 if address == 0x529560 else 16, 1)
        elif address == 0x474430:
            self.events.append(dict(kind="save_file"))
            self.ret(8, 1)
        elif address == 0x65CA00:
            self.ret(0)


def run_case(name, inputs, initial=None):
    initial = dict(INITIAL, **(initial or {}))
    inputs = dict(dict(socket="", destination="", port="", card=2, send_delay=False), **inputs)
    machine = NetworkOptionsMachine(inputs, initial)
    machine.call(CALLBACK, stack_args=(PARENT, 0x111, 1, 0), count=500_000)
    return dict(name=name, inputs=inputs, initial=initial, state=machine.state(),
                dialog_result=machine.r32(machine.result), events=machine.events,
                native_visits=machine.native_visits)


class SessionInitializationFixture(IniFixture):
    """Original once-only session prefix on the existing cached-INI fixture.

    Only cached key/entry objects are supplied. Every original ReadInt/ReadBool,
    RandomRanged and FirewallHelper body runs unchanged in the inherited VM.
    """

    def __init__(self, seed, settings):
        super().__init__()
        self.settings = settings
        self.u.mem_write(MAIN_RNG, seeded_struct(seed))
        self.u.mem_write(INITIALIZED, b"\0")
        self.u.mem_write(TYPE + 0x1F8, b"MultiPlayer\0")
        self.cached_entries = {}
        # Build native-CRC caches outside timed emulation. Re-entering another
        # Unicorn VM from an instruction hook can exhaust the outer timeout.
        for key in (0x826438, 0x83F19C, 0x826420, 0x830CC0, 0x830CB0, 0x82A2E8, 0x82A2D4):
            raw = settings.get(Image(self.u).cstr(key))
            self.ini(key, raw)
            self.cached_entries[key] = {
                address: bytes(self.u.mem_read(address, size))
                for address, size in ((INI, 0x40), (SECTION, 0x44), (INDEX, 8),
                                      (ENTRY, 0x14), (RAW, len(raw or "") + 1))
            }
        self.reads = []
        self.raw_draws = []
        self.ranged_result = None
        self.ranged_calls = 0
        self.original_code = {
            (INITIALIZE_BEGIN, INITIALIZE_END): bytes(self.u.mem_read(
                INITIALIZE_BEGIN, INITIALIZE_END - INITIALIZE_BEGIN)),
            (0x65C6D0, 0x65C88D): bytes(self.u.mem_read(0x65C6D0, 0x65C88D - 0x65C6D0)),
        }
        self.u.hook_add(UC_HOOK_CODE, self.observe)

    def read32(self, address):
        return struct.unpack("<I", self.u.mem_read(address, 4))[0]

    def signed32(self, address):
        return struct.unpack("<i", self.u.mem_read(address, 4))[0]

    def rng_hex(self):
        return bytes(self.u.mem_read(MAIN_RNG, STRUCT_LEN)).hex()

    def observe(self, uc, address, _size, _data):
        if address in (0x5276D0, 0x5295F0):
            sp = uc.reg_read(UC_X86_REG_ESP)
            section, key, default = (self.read32(sp + offset) for offset in (4, 8, 12))
            if uc.reg_read(UC_X86_REG_ECX) != PROCESS_INI or Image(uc).cstr(section) != "MultiPlayer":
                raise OracleError("session prefix used unexpected INI owner/section")
            name = Image(uc).cstr(key)
            raw = self.settings.get(name)
            self.reads.append(dict(key=name, raw=raw, default=default))
            # Reuse the shared native-CRC cached entry builder. Copy its INI
            # header to the actual process address the unchanged caller uses.
            # This supplies input data, never a parsed value or return address.
            for target, data in self.cached_entries[key].items():
                uc.mem_write(PROCESS_INI if target == INI else target, data)
            self.write(PROCESS_INI + 4, section)
        elif address == 0x65C7E0:
            sp = uc.reg_read(UC_X86_REG_ESP)
            bounds = (self.read32(sp + 4), self.read32(sp + 8))
            if uc.reg_read(UC_X86_REG_ECX) != MAIN_RNG or bounds != (0, 32768):
                raise OracleError("session prefix changed RNG owner or bounds")
            self.ranged_calls += 1
        elif address == 0x65C84B:
            # Observe the actual inlined XOR store, including rejected draws.
            self.raw_draws.append(uc.reg_read(UC_X86_REG_ESI) & 0xFFFFFFFF)
        elif address == 0x6981B6:
            self.ranged_result = uc.reg_read(UC_X86_REG_EAX)

    def initialize(self):
        guard_before = self.u.mem_read(INITIALIZED, 1)[0]
        rng_before = self.rng_hex()
        self.reads.clear()
        self.raw_draws.clear()
        self.ranged_result = None
        self.ranged_calls = 0
        self.u.reg_write(UC_X86_REG_ESP, SP)
        self.u.reg_write(UC_X86_REG_EBP, TYPE)
        required = [INITIALIZE_BEGIN, 0x6981E6, 0x5295F0, 0x5276D0]
        if not guard_before:
            required.extend([0x65C7E0, 0x65C84B, 0x5D93D0])
        run_checked(self.u, INITIALIZE_BEGIN, INITIALIZE_END, count=500_000,
                    required_addresses=required)
        # The following PhoneIndex read has pushed three args by this stop.
        if self.u.reg_read(UC_X86_REG_ESP) != SP - 12:
            raise OracleError("session initialization slice changed its stack balance")
        for (begin, end), original in self.original_code.items():
            if bytes(self.u.mem_read(begin, end - begin)) != original:
                raise OracleError("session initialization patched native instructions")
        row = dict(
            guard_before=guard_before, guard_after=self.u.mem_read(INITIALIZED, 1)[0],
            rng_before_hex=rng_before, rng_hex=self.rng_hex(),
            ranged_calls=self.ranged_calls, ranged_result=self.ranged_result,
            raw_draws=list(self.raw_draws), reads=list(self.reads),
            port_base=self.signed32(TYPE + 0x2FF0),
            force_port_base=self.signed32(TYPE + 0x2FF4),
            port_pool=self.signed32(0xABFED8),
            firewall_settings=self.signed32(0xABFECC),
            firewall_delta=self.signed32(0xABFED4),
            send_delay=bool(self.u.mem_read(SEND_DELAY, 1)[0]), port=self.signed32(PORT),
        )
        if row["guard_after"] != 1 or self.ranged_calls != int(not guard_before):
            raise OracleError("session prefix did not follow the original once gate")
        if guard_before and (self.raw_draws or row["rng_hex"] != rng_before):
            raise OracleError("repeated session initialization consumed RNG")
        return row


def initialization_case(name, seed, settings):
    machine = SessionInitializationFixture(seed, settings)
    initial = machine.rng_hex()
    calls = [machine.initialize(), machine.initialize()]
    return dict(name=name, seed=seed, settings=settings, initial_rng_hex=initial, calls=calls)


def generate():
    rows = [
        ("blank_defaults", {}, None),
        ("valid_boundaries", dict(socket="16383", destination="FF.00.a1.7f", port="65535"), dict(port=1234)),
        ("socket_above_limit", dict(socket="16384"), None),
        ("socket_wraps_u16", dict(socket="65536"), None),
        ("socket_too_many_characters", dict(socket="000001"), None),
        ("destination_missing_component", dict(destination="01.02.03"), None),
        ("destination_invalid_hex", dict(destination="01.GG.03.04"), None),
        ("destination_component_overflow", dict(destination="01.100.03.04"), None),
        ("destination_hex_prefix_and_suffix", dict(destination="0x1.2z.3.4"), None),
        ("port_invalid_after_lan_commit", dict(socket="123", destination="01.02.03.04", port="65536", send_delay=True), None),
        ("negative_port", dict(port="-1"), None),
        ("warnings_in_order", dict(port="1234", send_delay=True), None),
        ("warnings_already_enabled", dict(port="4321", send_delay=True), dict(port=1234, send_delay=True)),
        ("disable_send_delay", {}, dict(port=1234, send_delay=True)),
    ]
    initialization = [
        initialization_case("default_port_pool", 0, {}),
        initialization_case("explicit_port_pool_still_draws", 0,
                            dict(PortPool="$1234", SendDelay="yes", PortNumberOverride="-1")),
        initialization_case("range_rejects_two_draws", 1234, {}),
    ]
    if initialization[0]["calls"][0]["rng_hex"] != initialization[1]["calls"][0]["rng_hex"]:
        raise OracleError("explicit PortPool altered the unconditional RNG consumption")
    if len(initialization[2]["calls"][0]["raw_draws"]) != 3:
        raise OracleError("rejection control did not exercise two native rejections")
    return dict(source="unicorn/gamemd.exe", cases=[run_case(*row) for row in rows],
                initialization_cases=initialization)


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope="Original D7 WM_COMMAND/IDOK validation and field writes plus session698161..69822D once-only MainRng initialization and repeated settings reads",
        assumptions=[
            "D7 already exists; controls supply bounded ASCII text, selected adapter and checkbox state. Every row has a fresh VM and explicit previous settings.",
            "Original CRT atoi7C9BFD, strtok7C9CC2 and sscanf7CA530 execute with image static C-locale tables; per-thread CRT storage is supplied zeroed.",
            "The dialog's initial population, edit widget filtering/truncation, native Win32 rendering, file persistence internals and network transport are outside this component comparison.",
            "Initialization histories enter at698161 with Session in EBP and stop69822D after SendDelay/PortNumberOverride stores; the broader country/phonebook reader is not claimed.",
            "Each initialization history uses shared seeded_struct executing original65C6D0, a zero first-entry gate, and two calls on the same VM without resetting RNG or gate. Full3F4-byte RNG states and actual65C84B stores are retained.",
        ],
        substitutions=[
            "Existing shell_relayout.Machine provides mapped retail image, original D7 resource children and explicit Win32 import transport; unexpected imports fail.",
            "Common shell message handler622B50 returns unhandled; GetWindowLongA returns a supplied result cell; GetDlgItem uses original resource control IDs.",
            "StringTable734E60 returns an opaque original key; message-box5D3490 records that key and current settings instead of rendering or waiting for dismissal.",
            "CRT _getptd7D140B returns supplied per-thread storage; no decimal or hexadecimal parser is substituted.",
            "Firewall reset5D8FC0 and RawFile/INI persistence calls are ordered sinks; no disk or live network state changes.",
            "Initialization uses existing building_body_rules.Fixture to supply cached INI entries with original native key CRCs at process owner8870C0; original5276D0/5295F0 parsers,65C7E0 RNG and5D93D0 firewall reader execute unchanged. No parser or RNG result substitution.",
        ],
        entry_points={"dialog": CALLBACK, "atoi": 0x7C9BFD, "strtok": 0x7C9CC2, "sscanf": 0x7CA530,
                      "session_prefix": INITIALIZE_BEGIN, "session_stop": INITIALIZE_END,
                      "rng_seed": 0x65C6D0, "rng_ranged": 0x65C7E0,
                      "read_int": 0x5276D0, "read_bool": 0x5295F0, "firewall_read": 0x5D93D0},
    ))
