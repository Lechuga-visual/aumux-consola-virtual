"""Burst keep-alive connections must not starve the embedded console assets."""
import http.client, re, socket, subprocess
from pathlib import Path
root=Path(__file__).resolve().parents[2]
app=subprocess.Popen([str(root/'desktop/target/debug/monitor-mix'),'--no-browser'], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,text=True)
try:
    for line in app.stdout:
        match=re.search(r'http://127\.0\.0\.1:(\d+)',line)
        if match: port=int(match[1]); break
    for attempt in range(12):
        clients=[]
        try:
            # Hold every connection open until every response has arrived. This
            # reproduces browser keep-alive/preload pressure without Chromium.
            for i in range(12): clients.append(socket.create_connection(('127.0.0.1',port),timeout=2))
            for i,client in enumerate(clients):
                path=['/console.js','/tools-ui.js','/workspace-ui.js','/console.css'][i%4]
                client.sendall(f'GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: keep-alive\r\n\r\n'.encode())
            for client in clients:
                response = http.client.HTTPResponse(client)
                response.begin()
                assert response.status == 200, response.status
                assert len(response.read()) > 100
        finally:
            for client in clients: client.close()
    print('PASS: 12 bursts of 12 simultaneous persistent connections deliver all embedded assets without worker starvation')
finally: app.terminate(); app.wait(timeout=5)
