"""Artifact and call-site changes in pairs of complete decompilations."""
import collections
from concurrent.futures import ThreadPoolExecutor
import re

from .client import ReadError, address, callers, spread

VARS = re.compile(r'\b(?:unaff_|in_stack_|extraout_|in_E[A-Z]{2}\b)\w*')
ART = ('unaff_', 'in_stack_', 'in_ECX', 'in_EDX', 'in_EAX', 'extraout_', 'WARNING', 'halt_baddata', 'Bad instruction')
WARN = re.compile(r'/\*\s*(WARNING.*?)\*/', re.S)
CALL = re.compile(r'\b([A-Za-z_][\w:]*(?:<[^()\n;]*?>[\w:]*)*)\s*\(')     # template arguments only inside <...>
# A goto label followed by a parenthesised expression on the next line is not a call (House 0x500300 rehearsal).
NOT_CALLS = re.compile(r'(?:if|while|for|switch|return|sizeof|case|do|else|LAB_[0-9a-fA-F]+:?|(?:CONCAT|SUB|ZEXT|SEXT|CARRY|SCARRY|SBORROW|POPCOUNT)\d*)$')


def pointer_calls(body):
    """Calls through a pointer: a parenthesised `(*...)` followed by an argument list, which a long call puts on the next
    line."""
    n, k = 0, body.find('(*')
    while k >= 0:
        depth, j = 0, k
        while j < len(body):
            depth += {'(': 1, ')': -1}.get(body[j], 0)
            if depth == 0:
                break
            j += 1
        n += body[j + 1:].lstrip()[:1] == '('
        k = body.find('(*', k + 2)
    return n


def counts(text, expected=()):
    """Ignore quoted/comment text, but count real warning comments separately.

    A brace anywhere in an error message is not a successful decompile. Require
    a function header and balanced body after removing strings and comments.
    """
    tokens = re.compile(r'/\*.*?\*/|//[^\n]*|"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'', re.S)
    code = tokens.sub(' ', text)
    start = code.find('{')
    if start < 0 or not re.search(r'\)\s*$', code[:start]):
        raise ReadError(f'No complete function body: {text[:200]}')
    depth = 0
    for char in code[start:]:
        depth += (char == '{') - (char == '}')
        if depth < 0:
            break
    if depth != 0 or not code.rstrip().endswith('}'):
        raise ReadError('Truncated or unbalanced decompilation body')
    warnings = sorted(w.strip() for w in WARN.findall(text))
    result = {key: code.count(key) for key in ART}
    result['WARNING'] = sum(w not in expected for w in warnings)
    result['Bad instruction'] = sum('Bad instruction' in w and w not in expected for w in warnings)
    result.update(vars=sorted(set(VARS.findall(code))), warnings=warnings,
                  calls=dict(collections.Counter(n for n in CALL.findall(code[start:]) if not NOT_CALLS.match(n))),
                  pointer_calls=pointer_calls(code[start:]))
    return result


def lost_calls(before, after):
    if sum(after['calls'].values()) + after['pointer_calls'] >= sum(before['calls'].values()) + before['pointer_calls']:
        return {}
    lost = {key: count - after['calls'].get(key, 0) for key, count in before['calls'].items()
            if count > after['calls'].get(key, 0)}
    if before['pointer_calls'] > after['pointer_calls']:
        lost['(pointer)'] = before['pointer_calls'] - after['pointer_calls']
    return lost


def new_warnings(before, after, expected=()):
    return sorted((collections.Counter(w for w in after['warnings'] if w not in expected)
                   - collections.Counter(before['warnings'])).elements())


def worse_than(before, after, expected=()):
    why = {'rises': [key for key in ART if after[key] > before[key]],
           'new_vars': sorted(set(after['vars']) - set(before['vars'])),
           'new_warnings': new_warnings(before, after, expected),
           'lost_calls': lost_calls(before, after)}
    return why if any(why.values()) else {}


def read_counts(client, addr, expected):
    try:
        return counts(client.get('/decompile_function', address=addr), expected), None
    except (ReadError, OSError) as error:
        return None, str(error)


def compare(before, after, targets, *, computed=False, max_callers=None, expected=()):
    targets = sorted({address(a) for a in targets})
    report = dict(kind='decompile', before=before.identity(), after=after.identity(),
                  computed=computed, max_callers=max_callers, expected_warnings=sorted(expected),
                  read_errors=[], worse=[], better=[], decompiled=0,
                  readable_before=0, readable_after=0, planned=len(targets),
                  total={side: dict.fromkeys(ART, 0) for side in ('before', 'after')},
                  expected_counts={'before': 0, 'after': 0})
    # A failed caller read must not turn into a smaller, apparently successful run.
    try:
        all_callers = sorted(set(callers(before, targets, computed=computed)) - set(targets))
    except (ReadError, OSError) as error:
        report['read_errors'].append({'phase': 'callers', 'error': str(error)})
        all_callers = []
    picked = spread(all_callers, max_callers)
    sample = targets + picked
    report.update(callers=len(all_callers), sampled_callers=len(picked),
                  functions=sample, attempted=len(sample))
    with ThreadPoolExecutor(max_workers=2) as pool:
        for addr in sample:
            futures = [pool.submit(read_counts, client, addr, expected) for client in (before, after)]
            (cb, eb), (ca, ea) = [f.result() for f in futures]
            report['readable_before'] += cb is not None
            report['readable_after'] += ca is not None
            if cb is None or ca is None:
                failure = {'addr': addr}
                if cb is None:
                    failure['before'] = eb
                if ca is None:
                    failure['after'] = ea
                    if cb is not None:
                        report['worse'].append({'addr': addr, 'error_after': ea})
                report['read_errors'].append(failure)
                continue
            report['decompiled'] += 1
            for side, row in (('before', cb), ('after', ca)):
                for key in ART:
                    report['total'][side][key] += row[key]
                report['expected_counts'][side] += sum(w in expected for w in row['warnings'])
            why = worse_than(cb, ca, expected)
            if why:
                report['worse'].append(dict(addr=addr, before=cb, after=ca, **why))
            elif any(ca[k] < cb[k] for k in ART) or set(cb['vars']) - set(ca['vars']) or new_warnings(ca, cb, expected):
                report['better'].append(dict(addr=addr, before=cb, after=ca,
                    gone_vars=sorted(set(cb['vars']) - set(ca['vars'])),
                    gone_warnings=new_warnings(ca, cb, expected)))
    report['complete'] = not report['read_errors']
    report['status'] = 'incomplete' if not report['complete'] else 'findings' if report['worse'] else 'ok'
    return report
