"""Real TCP fragmentation/delays and port collisions; no physical Wi-Fi required."""
import json, os, re, socket, subprocess, time, urllib.request, urllib.error
from pathlib import Path
root = Path(__file__).resolve().parents[2]
app = subprocess.Popen([str(root/'desktop/target/debug/monitor-mix'), '--no-browser'], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, env={**os.environ, 'MONITOR_REMOTE_BIND':'127.0.0.1', 'MONITOR_LISTEN_HOST':'127.0.0.1'})
try:
    for line in app.stdout:
        match = re.search(r'http://127\.0\.0\.1:\d+', line)
        if match: local = match[0]; break
    token = re.search(r'name="monitor-token" content="([^"]+)"', urllib.request.urlopen(local).read().decode())[1]
    def api(route, data, expected=200):
        req = urllib.request.Request(local+'/api/'+route, data=json.dumps(data).encode(), headers={'Content-Type':'application/json','X-Monitor-Token':token,'Origin':local})
        try:
            with urllib.request.urlopen(req, timeout=10) as r: code, raw = r.status, r.read()
        except urllib.error.HTTPError as e: code, raw = e.code, e.read()
        assert code == expected, (code, raw)
        return json.loads(raw)
    remote = api('remote', {'action':'start','port':0})
    port = remote['port']
    api('remote', {'action':'access','scope':'mix','mode':'open'})
    with socket.create_connection(('127.0.0.1',port),timeout=8) as s:
        s.sendall(f'GET /mix?aux=1 HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n'.encode())
        time.sleep(2.2) # Used to expire at 2 seconds, even before a valid request finished.
        s.sendall(b'\r\n')
        with s.makefile('rb') as stream:
            response = stream.read()
        assert b'200 Response' in response and b'"channels"' in response, response[:300]
    with socket.create_connection(('127.0.0.1',port),timeout=8) as s:
        s.sendall(f'POST /mix?aux=1 HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\n\r\n'.encode())
        with s.makefile('rb') as stream: response = stream.read()
        assert b'400 Response' in response and b'"error"' in response, response
    for route, data in [('remote', {'action':'start','port':port}), ('listen', {'enabled':True,'port':port})]:
        # Remote restart releases its own socket, so reserve an unrelated socket.
        with socket.socket() as reserved:
            reserved.bind(('127.0.0.1',0)); reserved.listen()
            data['port'] = reserved.getsockname()[1]
            error = api(route, data, 400)['error']
            assert str(data['port']) in error and 'otra instancia' in error, error
    # Failed starts cannot advertise a running service belonging to another process.
    status = json.load(urllib.request.urlopen(local+'/api/status'))
    assert not status['remote']['enabled'] and not status['listen']['enabled']
    print('PASS: fragmented TCP headers delayed beyond two seconds, HTTP body preservation (Rust unit test), explicit occupied-port errors and disabled status after failed binds')
finally:
    app.terminate(); app.wait(timeout=5)
