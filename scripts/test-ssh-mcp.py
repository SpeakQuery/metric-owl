#!/usr/bin/env python3
"""Test the installed Linux binary over SSH using disposable data, never the owner's account."""
import argparse, base64, hashlib, hmac, http.cookiejar, json, secrets, selectors
import shlex, struct, subprocess, threading, time, urllib.request, uuid
from pathlib import Path

p = argparse.ArgumentParser()
p.add_argument('--host', required=True)
p.add_argument('--control', required=True)
p.add_argument('--ssh-port', type=int, default=22)
p.add_argument('--port', type=int, default=4898)
p.add_argument('--local-port', type=int, default=14898)
p.add_argument('--phase-seconds', type=int, default=30)
p.add_argument('--screenshots', action='store_true')
a = p.parse_args()
assert 1024 <= a.port <= 65535
assert 1 <= a.ssh_port <= 65535
assert 1024 <= a.local_port <= 65535 and a.local_port != a.port
assert 10 <= a.phase_seconds <= 60
ssh = ['ssh', '-S', a.control, '-o', 'BatchMode=yes', '-p', str(a.ssh_port), a.host]
suffix = uuid.uuid4().hex[:12]
directory = '/var/tmp/metric-owl-smoke-' + suffix
unit = 'metric-owl-smoke-' + suffix
forward = f'{a.local_port}:127.0.0.1:{a.port}'
origin = f'http://127.0.0.1:{a.local_port}'
bridge = None
client_id = None
csrf = ''
sse_stop = threading.Event()
sse_thread = None
sse_ages = []
sse_errors = []
sse_response = None
jar = http.cookiejar.CookieJar()
http = urllib.request.build_opener(urllib.request.ProxyHandler({}), urllib.request.HTTPCookieProcessor(jar))

def remote(command, data=None):
    return subprocess.run(ssh + [command], input=data, text=True, capture_output=True, check=True).stdout

def request(path, body=None, method=None):
    req = urllib.request.Request(origin + '/api/v1/' + path,
        data=None if body is None else json.dumps(body).encode(), method=method,
        headers={'Origin': origin, 'Content-Type': 'application/json', 'X-CSRF-Token': csrf})
    with http.open(req, timeout=10) as r:
        return json.load(r)

def poll(fn):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        try:
            result = fn()
            if result: return result
        except (OSError, ValueError): pass
        time.sleep(.2)
    raise RuntimeError('Remote service did not become ready')

def mcp(message):
    bridge.stdin.write(json.dumps(message) + '\n'); bridge.stdin.flush()
    with selectors.DefaultSelector() as selector:
        selector.register(bridge.stdout, selectors.EVENT_READ)
        if not selector.select(15): raise RuntimeError('SSH MCP response timed out')
        return json.loads(bridge.stdout.readline())

def tool(name, args, rpc_id):
    response = mcp({'jsonrpc':'2.0','id':rpc_id,'method':'tools/call','params':{'name':name,'arguments':args}})
    result = response['result']
    assert not result.get('isError'), result
    return json.loads(next(c['text'] for c in result['content'] if c['type']=='text'))

def process_snapshot(pids):
    # Read only fixed /proc files for known test processes. The owner service is untouched.
    code = '''import json, os, time
from pathlib import Path
pids = %s
result = {"at": time.monotonic(), "processes": {}}
for name, pid in pids.items():
    fields = Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1].split()
    result["processes"][name] = {"cpu_seconds": (int(fields[11]) + int(fields[12])) / os.sysconf("SC_CLK_TCK"),
        "rss_bytes": int(fields[21]) * os.sysconf("SC_PAGE_SIZE")}
print(json.dumps(result))
''' % repr(pids)
    return json.loads(remote('python3 -c ' + shlex.quote(code)))

