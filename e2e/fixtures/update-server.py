#!/usr/bin/env python3
"""Stand-in for the release server, for tests of the in-app updater.

    update-server.py <directory> <port-file> <log-file>

Serves the files in <directory> on a free port of 127.0.0.1, writes the port to <port-file>
once it listens, and appends one JSON line per request to <log-file>: the method, the path
with its query, and every header. The log is how a test knows what the app sent.
"""
import functools
import http.server
import json
import sys


class Handler(http.server.SimpleHTTPRequestHandler):
    log_path = ""

    def log_message(self, *_):
        pass

    def do_GET(self):
        with open(self.log_path, "a", encoding="utf-8") as log:
            log.write(json.dumps({"method": self.command, "path": self.path, "headers": dict(self.headers.items())}) + "\n")
        super().do_GET()


def main():
    directory, port_file, Handler.log_path = sys.argv[1:4]
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Handler, directory=directory))
    with open(port_file, "w", encoding="utf-8") as f:
        f.write(str(server.server_address[1]))
    server.serve_forever()


if __name__ == "__main__":
    main()
