"""Execute the once-only multiplayer settings prefix 0x698161..0x69822D.

Its first entry draws Main RNG RandomRanged(0, 32768) for the port pool and
reads the [MultiPlayer] settings; a second entry on the same process skips the
draw. The original readers, RNG and FirewallHelper body execute unchanged;
only cached INI entries are supplied.
"""

from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EBP, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP

from tools.native_oracle import OracleError, finish_vectors, provenance, run_checked
from tools.rmg_oracle.gen_rng_vectors import STRUCT_LEN, seeded_struct
from tools.spatial_oracle.building_body_rules import (
    Fixture as IniFixture, ENTRY, INDEX, INI, RAW, SECTION, SP, TYPE,
)
from tools.storage_oracle.shell_relayout import Image


PORT = 0xA8E254
INITIALIZE_BEGIN, INITIALIZE_END = 0x698161, 0x69822D
MAIN_RNG, PROCESS_INI, INITIALIZED = 0x886B88, 0x8870C0, 0xB07788


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

    def initialize(self):
        guard_before = self.u.mem_read(INITIALIZED, 1)[0]
        rng_before = self.rng_hex()
        self.reads.clear()
        self.raw_draws.clear()
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
            ranged_calls=self.ranged_calls, raw_draws=list(self.raw_draws),
            reads=list(self.reads), port=self.signed32(PORT),
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
    return dict(source="unicorn/gamemd.exe", initialization_cases=initialization)


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope="Session698161..69822D once-only MainRng initialization and repeated settings reads",
        assumptions=[
            "Initialization histories enter at698161 with Session in EBP and stop69822D after SendDelay/PortNumberOverride stores; the broader country/phonebook reader is not claimed.",
            "Each initialization history uses shared seeded_struct executing original65C6D0, a zero first-entry gate, and two calls on the same VM without resetting RNG or gate. Full3F4-byte RNG states and actual65C84B stores are retained.",
        ],
        substitutions=[
            "Initialization uses existing building_body_rules.Fixture to supply cached INI entries with original native key CRCs at process owner8870C0; original5276D0/5295F0 parsers,65C7E0 RNG and5D93D0 firewall reader execute unchanged. No parser or RNG result substitution.",
        ],
        entry_points={"session_prefix": INITIALIZE_BEGIN, "session_stop": INITIALIZE_END,
                      "rng_seed": 0x65C6D0, "rng_ranged": 0x65C7E0,
                      "read_int": 0x5276D0, "read_bool": 0x5295F0, "firewall_read": 0x5D93D0},
    ))
