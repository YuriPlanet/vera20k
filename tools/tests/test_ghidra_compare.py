"""Disconnected regressions for portable Ghidra comparison tools: synthetic inputs plus one exported p-code fixture."""
import contextlib
import io
import json
from pathlib import Path
import socket
import tempfile
import unittest
from unittest.mock import patch

from tools.ghidra_compare import __main__ as cli
from tools.ghidra_compare import client, decompile, frames
from tools.tests.pe_fixture import pe_image

BASE = 0x401000
BODY = 'void fixture(void) { return; }'


class FakeClient:
    def __init__(self, side='before', responses=None):
        self.side, self.responses, self.calls = side, responses or {}, []

    def identity(self):
        return {'url': self.side, 'program': '/fixture'}

    def get(self, path, **args):
        self.calls.append((path, args))
        fallback = BODY if path == '/decompile_function' else 'No references found to address: 0x00401000'
        response = self.responses.get(path, fallback)
        if callable(response):
            response = response(**args)
        if isinstance(response, Exception):
            raise response
        return response


def image(code):
    return frames.Image(pe_image([(0x1000, 0x200, bytes.fromhex(code), 0x100, 0x60000020)]))


def row(code, entry=BASE):
    return dict(entry=f'{entry:08x}', name='fixture', proto='void fixture()',
                external=False, thunk=False, noreturn=False, purge=0,
                body=[f'{entry:08x}', f'{entry + len(bytes.fromhex(code)) - 1:08x}', 1],
                ranges=[[f'{entry:08x}', f'{entry + len(bytes.fromhex(code)) - 1:08x}']])


def var(space, offset, size=4):
    return dict(space=space, offset=f'{offset & 0xFFFFFFFF:x}', size=size)


def op(mnemonic, inputs, output=None, addr=BASE):
    return dict(mnemonic=mnemonic, seq={'address': f'{addr:08x}'}, inputs=inputs, output=output)


def stop(ops):
    return max([o['seq']['address'] for o in ops], default=f'{BASE:08x}')


def high(ops):
    """One decoded block ending at its last operation, as pcode_frame receives it."""
    return [(stop(ops), ops)]


def pcode(ops, **extra):
    block = {'stop': {'address': stop(ops)}, 'pcodes': ops}
    return json.dumps(dict(address=f'{BASE:08x}', basic_blocks=[block], **extra))


class Disconnected(unittest.TestCase):
    def setUp(self):
        self.no_network = patch.object(socket.socket, 'connect', side_effect=AssertionError('tests cannot connect'))
        self.no_network.start()
        self.addCleanup(self.no_network.stop)


