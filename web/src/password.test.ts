import {describe, expect, it} from 'vitest';
import {PASSWORD_REQUIREMENTS, validatePassword} from './password';

describe('password policy', () => {
  it.each(['abcdefg', '1234567', 'é'.repeat(7), '🦉'.repeat(7)])('rejects seven characters: %s', password => {
    expect(() => validatePassword(password)).toThrow(PASSWORD_REQUIREMENTS);
  });
  it.each(['abcdefgh', '12345678', 'é'.repeat(8), '🦉'.repeat(8)])('accepts eight characters without complexity requirements: %s', password => {
    expect(() => validatePassword(password)).not.toThrow();
  });
  it.each([['a', 256], ['é', 128], ['🦉', 64]] as const)('keeps the 256-byte UTF-8 boundary for %s', (character, count) => {
    expect(() => validatePassword(character.repeat(count))).not.toThrow();
    expect(() => validatePassword(character.repeat(count + 1))).toThrow(PASSWORD_REQUIREMENTS);
  });
});
