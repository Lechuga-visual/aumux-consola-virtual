"""Exercise the actual app -> UDP worker -> receiver -> libobs audio path."""
import json
import os
import re
import selectors
import subprocess
import time
import urllib.request


def line_until(process, pattern, seconds=5):
    with selectors.DefaultSelector() as sel:
        sel.register(process.stdout, selectors.EVENT_READ)
        # Read bytes directly: avoid Python text buffering hiding later lines.
        data = b""
        import time
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if not sel.select(max(0, end-time.monotonic())):
                break
            chunk = os.read(process.stdout.fileno(), 4096)
            if not chunk:
                break
            data += chunk
            match = re.search(pattern, data.decode(errors="replace"))
            if match:
                return match
        raise RuntimeError(data.decode(errors="replace"))


app = receiver = None
try:
    receiver = subprocess.Popen([os.environ["MONITOR_OBS_TEST"], "--external"], stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    line_until(receiver, r"READY: libobs receiver bound")
    app = subprocess.Popen([os.environ.get("MONITOR_BINARY", "desktop/target/debug/monitor-mix"), "--no-browser"], stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    base = line_until(app, r"http://127\.0\.0\.1:\d+").group(0)
    page = urllib.request.urlopen(base).read().decode()
    token = re.search(r'name="monitor-token" content="([^"]+)"', page).group(1)
    def post(route, body):
        request = urllib.request.Request(base+"/api/"+route, data=json.dumps(body).encode(), headers={"X-Monitor-Token": token, "Content-Type": "application/json"})
        return json.load(urllib.request.urlopen(request))
    config = json.load(urllib.request.urlopen(base+"/api/config"))
    config["obs_bridge"] = True
    for channel in config["channels"]:
        channel["mute"] = True
    config["channels"][0]["mute"] = False
    config["channels"][0]["gain_db"] = 0
    config["channels"][0]["generator"] = {"signal": "sine", "level_db": -20, "frequency": 1000}
    config["buses"][0]["gain_db"] = 0
    post("config", config)
    post("start", {"mode": "demo"})
    # The source returns an acknowledgement only after handing audio to libobs.
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        status = json.load(urllib.request.urlopen(base+"/api/status"))
        if status["bridge"]["receiver_active"]:
            break
        time.sleep(0.02)
    assert status["bridge"]["receiver_active"], status
    assert status["bridge"]["sent"] > 0, status
    output = receiver.communicate(timeout=10)[0].decode(errors="replace")
    assert receiver.returncode == 0, output
    assert "PASS: app master reached actual libobs capture callbacks" in output, output
    post("stop", {})
    print("PASS: generated input -> app mixer -> bounded bridge queue -> UDP -> actual libobs audio capture")
finally:
    for process in [app, receiver]:
        if process and process.poll() is None:
            process.terminate()
            process.communicate(timeout=3)
