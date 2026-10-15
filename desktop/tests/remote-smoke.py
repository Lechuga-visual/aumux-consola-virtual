"""Real app: aux capabilities, concurrency, measured PCM, and advertised tailnet addresses."""
import concurrent.futures,json,math,os,re,struct,subprocess,time,urllib.error,urllib.request,statistics
from pathlib import Path
root=Path(__file__).resolve().parents[2]
app=subprocess.Popen([str(root/'desktop/target/debug/aumux'),'--no-browser'],stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True,env={**os.environ,'MONITOR_LISTEN_HOST':'127.0.0.1','MONITOR_REMOTE_BIND':'127.0.0.1'})
try:
    for line in app.stdout:
        m=re.search(r'http://127\.0\.0\.1:\d+',line)
        if m:url=m[0];break
    token=re.search(r'name="monitor-token" content="([^"]+)"',urllib.request.urlopen(url).read().decode())[1]
    def http(link,data=None,headers=None,expected=200):
        req=urllib.request.Request(link,data=None if data is None else json.dumps(data).encode(),headers={'Content-Type':'application/json',**(headers or {})})
        try:
            with urllib.request.urlopen(req,timeout=3) as r:raw=r.read();code=r.status
        except urllib.error.HTTPError as e:raw=e.read();code=e.code
        assert code==expected,(code,expected,raw)
        try:return json.loads(raw)
        except ValueError:return raw
    def api(route,data):return http(url+'/api/'+route,data,{'X-Monitor-Token':token})
    c=http(url+'/api/config')
    for ch in c['channels']:ch['mute']=True
    c['channels'][0].update(mute=False,gain_db=0,generator={'signal':'sine','level_db':-20,'frequency':1000})
    c['channels'][0]['sends_db'][:2]=[-6,-12]
    for b in c['buses']:b['gain_db']=0
    api('config',c);api('start',{'mode':'demo'})
    net=api('listen',{'enabled':True,'port':0});audio=net['url'].split('/?')[0];audio_key=net['url'].split('key=')[1]
    admin=api('remote',{'action':'start','port':0});port=admin['port'];api('network-host',{'host':'127.0.0.1'})
    api('remote',{'action':'create','label':'Voz','aux':1,'max_db':0})
    admin=api('remote',{'action':'create','label':'Bateria','aux':2,'max_db':-6,'allow_pre':True});a,b=admin['sessions'];origin=f'http://127.0.0.1:{port}'
    def key(s):return s['url'].split('key=')[1]
    def view(s):return http(origin+'/mix',headers={'X-Mix-Key':key(s)})
    def change(s,values,expected=200,headers=None):return http(origin+'/mix',{'epoch':view(s)['epoch'],'channel':0,**values},{'X-Mix-Key':key(s),**(headers or {})},expected)
    va=view(a);assert va['bus']['index']==1 and len(va['channels'])==8 and 'buses' not in va and 'sends_db' not in va['channels'][0]
    http(origin+'/mix',expected=403);http(origin+'/api/config',expected=403)
    change(a,{'aux':0,'level_db':0},400);change(a,{'gain_db':0},400);change(a,{'channel':999,'mute':True},400);change(a,{'pre':False},400);change(b,{'level_db':0},400);change(a,{'level_db':0},403,{'Origin':'http://unrelated.invalid'})
    change(a,{'level_db':-18});change(b,{'level_db':-24,'pre':False})
    def rms(bus):
        with urllib.request.urlopen(audio+'/stream?key='+audio_key+'&bus='+str(bus),timeout=3) as stream:raw=stream.read(48000*8//5)
        values=struct.unpack('<'+'f'*(len(raw)//4),raw);return math.sqrt(sum(v*v for v in values)/len(values))
    time.sleep(.03)
    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:main,aux1,aux2=list(pool.map(rms,[0,1,2]))
    assert .06<main<.075 and abs(aux1/main-10**(-18/20))<.005 and abs(aux2/main-10**(-24/20))<.005,(main,aux1,aux2)
    change(a,{'mute':True});time.sleep(.03);assert rms(1)==0;assert rms(0)>.06;change(a,{'mute':False});assert view(a)['channels'][0]['level_db']==-18
    # A desktop snapshot taken before remote changes must preserve untouched sends.
    base=http(url+'/api/config');change(a,{'level_db':-9});candidate=json.loads(json.dumps(base));candidate['channels'][0]['pan']=.25
    api('config',{'config':candidate,'base':base});current=http(url+'/api/config');assert current['channels'][0]['sends_db'][0]==-9 and current['channels'][0]['pan']==.25
    timings=[]
    def move(pair):
        session,level=pair;t=time.perf_counter();change(session,{'level_db':level});timings.append((time.perf_counter()-t)*1000)
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:list(pool.map(move,[(a,-12),(b,-18)]*12))
    change(a,{'level_db':-12});change(b,{'level_db':-18});assert view(a)['channels'][0]['level_db']==-12 and view(b)['channels'][0]['level_db']==-18
    for host in ['100.101.102.103','studio.my-tailnet.ts.net','fd7a:115c:a1e0::1234']:
        status=api('network-host',{'host':host});assert host in status['listen']['url'] and host in status['remote']['sessions'][0]['url']
    http(url+'/api/network-host',{'host':'http://bad/path'},{'X-Monitor-Token':token},400)
    api('remote',{'action':'revoke','id':a['id']});http(origin+'/mix',headers={'X-Mix-Key':key(a)},expected=403);assert view(b)['bus']['index']==2
    # Scene/structure replacement invalidates every capability rather than remapping users.
    current=http(url+'/api/config');current['channels'].pop();api('config',current);http(origin+'/mix',headers={'X-Mix-Key':key(b)},expected=403)
    api('remote',{'action':'stop'});api('listen',{'enabled':False});api('stop',{})
    print(f'PASS: assigned aux only, limits/PRE permissions, real main+aux PCM isolation, send mute/resume, desktop merge, concurrent users, revocation, topology invalidation, Tailscale/MagicDNS/IPv6 link formatting; loopback HTTP change RTT median {statistics.median(timings):.1f} ms / p95 {sorted(timings)[int(len(timings)*.95)-1]:.1f} ms (not audio latency)')
finally:app.terminate();app.wait(timeout=5)