class DecompileTests(Disconnected):
    def test_both_or_either_unreadable_pair_fails_completeness(self):
        for b, a in ((False, False), (False, True), (True, False)):
            with self.subTest(before=b, after=a):
                before = FakeClient(responses={'/decompile_function': BODY if b else 'Error: unavailable'})
                after = FakeClient('after', {'/decompile_function': BODY if a else client.ReadError('timeout')})
                result = decompile.compare(before, after, ['0x401000'])
                self.assertEqual(result['status'], 'incomplete')
                self.assertEqual(result['attempted'], 1)
                self.assertEqual(result['decompiled'], 0)
                self.assertEqual((result['readable_before'], result['readable_after']), (int(b), int(a)))
                self.assertEqual(len(result['read_errors']), 1)
                if b:
                    self.assertIn('error_after', result['worse'][0])

    def test_readable_pair_and_deduplicated_plan(self):
        result = decompile.compare(FakeClient(), FakeClient('after'), ['401000', '0x00401000'])
        self.assertEqual((result['status'], result['decompiled'], result['attempted']), ('ok', 1, 1))

    def test_truncated_json_error_and_nonfunction_bodies_are_rejected(self):
        for text in ('void f() { return;', '{"error":"broken"}', '{}', 'Error: failed { code }'):
            with self.subTest(text=text), self.assertRaises(client.ReadError):
                decompile.counts(text)

    def test_strings_and_plate_comments_do_not_invent_artifacts_or_calls(self):
        result = decompile.counts('/* unaff_fake call() */ void f() { puts("unaff_string fake() }"); }')
        self.assertEqual(result['unaff_'], 0)
        self.assertEqual(result['calls'], {'puts': 1})

    def test_warning_identity_is_detected_even_when_counts_match(self):
        before = decompile.counts('void f() {/* WARNING: old */ return;}')
        after = decompile.counts('void f() {/* WARNING: new */ return;}')
        self.assertEqual(decompile.worse_than(before, after)['new_warnings'], ['WARNING: new'])

    def test_expected_warning_is_reported_without_global_leakage(self):
        before = FakeClient()
        after = FakeClient('after', {'/decompile_function': 'void f() {/* WARNING: expected */ return;}'})
        expected = decompile.compare(before, after, ['401000'], expected={'WARNING: expected'})
        normal = decompile.compare(before, after, ['401000'])
        self.assertEqual(expected['status'], 'ok')
        self.assertEqual(expected['expected_counts']['after'], 1)
        self.assertEqual(normal['status'], 'findings')

    def test_artifact_identity_change_with_same_counts_is_worse(self):
        before = decompile.counts('void f() { unaff_A = 1; }')
        after = decompile.counts('void f() { unaff_B = 1; }')
        self.assertEqual(decompile.worse_than(before, after)['new_vars'], ['unaff_B'])

    def test_named_call_can_become_pointer_but_lost_total_calls_is_a_finding(self):
        before = decompile.counts('void f() { target(); }')
        pointer = decompile.counts('void f() { (**(code **)(p + 8))(1); }')
        self.assertEqual(decompile.lost_calls(before, pointer), {})
        self.assertEqual(decompile.lost_calls(pointer, decompile.counts(BODY)), {'(pointer)': 1})

    def test_goto_label_is_not_a_call(self):
        result = decompile.counts('void f() { LAB_00401000:\n (a = 1); if(a) target(); }')
        self.assertEqual(result['calls'], {'target': 1})

    def test_caller_failure_is_not_silently_a_smaller_success(self):
        before = FakeClient(responses={'/get_xrefs_to': '{"error":"unavailable"}'})
        result = decompile.compare(before, FakeClient('after'), ['401000'])
        self.assertEqual(result['status'], 'incomplete')
        self.assertEqual(result['decompiled'], 1)
        self.assertEqual(result['read_errors'][0]['phase'], 'callers')


