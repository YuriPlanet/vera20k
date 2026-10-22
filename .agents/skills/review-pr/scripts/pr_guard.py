"""Mechanical pre-review guard for one pull request.

Usage: python pr_guard.py <number> [--repo OWNER/NAME] [--maintainer LOGIN]

Reads the PR through `gh` and never runs its code. It reports maintainer-only
AGENTS.md edits (exit 2), protected paths, risky added lines, dependency
changes, binary files and hidden Unicode. Hits are leads for reading in
context, not verdicts; a clean report is not a malware clearance.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys

REPO = 'YuriPlanet/vera20k'
MAINTAINER = 'hrusten'
# AGENTS.md commits on the default branch by someone else that the maintainer
# has read and kept. Only the maintainer adds entries.
ACCEPTED_AGENTS_COMMITS: set[str] = set()

# Steer agents (AGENTS.md/CLAUDE.md at any depth, skills, hooks, settings) or
# run on maintainer machines (CI, Cargo config, build scripts, the build runner).
PROTECTED = re.compile(
    r'(^|/)(AGENTS|CLAUDE)\.md$|^\.(agents|claude|codex|github|cargo|vscode)/|(^|/)build\.rs$'
    r'|(^|/)Cargo\.(toml|lock)$|^rust-toolchain|^\.git(attributes|modules|ignore)$'
    r'|^tools/(cargo_run|prepare_checkout|run_tests|skill_sync)\.py$|\.(sh|ps1|bat|cmd)$')
MAINTAINER_ONLY = re.compile(r'(^|/)AGENTS\.md$')

RISKY = [
    ('process', r'\bsubprocess\b|os\.(system|popen|exec\w*|spawn\w*)\(|std::process|\bCommand::new\b'
                r'|Start-Process|\bInvoke-Expression\b|\biex\b|\bbash -c\b|\|\s*(ba)?sh\b'),
    ('network', r'\b(import|from)\s+(socket|urllib|requests|http|httpx|aiohttp|ftplib|smtplib)\b'
                r'|\brequests\.(get|post)|std::net|TcpStream|UdpSocket|\b(reqwest|ureq|hyper)::|\bcurl\b'
                r'|\bwget\b|Invoke-WebRequest|https?://(?!github\.com/YuriPlanet/)'),
    ('dynamic code', r'\b(eval|exec|compile|__import__)\s*\(|importlib|pickle\.loads?|marshal\.loads?'
                     r'|\bctypes\b|extern\s+"(C|system)"|#\[link|libloading|\bunsafe\b'),
    ('obfuscation', r'b(64|32|85)decode|codecs\.decode|\\x[0-9a-fA-F]{2}(\\x[0-9a-fA-F]{2}){15}'
                    r'|[A-Za-z0-9+/]{120,}={0,2}'),
    ('destructive', r'rmtree|remove_dir_all|\brm -rf\b|Remove-Item.*-Recurse|git (push|reset --hard|clean)'
                    r'|cargo clean'),
    ('secrets', r'\.ssh\b|id_rsa|\.aws\b|keychain|GH_TOKEN|GITHUB_TOKEN|api[_-]?key|password'),
    ('agent text', r'(?i)ignore (all |any )?(previous|prior|above) (instructions|rules)|\byou are (claude|codex|an ai)'
                   r'|system prompt|(?<!\w)(claude|codex|reviewer|agent)s?,? (please )?(merge|approve|skip|run|ignore)'
                   r'|do not (tell|mention|report)'),
]
RISKY = [(name, re.compile(pattern)) for name, pattern in RISKY]
# AGENTS.md caps committed test data at about 100 KB per file.
LARGE_FILE_BYTES = 100 * 1024
HEX_ONLY = re.compile(r'^[0-9a-fA-F]+$')
HIDDEN = re.compile('[​-‏‪-‮⁦-⁩﻿­]')
DEPENDENCY = re.compile(r'^\s*[\w-]+\s*=\s*(\{.*\}|"[^"]*")\s*$|\b(git|path|registry)\s*=')


def parse_diff(diff: str) -> dict[str, dict]:
    """Added lines per file (new path), plus binary markers, from `gh pr diff`."""
    files: dict[str, dict] = {}
    current = None
    for line in diff.splitlines():
        if line.startswith('diff --git '):
            current = files.setdefault(line.rsplit(' b/', 1)[-1], {'added': [], 'binary': False})
            number = 0
        elif current is None:
            continue
        elif line.startswith('@@'):
            match = re.search(r'\+(\d+)', line)
            number = int(match.group(1)) - 1 if match else 0
        elif line.startswith(('Binary files', 'GIT binary patch')):
            current['binary'] = True
        elif line.startswith('+') and not line.startswith('+++'):
            number += 1
            current['added'].append((number, line[1:]))
        elif not line.startswith(('-', '\\')):
            number += 1
    return files


def risky_hits(path: str, added: list[tuple[int, str]]) -> list[str]:
    hits = []
    for number, text in added:
        for name, pattern in RISKY:
            match = pattern.search(text)
            if match and not (name == 'obfuscation' and HEX_ONLY.match(match.group(0))):
                hits.append(f'{path}:{number} [{name}] {text.strip()[:160]}')
                break
        if HIDDEN.search(text):
            hits.append(f'{path}:{number} [hidden unicode] {text.encode("unicode_escape").decode()[:160]}')
    return hits


def large_additions(files: dict[str, dict], limit: int = LARGE_FILE_BYTES) -> list[str]:
    """Files whose added text exceeds AGENTS.md's test-data limit."""
    sizes = {p: sum(len(t.encode('utf-8')) + 1 for _, t in f['added']) for p, f in files.items()}
    return [f'{p}: {size // 1024} KB added' for p, size in sorted(sizes.items(), key=lambda i: -i[1])
            if size > limit]


