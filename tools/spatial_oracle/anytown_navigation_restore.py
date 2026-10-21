"""Original 581F50 post-load hierarchy rebuild on physical Anytown bridge states."""
from pathlib import Path
import gc
import gzip
import hashlib
import json
import struct

from tools.native_oracle import _canonical, first_difference, provenance, finish_vectors
from tools.spatial_oracle.shrapnel_repair import packet_io
import tools.spatial_oracle.anytown_damage.navigation as nav_owner
from tools.spatial_oracle.anytown_damage.navigation import Navigation, MAP, sr
from tools.spatial_oracle.anytown_damage.navigation_inputs import (
    Inputs, extract_tiles, identity,
)
from unicorn.x86_const import (UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EDI,
                               UC_X86_REG_ESI, UC_X86_REG_EBP)

HERE = Path(__file__).resolve().parent
FROZEN = Path(nav_owner.__file__).with_name('navigation.json.gz')
SOURCE_EVIDENCE = HERE / 'anytown_navigation_restore.native_bytes.json'
FROZEN_SHA256 = 'a2197179f0818a7b2c231c8956601b2269df164db04e05d856b99db380a15d35'
FROZEN_PAYLOAD_SHA256 = '584e7fbd46e04bfff30b58d3c0c463df80fbcca8263aa15a3868de327842d14e'
STAGES = ('first_damage', 'collapse', 'repair')


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def normalized(value):
    return json.loads(_canonical(value))


class AtBoundary(Exception):
    """End the existing owner driver after its selected complete primitive call."""


