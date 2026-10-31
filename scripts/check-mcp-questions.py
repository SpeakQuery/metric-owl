#!/usr/bin/env python3
"""Read-only acceptance using the exact MCP command registered in local Codex.

No alternate service, SSH identity, server credential or server-side file is created.
The server scope grant is authoritative. Output can contain private monitoring data.
"""
import argparse
import json
from pathlib import Path
import selectors
import subprocess
import time

parser = argparse.ArgumentParser()
parser.add_argument('--server', required=True, help='Existing server name registered in this Mac\'s Codex configuration')
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--extended', action='store_true')
args = parser.parse_args()
config = json.loads(subprocess.check_output(['codex', 'mcp', 'get', args.server, '--json'], text=True))
assert config['enabled'] and config['transport']['type'] == 'stdio'
transport = config['transport']
bridge = subprocess.Popen([transport['command'], *transport['args']], stdin=subprocess.PIPE,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
sequence = 0
result = {'transport': 'Exact local Codex MCP configuration; direct protocol acceptance client',
          'server_name': args.server, 'checks': {}, 'errors': {}}

def rpc(method, params=None):
    global sequence
    sequence += 1
    message = {'jsonrpc': '2.0', 'id': sequence, 'method': method}
    if params is not None:
        message['params'] = params
    bridge.stdin.write(json.dumps(message) + '\n')
    bridge.stdin.flush()
    deadline = time.monotonic() + 15
    with selectors.DefaultSelector() as selector:
        selector.register(bridge.stdout, selectors.EVENT_READ)
        while time.monotonic() < deadline:
            if not selector.select(max(0, deadline - time.monotonic())):
                break
            raw = bridge.stdout.readline()
            if not raw:
                raise RuntimeError('MCP transport ended: ' + bridge.stderr.read()[:500])
            response = json.loads(raw)
            if response.get('id') != sequence:
                continue
            if 'error' in response:
                raise RuntimeError(str(response['error'])[:500])
            return response['result']
    raise RuntimeError('MCP response timed out')

def tool(name, values=None):
    reply = rpc('tools/call', {'name': name, 'arguments': values or {}})
    text = next(c['text'] for c in reply['content'] if c['type'] == 'text')
    if reply.get('isError'):
        raise RuntimeError(text[:500])
    return json.loads(text)

def check(identifier, name, values=None):
    try:
        result['checks'][identifier] = tool(name, values)
    except Exception as error:
        result['errors'][identifier] = str(error)

try:
    result['initialize'] = rpc('initialize', {'protocolVersion': '2025-11-25', 'capabilities': {},
                                            'clientInfo': {'name': 'metricowl-question-acceptance', 'version': '1'}})
    bridge.stdin.write(json.dumps({'jsonrpc': '2.0', 'method': 'notifications/initialized'}) + '\n')
    bridge.stdin.flush()
    result['tools'] = [t['name'] for t in rpc('tools/list')['tools']]
    result['info'] = tool('server_info')
    result['current'] = tool('current_metrics')
    at = result['current']['at']
    result['at'] = at
    queries = [
        ('diskUsed3h', '/', 'disk_used', at-10800, at),
        ('diskFree3h', '/', 'disk_free', at-10800, at),
        ('diskFree10h', '/', 'disk_free', at-36000, at),
        ('diskUsed20m', '/', 'disk_used', at-1200, at),
        ('diskUsedBaseline3h', '/', 'disk_used', at-12000, at-1200),
        ('cpu3h', 'host', 'cpu', at-10800, at),
        ('cpuEarlier3h', 'host', 'cpu', at-21600, at-10800),
        ('cpu20m', 'host', 'cpu', at-1200, at),
        ('memory3h', 'host', 'memory', at-10800, at),
        ('networkIn20m', 'host', 'network_in', at-1200, at),
        ('networkOut20m', 'host', 'network_out', at-1200, at),
        ('networkInBaseline3h', 'host', 'network_in', at-12000, at-1200),
        ('networkOutBaseline3h', 'host', 'network_out', at-12000, at-1200),
        ('pressure3h', 'host', 'memory_pressure', at-10800, at),
        ('swap3h', 'host', 'swap_used_percent', at-10800, at),
        ('oom10h', 'host', 'oom_kills_5m', at-36000, at),
        ('latency20m', '/dev/sda', 'disk_io_latency', at-1200, at),
        ('busy20m', '/dev/sda', 'disk_io_utilization', at-1200, at),
        ('iowait20m', 'host', 'cpu_iowait', at-1200, at),
    ]
    result['query_arguments'] = {}
    for identifier, source, metric, start, end in queries:
        values = {'source': source, 'metric': metric, 'from': start, 'to': end}
        result['query_arguments'][identifier] = values
        check(identifier, 'history', values)
    if args.extended:
        for identifier, metric, source, duration, compare in [
            ('reportDiskGrowth', 'disk_free', '/', 10800, False),
            ('reportDiskAnomaly', 'disk_used', '/', 1200, True),
            ('reportDisk10h', 'disk_free', '/', 36000, False),
            ('reportCpuComparison', 'cpu', 'host', 10800, True),
            ('reportCpuPeak', 'cpu', 'host', 1200, False),
            ('reportMemoryTrend', 'memory', 'host', 10800, False),
            ('reportNetworkChange', 'network_in', 'host', 1200, True),
            ('reportOOM', 'oom_kills_5m', 'host', 36000, False),
            ('reportDiskLatency', 'disk_io_latency', '/dev/sda', 1200, False),
        ]:
            values = {'source': source, 'metric': metric, 'from': at-duration, 'to': at}
            if compare:
                values.update(compare_from=at-duration-10800, compare_to=at-duration)
            check(identifier, 'metric_report', values)
        check('processHistory20m', 'process_history', {'from': at-1200, 'to': at})
        check('reportCpuMemory', 'metric_report', {'source': 'host', 'metric': 'cpu', 'related_metric': 'memory', 'from': at-10800, 'to': at})
        check('monitoredSources', 'monitored_sources')
        check('diskCapacity10h', 'history', {'source': '/', 'metric': 'disk_total', 'from': at-36000, 'to': at})
        log_sources = [s for s in result['checks'].get('monitoredSources', {}).get('items', [])
                       if s.get('kind') in ('log', 'journal')]
        if log_sources:
            check('logSummary', 'log_summary', {'id': log_sources[0]['id']})
        else:
            result['log_summary_check'] = 'Not exercised: no log or journal source is configured; do not invent a source.'
        check('monitoringHealth', 'monitoring_health')
        check('incidentList', 'list_incidents', {'offset': 0})
        incidents = result['checks'].get('incidentList', [])
        if incidents:
            check('latestIncident', 'incident_detail', {'id': incidents[0]['id']})
    result['finished_at'] = time.time()
finally:
    bridge.stdin.close()
    try:
        bridge.wait(timeout=3)
    except subprocess.TimeoutExpired:
        bridge.terminate()
        bridge.wait(timeout=3)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2))
print(json.dumps({'server': result.get('info', {}).get('name'), 'tools': result.get('tools'),
                  'successful_queries': len(result['checks']), 'errors': result['errors'], 'output': str(args.output)}))
