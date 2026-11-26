"""Compare high p-code stack locations with the original x86 instruction roles."""
from functools import lru_cache
import json
import struct

from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from capstone import x86_const as X

from tools import native_oracle as native
from .client import ReadError, address
from .walk import Walker, census_func

SKIP = {'CALL', 'CALLIND', 'INDIRECT', 'MULTIEQUAL'}
ESP = '10'  # x86 SLEIGH register offset, not a machine address.


class Image:
    """Original file-backed bytes only; production passes native.image_bytes().

    Explicit byte injection permits synthetic offline fixtures. This class does
    not certify a binary identity; the CLI always obtains bytes from the shared
    native-oracle identity checker, and records that identity in its report.
    """
    def __init__(self, data):
        self.data = data
        self.decoder = Cs(CS_ARCH_X86, CS_MODE_32)
        self.decoder.detail = True

    def read(self, addr, size):
        try:
            return native.file_span(self.data, addr, size)[1]
        except native.OracleError as error:
            raise ReadError(str(error)) from error

    def u32(self, addr):
        return struct.unpack('<I', self.read(addr, 4))[0]

    @lru_cache(maxsize=65536)
    def instruction(self, addr):
        # A valid last instruction need not have 15 bytes remaining in its
        # section. Shrink the read; never synthesize BSS or cross a section.
        for size in range(15, 0, -1):
            try:
                data = self.read(addr, size)
            except ReadError:
                continue
            return next(self.decoder.disasm(data, addr, count=1), None)
        return None

    @lru_cache(maxsize=64)
    def is_probe(self, entry):
        # Original 0x7CA650 saves/restores ECX and rebuilds the return stack.
        # Checking the whole body avoids recognizing a two-instruction lookalike.
        code = bytes.fromhex('513d001000008d4c2408721481e9001000002d0010000085013d0010000073ec'
                             '2bc88bc485018be18b088b400450c3')
        return entry == 0x7CA650 and self.read(entry, len(code)) == code


def census_rows(text):
    """Validate a SignatureCensus-style JSONL snapshot, rejecting overlaps."""
    try:
        rows = [json.loads(line) for line in text.splitlines() if line.strip()]
        if not rows:
            raise ValueError('Empty census')
        entries, spans = set(), []
        for row in rows:
            entry = int(address(row['entry']), 16)
            if entry in entries:
                raise ValueError(f'Duplicate census entry {entry:X}')
            entries.add(entry)
            if not isinstance(row['name'], str) or not isinstance(row['proto'], str):
                raise ValueError('Census names/prototypes must be strings')
            for key in ('external', 'thunk', 'noreturn'):
                if not isinstance(row[key], bool):
                    raise ValueError(f'Census {key} must be boolean')
            if not isinstance(row['purge'], int):
                raise ValueError('Census purge must be an integer')
            if row['external']:
                continue
            if not row['ranges']:
                raise ValueError('Internal function has no ranges')
            for lo, hi in row['ranges']:
                lo, hi = int(address(lo), 16), int(address(hi), 16)
                if lo > hi:
                    raise ValueError('Reversed function range')
                spans.append((lo, hi, entry))
            lo, hi = (int(address(a), 16) for a in row['body'][:2])
            if lo != min(int(a, 16) for a, _ in row['ranges']) or hi != max(int(b, 16) for _, b in row['ranges']):
                raise ValueError('Body and ranges disagree')
            if not any(int(a, 16) <= entry <= int(b, 16) for a, b in row['ranges']):
                raise ValueError('Entry outside function ranges')
        spans.sort()
        if any(a[1] >= b[0] for a, b in zip(spans, spans[1:])):
            raise ValueError('Overlapping census ranges')
        return rows
    except (KeyError, TypeError, ValueError) as error:
        raise ReadError(f'Invalid signature census: {error}') from error


