"""Original global-channel controls for the existing LAN oracle entry.

Only allocator/deallocator, wall-clock input, debug output and interface I/O are
substituted. Original constructor, framing, receive, queue and route bodies run.
No Python encoder or expected frame bytes. See generated .meta.json.
"""
from collections import Counter
import struct

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_ECX, UC_X86_REG_EIP,
    UC_X86_REG_ESP, UC_X86_REG_FPCW,
)

from tools.native_oracle import (
    NATIVE_FPCW, NativeCallTrace, OracleError, STACK_BASE, STACK_SIZE,
    provenance, run_checked,
)
from tools.storage_oracle.shell_relayout import Machine, HEAP, HEAP_SIZE

ENTRIES = {
    'global_ctor': 0x53FA90, 'ipx_ctor': 0x53F430, 'connection_ctor': 0x48BE60,
    'comm_ctor': 0x48B0E0, 'global_send': 0x53FBD0, 'global_receive': 0x53FCB0,
    'global_pop': 0x53FF10, 'consumed_cleanup': 0x5400D0, 'age_strip': 0x540110,
    'service': 0x48C3B0, 'send_service': 0x48C3E0, 'response': 0x48BA10,
    'send_route': 0x540050, 'direct_route': 0x5403F0, 'queue_send': 0x48B410,
    'queue_receive': 0x48B750, 'delete_receive': 0x48B890, 'failed_cleanup': 0x5402D0,
    'cancel_sends': 0x540340,
}


