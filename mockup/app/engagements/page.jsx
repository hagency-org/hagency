'use client';

import { useState } from 'react';
import AgentAllocationChoice from '@/components/AgentAllocationChoice';
import Link from 'next/link';
import PageHead from '@/components/PageHead';
import TechnicalDetails from '@/components/TechnicalDetails';
import { Toast, useToast } from '@/components/Toast';
import { Blank } from '@/components/Blank';
import { useT } from '@/components/Prefs';
import { fmtTokens } from '@/lib/mock-data';
import { useData, Provenance } from '@/components/Data';
import { revokeEngagement, send } from '@/lib/api';
import CredentialForm from '@/components/CredentialForm';
import { allocationValue, approvalVerdict, projectLabel } from '@/lib/console-workflow';
import { NATIVE_MODE } from '@/lib/native-api';
import NativeEngagements from '@/components/NativeEngagements';

/*
 * ④ 接洽 — what replaces dispatch.
 *
 * Nothing here schedules work: which task an agent does is decided on the project
 * side. What the contributor decides is whether to be in a project at all, and
 * for how much.
 *
 * The routing:
 *
 *   whitelisted + within the standing offer + within the agent's ceiling → auto-join
 *   whitelisted + over either                                            → FALLS BACK to approval
 *   not whitelisted                                                      → approval
 *
 * Falling back rather than rejecting is the rule that keeps the two halves
 * coherent: the project did nothing wrong by asking for more than is left, so
 * refusing it would send the wrong signal. The owner decides.
 *
 * Over-commitment is computed PER AGENT, because an engagement draws on one
 * agent's ceiling. Two projects wanting an architect served by the same Opus agent
 * share that 5M — so the approval row has to name the agent BEFORE the decision,
 * not after.
 */

/**
 * Why this request needs a decision instead of auto-joining.
 *
 * Reads the data context itself rather than taking `offers` and `remaining` as
 * props. It is a component, so a hook is legal here, and threading two more props
 * through every call site would make the row markup harder to read for no gain.
 */
function RouteReason({ e, t }) {
  const { offers, remaining } = useData();
  if (e.route === 'notWhitelisted') {
    return <span className="stranded">{t('en.route.notWhitelisted')}</span>;
  }
  const offer = offers.find((o) => o.role === e.role);
  if (e.route === 'overOffer') {
    return <span className="overqual">{t('en.route.overOffer', { cap: fmtTokens(offer?.budgetCapPerEngagement) })}</span>;
  }
  return (
    <span className="overqual">
      {t('en.route.overCeiling', { left: fmtTokens(remaining(e.agent)), agent: e.agent })}
    </span>
  );
}

/*
 * 项目方 — WHOSE budget each request spends.
 *
 * On this page rather than its own, because a request's route and its refusal come from the same place:
 * a room id carries its origin server, that server IS the project side, and the side's allocation is the
 * second ceiling every request has to pass. The first is the agent's own. A reader looking at "declined
 * — over allocation" needs the allocation on the same screen or the sentence is unattributable.
 *
 * THREE ALLOCATION STATES, RENDERED AS THREE. `null` is UNALLOCATED and refuses everything; `0` is a
 * deliberate close that leaves the side configured; a number is a budget. Printing null as 0 would erase
 * the distinction the store exists to keep, and this console's own rule is that a blank is never a zero.
 */
