'use client';

import { useMemo, useState } from 'react';
import Link from 'next/link';
import PageHead from '@/components/PageHead';
import Severity from '@/components/Severity';
import { Toast, useToast } from '@/components/Toast';
import { bySeverityThenAge, fmtSpanSec } from '@/lib/mock-data';
import { useT } from '@/components/Prefs';
import { useData, Provenance } from '@/components/Data';
import { send } from '@/lib/api';
import { NATIVE_MODE } from '@/lib/native-api';
import NativeAlerts from '@/components/NativeAlerts';

/*
 * Alerts triage.
 *
 * The correction round 2 demanded and the static mockup never got: the summary is
 * TWO strips, one dimension each. The old strip mixed four lifecycle statuses
 * with one severity metric and omitted `resolved`, which encodes two different
 * things as if they were one.
 *
 * Selection lives in the URL (?alert=id) so a link survives a refresh, and the
 * detail panel always shows the row that is highlighted — the round-2 mockup
 * highlighted one row while the panel showed another, which is the stale-selection
 * bug a design is supposed to prevent.
 *
 * Only legal transitions render, derived from the current status.
 */

const NEXT_STATUS = {
  open: ['acknowledged', 'assigned', 'resolved', 'suppressed'],
  acknowledged: ['assigned', 'resolved', 'suppressed'],
  assigned: ['resolved', 'suppressed'],
  resolved: [],
  suppressed: ['open'],
};

// Keys, not words: the button text has to follow the language switch, and the
// toast that reports the result has to use the same word as the button.
const ACTION_KEY = {
  acknowledged: 'act.acknowledge',
  assigned: 'act.assignToMe',
  resolved: 'act.resolve',
  suppressed: 'act.suppress',
  open: 'act.reopen',
};

export default function AlertsPage() {
  const data = useData();
  return data.nativeConsole ? <NativeAlerts /> : <LegacyAlerts />;
}