class GlobalMachine(Machine):
    def __init__(self, send_capacity=8, receive_capacity=8, product=0xAA04):
        super().__init__(0xBB, 800, 600)
        self.hits = Counter()
        self.sent = []
        self.inputs = []
        self.uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        self.w32(0xA8B238, 3)  # Explicit direct-LAN context; IPv4 is an adaptation.
        self.w32(0x828140, 2)  # Supplied address mode (not the event codec).
        self.w32(0xA8DBA0, 0)  # No packet router.
        self.set_time(1000)
        interface = self.alloc(16)
        vtable = self.alloc(0x80)
        self.w32(interface, vtable)
        self.w32(vtable + 0x0C, self.stub(self.capture_direct))
        self.w32(vtable + 0x10, self.stub(self.capture_broadcast))
        self.w32(0x887628, interface)
        self.uc.hook_add(UC_HOOK_CODE, self.observe)
        self.connection = self.alloc(0xDC)
        self.native(0x53FA90, self.connection,
                    (send_capacity, receive_capacity, 455, product, 0x1236))
        self.native(0x53F4E0, self.connection)
        self.constructed = self.snapshot()

    def alloc(self, size):
        address = super().alloc(size)
        if self.heap > HEAP + HEAP_SIZE:
            raise OracleError('scratch allocator exhausted')
        return address

    def observe(self, _uc, address, _size, _data):
        if address in ENTRIES.values():
            self.hits[f'{address:08X}'] += 1
        if address == 0x4068E0:
            self.ret(0)  # Debug sink; caller retains variadic cleanup.

    def set_time(self, bucket):
        self.now = bucket & 0xFFFFFFFF
        self.w32(0x887338, -1)
        self.w32(0x887340, self.now)

    def native(self, address, ecx=0, args=()):
        # Poison only the external scratch stack; each body must initialize its
        # own fields. Final direct ACK route normalization executes unchanged.
        self.uc.mem_write(STACK_BASE + STACK_SIZE - 0x4000, b'\xA5' * 0x4000)
        Machine.call(self, address, ecx=ecx, stack_args=args, count=2_000_000)
        if self.uc.reg_read(UC_X86_REG_ESP) != STACK_BASE + STACK_SIZE - 0x2000:
            raise OracleError(f'bad native stack cleanup at {address:08X}')
        return self.uc.reg_read(UC_X86_REG_EAX)

    def endpoint(self, last_octet, port=1234):
        address = self.alloc(12)
        parts = self.alloc(12)
        self.w32(parts, 0)
        self.uc.mem_write(parts + 4, bytes([192, 0, 2, last_octet, 0, 0]))
        self.native(0x53ECB0, address)
        self.native(0x53ED20, address, (parts, parts + 4, port))
        return bytes(self.uc.mem_read(address, 12))

    def capture_direct(self):
        packet, length, endpoint, address_len, alternate, direct, port = [self.arg(i) for i in range(7)]
        if address_len != 12 or length > 471:
            raise OracleError('unexpected native global I/O extent')
        self.sent.append(dict(kind='direct', bucket=self.now,
                              frame=bytes(self.uc.mem_read(packet, length)).hex(),
                              endpoint=bytes(self.uc.mem_read(endpoint, address_len)).hex(),
                              alternate=alternate & 255, direct=direct & 255, port=port & 0xFFFF))
        self.ret(28, 1)

    def capture_broadcast(self):
        packet, length, alternate = [self.arg(i) for i in range(3)]
        if length > 471:
            raise OracleError('unexpected native broadcast extent')
        self.sent.append(dict(kind='broadcast', bucket=self.now,
                              frame=bytes(self.uc.mem_read(packet, length)).hex(),
                              alternate=alternate & 255))
        self.ret(12, 1)

    def send(self, payload, endpoint=None, reliable=True, override=0, port=0):
        packet = self.alloc(max(1, len(payload)))
        self.uc.mem_write(packet, payload)
        destination = 0
        if endpoint is not None:
            destination = self.alloc(12)
            self.uc.mem_write(destination, endpoint)
        result = self.native(0x53FBD0, self.connection,
                             (packet, len(payload), destination, int(reliable), port, override))
        return dict(returned=result, state=self.snapshot())

    def service(self, bucket):
        self.set_time(bucket)
        before = len(self.sent)
        returned = self.native(0x48C3B0, self.connection)
        return dict(returned=returned, emitted=self.sent[before:], state=self.snapshot())

    def receive(self, frame, source, port=0):
        raw = bytes.fromhex(frame) if isinstance(frame, str) else frame
        packet = self.alloc(len(raw))
        endpoint = self.alloc(12)
        self.uc.mem_write(packet, raw)
        self.uc.mem_write(endpoint, source)
        before = len(self.sent)
        returned = self.native(0x53FCB0, self.connection, (packet, len(raw), endpoint, port))
        return dict(returned=returned, emitted=self.sent[before:], state=self.snapshot())

    def pop(self):
        output = self.alloc(512)
        length = self.alloc(4)
        endpoint = self.alloc(12)
        product = self.alloc(4)
        self.uc.mem_write(output, b'\xA5' * 512)
        returned = self.native(0x53FF10, self.connection, (output, length, endpoint, product))
        result = dict(returned=returned, state=self.snapshot())
        if returned:
            size = self.r32(length)
            if size > 455:
                raise OracleError('global pop exceeded native payload extent')
            result.update(payload=bytes(self.uc.mem_read(output, size)).hex(),
                          endpoint=bytes(self.uc.mem_read(endpoint, 12)).hex(),
                          product=struct.unpack('<H', self.uc.mem_read(product, 2))[0])
        return result

    def cleanup(self):
        returned = self.native(0x5400D0, self.connection)
        return dict(returned=returned, state=self.snapshot())

    def strip(self, bucket, maximum_age):
        self.set_time(bucket)
        self.native(0x540110, self.connection, (maximum_age,))
        return self.snapshot()

    def cancel(self, endpoint, port):
        address = self.alloc(12)
        self.uc.mem_write(address, endpoint)
        returned = self.native(0x540340, self.connection, (address, port))
        return dict(returned=returned, state=self.snapshot())

    def snapshot(self):
        c = self.connection
        comm = self.r32(c + 4)
        sends = []
        for index in range(self.r32(comm + 0x28)):
            slot = self.r32(self.r32(comm + 0x30) + index * 4)
            entry = self.r32(comm + 0x24) + slot * 0x24
            length, packet = self.r32(entry + 0x10), self.r32(entry + 0x14)
            sends.append(dict(flags=self.r32(entry), first=self.r32(entry + 4),
                              last=self.r32(entry + 8), attempts=self.r32(entry + 12),
                              frame=bytes(self.uc.mem_read(packet, length)).hex(),
                              destination=bytes(self.uc.mem_read(self.r32(entry + 0x1C),
                                                                 self.r32(entry + 0x18))).hex(),
                              port=struct.unpack('<H', self.uc.mem_read(entry + 0x20, 2))[0]))
        receives = []
        for index in range(self.r32(comm + 0x38)):
            slot = self.r32(self.r32(comm + 0x40) + index * 4)
            entry = self.r32(comm + 0x34) + slot * 0x18
            length, packet = self.r32(entry + 8), self.r32(entry + 12)
            receives.append(dict(flags=self.r32(entry), arrival=self.r32(entry + 4),
                                 frame=bytes(self.uc.mem_read(packet, length)).hex(),
                                 source=bytes(self.uc.mem_read(self.r32(entry + 0x14),
                                                              self.r32(entry + 0x10))).hex()))
        ring = []
        for index in range(self.r32(c + 0xD4)):
            ring.append(dict(sequence=self.r32(self.r32(c + 0xD0) + index * 4),
                             endpoint=bytes(self.uc.mem_read(self.r32(c + 0xCC) + index * 12, 12)).hex()))
        return dict(send_count=len(sends), receive_count=len(receives),
                    total_send=self.r32(comm + 0x2C), total_receive=self.r32(comm + 0x3C),
                    response=[self.r32(comm + offset) for offset in (0x14, 0x18, 0x1C, 0x20)],
                    ring_cursor=self.r32(c + 0xD8), ring=ring, sends=sends, receives=receives)


