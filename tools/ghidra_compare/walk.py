"""Bounded native function walking and stack-depth equations.

Adapted from the checked 2026-10-03 fnwalk.py snapshot; see ghidra_compare.md.
Census boundaries and unknown call effects remain hypotheses, not semantic proof.
"""
import bisect
from fractions import Fraction
from capstone import x86_const as X

FAM = {'ecx': 'ecx', 'cx': 'ecx', 'cl': 'ecx', 'ch': 'ecx', 'edx': 'edx', 'dx': 'edx', 'dl': 'edx', 'dh': 'edx'}


def census_func(rows):
    """func(a) over SignatureCensus.java rows (ghidra-signatures-20261001/census/census1.jsonl): (name, entry, end exclusive,
    prototype) of the function whose body holds a, or None. The end is the body's maximum + 1, as receivers.py takes it
    from the server (moved here from stackobj_scan.py on 2026-10-01 without change)."""
    by_entry = {int(f['entry'], 16): f for f in rows}
    spans = sorted((int(lo, 16), int(hi, 16) + 1, int(f['entry'], 16)) for f in rows for lo, hi in f['ranges'])
    starts = [s[0] for s in spans]

    def func(a):
        k = bisect.bisect_right(starts, a) - 1
        if k < 0 or not spans[k][0] <= a < spans[k][1]:
            return None
        f = by_entry[spans[k][2]]
        return f['name'], spans[k][2], int(f['body'][1], 16) + 1, f['proto']
    return func


