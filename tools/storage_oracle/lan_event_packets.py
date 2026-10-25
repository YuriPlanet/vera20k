"""Bounded Event packet controls for the existing LAN oracle entry.

Run with ``python -m tools.storage_oracle.lan_lobby --event-packets --write``.
Executes original constructors, 649CA0's header writer, 64B660/64B4D0 and
64BDD0. The stop at 649D94 is before transport: no packets leave this VM.
"""
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX,
    UC_X86_REG_EIP, UC_X86_REG_ESP,
)

from tools.native_oracle import (
    OracleError, RET_MAGIC, SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE,
    load_image, provenance, run_checked,
)

RECORD_LEN = 111
PACKET = SCRATCH + 0x1000


def compact(raw, fill):
    """Lossless fixed-length records; Rust restores the declared trailing byte."""
    return bytes(raw).rstrip(bytes([fill])).hex()


class PacketMachine:
    def __init__(self):
        self.uc = uc = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(uc)
        uc.mem_map(STACK_BASE, STACK_SIZE)
        uc.mem_map(SCRATCH, SCRATCH_SIZE)
        uc.mem_map(RET_MAGIC, 0x1000)
        self.w32(0xA8B24C, 2)  # Active UDP protocol's compression branch.
        self.w32(0xA8B554, 5)
        self.w32(0xA8ED84, 1)
        self.w32(0xA83D4C, SCRATCH + 0x100)
        self.w32(SCRATCH + 0x130, 2)
        self.w32(0xAC51FC, 0x12345678)
        self.w32(0x7E1530, SCRATCH + 0x200)
        uc.hook_add(UC_HOOK_CODE, self.hook)

    def w32(self, address, value):
        self.uc.mem_write(address, struct.pack('<I', value & 0xFFFFFFFF))

    def r32(self, address):
        return struct.unpack('<I', self.uc.mem_read(address, 4))[0]

    def hook(self, _uc, address, _size, _data):
        if address not in (0x4068E0, SCRATCH + 0x200):
            return
        sp = self.uc.reg_read(UC_X86_REG_ESP)
        self.uc.reg_write(UC_X86_REG_EIP, self.r32(sp))
        self.uc.reg_write(UC_X86_REG_ESP, sp + 4)
        self.uc.reg_write(UC_X86_REG_EAX, 12345 if address == SCRATCH + 0x200 else 0)

    def setup_call(self, ecx, edx, args):
        sp = STACK_BASE + STACK_SIZE - 0x1000 - 4 * (len(args) + 1)
        self.uc.mem_write(sp, struct.pack('<' + 'I' * (len(args) + 1),
                                         RET_MAGIC, *(x & 0xFFFFFFFF for x in args)))
        self.uc.reg_write(UC_X86_REG_ESP, sp)
        self.uc.reg_write(UC_X86_REG_ECX, ecx)
        self.uc.reg_write(UC_X86_REG_EDX, edx)
        return sp

    def call(self, entry, ecx=0, edx=0, args=()):
        sp = self.setup_call(ecx, edx, args)
        run_checked(self.uc, entry, RET_MAGIC, count=500_000, timeout_us=1_000_000)
        if self.uc.reg_read(UC_X86_REG_ESP) != sp + 4 * (len(args) + 1):
            raise OracleError(f'wrong native return stack at {entry:#x}')
        return self.uc.reg_read(UC_X86_REG_EAX)

    def record(self, kind, actor=100, destination=50087, auxiliary=0):
        address = SCRATCH + 0x500
        self.uc.mem_write(address, bytes([0xA5]) * RECORD_LEN)
        self.uc.mem_write(address + 1, b'\xf3')
        if kind == 'move':
            self.call(0x4C6860, address, args=(9, actor, 0x34, 2, 0, 0,
                                             destination, 0x0B, auxiliary, 0))
        elif kind == 'stop':
            self.call(0x4C65E0, address, args=(9, 6, actor, 0x34))
        else:
            raise OracleError(f'unknown constructor control {kind}')
        return bytes(self.uc.mem_read(address, RECORD_LEN))

    def fixed_record(self, opcode):
        # Declared structural payload controls, not gameplay receiver inputs.
        # The original no-payload constructor establishes the event header.
        address = SCRATCH + 0x500
        self.uc.mem_write(address, bytes([0xA5]) * RECORD_LEN)
        self.uc.mem_write(address + 1, b'\xf3')
        self.call(0x4C66C0, address, args=(9, opcode))
        self.uc.mem_write(address + 7, bytes(range(104)))
        return bytes(self.uc.mem_read(address, RECORD_LEN))

    def send(self, records, frame=15, capacity=558, do_count=0):
        self.w32(0xA8ED84, frame)
        self.w32(0xA802C8, len(records))
        self.w32(0xA802CC, 0)
        self.w32(0xA802D0, len(records) & 127)
        self.w32(0x8B41F8, do_count)
        self.w32(0x8B4200, 0)
        for index, record in enumerate(records):
            self.uc.mem_write(0xA802D4 + RECORD_LEN * index, record)
            self.w32(0xA83A54 + 4 * index, 9000 + index)
        self.uc.mem_write(PACKET, bytes(1024))
        self.setup_call(0, PACKET, (capacity, 10, 7))
        # Execute the production header writer and its signed mirror allowance.
        run_checked(self.uc, 0x649CA0, 0x64B660, count=10_000, timeout_us=1_000_000)
        sp = self.uc.reg_read(UC_X86_REG_ESP)
        written_pointer = self.r32(sp + 16)
        allowance = self.r32(sp + 12)
        run_checked(self.uc, 0x64B660, 0x649D94, count=500_000, timeout_us=1_000_000)
        length = self.uc.reg_read(UC_X86_REG_EAX)
        count = self.r32(written_pointer)
        mirrors = [bytes(self.uc.mem_read(0x8B4204 + RECORD_LEN * i, RECORD_LEN))
                   for i in range(count)]
        return dict(frame=frame, capacity=capacity, do_count=do_count,
                    allowance=allowance, inputs=[compact(r, 0xA5) for r in records],
                    packet=bytes(self.uc.mem_read(PACKET, length)).hex(), count=count,
                    remaining=self.r32(0xA802C8), mirrors=[compact(r, 0xA5) for r in mirrors],
                    source_headers=[bytes(self.uc.mem_read(0xA802D4 + RECORD_LEN * i, 7)).hex()
                                    for i in range(len(records))],
                    timestamps=[self.r32(0xA70204 + 4 * i) for i in range(count)])

    def decode(self, packet, initial_count=0):
        raw = bytes.fromhex(packet)
        self.uc.mem_write(PACKET, raw)
        self.w32(0x8B41F8, initial_count)
        self.w32(0x8B4200, 0)
        returned = self.call(0x64BDD0, PACKET, len(raw))
        count = self.r32(0x8B41F8) - initial_count
        records = [compact(self.uc.mem_read(0x8B4204 + RECORD_LEN * i, RECORD_LEN), 0)
                   for i in range(count)]
        cache = []
        for slot in range(8):
            index = 2 * 8 + slot
            actors = self.r32(0xAC50FC + index * 4)
            if actors > 128:
                raise OracleError(f'invalid native cache count {actors}')
            cache.append([bytes(self.uc.mem_read(0xAFA468 + index * 0x280 + i * 5, 5)).hex()
                          for i in range(actors)])
        return dict(initial_count=initial_count, returned=returned, records=records, cache=cache)

    def probe(self, frame, lookahead, sequence):
        self.w32(0xA8ED84, frame)
        self.w32(0xA8B550, lookahead)
        self.w32(0xA8E7A8, 0x0BADC0DE)
        self.w32(SCRATCH + 0x700, SCRATCH + 0x710)
        self.setup_call(SCRATCH + 0x700, sequence, ())
        # No transport substitution: stop immediately before the virtual call.
        run_checked(self.uc, 0x64A000, 0x64A090, count=10_000, timeout_us=1_000_000)
        pointer = self.r32(self.uc.reg_read(UC_X86_REG_ESP))
        return dict(frame=frame, lookahead=lookahead, sequence=sequence, seed=0x0BADC0DE,
                    packet=bytes(self.uc.mem_read(pointer, 14)).hex())


