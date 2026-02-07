'use client';

import { usePathname } from 'next/navigation';
import Link from 'next/link';
import { useTheme } from './ThemeProvider';
import { useSidebar } from './SidebarProvider';

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

const SettingsIcon = () => (
  <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.75} d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.75} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
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

const LogoIcon = () => (
  <svg className="w-7 h-7" viewBox="0 0 32 32" fill="none">
    <rect width="32" height="32" rx="8" className="fill-accent" />
    <path d="M8 22V10l5 6 5-6v12" stroke="white" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" />
    <path d="M22 10v12" stroke="white" strokeWidth="2.5" strokeLinecap="round" />
    <circle cx="22" cy="10" r="1.5" fill="white" />
  </svg>
);

const NAV_ITEMS = [
  { href: '/', label: 'Home', icon: HomeIcon },
  { href: '/compare', label: 'Compare', icon: CompareIcon },
];

const BOTTOM_ITEMS = [
  { href: '/settings', label: 'Settings', icon: SettingsIcon },
];

export function Sidebar() {
  const pathname = usePathname();
  const { toggleTheme, isDark } = useTheme();
  const { collapsed, toggleCollapsed } = useSidebar();

  const isActive = (href: string) => {
    if (href === '/') return pathname === '/';
    return pathname.startsWith(href);
  };

  return (
    <aside
      className={`fixed top-0 left-0 h-screen z-50 flex flex-col border-r transition-all duration-200
        bg-sidebar-bg border-sidebar-border
        ${collapsed ? 'w-16' : 'w-56'}`}
    >
      {/* Logo */}
      <div className={`flex items-center h-16 border-b border-sidebar-border px-4 ${collapsed ? 'justify-center' : 'gap-3'}`}>
        <LogoIcon />
        {!collapsed && (
          <div className="flex flex-col">
            <span className="text-base font-bold text-text-primary tracking-tight">MLRunX</span>
            <span className="text-[10px] text-text-muted leading-none">Experiment Tracking</span>
          </div>
        )}
      </div>

      {/* Navigation */}
      <nav className="flex-1 py-3 px-2 space-y-1">
        {NAV_ITEMS.map((item) => {
          const active = isActive(item.href);
          return (
            <Link
              key={item.href}
              href={item.href}
              className={`flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors
                ${active
                  ? 'bg-sidebar-active text-accent'
                  : 'text-sidebar-text hover:bg-sidebar-hover hover:text-sidebar-text-active'
                }
                ${collapsed ? 'justify-center' : ''}
              `}
              title={collapsed ? item.label : undefined}
            >
              <item.icon />
              {!collapsed && <span>{item.label}</span>}
            </Link>
          );
        })}
      </nav>

      {/* Bottom section */}
      <div className="border-t border-sidebar-border py-3 px-2 space-y-1">
        {BOTTOM_ITEMS.map((item) => {
          const active = isActive(item.href);
          return (
            <Link
              key={item.href}
              href={item.href}
              className={`flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors
                ${active
                  ? 'bg-sidebar-active text-accent'
                  : 'text-sidebar-text hover:bg-sidebar-hover hover:text-sidebar-text-active'
                }
                ${collapsed ? 'justify-center' : ''}
              `}
              title={collapsed ? item.label : undefined}
            >
              <item.icon />
              {!collapsed && <span>{item.label}</span>}
            </Link>
          );
        })}

        {/* Theme toggle */}
        <button
          onClick={toggleTheme}
          className={`flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors w-full
            text-sidebar-text hover:bg-sidebar-hover hover:text-sidebar-text-active
            ${collapsed ? 'justify-center' : ''}`}
          title={collapsed ? (isDark ? 'Light mode' : 'Dark mode') : undefined}
        >
          {isDark ? <SunIcon /> : <MoonIcon />}
          {!collapsed && <span>{isDark ? 'Light Mode' : 'Dark Mode'}</span>}
        </button>

        {/* Collapse toggle */}
        <button
          onClick={toggleCollapsed}
          className={`flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors w-full
            text-sidebar-text hover:bg-sidebar-hover hover:text-sidebar-text-active
            ${collapsed ? 'justify-center' : ''}`}
          title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
        >
          <CollapseIcon collapsed={collapsed} />
          {!collapsed && <span>Collapse</span>}
        </button>
      </div>
    </aside>
  );
}
