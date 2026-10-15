"""End-to-end LAN PCM checks against the real app, with no audio hardware."""
import json,os,re,struct,subprocess,time,urllib.request,urllib.error,concurrent.futures,math
from pathlib import Path
root=Path(__file__).resolve().parents[2]
app=subprocess.Popen([str(root/'desktop/target/debug/aumux'),'--no-browser'],stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True,env={**os.environ,'MONITOR_LISTEN_HOST':'127.0.0.1'})
streams=[]
try:
    url=''
    for line in app.stdout:
        m=re.search(r'http://127\.0\.0\.1:\d+',line)
        if m:url=m[0];break
    assert url
    page=urllib.request.urlopen(url).read().decode();token=re.search(r'name="monitor-token" content="([^"]+)"',page)[1]
    def api(route,data):
        req=urllib.request.Request(url+'/api/'+route,data=json.dumps(data).encode(),headers={'Content-Type':'application/json','X-Monitor-Token':token})
        return json.load(urllib.request.urlopen(req,timeout=5))
    cfg=json.load(urllib.request.urlopen(url+'/api/config'))
    for c in cfg['channels']:c['mute']=True
    cfg['channels'][0].update(mute=False,gain_db=0,generator={'signal':'sine','level_db':-20,'frequency':1000})
    cfg['channels'][0]['sends_db'][0]=-6
    for b in cfg['buses']:b['gain_db']=0
    api('config',cfg);api('start',{'mode':'demo'});network=api('listen',{'enabled':True,'port':0});link=network['url'];origin=link.split('/?')[0];key=link.split('key=')[1]
    for path,expected in [('/',403),('/api/config?key='+key,404)]:
        try:urllib.request.urlopen(origin+path);raise AssertionError('Read-only monitor access check failed')
        except urllib.error.HTTPError as e:assert e.code==expected
    assert b'AuMux' in urllib.request.urlopen(link).read()
    metadata=json.load(urllib.request.urlopen(origin+'/status?key='+key));assert len(metadata['buses'])==5 and metadata['sample_rate']==48000
    def record(bus):
        stream=urllib.request.urlopen(origin+'/stream?key='+key+'&bus='+str(bus),timeout=5);streams.append(stream);raw=stream.read(48000*8//4);stream.close();values=struct.unpack('<'+'f'*(len(raw)//4),raw);assert all(math.isfinite(v) for v in values);return math.sqrt(sum(v*v for v in values)/len(values))
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:main,aux=list(pool.map(record,[0,1]))
    assert .06<main<.075,(main,aux)
    assert .48<aux/main<.52,(main,aux)
    api('panic',{});time.sleep(.04)
    stream=urllib.request.urlopen(origin+'/stream?key='+key+'&bus=0',timeout=5);streams.append(stream);values=struct.unpack('<'+'f'*960,stream.read(3840));assert all(v==0 for v in values);stream.close()
    api('resume',{});api('stop',{});api('listen',{'enabled':False});assert not json.load(urllib.request.urlopen(url+'/api/status'))['listen']['enabled']
    print('PASS: LAN stereo float32 at 48 kHz, simultaneous main/aux separation, silence/resume, metadata, capability link and read-only isolation')
finally:
    for stream in streams:stream.close()
    app.terminate();app.wait(timeout=5)
