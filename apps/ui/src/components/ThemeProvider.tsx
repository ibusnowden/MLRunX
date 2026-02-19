'use client';

import { createContext, useContext, useEffect, useState, useCallback } from 'react';

type Theme = 'light' | 'dark';

interface ThemeContextValue {
  theme: Theme;
  toggleTheme: () => void;
  isDark: boolean;
}

const ThemeContext = createContext<ThemeContextValue>({
  theme: 'dark',
  toggleTheme: () => {},
  isDark: true,
});

export function useTheme() {
  return useContext(ThemeContext);
}

function getInitialTheme(): Theme {
  if (typeof window === 'undefined') return 'dark';
  // Read from DOM class set by the inline script in layout.tsx
  if (document.documentElement.classList.contains('light')) return 'light';
  if (document.documentElement.classList.contains('dark')) return 'dark';
  // Fallback to localStorage
  const stored = localStorage.getItem('mlrunx-theme') as Theme | null;
  if (stored === 'light' || stored === 'dark') return stored;
  return 'dark';
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [theme, setTheme] = useState<Theme>(getInitialTheme);

  // Sync theme to <html> class and localStorage after mount
  useEffect(() => {
    const root = document.documentElement;
    root.classList.remove('light', 'dark');
    root.classList.add(theme);
    localStorage.setItem('mlrunx-theme', theme);
  }, [theme]);

  const toggleTheme = useCallback(() => {
    const root = document.documentElement;
    root.classList.add('theme-transition');
    setTheme((prev) => (prev === 'dark' ? 'light' : 'dark'));
    // Remove transition class after animation completes
    setTimeout(() => {
      root.classList.remove('theme-transition');
    }, 300);
  }, []);

  const value: ThemeContextValue = {
    theme,
    toggleTheme,
    isDark: theme === 'dark',
  };

  return (
    <ThemeContext.Provider value={value}>
      {children}
    </ThemeContext.Provider>
  );
}
