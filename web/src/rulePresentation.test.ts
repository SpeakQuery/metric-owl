import { describe, expect, it } from 'vitest';
import { basicRules, conditionText, metrics, newRule, recoveryFor, secondsLabel, sourceHelp, sourceOptions, thresholdLabel, timingText } from './rulePresentation';
import type { Rule } from './generated/Rule';
import type { Sample } from './generated/Sample';
const rule = (changes: Partial<Rule> = {}): Rule => ({ id: 'cpu-warning', name: 'CPU warning', metric: 'cpu', source: 'host', threshold: 80, operator: '>', window_seconds: 300, fraction: .8, severity: 'warning', recovery_threshold: 75, recovery_seconds: 120, enabled: true, notify: true, ...changes });
describe('plain language alerts', () => {
  it('never describes a partial window as continuous', () => {
    expect(timingText(rule())).toBe('For at least 4 minutes within 5 minutes');
    expect(timingText(rule({ fraction: 1 }))).toBe('For 5 minutes');
    expect(secondsLabel(10)).toBe('10 seconds');
    expect(timingText(rule({ window_seconds: 180 }))).toBe('For at least 2 minutes 24 seconds within 3 minutes');
  });
  it('shows storage and network in human units, preserving stored bytes', () => {
    const disk = rule({ metric: 'disk_free', operator: '<', threshold: 1073741824 });
    expect(conditionText(disk)).toBe('Free disk space below 1.0 GiB');
    expect(disk.threshold).toBe(1073741824);
    expect(thresholdLabel(rule({ metric: 'network_in', threshold: 1048576 }))).toBe('1 MiB/s');
  });
  it('explains all supported metrics, including temporary storage and file limits', () => {
    expect(Object.keys(metrics)).toHaveLength(20);
    expect(metrics.inode_used.help).toContain('even when storage space remains');
    expect(sourceHelp(rule({ metric: 'disk_used', source: '/tmp' }))).toContain('not temperature');
    expect(sourceHelp(rule({ metric: 'cpu' }))).toBe('');
  });
  it('keeps recovery on the correct side when changing comparator or byte threshold', () => {
    expect(recoveryFor(rule(), 85)).toBe(80);
    expect(recoveryFor(rule(), 20, '<')).toBe(25);
    const disk = rule({ metric: 'disk_free', threshold: 1024 ** 3, recovery_threshold: 1.3 * 1024 ** 3 });
    expect(recoveryFor(disk, 2 * 1024 ** 3, '<')).toBeCloseTo(2.3 * 1024 ** 3);
  });
  it('Basic selection does not mutate, enable, reseed, or discard custom rules', () => {
    const saved = [rule({ threshold: 63, enabled: false }), rule({ id: 'custom', metric: 'inode_used', source: '/tmp' })];
    const before = structuredClone(saved);
    const basic = basicRules(saved, null);
    expect(basic).toHaveLength(6);
    expect(basic[0].rule?.threshold).toBe(63);
    expect(basic[0].rule?.enabled).toBe(false);
    expect(basic[1].rule).toBeUndefined();
    expect(saved).toEqual(before);
    expect(basicRules([], null).every(item => !item.rule)).toBe(true);
  });
  it('Basic uses the main disk rather than a boot partition from ordering', () => {
    const saved = [rule({ id: 'disk-boot', metric: 'disk_used', source: '/boot' }), rule({ id: 'disk-root', metric: 'disk_used', source: '/' })];
    expect(basicRules(saved, null)[4].rule?.id).toBe('disk-root');
  });
  it('prefers the sampled root filesystem when no root rule has been saved', () => {
    const sample = { disks: [{ id: '/boot', mount: '/boot' }, { id: '/', mount: '/' }] } as Sample;
    const saved = [rule({ id: 'disk-boot', metric: 'disk_used', source: '/boot' })];
    expect(basicRules(saved, sample)[4].source).toBe('/');
    expect(basicRules(saved, sample)[4].rule).toBeUndefined();
    expect(saved).toHaveLength(1);
  });
  it('retains unavailable sources and offers matching configured monitors by name', () => {
    const target = { id: 'site-1', kind: 'http', name: 'Shop website', address: 'https://example.test', enabled: false };
    const sources = sourceOptions('http', null, [target], [rule({ source: 'old-site', metric: 'http' })]);
    expect(sources).toEqual([{ value: 'site-1', label: 'Shop website (monitor paused)' }, { value: 'old-site', label: 'old-site' }]);
    expect(sourceOptions('cpu', null, [], [])).toEqual([{ value: 'host', label: 'This server' }]);
  });
  it('new simple alerts use continuous windows and appropriate free-space recovery', () => {
    expect(newRule().fraction).toBe(1);
    expect(newRule('disk_free', '/').recovery_threshold).toBeGreaterThan(1024 ** 3);
    expect(newRule('tls_days', 'site').operator).toBe('<');
  });
});
