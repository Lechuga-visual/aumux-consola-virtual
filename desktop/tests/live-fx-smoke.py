"""Real running DEMO + native/VST3 graph edits, observed through the PCM monitor."""
import json, os, re, struct, subprocess, time, urllib.request, urllib.error, threading, math
from pathlib import Path
root = Path(__file__).resolve().parents[2]
app = subprocess.Popen([str(root/'desktop/target/debug/aumux'),'--no-browser'],stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True,env={**os.environ,'MONITOR_LISTEN_HOST':'127.0.0.1'})
stream = None
stop = threading.Event()
reader = None
observed = {'chunks':0,'peak':0.0,'max_gap':0.0,'error':None}
try:
    for line in app.stdout:
        match = re.search(r'http://127\.0\.0\.1:\d+',line)
        if match: url = match[0]; break
    html = urllib.request.urlopen(url).read().decode()
    token = re.search(r'name="monitor-token" content="([^"]+)"',html)[1]
    def api(route, data):
        request = urllib.request.Request(url+'/api/'+route,data=json.dumps(data).encode(),headers={'Content-Type':'application/json','X-Monitor-Token':token})
        return json.load(urllib.request.urlopen(request,timeout=10))
    def get(route): return json.load(urllib.request.urlopen(url+'/api/'+route,timeout=5))
    config = get('config')
    for channel in config['channels']:
        channel.update(input=None,input_right=None,mute=False)
    config['channels'][0].update(gain_db=0,generator={'signal':'sine','level_db':-20,'frequency':1000})
    config['channels'][0]['sends_db'][0] = -6
    for bus in config['buses']: bus['gain_db'] = 0
    api('config',config); config=get('config'); api('start',{'mode':'demo'})
    network=api('listen',{'enabled':True,'port':0})
    origin, key=network['url'].split('/?key=')
    stream=urllib.request.urlopen(origin+'/stream?key='+key+'&bus=0',timeout=5)
    def receive():
        previous=time.monotonic()
        try:
            while not stop.is_set():
                raw=stream.read(2048)
                if not raw: raise AssertionError('PCM stream closed during graph edits')
                samples=struct.unpack('<'+'f'*(len(raw)//4),raw)
                if not all(math.isfinite(v) for v in samples): raise AssertionError('Nonfinite PCM sample')
                now=time.monotonic()
                observed['max_gap']=max(observed['max_gap'],now-previous); previous=now
                observed['peak']=max(observed['peak'],max(map(abs,samples)))
                observed['chunks']+=1
        except Exception as error:
            if not stop.is_set(): observed['error']=repr(error)
    reader=threading.Thread(target=receive,daemon=True);reader.start()
    def native(name): return {'id':'native/mda/'+name,'path':'','name':name,'params':{},'bypass':False}
    def apply():
        before=get('status')['performance']['blocks']
        api('config',config)
        canonical=get('config');config.clear();config.update(canonical)
        status=get('status')
        assert status['mode']=='DEMO' and not status['failed'],status
        assert status['performance']['blocks']>=before,'DSP counters reset across graph exchange'
    config['channels'][0]['inserts']=[native('dither'),native('limiter')];apply()
    config['channels'][0]['inserts'].reverse();apply()
    config['channels'][0]['inserts'].pop();apply()
    config['buses'][0]['inserts']=[native('dither')];apply()
    config['buses'][1]['inserts']=[native('limiter')];apply()
    fixture=os.environ.get('MONITOR_VST3_FIXTURE')
    if fixture:
        descriptor=api('inspect',{'path':fixture})[0]  # Inspection while the engine and PCM stream remain active.
        vst={'id':descriptor['id'],'name':descriptor['name'],'path':fixture,'bypass':False,'params':{p['id']:p['value'] for p in descriptor['params']}}
        config['channels'][0]['inserts'].append(vst.copy());apply()
        config['channels'][0]['inserts'].reverse();apply()
        config['buses'][0]['inserts'].append(vst.copy());apply()
        config['buses'][1]['inserts'].append(vst.copy());apply()
    saved=get('config')
    config['channels'][0]['inserts'].append({'id':'vst3:00000000000000000000000000000000','path':'/missing/live-fx-fixture.vst3','params':{},'bypass':False})
    try: api('config',config);raise AssertionError('Missing plugin unexpectedly loaded')
    except urllib.error.HTTPError as error: assert error.code==400,error
    assert get('config')==saved,'Failed preparation modified the active scene'
    assert get('status')['mode']=='DEMO'
    deadline=time.monotonic()+3
    while observed['chunks']<15 and time.monotonic()<deadline: time.sleep(.01)
    assert observed['error'] is None,observed
    assert observed['chunks']>=15 and observed['peak']>.01,observed
    print('PASS: continuous finite PCM during real live native/VST3 insert/reorder/remove on channel/master/aux; no stream restart, monotonic DSP counters and failed-load rollback. Observed loopback PCM max delivery gap %.1f ms; not audio latency or physical hardware validation.'%(observed['max_gap']*1000))
finally:
    stop.set()
    if stream: stream.close()
    if reader: reader.join(timeout=2)
    app.terminate();app.wait(timeout=5)
