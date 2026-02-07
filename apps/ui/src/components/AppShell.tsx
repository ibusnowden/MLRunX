'use client';

import { Sidebar } from './Sidebar';
import { useSidebar } from './SidebarProvider';

export function AppShell({ children }: { children: React.ReactNode }) {
  const { collapsed } = useSidebar();

  return (
    <div className="flex min-h-screen">
      <Sidebar />
      <div
        className={`flex-1 min-h-screen transition-all duration-200 ${
          collapsed ? 'ml-16' : 'ml-56'
        }`}
      >
        {children}
      </div>
    </div>
  );
}
