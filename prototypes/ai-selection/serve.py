#!/usr/bin/env python3
"""Run the throwaway FilmCraft UI prototype; serve only this folder on localhost."""

import argparse
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import re


ROOT = Path(__file__).resolve().parent


class PrototypeHandler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(ROOT), **kwargs)

    def send_head(self):
        self.remaining = None
        path = Path(self.translate_path(self.path)).resolve()
        if not path.is_relative_to(ROOT):
            self.send_error(403, "Only the prototype folder is served")
            return None
        if path.is_dir():
            for name in ("index.html", "index.htm"):
                index = path / name
                if index.is_file() and not index.resolve().is_relative_to(ROOT):
                    self.send_error(403, "Only the prototype folder is served")
                    return None
        requested = self.headers.get("Range")
        if not requested or not path.is_file():
            return super().send_head()
        size = path.stat().st_size
        match = re.fullmatch(r"bytes=(\d*)-(\d*)", requested.strip())
        start, end = 0, size - 1
        if match and any(match.groups()):
            first, last = match.groups()
            if first:
                start = int(first)
                end = min(int(last), size - 1) if last else size - 1
            else:
                start = max(0, size - int(last))
        else:
            start = size
        if start > end or start >= size:
            self.send_response(416)
            self.send_header("Content-Range", f"bytes */{size}")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return None
        stream = path.open("rb")
        stream.seek(start)
        self.remaining = end - start + 1
        self.send_response(206)
        self.send_header("Content-Type", self.guess_type(str(path)))
        self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        self.send_header("Content-Length", str(self.remaining))
        self.send_header("Last-Modified", self.date_time_string(path.stat().st_mtime))
        self.end_headers()
        return stream

    def end_headers(self):
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def copyfile(self, source, outputfile):
        if self.remaining is None:
            return super().copyfile(source, outputfile)
        while self.remaining > 0:
            data = source.read(min(65536, self.remaining))
            if not data:
                break
            outputfile.write(data)
            self.remaining -= len(data)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=8791)
    args = parser.parse_args()
    with ThreadingHTTPServer(("127.0.0.1", args.port), PrototypeHandler) as server:
        print(f"FilmCraft prototype: http://127.0.0.1:{args.port}/?variant=A", flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass


if __name__ == "__main__":
    main()