class RestoreProbe(Navigation):
    def __init__(self, readers, theater, tiles, stop_after, *,
                 primitive_entries=(0x57CCF0, 0x573540), **navigation_inputs):
        self.capture_enabled = False
        self.stop_after = stop_after
        self.primitive_entries = primitive_entries
        self.primitive_calls = []
        self.rebuild_trace = []
        self.stream_callback = None
        self.stream_calls = []
        self.stream_payloads = []
        self.stream_cursor = 0
        self.stream_stop = None
        super().__init__(readers, theater, tiles, **navigation_inputs)
        self.capture_enabled = True

    def call(self, address, *args, **kwargs):
        result = super().call(address, *args, **kwargs)
        if self.capture_enabled and address in self.primitive_entries:
            self.primitive_calls.append(dict(entry=address, returned_eax=result,
                                            returned_low_byte=result & 255))
            if len(self.primitive_calls) == self.stop_after:
                raise AtBoundary
        return result

    def observe(self, u, address, size, data):
        if address == self.stream_callback:
            sp = u.reg_read(UC_X86_REG_ESP)
            receiver, buffer, length, written = struct.unpack('<4I', u.mem_read(sp + 4, 16))
            assert receiver == self.stream_receiver and written == 0
            if self.activity == 'retained_navigation_save':
                payload = bytes(u.mem_read(buffer, length))
                self.stream_payloads.append(payload)
            else:
                assert self.activity == 'retained_navigation_load'
                payload = self.stream_payloads[self.stream_cursor]
                assert len(payload) == length
                u.mem_write(buffer, payload)
                self.stream_cursor += 1
            self.stream_calls.append(dict(caller=f'{sr.u32(u, sp):08x}',
                                          length=length, sha256=sha(payload)))
            self.ret(16, 0)  # Explicit host IStream transport seam, HRESULT S_OK.
            return
        if address == self.stream_stop:
            self.ret()  # End the selected original block before the next saved field.
            return
        if self.activity == 'restore_rebuild' and address in (
                0x581F50, 0x588D60, 0x581F90, 0x42C1C0):
            receiver = u.reg_read(UC_X86_REG_ECX)
            row = dict(entry=address, receiver=receiver,
                       caller=sr.u32(u, u.reg_read(UC_X86_REG_ESP)))
            if address == 0x581F90:
                row['level'] = sr.u32(u, u.reg_read(UC_X86_REG_ESP) + 4)
            if address == 0x588D60:
                assert receiver in (MAP + 0x8C, MAP + 0xA4, MAP + 0xBC)
                row['level'] = (receiver - MAP - 0x8C) // 24
                row['record_count_before'] = sr.u32(u, receiver + 16)
            self.rebuild_trace.append(row)
        return super().observe(u, address, size, data)

    def retained_navigation_roundtrip(self):
        """Execute original Save727..767 and Load355..3A7 on retained map bytes.

        IStream is a byte transport seam; native instructions own pointer,
        count, row iteration, allocation, transfer order and failure gates.
        Whole Mouse raw-object persistence/Cell loading is not emulated here.
        """
        from tools.spatial_oracle.map_queries import dwords
        u = self.uc
        before = self.state()
        base = sr.u32(u, MAP + 0x68)
        count = sr.u32(u, MAP + 0x6C)
        labels = sr.u32(u, MAP + 0x4C)
        plane = bytes(u.mem_read(base, count * 4))
        rows = [bytes(u.mem_read(sr.u32(u, MAP + 0x18 + i * 4), labels * 2))
                for i in range(13)]
        # Allocation domain beyond the exported rectangle is explicit. The
        # Rust owner currently represents those native cached-class7 slots by
        # the padding sentinel rather than a second terrain rectangle.
        padding = [plane[(y * self.side + x) * 4:(y * self.side + x + 1) * 4]
                   for y in range(self.side) for x in range(self.side)
                   if x >= self.width or y >= self.width]
        callback = self.allocate(16)
        table = self.allocate(20)
        stream = self.allocate(4)
        u.mem_write(callback, b'\xc3')
        u.mem_write(table, dwords(0, 0, 0, callback, callback))
        u.mem_write(stream, dwords(table))
        self.stream_callback = callback
        self.stream_receiver = stream
        self.stream_payloads = []
        self.stream_calls = []
        self.stream_stop = 0x5BE769
        self.activity = 'retained_navigation_save'
        u.reg_write(UC_X86_REG_EDI, MAP)
        u.reg_write(UC_X86_REG_ESI, stream)
        self.call(0x5BE727)
        saved = list(self.stream_calls)
        assert self.stream_payloads == [plane, *rows]
        assert [r['length'] for r in saved] == [count * 4, *([labels * 2] * 13)]
        assert before == self.state()
        # Clear the receiver bytes so equality cannot pass without the reads.
        u.mem_write(base, b'\xa5' * (count * 4))
        for i in range(13):
            u.mem_write(sr.u32(u, MAP + 0x18 + i * 4), b'\xa5' * (labels * 2))
        self.stream_calls = []
        self.stream_cursor = 0
        self.stream_stop = 0x5BE3A9
        self.activity = 'retained_navigation_load'
        u.reg_write(UC_X86_REG_EBP, MAP)
        self.call(0x5BE355, args=(*([0] * 16), stream))
        loaded = list(self.stream_calls)
        assert self.stream_cursor == 14
        assert before == self.state()
        assert bytes(u.mem_read(base, count * 4)) == plane
        assert [bytes(u.mem_read(sr.u32(u, MAP + 0x18 + i * 4), labels * 2))
                for i in range(13)] == rows
        self.stream_callback = None
        self.stream_stop = None
        assert sha(bytes(u.mem_read(0x401000, 0x3E0000))) == self.code_hash
        return dict(save_block='005be727..005be769', load_block='005be355..005be3a9',
                    save=saved, load=loaded, base_record_count=count,
                    movement_label_count=labels,
                    padding_record_count=len(padding),
                    padding_nondefault_count=sum(p != b'\x07\0\0\0' for p in padding),
                    state_equal=True, rng_equal=True, cells_equal=True,
                    text_unchanged=True,
                    instruction_sha256=dict(save=sha(bytes(u.mem_read(0x5BE727, 0x42))),
                                            load=sha(bytes(u.mem_read(0x5BE355, 0x54)))))


