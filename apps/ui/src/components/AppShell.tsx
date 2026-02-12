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
  if (pathname === '/login') return 'Login';
  if (pathname === '/signup') return 'Sign Up';
  return 'MLRunX';
}

export function AppShell({ children }: { children: React.ReactNode }) {
  const router = useRouter();
  const pathname = usePathname();
  const { collapsed, toggleMobile } = useSidebar();
  const pageTitle = useMemo(() => getPageTitle(pathname), [pathname]);
  const [authChecked, setAuthChecked] = useState(false);
  const [authorized, setAuthorized] = useState(false);

  useEffect(() => {
    let cancelled = false;

    if (isPublicAuthPath(pathname)) {
      setAuthChecked(true);
      setAuthorized(true);
      return () => {
        cancelled = true;
      };
    }

    const { apiKey } = getStoredApiConfig();
    if (apiKey.trim()) {
      setAuthChecked(true);
      setAuthorized(true);
      return () => {
        cancelled = true;
      };
    }

    setAuthChecked(false);
    setAuthorized(false);

    const verify = async () => {
      try {
        await api.getUiSession();
        if (!cancelled) {
          setAuthorized(true);
          setAuthChecked(true);
        }
      } catch {
        if (!cancelled) {
          const nextPath = pathname.startsWith('/') ? pathname : '/';
          router.replace(`/login?next=${encodeURIComponent(nextPath)}`);
          setAuthorized(false);
          setAuthChecked(true);
        }
      }
    };

    void verify();
    return () => {
      cancelled = true;
    };
  }, [pathname, router]);

  if (isPublicAuthPath(pathname)) {
    return <div className="min-h-screen bg-background">{children}</div>;
  }

  if (!authChecked || !authorized) {
    return (
      <div className="min-h-screen bg-background flex items-center justify-center">
        <div className="rounded-lg border border-border bg-surface px-4 py-3 text-sm text-text-secondary">
          Checking session...
        </div>
      </div>
    );
  }

  return (
    <div className="flex min-h-screen bg-background">
      <Sidebar />
      <div
        className={`flex-1 min-h-screen transition-all duration-200 ${
          collapsed ? 'md:ml-16' : 'md:ml-56'
        }`}
      >
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
