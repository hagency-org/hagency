'use client';

/*
 * Native engagements: a READ-ONLY triage list over the same
 * /console/api/engagements read the usage page selects from — no new data
 * path, the page just renders the list slice `Data.jsx` already carries
 * (`fetchNative` returns `engagements` + `next_after`; pagination rides
 * `nextPage`/`firstPage`). No create, verdict, revoke or whitelist action:
 * those mutate enforcement and need their own reviewed decision; buttons
 * that would 404 lie (the same rule as the alerts page).
 *
 * The retained page's route reasons (notWhitelisted / overOffer /
 * overCeiling) have no native counterpart — the whitelist is the retained
 * fleet model's admission surface. The native analogue of "awaiting my
 * decision" is the engagement STATE column (pending → reserved/active …),
 * which is exactly what renders here.
 */
import { useMemo, useState } from 'react';
import PageHead from '@/components/PageHead';
import NativeStatusStrip from '@/components/NativeStatusStrip';
import { useT } from '@/components/Prefs';
import { useData } from '@/components/Data';
import { fmtTokens } from '@/lib/mock-data';

const NATIVE_STATES = ['pending', 'reserved', 'active', 'rejected', 'revoked', 'failed'];

export default function NativeEngagements() {
  const t = useT();
  const data = useData();
  const { phase, error, refreshing, engagements = [], next_after: nextAfter } = data;
  const [state, setState] = useState('all');
  const [agent, setAgent] = useState('all');

  const agentNames = useMemo(
    () => [...new Set(engagements.map((e) => e.agentName))].filter(Boolean),
    [engagements],
  );
  const counts = useMemo(() => {
    const out = Object.fromEntries(NATIVE_STATES.map((s) => [s, 0]));
    for (const e of engagements) out[e.state] = (out[e.state] ?? 0) + 1;
    return out;
  }, [engagements]);
  const rows = useMemo(
    () => engagements
      .filter((e) => state === 'all' || e.state === state)
      .filter((e) => agent === 'all' || e.agentName === agent),
    [engagements, state, agent],
  );

  if (phase === 'error') {
    return (
      <section className="panel" role="alert">
        <h2>{t('nu.failed')}</h2>
        <p>{t(error === 'not_found' ? 'nu.notFound' : 'nu.retryHelp')}</p>
        <button className="btn" onClick={data.refresh}>{t('nu.refresh')}</button>
      </section>
    );
  }
  if (phase === 'access') return null;

  return (
    <div data-native-state={phase} aria-busy={refreshing === true}>
      {refreshing && <p role="status">{t('nu.refreshing')}</p>}

      <PageHead title={t('nav.engagements')}><NativeStatusStrip /></PageHead>
      <h2 style={{ marginTop: 0 }}>{t('nav.engagements')}<span className="note"> {t('ng.readonly')}</span></h2>

      {/* One strip: the state split of this page. */}
      <div className="cards">
        {NATIVE_STATES.map((s) => (
          <div className="card" key={s}>
            <div className="cap">{s}</div>
            <div className={`val${s === 'pending' && counts[s] > 0 ? ' warn' : ''}`}>{counts[s]}</div>
          </div>
        ))}
      </div>

      <div className="btn-row" style={{ margin: '22px 0 12px' }}>
        <label style={{ fontSize: 12, color: 'var(--ink-dim)' }}>
          {t('col.state')}{' '}
          <select value={state} onChange={(e) => setState(e.target.value)}>
            <option value="all">{t('common.all')}</option>
            {NATIVE_STATES.map((s) => <option key={s} value={s}>{s}</option>)}
          </select>
        </label>
        <label style={{ fontSize: 12, color: 'var(--ink-dim)' }}>
          {t('col.agent')}{' '}
          <select value={agent} onChange={(e) => setAgent(e.target.value)}>
            <option value="all">{t('common.all')}</option>
            {agentNames.map((a) => <option key={a} value={a}>{a}</option>)}
          </select>
        </label>
        <span className="spacer" style={{ flex: 1 }} />
        <span className="sub dim" style={{ fontSize: 12 }}>
          {t('common.shown', { a: rows.length, b: engagements.length })}
        </span>
      </div>

      {rows.length === 0 ? (
        <div className="empty">
          <div className="big">{t('ng.none')}</div>
        </div>
      ) : (
        <div className="list">
          <table>
            <thead>
              <tr>
                <th>{t('col.state')}</th>
                <th>{t('col.agent')}</th>
                <th>{t('col.project')}</th>
                <th>{t('col.role')}</th>
                <th className="num">{t('col.requested')}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((e) => (
                <tr key={e.id}>
                  <td>{e.state}</td>
                  <td>{e.agentName}</td>
                  <td>{e.projectName ?? '—'}</td>
                  <td>{e.role}</td>
                  <td className="num dim">{fmtTokens(e.requestedTokens)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <div className="btn-row" style={{ marginTop: 14 }}>
        <button className="btn" onClick={data.refresh}>{t('nu.refresh')}</button>
        <button className="btn" disabled={nextAfter === null} onClick={data.nextPage}>{t('ng.nextPage')}</button>
        <button className="btn" onClick={data.firstPage}>{t('nu.firstPage')}</button>
      </div>
    </div>
  );
}
