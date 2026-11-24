"""Cargo test runner that records each test executable instead of starting it.

tools.cargo_run sets this as Cargo's target runner for `cargo test`, so Cargo
hands it every test executable with that test's arguments, environment and
working directory. It appends one JSON line per call to the record file and
reports success; cargo_run runs the recorded calls after releasing the build
lock. Standard library only.

    python _cargo_test_record.py RECORD EXECUTABLE [ARGS...]
"""
import json
import os
import sys

record, *command = sys.argv[1:]
with open(record, 'a', encoding='utf-8') as handle:
    handle.write(json.dumps({'command': command, 'cwd': os.getcwd(), 'env': dict(os.environ)}) + '\n')
