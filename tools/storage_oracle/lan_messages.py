"""Original455-byte LAN application construction blocks before transport.

The active producers' validation/timer gates are explicit slice inputs. The
original zeroing, string copies, field getters and packet stores execute; no
Python packet encoder or substitute hash/version result is used.
"""

from pathlib import Path
import struct

from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EBP,
                               UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EDI, UC_X86_REG_ESP)

from tools.native_oracle import STACK_BASE, STACK_SIZE, OracleError, provenance, run_checked
from tools.storage_oracle.shell_relayout import Machine


class PacketMachine(Machine):
    def __init__(self):
        super().__init__(0xBB, 800, 600)
        self.name = b'Host-\xe9'
        self.game = b'Game-\xff'
        self.serial = b'1234567890123456789012'
        self.uc.mem_write(0xA8B380, self.name + b'\0')
        self.uc.mem_write(0xABFF7C, self.serial + b'\0')
        self.uc.mem_write(0xA8D610, b''.join(bytes((c, 0)) for c in self.game) + b'\0\0')
        self.w32(0xA8B238 + 0x174, -3)
        self.w32(0xA8B238 + 0x178, 0)
        self.w32(0xA8B238 + 0x15C, 6)
        self.w32(0xA8B238 + 0x160, 0)
        self.w32(0xA8B37C, 2)
        self.w32(0xAC026C, 0x11223344)
        self.w32(0xAC0274, 0x55667788)
        self.w32(0xAC0270, 0x99AABBCC)
        self.w32(0xA8DA8C, 4)
        self.w32(0x8316A4, 7)
        self.uc.mem_write(0xA8D60E, b'\x01')
        self.node = self.alloc(0x90)
        self.uc.mem_write(self.node, b''.join(bytes((c, 0)) for c in self.game) + b'\0\0')
        self.w32(self.node + 0x4B, -3)
        self.w32(self.node + 0x4F, 0)
        self.w32(self.node + 0x53, 6)
        self.w32(self.node + 0x57, 0)
        self.w32(self.node + 0x6B, -1)
        self.vector = self.alloc(8)
        self.w32(self.vector, self.node)
        self.w32(self.vector + 4, self.node)
        self.w32(0xA8DA60, self.vector)
        self.w32(0xA8DA48, self.vector)
        self.w32(0xA8DA78, self.vector)
        self.w32(0xA8DA84, 2)
        self.w32(0xAC0260, 1)

    def packet(self, name, entry, *, ebx=0, ebp=0, eax=0, ecx=0, edx=None):
        stack = STACK_BASE + STACK_SIZE - 0x4000
        self.uc.mem_write(stack, b'\xA5' * 0x3000)
        for register, value in ((UC_X86_REG_ESP, stack), (UC_X86_REG_EAX, eax),
                                (UC_X86_REG_EBX, ebx), (UC_X86_REG_EBP, ebp),
                                (UC_X86_REG_ECX, ecx)):
            self.uc.reg_write(register, value)
        # Keep the list pointer used by the keepalive construction's caller.
        self.uc.reg_write(UC_X86_REG_EDX, self.vector if edx is None else edx)
        run_checked(self.uc, entry, 0x5410F0, count=500_000, timeout_us=1_000_000)
        sp = self.uc.reg_read(UC_X86_REG_ESP)
        pointer, length, reliable, destination, port, sequence = [self.r32(sp + 4 + 4*i) for i in range(6)]
        if length != 455:
            raise OracleError(f'{name} emitted unexpected extent{length}')
        return dict(name=name, entry=f'{entry:08X}', payload=bytes(self.uc.mem_read(pointer, length)).hex(),
                    reliable=reliable, port=port, sequence=sequence,
                    destination=None if not destination else bytes(self.uc.mem_read(destination, 12)).hex())