MANAGER = 0xA8E9C0
STATS = (0x14, 0x18, 0x1C, 0x20)
MANAGER_ENTRIES = {
    'prepare_begin': 0x52E9DF, 'prepare_end': 0x52EA54,
    'guest_begin': 0x5DD9AC, 'guest_end': 0x5DDA02,
    'initial_reset_begin': 0x647B2B, 'initial_reset_end': 0x647B35,
    'manager_set_timing': 0x540C60,
    'global_set_timing': 0x540D10, 'global_mean': 0x5424C0,
    'private_response_maximum': 0x542450,
    'mean': 0x48BA80, 'reset_all': 0x542EC0,
    'reset_stats': 0x48BAA0, 'record_response': 0x48BA10,
}


class ManagerTimingMachine(GlobalMachine):
    def __init__(self):
        super().__init__(send_capacity=4, receive_capacity=4)
        self.channels = {'global': self.connection}
        for role in ('private0', 'private1', 'alternate', 'extra'):
            connection = self.alloc(0xDC)
            if role == 'alternate':
                self.native(0x53FA90, connection, (4, 4, 455, 0xAA04, 0xCDEF))
                self.native(0x53F4E0, connection)
            else:
                # Original common connection/CommBuffer constructors; endpoint
                # and protocol-specific subclass fields are outside these calls.
                self.native(0x48BE60, connection,
                            (4, 4, 558, 0xAA04, 11, 0xFFFFFFFF, 999, 12))
                self.native(0x48BF10, connection)
            self.channels[role] = connection
        self.uc.mem_write(MANAGER, bytes(0xB0))
        self.w32(MANAGER, 0x7EC16C)  # Original manager vtable, not a hook.
        self.uc.mem_write(MANAGER + 4, b'\1\1')
        self.w32(MANAGER + 0x28, self.channels['private0'])
        self.w32(MANAGER + 0x2C, self.channels['private1'])
        self.w32(MANAGER + 0x44, 2)
        self.w32(MANAGER + 0x48, self.channels['global'])
        self.w32(MANAGER + 0x4C, self.channels['alternate'])
        self.w32(MANAGER + 0x50, self.channels['extra'])
        self.w32(MANAGER + 0x58, 111)
        self.w32(MANAGER + 0x5C, 222)
        self.w32(MANAGER + 0x60, 333)
        self.comm = {role: self.r32(c + 4) for role, c in self.channels.items()}
        for index, (role, comm) in enumerate(self.comm.items()):
            payload = bytes([0x20 + index, 0x70, 0x80, 0xFF, index])
            data = self.alloc(len(payload))
            auxiliary = self.alloc(12)
            self.uc.mem_write(data, payload)
            self.uc.mem_write(auxiliary, bytes([index + 1]) * 12)
            if self.native(0x48B410, comm, (data, len(payload), auxiliary, 12, 1234)) != 1:
                raise OracleError('native send fixture was not admitted')
            if self.native(0x48B750, comm, (data, len(payload), auxiliary, 12)) != 1:
                raise OracleError('native receive fixture was not admitted')
        self.response_schedule = None
        self.getters = []
        self.leaf_reads = []
        self.leaf_roles = []
        self.expected_response_roles = None
        self.calls = []
        self.trace = NativeCallTrace(self.uc, self.r32, self.calls)
        self.capture = False
        self.allowed = []
        self.uc.hook_add(UC_HOOK_CODE, self.observe_timing)
        self.uc.hook_add(UC_HOOK_MEM_WRITE, self.observe_write)

    def observe_timing(self, uc, address, _size, _data):
        if not self.capture:
            return
        sp = uc.reg_read(UC_X86_REG_ESP)
        self.trace.returned(address, sp)
        names = {
            0x540C60: ('manager_set_timing', 4, 16),
            0x540D10: ('global_set_timing', 3, 12),
            0x542EC0: ('manager_reset', 1, 4),
            0x48BAA0: ('response_reset', 1, 4),
        }
        if address in names:
            self.trace.entered(address, sp, names[address])
        if address == 0x5424C0:
            if uc.reg_read(UC_X86_REG_ECX) != MANAGER:
                raise OracleError('wrong manager response receiver')
            if self.response_schedule is not None:
                if len(self.getters) >= len(self.response_schedule):
                    raise OracleError('unexpected extra native response read')
                self.w32(self.comm['global'] + 0x1C,
                         self.response_schedule[len(self.getters)])
            self.getters.append(dict(value=self.r32(self.comm['global'] + 0x1C),
                                     caller=hex(self.r32(sp))))
        if address == 0x48BA80:
            receiver = uc.reg_read(UC_X86_REG_ECX)
            role = next((role for role, comm in self.comm.items() if comm == receiver), None)
            expected = ('global',) if self.expected_response_roles is None else self.expected_response_roles
            if role not in expected:
                raise OracleError('wrong response leaf receiver')
            self.leaf_roles.append(role)
            self.leaf_reads.append(self.r32(receiver + 0x1C))

    def observe_write(self, _uc, _access, address, size, _value, _data):
        if not self.capture or STACK_BASE <= address < STACK_BASE + STACK_SIZE:
            return
        if not any(start <= address and address + size <= end
                   for start, end in self.allowed):
            raise OracleError(f'unexpected native write at {address:#x}+{size}')

    def timing_state(self):
        return dict(default=list(struct.unpack('<3I', self.uc.mem_read(MANAGER + 0x58, 12))),
                    channels={role: list(struct.unpack('<3I', self.uc.mem_read(c + 0x28, 12)))
                              for role, c in self.channels.items()})

    def response_state(self):
        return {role: [self.r32(comm + offset) for offset in STATS]
                for role, comm in self.comm.items()}

    def check_queues(self):
        for role, comm in self.comm.items():
            if [self.r32(comm + x) for x in (0x28, 0x38, 0x2C, 0x3C)] != [1] * 4:
                raise OracleError(f'queue counters changed: {role}')

    def run_slice(self, begin, ends, *, allowed, required=(), eax=0, ebp=0,
                  response_schedule=None, direct_args=None, response_roles=None):
        self.allowed = allowed
        self.response_schedule = response_schedule
        self.expected_response_roles = response_roles
        old_heap = bytes(self.uc.mem_read(HEAP, self.heap - HEAP))
        old_manager = bytes(self.uc.mem_read(MANAGER, 0xB0))
        self.uc.reg_write(UC_X86_REG_EAX, eax)
        self.uc.reg_write(UC_X86_REG_EBP, ebp)
        self.capture = True
        try:
            if direct_args is None:
                sp = STACK_BASE + STACK_SIZE - 0x2000
                self.uc.mem_write(sp - 0x1000, b'\xA5' * 0x2000)
                self.uc.reg_write(UC_X86_REG_ESP, sp)
                reached = run_checked(self.uc, begin, ends, count=50_000,
                                      timeout_us=1_000_000, required_addresses=required)
                self.trace.returned(reached, self.uc.reg_read(UC_X86_REG_ESP))
                if self.uc.reg_read(UC_X86_REG_ESP) != sp:
                    raise OracleError('caller slice did not balance stack')
            else:
                ecx, args = direct_args
                self.native(begin, ecx, args)
                reached = self.uc.reg_read(UC_X86_REG_EIP)
                self.trace.returned(reached, self.uc.reg_read(UC_X86_REG_ESP))
        finally:
            self.capture = False
        if self.trace.pending:
            raise OracleError(f'call trace did not return: {self.trace.pending}')
        if response_schedule is not None and len(self.getters) != len(response_schedule):
            raise OracleError('not all supplied successive response inputs were read')
        if response_roles is not None and self.leaf_roles != list(response_roles):
            raise OracleError('unexpected response maximum leaf order')
        if response_roles is None and len(self.getters) != len(self.leaf_reads):
            raise OracleError('original response getter leaf was bypassed')
        exclusions = list(allowed)
        if response_schedule is not None:
            exclusions.append((self.comm['global'] + 0x1C, self.comm['global'] + 0x20))
        for base, old in ((HEAP, old_heap), (MANAGER, old_manager)):
            after = bytes(self.uc.mem_read(base, len(old)))
            for index, (a, b) in enumerate(zip(old, after)):
                address = base + index
                if a != b and not any(start <= address < end for start, end in exclusions):
                    raise OracleError(f'unrelated state changed at {address:#x}')
        self.check_queues()


