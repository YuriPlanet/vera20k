"""Test groups: which test modules a focused test run compiles.

Each workspace crate lists its groups in its Cargo.toml: a `tests-<group>`
feature and, under `[package.metadata.test-groups]`, the module path prefixes
the group owns. A test module carries its group's cfg,
`#[cfg(all(test, tests_<group>))]`; the crate's build script switches every
group on unless the `test-focus` feature narrows the build to the named ones
(src/test_group_cfgs.rs). tools.cargo_run picks the group from a test filter.
See tools/README.md ("Test groups"). Standard library only; Python >= 3.11.

    python -m tools.group_tests check    every test module names its own group
    python -m tools.group_tests tag      write each test module's group cfg
    python -m tools.group_tests list     groups, prefixes and test module counts
    python -m tools.group_tests lend MODULE GROUP
                                         let GROUP's build compile test module MODULE
                                         too; GROUP `all` is every test build
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
import re
import sys
import tomllib

# Package name -> (manifest, crate root file), relative to the checkout.
CRATES = {
    'vera20k-core': ('src/Cargo.toml', 'src/core_lib.rs'),
    'vera20k': ('Cargo.toml', 'src/lib.rs'),
}
FOCUS_FEATURE = 'test-focus'
# A test module that code outside every group uses keeps a cfg without a group
# and says so on the same line.
SHARED_MARK = '// shared by test groups'

_ATTRIBUTE = re.compile(r'^([ \t]*)#\[cfg\(', re.M)
_MOD = re.compile(r'\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*[;{]')
_TOP_MOD = re.compile(r'^(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;', re.M)
_GROUP_TERM = re.compile(r'tests_\w+|any\(\s*tests_\w+(?:\s*,\s*tests_\w+)*\s*,?\s*\)')
# rustfmt's defaults: max_width and attr_fn_like_width.
_MAX_WIDTH, _ARGUMENTS_WIDTH = 100, 70


def feature(group: str) -> str:
    return f'tests-{group}'


def cfg_name(group: str) -> str:
    return 'tests_' + group.replace('-', '_')


def is_group_feature(name: str) -> bool:
    """Whether a Cargo feature only chooses which tests compile; it is no capability."""
    return name == FOCUS_FEATURE or name.startswith('tests-')


@dataclass(frozen=True)
class Crate:
    package: str
    top_modules: frozenset[str]
    prefixes: tuple[tuple[str, str], ...]  # (module path prefix, group), longest first
    features: frozenset[str]

    @property
    def groups(self) -> list[str]:
        return sorted({group for _, group in self.prefixes})

    def group_of(self, module_path: str) -> str | None:
        """The group owning a module path: its longest listed prefix (plain text)."""
        return next((group for prefix, group in self.prefixes if module_path.startswith(prefix)), None)

    def groups_for_filter(self, text: str) -> list[str]:
        """The groups holding the tests whose path starts with a filter.

        Empty when the filter names no group: libtest matches a filter anywhere
        in a test's path, so one counts only when it starts with a listed prefix.
        """
        own = self.group_of(text)
        if own is None:
            return []
        return sorted({own} | {group for prefix, group in self.prefixes if prefix.startswith(text)})


def load(root: Path) -> dict[str, Crate]:
    crates = {}
    for package, (manifest, crate_root) in CRATES.items():
        data = tomllib.loads((root / manifest).read_text(encoding='utf-8'))
        table = data.get('package', {}).get('metadata', {}).get('test-groups', {})
        prefixes = sorted(((prefix, group) for group, owned in table.items() for prefix in owned),
                          key=lambda item: -len(item[0]))
        crates[package] = Crate(
            package, frozenset(_TOP_MOD.findall((root / crate_root).read_text(encoding='utf-8'))),
            tuple(prefixes), frozenset(data.get('features', {})))
    return crates


def module_path(relative: Path) -> str:
    """`src/sim/combat/mod.rs` -> `sim::combat`; `src/sim/ammo.rs` -> `sim::ammo`."""
    parts = list(relative.with_suffix('').parts[1:])
    if parts and parts[-1] == 'mod':
        parts.pop()
    return '::'.join(parts)


def _split(arguments: str) -> list[str]:
    """Top-level comma-separated terms of a cfg argument list."""
    terms, depth, start, quoted = [], 0, 0, False
    for index, char in enumerate(arguments):
        if char == '"':
            quoted = not quoted
        elif not quoted:
            depth += char == '('
            depth -= char == ')'
            if char == ',' and depth == 0:
                terms.append(arguments[start:index])
                start = index + 1
    terms.append(arguments[start:])
    return [' '.join(term.split()) for term in terms if term.strip()]


def _conjuncts(expression: str) -> list[str] | None:
    """The `all(...)` terms of a cfg that requires `test`, else None."""
    expression = expression.strip()
    if expression == 'test':
        return ['test']
    if not (expression.startswith('all(') and expression.endswith(')')):
        return None
    terms = _split(expression[4:-1])
    return terms if 'test' in terms else None


@dataclass
class TestMod:
    path: Path
    start: int      # character span of the cfg attribute, `#` to `]`
    end: int
    line: int       # 1-based line of the attribute
    indent: str
    terms: list[str]
    shared: bool
    module: str     # full module path of the test module
    package: str

    @property
    def named(self) -> list[str]:
        """The group cfgs the attribute names: its own first, then borrowers."""
        return [name for term in self.terms if _GROUP_TERM.fullmatch(term)
                for name in re.findall(r'tests_\w+', term)]


def _attribute_end(text: str, start: int) -> int | None:
    """Index after the `]` closing the attribute whose `#` is at START."""
    depth, quoted = 0, False
    for index in range(start, len(text)):
        char = text[index]
        if char == '"':
            quoted = not quoted
        elif not quoted:
            depth += char in '[('
            depth -= char in ')]'
            if depth == 0 and char == ']':
                return index + 1
    return None


def test_modules(root: Path, crates: dict[str, Crate]) -> list[TestMod]:
    """Every `mod` item behind a cfg that requires `test`, in either crate."""
    owner = {top: crate for crate in crates.values() for top in crate.top_modules}
    roots = {crate_root: package for package, (_, crate_root) in CRATES.items()}
    found = []
    for path in sorted((root / 'src').rglob('*.rs')):
        relative = path.relative_to(root)
        top = relative.parts[1].removesuffix('.rs')
        if relative.as_posix() in roots:
            package, base = roots[relative.as_posix()], ''
        elif top in owner:
            package, base = owner[top].package, module_path(relative)
        else:
            continue
        text = path.read_text(encoding='utf-8')
        for match in _ATTRIBUTE.finditer(text):
            start = match.end(1)
            end = _attribute_end(text, start)
            terms = _conjuncts(text[match.end():end - 2]) if end else None
            if terms is None:
                continue
            # The item follows on this line or after further attributes and comments.
            rest = text[end:]
            line_end = rest.find('\n')
            trailer = rest if line_end < 0 else rest[:line_end]
            position = 0
            while True:
                skipped = re.compile(r'\s*(?://[^\n]*|#\[)').match(rest, position)
                if not skipped:
                    break
                if skipped.group().endswith('#['):
                    position = _attribute_end(rest, skipped.end() - 2) or len(rest)
                else:
                    position = skipped.end()
            name = _MOD.match(rest, position)
            if not name:
                continue
            found.append(TestMod(path, start, end, text.count('\n', 0, start) + 1, match.group(1), terms,
                                 SHARED_MARK in trailer,
                                 f'{base}::{name.group(1)}' if base else name.group(1), package))
    return found


def _render(expression: str, indent: str, room: int) -> str:
    """One cfg expression as rustfmt writes it; ROOM is the columns left on its line."""
    if '(' not in expression:
        return expression
    name, arguments = expression.split('(', 1)
    terms = _split(arguments[:-1])
    flat = f'{name}({", ".join(terms)})'
    if len(', '.join(terms)) <= _ARGUMENTS_WIDTH and len(flat) <= room:
        return flat
    inner = indent + '    '
    lines = [_render(term, inner, _MAX_WIDTH - len(inner) - 1) for term in terms]
    return f'{name}(\n' + ',\n'.join(inner + line for line in lines) + f'\n{indent})'


def attribute(terms: list[str], indent: str) -> str:
    expression = terms[0] if len(terms) == 1 else f'all({", ".join(terms)})'
    return f'#[cfg({_render(expression, indent, _MAX_WIDTH - len(indent) - len("#[cfg()]"))})]'


def group_terms(mod: TestMod, own: str, borrowers: list[str] | None = None) -> list[str]:
    """MOD's cfg terms naming OWN first, then the groups that borrow it."""
    names = [own] + [name for name in (mod.named if borrowers is None else borrowers) if name != own]
    names = list(dict.fromkeys(names))
    term = names[0] if len(names) == 1 else f'any({", ".join(names)})'
    terms = [term for term in mod.terms if not _GROUP_TERM.fullmatch(term)]
    terms.insert(terms.index('test') + 1, term)
    return terms