def observe_phase(pids, with_mcp=False):
    first = process_snapshot(pids)
    snapshots = [first]
    ages, api_ms, mcp_ms = [], [], []
    deadline = time.monotonic() + a.phase_seconds
    calls = 0
    while True:
        start = time.monotonic()
        current = request('current')
        api_ms.append((time.monotonic() - start) * 1000)
        ages.append(max(0, time.time() - current['at']))
        if with_mcp:
            start = time.monotonic()
            reading = tool('current_metrics', {}, 100 + calls)
            mcp_ms.append((time.monotonic() - start) * 1000)
            assert reading['cpu']['value'] is not None
        calls += 1
        remaining = deadline - time.monotonic()
        if remaining <= 0: break
        time.sleep(min(5, remaining))
        snapshots.append(process_snapshot(pids))
    last = process_snapshot(pids)
    snapshots.append(last)
    elapsed = last['at'] - first['at']
    processes = {}
    for name in pids:
        processes[name] = {
            'cpu_percent_one_core': round(100 * (last['processes'][name]['cpu_seconds'] - first['processes'][name]['cpu_seconds']) / elapsed, 3),
            'rss_peak_bytes': max(s['processes'][name]['rss_bytes'] for s in snapshots),
            'rss_final_bytes': last['processes'][name]['rss_bytes'],
        }
    return {'seconds': round(elapsed, 2), 'processes': processes, 'api_requests': calls,
        'api_latency_ms_max': round(max(api_ms), 2), 'sample_age_seconds_max': round(max(ages), 3),
        'mcp_calls': len(mcp_ms), 'mcp_latency_ms_max': round(max(mcp_ms), 2) if mcp_ms else None}

def observe_sse():
    global sse_response
    # Separate HTTP connection; cookies are read only after setup/login completes.
    req = urllib.request.Request(origin + '/api/v1/live', headers={'Origin': origin})
    jar.add_cookie_header(req)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    try:
        with opener.open(req, timeout=10) as response:
            sse_response = response
            event = ''
            while not sse_stop.is_set():
                line = response.readline().decode().strip()
                if line.startswith('event:'): event = line[6:].strip()
                if line.startswith('data:') and event == 'sample':
                    sample = json.loads(line[5:].strip())
                    sse_ages.append(max(0, time.time() - sample['at']))
    except Exception as error:
        if not sse_stop.is_set(): sse_errors.append(type(error).__name__)

