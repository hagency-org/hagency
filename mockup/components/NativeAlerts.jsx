'use client';

/*
 * Native alerts: a READ-ONLY triage list over /console/api/alerts.
 *
 * The native store has no operator close path (ADR-124), so this surface
 * renders no transition buttons — controls that would 404 lie. What it does
 * show is everything an operator needs to ACT elsewhere: the four actionable
 * fields (owner/impact/runbook/recovery condition), the raw figures, and the
 * repeat count. `severity`/`status` are server-derived constants for this
 * alert type, which is why there is no status filter — every row here is
 * open by construction, and the sweep is what resolves them.
 */
import { useMemo, useState } from 'react';
import Severity from '@/components/Severity';
import { useT } from '@/components/Prefs';
import { useData } from '@/components/Data';
import { fmtSpanSec } from '@/lib/mock-data';

export default function NativeAlerts() {
  const t = useT();
  const data = useData();
  const { phase, error, refreshing, at_ms: atMs, alerts = [] } = data;
  const [agent, setAgent] = useState('all');
  const [selectedKey, setSelectedKey] = useState(null);

  const agentNames = useMemo(
    () => [...new Set(alerts.map((a) => a.detail && typeof a.detail === 'object' ? a.detail.agent : a.resource_id))].filter(Boolean),
    [alerts],
  );
  const rows = useMemo(
    () => alerts.filter((a) => agent === 'all' || (a.detail && typeof a.detail === 'object' ? a.detail.agent : a.resource_id) === agent),
    [alerts, agent],
  );
  // The selected row is always one of the visible rows.
  const selected = rows.find((r) => r.dedupe_key === selectedKey) ?? rows[0] ?? null;

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

  const detailText = (a) => {
    if (typeof a.detail === 'string') return a.detail; // the truncated-payload arm
    const d = a.detail ?? {};
    return `${d.committedTokens ?? '?'} committed · ${d.measuredTokens ?? '—'} measured · ${d.drawnTokens ?? '?'} drawn · ${d.overByTokens ?? '?'} past`;
  };

  return (
    <div data-native-state={phase} aria-busy={refreshing === true}>
      {refreshing && <p role="status">{t('nu.refreshing')}</p>}

      <h2 style={{ marginTop: 0 }}>{t('al.title')}<span className="note"> {t('al.nativeReadonly')}</span></h2>

      {/* One strip: the open set. There is only one status natively. */}
      <div className="cards">
        <div className="card">
          <div className="cap">{t('al.openNow')}</div>
          <div className={`val${alerts.length > 0 ? ' warn' : ''}`}>{alerts.length}</div>
        </div>
      </div>

      <div className="btn-row" style={{ margin: '22px 0 12px' }}>
        <label style={{ fontSize: 12, color: 'var(--ink-dim)' }}>
          {t('col.agent')}{' '}
          <select value={agent} onChange={(e) => { setAgent(e.target.value); setSelectedKey(null); }}>
            <option value="all">{t('common.all')}</option>
            {agentNames.map((a) => <option key={a} value={a}>{a}</option>)}
          </select>
        </label>
        <span className="spacer" style={{ flex: 1 }} />
        <span className="sub dim" style={{ fontSize: 12 }}>{t('common.shown', { a: rows.length, b: alerts.length })}</span>
      </div>

      {rows.length === 0 ? (
        <div className="empty">
          <div className="big">{t('al.noMatch')}</div>
          <p className="small">{t('al.noneOpen')}</p>
        </div>
      ) : (
        <div className="split">
          <div className="list">
            <table>
              <thead>
                <tr>
                  <th>{t('col.severity')}</th>
                  <th>{t('col.alert')}</th>
                  <th className="num">{t('col.age')}</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((a) => (
                  <tr
                    key={a.dedupe_key}
                    aria-selected={selected?.dedupe_key === a.dedupe_key}
                    onClick={() => setSelectedKey(a.dedupe_key)}
                    style={{ cursor: 'pointer' }}
                  >
                    <td><Severity level={a.severity} /></td>
                    <td>
                      <div>{a.summary}</div>
                      <div className="faint" style={{ fontSize: 11 }}>
                        {a.detail && typeof a.detail === 'object' ? a.detail.agent : a.resource_id} · ×{a.occurrences}
                      </div>
                    </td>
                    <td className="num dim">
                      {fmtSpanSec(atMs && a.first_seen_ms ? Math.max(0, (atMs - a.first_seen_ms) / 1000) : 0)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {selected && (
            <div className="panel">
              <h3>{selected.summary}</h3>
              <dl className="kv">
                <dt>{t('al.firstSeen')}</dt>
                <dd>{t('common.ago', { n: fmtSpanSec(atMs && selected.first_seen_ms ? Math.max(0, (atMs - selected.first_seen_ms) / 1000) : 0) })}</dd>
                <dt>{t('al.lastSeen')}</dt>
                <dd>{t('common.ago', { n: fmtSpanSec(atMs && selected.last_seen_ms ? Math.max(0, (atMs - selected.last_seen_ms) / 1000) : 0) })}</dd>
                <dt>{t('al.occurrences')}</dt><dd>{selected.occurrences}</dd>
                <dt>{t('col.agent')}</dt>
                <dd>{selected.detail && typeof selected.detail === 'object' ? selected.detail.agent : selected.resource_id}</dd>
              </dl>

              <p style={{ fontSize: 12.5, color: 'var(--ink-2)', marginTop: 12 }}>{detailText(selected)}</p>

              <dl className="kv" style={{ marginTop: 12 }}>
                <dt>{t('al.impact')}</dt><dd>{selected.impact}</dd>
                <dt>{t('al.runbook')}</dt><dd>{selected.runbook}</dd>
                <dt>{t('al.recovery')}</dt><dd>{selected.recovery_condition}</dd>
              </dl>

              {/* The buttons come ONLY from the served `next` array (the
                * one server-owned map) — a transition the server refuses is
                * never offered as a control, and a terminal row offers
                * none. Display state only: nothing here enforces. */}
              <div className="btn-row" style={{ marginTop: 14 }}>
                {selected.next.length === 0 && (
                  <span className="dim" style={{ fontSize: 12 }}>{t('al.noTransitions', { s: selected.status })}</span>
                )}
                {selected.next.map((to) => (
                  <button
                    key={to}
                    className="btn"
                    data-transition={to}
                    disabled={data.action?.kind === 'pending'}
                    onClick={() => data.transition(selected.dedupe_key, to)}
                  >
                    {t(to === 'open' ? 'act.reopen' : `act.${to === 'acknowledged' ? 'acknowledge' : to === 'resolved' ? 'resolve' : 'suppress'}`)}
                  </button>
                ))}
              </div>

              <div className="btn-row" style={{ marginTop: 14 }}>
                <button className="btn" onClick={data.refresh}>{t('nu.refresh')}</button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
