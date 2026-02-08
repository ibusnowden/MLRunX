'use client';

import { createContext, useCallback, useContext, useMemo, useState } from 'react';

interface SidebarContextValue {
  collapsed: boolean;
  mobileOpen: boolean;
  toggleCollapsed: () => void;
  openMobile: () => void;
  closeMobile: () => void;
  toggleMobile: () => void;
}

const SidebarContext = createContext<SidebarContextValue>({
  collapsed: false,
  mobileOpen: false,
  toggleCollapsed: () => {},
  openMobile: () => {},
  closeMobile: () => {},
  toggleMobile: () => {},
});

export function useSidebar() {
  return useContext(SidebarContext);
}

export function SidebarProvider({ children }: { children: React.ReactNode }) {
  const [collapsed, setCollapsed] = useState(false);
  const [mobileOpen, setMobileOpen] = useState(false);

  const toggleCollapsed = useCallback(() => {
    setCollapsed((prev) => !prev);
  }, []);

  const openMobile = useCallback(() => {
    setMobileOpen(true);
  }, []);

  const closeMobile = useCallback(() => {
    setMobileOpen(false);
  }, []);

  const toggleMobile = useCallback(() => {
    setMobileOpen((prev) => !prev);
  }, []);

  const value = useMemo(
    () => ({
      collapsed,
      mobileOpen,
      toggleCollapsed,
      openMobile,
      closeMobile,
      toggleMobile,
    }),
    [collapsed, mobileOpen, toggleCollapsed, openMobile, closeMobile, toggleMobile]
  );

  return (
    <SidebarContext.Provider value={value}>
      {children}
    </SidebarContext.Provider>
  );
}