def problems(root: Path, crates: dict[str, Crate] | None = None) -> list[str]:
    """What keeps a focused build from compiling exactly its group's tests."""
    crates = crates or load(root)
    found = []
    used: dict[str, set[str]] = {package: set() for package in crates}
    for crate in crates.values():
        wanted = {feature(group) for group in crate.groups}
        declared = {name for name in crate.features if name.startswith('tests-')}
        for name in sorted(wanted - declared):
            found.append(f'{crate.package}: feature {name} is missing from [features]')
        for name in sorted(declared - wanted):
            found.append(f'{crate.package}: feature {name} has no [package.metadata.test-groups] entry')
        if wanted and FOCUS_FEATURE not in crate.features:
            found.append(f'{crate.package}: feature {FOCUS_FEATURE} is missing from [features]')
    for mod in test_modules(root, crates):
        crate = crates[mod.package]
        if not crate.prefixes:
            continue
        where = f'{mod.path.relative_to(root).as_posix()}:{mod.line}'
        group = crate.group_of(mod.module)
        if group is None:
            found.append(f'{where}: no test group owns {mod.module}; add a prefix to {CRATES[mod.package][0]}')
            continue
        used[mod.package].add(group)
        known = {cfg_name(name) for name in crate.groups}
        if mod.shared:
            if mod.named:
                found.append(f'{where}: shared test module {mod.module} also names a group')
        elif mod.named[:1] != [cfg_name(group)] or not set(mod.named) <= known:
            found.append(f'{where}: test module {mod.module} belongs to {cfg_name(group)} but names '
                         f'{", ".join(mod.named) or "no group"}; run python -m tools.group_tests tag')
    for crate in crates.values():
        for group in sorted(set(crate.groups) - used[crate.package]):
            found.append(f'{crate.package}: test group {group} owns no test module')
    return found