def setup_packets():
    m = PacketMachine()
    for address in (0xAC119C, 0xAC11A0, 0xAC11B0, 0xAC11A4, 0xAC11A8, 0xAC11AC, 0xAC11B4):
        m.w32(address, m.alloc(128))
    slots = [m.node] + [m.r32(a) for a in (0xAC119C, 0xAC11A0, 0xAC11B0, 0xAC11A4, 0xAC11A8, 0xAC11AC, 0xAC11B4)]
    for index, pointer in enumerate(slots):
        m.w32(0xA8DA90 + index * 4, pointer)
    rows = [m.packet('roster', 0x5DEAC7)]
    m = PacketMachine()
    text = m.alloc(16)
    m.uc.mem_write(text, b'K1\0')
    rows.append(m.packet('options_status', 0x5E3765, edx=text))
    m = PacketMachine()
    m.stubs[0x4068E0] = lambda: m.ret(0)
    host, text = m.alloc(32), m.alloc(64)
    m.uc.mem_write(host, m.game + b'\0')
    m.uc.mem_write(text, b'R-2,6,-2,-2\0')
    rows.append(m.packet('player_choices', 0x5E3850, ecx=host, edx=text))
    for name, entry in [('go', 0x5DD57B), ('ready', 0x5DD916), ('start_prompt', 0x5E3630)]:
        m = PacketMachine()
        m.w32(0xA8B238, 3)
        m.w32(0xA8B550, 27)
        rows.append(m.packet(name, entry))
    return rows


def options_format_case(name, fields):
    from unicorn import UC_HOOK_CODE
    m = PacketMachine()
    m.uc.hook_add(UC_HOOK_CODE, lambda _u, a, _s, _d: m.ret(0) if a == 0x4068E0 else None)
    for a, key in [(0xA8ED94, 'seed'), (0xA8EB60, 'current_speed'), (0xA8B268, 'game_speed'),
                   (0xA8B25C, 'credits'), (0xA8B270, 'unit_count'), (0xA8B250, 'mode_index'),
                   (0xA8EB68, 'cooperative_difficulty'), (0xA8BB0C, 'map_player_capacity'),
                   (0xA8BB04, 'map_file_size')]:
        m.w32(a, fields[key])
    for a, key in [(0xA8B262, 'short_game'), (0xA8B263, 'superweapons'), (0xA8B264, 'build_off_ally'),
                   (0xA8B320, 'mcv_redeploy'), (0xA8B261, 'crates'), (0xA8BB08, 'official_map')]:
        m.uc.mem_write(a, bytes([int(fields[key])]))
    m.uc.mem_write(0xA8B322, (fields['map_description'] + '\0').encode('utf-16-le'))
    m.uc.mem_write(0xA8B8E0, fields['scenario_filename'].encode('latin1') + b'\0')
    m.uc.mem_write(0xA8BAE2, fields['map_digest'].encode('latin1') + b'\0')
    pointers = m.alloc(4 * len(fields['humans']))
    for index, human in enumerate(fields['humans']):
        node = m.alloc(0x90)
        m.uc.mem_write(node, (human['name'] + '\0').encode('utf-16-le'))
        for offset, key in ((0x4B, 'country'), (0x53, 'color'), (0x5B, 'start'), (0x63, 'team')):
            m.w32(node + offset, human[key])
            m.w32(node + offset + 4, 0)
        m.w32(pointers + index * 4, node)
    m.w32(0xA8DA78, pointers)
    m.w32(0xA8DA84, len(fields['humans']))
    for index, ai in enumerate(fields['ai']):
        for a, key in [(0xA8B27C, 'difficulty'), (0xA8B29C, 'country'), (0xA8B2BC, 'color'),
                       (0xA8B2DC, 'start'), (0xA8B2FC, 'team')]:
            m.w32(a + index * 4, ai[key])
    output = m.alloc(2048)
    m.call(0x5DBB60, ecx=output, count=500_000)
    raw = bytes(m.uc.mem_read(output, 2048))
    return dict(name=name, fields=fields, text=raw[:raw.index(0)].hex())


