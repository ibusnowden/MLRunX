import { useEffect, useRef } from 'react';

interface UseAutoRefreshOptions {
  enabled?: boolean;
  intervalMs: number;
  runOnMount?: boolean;
}

export function useAutoRefresh(
  callback: () => Promise<void> | void,
  { enabled = true, intervalMs, runOnMount = false }: UseAutoRefreshOptions
) {
  const callbackRef = useRef(callback);
  const inFlightRef = useRef(false);

  useEffect(() => {
    callbackRef.current = callback;
  }, [callback]);

  useEffect(() => {
    if (!enabled || intervalMs <= 0 || typeof window === 'undefined') {
      return;
    }

    let isStopped = false;

    const run = async () => {
      if (isStopped || document.visibilityState === 'hidden' || inFlightRef.current) {
        return;
      }

      inFlightRef.current = true;
      try {
        await callbackRef.current();
      } finally {
        inFlightRef.current = false;
      }
    };

    if (runOnMount) {
      void run();
    }

    const intervalId = window.setInterval(() => {
      void run();
    }, intervalMs);

    const onVisibilityChange = () => {
      if (document.visibilityState === 'visible') {
        void run();
      }
    };
    document.addEventListener('visibilitychange', onVisibilityChange);

    return () => {
      isStopped = true;
      window.clearInterval(intervalId);
      document.removeEventListener('visibilitychange', onVisibilityChange);
    };
  }, [enabled, intervalMs, runOnMount]);
}
