"""Real app WebSocket: persistent aux control, authorization and reconnection.

Requires Python websockets >= 12 (sync client); no special audio hardware.
"""
import concurrent.futures
import http.cookiejar as cookiejar
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import time
import urllib.error
import urllib.request

from websockets.exceptions import ConnectionClosed, InvalidStatus
from websockets.sync.client import connect

root = Path(__file__).resolve().parents[2]
binary = os.environ.get("AUMUX_TEST_BINARY") or str(root / "desktop/target/debug/aumux")
if not Path(binary).exists():
    binary = str(root / "desktop/target/debug/monitor-mix")
app = subprocess.Popen(
    [binary, "--no-browser"], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
    text=True, env={**os.environ, "MONITOR_REMOTE_BIND": "127.0.0.1", "MONITOR_LISTEN_HOST": "127.0.0.1"},
)
try:
    for line in app.stdout:
        match = re.search(r"http://127\.0\.0\.1:\d+", line)
        if match:
            url = match[0]
            break
    else:
        raise AssertionError("App did not announce its loopback console")
    token = re.search(r'name="(?:monitor|aumux)-token" content="([^"]+)"', urllib.request.urlopen(url).read().decode())[1]

    def http(link, data=None, headers=None, opener=None):
        request = urllib.request.Request(link, data=None if data is None else json.dumps(data).encode(), headers={"Content-Type": "application/json", **(headers or {})})
        fetch = opener.open if opener else urllib.request.urlopen
        with fetch(request, timeout=5) as response:
            return json.load(response)

    def api(route, data):
        return http(url + "/api/" + route, data, {"X-Monitor-Token": token})

    api("remote", {"action": "start", "port": 0})
    api("network-host", {"host": "127.0.0.1"})
    admin = api("remote", {"action": "create", "label": "Voz", "aux": 1, "max_db": 0})
    personal = admin["sessions"][0]
    key = personal["url"].split("key=")[1]
    origin = f'http://127.0.0.1:{admin["port"]}'
    socket_url = origin.replace("http://", "ws://") + "/mix/ws"
    time.sleep(.1)

    def ws(cookie=None, supplied_origin=origin):
        return connect(socket_url, origin=supplied_origin, additional_headers={"Cookie": cookie} if cookie else None, open_timeout=3, close_timeout=1, max_size=65536, proxy=None)

    def receive(connection, kind=None, seq=None, timeout=3):
        deadline = time.monotonic() + timeout
        while True:
            message = json.loads(connection.recv(timeout=max(.01, deadline - time.monotonic())))
            if (kind is None or message["type"] == kind) and (seq is None or message.get("seq") == seq):
                return message
            assert time.monotonic() < deadline, (kind, message)

    def hello(connection, supplied_key=key, aux=None):
        connection.send(json.dumps({"type": "hello", "key": supplied_key, **({"aux": aux} if aux is not None else {})}))
        return receive(connection)

    with ws() as connection:
        try:
            connection.recv(timeout=.2)
            raise AssertionError("State leaked before hello")
        except TimeoutError:
            pass
        state = hello(connection)["state"]
        assert state["bus"]["index"] == 1 and "buses" not in state
        epoch = state["epoch"]
        connection.send(json.dumps({"type": "change", "seq": 1, "epoch": epoch, "changes": [{"channel": 0, "level_db": -12}, {"channel": 1, "level_db": -6}]}))
        ack = receive(connection, "ack", 1)
        assert ack["state"]["channels"][0]["level_db"] == -12 and ack["state"]["channels"][1]["level_db"] == -6
        connection.send(json.dumps({"type": "change", "seq": 2, "epoch": epoch, "changes": [{"channel": 0, "level_db": -20}, {"channel": 1, "level_db": 6}]}))
        assert receive(connection, "error", 2)["code"] == "invalid"
        assert http(url + "/api/config")["channels"][0]["sends_db"][0] == -12
        connection.send(json.dumps({"type": "change", "seq": 3, "epoch": epoch, "aux": 2, "changes": [{"channel": 0, "level_db": -20}]}))
        assert receive(connection, "error", 3)["code"] == "invalid"
        connection.send(json.dumps({"type": "change", "seq": 1, "epoch": epoch, "changes": [{"channel": 0, "level_db": -20}]}))
        assert receive(connection, "error", 1)["code"] == "invalid"
        received = 0
        started = time.monotonic()
        while time.monotonic() - started < 6.2:
            assert receive(connection, "state")["state"]["bus"]["index"] == 1
            received += 1
        assert received >= 45, received
        sent = time.monotonic()
        connection.send(json.dumps({"type": "ping", "sent": sent}))
        assert receive(connection, "pong")["sent"] == sent
    with ws() as connection:
        assert hello(connection)["state"]["channels"][0]["level_db"] == -12

    try:
        with ws(supplied_origin="http://unrelated.invalid"):
            raise AssertionError("Cross-origin WebSocket accepted")
    except InvalidStatus as error:
        assert error.response.status_code == 403
    with ws() as connection:
        assert hello(connection, "invalid-key")["code"] == "auth"

    # Persistent clients independently control different auxiliaries.
    admin = api("remote", {"action": "create", "label": "Batería", "aux": 2, "max_db": 0})
    second_key = admin["sessions"][-1]["url"].split("key=")[1]
    timings = []
    def moves(values):
        session_key, value = values
        with ws() as connection:
            state = hello(connection, session_key)["state"]
            for seq in range(1, 16):
                started = time.monotonic()
                connection.send(json.dumps({"type": "change", "seq": seq, "epoch": state["epoch"], "changes": [{"channel": 0, "level_db": value}]}))
                receive(connection, "ack", seq)
                timings.append((time.monotonic() - started) * 1000)
                time.sleep(.08)
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        list(pool.map(moves, [(key, -9), (second_key, -18)]))
    assert http(url + "/api/config")["channels"][0]["sends_db"][:2] == [-9, -18]

    with ws() as connection:
        hello(connection)
        api("remote", {"action": "revoke", "id": personal["id"]})
        assert receive(connection, "error")["code"] == "auth"

    # Shared password cookies authorize only their role and remain revocable.
    api("remote", {"action": "access", "scope": "mix", "mode": "password", "password": "prueba"})
    with ws() as connection:
        assert hello(connection, "", 2)["code"] == "auth"
    jar = cookiejar.CookieJar()
    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
    assert http(origin + "/login/mix", {"password": "prueba"}, opener=opener)["ok"]
    cookie = "; ".join(f"{item.name}={item.value}" for item in jar)
    with ws(cookie=cookie) as connection:
        assert hello(connection, "", 2)["state"]["bus"]["index"] == 2
        api("remote", {"action": "access", "scope": "mix", "mode": "password", "password": "nueva"})
        assert receive(connection, "error")["code"] == "auth"
    api("remote", {"action": "access", "scope": "mix", "mode": "open"})
    with ws() as connection:
        state = hello(connection, "", 2)["state"]
        config = http(url + "/api/config")
        config["channels"].pop()
        api("config", config)
        assert receive(connection, "error")["code"] == "topology"
    api("remote", {"action": "stop"})
    print(f"PASS: WebSocket authorization/Origin, no state before hello, atomic batches, aux isolation, sequence checks, persistent snapshots >6s, ping RTT, concurrent clients, reconnection, password revocation/topology; ACK loopback median {statistics.median(timings):.1f} ms / max {max(timings):.1f} ms (not audio latency)")
finally:
    app.terminate()
    app.wait(timeout=5)