def options_formats():
    fields = dict(seed=12345, current_speed=3, game_speed=4, credits=10000, unit_count=10,
                  short_game=True, superweapons=True, build_off_ally=False, mcv_redeploy=True,
                  crates=False, mode_index=0, cooperative_difficulty=2, map_description='MapéΩ',
                  map_player_capacity=2, official_map=False, map_file_size=1234,
                  scenario_filename='map.mpr', map_digest='ABCDef01',
                  humans=[dict(name='Host-é', country=-2, color=6, start=-1, team=-1),
                          dict(name='Guest-Ω', country=3, color=1, start=2, team=0)],
                  ai=[dict(difficulty=-1, country=-1, color=-1, start=-2, team=-2) for _ in range(8)])
    fields['ai'][0] = dict(difficulty=2, country=-3, color=5, start=-2, team=1)
    normal = options_format_case('two_humans', fields)
    import copy
    extreme = copy.deepcopy(fields)
    extreme.update(seed=0xFFFFFFFF, current_speed=-0x80000000, credits=-1, map_file_size=-1,
                   map_description='A😀B', official_map=True, short_game=False, crates=True)
    return [normal, options_format_case('signed_fields', extreme)]


def packed_descriptions():
    encodings, decodings = [], []
    controls = [[], [65], [0xE9], [0x100], [0xD83D, 0xDE00], [97, 0, 98],
                [65, 66, 67, 68, 69, 70, 71], [0xFFFF], [0xD800], [0xDC00]]
    for units in controls:
        m = PacketMachine()
        source = m.alloc(2 * (len(units) + 1))
        m.uc.mem_write(source, struct.pack('<' + 'H' * (len(units) + 1), *units, 0))
        wide, narrow = m.alloc(4), m.alloc(4)
        m.w32(wide, source)
        m.call(0x7B82E0, ecx=narrow, edx=wide)
        raw = bytes(m.uc.mem_read(m.r32(narrow), 512))
        encodings.append(dict(units=units, packed=raw[:raw.index(0)].hex()))
    controls = [bytes.fromhex(row['packed']) for row in encodings]
    controls += [b'', b'\x01', b'\x7F', b'\x80', b'\xFF', b'\xFF'*4, b'\x80'*6, b'\x41\x01\x80\x80']
    for packed in dict.fromkeys(controls):
        m = PacketMachine()
        source, narrow, wide = m.alloc(len(packed) + 1), m.alloc(4), m.alloc(4)
        m.uc.mem_write(source, packed + b'\0')
        m.w32(narrow, source)
        m.call(0x7B83E0, ecx=wide, edx=narrow)
        pointer, units = m.r32(wide), []
        while unit := struct.unpack('<H', m.uc.mem_read(pointer + 2 * len(units), 2))[0]:
            units.append(unit)
            if len(units) > 256:
                raise OracleError('unterminated packed description output')
        decodings.append(dict(packed=packed.hex(), units=units))
    return dict(encodings=encodings, decodings=decodings)



def go_waits():
    """Original5DD5E5/5DD950 branch gates; service is a declared input."""
    host, guest = [], []
    for queued, start, now in [(0, 100, 100), (1, 100, 100), (1, 100, 101),
                               (1, 100, 1299), (1, 100, 1300), (1, 100, 1301)]:
        m = PacketMachine()
        stack = STACK_BASE + STACK_SIZE - 0x3000
        m.uc.reg_write(UC_X86_REG_ESP, stack)
        m.uc.reg_write(UC_X86_REG_EBP, 0)
        m.uc.reg_write(UC_X86_REG_EBX, 0xFFFFFFFF)
        clocks = iter((start, now, now))
        count, calls = [queued], []
        m.stubs[0x6C8C40] = lambda: m.ret(0, next(clocks))
        m.stubs[0x5422D0] = lambda: m.ret(0, count[0])
        def service():
            calls.append(1)
            count[0] = 0
            m.ret(0)
        m.stubs[0x48D080] = service
        run_checked(m.uc, 0x5DD5E5, 0x5DD63E, count=10_000, timeout_us=1_000_000)
        host.append(dict(queued=queued, start=start, now=now, service_calls=len(calls)))
    for queued, service_ok in [(0, True), (1, True), (1, False), (0, False)]:
        m = PacketMachine()
        m.uc.reg_write(UC_X86_REG_ESP, STACK_BASE + STACK_SIZE - 0x3000)
        count, calls = [queued], []
        m.stubs[0x5422D0] = lambda: m.ret(0, count[0])
        m.stubs[0x541820] = lambda: m.ret(0, int(service_ok))
        def service():
            calls.append(1)
            count[0] = 0
            m.ret(0)
        m.stubs[0x48D080] = service
        run_checked(m.uc, 0x5DD950, 0x5DD97F, count=10_000, timeout_us=1_000_000)
        guest.append(dict(queued=queued, service_ok=service_ok, service_calls=len(calls)))
    return dict(host=host, guest=guest)


