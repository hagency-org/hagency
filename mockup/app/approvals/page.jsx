'use client';

import Link from 'next/link';
import PageHead from '@/components/PageHead';
import { useT } from '@/components/Prefs';
import { useData } from '@/components/Data';
import { NATIVE_MODE } from '@/lib/native-api';
import NativeApprovals from '@/components/NativeApprovals';

/*
 * 审批观察 — read-only approval observation (ADR-138, PC-C2b).
 *
 * Nothing here decides, consumes or delivers: the two observation routes are
 * GET-only, mounted under the authenticate hoop with no scope, and the page
 * renders the seven-key row's state/choice WORDS — never a card, never a
 * preview, never an owner identity, never a tool name. The delivery stage is
 * the deferred delivery route's own field and does not appear here.
 */
export default function ApprovalsPage() {
  const data = useData();
  const t = useT();
  return data.nativeConsole ? <NativeApprovals /> : <LegacyApprovals />;
}

function LegacyApprovals() {
  const t = useT();
  return (
    <>
      <PageHead title={t('nav.approvals')} />
      <section className="panel">
        <h2>{t('nav.approvals')}</h2>
        <p>{t('ap.nativeOnly')}</p>
      </section>
    </>
  );
}