function ProjectSides({ t }) {
  /*
   * `roleCapacity` is read HERE and not taken as a prop. The first version called `roleName` — which is
   * defined inside `EngagementsPage`, not in this scope — and the build caught it with
   * `ReferenceError: roleName is not defined` while prerendering. Worth noting that the root eslint
   * config ignores `mockup/**`, so `no-undef` never sees this file: in the backend that same class of
   * mistake is caught by a linter, and here only by `next build`.
   */
  const { projectSides, provenance, roleCapacity, refresh } = useData();
  const sides = projectSides ?? [];
  const roleName = (key) => roleCapacity.roles[key]?.displayName ?? key;

  /*
   * THE VOCABULARY HERE WAS INVENTED, and it made every working side look unexamined.
   *
   * It matched `'ok'`, `'unauthorized'` and `'forbidden'` — none of which the backend produces. `ACCESS_STATES`
   * is `unverified | accepted | rejected | unreachable | blocked`, so `accepted` fell through to the default and
   * a side that had verified successfully rendered as "never checked". The operator asked why: they had a
   * working appservice and a console telling them it had never been looked at.
   *
   * A display that invents its own state names cannot be wrong about them loudly — it can only be wrong
   * quietly, which is why this survived. Named from the store's exported list rather than from memory.
   */
  const reach = (state) => {
    if (state === 'accepted') return <span className="ok">{t('en.reachOk')}</span>;
    if (state === 'rejected') return <span className="stranded">{t('en.reachBad')}</span>;
    // `blocked` is a working server with an account in the way — actionable, and not a refusal of the token.
    if (state === 'blocked') return <span className="overqual">{t('en.reachBlocked')}</span>;
    if (state === 'unreachable') return <span className="overqual">{t('en.reachUnknown')}</span>;
    return <span className="dim">{t('en.reachNever')}</span>;
  };

  /*
   * The budget cell. `budget === null` means that side's own read failed — distinct from an unallocated
   * side, and it must not borrow the unallocated wording, because "refuses all work" would be a claim
   * about the side rather than about our ignorance.
   */
  const alloc = (side) => {
    if (!side.budget) return <Blank why="en.why.sideBudgetUnread" t={t} />;
    const { allocated, committed, remaining, poolCommitted } = side.budget;
    const poolSummary = <span className="dim">{t('en.poolSideCommitted', { n: fmtTokens(poolCommitted ?? 0) })}</span>;
    if (allocated === null) {
      return (
        <>
          <span className="stranded">{t('en.allocUnset')}</span>
          <span className="dim">{t('en.allocUnsetWhy')}</span>
          {poolSummary}
        </>
      );
    }
    if (allocated === 0) return <><span className="overqual">{t('en.allocClosed')}</span>{poolSummary}</>;
    /*
     * THE ORPHAN LINE, and it says nothing on a healthy side. A delete now releases its commitments, so
     * `orphanedCommitted` is 0 and this renders nothing — it exists for fleets that predate that fix, where
     * the committed figure includes promises to agents that no longer exist. Silence when the number is
     * zero is what keeps it worth reading when it is not.
     */
    const orphaned = side.budget.orphanedCommitted ?? 0;
    return (
      <>
        <span>{t('en.allocLeft', { left: fmtTokens(remaining), alloc: fmtTokens(allocated) })}</span>
        <span className="dim">{t('en.allocCommitted', { n: fmtTokens(committed) })}</span>
        {poolSummary}
        {orphaned > 0 ? (
          <span className="stranded">{t('en.allocOrphaned', { n: fmtTokens(orphaned) })}</span>
        ) : null}
      </>
    );
  };

  return (
    <>
      <h2 className="sec">{t('en.sidesHead')}<span className="note">{t('en.sidesNote')}</span></h2>
      <div className="notice">{t('en.sidesIntro')}</div>
      {provenance.projectSides === 'absent' ? (
        <div className="notice">{t('en.sidesAbsent')}</div>
      ) : sides.length === 0 ? (
        <div className="notice">{t('en.sidesEmpty')}</div>
      ) : (
        <div className="tbl-wrap">
          {/* `tbl`, like every other table here. Without it the three anti-weld rules
              (.tbl td > span.dim, .tbl td .proj, .tbl td .staff) missed this table entirely and it
              shipped "490k of 1.0M left510k committed". */}
          <table className="tbl">
            <thead>
              <tr>
                <th>{t('en.colSide')}</th>
                <th>{t('en.repHead')}</th>
                <th>{t('en.colCred')}</th>
                <th>{t('col.state')}</th>
                <th>{t('en.colAlloc')}</th>
                <th>{t('en.colProjects')}</th>
              </tr>
            </thead>
            <tbody>
              {sides.map((side) => (
                <tr key={side.id}>
                  <td>
                    <strong>{side.label || side.id}</strong>
                    {side.label && <TechnicalDetails><code>{side.id}</code></TechnicalDetails>}
                    {!side.active && <span className="dim">{t('en.sideInactive')}</span>}
                  </td>
                  <td>
                    {side.representative
                      ? <span className="mono">{side.representative}</span>
                      : <span className="dim">{t('en.repNone')}</span>}
                    {/* The namespace is the point of an appservice: every future agent is already
                        covered by it, so nothing has to be registered one at a time. */}
                    {side.namespace && (
                      <TechnicalDetails><code>{side.namespace}</code><p>{t('en.nsNote')}</p></TechnicalDetails>
                    )}
                  </td>
                  <td>
                    {side.credentialKind
                      ? <span>{t(side.credentialKind === 'appservice' ? 'en.credentialAppservice' : side.credentialKind === 'registrationToken' ? 'en.credentialToken' : 'en.colCred')}</span>
                      : <span className="stranded">{t('en.credNone')}</span>}
                    {/* Entering one was a curl-only act until now (ADR-016 decision 8). The form can
                        write a credential it can never read back — the read side stays closed. */}
                    {side.connectionMode === 'outbound' && <TechnicalDetails label={t('en.outbound')}>
                      <code>{side.outboundEndpoint}</code>
                    </TechnicalDetails>}
                    <CredentialForm
                      side={side}
                      live={provenance.projectSides === 'live'}
                      onDone={refresh}
                    />
                    {/*
                      * THE TWO THINGS AN OPERATOR CAN ACTUALLY DO, and neither existed here.
                      *
                      * The only action on this column was a form asking for tokens that — when Hagency issued
                      * them — were readable for one moment and are write-only afterwards. So for the common
                      * case the sole affordance was one the operator could not complete. They asked twice why
                      * "set credential" was still there.
                      *
                      * `check status` answers the question the column is FOR. `reissue` is safe to offer now
                      * because issuing stages: the live credential keeps working until the new one is
                      * installed and verified, so a mistaken click costs nothing.
                      */}
                    <SideActions side={side} live={provenance.projectSides === 'live'} onDone={refresh} />
                  </td>
                  {/*
                    * WAITING IS NOT FAILING. A registration loads only when their homeserver restarts,
                    * so between issuing it and them acting there is a gap we do not control. Rendered as
                    * its own state rather than as "unverified", which reads as something we got wrong.
                    */}
                  <td>{side.awaitingInstall ? (
                    <>
                      <span className="overqual">{t('en.awaitingInstall')}</span>
                      <span className="dim">{t('en.awaitingInstallWhy')}</span>
                    </>
                  ) : reach(side.accessState)}</td>
                  <td>
                    {alloc(side)}
                    <AllocationEditor side={side} live={provenance.projectSides === 'live'} onDone={refresh} />
                  </td>
                  <td>{!side.projects?.length
                    ? <span className="dim">{t('en.projNone')}</span>
                    : side.projects.map((pr) => (
                      <div key={pr.id} className="proj">
                        <span className={pr.archived ? 'dim' : ''}>{pr.name}</span>
                        {pr.archived && <span className="dim">{t('en.projArchived')}</span>}
                        {pr.roomId
                          ? <TechnicalDetails><code>{pr.roomId}</code></TechnicalDetails>
                          : <span className="dim">{t('en.projNoRoom')}</span>}
                        {pr.agents.length === 0
                          ? (pr.awaitingBind?.length
                            /*
                             * APPROVED IS NOT STAFFED, and saying only 「还没派人」 hid a real state: a project
                             * with 50k committed to `soaker` read as untouched, while the engagement carried
                             * `bindError: no owner known for this agent…`. The budget column said 已承诺 and
                             * this column said nobody — two true statements that together made no sense.
                             *
                             * The reason is shown verbatim from the backend rather than reworded. It names the
                             * two settings that fix it, which is more than any phrasing here could.
                             */
                            ? pr.awaitingBind.map((w) => (
                              <span key={w.agent} className="staff">
                                <Link href={`/agents/${encodeURIComponent(w.agent)}`}>{w.agent}</Link>
                                <span className="stranded">{t('en.projStaffAwaiting')}</span>
                                {w.bindError && <span className="dim">{w.bindError}</span>}
                              </span>
                            ))
                            : <span className="dim">{t('en.projStaffNone')}</span>)
                          : pr.agents.map((a) => (
                            <span key={a.name} className="staff">
                              <Link href={`/agents/${encodeURIComponent(a.name)}`}>{a.name}</Link>
                              {a.role && <span className="dim">{roleName(a.role)}</span>}
                              {/*
                                * Four separate facts, never collapsed into one badge. An agent can be
                                * reachable and stopped, or running and cut off, and a single "status"
                                * would have to pick one to report. `online === null` is "no such agent
                                * record", which is not the same as offline.
                                */}
                              {a.retiredAt && <span className="stranded">{t('en.staffRetired')}</span>}
                              {!a.bound && <span className="stranded">{t('en.staffUnbound')}</span>}
                              {a.online === false && !a.retiredAt && <span className="overqual">{t('en.staffDown')}</span>}
                              {a.online === null && <span className="overqual">{t('en.staffNoRecord')}</span>}
                            </span>
                          ))}
                      </div>
                    ))}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}

function AllocationEditor({ side, live, onDone }) {
  const t = useT();
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState('');
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState(null);
  async function save(event) {
    event.preventDefault();
    if (!live || busy) return;
    let allocated_tokens;
    try { allocated_tokens = allocationValue(value); }
    catch (error) { setNote(t(error.message)); return; }
    setBusy(true);
    const res = await send(`project-sides/${encodeURIComponent(side.id)}/allocation`, {
      method: 'PUT', body: { allocated_tokens },
    });
    setBusy(false);
    if (!res.ok) { setNote(res.error); return; }
    setEditing(false);
    setNote(null);
    await onDone?.();
  }
  if (!editing) return (
    <button type="button" className="btn-s" disabled={!live} onClick={() => {
      setValue(String(side.allocatedTokens ?? ''));
      setEditing(true);
    }}>{t('en.editAllocation')}</button>
  );
  return (
    <form onSubmit={save}>
      <label>{t('en.allocationTokens')}
        <input className="inp" type="number" min="0" step="1" value={value}
          onChange={(e) => setValue(e.target.value)} />
      </label>
      <p className="why-inline">{t('en.allocationHelp')}</p>
      <div className="btn-row">
        <button className="btn-s" type="submit" disabled={busy}>{t('en.saveAllocation')}</button>
        <button className="btn-s" type="button" disabled={busy} onClick={() => setEditing(false)}>{t('act.cancel')}</button>
      </div>
      {note && <p role="alert" className="warn-text">{note}</p>}
    </form>
  );
}

function ApprovalForm({ engagement, onApprove, onCancel }) {
  const t = useT();
  const [allocation, setAllocation] = useState(null);
  const [candidatesReady, setCandidatesReady] = useState(false);
  const [tokens, setTokens] = useState(String(engagement.allocatedTokens > 0
    ? engagement.allocatedTokens : engagement.requestedTokens ?? ''));
  const [ownerMxid, setOwnerMxid] = useState(engagement.requestContext?.ownerMxid ?? '');
  const [ownerDmRoomId, setOwnerDmRoomId] = useState(engagement.requestContext?.ownerDmRoomId ?? '');
  const [ownerMissing, setOwnerMissing] = useState(false);
  const requireOwner = ownerMissing || engagement.ownerBindingRequired !== false;
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState(null);
  async function approve(event) {
    event.preventDefault();
    if (busy) return;
    let body;
    try { body = approvalVerdict({ tokens, ownerMxid, ownerDmRoomId, projectRoomId: engagement.projectRoomId, requireOwner }); }
    catch (error) { setNote(t(error.message)); return; }
    setBusy(true);
    const res = await onApprove({ ...body, ...(allocation ? { allocation } : {}) });
    setBusy(false);
    if (res?.ok) onCancel();
    else if (res?.code === 'owner_unavailable') {
      setOwnerMissing(true);
      setNote(t('en.ownerRequired'));
    } else if (res?.error) setNote(res.error);
  }
  return (
    <form onSubmit={approve}>
      <AgentAllocationChoice engagement={engagement} value={allocation} onChange={setAllocation} onReady={setCandidatesReady} />
      <label>{t('en.approvalTokens')}
        <input className="inp" type="number" min="1" step="1" required value={tokens}
          onChange={(e) => setTokens(e.target.value)} />
      </label>
      <details open={requireOwner}>
        <summary>{t('en.ownerBinding')}</summary>
        {requireOwner && <p className="why-inline">{t('en.ownerRequired')}</p>}
        <p className="why-inline">{t('en.ownerBindingHelp')}</p>
        <label>{t('en.ownerMxid')}
          <input className="inp" value={ownerMxid} onChange={(e) => setOwnerMxid(e.target.value)}
            required={requireOwner} autoComplete="off" placeholder="@owner:example.org" />
        </label>
        <label>{t('en.ownerDmRoomId')}
          <input className="inp" value={ownerDmRoomId} onChange={(e) => setOwnerDmRoomId(e.target.value)}
            required={requireOwner} autoComplete="off" placeholder="!private:example.org" />
        </label>
      </details>
      {note && <p role="alert" className="warn-text">{note}</p>}
      <div className="btn-row">
        <button className="btn primary" type="submit" disabled={busy || !candidatesReady}>{t('en.confirmApproval')}</button>
        <button className="btn" type="button" disabled={busy} onClick={onCancel}>{t('act.cancel')}</button>
      </div>
    </form>
  );
}

function SideActions({ side, live, onDone }) {
  const t = useT();
  const [busy, setBusy] = useState(null);
  const [note, setNote] = useState(null);

  async function run(action) {
    if (!live) return;
    /*
     * A REISSUE WITHOUT A KNOWN ADDRESS IS REFUSED, not defaulted. The url decides where the homeserver pushes
     * events; guessing it produces a registration that installs cleanly and receives nothing, which is the one
     * failure this whole area keeps circling. A credential typed in by hand never told us its url, so "we do
     * not know" is a real state and the answer is to send the operator to the flow that asks.
     */
    if (action === 'reissue' && !side.appserviceUrl) {
      setNote(t('cr.noUrl'));
      return;
    }
    setBusy(action);
    setNote(null);
    const res = action === 'verify'
      ? await send(`project-sides/${encodeURIComponent(side.id)}/verify`, { method: 'POST', body: {} })
      : await send(`project-sides/${encodeURIComponent(side.id)}/registration-file`, {
        method: 'POST',
        /*
         * The address the homeserver reaches us at is not this component's to choose, so a reissue keeps
         * whatever the side was configured with. A wrong url here would produce a registration that installs
         * cleanly and receives nothing — the documented silent failure.
         */
        body: { url: side.appserviceUrl },
      });
    setBusy(null);
    if (res.ok === false) { setNote(res.error); return; }
    const body = res.body ?? {};
    /*
     * A CHECK SAYS NOTHING BY ITSELF, because the column beside it already says the answer.
     *
     * A first version echoed `body.side.accessState` here, so the row read 「accepted 可达」 — the same fact
     * twice, once in the operator's language and once in the store's raw enum. Nothing is gained by repeating
     * the status next to the status, and leaking the internal vocabulary invites someone to start matching on
     * it, which is how the invented-enum defect began.
     *
     * A PROMOTION is different: it is a change this click caused, and the status column cannot express it —
     * `accepted` before and `accepted` after, with a different credential live in between.
     */
    setNote(action === 'verify'
      ? (body.promoted ? t('cr.promoted') : null)
      : (body.stagedNote ? t('cr.staged') : t('cr.issued', { path: body.path ?? '' })));
    onDone?.();
  }

  if (!side.hasCredential) return null;
  return (
    <div className="btn-row">
      <button type="button" className="btn-s" disabled={!live || busy} onClick={() => run('verify')}>
        {busy === 'verify' ? t('cr.verifying') : t('cr.verify')}
      </button>
      {side.credentialKind === 'appservice' && (
        <button type="button" className="btn-s" disabled={!live || busy} onClick={() => run('reissue')}>
          {busy === 'reissue' ? t('cr.reissuing') : t('cr.reissue')}
        </button>
      )}
      {note && <span className="why-inline">{note}</span>}
    </div>
  );
}

export default function EngagementsPage() {
  const data = useData();
  return data.nativeConsole ? <NativeEngagements /> : <LegacyEngagements />;
}

function LegacyEngagements() {
  const t = useT();
  const {
    pendingEngagements, activeEngagements, endedEngagements, whitelist,
    roleCapacity, remaining, overCommits, offers, presetOf, agents, capability,
    provenance, refresh, projectSides,
  } = useData();
  const roleName = (key) => roleCapacity.roles[key]?.displayName ?? key;
  const [toast, say] = useToast();
  const [wlRoom, setWlRoom] = useState('');
  const [wlName, setWlName] = useState('');
  const [approving, setApproving] = useState(null);
  const [revoking, setRevoking] = useState(null);
  const label = e => projectLabel(e, projectSides, whitelist);

  /*
   * Real writes when the endpoint is behind the page, simulated otherwise.
   *
   * The server re-derives the headroom and refuses an over-committing allocation
   * itself, so the check the form performs before the click is a courtesy rather
   * than the guard — a client-side-only limit is one any other client can ignore.
   * A refusal is surfaced with the server's own message, which names the agent and
   * what is left on it, because "declined" alone leaves nothing to act on.
   */
  const live = provenance.engagements === 'live';
  async function act(kind, e, body) {
    if (!live) return null;
    const path = kind === 'verdict' ? `engagements/${e.id}/verdict`
      : kind === 'revoke' ? `engagements/${e.id}/revoke`
        : null;
    if (!path) return null;
    const res = kind === 'revoke' ? await revokeEngagement(e.id, body) : await send(path, { body });
    await refresh();
    if (!res.ok) say('fail', res.error);
    return res;
  }

  async function revoke(e) {
    if (!live) return say('ok', t('en.wouldRevoke', { project: label(e) }));
    if (revoking) return;
    setRevoking(e.id);
    try {
      const res = await act('revoke', e, { reason: 'revoked from the console' });
      if (!res?.ok) return;
      const state = res.body?.engagement?.withdrawal?.state;
      const scope = res.body?.engagement?.withdrawal?.scope === 'agent' ? 'retirement' : 'withdrawal';
      if (state === 'failed' || res.body?.roomWithdrawal?.left === false) say('fail', t(`en.${scope}.failed`));
      else if (res.reconciled && !['complete', 'retained'].includes(state)) say('fail', t(`en.${scope}.pending`));
      else say('ok', t('en.didRevoke', { project: label(e) }));
    } finally { setRevoking(null); }
  }

  const pending = pendingEngagements();
  const active = activeEngagements();
  const ended = endedEngagements();

  return (
    <>
      <PageHead title={t('en.title')} sub={t('en.sub')} />

      {/* All five slices have endpoints now. The banner stays because the page's
          controls behave differently behind a live store — they write — and a
          reader pressing Approve is entitled to know whether it committed
          anything. */}
      <Provenance slices={['agents', 'engagements', 'offers', 'whitelist', 'ceilings', 'projectSides']} />

      {/* The routing stated once, at the top, because every row below is an
          instance of it and a reader who has to infer the rule from examples will
          infer the wrong one. */}
      <div className="notice">{t('en.routingNote')}</div>

      <ProjectSides t={t} />

      <div className="cards">
        <div className="card">
          <div className="cap">{t('en.cPending')}</div>
          <div className={`val${pending.length > 0 ? ' warn' : ''}`}>{pending.length}</div>
        </div>
        <div className="card"><div className="cap">{t('en.cActive')}</div><div className="val ok">{active.length}</div></div>
        <div className="card"><div className="cap">{t('en.cWhitelisted')}</div><div className="val">{whitelist.length}</div></div>
        <div className="card">
          <div className="cap">{t('en.cCommitted')}</div>
          <div className="val">{fmtTokens(active.reduce((n, e) => n + (e.allocatedTokens ?? 0), 0))}</div>
        </div>
      </div>

      <h2 className="sec">{t('en.pendingHead')}<span className="note">{t('en.pendingNote')}</span></h2>
      <div className="tbl-wrap">
        <table className="tbl">
          <thead>
            <tr>
              <th>{t('col.project')}</th>
              <th>{t('col.role')}</th>
              <th>{t('en.wouldServe')}</th>
              <th>{t('col.requested')}</th>
              <th>{t('en.needsYou')}</th>
              <th>{t('col.action')}</th>
            </tr>
          </thead>
          <tbody>
            {pending.map((e) => {
              const over = overCommits(e);
              return (
                <tr key={e.id}>
                  <td>
                    <div>{label(e)}</div>
                    {/* The room id is the identity; the name is decoration. Shown
                        together so the reader can see which one they are trusting. */}
                    <span className="dim mono-s">{e.projectRoomId}</span>
                    <span className="dim">{`${e.requester} · ${e.since}`}</span>
                  </td>
                  <td>{roleName(e.role)}</td>
                  <td>
                    {/* `agent` is legitimately null when no configured agent qualifies
                        for the role. Rendering it anyway produced a link to
                        /agents/null and a headroom of "—" with no explanation; the
                        useful answer is that nothing can serve this request. */}
                    {e.requestContext?.agentDefinition && <strong>{e.requestContext.agentDefinition.name}</strong>}
                    {e.agent ? (
                      <>
                        <Link href={`/agents/${e.agent}`}>{e.agent}</Link>
                        <span className="dim">{t('en.leftN', { n: fmtTokens(remaining(e.agent)) })}</span>
                      </>
                    ) : capability().find((row) => row.key === e.role)?.resources?.length > 0
                      ? <span className="dim">{t('en.provisionOnApproval')}</span>
                      : <Blank why="en.why.noQualifyingAgent" t={t} />}
                  </td>
                  <td>
                    <span className="amount">{fmtTokens(e.requestedTokens)}</span>
                    <span className="dim">{`${fmtTokens(e.ratePerDay)}/d`}</span>
                  </td>
                  <td><RouteReason e={e} t={t} /></td>
                  <td>
                    <div className="btn-row tight">
                      <button
                        className="btn primary"
                        onClick={() => {
                          if (!live) {
                            return say(over ? 'err' : 'ok', over
                              ? t('en.wouldRefuse', { left: fmtTokens(remaining(e.agent)), agent: e.agent })
                              : t('en.wouldApprove', { n: fmtTokens(e.requestedTokens) }));
                          }
                          return setApproving(e.id);
                        }}
                      >
                        {t('en.approve')}
                      </button>
                      <button
                        className="btn"
                        onClick={async () => {
                          if (!live) return say('ok', t('en.wouldReject'));
                          const res = await act('verdict', e, { approve: false, reason: 'rejected from the console' });
                          if (res?.ok) say('ok', t('en.didReject', { id: e.id }));
                          return null;
                        }}
                      >
                        {t('en.reject')}
                      </button>
                    </div>
                    {approving === e.id && <ApprovalForm engagement={e} onCancel={() => setApproving(null)}
                      onApprove={async (body) => {
                        const res = await act('verdict', e, body);
                        if (res?.ok) say('ok', t('en.didApprove', { n: fmtTokens(body.allocatedTokens) }));
                        return res;
                      }} />}
                    {/* The constraint is shown before the click, not discovered
                        after it: approving 1.2M against 1.1M remaining is the
                        error this column exists to prevent. */}
                    {over && <span className="stranded">{t('en.wouldOverCommit')}</span>}
                  </td>
                </tr>
              );
            })}
            {pending.length === 0 && (
              <tr><td colSpan={6} className="dim">{t('en.noPending')}</td></tr>
            )}
          </tbody>
        </table>
      </div>

      <h2 className="sec">{t('en.activeHead')}<span className="note">{t('en.activeNote')}</span></h2>
      <div className="tbl-wrap">
        <table className="tbl">
          <thead>
            <tr>
              <th>{t('col.project')}</th><th>{t('col.role')}</th><th>{t('col.agent')}</th>
              <th>{t('col.allocated')}</th><th>{t('col.used')}</th>
              <th>{t('col.joinedHow')}</th><th>{t('col.action')}</th>
            </tr>
          </thead>
          <tbody>
            {active.map((e) => (
              <tr key={e.id}>
                <td><div>{label(e)}</div><span className="dim mono-s">{e.projectRoomId}</span></td>
                <td>{roleName(e.role)}</td>
                <td><Link href={`/agents/${e.agent}`}>{e.agent}</Link></td>
                <td className="amount">{fmtTokens(e.allocatedTokens)}</td>
                {/* Same systemic blank as everywhere else: nothing meters tokens. */}
                <td><Blank why="en.why.notMetered" t={t} /></td>
                <td>
                  {/* An auto-approval I cannot see afterwards is indistinguishable
                      from a compromise, so auto-joined engagements are listed
                      rather than hidden. */}
                  {e.autoJoined
                    ? <span className="badge ok">{t('en.autoJoined')}</span>
                    : <span className="badge">{t('en.byApproval')}</span>}
                </td>
                <td>
                  <button
                    className="btn danger"
                    disabled={Boolean(revoking)}
                    onClick={() => revoke(e)}
                  >
                    {t(revoking === e.id ? 'en.revoking' : 'en.revoke')}
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <h2 className="sec">{t('en.endedHead')}</h2>
      <div className="tbl-wrap">
        <table className="tbl">
          <thead>
            <tr><th>{t('col.project')}</th><th>{t('col.role')}</th><th>{t('col.allocated')}</th><th>{t('col.reason')}</th></tr>
          </thead>
          <tbody>
            {ended.map((e) => (
              <tr key={e.id}>
                <td><div>{label(e)}</div><small className="dim mono-s">{e.requestContext?.agentDefinition?.name || e.agent}</small></td>
                <td>{roleName(e.role)}</td>
                {/* An engagement that ended without ever being approved never had
                    an allocation, and a blank cell here says nothing — the rule
                    everywhere else on this console is that an absence carries its
                    reason. `fmtTokens(null)` returns null, which React renders as
                    literally nothing. */}
                <td>
                  {e.allocatedTokens === null || e.allocatedTokens === undefined
                    ? <Blank why="en.why.neverAllocated" t={t} />
                    : <span className="amount">{fmtTokens(e.allocatedTokens)}</span>}
                </td>
                {/*
                  * The reason may be a dictionary key (the fixture's
                  * `en.ended.completed`) or free text a person typed at the point
                  * of rejection. `t()` returns the key back when it does not
                  * resolve, which printed a raw `en.ended.*` string on screen for
                  * anything the backend recorded.
                  */}
                <td className="dim">
                  {/^[a-z]+\.[a-zA-Z.]+$/.test(e.endedReason ?? '') ? t(e.endedReason) : (e.endedReason ?? '')}
                  {e.withdrawal && <p>{t(`en.${e.withdrawal.scope === 'agent' ? 'retirement' : 'withdrawal'}.${e.withdrawal.state}`)}</p>}
                  {e.withdrawal?.reason && <p>{e.withdrawal.reason}</p>}
                  {['failed', 'pending'].includes(e.withdrawal?.state)
                    ? <button className="btn" disabled={Boolean(revoking)} onClick={() => revoke(e)}>{t(e.withdrawal.scope === 'agent' ? 'en.retryRetirement' : 'en.retryWithdrawal')}</button>
                    : e.requestContext?.fleetId && e.allocatedTokens > 0 && e.withdrawal?.scope !== 'agent'
                      && !active.some(row => row.agent === e.agent)
                      && <button className="btn" disabled={Boolean(revoking)} onClick={() => revoke(e)}>{t('en.removeFromMatrix')}</button>}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Below a divider and under its own heading, because editing it is a
          privilege change and not a preference: adding a project means its future
          requests bypass the owner entirely. */}
      <hr className="divider" />
      <h2 className="sec danger-head">{t('en.wlHead')}<span className="note">{t('en.wlNote')}</span></h2>
      <div className="notice warn">{t('en.wlKeyNote')}</div>

      {/*
        * ADDING, which the page could not do at all until now — it offered only
        * removal, so the trust list was read-mostly and the one direction that
        * grants power had no control.
        *
        * The room id is typed, never picked from a list of projects that have
        * asked: a chooser would make it trivial to trust the wrong one by clicking
        * a familiar display name, and the display name is precisely the spoofable
        * part. The backend validates the id shape too, so a mistyped entry is
        * refused rather than stored as a rule that can never match.
        */}
      <form
        className="btn-row"
        style={{ margin: '10px 0 14px', flexWrap: 'wrap' }}
        onSubmit={async (ev) => {
          ev.preventDefault();
          const room = wlRoom.trim();
          if (!room) return say('fail', t('en.wlNeedRoom'));
          if (!live) return say('ok', t('en.wouldAdd', { room }));
          const res = await send('whitelist', {
            body: { projectRoomId: room, displayName: wlName.trim() || null },
          });
          if (!res.ok) return say('fail', res.error);
          setWlRoom('');
          setWlName('');
          await refresh();
          return say('ok', t('en.didAdd', { room }));
        }}
      >
        <input
          className="inp mono-s"
          style={{ minWidth: 260 }}
          value={wlRoom}
          onChange={(e) => setWlRoom(e.target.value)}
          placeholder={t('en.wlRoomPlaceholder')}
          aria-label={t('en.wlRoom')}
        />
        <input
          className="inp"
          style={{ minWidth: 200 }}
          value={wlName}
          onChange={(e) => setWlName(e.target.value)}
          placeholder={t('en.wlNamePlaceholder')}
          aria-label={t('en.wlName')}
        />
        <button className="btn" type="submit">{t('en.wlAdd')}</button>
        <span className="dim" style={{ flexBasis: '100%' }}>{t('en.wlAddWarn')}</span>
      </form>

      <div className="tbl-wrap">
        <table className="tbl">
          <thead>
            <tr><th>{t('en.wlRoom')}</th><th>{t('en.wlName')}</th><th>{t('col.added')}</th><th>{t('col.action')}</th></tr>
          </thead>
          <tbody>
            {whitelist.map((w) => (
              <tr key={w.projectRoomId}>
                <td className="mono-s">{w.projectRoomId}</td>
                <td className="dim">{w.displayName}</td>
                <td className="dim">{`${w.addedAt} · ${w.addedBy}`}</td>
                <td>
                  <button
                    className="btn danger"
                    onClick={async () => {
                      if (!live) return say('ok', t('en.wouldRemove', { name: w.displayName }));
                      const res = await send(`whitelist/${encodeURIComponent(w.projectRoomId)}`, { method: 'DELETE' });
                      if (!res.ok) return say('fail', res.error);
                      await refresh();
                      /*
                       * The count of surviving engagements is reported, not
                       * swallowed. Removal affects future requests only, so a
                       * silent success would read as "that project is gone" while
                       * its work is still running under the trust just withdrawn.
                       */
                      const still = res.body?.stillActive?.length ?? 0;
                      return say('ok', still > 0
                        ? t('en.didRemoveStillActive', { name: w.displayName ?? w.projectRoomId, n: still })
                        : t('en.didRemove', { name: w.displayName ?? w.projectRoomId }));
                    }}
                  >
                    {t('en.wlRemove')}
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {/* Removing trust must not kill work in flight, or de-trusting a project
          silently cancels an engagement nobody meant to cancel. */}
      <div className="notice">{t('en.wlRemoveNote')}</div>

      <Toast toast={toast} />
    </>
  );
}
