'use client';
import { useEffect, useRef, useState } from 'react';
import { useT } from '@/components/Prefs';
import { fetchStopped, inspectStopped, resolutionBody, resolveStopped } from '@/lib/native-recovery';

export default function NativeStoppedWork({ agent, onHold }) {
  const t = useT();
  const [page, setPage] = useState(null), [inspection, setInspection] = useState(null);
  const [note, setNote] = useState(''), [instruction, setInstruction] = useState('');
  const [busy, setBusy] = useState(false), [error, setError] = useState(null), [result, setResult] = useState(null);
  const [pending, setPending] = useState(null), [now, setNow] = useState(Date.now());
  const inFlight = useRef(false);
  useEffect(() => {
    let current = true;
    fetchStopped(agent.engagement_id).then((v) => { if (current) setPage(v); }).catch((e) => { if (current) setError(e.message); });
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => { current = false; clearInterval(timer); };
  }, [agent.engagement_id]);

  async function perform(work) {
    if (inFlight.current) return;
    inFlight.current = true; setBusy(true); setError(null);
    try { await work(); } catch (e) { setError(e.message); }
    finally { inFlight.current = false; setBusy(false); }
  }
  function read(after = '') {
    perform(async () => { setPage(await fetchStopped(agent.engagement_id, after)); setInspection(null); setResult(null); });
  }
  function inspect(original) {
    perform(async () => {
      setInspection(null); setResult(null); setNote(''); setInstruction('');
      setInspection(await inspectStopped(agent.engagement_id, original));
    });
  }
  async function submit(body) {
    try {
      const receipt = await resolveStopped(agent.engagement_id, body, inspection.snapshot.task);
      setResult(receipt); setInspection(null); setPending(null); onHold(false);
    } catch (e) {
      // Retain byte-identical replay only for uncertain outcomes. Nothing in a
      // refresh/effect resubmits a decision, including after credential expiry.
      if (e.message !== 'outcome_unknown') { setPending(null); setInspection(null); onHold(false); }
      throw e;
    }
  }
  function decide(action) {
    perform(async () => {
      const body = resolutionBody(inspection, action, note, instruction, `resolution_${crypto.randomUUID()}`, `continuation_${crypto.randomUUID()}`);
      setPending(body); onHold(true);
      await submit(body);
    });
  }
  const expired = inspection !== null && inspection.expiresAt <= now;
  const disabled = busy || pending !== null;
  const s = inspection?.snapshot;
  const errorKey = error === 'outcome_unknown' ? 'unknown'
    : ['resolution_conflict', 'dispatch_not_resolvable'].includes(error) ? 'refused'
      : ['console_access_required', 'agent_lifecycle_scope_required'].includes(error) ? 'access'
        : error === 'invalid_selection' ? 'invalid' : 'failed';

  return (
    <section className="panel" data-recovery-panel aria-busy={busy} style={{ marginTop: 20, overflowWrap: 'anywhere' }}>
      <h2>{t('nrec.title', { name: agent.name })}</h2>
      <p className="sub">{t('nrec.help')}</p>
      {error && <p role="alert" data-recovery-error>{t(`nrec.${errorKey}`)}</p>}
      {pending && !busy && <button className="btn" data-recovery-retry onClick={() => perform(() => submit(pending))}>{t('nrec.retryExact')}</button>}
      {result && <p role="status" data-recovery-result>{t('nrec.recorded', { action: t(`nrec.${result.action}`), state: result.task.status })}</p>}
      {!page && !error && <p role="status">{t('nrec.loading')}</p>}
      {page && <>
        {page.dispatches.length === 0 ? <p>{t('nrec.empty')}</p> : <ul>
          {page.dispatches.map((row) => <li key={row.dispatchId} style={{ marginBottom: 12 }}>
            <code>{row.dispatchId}</code> · {row.reason}{' '}
            <button className="btn" data-recovery-inspect={row.dispatchId} disabled={disabled || !row.inspectionAvailable} onClick={() => inspect(row.dispatchId)}>{t('nrec.inspect')}</button>
            {!row.inspectionAvailable && <p className="note">{t('nrec.missing')}</p>}
          </li>)}
        </ul>}
        <div className="btn-row">
          <button className="btn" disabled={disabled} onClick={() => read()}>{t('nu.refresh')}</button>
          {page.nextAfter && <button className="btn" disabled={disabled} onClick={() => read(page.nextAfter)}>{t('nrec.next')}</button>}
        </div>
      </>}
      {s && <div data-recovery-inspection style={{ marginTop: 16 }}>
        <h3>{s.task.title}</h3>
        <p>{t('nrec.context', { task: s.task.id, state: s.task.status, workspace: s.observation.workspace })}</p>
        <p>{t('nrec.receipt', { digest: s.receiptDigest })}</p>
        <p role={expired ? 'alert' : undefined}>{expired ? t('nrec.expired') : t('nrec.expires', { time: new Date(inspection.expiresAt).toISOString() })}</p>
        <details open>
          <summary>{t('nrec.inventory', { count: s.observation.inventory.entries.length })}</summary>
          <div style={{ overflowX: 'auto' }}><table className="tbl">
            <thead><tr><th>{t('nrec.path')}</th><th>{t('nrec.kind')}</th><th>{t('nrec.bytes')}</th><th>{t('nrec.hash')}</th></tr></thead>
            <tbody>{s.observation.inventory.entries.map((entry) => <tr key={entry.path}>
              <td>{entry.path}</td><td>{entry.kind}</td><td>{entry.bytes ?? '—'}</td><td><code>{entry.sha256 ?? entry.target ?? '—'}</code></td>
            </tr>)}</tbody>
          </table></div>
        </details>
        <label style={{ display: 'block', marginTop: 16 }}>{t('nrec.note')}
          <textarea data-recovery-note value={note} maxLength={2000} disabled={disabled || expired} onChange={(e) => setNote(e.target.value)} style={{ display: 'block', width: '100%', minHeight: 70 }} />
        </label>
        <label style={{ display: 'block', marginTop: 12 }}>{t('nrec.instruction')}
          <textarea data-recovery-instruction value={instruction} maxLength={8192} disabled={disabled || expired} onChange={(e) => setInstruction(e.target.value)} style={{ display: 'block', width: '100%', minHeight: 70 }} />
        </label>
        <p className="note">{t('nrec.actionsHelp')}</p>
        <div className="btn-row" style={{ flexWrap: 'wrap' }}>
          {['continue', 'accept_completed', 'keep_blocked'].map((action) => <button className="btn" key={action} data-recovery-action={action}
            disabled={disabled || expired || !note.trim() || (action === 'continue' && !instruction.trim())} onClick={() => decide(action)}>{t(`nrec.${action}`)}</button>)}
        </div>
      </div>}
    </section>
  );
}
