'use client';

import { useMemo } from 'react';
import { usePathname } from 'next/navigation';
import { Sidebar } from './Sidebar';
import { useSidebar } from './SidebarProvider';

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
  if (pathname.startsWith('/runs/')) return 'Run Details';
  if (pathname === '/compare') return 'Compare';
  if (pathname === '/settings') return 'Settings';
  return 'MLRunX';
}

export function AppShell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const { collapsed, toggleMobile } = useSidebar();
  const pageTitle = useMemo(() => getPageTitle(pathname), [pathname]);

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
