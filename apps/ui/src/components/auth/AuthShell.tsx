'use client';

import Link from 'next/link';
import { PropsWithChildren } from 'react';
import styles from './AuthShell.module.css';

type AuthMode = 'login' | 'signup';

interface AuthShellProps extends PropsWithChildren {
  mode: AuthMode;
}

const FeatureChartIcon = () => (
  <svg className="h-[18px] w-[18px]" fill="none" viewBox="0 0 24 24" strokeWidth={1.8} stroke="currentColor">
    <path
      strokeLinecap="round"
      strokeLinejoin="round"
      d="M3 13.125C3 12.504 3.504 12 4.125 12h2.25c.621 0 1.125.504 1.125 1.125v6.75C7.5 20.496 6.996 21 6.375 21h-2.25A1.125 1.125 0 013 19.875v-6.75zM9.75 8.625c0-.621.504-1.125 1.125-1.125h2.25c.621 0 1.125.504 1.125 1.125v11.25c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V8.625zM16.5 4.125c0-.621.504-1.125 1.125-1.125h2.25C20.496 3 21 3.504 21 4.125v15.75c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V4.125z"
    />
  </svg>
);

const FeatureCompareIcon = () => (
  <svg className="h-[18px] w-[18px]" fill="none" viewBox="0 0 24 24" strokeWidth={1.8} stroke="currentColor">
    <path strokeLinecap="round" strokeLinejoin="round" d="M7.5 21L3 16.5m0 0L7.5 12M3 16.5h13.5m0-13.5L21 7.5m0 0L16.5 12M21 7.5H7.5" />
  </svg>
);

const FeatureTeamIcon = () => (
  <svg className="h-[18px] w-[18px]" fill="none" viewBox="0 0 24 24" strokeWidth={1.8} stroke="currentColor">
    <path
      strokeLinecap="round"
      strokeLinejoin="round"
      d="M18 18.72a9.094 9.094 0 003.741-.479 3 3 0 00-4.682-2.72m.94 3.198l.001.031c0 .225-.012.447-.037.666A11.944 11.944 0 0112 21c-2.17 0-4.207-.576-5.963-1.584A6.062 6.062 0 016 18.719m12 0a5.971 5.971 0 00-.941-3.197m0 0A5.995 5.995 0 0012 12.75a5.995 5.995 0 00-5.058 2.772m0 0a3 3 0 00-4.681 2.72 8.986 8.986 0 003.74.477m.94-3.197a5.971 5.971 0 00-.94 3.197M15 6.75a3 3 0 11-6 0 3 3 0 016 0zm6 3a2.25 2.25 0 11-4.5 0 2.25 2.25 0 014.5 0zm-13.5 0a2.25 2.25 0 11-4.5 0 2.25 2.25 0 014.5 0z"
    />
  </svg>
);

const BrandLogo = () => (
  <svg viewBox="0 0 40 40" fill="none" xmlns="http://www.w3.org/2000/svg">
    <path d="M6 30L14 12L20 22L26 16L34 30" stroke="var(--accent)" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" />
    <path
      d="M6 30L14 18L20 26L26 20L34 30"
      stroke="color-mix(in srgb, var(--accent) 70%, #0f47c4 30%)"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      opacity="0.65"
    />
    <path d="M14 12L18 8" stroke="var(--accent)" strokeWidth="2" strokeLinecap="round" />
  </svg>
);

function tabClass(isActive: boolean): string {
  return isActive ? `${styles.tab} ${styles.tabActive}` : styles.tab;
}

export function AuthShell({ mode, children }: AuthShellProps) {
  return (
    <main className={styles.root}>
      <section className={styles.brandPanel}>
        <div className={styles.gridBackground} />
        <div className={styles.brandContent}>
          <div className={styles.logo}>
            <BrandLogo />
            <div className={styles.logoText}>
              MLRun<span className={styles.logoTextAccent}>X</span>
            </div>
          </div>

          <h1 className={styles.headline}>
            Track, compare, and
            <br />
            ship ML models.
          </h1>
          <p className={styles.subtext}>
            The lightweight experiment tracker built for teams that move fast. Log metrics, compare runs, and
            reproduce results without operational overhead.
          </p>

          <div className={styles.features}>
            <div className={styles.feature}>
              <div className={`${styles.featureIconWrap} text-accent`}>
                <FeatureChartIcon />
              </div>
              <div>
                <div className={styles.featureLabel}>Experiment Tracking</div>
                <div className={styles.featureDesc}>Log params, metrics, and artifacts with minimal code changes.</div>
              </div>
            </div>

            <div className={styles.feature}>
              <div className={`${styles.featureIconWrap} text-accent`}>
                <FeatureCompareIcon />
              </div>
              <div>
                <div className={styles.featureLabel}>Run Comparison</div>
                <div className={styles.featureDesc}>Inspect side-by-side differences across hyperparameters and outcomes.</div>
              </div>
            </div>

            <div className={styles.feature}>
              <div className={`${styles.featureIconWrap} text-accent`}>
                <FeatureTeamIcon />
              </div>
              <div>
                <div className={styles.featureLabel}>Team Workspaces</div>
                <div className={styles.featureDesc}>Collaborate with project-level access controls and shared visibility.</div>
              </div>
            </div>
          </div>
        </div>
      </section>

      <section className={styles.authPanel}>
        <div className={styles.authContainer}>
          <div className={styles.tabs}>
            <Link href="/login" className={tabClass(mode === 'login')}>
              Sign In
            </Link>
            <Link href="/signup" className={tabClass(mode === 'signup')}>
              Create Account
            </Link>
          </div>
          {children}
        </div>
      </section>
    </main>
  );
}