def timing_case(name, role, responses):
    m = ManagerTimingMachine()
    begin, end = (0x52E9DF, 0x52EA54) if role == 'prepare' else (0x5DD9AC, 0x5DDA02)
    allowed = [(MANAGER + 0x58, MANAGER + 0x64)]
    allowed += [(c + 0x28, c + 0x34) for c in m.channels.values()]
    m.run_slice(begin, end, allowed=allowed,
                required=(0x5424C0, 0x48BA80, 0x540C60, 0x540D10),
                eax=3, ebp=0xFFFFFFFF, response_schedule=responses)
    after = m.timing_state()
    return dict(name=name, role=role, input_responses=responses,
                private=after['default'], global_timing=after['channels']['global'])


def reset_case(name, kind, samples):
    m = ManagerTimingMachine()
    for index, comm in enumerate(m.comm.values()):
        for sample in samples[index % len(samples)]:
            m.native(0x48BA10, comm, (sample,))
    before = m.response_state()
    allowed = [(comm + 0x14, comm + 0x24) for comm in m.comm.values()]
    if kind == 'initial':
        m.run_slice(0x647B2B, 0x647B35, allowed=allowed, ebp=MANAGER,
                    required=(0x542EC0, 0x48BAA0))
    else:
        m.run_slice(0x542EC0, (), allowed=allowed,
                    direct_args=(MANAGER, (0,)))
    def consumed(state):
        return {role: state[role][:3] for role in ('global', 'private0', 'private1')}
    return dict(name=name, input_samples=samples[:3], clear=kind == 'initial',
                before=consumed(before), after=consumed(m.response_state()))


