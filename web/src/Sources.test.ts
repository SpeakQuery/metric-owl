/// <reference types="node" />
import { describe, expect, it } from 'vitest';
import { execFileSync } from 'node:child_process';
import { blankSourceForm, jobReportCommand, targetFromForm } from './Sources';

const id = '184a8478-5bcf-4230-9378-7fdb017ad141';
describe('monitor source boundaries', () => {
  it('distinguishes an optional JSON check from a root-document check and preserves JSON value types', () => {
    const form = { ...blankSourceForm(), name: 'Health', address: 'https://example.com/health' };
    expect(targetFromForm(form, id).options?.json_pointer).toBeNull();
    const root = targetFromForm({ ...form, inspectJson: true, jsonPointer: '', compareJson: true, jsonExpected: 'true' }, id);
    expect(root.options?.json_pointer).toBe('');
    expect(root.options?.json_expected).toBe('true');
    expect(() => targetFromForm({ ...form, inspectJson: true, compareJson: true, jsonExpected: 'ready' }, id)).toThrow('valid JSON value');
  });
  it('requires verified-scheme certificate targets and rejects URL-embedded secrets and fragments', () => {
    const form = { ...blankSourceForm('tls'), name: 'Certificate', address: 'https://example.com' };
    expect(targetFromForm(form, id).kind).toBe('tls');
    for (const address of ['http://example.com', 'https://name:secret@example.com', 'https://example.com/#', 'file:///etc/passwd']) expect(() => targetFromForm({ ...form, address }, id)).toThrow();
  });
  it('delegates managed paths to the server while enforcing duration limits and explicit custom paths', () => {
    const form = { ...blankSourceForm('job'), name: 'Daily backup' };
    const target = targetFromForm(form, id);
    expect(target.address).toBe('');
    expect(target.options?.job_interval_seconds).toBe(86400);
    expect(target.options?.job_added_at).toBeNull();
    expect(() => targetFromForm({ ...form, jobGrace: '1441' }, id)).toThrow('Grace period');
    expect(() => targetFromForm({ ...form, jobRuntime: '0' }, id)).toThrow('Maximum runtime');
    expect(() => targetFromForm({ ...form, managedJob: false, address: 'relative.json' }, id)).toThrow('absolute');
  });
  it('bounds UTF-8 payload size rather than character count', () => {
    const form = { ...blankSourceForm(), name: 'Health', address: 'https://example.com', expectedText: '界'.repeat(342) };
    expect(() => targetFromForm(form, id)).toThrow('1,024 bytes');
  });
});

describe('copy-only job reporting command', () => {
  it('quotes shell metacharacters and spaces without invoking substitutions', () => {
    const connection = { command: "/Applications/Owl's app/$(printf UNEXPECTED)/server-guard", args: ['--data-dir', "/Users/Owl's data/`printf UNEXPECTED`", 'mcp'] };
    const command = jobReportCommand(connection, id, 'success', false)!;
    // Replace the executable word with a shell function so the real copied
    // argument vector is checked without executing any application or job.
    const first = command.indexOf(" '--data-dir'");
    const output = execFileSync('/bin/sh', ['-c', `capture() { printf '%s\\n' "$@"; }; capture${command.slice(first)}`], { encoding: 'utf8' });
    expect(output.split('\n').slice(0, 2)).toEqual(['--data-dir', connection.args[1]]);
    expect(command).toContain("'\\''");
    expect(command).toContain('$(printf UNEXPECTED)');
    expect(jobReportCommand(connection, id, 'success', true)).toBe(`runuser -u server-guard -- ${command}`);
  });
  it('does not emit a command for missing installation details or arbitrary states and IDs', () => {
    const connection = { command: '/usr/local/bin/server-guard', args: ['--data-dir', '/var/lib/server-guard', 'mcp'] };
    expect(jobReportCommand(null, id, 'success', false)).toBeNull();
    expect(jobReportCommand(connection, 'id; touch /tmp/anything', 'success', false)).toBeNull();
    expect(jobReportCommand(connection, id, 'success; false', false)).toBeNull();
  });
});