class NativeFrames:
    def __init__(self, rows, image, noreturn=()):
        self.rows = {int(row['entry'], 16): row for row in rows}
        self.image = image
        self.walker = Walker(census_func([r for r in rows if not r['external']]), image)
        self.noreturn = {e for e, row in self.rows.items() if row['noreturn']} | set(noreturn)
        self.purges = {}

    def pops(self, instruction, addr):
        operand = instruction.operands[0]
        if operand.type != X.X86_OP_IMM:
            return None
        target = operand.imm & 0xFFFFFFFF
        if self.image.is_probe(target):
            return 'alloca'
        if target in self.noreturn:
            return 'noreturn'
        row = self.rows.get(target)
        if row is None or row['external'] or row['thunk']:
            return None
        if target not in self.purges:
            self.purges[target] = self.walker.purge(self.walker.walk(target, int(row['body'][1], 16) + 1))
        return self.purges[target]

    def code(self, addr):
        entry = int(addr, 16)
        row = self.rows.get(entry)
        if row is None or row['external']:
            raise ReadError(f'No internal census function at {addr}')
        result = self.walker.depths(entry, int(row['body'][1], 16) + 1, self.pops)
        out = {}
        for site, depth in result['depth'].items():
            instruction = self.image.instruction(site)
            for operand in instruction.operands:
                if operand.type != X.X86_OP_MEM or operand.mem.index or not operand.mem.base:
                    continue
                base = instruction.reg_name(operand.mem.base)
                at = depth if base == 'esp' else result['ebp'].get(site) if base == 'ebp' else None
                if at is not None:
                    out.setdefault(f'{site:08x}', set()).add(
                        (at + operand.mem.disp, None if instruction.mnemonic == 'lea' else operand.size))
        mapped = {a: sorted([list(o) for o in values], key=lambda o: (o[0], o[1] or 0)) for a, values in out.items()}
        return mapped, result

def signed(hexstr):
    v = int(hexstr, 16) & 0xFFFFFFFF
    return v - (1 << 32) if v & 0x80000000 else v


def key(v):
    return (v['space'], v['offset'], v['size'])