def response_maximum_case(name, private):
    m = ManagerTimingMachine()
    m.w32(MANAGER + 0x44, len(private))
    m.w32(MANAGER + 0x50, 0)  # Extra channel has no ordinary-LAN owner.
    for index, mean in enumerate(private):
        m.native(0x48BA10, m.comm[f'private{index}'], (mean,))
    primary = 0xFFFFFFFF
    m.native(0x48BA10, m.comm['global'], (primary,))
    m.native(0x48BA10, m.comm['alternate'], (0xFFFFFFFE,))
    m.run_slice(0x542450, (), allowed=[], direct_args=(MANAGER, ()),
                response_roles=tuple(f'private{i}' for i in range(len(private))))
    return dict(name=name, private=private, primary=primary,
                result=m.uc.reg_read(UC_X86_REG_EAX))


def manager_timing_controls():
    # Exact boundary read counts come from the native branch, not a Python
    # implementation of the selected timing result.
    stable = [(0, 2), (59, 4), (0xFFFFFFFE, 2)]
    timing = [timing_case(f'prepare_mean_{mean:08x}', 'prepare', [mean] * count)
              for mean, count in stable]
    timing += [
        timing_case('prepare_private_equal_floor_second_wrap', 'prepare', [13, 0xFFFFFFFE, 0]),
        timing_case('prepare_global_equal_floor_second_wrap', 'prepare', [0, 58, 0xFFFFFFFE]),
        timing_case('prepare_two_independent_second_reads', 'prepare', [14, 1, 59, 2]),
    ]
    timing += [timing_case(f'guest_mean_{mean:08x}', 'guest', [mean])
               for mean in (0, 0x10000000, 0xFFFFFFFF)]
    samples = [[9, 17, 3], [0], [13], [0xFFFFFFFE], [0xFFFFFFFF]]
    reset = [
        reset_case('initial_all_channels_clear', 'initial', samples),
        reset_case('direct_false_high_private', 'direct',
                   [[9, 17, 3], [0xFFFFFFFE], [0xFFFFFFFF], [0], [13]]),
        reset_case('direct_false_empty_history', 'direct', [[], [], [], [], []]),
    ]
    response = [response_maximum_case(name, private) for name, private in (
        ('empty_private', []), ('one_private', [13]),
        ('unsigned_high_private', [0x7FFFFFFF, 0x80000000]),
        ('maximum_private', [0xFFFFFFFF, 1]),
    )]
    return dict(timing=timing, reset=reset, response_maximum=response)



