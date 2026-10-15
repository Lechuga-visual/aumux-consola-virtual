"""Unified access, independent password roles, main-console RPC, names and honest latency data."""
from http.cookiejar import CookieJar
import json,math,os,re,subprocess,time,urllib.request,urllib.error
from pathlib import Path
root=Path(__file__).resolve().parents[2]
app=subprocess.Popen([str(root/'desktop/target/debug/aumux'),'--no-browser'],stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True,env={**os.environ,'MONITOR_REMOTE_BIND':'127.0.0.1','MONITOR_LISTEN_HOST':'127.0.0.1'})
try:
 for line in app.stdout:
  match=re.search(r'http://127\.0\.0\.1:\d+',line)
  if match:local=match[0];break
 token=re.search(r'name="monitor-token" content="([^"]+)"',urllib.request.urlopen(local).read().decode())[1]
 def http(url,data=None,expected=200,headers=None,opener=None):
  request=urllib.request.Request(url,data=None if data is None else json.dumps(data).encode(),headers={'Content-Type':'application/json',**(headers or {})})
  try:
   with (opener.open(request,timeout=5) if opener else urllib.request.urlopen(request,timeout=5)) as r:code=r.status;raw=r.read()
  except urllib.error.HTTPError as e:code=e.code;raw=e.read()
  assert code==expected,(url,code,expected,raw[:300])
  try:return json.loads(raw)
  except ValueError:return raw.decode()
 def api(route,data):return http(local+'/api/'+route,data,headers={'X-Monitor-Token':token})
 def jar():return urllib.request.build_opener(urllib.request.HTTPCookieProcessor(CookieJar()))
 cfg=http(local+'/api/config');cfg['channels'][0]['name']='Voz Ñ';cfg['buses'][1]['name']='Oídos cantante';cfg['buses'][2]['name']='Batería'
 cfg['channels'][0]['generator']={'signal':'sine','level_db':-20,'frequency':1000};api('config',cfg);api('start',{'mode':'demo'})
 api('network-host',{'host':'127.0.0.1'});remote=api('remote',{'action':'start','port':0});origin=f"http://127.0.0.1:{remote['port']}"
 http(origin+'/api/config',expected=403)
 api('remote',{'action':'access','scope':'mix','mode':'open','max_db':0,'allow_pre':True})
 view=http(origin+'/mix?aux=2');assert view['shared'] and view['choices'][0]['name']=='Oídos cantante' and view['channels'][0]['name']=='Voz Ñ'
 change={'epoch':view['epoch'],'aux':2,'channel':0,'level_db':-12,'pre':False};http(origin+'/mix?aux=2',change)
 assert http(local+'/api/config')['channels'][0]['sends_db'][1]==-12
 http(origin+'/mix?aux=2',{**change,'aux':0},400)
 api('remote',{'action':'access','scope':'mix','mode':'password','password':'participantes','max_db':0})
 http(origin+'/mix',expected=403);mix=jar();http(origin+'/login/mix',{'password':'incorrecta'},403,opener=mix);http(origin+'/login/mix',{'password':'participantes'},opener=mix);assert http(origin+'/mix?aux=1',opener=mix)['bus']['name']=='Oídos cantante'
 http(origin+'/api/config',expected=403,opener=mix)
 api('remote',{'action':'access','scope':'console','mode':'password','password':'operador'})
 assert 'Acceso protegido' in http(origin+'/console/')
 console=jar();http(origin+'/login/console',{'password':'participantes'},403,opener=console);http(origin+'/login/console',{'password':'operador'},opener=console)
 assert 'AuMux' in http(origin+'/console/',opener=console)
 base=http(origin+'/api/config',opener=console);now=json.loads(json.dumps(base));now['channels'][0]['pan']=.3;now['channels'][0]['name']='Micrófono principal';now['buses'][2]['name']='Mezcla batería'
 http(origin+'/api/config',{'config':now,'base':base},opener=console)
 assert http(origin+'/mix?aux=2',opener=mix)['bus']['name']=='Mezcla batería'
 assert http(origin+'/mix?aux=1',opener=mix)['channels'][0]['name']=='Micrófono principal'
 # Two complete operators changing different fields must preserve each other.
 stale=base;candidate=json.loads(json.dumps(stale));candidate['buses'][0]['gain_db']=-3
 http(origin+'/api/config',{'config':candidate,'base':stale},opener=console)
 current=http(local+'/api/config');assert abs(current['channels'][0]['pan']-.3)<1e-6 and current['channels'][0]['name']=='Micrófono principal' and current['buses'][0]['gain_db']==-3
 latency=http(origin+'/api/latency',opener=console);assert abs(latency['block_ms']-256/48)<1e-6 and latency['driver_ms'] is None and latency['end_to_end_measured_ms'] is None
 addresses=http(origin+'/api/addresses',opener=console);assert any(v['loopback'] for v in addresses['interfaces'])
 # Process metrics must be sampled, scoped and separate from audio latency.
 deadline=time.monotonic()+4
 while True:
  system=http(origin+'/api/status',opener=console)['system']
  if system['sample_interval_ms'] is not None:break
  assert time.monotonic()<deadline,'process metrics never sampled'
  time.sleep(.05)
 assert system['scope']=='process' and system['logical_processors']>=1
 assert system['cpu_normalization']=='all_logical_processors' and system['ram_kind']=='resident_working_set'
 assert system['cpu_percent'] is not None and math.isfinite(system['cpu_percent']) and 0<=system['cpu_percent']<=100
 assert system['ram_bytes'] is not None and system['ram_bytes']>0
 assert system['sample_interval_ms']>=1000
 api('remote',{'action':'access','scope':'console','mode':'disabled'});http(origin+'/api/config',expected=403,opener=console)
 api('remote',{'action':'access','scope':'console','mode':'open'});assert 'channels' in http(origin+'/api/config')
 net=api('listen',{'enabled':True,'port':0});listen=f"http://127.0.0.1:{net['port']}"
 api('listen-access',{'mode':'open'});assert '<title>AuMux' in http(listen+'/');status=http(listen+'/status');assert status['buses'][2]['name']=='Mezcla batería' and status['latency']['audio_network_one_way_ms'] is None
 with urllib.request.urlopen(listen+'/stream?bus=0',timeout=3) as s:assert len(s.read(3840))==3840
 api('listen-access',{'mode':'password','password':'oyentes'});assert 'Acceso protegido' in http(listen+'/');http(listen+'/status',expected=403)
 listeners=jar();http(listen+'/login/listen',{'password':'oyentes'},opener=listeners);assert http(listen+'/status',opener=listeners)['sample_rate']==48000
 http(origin+'/mix',expected=403,opener=listeners)
 api('remote',{'action':'access','scope':'mix','mode':'disabled'});http(origin+'/mix',expected=403,opener=mix)
 api('listen-access',{'mode':'disabled'});http(listen+'/status',expected=403,opener=listeners)
 api('listen',{'enabled':False});api('remote',{'action':'stop'});api('stop',{})
 print('PASS: generic open aux choice, shared passwords with HttpOnly sessions, separate mix/listen/operator permissions, revocation/disable, full-console control and three-way merge, live names, network-interface enumeration, latency components without fabricated end-to-end values, scoped CPU/RAM samples')
finally:app.terminate();app.wait(timeout=5)