def rewrite(edits: list[tuple[TestMod, str]]) -> int:
    """Replace each module's cfg attribute with new text; returns how many changed."""
    changed = 0
    by_file: dict[Path, list[tuple[TestMod, str]]] = {}
    for mod, text in edits:
        by_file.setdefault(mod.path, []).append((mod, text))
    for path, items in by_file.items():
        source = path.read_bytes().decode('utf-8')
        result = source
        for mod, text in sorted(items, key=lambda item: -item[0].start):
            changed += result[mod.start:mod.end] != text
            result = result[:mod.start] + text + result[mod.end:]
        if result != source:
            path.write_bytes(result.encode('utf-8'))
    return changed


def tag(root: Path, crates: dict[str, Crate] | None = None) -> int:
    """Write each test module's group cfg; returns how many attributes changed."""
    crates = crates or load(root)
    edits = []
    for mod in test_modules(root, crates):
        crate = crates[mod.package]
        group = crate.group_of(mod.module)
        if group is None or mod.shared:
            continue
        known = {cfg_name(name) for name in crate.groups}
        borrowers = [name for name in mod.named if name in known]
        edits.append((mod, attribute(group_terms(mod, cfg_name(group), borrowers), mod.indent)))
    return rewrite(edits)


def lend(root: Path, module: str, group: str, crates: dict[str, Crate] | None = None) -> None:
    """Let GROUP's focused build compile test module MODULE too; `all` is every test build."""
    crates = crates or load(root)
    found = [mod for mod in test_modules(root, crates) if mod.module == module]
    if len(found) != 1:
        raise ValueError(f'{len(found)} test modules are named {module}; give the full module path')
    mod = found[0]
    crate = crates[mod.package]
    own = crate.group_of(module)
    if own is None or (group != 'all' and group not in crate.groups):
        raise ValueError(f'{crate.package} has no test group for {module if own is None else group}')
    if mod.shared:
        return
    if group == 'all':
        plain = [term for term in mod.terms if not _GROUP_TERM.fullmatch(term)]
        rewrite([(mod, f'{attribute(plain, mod.indent)} {SHARED_MARK}')])
    else:
        rewrite([(mod, attribute(group_terms(mod, cfg_name(own), mod.named + [cfg_name(group)]), mod.indent))])


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('command', choices=('check', 'tag', 'list', 'lend'))
    parser.add_argument('lend', nargs='*', metavar='MODULE GROUP', help='for lend: test module path, group')
    options = parser.parse_args(argv)
    if len(options.lend) != (2 if options.command == 'lend' else 0):
        parser.error('lend takes a test module path and a group; the other commands take nothing')
    root = Path(__file__).resolve().parents[1]
    crates = load(root)
    if options.command == 'lend':
        try:
            lend(root, *options.lend, crates=crates)
        except ValueError as error:
            print(error, file=sys.stderr)
            return 2
    if options.command == 'tag':
        print(f'{tag(root, crates)} test module attributes changed')
    if options.command == 'list':
        counts: dict[tuple[str, str | None], int] = {}
        for mod in test_modules(root, crates):
            group = None if mod.shared else crates[mod.package].group_of(mod.module)
            counts[mod.package, group] = counts.get((mod.package, group), 0) + 1
        for crate in crates.values():
            for group in crate.groups:
                owned = ', '.join(prefix for prefix, name in sorted(crate.prefixes) if name == group)
                print(f'{crate.package:13} {group:22} {counts.get((crate.package, group), 0):5}  {owned}')
            print(f'{crate.package:13} {"(shared)":22} {counts.get((crate.package, None), 0):5}')
        return 0
    found = problems(root, crates)
    for problem in found:
        print(problem, file=sys.stderr)
    return 1 if found else 0


if __name__ == '__main__':
    raise SystemExit(main())