def generate():
    machines = []
    def machine(**kwargs):
        m = GlobalMachine(**kwargs)
        machines.append(m)
        return m

    sender = machine()
    source_a, source_b, source_c = [sender.endpoint(i) for i in (11, 22, 33)]
    sender.send(b'global-A', source_b)
    emitted = sender.service(1000)['emitted']
    if len(emitted) != 1:
        raise OracleError('initial native send did not emit one frame')
    frame = emitted[0]['frame']
    basic = dict(native_frame=emitted[0], constructor=sender.constructed)
    receiver = machine(receive_capacity=2)
    basic['unknown_a'] = receiver.receive(frame, source_a)
    basic['distinct_b_same_sequence'] = receiver.receive(frame, source_b)
    basic['duplicate_a_when_full'] = receiver.receive(frame, source_a)
    basic['new_c_when_full'] = receiver.receive(frame, source_c)
    basic['pop_a'] = receiver.pop()
    basic['pop_again_before_service'] = receiver.pop()
    basic['cleanup_a'] = receiver.cleanup()
    basic['duplicate_a_after_cleanup'] = receiver.receive(frame, source_a)
    basic['pop_b'] = receiver.pop()
    basic['cleanup_b'] = receiver.cleanup()
    basic['duplicate_a_with_empty_queue'] = receiver.receive(frame, source_a)

    basic['c_retried_after_capacity_recovered'] = receiver.receive(frame, source_c)

    wrong_ack = dict(sender_before=sender.snapshot())
    ack = basic['unknown_a']['emitted'][0]['frame']
    wrong_ack['ack_frame'] = ack
    wrong_ack['received_from_unrelated_c'] = sender.receive(ack, source_c)
    wrong_ack['after_service'] = sender.service(1004)

    product_sender = machine(product=0x1357)
    product_sender.send(b'foreign-product', source_b)
    product_frame = product_sender.service(1000)['emitted'][0]['frame']
    product_receiver = machine()
    product = dict(native_frame=product_frame,
                   receive=product_receiver.receive(product_frame, source_a))
    product['pop'] = product_receiver.pop()

    age_receiver = machine(receive_capacity=4)
    age = dict(first_a=age_receiver.receive(frame, source_a),
               second_b=age_receiver.receive(frame, source_b))
    age['consume_a'] = age_receiver.pop()
    age['cleanup_a'] = age_receiver.cleanup()
    age['exact_age_retained'] = age_receiver.strip(1002, 2)
    age['over_age_b_removed'] = age_receiver.strip(1003, 2)
    age['b_duplicate_after_its_queue_entry_aged'] = age_receiver.receive(frame, source_b)
    age['a_reaccepted_after_first_matching_sequence_invalidated'] = age_receiver.receive(frame, source_a)

    sequence_sender = machine(send_capacity=3)
    sequence = []
    for reliable, override in ((False, 0), (True, 0), (True, 77), (True, 0)):
        sequence.append(dict(reliable=reliable, override=override,
                             send=sequence_sender.send(bytes([len(sequence) + 1]), source_b,
                                                       reliable=reliable, override=override)))
    sequence_frames = sequence_sender.service(1000)
    retry = dict(equal_delta=sequence_sender.service(1002),
                 greater_delta=sequence_sender.service(1003),
                 timeout=sequence_sender.service(1064))
    retry['failed_cleanup_returned'] = sequence_sender.native(0x5402D0, sequence_sender.connection)
    retry['after_failed_cleanup'] = sequence_sender.snapshot()

    normalization_receiver = machine()
    normalization = normalization_receiver.receive(frame, source_a, port=4567)
    normalization['pop'] = normalization_receiver.pop()
    broadcast_sender = machine()
    broadcast_sender.send(b'global-broadcast', reliable=True)
    broadcast = broadcast_sender.service(1000)

    cancellation_sender = machine()
    cancellation_sender.send(b'completed', source_b, port=0)
    cancellation_sender.send(b'same-host-other-endpoint-port',
                             cancellation_sender.endpoint(22, port=4444), port=0)
    cancellation_sender.send(b'other-override', source_b, port=5555)
    cancellation = dict(sent=cancellation_sender.service(1000))
    cancellation['acknowledged'] = cancellation_sender.receive(ack, source_c)
    cancellation['cancelled'] = cancellation_sender.cancel(source_b, 0)
    cancellation['serviced'] = cancellation_sender.service(1004)

    age_boundaries = []
    for arrived, now, maximum_age in [(0x7FFFFFFE, 0x7FFFFFFF, 2),
                                      (0x7FFFFFFE, 0x80000000, 2),
                                      (0xFFFFFFFE, 0, 3), (0xFFFFFFFE, 2, 3)]:
        boundary_receiver = machine()
        boundary_receiver.set_time(arrived)
        admitted = boundary_receiver.receive(frame, source_a)
        after = boundary_receiver.strip(now, maximum_age)
        age_boundaries.append(dict(arrived=arrived, now=now, maximum_age=maximum_age,
                                   admitted=admitted, after=after))
    retail_constructor = machine(send_capacity=160, receive_capacity=320)
    payload_extent = []
    for payload in (b'', bytes(range(256)) + bytes(range(199))):
        extent_sender = machine()
        extent_sender.send(payload, source_b)
        extent_frame = extent_sender.service(1000)['emitted'][0]
        extent_receiver = machine()
        payload_extent.append(dict(input=payload.hex(), native_frame=extent_frame,
                                   receive=extent_receiver.receive(extent_frame['frame'], source_a),
                                   pop=extent_receiver.pop()))

    required = set(ENTRIES.values())
    combined = Counter()
    for m in machines:
        combined.update(m.hits)
    missing = [f'{pc:08X}' for pc in required if not combined[f'{pc:08X}']]
    if missing:
        raise OracleError(f'original entry coverage missing: {missing}')
    return dict(source='unicorn/gamemd.exe', endpoints=dict(a=source_a.hex(), b=source_b.hex(), c=source_c.hex()),
                basic=basic, wrong_source_ack=wrong_ack, foreign_product=product,
                age_strip=age, global_sequence=dict(admission=sequence, service=sequence_frames),
                retry_failure=retry, source_port_normalization=normalization,
                null_destination_broadcast=broadcast, retail_constructor=retail_constructor.constructed,
                cancellation=cancellation, age_boundaries=age_boundaries,
                payload_extents=payload_extent,
                original_entries=dict(combined))