def generate(*, frozen_path=FROZEN, frozen_sha256=FROZEN_SHA256,
             frozen_payload_sha256=FROZEN_PAYLOAD_SHA256, stages=STAGES,
             input_factory=None, retained_base=False):
    frozen_bytes = frozen_path.read_bytes()
    assert sha(frozen_bytes) == frozen_sha256
    frozen = json.loads(gzip.decompress(frozen_bytes))
    assert sha(_canonical(frozen)) == frozen_payload_sha256
    if input_factory is None:
        theater = identity.theater()
        readers = Inputs(theater)
        tiles, assets = extract_tiles(theater)
        machine_factory = lambda stop_after: RestoreProbe(readers, theater, tiles, stop_after)
    else:
        readers, assets, machine_factory = input_factory()
    cases = []
    for index, stage in enumerate(stages):
        print('Preparing independent native boundary:', stage, flush=True)
        machine = machine_factory(index + 1)
        assert not first_difference(frozen['initial'], normalized(machine.initial))
        try:
            machine.run()  # Exact frozen owner driver; no copied primitive dispatch loop.
        except AtBoundary:
            pass
        else:
            raise AssertionError('Selected original primitive boundary was not reached')
        machine.capture_enabled = False
        assert len(machine.primitive_calls) == index + 1
        frozen_stage = frozen['stages'][index]
        assert machine.primitive_calls[-1]['returned_low_byte'] == frozen_stage['returned_low_byte']
        assert not first_difference(frozen_stage['trace'], normalized(machine.trace))
        before = machine.state()
        assert not first_difference(frozen_stage['state'], normalized(before)), first_difference(
            frozen_stage['state'], normalized(before))
        assert not machine.pending and not machine.range_pending
        retained_roundtrip = machine.retained_navigation_roundtrip() if retained_base else None
        machine.activity = 'restore_rebuild'
        machine.trace.clear()
        machine.writes.clear()
        counters_before = machine.counters.copy()
        frame_before = sr.i32(machine.uc, 0xA8ED84)
        returned_eax = machine.call(0x581F50, count=240000000)
        frame_after = sr.i32(machine.uc, 0xA8ED84)
        after = machine.state()
        unchanged = {key: before[key] == after[key] for key in (
            'rng', 'navigation', 'cells', 'movement_admissions')}
        assert all(unchanged.values()), unchanged
        assert frame_before == frame_after
        assert [row['level'] for row in machine.rebuild_trace
                if row['entry'] == 0x581F90] == [2, 1, 0]
        assert [row['level'] for row in machine.rebuild_trace
                if row['entry'] == 0x588D60] == [2, 1, 0]
        assert sum(row['entry'] == 0x42C1C0 for row in machine.rebuild_trace) == 1
        assert sha(bytes(machine.uc.mem_read(0x401000, 0x3E0000))) == machine.code_hash
        cases.append(dict(stage=stage, before=before, after=after,
                          frozen_boundary_equal=True,
                          primitive_calls=machine.primitive_calls,
                          rebuild_trace=machine.rebuild_trace,
                          native_counters_delta=dict(machine.counters - counters_before),
                          returned_eax=returned_eax, return_contract='void; EAX recorded mechanically',
                          frame_before=frame_before, frame_after=frame_after,
                          unchanged=unchanged))
        if retained_base:
            cases[-1]['retained_navigation_roundtrip'] = retained_roundtrip
        print(stage, 'rebuild records', [len(g['records']) for g in before['graphs']],
              '->', [len(g['records']) for g in after['graphs']], flush=True)
        del machine
        gc.collect()
    return dict(schema=1, frozen_navigation_file=frozen_path.name,
                frozen_navigation_sha256=frozen_sha256,
                frozen_navigation_payload_sha256=frozen_payload_sha256,
                source_evidence_sha256=sha(SOURCE_EVIDENCE.read_bytes()),
                native_inputs=readers.snapshot(), assets=assets,
                native_size=frozen['case']['size'], cases=cases)



