#!/usr/bin/env python3
"""Temporary-data HTTP acceptance for sources, logs and diagnostics. No MCP or real email."""
import base64, hashlib, hmac, http.cookiejar, http.server, json, os, pathlib, secrets, socket, sqlite3, struct, subprocess, sys, tempfile, threading, time, urllib.request, urllib.error, uuid
ROOT=pathlib.Path(__file__).resolve().parents[1]
BIN=pathlib.Path(os.environ.get('GUARD_TEST_BIN',str(ROOT/'target/debug/server-guard')))
def free_port():
    with socket.socket() as sock:sock.bind(('127.0.0.1',0));return sock.getsockname()[1]
def poll(fn,timeout=40):
    deadline=time.monotonic()+timeout
    while time.monotonic()<deadline:
        try:
            value=fn()
            if value:return value
        except (OSError,ValueError,KeyError,AssertionError):pass
        time.sleep(.25)
    raise AssertionError('Timed out waiting for source state')
class Fixture(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path=='/redirect':self.send_response(302);self.send_header('Location','http://169.254.169.254/latest/meta-data');self.end_headers();return
        body=b'{"ready":true,"message":"healthy"}' if self.path=='/health' else b'{"ready":false,"message":"error"}'
        self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
    def log_message(self,*args):pass
fixture=http.server.ThreadingHTTPServer(('127.0.0.1',0),Fixture)
threading.Thread(target=fixture.serve_forever,daemon=True).start()
checks=[]
with tempfile.TemporaryDirectory(prefix='metricowl-features-') as temp:
    folder=pathlib.Path(temp);data=folder/'data';port=free_port();origin=f'http://127.0.0.1:{port}';csrf=''
    log=(folder/'agent.log').open('w');process=subprocess.Popen([BIN,'--data-dir',data,'serve','--port',str(port)],stdout=log,stderr=log)
    client=urllib.request.build_opener(urllib.request.ProxyHandler({}),urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
    def request(path,body=None,method=None):
        req=urllib.request.Request(origin+'/api/v1/'+path,data=json.dumps(body).encode() if body is not None else None,method=method,headers={'Origin':origin,'Content-Type':'application/json','X-CSRF-Token':csrf})
        with client.open(req,timeout=12) as response:return json.load(response)
    def status(id):return next((x for x in request('targets')['status'] if x['id']==id),{})
    def target(kind,name,address,options=None):return dict(id=str(uuid.uuid4()),kind=kind,name=name,address=address,enabled=True,options=options)
    try:
        poll(lambda:request('auth/status'))
        # Simulate the browser authority preserved by an SSH alternate-port forward.
        forwarded='http://127.0.0.1:14877'
        req=urllib.request.Request(origin+'/api/v1/auth/status',headers={'Host':'127.0.0.1:14877','Origin':forwarded})
        with client.open(req,timeout=8) as response:assert response.status==200
        for headers in [{'Host':'evil.example:14877','Origin':'http://evil.example:14877'}, {'Host':'127.0.0.1:14877','Origin':'http://127.0.0.1:12345'}]:
            try:client.open(urllib.request.Request(origin+'/api/v1/auth/status',headers=headers),timeout=8);raise AssertionError('Host/origin isolation failed')
            except urllib.error.HTTPError as error:assert error.code==403
        checks.append('alternate SSH tunnel authority accepted; non-loopback host and cross-port Origin rejected')
        setup=(data/'setup-key').read_text();password=secrets.token_urlsafe(30)
        challenge=request('auth/setup/begin',dict(key=setup,username='feature-test',password=password))
        digest=hmac.new(base64.b32decode(challenge['secret']),struct.pack('>Q',int(time.time())//30),hashlib.sha1).digest();offset=digest[-1]&15
        code=str((struct.unpack('>I',digest[offset:offset+4])[0]&0x7fffffff)%1000000).zfill(6)
        recovery=request('auth/setup/finish',dict(key=setup,code=code))['recovery_codes']
        csrf=request('auth/login',dict(username='feature-test',password=password,code=recovery[0]))['session']['csrf']
        sample=poll(lambda: (s if (s:=request('current')).get('diagnostics') and s.get('memory_total',0)>0 and s.get('cores') else None))
        assert sample['memory_total']>0 and len(sample['cores'])>0
        diagnostics=sample['diagnostics'];assert any(d['metric']=='swap_total' for d in diagnostics)
        if sys.platform!='linux':
            for d in diagnostics:
                if d['metric'] in ('memory_pressure','cpu_iowait','oom_kills_5m','disk_io_latency','disk_io_utilization','inode_used'):
                    assert d['reading']['value'] is None and d['reading']['quality']=='unavailable'
        checks.append('real metrics and explicit unavailable platform diagnostics')
        inventory=poll(lambda: (i if (i:=request('inventory')).get('services_checked_at') else None))
        assert isinstance(inventory['sites'],list) and isinstance(inventory['services'],list)
        for service in inventory['services']:
            if service['kind']=='database':assert 'require a separately configured read-only' in service['database_details']
        checks.append('bounded domain and service inventory; database detail limitations')
        logs=folder/'application.log';logs.write_text('startup healthy\npassword=fixture-secret-value user@example.invalid\n')
        missing=folder/'missing.log'
        content=target('http','HTTP JSON ready',f'http://127.0.0.1:{fixture.server_port}/health',{'expected_status':200,'json_pointer':'/ready','json_expected':'true','expected_text':'healthy'})
        false200=target('http','HTTP error with 200',f'http://127.0.0.1:{fixture.server_port}/error',{'json_pointer':'/ready','json_expected':'true'})
        redirect=target('http','Redirect rejected',f'http://127.0.0.1:{fixture.server_port}/redirect')
        metadata=target('http','Metadata blocked','http://169.254.169.254/latest/meta-data')
        job=target('job','Scheduled test job','',{'job_interval_seconds':60,'job_grace_seconds':0,'job_max_duration_seconds':60})
        log_source=target('log','Selected application log',str(logs));missing_source=target('log','Unavailable log',str(missing))
        sources=[content,false200,redirect,metadata,job,log_source,missing_source]
        if os.environ.get('GUARD_TEST_TLS_URL'):
            sources.append(target('tls','Owned HTTPS certificate',os.environ['GUARD_TEST_TLS_URL']))
        normalized=request('targets',sources,'PUT')['targets'];job=next(t for t in normalized if t['id']==job['id'])
        assert job['address']==str(data.resolve()/'job-reports'/f"{job['id']}.json") and job['options']['job_added_at']>0
        poll(lambda:status(content['id']).get('status')=='ok')
        poll(lambda:status(false200['id']).get('status')=='error')
        assert 'JSON value did not match' in status(false200['id'])['detail']
        poll(lambda:status(redirect['id']).get('status')=='error')
        poll(lambda:status(metadata['id']).get('status')=='unavailable')
        assert 'blocked' in status(metadata['id'])['detail']
        checks.append('HTTP content/JSON validation catches 200 error pages; redirects and metadata blocked')
        poll(lambda:status(job['id']).get('status')=='pending')
        checks.append('missing first job report remains pending, not successful')
        poll(lambda:'[REDACTED]' in status(log_source['id']).get('detail',''))
        detail=status(log_source['id'])['detail'];assert 'fixture-secret-value' not in detail and 'user@example.invalid' not in detail
        poll(lambda:status(missing_source['id']).get('status')=='unavailable')
        checks.append('log secrets redacted; missing files unavailable')
        logs.rename(folder/'application.log.1');logs.write_text('rotated log arrived\n')
        poll(lambda:'rotated log arrived' in status(log_source['id']).get('detail',''))
        checks.append('rotated selected log continues reading')
        for state,expected in [('running','running'),('success','ok'),('failure','error')]:
            report=subprocess.run([BIN,'--data-dir',data,'report-job','--id',job['id'],'--state',state],capture_output=True,text=True,check=True)
            assert 'No job was executed or modified' in report.stdout
            poll(lambda:status(job['id']).get('status')==expected,timeout=38)
        checks.append('copyable job report CLI records running/success/failure without running the job')
        if os.environ.get('GUARD_TEST_TLS_URL'):
            tls=next(t for t in sources if t['kind']=='tls')
            tls_status=poll(lambda:s if (s:=status(tls['id'])).get('status')=='ok' else None)
            assert tls_status['value']>0 and tls_status['measurements']['tls_failure']==0
            checks.append('verified live TLS certificate expiry from explicitly selected owned site')
        rules=request('rules')
        if isinstance(rules,dict):rules=rules['rules']
        assert any(r['source']==job['id'] and r['metric']=='job' for r in rules)
        checks.append('new targets create ready-made alert rules')
        normalized=[t for t in normalized if t['id']!=content['id']]
        request('targets',normalized,'PUT');poll(lambda:all(s['id']!=content['id'] for s in request('targets')['status']))
        assert all(r['source']!=content['id'] for r in request('rules'))
        checks.append('removed source stops live monitoring and removes its rules')
        with sqlite3.connect(data/'guard.sqlite') as db:
            details=db.execute('select count(*) from aggregates').fetchone()[0]
        assert details>0
        print(json.dumps({'result':'passed','platform':sys.platform,'checks':checks,'mcp_used':False,'real_email_sent':False,'diagnostic_metrics':sorted(set(d['metric'] for d in diagnostics)),'history_rows':details,'live_age_seconds':round(time.time()-request('current')['at'],3)}))
    finally:
        process.terminate()
        try:process.wait(timeout=8)
        except subprocess.TimeoutExpired:process.kill();process.wait()
        log.close();fixture.shutdown()