def start_timing_case(name, host, completed, responses):
    """Execute caller arithmetic; only ResponseTime and debug I/O are supplied."""
    machine = PacketMachine()
    stack = STACK_BASE + STACK_SIZE - 0x3000
    machine.uc.mem_write(stack - 0x1000, b'\xA5' * 0x2000)
    machine.uc.reg_write(UC_X86_REG_ESP, stack)
    # Host5DD493 establishes EDI=2 before the pre-GO block.
    machine.uc.reg_write(UC_X86_REG_EDI, 2)
    reads = []
    def response_time():
        if len(reads) >= len(responses):
            raise OracleError(f'{name}: unexpected ResponseTime read')
        value = responses[len(reads)]
        reads.append(value)
        machine.ret(0, value)
    machine.stubs[0x5424C0] = response_time
    machine.stubs[0x4068E0] = lambda: machine.ret(0)
    entry = {(True, False): 0x5DD541, (True, True): 0x5DD819,
             (False, False): 0x5DD8A0, (False, True): 0x5DD9AC}[host, completed]
    run_checked(machine.uc, entry, 0x540C60, count=10_000, timeout_us=1_000_000)
    if machine.uc.reg_read(UC_X86_REG_ECX) != 0xA8E9C0:
        raise OracleError(f'{name}: wrong timing receiver')
    if len(reads) != len(responses):
        raise OracleError(f'{name}: unused declared ResponseTime observations')
    sp = machine.uc.reg_read(UC_X86_REG_ESP)
    retry_delta, max_retries, timeout, apply_global = [machine.r32(sp + 4 + 4*i) for i in range(4)]
    return dict(name=name, host=host, completed=completed, entry=f'{entry:08X}',
                responses=responses, reads=len(reads), retry_delta=retry_delta,
                max_retries=max_retries, timeout=timeout, apply_global=apply_global)


def start_timings():
    controls = [
        ('host_pre_below', True, False, [27]),
        ('host_pre_equal_rereads', True, False, [28, 10]),
        ('host_pre_above_rereads', True, False, [29, 70]),
        ('host_pre_unsigned_high', True, False, [0x80000000, 0xFFFFFFFD]),
        ('host_pre_add_wrap', True, False, [0xFFFFFFFE]),
        ('host_pre_minus_one', True, False, [0xFFFFFFFF]),
        ('host_post_below_equal_retry', True, True, [14, 28]),
        ('host_post_equal_timeout_rereads', True, True, [15, 16, 28]),
        ('host_post_both_reread_without_reclamp', True, True, [16, 4, 29, 7]),
        ('host_post_shift_and_add_wrap', True, True, [0x20000000, 0xFFFFFFFE]),
        ('host_post_unsigned_shift_high', True, True, [0x10000000, 0x20000000, 0x80000000, 0xFFFFFFFF]),
        ('host_post_minus_one', True, True, [0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF]),
        ('guest_pre_below', False, False, [27]),
        ('guest_pre_equal_no_reread', False, False, [28]),
        ('guest_pre_above_rereads', False, False, [29, 4]),
        ('guest_pre_unsigned_high', False, False, [0x80000000, 0xFFFFFFFF]),
        ('guest_pre_add_wrap', False, False, [0xFFFFFFFE]),
        ('guest_pre_minus_one', False, False, [0xFFFFFFFF]),
        ('guest_post_below', False, True, [19]),
        ('guest_post_equal', False, True, [20]),
        ('guest_post_above', False, True, [21]),
        ('guest_post_signed_high', False, True, [0x80000000]),
        ('guest_post_minus_one', False, True, [0xFFFFFFFF]),
        ('guest_post_shift_signed_high', False, True, [0x10000000]),
        ('guest_post_shift_wrap', False, True, [0x20000000]),
        ('guest_post_shift_negative', False, True, [0x7FFFFFFF]),
        ('guest_post_shift_wrap_positive', False, True, [0x20000010]),
    ]
    return [start_timing_case(*control) for control in controls]


