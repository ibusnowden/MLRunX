import { describe, expect, it } from 'vitest';
import { isPublicAuthPath, sanitizeNextPath } from './routes';

describe('auth route helpers', () => {
  it('identifies public auth paths', () => {
    expect(isPublicAuthPath('/login')).toBe(true);
    expect(isPublicAuthPath('/signup')).toBe(true);
    expect(isPublicAuthPath('/auth/callback')).toBe(true);
    expect(isPublicAuthPath('/')).toBe(false);
  });

  it('sanitizes next paths to prevent redirect abuse', () => {
    expect(sanitizeNextPath('/onboarding')).toBe('/onboarding');
    expect(sanitizeNextPath('https://evil.com')).toBe('/');
    expect(sanitizeNextPath('//evil.com')).toBe('/');
    expect(sanitizeNextPath('/login')).toBe('/');
    expect(sanitizeNextPath(undefined, '/dashboard')).toBe('/dashboard');
  });
});