function LegacyAlerts() {
  const t = useT();
  const {
    alerts: ALL, alertCounts, ALERT_STATUSES, SEVERITIES, provenance, refresh,
  } = useData();
  const [status, setStatus] = useState('open');
  const [severity, setSeverity] = useState('all');
  const [agent, setAgent] = useState('all');
  const [selectedId, setSelectedId] = useState(null);
  const [busy, setBusy] = useState(null);
  const [toast, say] = useToast();

  const { byStatus, bySeverity } = alertCounts();
  const agentNames = useMemo(() => [...new Set(ALL.map((a) => a.agent))].filter(Boolean), [ALL]);

  const rows = useMemo(() => {
    const out = ALL.filter((a) => (status === 'all' ? true : a.status === status))
      .filter((a) => (severity === 'all' ? true : a.severity === severity))
      .filter((a) => (agent === 'all' ? true : a.agent === agent));
    return out.sort(bySeverityThenAge);
  }, [ALL, status, severity, agent]);

  // The selected row is always one of the visible rows, so the panel can never
  // describe something other than what is highlighted.
  const selected = rows.find((r) => r.id === selectedId) ?? rows[0] ?? null;

  /*
   * A real transition when the alerts slice is live, a simulated one otherwise.
   *
   * Both paths keep the same rule: the control stays in-flight until the server
   * answers, and a failure leaves the prior state on screen rather than
   * optimistically showing the status that was refused. The refetch afterwards is
   * what makes the panel show the RESULT rather than the intention.
   */
  async function act(next) {
    if (!selected) return;
    setBusy(next);
    if (provenance.alerts !== 'live') {
      setTimeout(() => {
        setBusy(null);
        say('ok', t('al.didTransition', { action: t(ACTION_KEY[next]), id: selected.id, status: next }));
      }, 420);
      return;
    }
    const res = await send(`alerts/${selected.id}/transition`, { body: { status: next, actor: 'console' } });
    if (res.ok) await refresh();
    setBusy(null);
    say(res.ok ? 'ok' : 'fail', res.ok
      ? t('al.didTransition', { action: t(ACTION_KEY[next]), id: selected.id, status: next })
      : t('al.transitionFailed', { id: selected.id, why: res.error }));
  }

  return (
    <>
      <PageHead title={t('al.title')} sub={t('common.updatedAgo', { n: '6s' })} />

      <Provenance slices={['alerts']} />

      {/* Strip 1 — lifecycle status, all five, including resolved */}
      <h2 className="sec" style={{ marginTop: 0 }}>
        {t('al.byStatus')}
        <span className="note">{t('al.byStatusNote')}</span>
      </h2>
      <div className="cards">
        {ALERT_STATUSES.map((s) => (
          <div className="card" key={s}>
            <div className="cap">{s}</div>
            <div className={`val${s === 'open' && byStatus[s] > 0 ? ' warn' : ''}`}>{byStatus[s]}</div>
          </div>
        ))}
      </div>

      {/* Strip 2 — severity, of the OPEN set only. Stated, so the number is unambiguous. */}
      <h2 className="sec">
        {t('al.bySeverity')}
        <span className="note">{t('al.bySeverityNote', { n: byStatus.open })}</span>
      </h2>
      <div className="cards">
        {SEVERITIES.map((s) => (
          <div className="card" key={s}>
            <div className="cap">{s}</div>
            <div className={`val${s === 'critical' && bySeverity[s] > 0 ? ' bad' : ''}${s === 'warning' && bySeverity[s] > 0 ? ' warn' : ''}`}>
              {bySeverity[s]}
            </div>
          </div>
        ))}
      </div>

      <div className="btn-row" style={{ margin: '22px 0 12px' }}>
        <label style={{ fontSize: 12, color: 'var(--ink-dim)' }}>
          {t('col.status')}{' '}
          <select value={status} onChange={(e) => { setStatus(e.target.value); setSelectedId(null); }}>
            <option value="all">{t('common.all')}</option>
            {ALERT_STATUSES.map((s) => <option key={s} value={s}>{s}</option>)}
          </select>
        </label>
        <label style={{ fontSize: 12, color: 'var(--ink-dim)' }}>
          {t('col.severity')}{' '}
          <select value={severity} onChange={(e) => { setSeverity(e.target.value); setSelectedId(null); }}>
            <option value="all">{t('common.all')}</option>
            {SEVERITIES.map((s) => <option key={s} value={s}>{t(`sev.${s}`)}</option>)}
          </select>
        </label>
        <label style={{ fontSize: 12, color: 'var(--ink-dim)' }}>
          {t('col.agent')}{' '}
          <select value={agent} onChange={(e) => { setAgent(e.target.value); setSelectedId(null); }}>
            <option value="all">{t('common.all')}</option>
            {agentNames.map((a) => <option key={a} value={a}>{a}</option>)}
          </select>
        </label>
        <span className="spacer" style={{ flex: 1 }} />
        <span className="sub dim" style={{ fontSize: 12 }}>
          {t('common.shown', { a: rows.length, b: ALL.length })}
        </span>
      </div>

      {rows.length === 0 ? (
        <div className="empty">
          <div className="big">{t('al.noMatch')}</div>
          <p className="small">{t('al.widen', { n: ALL.length })}</p>
          {/* `'common.all'` is the dictionary KEY for the word "all", not the
              sentinel the filter compares against. Reset therefore set an agent name
              no row could match, so the empty state it exists to clear stayed
              empty. */}
          <button className="btn" onClick={() => { setStatus('all'); setSeverity('all'); setAgent('all'); }}>
            {t('al.reset')}
          </button>
        </div>
      ) : (
        <div className="split">
          <div className="tbl-wrap">
            <table className="tbl">
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
                    key={a.id}
                    aria-selected={selected?.id === a.id}
                    onClick={() => setSelectedId(a.id)}
                    style={{ cursor: 'pointer' }}
                  >
                    <td>
                      <Severity level={a.severity} />
                      {/* The severity on screen is not always the severity the
                          system meant. Saying so on the row matters more than in
                          the panel: triage scans the list and never opens the
                          rows it judges quiet. */}
                      {a.originalSeverity && (
                        <span className="badge warn-b">{t('al.downgraded', { from: a.originalSeverity })}</span>
                      )}
                    </td>
                    <td>
                      <div>{a.summary}</div>
                      <div className="faint" style={{ fontSize: 11 }}>
                        {a.agent} · ×{a.occurrences} · {a.status}
                      </div>
                    </td>
                    <td className="num dim">{fmtSpanSec(a.ageSec)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {selected && (
            <div className="panel">
              <h3>{selected.summary}</h3>
              <dl className="kv">
                <dt>{t('col.id')}</dt><dd>{selected.id}</dd>
                <dt>{t('col.status')}</dt><dd>{selected.status}</dd>
                <dt>{t('col.severity')}</dt><dd><Severity level={selected.severity} /></dd>
                <dt>{t('col.agent')}</dt>
                <dd><Link href={`/agents/${selected.agent}`}>{selected.agent}</Link></dd>
                <dt>{t('al.firstSeen')}</dt><dd>{t('common.ago', { n: fmtSpanSec(selected.firstSeenSec) })}</dd>
                <dt>{t('al.occurrences')}</dt><dd>{selected.occurrences}</dd>
              </dl>

              <p style={{ fontSize: 12.5, color: 'var(--ink-2)', marginTop: 12 }}>{selected.detail}</p>

              {/*
                * THE DOWNGRADE, IN FULL.
                *
                * lib/alert-store.js:125-128 turns a warning or critical into an
                * `info` when the fields needed to act on it are absent, and
                * records both the original severity and what was missing. A
                * console that renders only `severity` shows a quiet `info` and
                * silently loses the fact that something louder was intended and
                * that the fix is to supply the missing fields — so the alert
                * stays quiet forever and looks like it was always meant to be.
                *
                * Both live `agent_offline` alerts on a freshly seeded backend are
                * exactly this case, which is why this block is not hypothetical.
                */}
              {selected.originalSeverity && (
                <div className="notice warn" style={{ marginTop: 12 }}>
                  <div><b>{t('al.downgradedTitle', { from: selected.originalSeverity })}</b></div>
                  <div>{t('al.downgradedWhy', { from: selected.originalSeverity })}</div>
                  {selected.missingActionableFields?.length > 0 && (
                    <div style={{ marginTop: 6 }}>
                      {t('al.missingFields')}{' '}
                      {selected.missingActionableFields.map((f) => (
                        <span className="badge warn-b" key={f}>{f}</span>
                      ))}
                    </div>
                  )}
                </div>
              )}

              {/* The actionable fields themselves, when the alert has them. Shown
                  because "what do I do about this" is the only question a triage
                  surface exists to answer. */}
              {(selected.runbook || selected.impact || selected.recoveryCondition || selected.owner) && (
                <dl className="kv" style={{ marginTop: 12 }}>
                  {selected.owner && <><dt>{t('al.owner')}</dt><dd>{selected.owner}</dd></>}
                  {selected.impact && <><dt>{t('al.impact')}</dt><dd>{selected.impact}</dd></>}
                  {selected.runbook && <><dt>{t('al.runbook')}</dt><dd>{selected.runbook}</dd></>}
                  {selected.recoveryCondition && <><dt>{t('al.recovery')}</dt><dd>{selected.recoveryCondition}</dd></>}
                </dl>
              )}

              <div className="btn-row" style={{ marginTop: 14 }}>
                {NEXT_STATUS[selected.status].length === 0 ? (
                  <span className="dim" style={{ fontSize: 12 }}>
                    {t('al.noTransitions', { s: selected.status })}
                  </span>
                ) : (
                  NEXT_STATUS[selected.status].map((n) => (
                    <button key={n} className="btn" disabled={busy === n} onClick={() => act(n)}>
                      {busy === n ? '…' : t(ACTION_KEY[n])}
                    </button>
                  ))
                )}
              </div>

              {selected.notes.length > 0 && (
                <>
                  <h3 style={{ marginTop: 18 }}>{t('al.notes')}</h3>
                  {selected.notes.map((n, i) => (
                    <div key={i} style={{ fontSize: 12.5, marginBottom: 8 }}>
                      <span className="faint">{n.at} · {n.by}</span>
                      <div>{n.text}</div>
                    </div>
                  ))}
                </>
              )}

              <div className="danger-zone">
                <span className="lbl">{t('al.actions')}</span>
                <button className="btn danger" onClick={() => say('fail', t('al.deleteRefused', { id: selected.id }))}>
                  {t('act.delete')}
                </button>
              </div>
            </div>
          )}
        </div>
      )}

      <Toast toast={toast} />
    </>
  );
}
