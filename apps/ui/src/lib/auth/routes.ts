const PUBLIC_AUTH_PATHS = ['/login', '/signup', '/auth/callback'];

export function isPublicAuthPath(pathname: string): boolean {
  return PUBLIC_AUTH_PATHS.some((publicPath) => {
    if (pathname === publicPath) return true;
    return publicPath.endsWith('/callback') && pathname.startsWith(`${publicPath}/`);
  });
}

export function sanitizeNextPath(candidate: string | null | undefined, fallback = '/'): string {
  if (!candidate) return fallback;
  const trimmed = candidate.trim();
  if (!trimmed.startsWith('/')) return fallback;
  if (trimmed.startsWith('//')) return fallback;
  if (isPublicAuthPath(trimmed)) return fallback;
  return trimmed;
}