def added_line_split(files: dict[str, dict]) -> str:
    """Added lines as code, tests and other (tools, data, docs), to spot bloat."""
    code = tests = other = 0
    for path, f in files.items():
        count = len(f['added'])
        if path.endswith('.rs') and 'test' not in path:
            code += count
        elif 'test' in path:
            tests += count
        else:
            other += count
    return f'code {code}, tests {tests}, other {other}'


def maintainer_only_findings(paths: list[str], author: str, commit_authors: set[str],
                             maintainer: str) -> list[str]:
    touched = [path for path in paths if MAINTAINER_ONLY.search(path)]
    others = sorted((commit_authors | {author}) - {maintainer})
    if touched and others:
        return [f'{path} changed by {", ".join(others)}' for path in touched]
    return []


def gh(*args: str) -> str:
    return subprocess.run(['gh', *args], check=True, capture_output=True, text=True,
                          encoding='utf-8').stdout


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('number')
    parser.add_argument('--repo', default=REPO)
    parser.add_argument('--maintainer', default=MAINTAINER)
    args = parser.parse_args(argv)

    view = json.loads(gh('pr', 'view', args.number, '--repo', args.repo, '--json',
                         'author,commits,files,headRefOid,headRepositoryOwner,maintainerCanModify'))
    files = parse_diff(gh('pr', 'diff', args.number, '--repo', args.repo))
    paths = sorted({f['path'] for f in view['files']} | set(files))
    author = view['author']['login']
    commit_authors = {a.get('login') or a.get('email', '?') for c in view['commits'] for a in c['authors']}
    # Verified GitHub accounts of every AGENTS.md commit on the default branch.
    history = json.loads(gh('api', f'repos/{args.repo}/commits?path=AGENTS.md&per_page=100'))
    foreign_history = [f'{c["sha"][:9]} by {(c.get("author") or {}).get("login") or c["commit"]["author"]["email"]}'
                       for c in history if (c.get('author') or {}).get('login') != args.maintainer
                       and c['sha'] not in ACCEPTED_AGENTS_COMMITS]

    print(f'PR #{args.number} by {author} from {view["headRepositoryOwner"]["login"]}, '
          f'head {view["headRefOid"]}, maintainer edits {"allowed" if view["maintainerCanModify"] else "BLOCKED"}')
    print(f'Commit authors: {", ".join(sorted(commit_authors))}')
    print(f'Added lines: {added_line_split(files)}')
    blocks = maintainer_only_findings(paths, author, commit_authors, args.maintainer)
    sections = [
        ('STOP: maintainer-only AGENTS.md edit', blocks),
        ('STOP: AGENTS.md commits on the default branch not by the maintainer', foreign_history),
        ('Protected paths (read every changed line)', [p for p in paths if PROTECTED.search(p)]),
        ('Binary files', [p for p, f in files.items() if f['binary']]),
        ('Files over 100 KB (AGENTS.md test-data limit)', large_additions(files)),
        ('Dependency lines', [f'{p}:{n} {t.strip()}' for p, f in files.items() if p.endswith('Cargo.toml')
                              for n, t in f['added'] if DEPENDENCY.search(t)]),
        ('Risky added lines (read in context)', [h for p, f in files.items() for h in risky_hits(p, f['added'])]),
    ]
    for title, items in sections:
        print(f'\n{title}: {len(items) or "none"}')
        for item in items[:200]:
            print(f'  {item}')
        if len(items) > 200:
            print(f'  ... {len(items) - 200} more')
    return 2 if blocks or foreign_history else 0


if __name__ == '__main__':
    sys.exit(main())
