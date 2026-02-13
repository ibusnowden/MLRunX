'use client';

import Link from 'next/link';
import { usePathname, useRouter } from 'next/navigation';
import { useEffect, useMemo, useState, type ReactElement } from 'react';
import { clearStoredApiConfig, api } from '@/lib/api';
import { getSupabaseUserIdentity, signOutSupabase, type SupabaseUserIdentity } from '@/lib/auth/supabase';
import { useSidebar } from './SidebarProvider';
import { useTheme } from './ThemeProvider';

const ExperimentsIcon = () => (
  <svg className="h-[18px] w-[18px]" fill="none" viewBox="0 0 24 24" stroke="currentColor">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M9.75 3.104v5.714a2.25 2.25 0 01-.659 1.591L5 14.5m4.75-11.396c1.49-.136 3.01-.136 4.5 0m0 0v5.714c0 .597.237 1.17.659 1.591L19 14.5m0 0l.4 6.142A2.262 2.262 0 0117.156 23H6.844A2.262 2.262 0 014.6 20.642L5 14.5m14 0l-1.223 1.256A11.959 11.959 0 0012 18.004a11.959 11.959 0 00-5.777-2.248L5 14.5" />
  </svg>
);

const CompareIcon = () => (
  <svg className="h-[18px] w-[18px]" fill="none" viewBox="0 0 24 24" stroke="currentColor">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M7.5 21L3 16.5m0 0L7.5 12M3 16.5h13.5m0-13.5L21 7.5m0 0L16.5 12M21 7.5H7.5" />
  </svg>
);

const ModelsIcon = () => (
  <svg className="h-[18px] w-[18px]" fill="none" viewBox="0 0 24 24" stroke="currentColor">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M20.25 6.375c0 2.278-3.694 4.125-8.25 4.125S3.75 8.653 3.75 6.375m16.5 0c0-2.278-3.694-4.125-8.25-4.125S3.75 4.097 3.75 6.375m16.5 0v11.25c0 2.278-3.694 4.125-8.25 4.125s-8.25-1.847-8.25-4.125V6.375" />
  </svg>
);

const ProfileIcon = () => (
  <svg className="h-[18px] w-[18px]" fill="none" viewBox="0 0 24 24" stroke="currentColor">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M15.75 5.25a3 3 0 11-6 0 3 3 0 016 0zm-10.154 13.573A7.49 7.49 0 0112 15a7.49 7.49 0 016.404 3.823" />
  </svg>
);

const ApiKeyIcon = () => (
  <svg className="h-[18px] w-[18px]" fill="none" viewBox="0 0 24 24" stroke="currentColor">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M15.75 5.25a3 3 0 014.24 4.24l-5.92 5.92a4.5 4.5 0 01-6.36 0l-.2-.2a4.5 4.5 0 010-6.36l1.44-1.43m2.31 11.18l2.25-2.25M13.5 15l2.25-2.25" />
  </svg>
);

const ProjectsIcon = () => (
  <svg className="h-[18px] w-[18px]" fill="none" viewBox="0 0 24 24" stroke="currentColor">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M3.75 7.5A2.25 2.25 0 016 5.25h4.19a2.25 2.25 0 011.59.66l.81.84a2.25 2.25 0 001.59.66H18A2.25 2.25 0 0120.25 9.5v7A2.25 2.25 0 0118 18.75H6a2.25 2.25 0 01-2.25-2.25v-9z" />
  </svg>
);

const SettingsIcon = () => (
  <svg className="h-[18px] w-[18px]" fill="none" viewBox="0 0 24 24" stroke="currentColor">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
  </svg>
);

const AdminIcon = () => (
  <svg className="h-[18px] w-[18px]" fill="none" viewBox="0 0 24 24" stroke="currentColor">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M12 3l7.5 3v6.5c0 4.15-2.66 7.86-6.6 9.2L12 22l-.9-.3c-3.94-1.34-6.6-5.05-6.6-9.2V6L12 3z" />
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M9.5 12l1.8 1.8 3.2-3.2" />
  </svg>
);

const LogoutIcon = () => (
  <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M15.75 9V5.625A2.625 2.625 0 0013.125 3h-6.75A2.625 2.625 0 003.75 5.625v12.75A2.625 2.625 0 006.375 21h6.75a2.625 2.625 0 002.625-2.625V15m2.25-3H9m9 0l-3-3m3 3l-3 3" />
  </svg>
);

