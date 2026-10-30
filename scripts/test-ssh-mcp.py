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
sse_clock_offset = 0
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
result = {"at": time.monotonic(), "unix_at": time.time(), "processes": {}}
for name, pid in pids.items():
    fields = Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1].split()
    result["processes"][name] = {"cpu_seconds": (int(fields[11]) + int(fields[12])) / os.sysconf("SC_CLK_TCK"),
        "rss_bytes": int(fields[21]) * os.sysconf("SC_PAGE_SIZE")}
print(json.dumps(result))
''' % repr(pids)
    started = time.time()
    result = json.loads(remote('python3 -c ' + shlex.quote(code)))
    finished = time.time()
    result['clock_offset_seconds'] = result['unix_at'] - (started + finished) / 2
    result['clock_uncertainty_seconds'] = (finished - started) / 2
    return result

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
        ages.append(max(0, time.time() + first['clock_offset_seconds'] - current['at']))
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
        'estimated_server_clock_offset_seconds': round(first['clock_offset_seconds'], 3),
        'clock_estimate_uncertainty_seconds': round(first['clock_uncertainty_seconds'], 3),
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
                    sse_ages.append(max(0, time.time() + sse_clock_offset - sample['at']))
    except Exception as error:
        if not sse_stop.is_set(): sse_errors.append(type(error).__name__)

def process_identities(processes):
    selected = [{'pid': item['pid'], 'name': item['name']} for item in processes]
    code = '''import json
from pathlib import Path
result = []
for item in %s:
    try:
        status = dict(line.split(":", 1) for line in Path(f"/proc/{item['pid']}/status").read_text().splitlines() if ":" in line)
        result.append({**item, "tgid": int(status["Tgid"]), "observed": "present"})
    except FileNotFoundError:
        result.append({**item, "tgid": None, "observed": "vanished before verification"})
print(json.dumps(result))
''' % repr(selected)
    rows = json.loads(remote('python3 -c ' + shlex.quote(code)))
    present = [row for row in rows if row['tgid'] is not None]
    assert present, 'No current process survived long enough for identity verification'
    assert all(row['pid'] == row['tgid'] for row in present), 'Userland thread listed as a separate process'
    return rows

try:
    installed_hash = remote('sha256sum /usr/local/bin/server-guard').split()[0]
    assert len(installed_hash) == 64 and all(c in '0123456789abcdef' for c in installed_hash)
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
    sse_clock = process_snapshot({'agent':agent_pid})
    sse_clock_offset = sse_clock['clock_offset_seconds']
    sse_thread = threading.Thread(target=observe_sse, daemon=True)
    sse_thread.start()
    monitored = observe_phase({'agent': agent_pid, 'mcp_bridge': bridge_pid}, with_mcp=True)
    sse_stop.set()
    sse_thread.join(timeout=3)
    assert not sse_errors, sse_errors
    assert len(sse_ages) >= a.phase_seconds - 5, 'SSE did not produce continuous live samples'
    monitored['sse_samples'] = len(sse_ages)
    monitored['sse_sample_age_seconds_max'] = round(max(sse_ages), 3)
    monitored['sse_estimated_server_clock_offset_seconds'] = round(sse_clock_offset, 3)
    monitored['sse_clock_estimate_uncertainty_seconds'] = round(sse_clock['clock_uncertainty_seconds'], 3)
    current = tool('current_metrics', {}, 1000)
    verified_processes = process_identities(current['processes'])
    discovery = request('inventory')
    rules = request('rules')
    small_partition_checks = []
    for disk in current['disks']:
        if disk['total'] < 10 * 1024**3:
            absolute = [rule for rule in rules if rule['source'] == disk['id']
                and rule['metric'] == 'disk_free' and rule['threshold'] == 1024**3 and rule['enabled']]
            assert not absolute, 'Small partition retained an enabled absolute 1GiB default'
            percentages = [rule for rule in rules if rule['source'] == disk['id'] and rule['metric'] == 'disk_used' and rule['enabled']]
            assert len(percentages) >= 2, 'Small partition lost its percentage protection'
            small_partition_checks.append({'mount':disk['mount'],'total_bytes':disk['total'],
                'absolute_one_gib_rules':len(absolute),'percentage_rules':len(percentages)})
    diagnostics = [{ 'source': item['source'], 'metric': item['metric'], 'label': item['label'],
        'quality': item['reading']['quality'], 'value': item['reading']['value'],
        'unit': item['reading']['unit'], 'reason': item['reading'].get('reason') }
        for item in current.get('diagnostics', [])]
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
            input=json.dumps(payload),text=True,capture_output=True,timeout=90)
        if capture.returncode:
            raise RuntimeError(capture.stderr.strip() or 'Remote screenshot capture failed')
        screenshots = json.loads(capture.stdout)
    assert remote('sha256sum /usr/local/bin/server-guard').split()[0] == installed_hash, 'Installed binary changed during acceptance'
    result = {'result':'passed','transport':'SSH stdio -> scoped local IPC',
        'installed_binary_sha256':installed_hash,
        'dashboard_tunnel':{'local_port':a.local_port,'remote_port':a.port,'authenticated':True},
        'os':inventory['os'],'cores':inventory['cores'],'memory_total':current['memory_total'],
        'cpu_percent':current['cpu']['value'],'memory_percent':current['memory']['value'],
        'linux_diagnostics_after_warmup':diagnostics,
        'top_process_identity_verification':verified_processes,
        'service_inventory':discovery['services'],
        'hosted_domain_declarations':len(discovery['sites']),
        'small_partition_defaults':small_partition_checks,
        'actual_history_rows':len(history['rows']),'synthetic_history':False,
        'key_revoked':True,'private_key_file_removed':True,'secret_in_client_config':False,'owner_account_changed':False,'screenshots':screenshots,
        'performance':{'baseline_without_mcp':baseline,'sse_and_mcp':monitored,
            'target_cpu_below_one_percent':all(v['processes']['agent']['cpu_percent_one_core'] < 1 for v in [baseline,monitored]),
            'target_memory_below_100mib':all(v['processes']['agent']['rss_peak_bytes'] < 100*1024*1024 for v in [baseline,monitored]),
            'target_live_age_below_two_seconds':max(baseline['sample_age_seconds_max'],monitored['sample_age_seconds_max'],monitored['sse_sample_age_seconds_max']) < 2,
            'note':'Short bounded observation on a shared real host, not a stress test or long-term performance guarantee. Sample ages use an SSH round-trip midpoint estimate of server clock offset; network asymmetry remains measurement uncertainty.'}}
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
