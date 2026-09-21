#!/usr/bin/env python3
"""Serve the current browser build, always fetching a fresh entry page."""
import argparse
import json
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit


class PreviewHandler(SimpleHTTPRequestHandler):
    def entry_page(self):
        return urlsplit(self.path).path in ('/', '/index.html', '/build.json')

    def send_head(self):
        self.build_mismatch = False
        try:
            current = json.loads((Path(self.directory) / 'build.json').read_text())['build']
        except (OSError, ValueError, KeyError):
            self.send_error(503, 'The browser build is unavailable')
            return None
        requested = self.headers.get('X-Hookrunner-Build')
        parts = urlsplit(self.path).path.split('/')
        if len(parts) >= 3 and parts[1] == 'pkg':
            requested = parts[2]
        self.build_mismatch = requested is not None and requested != current
        if self.entry_page():
            # Even rapid rebuilds with equal timestamp precision must load new URLs.
            for header in ('If-Modified-Since', 'If-None-Match'):
                if header in self.headers:
                    del self.headers[header]
        return super().send_head()

    def end_headers(self):
        self.send_header('Cache-Control', 'no-store' if self.entry_page() else 'no-cache')
        if getattr(self, 'build_mismatch', False):
            self.send_header('Clear-Site-Data', '"cache"')
        super().end_headers()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bind', default='127.0.0.1')
    parser.add_argument('--port', type=int, default=8080)
    args = parser.parse_args()
    directory = Path(__file__).resolve().parents[1] / 'dist'
    if not all((directory / name).is_file() for name in ('index.html', 'build.json')):
        parser.error('Build the browser client first: ./scripts/build-web.sh')
    handler = partial(PreviewHandler, directory=str(directory))
    with ThreadingHTTPServer((args.bind, args.port), handler) as server:
        print(f'Hookrunner preview: http://{args.bind}:{server.server_port}', flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass


if __name__ == '__main__':
    main()