class CallerTests(Disconnected):
    def test_pagination_visits_every_page(self):
        fake = FakeClient(responses={'/get_xrefs_to': lambda offset, **_: '\n'.join(
            f'From {0x410000 + i:x} in caller [UNCONDITIONAL_CALL]' for i in range(offset, min(offset + 500, 1167)))})
        self.assertEqual(len(client.xrefs(fake, '401000')), 1167)
        self.assertEqual([args['offset'] for _, args in fake.calls], [0, 499, 998])

    def test_repeat_page_and_partial_malformed_page_fail(self):
        for text in ('From 410000 in caller [UNCONDITIONAL_CALL]\nFrom 410001 in caller [UNCONDITIONAL_CALL]',
                     'From 410000 in caller [UNCONDITIONAL_CALL]\ntruncated'):
            with self.subTest(text=text), self.assertRaises(client.ReadError):
                client.xrefs(FakeClient(responses={'/get_xrefs_to': text}), '401000', page_size=2)

    def test_blank_xref_body_cannot_certify_zero_callers(self):
        for body in ('', ' \n\t'):
            with self.subTest(body=body), self.assertRaises(client.ReadError):
                client.xrefs(FakeClient(responses={'/get_xrefs_to': body}), '401000')

    def test_exact_page_multiple_finishes_with_anchor_not_ambiguous_empty_body(self):
        fake = FakeClient(responses={'/get_xrefs_to': lambda offset, limit, **_: '\n'.join(
            f'From {0x410000 + i:x} in caller [UNCONDITIONAL_CALL]' for i in range(offset, min(offset + limit, 500)))})
        self.assertEqual(len(client.xrefs(fake, '401000')), 500)
        self.assertEqual([args['offset'] for _, args in fake.calls], [0, 499])

    def test_shifted_page_anchor_marks_unstable_xrefs_incomplete(self):
        fake = FakeClient(responses={'/get_xrefs_to': lambda offset, limit, **_: '\n'.join(
            f'From {0x410000 + i:x} in caller [UNCONDITIONAL_CALL]'
            for i in range(offset + bool(offset), offset + bool(offset) + limit))})
        with self.assertRaisesRegex(client.ReadError, 'anchor changed'):
            client.xrefs(fake, '401000', page_size=2)

    def test_direct_tail_thunk_and_optional_computed_callers(self):
        refs = {'0x00401000': '\n'.join([
            'From 00402004 in caller [UNCONDITIONAL_CALL]',
            'From 00403000 in thunk [UNCONDITIONAL_JUMP]',
            'From 00404004 in virtual [COMPUTED_CALL]',
            'From 00407004 in tail [UNCONDITIONAL_JUMP]']),
            '0x00403000': 'From 00405004 in indirect_user [UNCONDITIONAL_CALL]'}
        fake = FakeClient(responses={
            '/get_xrefs_to': lambda address, **_: refs.get(address, 'No references found to address: ' + address),
            '/get_function_by_address': lambda address: f'Entry: {int(address, 16) & ~0xfff:08x}'})
        direct = client.callers(fake, ['0x00401000'])
        self.assertEqual(direct, ['0x00402000', '0x00403000', '0x00405000', '0x00407000'])
        self.assertIn('0x00404000', client.callers(fake, ['0x00401000'], computed=True))
        self.assertEqual(client.callers(fake, ['0x00401000'], include_tail=False), ['0x00402000', '0x00405000'])

    def test_missing_containing_function_cannot_drop_a_caller(self):
        for line in ('From 00402004 [UNCONDITIONAL_CALL]', 'From 00402004 in caller [UNCONDITIONAL_CALL]'):
            with self.subTest(line=line), self.assertRaises(client.ReadError):
                client.callers(FakeClient(responses={'/get_xrefs_to': line}), ['401000'])

    def test_sample_spreads_across_address_range(self):
        self.assertEqual(client.spread(list(range(473)), 3), [0, 157, 315])
        self.assertEqual(client.spread([1], 0), [])

    def test_program_is_required_and_mutating_endpoint_rejected(self):
        with self.assertRaises(ValueError):
            client.Client('http://localhost:8089', '')
        with self.assertRaises(ValueError):
            client.Client('http://localhost:8089', '/fixture').get('/rename_function')

    def test_get_includes_explicit_program_and_rejects_truncation(self):
        class Response(io.BytesIO):
            headers = {'Content-Length': '100'}
        class Opener:
            def open(self, url, **kwargs):
                self.url = url
                return Response(b'void fixture() {}')
        opener = Opener()
        with patch.object(client.urllib.request, 'build_opener', return_value=opener):
            with self.assertRaises(client.ReadError):
                client.Client('http://localhost:8089', '/saved/fixture', retries=1).get('/decompile_function', address='401000')
        self.assertIn('program=%2Fsaved%2Ffixture', opener.url)


