#!/usr/bin/env python3
"""Real local agent+relay+MCP integration. Never sends real mail; temporary data only."""
import tempfile, pathlib, subprocess, time, json, urllib.request, urllib.error, http.cookiejar, base64, hmac, hashlib, struct, socket, os, sys, re, selectors, sqlite3
ROOT=pathlib.Path(__file__).resolve().parents[1]
BIN=pathlib.Path(os.environ.get('GUARD_TEST_BIN',str(ROOT/'target/debug/server-guard'))); RELAY=pathlib.Path(os.environ.get('GUARD_TEST_RELAY',str(ROOT/'target/debug/guard-relay')))
def poll(fn,timeout=25):
    deadline=time.monotonic()+timeout
    while True:
        try:
            result=fn()
            if result:return result
        except (OSError,ValueError,KeyError,AssertionError):pass
        if time.monotonic()>deadline:raise AssertionError('Timed out waiting for condition')
        time.sleep(.15)
def totp(secret):
    key=base64.b32decode(secret); digest=hmac.new(key,struct.pack('>Q',int(time.time())//30),hashlib.sha1).digest();offset=digest[-1]&15
    return str((struct.unpack('>I',digest[offset:offset+4])[0]&0x7fffffff)%1000000).zfill(6)
def free_port():
    with socket.socket() as s:s.bind(('127.0.0.1',0));return s.getsockname()[1]
with tempfile.TemporaryDirectory(prefix='guard-integration-') as temp:
    data=pathlib.Path(temp);agent_dir=data/'agent';relay_dir=data/'relay';agent_port=free_port();relay_port=free_port();origin=f'http://127.0.0.1:{agent_port}'
    subprocess.run([RELAY,'--data-dir',relay_dir,'provision','--name','integration'],check=True,capture_output=True)
    relay_key=next(relay_dir.glob('device-*.key')).read_text()
    relay_log=open(data/'relay.log','w');agent_log=open(data/'agent.log','w')
    relay=subprocess.Popen([RELAY,'--data-dir',relay_dir,'serve','--port',str(relay_port),'--local-test'],stderr=relay_log)
    agent=subprocess.Popen([BIN,'--data-dir',agent_dir,'serve','--port',str(agent_port)],stderr=agent_log,env={**os.environ,'GUARD_ALLOW_LOCAL_RELAY':'1'})
    bridge=None
    try:
        jar=http.cookiejar.CookieJar();client=urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar));csrf=''
        def request(path,body=None,method=None,headers=None):
            req=urllib.request.Request(origin+'/api/v1/'+path,data=json.dumps(body).encode() if body is not None else None,method=method or ('POST' if body is not None else 'GET'),headers={'Origin':origin,'Content-Type':'application/json','X-CSRF-Token':csrf,**(headers or {})})
            with client.open(req,timeout=8) as r:return json.load(r)
        poll(lambda:request('auth/status'))
        setup=(agent_dir/'setup-key').read_text()
        summary=subprocess.run(['bash',ROOT/'scripts/show-setup.sh',agent_dir,'ssh',str(agent_port),str(agent.pid)],check=True,capture_output=True,text=True,env={**os.environ,'SSH_CONNECTION':'198.51.100.8 50000 203.0.113.12 2222','USER':'root','SUDO_USER':'root','GUARD_LOCAL_PORT':'14877','GUARD_SSH_HOST':'','GUARD_SSH_USER':'','GUARD_SSH_PORT':''}).stdout
        assert 'Setup key: '+setup in summary and 'root@203.0.113.12' in summary and 'http://127.0.0.1:14877/admin' in summary
        assert f'-L 127.0.0.1:14877:127.0.0.1:{agent_port} -p 2222' in summary and 'Setup guide -> Set up MCP connection' in summary
        hidden=subprocess.run(['bash',ROOT/'scripts/show-setup.sh',agent_dir,'local',str(agent_port),str(agent.pid)],check=True,capture_output=True,text=True,env={**os.environ,'GUARD_HIDE_SETUP_KEY':'1'}).stdout
        assert setup not in hidden and 'package-manager logs' in hidden
        for path in ('/', '/index.html', '/login', '/anything'):
            try:urllib.request.urlopen(origin+path);raise AssertionError('Unexpected dashboard at '+path)
            except urllib.error.HTTPError as e:assert e.code==404
        with urllib.request.urlopen(origin+'/admin') as response:assert 'Metric Owl' in response.read().decode()
        challenge=request('auth/setup/begin',{'key':setup,'username':'admin','password':'integration-password-2026'})
        code=totp(challenge['secret']);codes=request('auth/setup/finish',{'key':setup,'code':code})['recovery_codes']
        assert not (agent_dir/'setup-key').exists(), 'Setup key survived completed setup'
        assert request('auth/status')['configured'] is True
        try:request('auth/setup/begin',{'key':setup,'username':'intruder','password':'integration-password-2026'});raise AssertionError('Completed setup key was reusable')
        except urllib.error.HTTPError as e:assert e.code==400
        summary=subprocess.run(['bash',ROOT/'scripts/show-setup.sh',agent_dir,'local',str(agent_port),str(agent.pid)],check=True,capture_output=True,text=True).stdout
        assert 'Your administrator account is ready' in summary and setup not in summary
        renewal=subprocess.run([BIN,'--data-dir',agent_dir,'reset-setup'],capture_output=True,text=True)
        assert renewal.returncode!=0 and not (agent_dir/'setup-key').exists(), 'Configured account allowed setup renewal'

        try:request('auth/login',{'username':'admin','password':'integration-password-2026','code':code});raise AssertionError('TOTP replay accepted')
        except urllib.error.HTTPError as e:assert e.code==400
        csrf=request('auth/login',{'username':'admin','password':'integration-password-2026','code':codes[0]})['session']['csrf']
        # Consumed recovery code must not log in again.
        try:request('auth/login',{'username':'admin','password':'integration-password-2026','code':codes[0]});raise AssertionError('Recovery reused')
        except urllib.error.HTTPError as e:assert e.code==400
        metrics=poll(lambda:request('current'))
        assert metrics['memory_total']>0 and metrics['source_id']
        assert time.time()-metrics['at']<2, 'Live sample older than two seconds'
        connection=request('mcp/connection')
        assert pathlib.Path(connection['command']).is_absolute()
        assert connection['args']==['--data-dir',str(agent_dir.resolve()),'mcp']
        assert connection['key_variable']=='GUARD_MCP_KEY' and 'key' not in connection
        try:urllib.request.urlopen(origin+'/api/v1/mcp/connection');raise AssertionError('MCP configuration exposed without login')
        except urllib.error.HTTPError as e:assert e.code==401
        s=request('settings')['settings'];s.update(email_transport='relay',relay_url=f'http://127.0.0.1:{relay_port}',recipient='integration@example.invalid')
        request('settings',{'settings':s,'keys':{'relay':relay_key}},'PUT')
        assert request('connections/test',{'kind':'relay'})['ok']
        try:request('notifications/test',{},'POST');raise AssertionError('Unverified recipient accepted')
        except urllib.error.HTTPError as e:assert e.code==400
        request('recipients/start',{},'POST')
        mailbox=relay_dir/'test-mailbox.jsonl'
        def verification():
            rows=[json.loads(x) for x in mailbox.read_text().splitlines()]
            assert rows[0]['message']['Subject']=='Your Metric Owl verification code'
            assert rows[0]['message']['TextBody'].startswith('Your verification code: ')
            assert rows[0]['message']['TextBody'].endswith('Valid for 10 minutes.')
            return re.search(r'\b[0-9]{6}\b',rows[0]['message']['TextBody']).group(0)
        email_code=poll(verification)
        assert request('recipients/verify',{'code':email_code})['verified']
        request('notifications/test',{},'POST')
        def delivered():return any(x['state']=='test_delivered' for x in request('notifications')['items'])
        poll(delivered,45)
        # Provider outage: reconcile an old delivery even beyond a newest-50 status page.
        with sqlite3.connect(relay_dir/'relay.sqlite') as db:
            device_id=db.execute('SELECT id FROM devices LIMIT 1').fetchone()[0]
            for i in range(65):db.execute("INSERT INTO outbox(id,kind,payload,state,next_at,created,error) VALUES(?, 'postmark','{}','failed',0,?, 'test provider failure')",(device_id+':fixture-'+str(i),i))
        req=urllib.request.Request(f'http://127.0.0.1:{relay_port}/v1/status',data=json.dumps({'ids':['fixture-0']}).encode(),headers={'Authorization':'Bearer '+relay_key,'Content-Type':'application/json'})
        with urllib.request.urlopen(req,timeout=5) as response:
            status=json.load(response);assert status['items'][0]['state']=='failed' and status['items'][0]['id'].endswith(':fixture-0')
        # Scope and revocation tested against real Unix IPC.
        key=request('mcp/clients',{'name':'limited','scopes':['server_info','history']})
        key_file=pathlib.Path(key['configuration']['key_file'])
        assert key_file.read_text()==key['key'] and key_file.stat().st_mode&0o777==0o600
        assert key_file.parent.stat().st_mode&0o777==0o700
        assert key['key'] not in json.dumps(key['configuration'])
        count_before=len(request('mcp/clients'))
        for host in ['-oProxyCommand=bad','server; id','https://example.com']:
            try:
                request('mcp/clients',{'name':'invalid SSH','scopes':['server_info'],'connection':{'transport':'ssh','host':host,'user':'root','port':22}})
                raise AssertionError('Unsafe SSH host accepted')
            except urllib.error.HTTPError as e:assert e.code==400
        assert len(request('mcp/clients'))==count_before, 'Invalid configuration created a client'
        wizard=request('mcp/clients',{'name':'wizard SSH','scopes':['server_info'],'connection':{'transport':'ssh','host':'server.example.com','user':'root','port':2222}})
        assert 'key' not in wizard, 'Wizard exposed its private secret'
        config=wizard['configuration']
        assert config['command']=='ssh' and '--key-file' in config['args'][-1] and 'BatchMode=yes' in config['args']
        assert 'GUARD_MCP_KEY' not in config['codex'] and '<YOUR_' not in config['codex']
        assert json.loads(config['custom'])['mcpServers'][config['server_name']]['args']==config['args']
        reconfigured=request('mcp/clients/'+wizard['id']+'/configuration',{'transport':'local'})
        assert reconfigured['command']==connection['command'] and '--key-file' in reconfigured['args']
        request('mcp/clients/'+wizard['id'],{},'DELETE')
        assert not pathlib.Path(config['key_file']).exists(), 'Revocation retained managed key file'
        try:request('mcp/clients/'+wizard['id']+'/configuration',{'transport':'local'});raise AssertionError('Revoked client generated configuration')
        except urllib.error.HTTPError as e:assert e.code==400
        def ipc(method,key_value):
            with socket.socket(socket.AF_UNIX) as sock:
                sock.settimeout(5);sock.connect(str(agent_dir/'guard.sock'));sock.sendall(json.dumps({'key':key_value,'method':method,'args':{}}).encode());sock.shutdown(socket.SHUT_WR);buf=b''
                while chunk:=sock.recv(65536):buf+=chunk
                return json.loads(buf)
        assert 'id' in ipc('server_info',key['key'])
        assert 'error' in ipc('current_metrics',key['key'])
        assert 'error' in ipc('server_info','bad-key')
        bridge_env={k:v for k,v in os.environ.items() if k!='GUARD_MCP_KEY'}
        conflict=subprocess.run([BIN,'--data-dir',agent_dir,'mcp','--key-file',key_file],env={**bridge_env,'GUARD_MCP_KEY':key['key']},capture_output=True,text=True,timeout=8)
        assert conflict.returncode!=0 and key['key'] not in conflict.stderr, 'MCP key inputs were not mutually exclusive or leaked a secret'
        bridge=subprocess.Popen([key['configuration']['command'],*key['configuration']['args']],env=bridge_env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,bufsize=1)
        def mcp_call(message):
            bridge.stdin.write(json.dumps(message)+'\n');bridge.stdin.flush()
            with selectors.DefaultSelector() as selector:
                selector.register(bridge.stdout,selectors.EVENT_READ);assert selector.select(8),'MCP response timed out'
                return json.loads(bridge.stdout.readline())
        init=mcp_call({'jsonrpc':'2.0','id':1,'method':'initialize','params':{'protocolVersion':'2025-11-25','capabilities':{},'clientInfo':{'name':'acceptance','version':'1'}}})
        assert 'result' in init,init
        assert init['result']['serverInfo']['name']=='Metric Owl'
        assert "Reply concisely in the user's language" in init['result']['instructions']
        bridge.stdin.write(json.dumps({'jsonrpc':'2.0','method':'notifications/initialized'})+'\n');bridge.stdin.flush()
        tools=mcp_call({'jsonrpc':'2.0','id':2,'method':'tools/list','params':{}})
        names={x['name'] for x in tools['result']['tools']};assert names=={'server_info','current_metrics','history','list_incidents','incident_detail','log_summary','metric_report','process_history','monitored_sources','monitoring_health'},names
        result=mcp_call({'jsonrpc':'2.0','id':3,'method':'tools/call','params':{'name':'server_info','arguments':{}}});assert 'result' in result,result
        # Synthetic history is written only to this temporary test-owned database.
        # Exercise real stdio MCP for a complete 24-hour table and aligned CPU/RAM peaks.
        history_end=(int(time.time())//60)*60
        history_start=history_end-86400
        history_source='mcp-history-fixture'
        with sqlite3.connect(agent_dir/'guard.sqlite') as db:
            for metric in ('cpu','memory'):
                for minute in range(1440):
                    value=(92.0 if 600<=minute<603 else 25.0) if metric=='cpu' else (88.0 if 602<=minute<605 else 40.0)
                    missing=metric=='cpu' and minute==800
                    start=history_start+minute*60
                    row={'source':history_source,'metric':metric,'start':float(start),'end':float(start+60),'resolution':60,
                         'min':None if missing else value,'max':None if missing else value,
                         'first':None if missing else value,'last':None if missing else value,
                         'weighted_sum':0.0 if missing else value*60,'observed_seconds':0.0 if missing else 60.0,'total':0.0}
                    db.execute('INSERT INTO aggregates(source,metric,start,resolution,json) VALUES(?,?,?,?,?)',
                               (history_source,metric,start,60,json.dumps(row,separators=(',',':'))))
        def read_mcp_history(metric,request_id,from_time=history_start,to_time=history_end):
            response=mcp_call({'jsonrpc':'2.0','id':request_id,'method':'tools/call','params':{'name':'history','arguments':
                              {'source':history_source,'metric':metric,'from':from_time,'to':to_time}}})
            assert 'result' in response,response
            return response['result']
        histories={}
        for metric,request_id in [('cpu',4),('memory',5)]:
            result=read_mcp_history(metric,request_id)
            assert not result.get('isError',False),result
            history=json.loads(next(item['text'] for item in result['content'] if item['type']=='text'))
            assert history['from']==history_start and history['to']==history_end and history['resolution']==60
            assert len(history['rows'])==1440,(metric,len(history['rows']))
            assert history['rows'][0]['start']==history_start and history['rows'][-1]['end']==history_end
            histories[metric]=history['rows']
        cpu=histories['cpu'];memory=histories['memory']
        assert cpu[599]['max']==25 and cpu[600]['max']==92 and cpu[603]['max']==25
        assert memory[601]['max']==40 and memory[602]['max']==88 and memory[605]['max']==40
        assert cpu[602]['start']==memory[602]['start'], 'CPU and memory timestamps are not aligned'
        assert cpu[800]['observed_seconds']==0 and cpu[800]['min'] is None, 'Missing history was fabricated'
        assert read_mcp_history('cpu',6,history_end,history_start).get('isError'), 'Invalid range accepted by MCP'
        request('mcp/clients/'+key['id'],{},'DELETE');assert 'error' in ipc('server_info',key['key'])
        assert not key_file.exists(), 'Revoked MCP key file still exists'
        revoked=mcp_call({'jsonrpc':'2.0','id':7,'method':'tools/call','params':{'name':'server_info','arguments':{}}})
        assert revoked['result'].get('isError'), 'Existing bridge retained access after revocation'
        request('auth/password',{'password':'new-integration-password-2026'},'PUT')
        try:request('current');raise AssertionError('Session survived password change')
        except urllib.error.HTTPError as e:assert e.code==401
        csrf=request('auth/login',{'username':'admin','password':'new-integration-password-2026','code':codes[1]})['session']['csrf']
        changed=request('auth/mfa/begin',{},'POST')
        new_codes=request('auth/mfa/finish',{'code':totp(changed['secret'])})['recovery_codes']
        try:request('current');raise AssertionError('Session survived MFA change')
        except urllib.error.HTTPError as e:assert e.code==401
        try:request('auth/login',{'username':'admin','password':'new-integration-password-2026','code':codes[2]});raise AssertionError('Old recovery survived MFA change')
        except urllib.error.HTTPError as e:assert e.code==400
        csrf=request('auth/login',{'username':'admin','password':'new-integration-password-2026','code':new_codes[0]})['session']['csrf']
        bridge.terminate();bridge.wait(timeout=8);bridge=None
        agent.terminate();agent.wait(timeout=8)
        agent=subprocess.Popen([BIN,'--data-dir',agent_dir,'serve','--port',str(agent_port)],stderr=agent_log,env={**os.environ,'GUARD_ALLOW_LOCAL_RELAY':'1'})
        poll(lambda:request('auth/status')['configured'])
        assert not (agent_dir/'setup-key').exists(), 'Restart recreated setup key for an existing account'
        # The accumulated negative login cases reach the persistent attempt budget.
        # Verify it survives restart, then age only this test-owned rate-limit fixture.
        try:request('auth/login',{'username':'admin','password':'new-integration-password-2026','code':new_codes[1]});raise AssertionError('Restart cleared the login attempt limit')
        except urllib.error.HTTPError as e:assert e.code==400 and 'Too many attempts' in e.read().decode()
        with sqlite3.connect(agent_dir/'guard.sqlite') as db:
            db.execute("UPDATE kv SET value=? WHERE key='auth_attempts'", (json.dumps([time.time()-301]),))
        csrf=request('auth/login',{'username':'admin','password':'new-integration-password-2026','code':new_codes[1]})['session']['csrf']
        assert request('host')['id'], 'Saved account failed after restart'
        print(json.dumps({'result':'passed','checks':['installer prints key and SSH destination','admin route only','setup consumed and never recreated after restart','MCP connection instructions authenticated','TOTP replay','recovery single-use','real host metrics','relay key test','recipient verification','durable test-email delivery','MCP private key files and ready local/SSH configuration','MCP shell injection rejection and key file cleanup','MCP official SDK tools','MCP 24-hour CPU and memory history with aligned peaks and missing data','MCP invalid-range rejection','IPC scopes and revocation','password session revocation','MFA rotation and recovery replacement','live freshness <2 seconds','old relay status beyond newest50'],'real_email_sent':False},ensure_ascii=False))
    finally:
        for p in [bridge,agent,relay]:
            if p:p.terminate()
        for p in [bridge,agent,relay]:
            if p:
                try:p.wait(timeout=8)
                except subprocess.TimeoutExpired:p.kill()
        relay_log.close();agent_log.close()
