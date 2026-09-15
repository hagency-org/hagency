'use client';

/*
 * Native agent roster (ADR-126): a READ-ONLY observation of the engagement
 * projections, one row per engagement. The seven columns render exactly
 * what /console/api/agents serves; the columns the SERVER names in
 * `unavailable` render as unknown — never zero, never invented — and the
 * list is server-owned, so a future source turns a column on by removing
 * its name server-side, not by editing this page. There is deliberately no
 * work item, progress, queue, task count or utilisation column: the
 * retained roster withdrew them on principle and native does not widen
 * what it narrowed. No lifecycle controls either — start/stop/preset are a
 * later slice behind their own finite scope, and a button that would
 * refuse lies.
 */
import { useT } from '@/components/Prefs';
import { useData } from '@/components/Data';
import { fmtTokens } from '@/lib/mock-data';
import { startAgent, stopAgent, applyPreset } from '@/lib/native-api';

export default function NativeAgents() {
  const t = useT();
  const data = useData();
  const { phase, error, refreshing, agents = [], unavailable = [], permissions = {} } = data;
  const manageLifecycle = permissions.manageLifecycle === true;

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

      <h2 style={{ marginTop: 0 }}>{t('nav.workforce')}<span className="note"> {t('na.readonly')}</span></h2>

      {/* The server's own gap list, rendered verbatim: the page never
          decides which columns are unknown. */}
      <p className="sub dim" style={{ fontSize: 12 }}>
        {t('na.unavailable', { list: unavailable.join(', ') })}
      </p>

      {agents.length === 0 ? (
        <div className="empty">
          <div className="big">{t('na.none')}</div>
        </div>
      ) : (
        <div className="list">
          <table>
            <thead>
              <tr>
                <th>{t('col.agent')}</th>
                <th>{t('col.framework')}</th>
                <th>{t('col.role')}</th>
                <th>{t('col.state')}</th>
                <th>{t('na.engagement')}</th>
                <th className="num">{t('col.requested')}</th>
                <th>{t('na.lastActivity')}</th>
                {manageLifecycle && <th>{t('na.controls')}</th>}
              </tr>
            </thead>
            <tbody>
              {agents.map((a) => (
                <tr key={a.engagement_id} data-engagement-id={a.engagement_id}>
                  <td>{a.name}</td>
                  <td className="dim">{a.framework}</td>
                  <td>{a.role}</td>
                  <td>{a.state}</td>
                  <td className="dim">{a.engagement_id}</td>
                  <td className="num dim">{fmtTokens(a.requested_tokens)}</td>
                  {/* Last dispatch activity, not last seen; null is unknown,
                      rendered as the word — never a zero clock. */}
                  <td className="dim">{a.last_activity_ms === null ? t('nu.unknown') : new Date(a.last_activity_ms).toISOString()}</td>
                  {manageLifecycle && (
                    <td>
                      <button className="btn" data-lifecycle-action="start" onClick={() => startAgent(a.engagement_id)}>{t('na.start')}</button>
                      <button className="btn" data-lifecycle-action="stop" onClick={() => stopAgent(a.engagement_id)}>{t('na.stop')}</button>
                      <button className="btn" data-lifecycle-action="preset" onClick={() => applyPreset(a.engagement_id, window.prompt(t('na.presetId')) ?? '')}>{t('na.preset')}</button>
                    </td>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <div className="btn-row" style={{ marginTop: 14 }}>
        <button className="btn" onClick={data.refresh}>{t('nu.refresh')}</button>
      </div>
    </div>
  );
}