try:
    remote(f'mkdir {directory} && chown server-guard:server-guard {directory} && chmod 700 {directory}')
    remote(f'systemd-run --quiet --unit={unit} --uid=server-guard --gid=server-guard '
           f'--property=RuntimeMaxSec=300 --property=NoNewPrivileges=yes '
           f'/usr/local/bin/server-guard --data-dir {directory} serve --port {a.port}')
    subprocess.run(ssh[:-1] + ['-O','forward','-L',forward,a.host], check=True, capture_output=True)
    poll(lambda: request('auth/status'))
    setup = remote(f'cat {directory}/setup-key').strip()
    password = secrets.token_urlsafe(32)
    challenge = request('auth/setup/begin', {'key':setup,'username':'smoke-admin','password':password})
    digest = hmac.new(base64.b32decode(challenge['secret']), struct.pack('>Q',int(time.time())//30), hashlib.sha1).digest()
    offset = digest[-1] & 15
    code = str((struct.unpack('>I',digest[offset:offset+4])[0]&0x7fffffff)%1000000).zfill(6)
    recovery = request('auth/setup/finish', {'key':setup,'code':code})['recovery_codes']
    csrf = request('auth/login', {'username':'smoke-admin','password':password,'code':recovery[0]})['session']['csrf']
    poll(lambda: request('current')['cpu']['value'] is not None)
    agent_pid = int(remote(f'systemctl show {unit} --property=MainPID --value').strip())
    assert agent_pid > 1
    baseline = observe_phase({'agent': agent_pid})
    ssh_user, separator, ssh_host = a.host.rpartition('@')
    if not separator: ssh_user, ssh_host = 'root', a.host
    client = request('mcp/clients', {'name':'SSH acceptance test','scopes':['server_info','current_metrics','history'],
        'connection':{'transport':'ssh','host':ssh_host,'user':ssh_user,'port':a.ssh_port}})
    client_id = client['id']
    assert 'key' not in client, 'Wizard response exposed a secret'
    configuration = client['configuration']
    assert configuration['command'] == 'ssh' and '--key-file' in configuration['args'][-1]
    generated_args = list(configuration['args'])
    # Keep the generated SSH connection/remote command; add only test PID accounting and
    # this session's existing ControlMaster socket. No SSH key or customer access changes.
    generated_args[-1] = 'echo $$ > ' + shlex.quote(directory + '/bridge.pid') + '; exec ' + generated_args[-1]
    bridge = subprocess.Popen([configuration['command'], '-S', a.control, *generated_args],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
    init = mcp({'jsonrpc':'2.0','id':1,'method':'initialize','params':{'protocolVersion':'2025-11-25','capabilities':{},'clientInfo':{'name':'ssh-acceptance','version':'1'}}})
    assert init['result']['serverInfo']['name'] == 'Metric Owl'
    bridge.stdin.write(json.dumps({'jsonrpc':'2.0','method':'notifications/initialized'})+'\n'); bridge.stdin.flush()
    inventory = tool('server_info', {}, 2)
    poll(lambda: request('current')['cpu']['value'] is not None)
    current = tool('current_metrics', {}, 3)
    bridge_pid = int(remote('cat ' + shlex.quote(directory + '/bridge.pid')).strip())
    assert bridge_pid > 1
    sse_thread = threading.Thread(target=observe_sse, daemon=True)
    sse_thread.start()
    monitored = observe_phase({'agent': agent_pid, 'mcp_bridge': bridge_pid}, with_mcp=True)
    sse_stop.set()
    sse_thread.join(timeout=3)
    assert not sse_errors, sse_errors
    assert len(sse_ages) >= a.phase_seconds - 5, 'SSE did not produce continuous live samples'
    monitored['sse_samples'] = len(sse_ages)
    monitored['sse_sample_age_seconds_max'] = round(max(sse_ages), 3)
    now = time.time()
    history = tool('history', {'source':'host','metric':'cpu','from':now-86400,'to':now}, 4)
    assert inventory['cores'] > 0 and current['memory_total'] > 0
    request('mcp/clients/'+client_id, {}, 'DELETE'); client_id = None
    rejected = mcp({'jsonrpc':'2.0','id':5,'method':'tools/call','params':{'name':'server_info','arguments':{}}})
    assert rejected['result'].get('isError'), 'Revoked client still had access'
    remote('test ! -e ' + shlex.quote(configuration['key_file']))
    screenshots = None
    if a.screenshots:
        payload = {'origin':origin,'sshHost':a.host.rsplit('@',1)[-1],
            'cookies':[{'name':c.name,'value':c.value,'httpOnly':True,'secure':c.secure,'sameSite':'Strict'} for c in jar]}
        capture = subprocess.run(['node',str(Path(__file__).with_name('capture-remote-test.mjs'))],
            input=json.dumps(payload),text=True,capture_output=True,check=True,timeout=90)
        screenshots = json.loads(capture.stdout)
    result = {'result':'passed','transport':'SSH stdio -> scoped local IPC',
        'dashboard_tunnel':{'local_port':a.local_port,'remote_port':a.port,'authenticated':True},
        'os':inventory['os'],'cores':inventory['cores'],'memory_total':current['memory_total'],
        'cpu_percent':current['cpu']['value'],'memory_percent':current['memory']['value'],
        'actual_history_rows':len(history['rows']),'synthetic_history':False,
        'key_revoked':True,'private_key_file_removed':True,'secret_in_client_config':False,'owner_account_changed':False,'screenshots':screenshots,
        'performance':{'baseline_without_mcp':baseline,'sse_and_mcp':monitored,
            'target_cpu_below_one_percent':all(v['processes']['agent']['cpu_percent_one_core'] < 1 for v in [baseline,monitored]),
            'target_memory_below_100mib':all(v['processes']['agent']['rss_peak_bytes'] < 100*1024*1024 for v in [baseline,monitored]),
            'target_live_age_below_two_seconds':max(baseline['sample_age_seconds_max'],monitored['sample_age_seconds_max'],monitored['sse_sample_age_seconds_max']) < 2,
            'note':'Short bounded observation on a shared real host, not a stress test or long-term performance guarantee.'}}
finally:
    sse_stop.set()
    if sse_response:
        sse_response.close()
    if client_id:
        try: request('mcp/clients/'+client_id, {}, 'DELETE')
        except Exception: pass
    if bridge:
        bridge.terminate()
        try: bridge.wait(timeout=5)
        except subprocess.TimeoutExpired: bridge.kill()
    subprocess.run(ssh[:-1] + ['-O','cancel','-L',forward,a.host], capture_output=True)
    remote(f'systemctl stop {unit} || true')
    remote(f'rm -rf -- {directory}')
print(json.dumps(result))
