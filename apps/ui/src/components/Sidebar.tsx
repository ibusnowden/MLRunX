'use client';

import { usePathname, useRouter } from 'next/navigation';
import Link from 'next/link';
import { useState } from 'react';
import { useTheme } from './ThemeProvider';
import { useSidebar } from './SidebarProvider';
import { api, clearStoredApiConfig } from '@/lib/api';
import { signOutSupabase } from '@/lib/auth/supabase';

// ── Icons ──

const HomeIcon = () => (
  <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.75} d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6" />
  </svg>
);

const CompareIcon = () => (
  <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.75} d="M9 19v-6a2 2 0 00-2-2H5a2 2 0 00-2 2v6a2 2 0 002 2h2a2 2 0 002-2zm0 0V9a2 2 0 012-2h2a2 2 0 012 2v10m-6 0a2 2 0 002 2h2a2 2 0 002-2m0 0V5a2 2 0 012-2h2a2 2 0 012 2v14a2 2 0 01-2 2h-2a2 2 0 01-2-2z" />
  </svg>
);

const OnboardingIcon = () => (
  <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={1.75}
      d="M12 3l2.2 4.8L19 10l-4.8 2.2L12 17l-2.2-4.8L5 10l4.8-2.2L12 3z"
    />
  </svg>
);

const SettingsIcon = () => (
  <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.75} d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.75} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
  </svg>
);

const ShieldIcon = () => (
  <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={1.75}
      d="M12 3l7.5 3v6.5c0 4.15-2.66 7.86-6.6 9.2L12 22l-.9-.3c-3.94-1.34-6.6-5.05-6.6-9.2V6L12 3z"
    />
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.75} d="M9.5 12l1.8 1.8 3.2-3.2" />
  </svg>
);

const ProfileIcon = () => (
  <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.75} d="M15.75 6.75a3.75 3.75 0 11-7.5 0 3.75 3.75 0 017.5 0zM4.5 20.12a7.5 7.5 0 0115 0" />
  </svg>
);

const ApiKeyIcon = () => (
  <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.75} d="M15.75 5.25a3 3 0 014.24 4.24l-5.92 5.92a4.5 4.5 0 01-6.36 0l-.2-.2a4.5 4.5 0 010-6.36l1.44-1.43" />
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.75} d="M9.75 18.75l2.25-2.25M13.5 15l2.25-2.25" />
  </svg>
);

const FolderIcon = () => (
  <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={1.75}
      d="M3.75 7.5A2.25 2.25 0 016 5.25h4.19a2.25 2.25 0 011.59.66l.81.84a2.25 2.25 0 001.59.66H18A2.25 2.25 0 0120.25 9.5v7A2.25 2.25 0 0118 18.75H6a2.25 2.25 0 01-2.25-2.25v-9z"
    />
  </svg>
);

const LogoutIcon = () => (
  <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.75} d="M15.75 9V5.625A2.625 2.625 0 0013.125 3h-6.75A2.625 2.625 0 003.75 5.625v12.75A2.625 2.625 0 006.375 21h6.75a2.625 2.625 0 002.625-2.625V15" />
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.75} d="M18 12H9m9 0l-3-3m3 3l-3 3" />
  </svg>
);

const SunIcon = () => (
  <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.75} d="M12 3v1m0 16v1m9-9h-1M4 12H3m15.364 6.364l-.707-.707M6.343 6.343l-.707-.707m12.728 0l-.707.707M6.343 17.657l-.707.707M16 12a4 4 0 11-8 0 4 4 0 018 0z" />
  </svg>
);

const MoonIcon = () => (
  <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.75} d="M20.354 15.354A9 9 0 018.646 3.646 9.003 9.003 0 0012 21a9.003 9.003 0 008.354-5.646z" />
  </svg>
);

const CollapseIcon = ({ collapsed }: { collapsed: boolean }) => (
  <svg className={`w-4 h-4 transition-transform ${collapsed ? 'rotate-180' : ''}`} fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M11 19l-7-7 7-7m8 14l-7-7 7-7" />
  </svg>
);

const CloseIcon = () => (
  <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
  </svg>
);

const LogoIcon = () => (
  <svg className="w-7 h-7" viewBox="0 0 32 32" fill="none">
    <rect width="32" height="32" rx="8" className="fill-accent" />
    <path d="M8 22V10l5 6 5-6v12" stroke="white" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" />
    <path d="M22 10v12" stroke="white" strokeWidth="2.5" strokeLinecap="round" />
    <circle cx="22" cy="10" r="1.5" fill="white" />
  </svg>
);

const NAV_ITEMS = [
  { href: '/onboarding', label: 'Onboarding', icon: OnboardingIcon },
  { href: '/', label: 'Home', icon: HomeIcon },
  { href: '/compare', label: 'Compare', icon: CompareIcon },
];

const ACCOUNT_ITEMS = [
  { href: '/settings#profile', label: 'Profile', icon: ProfileIcon },
  { href: '/settings#projects', label: 'Projects', icon: FolderIcon },
  { href: '/settings#api-keys', label: 'API Keys', icon: ApiKeyIcon },
  { href: '/settings#settings', label: 'Settings', icon: SettingsIcon },
  { href: '/admin', label: 'Admin', icon: ShieldIcon },
];