def pcode_frame(blocks, image):
    """Map checked stack expressions, including an aggregate base plus member.

    Resolve only constant expressions connected by unique varnodes at one
    instruction. Do not carry register/unique guesses between instructions.
    A base used solely to form a member address is intermediate, not the
    address passed by the native LEA (Cell rehearsal 0x425713, 0x5F6A85,
    0x75B5BF; native/p-code fixtures in cellclass/session).

    `blocks` is [(stop address, ops), ...]. Returns (frame, edges). A COPY the
    decompiler inserts to feed a successor's MULTIEQUAL carries its
    predecessor block's stop address (Ghidra Merge::trimOpInput), not an
    operand of the native instruction there: 0x41028C `mov ebx,[esp+0x10]`
    reads Stack+4 while its block ends with a COPY of Stack+8 for the next
    REPE CMPSD (fixtures/query_interface_410260.json). Such a COPY - at its
    block's stop, into a temporary defined once and read only by MULTIEQUALs
    - stays out of the frame; `edges` keeps its stack inputs by address.
    """
    defined, readers = {}, {}
    for _, ops in blocks:
        for op in ops:
            if op.get('output'):
                defined[key(op['output'])] = defined.get(key(op['output']), 0) + 1
            for v in op.get('inputs', []):
                readers.setdefault(key(v), []).append(op['mnemonic'])
    groups, edges = {}, {}
    for stop, ops in blocks:
        for op in ops:
            address, v = op['seq']['address'], op.get('output')
            use = readers.get(key(v), []) if v else []
            if (op['mnemonic'] == 'COPY' and address == stop and v and v['space'] == 'unique'
                    and defined[key(v)] == 1 and use and all(m == 'MULTIEQUAL' for m in use)):
                edges.setdefault(address, set()).update(
                    (signed(i['offset']), i['size']) for i in op['inputs'] if i['space'] == 'stack')
            else:
                groups.setdefault(address, []).append(op)
    edges = {a: sorted(list(o) for o in values) for a, values in edges.items() if values}
    out = {}
    for address, ops in groups.items():
        native = image.instruction(int(address, 16))
        if native is None:
            raise ReadError(f'P-code address {address} has no decodable file-backed instruction')
        operands = native.operands if native else ()
        write_only = (native is not None and native.mnemonic == 'mov' and len(operands) == 2
            and operands[0].type == X.X86_OP_MEM and not operands[0].mem.index
            and native.reg_name(operands[0].mem.base) in ('esp', 'ebp')
            and operands[1].type in (X.X86_OP_REG, X.X86_OP_IMM))
        definitions, users = {}, {}
        for n, op in enumerate(ops):
            v = op.get('output')
            if v and v['space'] == 'unique':
                definitions.setdefault(key(v), []).append(n)
            for v in op.get('inputs', []):
                users.setdefault(key(v), []).append(n)

        def value(v, seen):
            if v['space'] == 'register' and v['offset'] == ESP and v['size'] == 4:
                return 0
            k = key(v)
            ds = definitions.get(k, []) if v['space'] == 'unique' else []
            if len(ds) != 1 or ds[0] in seen:
                return None
            return expression(ds[0], seen | {ds[0]})

        def expression(n, seen):
            op = ops[n]
            ins, mnemonic = op.get('inputs', []), op['mnemonic']
            if mnemonic in ('COPY', 'CAST') and len(ins) == 1:
                return value(ins[0], seen)
            if mnemonic not in ('PTRSUB', 'PTRADD', 'INT_ADD') or len(ins) < 2:
                return None
            if ins[1]['space'] != 'const':
                return None
            base = value(ins[0], seen)
            if base is None:
                return None
            offset = signed(ins[1]['offset'])
            if mnemonic == 'PTRADD':
                if len(ins) != 3 or ins[2]['space'] != 'const':
                    return None
                offset *= signed(ins[2]['offset'])
            return base + offset

        affine = {n: expression(n, {n}) for n in range(len(ops))}
        offs = set()
        for n, op in enumerate(ops):
            if op['mnemonic'] in SKIP:
                continue
            if write_only:
                # Select destinations once for both raw stack components and
                # connected addresses. COPY/CAST affine inputs remain sources.
                v = op.get('output')
                if v and v['space'] == 'stack':
                    offs.add((signed(v['offset']), v['size']))
                ins = op.get('inputs', [])
                if op['mnemonic'] == 'STORE' and len(ins) == 3:
                    target = value(ins[1], {n})
                    if target is not None:
                        offs.add((target, None))
                continue
            vs = op.get('inputs', []) + ([op['output']] if op.get('output') else [])
            offs.update((signed(v['offset']), v['size']) for v in vs if v['space'] == 'stack')
            if affine[n] is None:
                continue
            v = op.get('output')
            use = users.get(key(v), []) if v and v['space'] == 'unique' else []
            # Drop a base only when all observed consumers form a checked
            # constant member expression. Calls or other uses retain it.
            intermediate = use and all(affine[u] is not None and
                key(ops[u]['inputs'][0]) == key(v) for u in use)
            if not intermediate:
                offs.add((affine[n], None))
        if offs or write_only:
            # An unrepresented destination is a failed agreement, not a
            # disappeared address that could silently reduce gate coverage.
            # This covers present high instructions; entirely absent high
            # instructions remain outside this mapping's declared coverage.
            out[address] = sorted([list(o) for o in offs], key=lambda o: (o[0], o[1] or 0))
    return out, edges


def fits(c, d):
    """Whether the code's [offset, size] and the decompiler's are the same place: bytes that overlap, or the same
    address when either is an address (lea, PTRSUB): CellClass__GetGroundHeight writes a word at Stack[6] that the
    decompiler keeps in its dword at Stack[4]."""
    if c[1] is None or d[1] is None:
        return c[0] == d[0]
    return c[0] < d[0] + d[1] and d[0] < c[0] + c[1]


def agreement(dec, real):
    """{'agree': n, 'wrong': [[address, code places, decompiler places], ...]} over the addresses both map."""
    ok, bad = 0, []
    for a, offs in sorted(real.items()):
        if a in dec:
            if any(fits(c, d) for c in offs for d in dec[a]):
                ok += 1
            else:
                bad.append([a, offs, dec[a]])
    return {'agree': ok, 'wrong': bad}


