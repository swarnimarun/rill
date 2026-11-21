#!/usr/bin/env python3
"""Forward a test editor's stdio while recording complete LSP frames."""
import json
import os
import subprocess
import sys
import threading


lock = threading.Lock()


def forward(reader, writer, direction):
    buffered = b""
    while True:
        chunk = os.read(reader, 65536)
        if not chunk:
            break
        buffered += chunk
        while b"\r\n\r\n" in buffered:
            header, body = buffered.split(b"\r\n\r\n", 1)
            length = next(int(line.split(b":", 1)[1]) for line in header.split(b"\r\n") if line.lower().startswith(b"content-length:"))
            if len(body) < length:
                break
            message = json.loads(body[:length])
            buffered = body[length:]
            with lock:
                with open(os.environ["RILL_LSP_TRACE"], "a") as output:
                    output.write(json.dumps({"direction": direction, "server_pid": process.pid, "message": message}) + "\n")
        remaining = memoryview(chunk)
        while remaining:
            remaining = remaining[os.write(writer, remaining):]


process = subprocess.Popen([os.environ["RILL_REAL_SERVER"], *sys.argv[1:]], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
with open(os.environ["RILL_LSP_TRACE"], "a") as output:
    output.write(json.dumps({"direction": "proxy", "message": {"method": "$test/serverPid", "params": {"pid": process.pid}}}) + "\n")
def client_input():
    try:
        forward(sys.stdin.fileno(), process.stdin.fileno(), "client")
    finally:
        process.stdin.close()


incoming = threading.Thread(target=client_input, daemon=True)
incoming.start()
try:
    forward(process.stdout.fileno(), sys.stdout.fileno(), "server")
    status = process.wait(timeout=5)
    incoming.join(timeout=1)
    sys.exit(status)
finally:
    if process.poll() is None:
        process.kill()
        process.wait()
