"""Speak errand's first two ACP requests to a session that runs `kage rpc`.

argv: the full command line that starts the session. Prints one line per
answer and exits 0 when both requests got a result, 1 otherwise. Used by
live.sh; run with `python3 -I`.
"""
import json
import subprocess
import sys
import time

p = subprocess.Popen(sys.argv[1:], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def ask(i, method, params):
    p.stdin.write((json.dumps({"jsonrpc": "2.0", "id": i, "method": method, "params": params}) + "\n").encode())
    p.stdin.flush()
    while True:
        line = p.stdout.readline()
        if not line:
            return None
        m = json.loads(line)
        if m.get("id") == i:
            return m


t0 = time.time()
r1 = ask(1, "initialize", {"protocolVersion": 1, "clientCapabilities": {}})
print("initialize: %s (%.2f s)" % ("result" if r1 and "result" in r1 else json.dumps(r1)[:200], time.time() - t0))
r2 = ask(2, "session/new", {"cwd": "/workspace", "mcpServers": []}) if r1 else None
print("session/new: %s (%.2f s)" % ("result" if r2 and "result" in r2 else json.dumps(r2)[:200], time.time() - t0))
p.stdin.close()
try:
    rc = p.wait(timeout=30)
except subprocess.TimeoutExpired:
    p.terminate()
    rc = p.wait()
print("launcher exit: %d (%.2f s)" % (rc, time.time() - t0))
ok = bool(r1 and "result" in r1 and r2 and "result" in r2)
if not ok:
    print("stderr: " + p.stderr.read().decode(errors="replace")[-400:])
sys.exit(0 if ok else 1)