def generate():
    rows = []
    for name, entry, ebx, ebp, eax in [
        ('discovery', 0x5DFB6A, 0, 0, 0),
        ('game_query', 0x5DFC47, 0, 0, 0),
        ('presence', 0x5DFD34, 0, 0, 0),
        ('advertisement', 0x5DFE94, 0, 0, 0),
        ('participant', 0x5DFF6E, 0, 0, 0),
        ('join', 0x5DF6F5, 1, 0x123, 0),
        ('acceptance', 0x5E1A8F, 3, None, 0),
        ('denial', 0x5E157B, 1, 0, 0),
        ('kick', 0x5DB600, 0, 0, 0),
        ('departure', 0x5E09CD, 1, 0, 0),
        ('keepalive', 0x5DB91D, 0, 1, 0),
    ]:
        machine = PacketMachine()
        rows.append(machine.packet(name, entry, ebx=ebx, ebp=machine.node if ebp is None else ebp,
                                   eax=eax, ecx=machine.node if name == 'kick' else 0))
    low_zero = PacketMachine()
    # 5DFC54..5DFC6E takes the selected node through A8DA60/AC0260,
    # not the local game's A8D610 wide buffer.
    low_zero.uc.mem_write(low_zero.node, struct.pack('<4H', ord('A'), 0x100, ord('B'), 0))
    rows.append(low_zero.packet('game_query_low_zero', 0x5DFC47))
    hashes = []
    for units in ([], [97, 98, 99], [65, 98, 90], [0xE9, 97],
                  [0x100, 98], [0xD83D, 0xDE00], [97, 0, 98]):
        machine = PacketMachine()
        pointer = machine.alloc((len(units) + 1) * 2)
        machine.uc.mem_write(pointer, struct.pack('<' + 'H' * (len(units) + 1), *units, 0))
        machine.call(0x5DAC00, ecx=pointer, count=100_000)
        hashes.append(dict(units=units, value=machine.uc.reg_read(UC_X86_REG_EAX)))
    return dict(source='unicorn/gamemd.exe', name='Host-é', game='Game-ÿ',
                serial='1234567890123456789012', packets=rows, game_name_hashes=hashes,
                setup_packets=setup_packets(), options_formats=options_formats(),
                packed_descriptions=packed_descriptions(), go_waits=go_waits(),
                start_timings=start_timings())