class FrameTests(Disconnected):
    def test_native_stack_write_rejects_carried_source_as_destination(self):
        native = image('890424')  # mov [esp], eax
        source = var('stack', -12)
        wrong = op('COPY', [source], var('stack', -8))
        mapping, _ = frames.pcode_frame(high([wrong]), native)
        self.assertEqual(frames.agreement(mapping, {f'{BASE:08x}': [[-12, 4]]})['agree'], 0)
        wrong['output'] = var('stack', -12)
        self.assertEqual(frames.agreement(frames.pcode_frame(high([wrong]), native)[0],
                                         {f'{BASE:08x}': [[-12, 4]]})['agree'], 1)

    def test_missing_write_output_retains_failed_agreement(self):
        mapping, _ = frames.pcode_frame(high([op('COPY', [var('stack', -12)])]), image('890424'))
        self.assertEqual(mapping, {f'{BASE:08x}': []})
        self.assertEqual(len(frames.agreement(mapping, {f'{BASE:08x}': [[-12, 4]]})['wrong']), 1)

    def test_member_offset_is_part_of_effective_stack_address(self):
        ops = [op('PTRSUB', [var('register', 0x10), var('const', -28)], var('unique', 0x1000)),
               op('PTRADD', [var('unique', 0x1000), var('const', 1), var('const', 4)], var('unique', 0x1004))]
        self.assertEqual(frames.pcode_frame(high(ops), image('8d442408'))[0],
                         {f'{BASE:08x}': [[-24, None]]})
        ops[-1]['inputs'][-1] = var('const', 8)
        self.assertEqual(frames.agreement(frames.pcode_frame(high(ops), image('8d442408'))[0],
                                         {f'{BASE:08x}': [[-24, None]]})['agree'], 0)

    def test_unique_expression_is_not_carried_between_instructions(self):
        ops = [op('PTRSUB', [var('register', 0x10), var('const', -28)], var('unique', 0x1000)),
               op('PTRADD', [var('unique', 0x1000), var('const', 1), var('const', 4)], var('unique', 0x1004), BASE + 4)]
        self.assertNotIn(f'{BASE + 4:08x}', frames.pcode_frame(high(ops), image('8d44240890'))[0])

    def test_store_uses_connected_destination_and_ignores_source_pointer(self):
        ops = [op('PTRSUB', [var('register', 0x10), var('const', -12)], var('unique', 0x1000)),
               op('STORE', [var('const', 0), var('unique', 0x1000), var('stack', -8)])]
        self.assertEqual(frames.pcode_frame(high(ops), image('890424'))[0],
                         {f'{BASE:08x}': [[-12, None]]})

    def test_call_inputs_do_not_certify_stack_offsets(self):
        ops = [op('CALL', [var('stack', -12)])]
        self.assertEqual(frames.pcode_frame(high(ops), image('ffd0'))[0], {})

    def test_only_a_block_end_copy_into_a_phi_input_is_an_edge(self):
        # mov eax,[esp+4] ends its block; the next block merges a Stack+8 copy.
        own = op('COPY', [var('stack', 4)], var('register', 0))
        edge = op('COPY', [var('stack', 8)], var('unique', 0x10000001))
        phi = op('MULTIEQUAL', [var('unique', 0x10000001), var('register', 0x18)], var('register', 0x18), BASE + 4)
        native = image('8b44240490')
        blocks = [(f'{BASE:08x}', [own, edge]), (f'{BASE + 4:08x}', [phi])]
        self.assertEqual(frames.pcode_frame(blocks, native),
                         ({f'{BASE:08x}': [[4, 4]]}, {f'{BASE:08x}': [[8, 4]]}))
        other_reader = op('INT_EQUAL', [var('unique', 0x10000001), var('const', 0)], var('register', 0x206), BASE + 4)
        for name, case in (('not the block end', [(f'{BASE + 4:08x}', [own, edge, phi])]),
                           ('non-phi reader', blocks + [(f'{BASE + 4:08x}', [other_reader])])):
            with self.subTest(name):
                self.assertEqual(frames.pcode_frame(case, native)[0][f'{BASE:08x}'], [[4, 4], [8, 4]])

    def test_partial_pcode_error_empty_and_wrong_function_fail(self):
        good = op('RETURN', [])
        cases = [pcode([good], high_pcodes_error='truncated'), pcode([]), '{"high_pcodes": [',
                 json.dumps({'address': '00402000', 'high_pcodes': [good]}), '{"error":"failed"}']
        for response in cases:
            with self.subTest(response=response), self.assertRaises(client.ReadError):
                frames.read_frame(FakeClient(responses={'/get_function_pcode': response}), '0x00401000', image('c3'))

    def test_low_memory_export_preserves_native_stack_destination(self):
        good = op('COPY', [var('register', 0)], var('stack', -12), BASE + 6)
        payload = pcode([good])
        def response(**args):
            if args.get('granularity') != 'basic':
                raise client.ReadError('No HTTP reply after full-export heap failure')
            return payload
        result, _ = frames.read_frame(FakeClient(responses={'/get_function_pcode': response}),
                                      '0x00401000', image('5589e583ec0889042483c4085dc3'))
        self.assertEqual(frames.agreement(result, {f'{BASE + 6:08x}': [[-12, 4]]}),
                         {'agree': 1, 'wrong': []})

    def test_partial_basic_block_export_never_becomes_a_frame(self):
        good = op('RETURN', [])
        block = {'stop': {'address': f'{BASE:08x}'}, 'pcodes': [good]}
        cases = [
            dict(address=f'{BASE:08x}', basic_blocks=[block], basic_blocks_error='truncated'),
            dict(address=f'{BASE:08x}', basic_blocks=[block, {}]),
            dict(address=f'{BASE:08x}', basic_blocks=[]),
            dict(address=f'{BASE:08x}', basic_blocks=[dict(block, pcodes=[])]),
            dict(address=f'{BASE:08x}', high_pcodes=[good]),
        ]
        for payload in cases:
            with self.subTest(payload=payload), self.assertRaises(client.ReadError):
                frames.read_frame(FakeClient(responses={'/get_function_pcode': json.dumps(payload)}),
                                  '0x00401000', image('c3'))

    def test_stack_depths_solve_unknown_call_purge_from_return(self):
        code = '6a01ffd08b442404c3'  # push 1; call eax; mov eax,[esp+4]; ret
        owner = frames.NativeFrames([row(code)], image(code))
        mapping, result = owner.code('0x00401000')
        self.assertEqual(result['pops'], {BASE + 2: 4})
        self.assertEqual(mapping[f'{BASE + 4:08x}'], [[4, 4]])
        self.assertEqual(result['conflicts'], [])

    def test_native_frame_uses_entry_stack_offsets(self):
        code = '5589e583ec0889042483c4085dc3'
        mapping, result = frames.NativeFrames([row(code)], image(code)).code('0x00401000')
        self.assertEqual(mapping[f'{BASE + 6:08x}'], [[-12, 4]])
        self.assertEqual(result['conflicts'], [])

    def test_native_frame_includes_stack_operand_at_entry(self):
        # Original gamemd.exe 0x465380 begins with MOV EAX,[ESP+8].
        code = '8b442408c3'
        mapping, result = frames.NativeFrames([row(code)], image(code)).code('0x00401000')
        self.assertEqual(mapping[f'{BASE:08x}'], [[8, 4]])
        self.assertEqual(result['conflicts'], [])

    def test_conflicting_native_stack_equations_are_reported(self):
        code = '6a01c3'
        _, result = frames.NativeFrames([row(code)], image(code)).code('0x00401000')
        self.assertTrue(result['conflicts'])

    def test_branch_allocation_sizes_cannot_produce_a_clean_frame_report(self):
        # Both paths have the same ESP/EBP at their merge, but _chkstk consumes
        # different EAX sizes. Restoring ESP through EBP hides this at RET.
        code = '5589e585c97407b808000000eb05b810000000e838963c00890c2489ec5dc3'
        native = image(code)
        owner = frames.NativeFrames([row(code)], native)
        high = pcode([op('COPY', [var('register', 4)], var('stack', -12), BASE + 24)])
        before = FakeClient(responses={'/get_function_pcode': high})
        after = FakeClient('after', {'/get_function_pcode': high})
        with patch.object(native, 'is_probe', return_value=True):
            report = frames.compare_frames(before, after, ['0x00401000'], owner)
        self.assertEqual(report['status'], 'incomplete')
        self.assertTrue(report['analysis_limits'])

    def test_equal_or_overwritten_branch_sizes_remain_analyzable(self):
        original = '5589e585c97407b808000000eb05b810000000e838963c00890c2489ec5dc3'
        for code in (original.replace('b810000000', 'b808000000'),
                     original.replace('e838963c00', 'b808000000e833963c00')):
            with self.subTest(code=code):
                native = image(code)
                owner = frames.NativeFrames([row(code)], native)
                with patch.object(native, 'is_probe', return_value=True):
                    _, result = owner.code('0x00401000')
                self.assertFalse(result['notes'])
                self.assertFalse(result['conflicts'])

    def test_unsupported_pop_esp_and_split_instruction_are_not_analyzed_as_valid(self):
        code = '5cc3'
        _, result = frames.NativeFrames([row(code)], image(code)).code('0x00401000')
        self.assertTrue(result['notes'])
        code = '83ec08c3'
        partial = row('83ec')
        _, result = frames.NativeFrames([partial], image(code)).code('0x00401000')
        self.assertTrue(result['notes'])

    def test_bss_is_not_decoded_as_zero_filled_native_instructions(self):
        native = image('c3')
        self.assertEqual(native.instruction(BASE).mnemonic, 'ret')
        self.assertIsNone(native.instruction(BASE + 1))
        with self.assertRaises(client.ReadError):
            native.read(BASE + 1, 4)

    def test_duplicate_or_overlapping_census_is_rejected(self):
        valid = row('90c3')
        self.assertEqual(len(frames.census_rows(json.dumps(valid))), 1)
        for other in (valid, row('90c3', BASE + 1)):
            with self.subTest(other=other), self.assertRaises(client.ReadError):
                frames.census_rows(json.dumps(valid) + '\n' + json.dumps(other))

    def test_unreadable_frame_is_not_a_zero_wrong_success(self):
        code = '5589e583ec0889042483c4085dc3'
        owner = frames.NativeFrames([row(code)], image(code))
        bad = FakeClient(responses={'/get_function_pcode': '{"error":"timeout"}'})
        report = frames.compare_frames(bad, bad, ['0x00401000'], owner)
        self.assertEqual((report['status'], report['compared']), ('incomplete', 0))
        self.assertIn('before', report['read_errors'][0])
        self.assertIn('after', report['read_errors'][0])

    def test_frame_comparison_detects_wrong_destination_and_counts_coverage(self):
        code = '5589e583ec0889042483c4085dc3'
        owner = frames.NativeFrames([row(code)], image(code))
        before_ops = [op('COPY', [var('register', 0)], var('stack', -12), BASE + 6)]
        after_ops = [op('COPY', [var('register', 0)], var('stack', -8), BASE + 6)]
        before = FakeClient(responses={'/get_function_pcode': pcode(before_ops)})
        after = FakeClient('after', {'/get_function_pcode': pcode(after_ops)})
        report = frames.compare_frames(before, after, ['0x00401000'], owner)
        self.assertEqual(report['status'], 'findings')
        self.assertEqual(report['functions'][0]['right_to_wrong'], [f'{BASE + 6:08x}'])
        self.assertEqual(report['functions'][0]['native_accesses'], 1)
        self.assertEqual(report['functions'][0]['before']['agree'], 1)
        self.assertEqual(report['functions'][0]['unmapped_after'], [])

    def test_phi_edge_copy_is_reported_beside_the_native_read_it_follows(self):
        # Original 0x41028C MOV EBX,[ESP+0x10] reads Stack+4; Ghidra's block ends
        # there with the Stack+8 copy feeding the REPE CMPSD at 0x410290.
        fixture = json.loads((Path(frames.__file__).parent / 'fixtures' / 'query_interface_410260.json').read_text())
        code, entry = fixture['code'], int(fixture['address'], 16)
        native = frames.Image(pe_image([(entry - 0x400000, 0x200, bytes.fromhex(code), 0x100, 0x60000020)]))
        owner = frames.NativeFrames([row(code, entry)], native)
        export = json.dumps({k: fixture[k] for k in ('name', 'address', 'basic_blocks')})
        side = FakeClient(responses={'/get_function_pcode': export})
        report = frames.compare_frames(side, side, [f'0x{entry:08x}'], owner)
        function = report['functions'][0]
        self.assertEqual(report['status'], 'ok')
        self.assertEqual(function['before']['wrong'], [])
        self.assertIn('0041028c', function['unmapped_before'])
        self.assertEqual(function['edges'], [['0041028c', [[4, 4]], [[8, 4]], [[8, 4]]]])
        edge = next(o for b in fixture['basic_blocks'] for o in b['pcodes'] if o['seq']['address'] == '0041028c'
                    and o['inputs'][0]['space'] == 'stack')
        edge['inputs'][0]['offset'] = 'c'
        moved = FakeClient('after', {'/get_function_pcode': json.dumps(fixture)})
        report = frames.compare_frames(side, moved, [f'0x{entry:08x}'], owner)
        self.assertEqual(report['status'], 'findings')
        self.assertEqual(report['functions'][0]['edge_changed'], [['0041028c', [[8, 4]], [[12, 4]]]])