def generate():
    flows = []

    def flow(name, batches):
        sender, receiver = PacketMachine(), PacketMachine()
        steps = []
        for specs, settings in batches:
            records = [sender.record(**spec) for spec in specs]
            sent = sender.send(records, **settings)
            sent['decoded'] = receiver.decode(sent['packet'])
            steps.append(sent)
        flows.append(dict(name=name, steps=steps))

    move = lambda actor, **args: dict(kind='move', actor=actor, **args)
    pair = [move(100), move(101)]
    flow('single_move', [([move(100)], {})])
    flow('stop', [([dict(kind='stop')], {})])
    flow('capacity_first', [([move(100)], dict(capacity=38))])
    flow('capacity_second', [(pair, dict(capacity=43))])
    flow('do_list_half_full', [(pair, dict(do_count=0x2000))])
    flow('do_list_over_half', [(pair, dict(do_count=0x2001))])
    flow('native_558_burst', [([move(i) for i in range(100, 228)], {})])
    flow('cache_and_early_hit', [(pair, {}), (pair, dict(frame=20)),
                               ([move(100, destination=50088), move(101, destination=50088),
                                 move(102, destination=50088)], dict(frame=25))])
    flow('auxiliary_not_group_key', [([move(100, auxiliary=12), move(101, auxiliary=34)], {})])
    flow('different_destination', [([move(100), move(101, destination=50088)], {})])
    # Fill all eight assignment slots, then exercise oldest replacement and a hit.
    flow('cache_replacement', [([move(100 + i * 2), move(101 + i * 2)], dict(frame=15 + 5 * i))
                              for i in range(9)] + [(pair, dict(frame=60))])
    # The entire fixed table passes through native copy logic, including1B's
    # unusual Event+0D payload origin. The values are transport-only controls.
    sender, receiver = PacketMachine(), PacketMachine()
    fixed = [sender.fixed_record(op) for op in range(47) if op not in (4, 0x19, 0x1C, 0x1F)]
    sent = sender.send(fixed, capacity=4096)
    sent['decoded'] = receiver.decode(sent['packet'])
    flows.append(dict(name='all_fixed_payloads', steps=[sent]))
    source = next(f['steps'][0]['packet'] for f in flows if f['name'] == 'cache_and_early_hit')
    failures = [dict(name=f'full_{count}', packet=source,
                     decoded=PacketMachine().decode(source, count)) for count in (0x4000, 0x3FFF, 0x3FFE)]
    single = flows[0]['steps'][0]['packet']
    failures += [dict(name=f'truncated_{length}', packet=single[:length * 2],
                      decoded=PacketMachine().decode(single[:length * 2])) for length in (13, 14, 38, 39)]
    return dict(source='unicorn/gamemd.exe', input_fill=0xA5, decoded_fill=0,
                payload_lengths=list(PacketMachine().uc.mem_read(0x8208EC, 47)),
                flows=flows, receives=failures,
                syncs=[PacketMachine().probe(frame, ahead, sequence)
                       for frame, ahead, sequence in ((0, 10, 0), (1, 10, 1), (15, 5, 0x12345))])