class Walker:
    def __init__(self, func, image):
        self.func = func
        self.image = image

    def instruction(self, addr, entry):
        instruction = self.image.instruction(addr)
        if instruction is None:
            return None
        if any(self.func(a) is None or self.func(a)[1] != entry
               for a in range(addr, addr + instruction.size)):
            return None
        return instruction

    def walk(self, entry, end):
        rets, tails, notes = set(), set(), []
        live = {'ecx': 'dead', 'edx': 'dead'}
        reach = {'ecx': set(), 'edx': set()}
        seen, work = set(), [(entry, frozenset())]
        def tail_or_call(killed, targets):
            for r in ('ecx', 'edx'):
                if r not in killed:
                    reach[r].update(targets)
                    if live[r] == 'dead':
                        live[r] = 'call'
        while work:
            a, killed = work.pop()
            while (a, killed) not in seen:
                seen.add((a, killed))
                if not entry <= a < end or self.func(a) is None or self.func(a)[1] != entry:
                    notes.append('ran past the body at 0x%X' % a); break
                i = self.instruction(a, entry)
                if i is None:
                    notes.append('undecodable at 0x%X' % a); break
                m, ops = i.mnemonic, i.operands
                if m == 'int3':
                    break
                rd, wr = i.regs_access()
                rd = {FAM.get(i.reg_name(r)) for r in rd} - {None}
                wr = {FAM.get(i.reg_name(r)) for r in wr} - {None}
                if m in ('xor', 'sub', 'sbb') and len(ops) == 2 and ops[0].type == ops[1].type == X.X86_OP_REG and ops[0].reg == ops[1].reg:
                    rd = set()
                if (m in ('or', 'and') and len(ops) == 2 and ops[0].type == X.X86_OP_REG and ops[0].size == 4
                        and ops[1].type == X.X86_OP_IMM and ops[1].imm & 0xFFFFFFFF == (0xFFFFFFFF if m == 'or' else 0)):
                    rd = set()      # `or r, -1` / `and r, 0` only write r
                if m == 'push' and ops[0].type == X.X86_OP_REG and i.reg_name(ops[0].reg) == 'ecx':
                    rd = set()
                for r in rd - killed:
                    live[r] = 'read'
                killed = killed | wr
                nxt = a + i.size
                if m.startswith('ret'):
                    rets.add(ops[0].imm if ops else 0); break
                if m == 'call':
                    t = ops[0].imm & 0xFFFFFFFF if ops[0].type == X.X86_OP_IMM else None
                    if t is None or not self.image.is_probe(t):
                        tail_or_call(killed, [t])
                        killed = frozenset(('ecx', 'edx'))
                    a = nxt; continue
                if m.startswith('j') or m.startswith('loop'):
                    inside, outside, note = self.jump(i, a, entry, end)
                    work += [(t, killed) for t in inside]
                    if outside or note and note.startswith('indirect'):
                        tail_or_call(killed, outside or [None])
                    tails.update(outside)
                    if note:
                        notes.append(note)
                    if m == 'jmp':
                        break
                a = nxt
        return {'rets': sorted(rets), 'tails': sorted(tails), 'ecx': live['ecx'], 'edx': live['edx'], 'notes': notes,
                'reach': {r: sorted(t, key=lambda x: -1 if x is None else x) for r, t in reach.items()}}

    def jump(self, i, a, entry, end):
        """The targets of the jump or loop instruction i at a in the function [entry, end): (inside, outside, note). An
        immediate target in this function's body is inside, any other is a tail jump (outside); `jmp [index*4 + table]`
        goes to the table's entries while they point into [entry, end); any other indirect jump is only a note."""
        op = i.operands[0]
        if op.type == X.X86_OP_IMM:
            t = op.imm & 0xFFFFFFFF
            f = self.func(t) if entry <= t < end else None
            return ([t], [], None) if f is not None and f[1] == entry else ([], [t], None)
        if op.type == X.X86_OP_MEM and op.mem.index and op.mem.scale == 4 and not op.mem.base:
            t0, out = op.mem.disp & 0xFFFFFFFF, []
            for _ in range(65536):
                try:
                    target = self.image.u32(t0 + 4 * len(out))
                except ValueError:
                    return out, [], 'unreadable jump table at 0x%X' % a
                owner = self.func(target)
                if not (entry <= target < end and owner and owner[1] == entry):
                    return out, [], None if out else 'unresolved jump table at 0x%X' % a
                out.append(target)
            return out, [], 'jump table limit at 0x%X' % a
        return [], [], 'indirect jump at 0x%X: %s %s' % (a, i.mnemonic, i.op_str)

    def purge(self, w, depth=0):
        """Bytes the function pops on return: its one RET immediate, or its tail targets' when it only tail-jumps; None when
        they disagree or are unknown."""
        if w['notes']:
            return None
        vals = set(w['rets'])
        for t in w['tails']:
            f = self.func(t)
            if f is None or f[1] != t or depth > 2:
                return None
            vals.add(self.purge(self.walk(f[1], f[2]), depth + 1))
        return vals.pop() if len(vals) == 1 and None not in vals else None

    def depths(self, entry, end, pops):
        """The stack depth at each instruction reached from the entry inside [entry, end): ESP minus the entry's ESP, so
        [esp + d] at depth D is stack offset D + d in the decompiler's numbering (the return address at 0, the first stack
        parameter at 4). pops(i, a) gives what the call i at a pops beyond its return address: bytes (0 for __cdecl),
        'noreturn', 'alloca' (the stack probe, which takes EAX bytes, set by `mov eax, imm` before it), or None when
        unknown. Each unknown is a variable solved from two rules compiled code keeps: paths that meet have the same
        depth, and a RET or a tail jump leaves ESP at the return address (depth 0). Ghidra's decompiler uses only the
        first and guesses that an unknown call pops nothing (StackSolver::build, coreaction.cc, Ghidra 12.1.2; stack-object
        pass, 2026-10-01). Paths stop at ESP writes it does not model (noted).
        Returns {'depth': {a: D or None}, 'ebp': {a: EBP's depth or None (when it holds no stack address)}, 'pops': {a:
        bytes or None for each unknown call}, 'notes': [...], 'conflicts': [what each equation that does not fit came
        from]}."""
        def add(e, k, v):
            r = dict(e)
            r[k] = r.get(k, 0) + v
            return {x: c for x, c in r.items() if c or x is None}
        def sub(e, f):
            r = dict(e)
            for k, v in f.items():
                r = add(r, k, -v)
            return r
        def reg(op, name):
            return op.type == X.X86_OP_REG and i.reg_name(op.reg) == name
        at, bpat, eax_seen, eqs, unknown, notes = {}, {}, {}, [], [], []
        work = [(entry, {None: 0}, None, None)]           # address, ESP, EBP as a stack address, EAX as a constant
        while work:
            a, d, bp, eax = work.pop()
            while True:
                if a in at:
                    if bpat[a] != bp:
                        notes.append('ambiguous EBP at merge 0x%X' % a)
                    if at[a] != d:
                        eqs.append((sub(at[a], d), 'paths meeting at 0x%X' % a))
                    if eax in eax_seen[a]:
                        break
                    # EAX is a stack input to _chkstk. Follow a newly observed
                    # constant until it is overwritten or its allocation meets
                    # the earlier path and creates a stack-depth constraint.
                    # Values are bounded to MOV immediates plus unknown (None),
                    # so loops cannot create an unbounded number of states.
                    eax_seen[a].add(eax)
                if not entry <= a < end or self.func(a) is None or self.func(a)[1] != entry:
                    notes.append('ran past the body at 0x%X' % a); break
                i = self.instruction(a, entry)
                if i is None:
                    notes.append('undecodable at 0x%X' % a); break
                if a not in at:
                    at[a], bpat[a], eax_seen[a] = d, bp, {eax}
                m, ops, nxt = i.mnemonic, i.operands, a + i.size
                if m in ('int3', 'hlt', 'ud2'):
                    break
                wr = {i.reg_name(r) for r in i.regs_access()[1]}
                if m == 'push':
                    d = add(d, None, -2 if ops[0].size == 2 else -4)
                elif m == 'pop' and ops[0].type == X.X86_OP_REG and i.reg_name(ops[0].reg) in ('esp', 'sp'):
                    notes.append('stack pointer loaded by POP at 0x%X' % a); break
                elif m == 'pop':
                    d = add(d, None, 2 if ops[0].size == 2 else 4)
                elif m in ('pushal', 'pushfd', 'popal', 'popfd'):
                    d = add(d, None, {'pushal': -32, 'pushfd': -4, 'popal': 32, 'popfd': 4}[m])
                elif m in ('sub', 'add') and reg(ops[0], 'esp') and ops[1].type == X.X86_OP_IMM:
                    d = add(d, None, (ops[1].imm if m == 'add' else -ops[1].imm))
                elif m == 'lea' and reg(ops[0], 'esp') and not ops[1].mem.index and i.reg_name(ops[1].mem.base) in ('esp', 'ebp') \
                        and (i.reg_name(ops[1].mem.base) == 'esp' or bp is not None):
                    d = add(d if i.reg_name(ops[1].mem.base) == 'esp' else bp, None, ops[1].mem.disp)
                elif m == 'mov' and reg(ops[0], 'esp') and reg(ops[1], 'ebp') and bp is not None:
                    d = bp
                elif m == 'leave' and bp is not None:
                    d = add(bp, None, 4)
                elif m == 'call':
                    p = pops(i, a)
                    if p == 'noreturn':
                        break
                    if p == 'alloca':
                        if eax is None:
                            notes.append('stack probe at 0x%X without a constant EAX' % a); break
                        d = add(d, None, -eax)
                    elif p is None:
                        unknown.append(a)
                        d = add(d, a, 1)
                    else:
                        d = add(d, None, p)
                elif m.startswith('ret'):
                    eqs.append((d, 'RET at 0x%X' % a)); break
                elif wr & {'esp', 'sp'}:
                    notes.append('ESP set by `%s %s` at 0x%X' % (m, i.op_str, a)); break
                if m == 'mov' and reg(ops[0], 'ebp') and reg(ops[1], 'esp'):
                    bp = d
                elif 'ebp' in wr or m == 'leave':
                    bp = None
                if m == 'mov' and reg(ops[0], 'eax') and ops[1].type == X.X86_OP_IMM:
                    eax = ops[1].imm
                elif wr & {'eax', 'ax', 'al', 'ah'} or m == 'call':
                    eax = None
                if m.startswith('j') or m.startswith('loop'):
                    inside, outside, note = self.jump(i, a, entry, end)
                    work += [(t, d, bp, eax) for t in inside]
                    eqs += [(d, 'tail jump at 0x%X' % a) for t in outside[:1]]
                    if note:
                        notes.append(note)
                    if m == 'jmp':
                        break
                a = nxt
        cols = sorted({k for e, _ in eqs for k in e if k is not None})
        rows = [[Fraction(e.get(v, 0)) for v in cols] + [Fraction(-e.get(None, 0))] for e, _ in eqs]
        why, rank, piv = [w for _, w in eqs], 0, []
        for col in range(len(cols)):
            pr = next((k for k in range(rank, len(rows)) if rows[k][col]), None)
            if pr is None:
                continue
            rows[rank], rows[pr], why[rank], why[pr] = rows[pr], rows[rank], why[pr], why[rank]
            rows[rank] = [x / rows[rank][col] for x in rows[rank]]
            for k in range(len(rows)):
                if k != rank and rows[k][col]:
                    f = rows[k][col]
                    rows[k] = [x - f * y for x, y in zip(rows[k], rows[rank])]
            piv.append(col)
            rank += 1
        sol = {cols[col]: rows[k][-1] for k, col in enumerate(piv)
               if not any(rows[k][c] for c in range(len(cols)) if c != col) and rows[k][-1].denominator == 1}
        def value(e):
            # reduce by the solved rows: a depth can be known while the pops it sums are not (two unknown calls in a row)
            c, co = Fraction(e.get(None, 0)), {k: Fraction(v) for k, v in e.items() if k is not None}
            for k, col in enumerate(piv):
                f = co.get(cols[col], 0)
                if f:
                    c += f * rows[k][-1]
                    for j in range(len(cols)):
                        co[cols[j]] = co.get(cols[j], 0) - f * rows[k][j]
            return int(c) if not any(co.values()) and c.denominator == 1 else None
        return {'depth': {a: value(e) for a, e in at.items()},
                'ebp': {a: (value(e) if e is not None else None) for a, e in bpat.items()},
                'pops': {a: (int(sol[a]) if a in sol else None) for a in unknown},
                'notes': notes, 'conflicts': [why[k] for k in range(rank, len(rows)) if rows[k][-1]]}