def metadata():
    result = provenance(
        scope='Original global ConnectionClass constructor/send/receive/dequeue/cleanup/age-strip and shared send service, with original direct route normalization',
        assumptions=[
            'Explicit boundary constructor capacities8/2/3/4 plus retail-sized send160/receive320 and payload455, discriminator1236, productAA04 (or foreign1357). No Windows process or real sockets.',
            'GameMode3 with supplied address mode2 is the explicit VERA IPv4-LAN adaptation; retail main-menu LAN constructs IPX. Three synthetic IPv4 sources192.0.2.11/.22/.33 use port1234; original53ECB0/53ED20 build their endpoint storage.',
            'External clock globals887338=-1/887340 expose declared16ms bucket inputs. Stack is poisonedA5 before each top-level call. Each call must reach its real return with exact callee stack cleanup.',
            'Every transmitted frame and ACK originates in original53FBD0/53FCB0; frame bytes are never encoded or edited by Python. Delivery changes only declared external source metadata. Observations retain native queue bytes, flags, counters and endpoint/sequence history.',
            'Cancellation controls keep an ACKed entry, remove an uncompleted send with matching host and override despite a different endpoint port, and retain a differing override. Age controls include signed-boundary and unsigned-addition-wrap inputs to the original540110 comparison.',
            'The shared shell_relayout.Machine maps retail through native_oracle.load_image and provides its allocator/import fail-closed hooks. Its unused dialog state is not evidence of shell or network integration.',
        ],
        substitutions=[
            'Existing Machine operator-new/delete storage. Debug4068E0 is a sink. The minimal supplied interface vtable captures final+0C addressed output and+10 broadcast output; original540050/5403F0 route bodies execute.',
            'Declared wall-clock globals and address-mode/GameMode inputs only. No global/private constructor, CommBuffer, send-service, response, framing, receive, delivery or cleanup body is substituted.',
        ], entry_points=ENTRIES)
    return result