def metadata():
    return provenance(
        scope='Original455-byte LAN discovery/join/setup packet construction blocks, full5DBB60 options formatting,7B82E0/7B83E0 packed UTF-16 and pre/post-GO retry timing; stops at transport/timing call boundaries',
        assumptions=[
            'Validation/timer gates are upstream slice inputs. Stack startsA5; each construction executes original full455-byte zeroing and field stores. Stops at Send_Global_Message entry before any queue or I/O effect.',
            'Local narrow nameHost-E9, wide gameGame-00FF and22-byte serial are supplied. Session resolved country-3/color6 and presence country2/color6 are explicit fields. Selector sentinels are0 so original69B700/69B720 and696F10/696F20 return the resolved values.',
            'Join compatibility inputs AC026C=11223344,AC0274=55667788,AC0270=99AABBCC are declared hash controls, not computed retail file hashes. Native74FDA0/74FDB0 and777B50 run unchanged. Observer input123h tests the actual low-byte store.',
            'Source vector points to a supplied node. Native5DAC00 computes its wide game-name hash; participant field53=4/observer row7 and observed node flag-1 are controlled. Acceptance bypasses allocation/validation and starts with assigned row3 and node pointer, but executes original field getters/stores.',
            'Denial5 executes the no-active-roster producer. Departure/keepalive slices use declared source-vector entries only to reach the firstsend; this does not establish validation, retries, or membership lifecycle.',
            'Kick control executes the complete5DB600 producer with a supplied destination node. Its455-byte payload has type8 at0 and reason4 at2F; original zeroing leaves all other byteszero. The send stops at5410F0 before admission.',
            'Game query selected node at [A8DA60+AC0260*4] contains A/U0100/B and executes original7350C0 narrowing and C-string copy; low-byte NUL terminates the copied name. Separate complete5DAC00 controls cover empty/mixed-case/high-unit/surrogate-pair/embeddedNUL names in the startup C locale; no hash result is supplied.',
            'Setup controls execute original full455-byte zero/copy stores for roster16 (all seven original sentinel names and one supplied human), options17 K1, GO0B MaxAhead27, Ready13 and LAN start-prompt28. Original5E8DC0 formats all eight wide comma-terminated roster slots.',
            'Player choices execute complete5E3850 LAN branch with host name and R-2,6,-2,-2 text. Original color/hash stores hit global receive bufferA8D7F7/A8D7FB, not the outgoing local packet, whose final8 bytes stayzero. Only DebugLog4068E0 is a sink.',
            'Full5DBB60 reads declared globals, human nodes and eight AI rows. Original getters,7B82E0 packed description and CRT sprintf execute. Two controls exercise signed fields, resolved selections, AIcountry-3 normalization and wide/narrow name differences. Only DebugLog4068E0 is a sink.',
            'GO branch controls execute5DD5E5..63E and5DD950..97F. Queue counts, wall-clock reads and541820 return are explicit boundary inputs; Network_ServiceLoop consumes one pending entry. Host1200 gate is inverted: fresh timer skips service; expired timer services. Guest waits only while queue>0 and541820 succeeds.',
            'Start timing controls execute5DD541,5DD819,5DD8A0 and5DD9AC to the540C60 call boundary. ResponseTime5424C0 returns each declared observation in order; DebugLog4068E0 is a sink. Host pre supplies EDI=2 from5DD493. Original arithmetic, signed/unsigned branch decisions and conditional re-reads produce retry/timeout arguments; inputs include threshold equality, changed second reads, add/shift wrap and high-bit signed values.',
            'Guest post5DD9EF..9FD additionally sets CDEF timing through540D10 to retry60/maxFFFFFFFF/timeout600. That separate router connection is absent from the current direct-LAN owner and is an explicit residual; these controls stop before that call.',
            'Complete7B82E0/7B83E0 and4335E0/433770 execute for short,7-unit,high-bit,surrogate and embeddedNUL controls. Decode controls also cover missing marker bits and truncated nonterminated wide content; native final-unit clearing is retained.',
        ], substitutions=['Existing shell_relayout.Machine allocator and fail-closed imports; declared ResponseTime/queue/time/service observations and DebugLog sinks for GO controls. No packet bytes, hash output, version getter, arithmetic or field-store instruction is substituted.'],
        entry_points=dict(discovery=0x5DFB6A, query=0x5DFC47, presence=0x5DFD34,
                          advertisement=0x5DFE94, participant=0x5DFF6E, join=0x5DF6F5,
                          acceptance=0x5E1A8F, denial=0x5E157B, kick=0x5DB600, departure=0x5E09CD,
                          keepalive=0x5DB91D, output=0x5410F0, game_name_hash=0x5DAC00, roster=0x5DEAC7,
                          options=0x5E3765, go=0x5DD57B, ready=0x5DD916,
                          start_prompt=0x5E3630, options_format=0x5DBB60,
                          player_choices=0x5E3850, go_host_drain=0x5DD5E5,
                          go_guest_drain=0x5DD950,
                          host_pre_timing=0x5DD541, host_post_timing=0x5DD819,
                          guest_pre_timing=0x5DD8A0, guest_post_timing=0x5DD9AC,
                          response_time=0x5424C0, set_timing=0x540C60,
                          pack_wide=0x7B82E0, unpack_wide=0x7B83E0))


SOURCE_PATHS = {'packets': Path(__file__),
                'native_oracle': Path(__import__('tools.native_oracle', fromlist=['x']).__file__),
                'shared_machine': Path(__import__('tools.storage_oracle.shell_relayout', fromlist=['x']).__file__)}
