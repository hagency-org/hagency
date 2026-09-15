'use client';
import { useT } from '@/components/Prefs';

/* The console account surface (MA-S3b). Every row carries exactly the six
 * public keys; the identity triple (namespace identity, identity tuple,
 * seat) never crosses the wire. The readiness word is the observed fact —
 * `subscription`/`api_key`/`unknown`, the operator's own login, never a
 * console check — and no credential byte or probe output ever crosses.
 * State words are wire values, never translated. */
export default function NativeAccounts({ phase, accounts }) {
  const t = useT();
  if (phase === 'access') {
    return (
      <section className="panel" data-native-state="access">
        <h2>{t('na.access')}</h2>
        <p>{t('na.accessHelp')}</p>
      </section>
    );
  }
  if (phase === 'loading') {
    return (
      <section className="panel" data-native-state="loading" aria-busy="true">
        <h2>{t('na.loading')}</h2>
      </section>
    );
  }
  if (!accounts?.length) {
    return (
      <section className="panel" data-native-state="ready" aria-busy="false">
        <h2>{t('na.title')}</h2>
        <p>{t('na.empty')}</p>
      </section>
    );
  }
  return (
    <section className="panel" data-native-state="ready" aria-busy="false">
      <h2>{t('na.title')}</h2>
      <p>{t('na.sub')}</p>
      <table>
        <thead>
          <tr>
            <th>{t('na.col.ordinal')}</th>
            <th>{t('na.col.state')}</th>
            <th>{t('na.col.readiness')}</th>
            <th>{t('na.col.profile')}</th>
            <th>{t('na.col.revision')}</th>
          </tr>
        </thead>
        <tbody>
          {accounts.map((a) => (
            <tr key={a.id} data-account-row={a.id}>
              <td data-account="ordinal">{a.ordinal}</td>
              <td data-account="state">{a.state}</td>
              <td data-account="readiness">{t(`na.readiness.${a.readiness}`)}</td>
              <td data-account="profile">{a.profile}</td>
              <td data-account="revision" title={a.revision}>{a.revision.slice(0, 12)}…</td>
            </tr>
          ))}
        </tbody>
      </table>
      <p>{t('na.opacity')}</p>
    </section>
  );
}