def manager_metadata():
    return provenance(
        scope='Original LAN handoff timing, manager response-history reset and private response maximum',
        assumptions=[
            'The existing GlobalMachine and shell_relayout.Machine load retail PE bytes through native_oracle.load_image. No Windows process, socket or whole-session comparison is claimed.',
            'Supplied manager storage uses original7EC16C vtable, two private common connections, primary1236 and alternateCDEF globals, and one extra common connection. Original constructors and48B410/48B750 admit occupied send/receive queues; complete heap/manager comparison rejects writes outside declared timing/stat fields.',
            'LAN prepare52E9DF..52EA54 enters with EAX3/EBP-1; guest5DD9AC..5DDA02 executes signed private timing plus primary-global override. Raw CommBuffer+1C observations change at declared5424C0 entries; both native getters run, including conditional second reads. Nonconstant schedules are branch controls, not claims of ordinary concurrent writes.',
            'Original48BA10 seeds response history;647B2B..647B35 executes the successful-wait reset call after the peer-field clear, and direct542EC0 controls cover preserved high private means and empty histories. Only Rust-consumed sum/count/mean outputs are retained; the native peak has no current Rust consumer. Mode4 periodic scheduling and alternate/extra-channel lifecycle remain outside the Rust integration.',
            'Full542450 controls seed original48BA10 private histories, with primaryFFFFFFFF and alternateFFFFFFFE histories deliberately larger than ordinary private values. Manager+50 is absent. Exact private leaf-call order and a no-write heap/manager guard cover empty and unsigned-high inputs; no expected result is calculated in Python. Extra-channel participation remains outside the direct-LAN Rust owner.',
        ],
        substitutions=[
            'Existing shared allocator/deallocator and fail-closed Windows import adapters. Debug4068E0 is an output sink. Declared manager/channel configuration and caller registers are supplied; original constructor, getter, setter and reset bodies execute.',
            'Successive raw response means and initial sample sequences are controlled inputs. No timing arithmetic, response result, socket I/O or gameplay body is substituted. Stack writes are allowed and exact native caller stack balance is required.',
        ],
        entry_points={**{key: ENTRIES[key] for key in (
            'global_ctor', 'connection_ctor', 'comm_ctor', 'queue_send', 'queue_receive',
        )}, **MANAGER_ENTRIES})