def generate_retained_base():
    data = generate(retained_base=True)
    baseline_path = HERE / 'anytown_navigation_restore.json.gz'
    baseline = packet_io.read_result(baseline_path)
    projected = normalized(packet_io.publication_projection(data))
    # These named fields include all native Cell/base/graph/edge facts,
    # concrete entry answers, three RNGs, frame, traces and native counters.
    # Current archive resolution provenance is preserved separately below;
    # it has legitimately expanded since this frozen numerical reference.
    compared_fields = ('schema', 'frozen_navigation_file', 'frozen_navigation_sha256',
                       'frozen_navigation_payload_sha256', 'source_evidence_sha256',
                       'native_inputs', 'native_size')
    for key in compared_fields:
        assert not first_difference(baseline[key], projected[key]), key
    case_fields = ('stage', 'before', 'after', 'frozen_boundary_equal', 'primitive_calls',
                   'rebuild_trace', 'native_counters_delta', 'return_contract',
                   'frame_before', 'frame_after', 'unchanged')
    assert len(baseline['cases']) == len(projected['cases'])
    receipts = []
    for expected, actual in zip(baseline['cases'], projected['cases'], strict=True):
        for key in case_fields:
            if difference := first_difference(expected[key], actual[key]):
                raise AssertionError(f"Original {actual['stage']} numerical field {key} changed: {difference}")
        receipts.append(dict(stage=actual['stage'],
                             retained_navigation_roundtrip=actual['retained_navigation_roundtrip'],
                             before_graph_record_counts=[len(g['records']) for g in actual['before']['graphs']],
                             after_graph_record_counts=[len(g['records']) for g in actual['after']['graphs']],
                             void_eax=dict(frozen=expected['returned_eax'],
                                           after_retained_load=actual['returned_eax'],
                                           contract='void; extra allocation can change incidental EAX, never a gameplay return')))
    return dict(schema=1, native_size=data['native_size'],
                frozen_rebuild_file=baseline_path.name,
                frozen_rebuild_sha256=sha(baseline_path.read_bytes()),
                frozen_rebuild_payload_sha256=sha(_canonical(baseline)),
                compared_top_level_fields=compared_fields,
                compared_case_fields=case_fields,
                original_rebuild_numerical_equal=True,
                assets=data['assets'], cases=receipts)


def metadata():
    return provenance(
        scope=__doc__,
        assumptions=[
            'A fresh original full-map Navigation machine is created independently for each boundary. Its initial state and selected original damage/collapse/repair state and trace must equal the frozen Navigation packet exactly before rebuild.',
            'The existing Navigation.run driver executes preparations; the observer stops only after the chosen complete native57CCF0 or573540 returns. No bridge or graph algorithm is reproduced in this driver.',
            'Original LoadContent67E730 calls581F50 at67E8CD after MouseLoad and object restoration. This witness executes that complete581F50 wrapper on the frozen live source facts; it does not emulate SaveGame, MouseLoad or pointer swizzling.',
            'Original Mouse Save5BE727..767 writes retained Map+68 four-byte records and13 raw movement rows; Load5BE355..3A7 reads the emitted bytes after their receiver storage is overwritten. Native pointers/counts/iteration/allocation/order execute, with explicit host IStream byte transport. Raw Mouse object, full Cell load and pointer swizzling remain outside this block witness. The hierarchy record vectors, ID planes and adjacency buckets are rebuilt.',
            'The selected stock span has no structural bridge/Tube records. Full post-load bridge record/dummy reconstruction and actor path invalidation are outside this bounded witness.',
        ],
        substitutions=[
            'Reuse all physical reader, cell/TMP/Terrain preparation, bounded allocation/free, display, shroud and waterfall animation seams declared by frozen Navigation. No new native gameplay seam is introduced during581F50.',
            'The successful LoadContent callsite is instruction-established and byte-checked against the loaded binary. This is a hierarchy-rebuild execution comparison, not a full native save/load run.',
        ],
        entry_points={'load_content': 0x67E730, 'load_hierarchy_call': 0x67E8CD,
                      'retained_save_block': 0x5BE727, 'retained_load_block': 0x5BE355,
                      'rebuild_all': 0x581F50, 'clear_vector': 0x588D60,
                      'build_level': 0x581F90, 'refresh_scratch': 0x42C1C0})