class CliTests(Disconnected):
    def test_import_and_help_need_no_retail_or_census_files(self):
        with contextlib.redirect_stdout(io.StringIO()), self.assertRaises(SystemExit) as result:
            cli.main(['--help'])
        self.assertEqual(result.exception.code, 0)

    def test_cli_preserves_receipt_and_reports_partial_read(self):
        with tempfile.TemporaryDirectory() as directory:
            plan = Path(directory) / 'plan.json'
            out = Path(directory) / 'out.json'
            plan.write_text(json.dumps([{'addr': '401000', 'action': 'thiscall'}]))
            args = ['decompile', '--before-url', 'http://localhost:8089', '--after-url', 'http://localhost:8090',
                    '--before-program', '/fixture', '--after-program', '/fixture', '--plan', str(plan), '--out', str(out)]
            def read(self, path, **_):
                return 'No references found to address: 0x00401000' if path == '/get_xrefs_to' else '{"error":"unavailable"}'
            with patch.object(client.Client, 'get', read), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(cli.main(args), 2)
            result = json.loads(out.read_text())
            self.assertEqual(result['status'], 'incomplete')
            self.assertEqual(len(result['inputs'][0]['sha256']), 64)
            original = out.read_bytes()
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(cli.main(args), 2)
            self.assertEqual(out.read_bytes(), original)

    def test_frame_cli_uses_checked_image_and_explicit_census(self):
        code = '5589e583ec0889042483c4085dc3'
        ops = [op('COPY', [var('register', 0)], var('stack', -12), BASE + 6)]
        with tempfile.TemporaryDirectory() as directory:
            census, out = Path(directory) / 'census.jsonl', Path(directory) / 'frames.json'
            census.write_text(json.dumps(row(code)))
            args = ['frames', '--before-url', 'http://localhost:8089', '--after-url', 'http://localhost:8090',
                    '--before-program', '/fixture', '--after-program', '/fixture', '--census', str(census),
                    '--address', '401000', '--out', str(out)]
            with patch.object(client.Client, 'get', return_value=pcode(ops)), \
                    patch.object(frames.native, 'image_bytes', return_value=image(code).data) as checked, \
                    contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(cli.main(args), 0)
            checked.assert_called_once_with()
            report = json.loads(out.read_text())
            self.assertEqual(report['compared'], 1)
            self.assertEqual(len(report['inputs']), 1)

    def test_changed_purge_includes_direct_callers_and_rejects_incomplete_reads(self):
        code = '5589e583ec0889042483c4085dc3'
        with tempfile.TemporaryDirectory() as directory:
            paths = [Path(directory) / name for name in ('before.jsonl', 'after.jsonl', 'frames.json')]
            original = row(code)
            changed = dict(original, purge=4)
            paths[0].write_text(json.dumps(original))
            paths[1].write_text(json.dumps(changed))
            args = ['frames', '--before-url', 'http://localhost:8089', '--after-url', 'http://localhost:8090',
                    '--before-program', '/fixture', '--after-program', '/fixture', '--census', str(paths[0]),
                    '--after-census', str(paths[1]), '--address', '401000', '--out', str(paths[2])]
            def read(self, path, **params):
                if path == '/get_xrefs_to':
                    return 'From 00402004 in caller [UNCONDITIONAL_CALL]'
                if path == '/get_function_by_address':
                    return 'Entry: 00402000'
                return '{"error":"unavailable"}'
            with patch.object(client.Client, 'get', read), \
                    patch.object(frames.native, 'image_bytes', return_value=image(code).data), \
                    contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(cli.main(args), 2)
            report = json.loads(paths[2].read_text())
            self.assertEqual(report['purge_changed'], ['0x00401000'])
            self.assertEqual(report['purge_callers'], ['0x00402000'])
            self.assertEqual(report['attempted'], 2)

    def test_empty_plan_fails_before_requests(self):
        with tempfile.TemporaryDirectory() as directory:
            plan, out = Path(directory) / 'plan.json', Path(directory) / 'out.json'
            plan.write_text('[]')
            args = ['decompile', '--before-url', 'http://localhost:8089', '--after-url', 'http://localhost:8090',
                    '--before-program', '/fixture', '--after-program', '/fixture', '--plan', str(plan), '--out', str(out)]
            with patch.object(client.Client, 'get') as get, contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(cli.main(args), 2)
            get.assert_not_called()
            self.assertFalse(out.exists())

    def test_frame_report_server_mismatch_fails_before_requests(self):
        with tempfile.TemporaryDirectory() as directory:
            census, previous, out = [Path(directory) / name for name in ('census', 'previous', 'out')]
            census.write_text(json.dumps(row('c3')))
            previous.write_text(json.dumps({'kind': 'decompile', 'before': {'url': 'wrong', 'program': '/fixture'}}))
            args = ['frames', '--before-url', 'http://localhost:8089', '--after-url', 'http://localhost:8090',
                    '--before-program', '/fixture', '--after-program', '/fixture', '--census', str(census),
                    '--comparison', str(previous), '--out', str(out)]
            with patch.object(client.Client, 'get') as get, contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(cli.main(args), 2)
            get.assert_not_called()
            self.assertFalse(out.exists())


if __name__ == '__main__':
    unittest.main()
