"""Explicit-program, GET-only access to the GhidraMCP 5.14.2 read endpoints."""
from dataclasses import dataclass
import http.client
import json
import re
import urllib.error
import urllib.parse
import urllib.request


class ReadError(ValueError):
    """A required read was absent, malformed or incomplete."""


def address(value):
    """Normalize an x86 hexadecimal address (JSON numbers are not accepted)."""
    if not isinstance(value, str) or not re.fullmatch(r'(?:0x)?[0-9a-fA-F]{1,8}', value):
        raise ValueError(f'Expected an x86 hexadecimal address, got {value!r}')
    return f'0x{int(value, 16):08X}'


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ReadError('Ghidra endpoint redirected; select the intended server explicitly')


@dataclass(frozen=True)
class Client:
    url: str
    program: str
    timeout: float = 60
    retries: int = 3

    def __post_init__(self):
        parsed = urllib.parse.urlsplit(self.url)
        if (parsed.scheme not in ('http', 'https') or not parsed.hostname or parsed.username
                or parsed.password or parsed.query or parsed.fragment or parsed.path not in ('', '/')):
            raise ValueError('Use an http(s) server origin, without credentials, path or query')
        if not self.program.strip() or self.timeout <= 0 or not 1 <= self.retries <= 3:
            raise ValueError('An explicit program, positive timeout and 1..3 attempts are required')

    def identity(self):
        return {'url': self.url.rstrip('/'), 'program': self.program}

    def get(self, path, **params):
        if path not in ('/decompile_function', '/get_xrefs_to',
                        '/get_function_by_address', '/get_function_pcode'):
            raise ValueError(f'Not a supported read endpoint: {path}')
        if 'program' in params:
            raise ValueError('The program selector belongs to the client')
        params['program'] = self.program
        request = self.url.rstrip('/') + path + '?' + urllib.parse.urlencode(params)
        for attempt in range(self.retries):
            try:
                with urllib.request.build_opener(NoRedirect).open(request, timeout=self.timeout) as response:
                    data = response.read()
                    length = response.headers.get('Content-Length')
                    if length is not None and len(data) != int(length):
                        raise ReadError('Truncated HTTP body')
                    result = data.decode('utf-8')
                if result.lstrip().startswith('{'):
                    try:
                        obj = json.loads(result)
                    except ValueError:
                        obj = None  # The endpoint-specific parser reports malformed content.
                    if isinstance(obj, dict) and ('error' in obj or obj.get('success') is False):
                        raise ReadError(str(obj.get('error', obj))[:500])
                return result
            except (OSError, http.client.HTTPException, UnicodeError, ReadError) as error:
                if attempt + 1 == self.retries:
                    raise ReadError(f'{path}: {error}') from error


XREF = re.compile(r'^From ((?:0x)?[0-9a-fA-F]+)(?: in (.*))? \[([A-Z_]+)\]$')


def xrefs(client, target, page_size=500):
    """Read overlapping pages; a blank body never certifies an empty list.

    Ghidra's exhausted paginator returns a blank body. Retaining one anchor row
    avoids that ambiguous response even for exact page-size multiples, and lets
    us detect a changed list before trusting the following page.
    """
    if page_size < 2:
        raise ValueError('Xref page size must leave room for an anchor and a new row')
    found, previous, anchor = [], set(), None
    for offset in range(0, 1_000_000, page_size - 1):
        text = client.get('/get_xrefs_to', address=address(target), limit=page_size, offset=offset)
        lines = [line.strip() for line in text.splitlines() if line.strip()]
        if not lines:
            raise ReadError(f'Empty xref response for {target}; list completeness is unknown')
        if len(lines) == 1 and lines[0].startswith('No references found to address:'):
            if offset:
                raise ReadError(f'Xref list disappeared while reading {target}')
            return found
        matches = [XREF.fullmatch(line) for line in lines]
        if not all(matches) or len(lines) > page_size:
            raise ReadError(f'Malformed xref page for {target}: {text[:200]}')
        signature = tuple(lines)
        if signature in previous:
            raise ReadError(f'Repeated xref page for {target}; pagination did not advance')
        previous.add(signature)
        if anchor is not None and lines[0] != anchor:
            raise ReadError(f'Xref page anchor changed for {target}; repeat on a stable project')
        found.extend((address(m[1]), m[2], m[3]) for m in matches[(1 if offset else 0):])
        anchor = lines[-1]
        if len(lines) < page_size:
            return found
    raise ReadError(f'Xref pagination limit reached for {target}')


def callers(client, targets, *, computed=False, include_tail=True):
    """Known direct/tail callers, optional computed callers, and callers of thunks.

    Ghidra references can reflect overrides and may-call edges. This is discovery,
    not proof that the native instruction calls or that the path is reachable.
    """
    todo, seen, entries, result = list(targets), set(targets), {}, set()
    while todo:
        target = todo.pop()
        for site, name, kind in xrefs(client, target):
            if kind not in ('UNCONDITIONAL_CALL', 'UNCONDITIONAL_JUMP') and not (
                    computed and kind == 'COMPUTED_CALL'):
                continue
            if name is None:
                raise ReadError(f'Caller at {site} has no containing function; repair/census it explicitly')
            if site not in entries:
                text = client.get('/get_function_by_address', address=site)
                match = re.search(r'^Entry: ((?:0x)?[0-9a-fA-F]+)\s*$', text, re.M)
                if not match:
                    raise ReadError(f'Unreadable containing function at {site}: {text[:200]}')
                entries[site] = address(match[1])
            entry = entries[site]
            if kind != 'UNCONDITIONAL_JUMP' or include_tail:
                result.add(entry)
            if kind == 'UNCONDITIONAL_JUMP' and entry == site and entry not in seen:
                seen.add(entry)
                todo.append(entry)
    return sorted(result)


def spread(items, maximum):
    if maximum is None or maximum >= len(items):
        return list(items)
    if maximum < 0:
        raise ValueError('Sample size cannot be negative')
    return [items[k * len(items) // maximum] for k in range(maximum)]