def publish(data=generate, argv=None):
    """Reuse the compressed publication owner and the frozen payload guard."""
    packet_io.finish_vectors(data, HERE / 'anytown_navigation_restore.json.gz',
                             provenance=metadata, argv=argv,
                             promotion_path=HERE / 'anytown_navigation_restore.promotion.json')


def compare_restored_prefix(prefix, *, native_path=None,
                            suffixes=('damaged', 'collapsed', 'repaired')):
    """Compare complete post-load graphs and base source facts for all three states."""
    native_path = native_path or HERE / 'anytown_navigation_restore.json.gz'
    native = packet_io.read_result(native_path)
    rows = []
    for case, suffix in zip(native['cases'], suffixes, strict=True):
        path = Path(str(prefix) + '.restored_' + suffix + '.json')
        exported = json.loads(path.read_bytes())
        actual = exported.get('navigation', exported)
        expected = case['after']
        checks = {}
        for key, value in expected['navigation'].items():
            found = actual.get(key, actual.get('rust', {}).get(key))
            difference = first_difference(value, found)
            checks['navigation.' + key] = dict(equal=difference is None,
                                               first_difference=difference)
        for level, graph in enumerate(expected['graphs']):
            for key, value in graph.items():
                difference = first_difference(value, actual['graphs'][level][key])
                checks[f'graphs.{level}.{key}'] = dict(equal=difference is None,
                                                      first_difference=difference)
        difference = first_difference(native['native_size'], actual['native_size'])
        checks['native_size'] = dict(equal=difference is None, first_difference=difference)
        rows.append(dict(stage=case['stage'], production_file=path.name,
                         production_sha256=sha(path.read_bytes()), checks=checks,
                         native_graph_record_counts=[len(g['records']) for g in expected['graphs']],
                         production_graph_record_counts=[len(g['records']) for g in actual['graphs']],
                         all_compared_equal=all(row['equal'] for row in checks.values())))
    count = 'three' if len(suffixes) == 3 else str(len(suffixes))
    return dict(schema=1, native_file=native_path.name,
                native_payload_sha256=sha(_canonical(native)),
                scope=f'Complete class/height/base-ID planes,13 movement rows,base zone count,native map size and all ordered hierarchy IDs/padding/records/edges for {count} restored bridge states. No full native save/load or actor-world comparison.',
                states=rows, all_compared_equal=all(row['all_compared_equal'] for row in rows))


def main():
    import argparse
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--retained-base', action='store_true')
    parser.add_argument('--compare-prefix', type=Path)
    parser.add_argument('--comparison-output', type=Path)
    args, remaining = parser.parse_known_args()
    if args.retained_base:
        if args.compare_prefix is not None or args.comparison_output is not None:
            parser.error('--retained-base cannot be combined with comparison arguments')
        finish_vectors(generate_retained_base, HERE / 'anytown_navigation_restore.retained.json',
                       provenance=metadata, argv=remaining)
        return
    if args.compare_prefix is None:
        if args.comparison_output is not None:
            parser.error('--comparison-output requires --compare-prefix')
        publish(argv=remaining)
        return
    if remaining:
        parser.error('Unexpected comparison arguments: ' + ' '.join(remaining))
    result = compare_restored_prefix(args.compare_prefix)
    text = json.dumps(result, indent=2) + '\n'
    if args.comparison_output is not None:
        args.comparison_output.write_text(text)
    else:
        print(text, end='')
    assert result['all_compared_equal'], 'Native restored navigation comparison differs'


if __name__ == '__main__':
    main()
