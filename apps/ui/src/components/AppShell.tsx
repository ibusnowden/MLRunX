'use client';

import { useEffect, useMemo, useState } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import { Sidebar } from './Sidebar';
import { useSidebar } from './SidebarProvider';
import { api, getStoredApiConfig } from '@/lib/api';
import { isPublicAuthPath } from '@/lib/auth/routes';

const MenuIcon = () => (
  <svg className="h-5 w-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={2}
      d="M4 6h16M4 12h16M4 18h16"
    />
  </svg>
);

function getPageTitle(pathname: string): string {
  if (pathname === '/') return 'Runs';
  if (pathname === '/onboarding') return 'Onboarding';
  if (pathname.startsWith('/runs/')) return 'Run Details';
  if (pathname === '/compare') return 'Compare';
  if (pathname === '/settings') return 'Access Console';
  if (pathname === '/admin') return 'Platform Admin';
  if (pathname === '/login') return 'Login';
  if (pathname === '/signup') return 'Sign Up';
  return 'MLRunX';
}

export function AppShell({ children }: { children: React.ReactNode }) {
  const router = useRouter();
  const pathname = usePathname();
  const { toggleMobile } = useSidebar();
  const pageTitle = useMemo(() => getPageTitle(pathname), [pathname]);
  const [uiSessionAuthorized, setUiSessionAuthorized] = useState(false);
  const [uiSessionChecked, setUiSessionChecked] = useState(false);
  const isPublicPath = isPublicAuthPath(pathname);
  const hasStoredApiKey = getStoredApiConfig().apiKey.trim().length > 0;

  useEffect(() => {
    let cancelled = false;

    if (isPublicPath || hasStoredApiKey) {
      return () => {
        cancelled = true;
      };
    }

    const verify = async () => {
      try {
        await api.getUiSession();
        if (!cancelled) {
          setUiSessionAuthorized(true);
          setUiSessionChecked(true);
        }
      } catch {
        if (!cancelled) {
          const nextPath = pathname.startsWith('/') ? pathname : '/';
          router.replace(`/login?next=${encodeURIComponent(nextPath)}`);
          setUiSessionAuthorized(false);
          setUiSessionChecked(true);
        }
      }
    };

    void verify();
    return () => {
      cancelled = true;
    };
  }, [hasStoredApiKey, isPublicPath, pathname, router]);

  if (isPublicPath) {
    return <div className="min-h-screen bg-background">{children}</div>;
  }

  const isAuthorized = hasStoredApiKey || uiSessionAuthorized;
  const isChecking = !hasStoredApiKey && !uiSessionChecked;

  if (isChecking || !isAuthorized) {
    return (
      <div className="min-h-screen bg-background flex items-center justify-center">
        <div className="rounded-lg border border-border bg-surface px-4 py-3 text-sm text-text-secondary">
          Checking session...
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-background md:pl-56">
      <Sidebar />
      <div className="min-h-screen">
        <header className="sticky top-0 z-30 flex items-center gap-3 border-b border-border bg-surface px-4 py-3 md:hidden">
          <button
            type="button"
            onClick={toggleMobile}
            className="rounded-md border border-border p-2 text-text-secondary hover:bg-surface-secondary hover:text-text-primary transition-colors"
            aria-label="Open navigation menu"
          >
            <MenuIcon />
          </button>
          <span className="text-sm font-semibold text-text-primary">{pageTitle}</span>
        </header>
        {children}
      </div>
    </div>
  );
}
