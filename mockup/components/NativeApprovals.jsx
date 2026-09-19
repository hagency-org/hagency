'use client';

/*
 * Native approvals: a READ-ONLY observation list over /console/api/approvals
 * (ADR-138, PC-C2b). Rows carry exactly seven keys — id, state, choice,
 * reusableScope, expiresAt, engagementId, projectRoomId — and no nested
 * object, so nothing here renders a card, a preview, an owner identity or a
 * tool name. An undelivered approval shows its `state` and `choice` WORDS;
 * the delivery stage is the deferred delivery route's own field and is never
 * fabricated here.
 *
 * No create, verdict, consume or delivery control: those mutate enforcement
 * and need their own reviewed decision; buttons that would 404 lie.
 */
import { useEffect, useState } from 'react';
import { useT } from '@/components/Prefs';
import { fetchApprovals } from '@/lib/native-api';

export default function NativeApprovals() {
  const t = useT();
  const [phase, setPhase] = useState('loading');
  const [error, setError] = useState(null);
  const [rows, setRows] = useState([]);
  const [nextAfter, setNextAfter] = useState(null);
  const [refreshing, setRefreshing] = useState(false);
  const [stateFilter, setStateFilter] = useState('all');

  const load = async (after = '') => {
    try {
      const value = await fetchApprovals(after);
      setRows(after ? (prev) => [...prev, ...value.approvals] : value.approvals);
      setNextAfter(value.next_after);
      setError(null);
      setPhase('ready');
    } catch (err) {
      setError(err.message);
      setPhase('error');
    }
  };

  useEffect(() => { load(); /* eslint-disable-line react-hooks/exhaustive-deps */ }, []);

  const refresh = async () => {
    setRefreshing(true);
    try { await load(); } finally { setRefreshing(false); }
  };

  if (phase === 'error') {
    return (
      <section className="panel" role="alert">
        <h2>{t('ap.failed')}</h2>
        <p>{t('ap.retryHelp')}</p>
        <button className="btn" onClick={refresh}>{t('common.refresh')}</button>
      </section>
    );
  }

  const visible = rows.filter((r) => stateFilter === 'all' || r.state === stateFilter);
  const counts = Object.fromEntries(
    ['pending', 'decided', 'applying', 'uncertain', 'applied', 'invalidated', 'not_applied']
      .map((s) => [s, rows.filter((r) => r.state === s).length]),
  );

  return (
    <div data-native-state={phase} aria-busy={refreshing === true}>
      {refreshing && <p role="status">{t('ap.refreshing')}</p>}

      <h2 style={{ marginTop: 0 }}>{t('nav.approvals')}<span className="note"> {t('ap.readonly')}</span></h2>

      <div className="cards">
        {Object.entries(counts).map(([s, n]) => (
          <div className="card" key={s}>
            <div className="cap">{s}</div>
            <div className={`val${s === 'pending' && n > 0 ? ' warn' : ''}`}>{n}</div>
          </div>
        ))}
      </div>

      <div className="btn-row" style={{ margin: '22px 0 12px' }}>
        <label style={{ fontSize: 12, color: 'var(--ink-dim)' }}>
          {t('col.state')}{' '}
          <select value={stateFilter} onChange={(e) => setStateFilter(e.target.value)}>
            <option value="all">{t('common.all')}</option>
            {Object.keys(counts).map((s) => <option key={s} value={s}>{s}</option>)}
          </select>
        </label>
        <span className="spacer" style={{ flex: 1 }} />
        <span className="sub dim" style={{ fontSize: 12 }}>{t('common.shown', { a: visible.length, b: rows.length })}</span>
      </div>

      {visible.length === 0 ? (
        <div className="empty">
          <div className="big">{t('ap.none')}</div>
          <p className="small">{t('ap.noneNote')}</p>
        </div>
      ) : (
        <table>
          <thead>
            <tr>
              <th>{t('col.id')}</th>
              <th>{t('col.state')}</th>
              <th>{t('ap.choice')}</th>
              <th>{t('ap.reusable')}</th>
              <th className="num">{t('ap.expires')}</th>
            </tr>
          </thead>
          <tbody>
            {visible.map((r) => (
              <tr key={r.id}>
                <td className="faint" style={{ fontSize: 11 }}>{r.id}</td>
                <td>{r.state}</td>
                <td>{r.choice ?? '—'}</td>
                <td>{r.reusableScope ? t('ap.reusableYes') : t('ap.reusableNo')}</td>
                <td className="num dim">{new Date(r.expiresAt).toLocaleString()}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {nextAfter && (
        <div className="btn-row" style={{ marginTop: 16 }}>
          <button className="btn" onClick={() => load(nextAfter)}>{t('ng.nextPage')}</button>
        </div>
      )}
    </div>
  );
}