const ThemeIcon = ({ isDark }: { isDark: boolean }) => (
  isDark ? (
    <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M12 3v1m0 16v1m9-9h-1M4 12H3m15.364 6.364l-.707-.707M6.343 6.343l-.707-.707m12.728 0l-.707.707M6.343 17.657l-.707.707M16 12a4 4 0 11-8 0 4 4 0 018 0z" />
    </svg>
  ) : (
    <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M20.354 15.354A9 9 0 018.646 3.646 9.003 9.003 0 0012 21a9.003 9.003 0 008.354-5.646z" />
    </svg>
  )
);

const CloseIcon = () => (
  <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
  </svg>
);

const LogoGlyph = () => (
  <svg width="34" height="34" viewBox="0 0 34 34" fill="none" xmlns="http://www.w3.org/2000/svg">
    <rect width="34" height="34" rx="9" fill="url(#paint0_linear_mlrunx_logo)" />
    <path d="M8.5 21.5L13.5 12.5L17.5 17L21 13.5L25.5 21.5" stroke="white" strokeWidth="2.1" strokeLinecap="round" strokeLinejoin="round" />
    <defs>
      <linearGradient id="paint0_linear_mlrunx_logo" x1="0" y1="0" x2="34" y2="34" gradientUnits="userSpaceOnUse">
        <stop stopColor="#3B7DFF" />
        <stop offset="1" stopColor="#1A4FD4" />
      </linearGradient>
    </defs>
  </svg>
);

type NavItem = {
  href: string;
  label: string;
  icon: () => ReactElement;
};

const MAIN_ITEMS: NavItem[] = [
  { href: '/', label: 'Experiments', icon: ExperimentsIcon },
  { href: '/compare', label: 'Compare', icon: CompareIcon },
  { href: '/onboarding', label: 'Models', icon: ModelsIcon },
];

const ACCOUNT_ITEMS: NavItem[] = [
  { href: '/settings#profile', label: 'Profile', icon: ProfileIcon },
  { href: '/settings#api-keys', label: 'API Keys', icon: ApiKeyIcon },
  { href: '/settings#projects', label: 'Projects', icon: ProjectsIcon },
  { href: '/settings#settings', label: 'Settings', icon: SettingsIcon },
  { href: '/admin', label: 'Admin', icon: AdminIcon },
];

function isActivePath(pathname: string, href: string): boolean {
  if (href.startsWith('/settings#')) return pathname === '/settings';
  if (href === '/') return pathname === '/';
  return pathname.startsWith(href);
}

function getUserDisplayName(identity: SupabaseUserIdentity | null): string {
  if (!identity) return 'MLRunX User';
  if (identity.displayName?.trim()) return identity.displayName.trim();
  if (identity.email?.trim()) return identity.email.trim().split('@')[0];
  return identity.id.slice(0, 8);
}

function getUserEmail(identity: SupabaseUserIdentity | null): string {
  if (identity?.email?.trim()) return identity.email.trim();
  return 'Signed in';
}

function getInitials(name: string): string {
  const segments = name
    .split(/\s+/)
    .map((segment) => segment.trim())
    .filter(Boolean);
  if (segments.length === 0) return 'U';
  if (segments.length === 1) return segments[0].slice(0, 2).toUpperCase();
  return `${segments[0][0] ?? ''}${segments[1][0] ?? ''}`.toUpperCase();
}

function SidebarLink({
  href,
  label,
  icon: Icon,
  active,
  onClick,
}: {
  href: string;
  label: string;
  icon: NavItem['icon'];
  active: boolean;
  onClick: () => void;
}) {
  return (
    <Link
      href={href}
      onClick={onClick}
      className={`group relative flex items-center gap-3 rounded-lg px-3 py-2 text-[15px] font-medium transition-all duration-200 ${
        active
          ? 'bg-sidebar-active text-accent shadow-[inset_0_0_0_1px_rgba(59,125,255,0.16)]'
          : 'text-sidebar-text hover:bg-sidebar-hover hover:text-sidebar-text-active'
      }`}
    >
      <Icon />
      <span>{label}</span>
    </Link>
  );
}

