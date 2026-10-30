#!/usr/bin/env python3
"""Local release binary, basic monitoring profile, no live dashboard client."""
import pathlib,subprocess,tempfile,time,json,socket,statistics,sys
root=pathlib.Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='guard-benchmark-')as directory:
 with socket.socket()as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
 p=subprocess.Popen([root/'target/release/server-guard','--data-dir',directory,'serve','--port',str(port)],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 try:
  def sample():
   fields=subprocess.check_output(['ps','-p',str(p.pid),'-o','time=,rss='],text=True).split();minutes,seconds=fields[0].split(':');return int(minutes)*60+float(seconds),int(fields[1])/1024
  time.sleep(5);cpu0,_=sample();start=time.monotonic();rss=[]
  for _ in range(60):time.sleep(1);_,mem=sample();rss.append(mem)
  cpu1,_=sample();elapsed=time.monotonic()-start;result={'platform':sys.platform,'duration_seconds':round(elapsed,2),'cpu_percent_one_core':round((cpu1-cpu0)/elapsed*100,3),'rss_mib_average':round(statistics.mean(rss),2),'rss_mib_max':round(max(rss),2),'profile':'release; basic host monitoring; no browser, folder scans, or external sources','cpu_target_percent':1,'memory_target_mib':100}
  print(json.dumps(result,ensure_ascii=False,indent=2))
 finally:p.terminate();p.wait(timeout=8)
