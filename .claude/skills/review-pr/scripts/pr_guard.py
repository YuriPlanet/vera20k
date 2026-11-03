"""Mechanical pre-review guard for one pull request.

Usage: python pr_guard.py <number> [--repo OWNER/NAME] [--maintainer LOGIN]

Reads the PR through `gh` and never runs its code. It reports maintainer-only
AGENTS.md edits (exit 2), private watch-list matches (exit 3), protected paths,
touched sim-scale deviations, risky added lines, dependency changes, binary
files, hidden Unicode, and branding and identity lines. Hits are leads for
reading in context, not verdicts; a clean report is not a malware clearance.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
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
# Code marked this way differs from gamemd on purpose, for speed at 20,000 units.
# AGENTS.md keeps it, and a PR seldom says that it undoes one.
SIM_SCALE = 'sim-scale deviation'

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
# Another identity for the program or a build, or an outside contact point
# (skill step 3). Bare domains stop at .com/.cn/.gg/.tv because .net, .io and
# .top also read as Rust field accesses.
BRAND = re.compile(
    r'(?i)™|®|©|\bcopyright\b|\btrade ?marks?\b|\bbrand(ing|ed)\b|\blogos?\b'
    r'|CFBundle(Identifier|DisplayName|Name)|PRODUCT_BUNDLE_IDENTIFIER|\bapplicationId\b|android:(label|icon)'
    r'|provisioning|codesign|app ?store|testflight|\.(apk|ipa)\b'
    r'|bilibili|b23\.tv|douyin|weibo|tiktok|youtu\.?be|discord\.(gg|com)|t\.me/|qq ?群|\bqq group|wechat|微信|公众号'
    r'|afdian|爱发电|patreon|ko-fi|paypal|alipay|支付宝'
    r'|\bwww\.[\w-]+\.|\b(?!github\.com/(YuriPlanet|user-attachments)/)[\w-]+\.(com|cn|gg|tv)\b'
    r'|\b[\w-]+\.vercel\.app\b|\b(com|cn|net|org|io)\.[a-z0-9_-]+\.[a-z0-9_.-]+')
# VERA20K's own name, notice or source link on a removed line.
IDENTITY = re.compile(r'VERA20K|VERA20k|YuriPlanet|\bGPL|\bGNU\b|(?i:copyright|©)')
IDENTITY_PATHS = re.compile(
    r'(?i)(^|/)(branding|icons?|logos?|splash)(/|\.)|\.(ico|icns|ttf|otf|bik|mp4|webm|apk|ipa|mobileprovision'
    r'|entitlements)$|(^|/)(Info\.plist(\.in)?|AndroidManifest\.xml)$|\.xcodeproj/')


def parse_diff(diff: str) -> dict[str, dict]:
    """Added and removed lines per file (new path), plus binary markers, from `gh pr diff`."""
    files: dict[str, dict] = {}
    current = None
    for line in diff.splitlines():
        if line.startswith('diff --git '):
            current = files.setdefault(line.rsplit(' b/', 1)[-1], {'added': [], 'removed': [], 'binary': False})
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
        elif line.startswith('-') and not line.startswith('--- '):
            current['removed'].append(line[1:])
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


def brand_hits(path: str, added: list[tuple[int, str]], removed: list[str]) -> list[str]:
    hits = [f'{path}:{number} [brand] {text.strip()[:160]}' for number, text in added if BRAND.search(text)]
    return hits + [f'{path} [identity removed] {text.strip()[:160]}' for text in removed if IDENTITY.search(text)]


def watchlist(path: Path | None = None) -> list[str]:
    """Private terms, one per line, from the main checkout's .git/info/pr-watchlist."""
    try:
        if path is None:
            common = subprocess.run(['git', 'rev-parse', '--git-common-dir'], check=True,
                                    capture_output=True, text=True).stdout.strip()
            path = Path(common, 'info', 'pr-watchlist')
        lines = path.read_text(encoding='utf-8').splitlines()
    except (OSError, subprocess.CalledProcessError):
        return []
    return [line.strip().lower() for line in lines if line.strip() and not line.lstrip().startswith('#')]


def watch_hits(terms: list[str], sources: list[tuple[str, str]]) -> list[str]:
    return [f'{where}: {term}' for where, text in sources for term in terms if term in text.lower()]


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


def sim_scale_files(paths: list[str], ref: str, cwd: str | None = None) -> list[str]:
    """Touched Rust files whose `ref` version holds a sim-scale deviation note."""
    found = subprocess.run(['git', 'grep', '-l', '-i', '-F', SIM_SCALE, ref, '--', '*.rs'], cwd=cwd,
                           capture_output=True, text=True, encoding='utf-8')
    if found.returncode > 1:
        return [f'(could not search {ref}: {found.stderr.strip()[:120]})']
    marked = {line.split(':', 1)[1] for line in found.stdout.splitlines()}
    return [path for path in paths if path in marked]


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
                         'author,baseRefName,body,commits,files,headRefName,headRefOid,headRepositoryOwner,'
                         'maintainerCanModify,title'))
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
    text = f'{view["title"]}\n{view["body"]}'
    sources = [('PR text', text), ('branch', view['headRefName']),
               ('accounts', f'{author} {view["headRepositoryOwner"]["login"]}')]
    sources += [(f'commit {c["oid"][:9]}', '\n'.join([c['messageHeadline'], c['messageBody'], *(
        f'{a.get("name")} {a.get("email")} {a.get("login")}' for a in c['authors'])])) for c in view['commits']]
    sources += [(p, p) for p in paths] + [(f'{p}:{n}', t) for p, f in files.items() for n, t in f['added']]
    watched = watch_hits(watchlist(), sources)
    branding = [f'PR text [brand] {line.strip()[:160]}' for line in text.splitlines() if BRAND.search(line)]
    branding += [f'{p} [identity path]' for p in paths if IDENTITY_PATHS.search(p)]
    branding += [h for p, f in files.items() for h in brand_hits(p, f['added'], f['removed'])]
    sections = [
        ('STOP: maintainer-only AGENTS.md edit', blocks),
        ('STOP: AGENTS.md commits on the default branch not by the maintainer', foreign_history),
        ('STOP: private watch-list matches (show the user)', watched),
        ('Protected paths (read every changed line)', [p for p in paths if PROTECTED.search(p)]),
        ('Sim-scale deviations touched (AGENTS.md keeps them; skill step 4)',
         sim_scale_files(paths, f'origin/{view["baseRefName"]}')),
        ('Binary files', [p for p, f in files.items() if f['binary']]),
        ('Files over 100 KB (AGENTS.md test-data limit)', large_additions(files)),
        ('Dependency lines', [f'{p}:{n} {t.strip()}' for p, f in files.items() if p.endswith('Cargo.toml')
                              for n, t in f['added'] if DEPENDENCY.search(t)]),
        ('Risky added lines (read in context)', [h for p, f in files.items() for h in risky_hits(p, f['added'])]),
        ('Branding and identity lines (skill step 3)', branding),
    ]
    for title, items in sections:
        print(f'\n{title}: {len(items) or "none"}')
        for item in items[:200]:
            print(f'  {item}')
        if len(items) > 200:
            print(f'  ... {len(items) - 200} more')
    return 2 if blocks or foreign_history else 3 if watched else 0


if __name__ == '__main__':
    sys.exit(main())