export function Sidebar() {
  const router = useRouter();
  const pathname = usePathname();
  const { toggleTheme, isDark } = useTheme();
  const { collapsed, toggleCollapsed, mobileOpen, closeMobile } = useSidebar();
  const [loggingOut, setLoggingOut] = useState(false);

  const isActive = (href: string) => {
    if (href.startsWith('/settings#')) return pathname === '/settings';
    if (href === '/') return pathname === '/';
    return pathname.startsWith(href);
  };

  const handleLogout = async () => {
    setLoggingOut(true);
    try {
      await api.logoutUiSession();
    } catch {
      // Ignore UI-session logout failures and continue best-effort sign-out.
    }

    try {
      await signOutSupabase();
    } catch {
      // Ignore Supabase logout failures and still clear local browser state.
    }

    clearStoredApiConfig();
    closeMobile();
    router.replace('/login');
    setLoggingOut(false);
  };

  return (
    <>
      <div
        className={`fixed inset-0 z-40 bg-black/45 transition-opacity duration-200 md:hidden ${
          mobileOpen ? 'opacity-100' : 'pointer-events-none opacity-0'
        }`}
        onClick={closeMobile}
        aria-hidden
      />

      <aside
        className={`fixed top-0 left-0 z-50 flex h-screen w-72 flex-col border-r border-sidebar-border bg-sidebar-bg transition-transform duration-200 md:transition-all ${
          collapsed ? 'md:w-16' : 'md:w-56'
        } ${mobileOpen ? 'translate-x-0' : '-translate-x-full md:translate-x-0'}`}
      >
        <div className="flex items-center gap-3 h-16 border-b border-sidebar-border px-4">
          <LogoIcon />
          <div className={`flex flex-col ${collapsed ? 'md:hidden' : ''}`}>
            <span className="text-base font-bold text-text-primary tracking-tight">MLRunX</span>
            <span className="text-[10px] text-text-muted leading-none">Experiment Tracking</span>
          </div>
          <button
            type="button"
            onClick={closeMobile}
            className="ml-auto rounded-md border border-sidebar-border p-1.5 text-sidebar-text hover:bg-sidebar-hover hover:text-sidebar-text-active transition-colors md:hidden"
            aria-label="Close navigation menu"
          >
            <CloseIcon />
          </button>
        </div>

        <nav className="flex-1 py-3 px-2 space-y-1">
          {NAV_ITEMS.map((item) => {
            const active = isActive(item.href);
            return (
              <Link
                key={item.href}
                href={item.href}
                onClick={closeMobile}
                className={`flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors ${
                  active
                    ? 'bg-sidebar-active text-accent'
                    : 'text-sidebar-text hover:bg-sidebar-hover hover:text-sidebar-text-active'
                } ${collapsed ? 'md:justify-center' : ''}`}
                title={collapsed ? item.label : undefined}
              >
                <item.icon />
                <span className={collapsed ? 'md:hidden' : ''}>{item.label}</span>
              </Link>
            );
          })}
        </nav>

        <div className="border-t border-sidebar-border py-3 px-2 space-y-1">
          <p className={`px-3 pb-1 text-[10px] font-semibold uppercase tracking-[0.08em] text-text-muted ${collapsed ? 'md:hidden' : ''}`}>
            Account
          </p>
          {ACCOUNT_ITEMS.map((item) => {
            const active = isActive(item.href);
            return (
              <Link
                key={item.href}
                href={item.href}
                onClick={closeMobile}
                className={`flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors ${
                  active
                    ? 'bg-sidebar-active text-accent'
                    : 'text-sidebar-text hover:bg-sidebar-hover hover:text-sidebar-text-active'
                } ${collapsed ? 'md:justify-center' : ''}`}
                title={collapsed ? item.label : undefined}
              >
                <item.icon />
                <span className={collapsed ? 'md:hidden' : ''}>{item.label}</span>
              </Link>
            );
          })}

          <button
            type="button"
            onClick={() => void handleLogout()}
            disabled={loggingOut}
            className={`flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors w-full text-sidebar-text hover:bg-sidebar-hover hover:text-sidebar-text-active disabled:opacity-60 disabled:cursor-not-allowed ${
              collapsed ? 'md:justify-center' : ''
            }`}
            title={collapsed ? 'Log out' : undefined}
          >
            <LogoutIcon />
            <span className={collapsed ? 'md:hidden' : ''}>{loggingOut ? 'Logging out...' : 'Log out'}</span>
          </button>

          <button
            type="button"
            onClick={toggleTheme}
            className={`flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors w-full text-sidebar-text hover:bg-sidebar-hover hover:text-sidebar-text-active ${
              collapsed ? 'md:justify-center' : ''
            }`}
            title={collapsed ? (isDark ? 'Light mode' : 'Dark mode') : undefined}
          >
            {isDark ? <SunIcon /> : <MoonIcon />}
            <span className={collapsed ? 'md:hidden' : ''}>{isDark ? 'Light Mode' : 'Dark Mode'}</span>
          </button>

          <button
            type="button"
            onClick={toggleCollapsed}
            className={`hidden md:flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors w-full text-sidebar-text hover:bg-sidebar-hover hover:text-sidebar-text-active ${
              collapsed ? 'justify-center' : ''
            }`}
            title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
          >
            <CollapseIcon collapsed={collapsed} />
            {!collapsed && <span>Collapse</span>}
          </button>
        </div>
      </aside>
    </>
  );
}
