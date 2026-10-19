"""Real app UDP routing: multi-destination, broadcast, bus isolation, mute, validation and libobs."""
import cmath
import copy
import json
import math
import os
import re
import select
import socket
import struct
import subprocess
import time
import urllib.request

app = subprocess.Popen(['desktop/target/debug/aumux', '--no-browser'], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
sockets = []
receiver = None
try:
    origin = None
    for line in app.stdout:
        match = re.search(r'http://127\.0\.0\.1:\d+', line)
        if match:
            origin = match[0]; break
    assert origin, 'app startup failed'
    html = urllib.request.urlopen(origin, timeout=5).read().decode()
    token = re.search(r'name="monitor-token" content="([^"]+)"', html)[1]
    def api(route, body=None):
        request = urllib.request.Request(origin+'/api/'+route, data=json.dumps(body).encode() if body is not None else None,
            headers={'Content-Type':'application/json', 'X-Monitor-Token':token, 'Origin':origin})
        with urllib.request.urlopen(request, timeout=8) as response: return json.load(response)
    def bind(host='127.0.0.1', port=0):
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.bind((host, port)); s.setblocking(False); sockets.append(s); return s
    first = bind(); second = bind('127.0.0.2', first.getsockname()[1]); broadcast = bind('0.0.0.0')
    def route(name, bus, port, destinations, address='127.0.0.1'):
        return {'enabled':True, 'bus':bus, 'stream':name, 'bind':address, 'port':port, 'destinations':destinations}
    cfg = api('config')
    for ch in cfg['channels']:
        ch['generator']['signal']='off'; ch['input']=None; ch['gain_db']=-90; ch['sends_db']=[-90]*4
    cfg['channels'][0]['generator']={'signal':'sine','level_db':-12,'frequency':440}; cfg['channels'][0]['gain_db']=0
    cfg['channels'][1]['generator']={'signal':'sine','level_db':-18,'frequency':880}; cfg['channels'][1]['sends_db'][0]=0
    cfg['buses'][0]['gain_db']=0; cfg['buses'][1]['gain_db']=0
    cfg['audio_network']=[route('MAIN',0,first.getsockname()[1],['localhost','127.0.0.2']),route('IEM',1,first.getsockname()[1],['127.0.0.1']),route('BC',0,broadcast.getsockname()[1],['127.255.255.255'])]
    api('config',cfg); api('start',{'mode':'demo'})
    def receive(s):
        p=s.recv(2048); assert p[:4]==b'AUM1' and len(p)<1200
        assert p[4:8]==bytes([1,2,80,0]); assert struct.unpack_from('<I',p,20)[0]==48000
        frames=struct.unpack_from('<H',p,24)[0]; assert len(p)==80+frames*8
        name=p[40:72].split(b'\0')[0].decode(); data=struct.unpack_from('<'+'f'*(frames*2),p,80)
        assert all(math.isfinite(v) and abs(v)<=1 for v in data)
        return name,data[::2]
    values={'MAIN':[],'IEM':[]}; second_names=set(); bc=False; deadline=time.monotonic()+4
    while time.monotonic()<deadline and (min(map(len,values.values()))<4096 or not bc or not second_names):
        ready,_,_=select.select(sockets,[],[],.1)
        for s in ready:
            name,data=receive(s)
            if s is first: values[name].extend(data)
            elif s is second: second_names.add(name)
            else: assert name=='BC'; bc=True
    assert second_names=={'MAIN'} and bc
    def magnitude(values,f):return abs(sum(v*cmath.exp(-2j*math.pi*f*i/48000) for i,v in enumerate(values[-4096:])))
    assert magnitude(values['MAIN'],440)>10*magnitude(values['MAIN'],880),'main bus isolation'
    assert magnitude(values['IEM'],880)>10*magnitude(values['IEM'],440),'aux bus isolation'
    # An invalid interface must report its error without stopping existing streams.
    bad=route('BAD',0,first.getsockname()[1],['127.0.0.1'],'192.0.2.1'); cfg['audio_network'].append(bad);api('config',cfg)
    deadline=time.monotonic()+3
    while time.monotonic()<deadline:
        status=api('status')['audio_network']
        if status[-1]['error'] and 'Preparando' not in status[-1]['error']:break
        time.sleep(.02)
    assert status[0]['enabled'] and status[-1]['error'] and not status[-1]['enabled']
    cfg['audio_network'].pop();api('config',cfg)
    api('panic',{}); time.sleep(.08)
    for s in sockets:
        while True:
            try:s.recv(2048)
            except BlockingIOError:break
    silent=[]; deadline=time.monotonic()+2
    while len(silent)<512 and time.monotonic()<deadline:
        if select.select([first],[],[],.1)[0]:silent.extend(receive(first)[1])
    assert silent and max(map(abs,silent))<1e-7,'emergency silence reaches network'
    api('resume',{})
    # Actual OBS plugin receives an app-generated named auxiliary.
    executable=os.environ.get('AUMUX_OBS_NETWORK_TEST')
    if executable:
        probe=bind(); port=probe.getsockname()[1];probe.close();sockets.remove(probe)
        receiver=subprocess.Popen([executable,str(port),'OBS-IEM'],stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True)
        while True:
            line=receiver.stdout.readline()
            assert line,'receiver failed before readiness'
            if line.startswith('READY:'):break
        cfg['audio_network'].append(route('OBS-IEM',1,port,['127.0.0.1']));api('config',cfg)
        output=receiver.communicate(timeout=7)[0];assert receiver.returncode==0,output
        assert 'PASS: app bus' in output
    else: raise AssertionError('AUMUX_OBS_NETWORK_TEST is required for the actual libobs integration')
    print('PASS: app PCM48k bus isolation, identical multi-destination streams, loopback broadcast, hostname resolution, bounded datagrams, interface errors, emergency silence/resume and named aux -> actual libobs capture; physical LAN/Tailscale not measured.')
finally:
    if receiver and receiver.poll() is None:receiver.terminate();receiver.wait(timeout=5)
    for s in sockets:s.close()
    app.terminate();app.wait(timeout=5)
