#!/usr/bin/env python3
"""Serve a browser package locally with the cross-origin isolation headers that
a --threads package needs for shared Wasm memory."""
import argparse
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class IsolatedHandler(SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header('Cross-Origin-Opener-Policy', 'same-origin')
        self.send_header('Cross-Origin-Embedder-Policy', 'require-corp')
        super().end_headers()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port', type=int, default=8000)
    parser.add_argument('--directory', type=Path, default=ROOT / 'target/wasm-dist')
    args = parser.parse_args()
    server = ThreadingHTTPServer(('127.0.0.1', args.port), partial(IsolatedHandler, directory=str(args.directory)))
    print(f'Serving {args.directory} at http://localhost:{args.port}')
    server.serve_forever()


if __name__ == '__main__':
    main()
