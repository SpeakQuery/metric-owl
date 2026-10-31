export const MIN_PASSWORD_CHARACTERS = 8;
export const PASSWORD_REQUIREMENTS = 'Password must contain at least 8 characters and at most 256 bytes';

/** Match the server's Unicode scalar count and UTF-8 byte limit. */
export function validatePassword(password: string): void {
  if ([...password].length < MIN_PASSWORD_CHARACTERS || new TextEncoder().encode(password).length > 256) {
    throw new Error(PASSWORD_REQUIREMENTS);
  }
}