def metadata():
    return provenance(
        scope='Original Event constructors4C6860/4C65E0, send header649CA0..649D94, probe64A000..64A090, Compress64B660/cache64B4D0 and Uncompress64BDD0; bounded raw packet and admission controls',
        assumptions=[
            'Move/Stop input records execute their whole native constructors with House9, object kind34, Move mission2, null target, Cell kind0B, explicit cell50087/50088 and auxiliary IDs. Constructor storage startsA5 with flagsF3, preserving untouched bytes; the first record issue frame is1 and later constructor calls see the prior send frame.',
            'Protocol2, send rate5, local House2, checksum12345678, ahead10 and prior low16 frontier7 are supplied upstream inputs. Original649CA0 computes header and signed min(OutListCount,2000h-DoListCount), then full64B660 executes. Stop at649D94 before any transport call. Packet capacity558 is the original Session constructor69768C value; 38/43 are boundary controls.',
            'Sender/receiver VMs persist separately per flow, preserving all native caches. OutList/DoList indexes start0, source timestamps9000+index, receiver capacity4000h. Successful and failing native calls retain their observed prefix and cache changes.',
            'Fixed-payload table controls execute original no-payload constructor4C66C0 then set payload bytes00..67 explicitly; these establish copying offsets, not semantic validity of gameplay receiver parameters. Full-admission controls record retained native cache counts/tokens without retrying after the fatal condition.',
            'FRAMESYNC controls execute64A000 through64A090 before its transport call, with explicit frame, MaxAhead, sequence and Scenario seed0BADC0DE. Native computes all14 header bytes; protocol2 and send-rate5 are retained.',
            'Records are stored losslessly as hex with trailing input/mirrorA5 or decoded00 omitted; consumers restore111 bytes. This is a bounded codec comparison, not execution of gameplay receivers or UDP transport.',
        ],
        substitutions=[
            'DebugLog4068E0 returns without output; timeGetTime import7E1530 returns12345. Original constructors, header arithmetic, compression/cache bodies, decode/admission and local mirror copies execute unchanged.',
            'Upstream session fields and already-admitted source records are placed directly in native storage. No encoded bytes, lengths, cache decisions, receiver events or native header values are calculated by Python.',
        ],
        entry_points=dict(move=0x4C6860, stop=0x4C65E0, header=0x4C66C0, send=0x649CA0,
                          probe=0x64A000, probe_stop=0x64A090,
                          send_stop=0x649D94, compress=0x64B660,
                          cache=0x64B4D0, decompress=0x64BDD0))
