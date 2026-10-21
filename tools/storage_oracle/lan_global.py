"""Original global-channel controls for the existing LAN oracle entry.

Only allocator/deallocator, wall-clock input, debug output and interface I/O are
substituted. Original constructor, framing, receive, queue and route bodies run.
No Python encoder or expected frame bytes. See generated .meta.json.
"""
from collections import Counter
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_FPCW

from tools.native_oracle import NATIVE_FPCW, OracleError, STACK_BASE, STACK_SIZE, finish_vectors, provenance
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

