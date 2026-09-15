'use client';

import { useEffect } from 'react';
import { useRouter } from 'next/navigation';
import Link from 'next/link';
import { useT } from '@/components/Prefs';
import { useData } from '@/components/Data';
import NativeAgents from '@/components/NativeAgents';

/*
 * The native roster route (ADR-126). The retained console's roster lives at
 * /workforce — /agents/<name> is the single-agent detail page — so the
 * retained arm of this route sends the reader there rather than rendering a
 * second, divergent roster.
 */
export default function AgentsPage() {
  const data = useData();
  return data.nativeConsole ? <NativeAgents /> : <RetainedRoster />;
}

function RetainedRoster() {
  const t = useT();
  const router = useRouter();
  useEffect(() => { router.replace('/workforce'); }, [router]);
  return (
    <section className="panel" role="status">
      <h2>{t('nav.workforce')}</h2>
      <p>{t('na.retiredRoute')}</p>
      <p><Link className="btn" href="/workforce">{t('nav.workforce')}</Link></p>
    </section>
  );
}