def read_frame(client, addr, image):
    try:
        # MCP 5.14.2 obtains a fresh HighFunction for either granularity. In
        # Ghidra 12.1.2 its AST decoder inserts every new operation into its
        # block, and this endpoint does not mutate that graph. Block iteration
        # therefore retains its decoded SSA operations without serializing
        # them twice. An arbitrary mutated PcodeSyntaxTree can also have dead
        # bank operations; that general case is not this endpoint's contract.
        data = json.loads(client.get('/get_function_pcode', function_address=addr, granularity='basic'))
        if not isinstance(data, dict) or any(key == 'error' or key.endswith('_error') for key in data):
            raise ReadError(f'P-code read contains an error: {str(data)[:200]}')
        if address(data['address']) != addr or data.get('address_space') not in (None, 'ram'):
            raise ReadError('P-code entry/address space differs from the requested x86 function')
        blocks = data['basic_blocks']
        if not isinstance(blocks, list) or not blocks:
            raise ReadError('Missing or empty decoded high p-code blocks')
        ops, decoded = [], []
        for block in blocks:
            if not isinstance(block, dict) or not isinstance(block.get('pcodes'), list):
                raise ReadError('Incomplete decoded high p-code block')
            ops.extend(block['pcodes'])
            decoded.append((address(block['stop']['address'])[2:].lower(), block['pcodes']))
        if not ops:
            raise ReadError('Empty decoded high p-code')
        for op in ops:
            op['seq']['address'] = address(op['seq']['address'])[2:].lower()
            if op['seq'].get('address_space') not in (None, 'ram'):
                raise ReadError('Unsupported p-code instruction address space')
            if not isinstance(op['mnemonic'], str) or not isinstance(op['inputs'], list):
                raise ReadError('Malformed p-code operation')
            for var in op['inputs'] + ([op['output']] if op.get('output') else []):
                if not isinstance(var['space'], str) or not isinstance(var['size'], int) or var['size'] <= 0:
                    raise ReadError('Malformed p-code varnode')
                var['offset'] = format(int(var['offset'], 16), 'x')
        return pcode_frame(decoded, image)
    except (KeyError, TypeError, ValueError, OSError) as error:
        raise ReadError(f'{addr}: unreadable high p-code: {error}') from error


def compare_frames(before, after, addresses, native_frames):
    report = dict(kind='frames', before=before.identity(), after=after.identity(),
                  attempted=len(addresses), compared=0, read_errors=[], analysis_limits=[], functions=[])
    for addr in addresses:
        maps, edges, failure = {}, {}, {'addr': addr}
        for side, client in (('before', before), ('after', after)):
            try:
                maps[side], edges[side] = read_frame(client, addr, native_frames.image)
            except ReadError as error:
                failure[side] = str(error)
        if len(maps) != 2:
            report['read_errors'].append(failure)
            continue
        try:
            real, result = native_frames.code(addr)
        except (ReadError, ValueError) as error:
            report['read_errors'].append({'addr': addr, 'native': str(error)})
            continue
        fb, fa = maps['before'], maps['after']
        eb, ea = edges['before'], edges['after']
        gb, ga = agreement(fb, real), agreement(fa, real)
        wb, wa = {x[0] for x in gb['wrong']}, {x[0] for x in ga['wrong']}
        common = sorted(set(fb) & set(fa))
        changed = [[site, fb[site], fa[site]] for site in common if fb[site] != fa[site]]
        unsolved = [f'0x{site:08X}' for site, pop in result['pops'].items() if pop is None]
        row = dict(addr=addr, common=len(common), changed=changed,
                   shifts=sorted({c[2][0][0] - c[1][0][0] for c in changed if len(c[1]) == len(c[2]) == 1}),
                   before=gb, after=ga, right_to_wrong=sorted(site for site in wa - wb if site in fb),
                   wrong_to_right=sorted(site for site in wb - wa if site in fa),
                   code=dict(unsolved=unsolved, notes=result['notes'], conflicts=result['conflicts']),
                   native_accesses=len(real), unmapped_before=sorted(set(real) - set(fb)),
                   unmapped_after=sorted(set(real) - set(fa)),
                   # Phi-edge stack copies at a native access: [site, native, before, after].
                   edges=[[site, real[site], eb.get(site, []), ea.get(site, [])]
                          for site in sorted(set(real) & (set(eb) | set(ea)))],
                   edge_changed=[[site, eb[site], ea[site]]
                                 for site in sorted(set(eb) & set(ea)) if eb[site] != ea[site]])
        report['functions'].append(row)
        report['compared'] += 1
        if unsolved or result['notes'] or result['conflicts']:
            report['analysis_limits'].append(dict(addr=addr, **row['code']))
    report['complete'] = not report['read_errors']
    has_findings = any(row['changed'] or row['edge_changed'] or row['after']['wrong']
                       for row in report['functions'])
    report['status'] = ('incomplete' if not report['complete'] or report['analysis_limits'] else
                        'findings' if has_findings else 'ok')
    return report