export function Sidebar() {
  const router = useRouter();
  const pathname = usePathname();
  const { mobileOpen, closeMobile } = useSidebar();
  const { isDark, toggleTheme } = useTheme();
  const [loggingOut, setLoggingOut] = useState(false);
  const [identity, setIdentity] = useState<SupabaseUserIdentity | null>(null);

  useEffect(() => {
    let active = true;
    void getSupabaseUserIdentity()
      .then((value) => {
        if (active) {
          setIdentity(value);
        }
      })
      .catch(() => {
        if (active) {
          setIdentity(null);
        }
      });
    return () => {
      active = false;
    };
  }, []);

  const profileName = useMemo(() => getUserDisplayName(identity), [identity]);
  const profileEmail = useMemo(() => getUserEmail(identity), [identity]);
  const initials = useMemo(() => getInitials(profileName), [profileName]);

  const handleLogout = async () => {
    setLoggingOut(true);
    try {
      await api.logoutUiSession();
    } catch {
      // Continue with provider and local cleanup.
    }

    try {
      await signOutSupabase();
    } catch {
      // Continue with local cleanup.
    }

    clearStoredApiConfig();
    closeMobile();
    router.replace('/login');
    setLoggingOut(false);
  };

  return (
    <>
      <div
        aria-hidden
        className={`fixed inset-0 z-40 bg-black/55 transition-opacity duration-200 md:hidden ${
          mobileOpen ? 'opacity-100' : 'pointer-events-none opacity-0'
        }`}
        onClick={closeMobile}
      />

      <aside
        className={`fixed left-0 top-0 z-50 flex h-screen w-56 flex-col border-r border-sidebar-border bg-[linear-gradient(180deg,_rgba(15,18,24,1)_0%,_rgba(12,15,21,1)_100%)] transition-transform duration-200 ${
          mobileOpen ? 'translate-x-0' : '-translate-x-full md:translate-x-0'
        }`}
      >
        <div className="flex h-[76px] items-center gap-3 border-b border-sidebar-border px-5">
          <LogoGlyph />
          <div className="min-w-0 flex-1">
            <div className="text-base font-semibold tracking-[-0.02em] text-text-primary">
              MLRun<span className="text-accent">X</span>
            </div>
            <div className="text-xs text-text-muted">Experiment Tracking</div>
          </div>
          <button
            type="button"
            onClick={closeMobile}
            className="rounded-md border border-sidebar-border p-1.5 text-sidebar-text hover:bg-sidebar-hover hover:text-sidebar-text-active md:hidden"
            aria-label="Close navigation menu"
          >
            <CloseIcon />
          </button>
        </div>

        <nav className="flex-1 space-y-5 overflow-y-auto px-3 py-6">
          <div>
            <p className="px-2 pb-2 text-[11px] font-semibold uppercase tracking-[0.14em] text-text-muted">Main</p>
            <div className="space-y-1">
              {MAIN_ITEMS.map((item) => (
                <SidebarLink
                  key={item.href}
                  href={item.href}
                  label={item.label}
                  icon={item.icon}
                  active={isActivePath(pathname, item.href)}
                  onClick={closeMobile}
                />
              ))}
            </div>
          </div>

          <div>
            <p className="px-2 pb-2 text-[11px] font-semibold uppercase tracking-[0.14em] text-text-muted">Account</p>
            <div className="space-y-1">
              {ACCOUNT_ITEMS.map((item) => (
                <SidebarLink
                  key={item.href}
                  href={item.href}
                  label={item.label}
                  icon={item.icon}
                  active={isActivePath(pathname, item.href)}
                  onClick={closeMobile}
                />
              ))}
            </div>
          </div>
        </nav>

        <div className="border-t border-sidebar-border p-4">
          <div className="rounded-xl border border-sidebar-border bg-background px-3 py-3 shadow-[0_0_0_1px_rgba(255,255,255,0.01)]">
            <div className="flex items-center gap-3">
              <div className="flex h-9 w-9 items-center justify-center rounded-full bg-gradient-to-br from-[#6366f1] to-[#3b7dff] text-xs font-semibold text-white">
                {initials}
              </div>
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm font-medium text-text-primary">{profileName}</div>
                <div className="truncate text-xs text-text-muted">{profileEmail}</div>
              </div>
            </div>
            <div className="mt-3 flex items-center gap-2">
              <button
                type="button"
                onClick={() => void handleLogout()}
                disabled={loggingOut}
                className="inline-flex flex-1 items-center justify-center gap-1.5 rounded-md border border-sidebar-border bg-surface px-2 py-1.5 text-xs font-medium text-text-secondary transition-all duration-200 hover:-translate-y-0.5 hover:bg-surface-hover hover:text-text-primary disabled:opacity-60"
              >
                <LogoutIcon />
                {loggingOut ? 'Signing out...' : 'Sign Out'}
              </button>
              <button
                type="button"
                onClick={toggleTheme}
                className="inline-flex items-center justify-center rounded-md border border-sidebar-border bg-surface p-1.5 text-text-secondary transition-all duration-200 hover:-translate-y-0.5 hover:bg-surface-hover hover:text-text-primary"
                title={isDark ? 'Switch to light mode' : 'Switch to dark mode'}
              >
                <ThemeIcon isDark={isDark} />
              </button>
            </div>
          </div>
        </div>
      </aside>
    </>
  );
}
