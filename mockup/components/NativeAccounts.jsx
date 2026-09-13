'use client';
import { useT } from '@/components/Prefs';

/* The console account surface without readiness (MA-S3a). Every row carries
 * exactly the five public keys; the identity triple (namespace identity,
 * identity tuple, seat) never crosses the wire, and no cell here renders a
 * readiness verdict — `uncertain` stays `uncertain`, never a success or a
 * failure. State words are wire values, never translated. */
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
            <th>{t('na.col.profile')}</th>
            <th>{t('na.col.revision')}</th>
          </tr>
        </thead>
        <tbody>
          {accounts.map((a) => (
            <tr key={a.id} data-account-row={a.id}>
              <td data-account="ordinal">{a.ordinal}</td>
              <td data-account="state">{a.state}</td>
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
