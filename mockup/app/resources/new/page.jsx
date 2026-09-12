'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import Link from 'next/link';
import PageHead from '@/components/PageHead';
import { Toast, useToast } from '@/components/Toast';
import { useT } from '@/components/Prefs';
import { presetCommand, fmtTokens } from '@/lib/mock-data';
import { ExecutionPolicyChoice } from '@/components/ExecutionPermissions';
import { useData, Provenance } from '@/components/Data';
import { send } from '@/lib/api';
import TechnicalDetails from '@/components/TechnicalDetails';
import { NativeAccessNotice } from '@/components/NativeUsage';

/*
 * ② 配置向导 — four steps, and three of them write a field that already exists.
 *
 * The output is a **preset** (framework-presets.json, POST /api/framework-presets),
 * whose fields are exactly what normalizeRuntimeProfileRole() accepts at
 * backend-v2.js:713 — framework, provider, model, reasoning. A preset is reusable,
 * so "my Opus donation" is configured once and attached to several agents.
 *
 * The fourth step, the budget, has NO upstream field. It is also the only step the
 * contributor really cares about, which is an uncomfortable combination: the thing
 * they are deciding is the thing the system cannot yet hold. So it is collected,
 * labelled unenforced, and left out of the printed command — a form that silently
 * sent a ceiling the endpoint drops would be worse than one that admits the gap.
 */

const STEPS = ['framework', 'model', 'reasoning', 'budget'];

/** Codex thinking levels, from the real enumeration's `reasoning` values. */
function reasoningChoices(roleCapacity, framework) {
  const seen = new Set();
  for (const tier of roleCapacity.tiers) {
    for (const c of roleCapacity.tierAccepts[tier] ?? []) {
      if (c.framework === framework && c.reasoning) seen.add(c.reasoning);
    }
  }
  return [...seen];
}

/** Which roles this combination would qualify for, computed live from the config. */
function qualifiesFor(roleCapacity, framework, model, reasoning) {
  const rank = { lightweight: 0, medium: 1, strong: 2 };
  let tier = null;
  for (const tr of roleCapacity.tiers) {
    const hit = (roleCapacity.tierAccepts[tr] ?? []).find((c) => (
      c.framework === framework && c.model === model
      && (c.reasoning === undefined || c.reasoning === reasoning)
    ));
    if (hit) { tier = tr; break; }
  }
  if (!tier) return { tier: null, roles: [] };
  const roles = Object.entries(roleCapacity.roles)
    .filter(([, r]) => rank[tier] >= rank[r.defaultTier])
    .map(([k, r]) => ({ key: k, name: r.displayName }));
  return { tier, roles };
}

export default function WizardPage() {
  const data = useData(); const t = useT();
  if (!data.nativeConsole) return <WizardForm />;
  return <>
    <NativeAccessNotice />
    {data.phase === 'loading' && <p role="status">{t('nr.loading')}</p>}
    {data.phase === 'error' && <section className="notice" role="alert"><p>{t('nr.failed')}</p><button className="btn" onClick={data.refresh}>{t('nu.refresh')}</button></section>}
    {data.action?.configuration && <section className="notice" data-configuration-action={data.action.kind} role={data.action.kind === 'saved' || data.action.kind === 'pending' ? 'status' : 'alert'}>
      <p>{t(`nc.action.${data.action.kind}`)}</p>
      {data.action.error === 'resource_in_use' && <p>{t('nc.inUse')}</p>}
      <a className="btn" href={`/console/resources/${data.action.result ? `?resource_id=${data.action.result.resourceId}` : ''}`}>{t(data.action.result ? 'nc.openSaved' : 'nr.reconcile')}</a>
      {data.action.result && <TechnicalDetails><code>{data.action.result.resourceId}</code></TechnicalDetails>}
    </section>}
    {['ready', 'stale'].includes(data.phase) && (data.editor
      ? <div data-native-configuration-id={data.editor.resource.id} aria-busy={data.refreshing === true}><WizardForm key={`${data.editing}:${data.editor.resource.id}`} native={data} /></div>
      : <section className="panel"><PageHead title={t('nc.create')} sub={t('nc.scope')} /><p>{t('nc.noSource')}</p><a className="btn" href="/console/resources/">{t('wz.cancel')}</a></section>)}
  </>;
}
const validNativeTokens = (value) => /^[0-9]+$/.test(String(value)) && Number.isSafeInteger(Number(value)) && Number(value) >= 0;
function nativeDraft(resource) {
  return { framework: resource.framework, provider: resource.provider, model: resource.model, reasoning: resource.reasoning, tokens: resource.ceiling?.tokens ?? '', profileKind: 'preserve', ceilingKind: 'preserve' };
}
function WizardForm({ native = null }) {
  const t = useT(); const data = useData();
  const [observation, setObservation] = useState(native?.editor ?? null);
  const base = observation?.resource;
  const { FRAMEWORKS = [], MODEL_SELECTABLE = {}, modelsFor = () => [], roleCapacity, frameworks = [], detected = [], provenance = {}, refresh } = data;
  const live = provenance.presets === 'live';
  const [toast, say] = useToast();
  const [step, setStep] = useState(0);
  const [draft, setDraft] = useState(() => native ? nativeDraft(base) : { name: '', framework: null, provider: null, model: null, reasoning: null, tokens: 1_000_000, rateCapPerDay: 50_000, yolo: false });
  const router = useRouter();
  const set = (patch) => setDraft((d) => ({ ...d, ...patch, ...(native && ('model' in patch || 'reasoning' in patch) ? { profileKind: 'select' } : {}) }));
  const selectable = draft.framework ? MODEL_SELECTABLE[draft.framework] : null;
  const models = native ? observation.choices.map((c) => ({ ...c, provider: base.provider })) : draft.framework ? modelsFor(draft.framework) : [];
  const reasonings = native ? [...new Set(observation.choices.filter((c) => c.model === draft.model).map((c) => c.reasoning))] : draft.framework ? reasoningChoices(roleCapacity, draft.framework) : [];
  const choice = native ? observation.choices.find((c) => c.model === draft.model && c.reasoning === draft.reasoning) : null;
  const outcome = native ? { tier: choice?.tier ?? (draft.profileKind === 'preserve' ? observation.modelTier : null), roles: (choice?.roles ?? (draft.profileKind === 'preserve' ? observation.modelRoles : [])).map((r) => ({ key: r, name: r })) }
    : draft.framework && draft.model ? qualifiesFor(roleCapacity, draft.framework, draft.model, draft.reasoning) : { tier: null, roles: [] };
  const manifest = frameworks.find((f) => f.id === draft.framework) ?? null;
  const chosenDetect = detected.find((f) => f.id === draft.framework) ?? null;
  const action = native?.action?.configuration ? native.action : null;
  const blocked = native && (native.phase !== 'ready' || !native.permissions?.configureResource || ['pending', 'unknown', 'saved'].includes(action?.kind)
    || (!native.editing && !choice) || (draft.ceilingKind === 'monthly' && !validNativeTokens(draft.tokens)));
  const reload = async () => {
    const value = await refresh();
    if (value?.editor?.resource.id === base.id && value.editing === native.editing) { setObservation(value.editor); setDraft(nativeDraft(value.editor.resource)); }
  };
  return (
    <>
      <PageHead title={t(native ? native.editing ? 'nc.edit' : 'nc.create' : 'wz.title')} sub={t(native ? 'nc.scope' : 'wz.sub')}>
        {native ? <a className="btn" href="/console/resources/">{t('wz.cancel')}</a> : <Link className="btn" href="/resources">{t('wz.cancel')}</Link>}
      </PageHead>

      {native ? <>
        <p>{t('nc.association')}</p>
        {native.refreshing && <p role="status">{t('nr.refreshing')}</p>}
        {native.phase === 'stale' && <p role="alert">{t('nr.stale')}</p>}
        {!native.permissions?.configureResource && <div className="notice"><p>{t('nc.readOnly')}</p><code>hagency console-access --state-dir &lt;state&gt; --listen &lt;address&gt; --manage-resource-configuration</code></div>}
        <div className="btn-row"><button className="btn" onClick={refresh}>{t('nu.refresh')}</button><button className="btn" onClick={reload}>{t('nc.reload')}</button><button className="btn" onClick={native.logout}>{t('nu.logout')}</button></div>
      </> : <Provenance slices={['frameworks', 'ceilings']} />}

      {/* Progress is a list of steps with the current one marked, not a bar: the
          reader needs to know which decision they are on, not a percentage. */}
      <ol className="steps wizard">
        {STEPS.map((s, i) => (
          <li key={s} className={i === step ? 'on' : i < step ? 'done' : ''}>
            <span className="n">{i + 1}</span>
            <span>{t(`wz.step.${s}`)}</span>
          </li>
        ))}
      </ol>

      {step === 0 && native && <section className="panel"><h3 className="sub">{t('nc.source')}</h3>
        {!native.editing && <div className="field"><label htmlFor="configuration-source">{t('nc.source')}</label><select id="configuration-source" value={base.id} onChange={(event) => native.choose(event.target.value)}>
          {!native.resources.some((r) => r.id === base.id) && <option value={base.id}>{t('nr.outsidePage')}</option>}
          {native.resources.map((r) => <option key={r.id} value={r.id}>{[r.framework, r.model, r.reasoning].filter(Boolean).join(' · ')}</option>)}
        </select><div className="btn-row"><button className="btn" onClick={native.firstPage}>{t('nu.firstPage')}</button><button className="btn" disabled={!native.next_after} onClick={native.nextPage}>{t('nu.nextPage')}</button></div></div>}
        <dl className="kv"><dt>{t('wz.step.framework')}</dt><dd>{base.framework}</dd><dt>{t('col.provider')}</dt><dd>{base.provider ?? t('nu.unknown')}</dd><dt>{t('col.model')}</dt><dd>{base.model}</dd></dl>
        <TechnicalDetails><code>{base.id}</code></TechnicalDetails>
      </section>}
      {step === 0 && !native && (
        <div className="panel">
          <h3 className="sub">{t('wz.pickFramework')}</h3>
          <div className="fw-grid">
            {FRAMEWORKS.map((f) => {
              const mf = frameworks.find((x) => x.id === f) ?? null;
              // The host probe, which knows whether it is actually installed here.
              const det = detected.find((x) => x.id === f) ?? null;
              return (
                <button
                  key={f}
                  /*
                   * Disabled when nothing can be selected for it.
                   *
                   * `codex-acp` has zero qualifying combinations, so choosing it led to
                   * an empty model step with a permanently disabled Next — a dead end
                   * reached by clicking, with nothing on screen explaining why. A
                   * control that cannot lead anywhere should not be pressable.
                   */
                  className={`fw${draft.framework === f ? ' on' : ''}${modelsFor(f).length === 0 ? ' fw-dead' : ''}`}
                  disabled={modelsFor(f).length === 0}
                  aria-pressed={draft.framework === f}
                  onClick={() => set({ framework: f, model: null, provider: null, reasoning: null })}
                >
                  <b>{mf?.displayName ?? f}</b>
                  <span className="dim">{f}</span>
                  <span className="dim">{t('wz.nModels', { n: modelsFor(f).length })}</span>
                  {modelsFor(f).length === 0
                    ? <span className="badge warn-b">{t('wz.noQualifyingModel')}</span>
                    : MODEL_SELECTABLE[f]?.ok === false && (
                      <span className="badge warn-b">{t('wz.modelFixed')}</span>
                    )}
                  {/*
                    * NOT INSTALLED is the fact worth warning about. `launchable:false`
                    * is not: every ACP manifest carries it, and each one's reason says
                    * "start it with hagency acp-up instead" — a different command, not
                    * an inability. Warning on it told a contributor their working
                    * framework could not run.
                    */}
                  {det && det.state === 'absent' && (
                    <span className="badge warn-b">{t('wz.notInstalled')}</span>
                  )}
                  {det && det.state === 'needs_auth' && (
                    <span className="badge warn-b">{t('wz.needsAuth')}</span>
                  )}
                </button>
              );
            })}
          </div>

          {/* Stated up front, because it changes what the next step can promise. */}
          {selectable?.ok === false && (
            <div className="notice warn">{t(selectable.why)}</div>
          )}

          {/* How it starts, from the probe — not a warning, a fact. */}
          {chosenDetect && chosenDetect.state !== 'absent' && (
            <div className="notice">
              {t('wz.startsWith', { cmd: chosenDetect.startWith })}
            </div>
          )}
          {chosenDetect && chosenDetect.state === 'absent' && (
            <div className="notice warn">
              <div><b>{t('wz.notInstalled')}</b></div>
              <div>{t('wz.notInstalledWhy', { fix: chosenDetect.fix ?? '' })}</div>
            </div>
          )}

          {/*
            * THE SANDBOX BEING LENT.
            *
            * The design asked for this in step 1 and the fixture could not supply
            * it: `permissionSummary` and the refused flags live in the manifests,
            * which had no endpoint until GET /api/frameworks existed. A
            * contributor deciding what to lend is deciding what permissions to
            * lend, so this is a first-step fact rather than a footnote.
            */}
          {manifest && (
            <dl className="kv" style={{ marginTop: 14 }}>
              <dt>{t('wz.transport')}</dt><dd className="mono-s">{manifest.transport}</dd>
              {manifest.permissionSummary && (
                <>
                  <dt>{t('wz.sandbox')}</dt>
                  <dd>{manifest.permissionSummary}</dd>
                </>
              )}
              {manifest.refusedFlags?.length > 0 && (
                <>
                  <dt>{t('wz.refuses')}</dt>
                  <dd>
                    {manifest.refusedFlags.map((fl) => (
                      <span className="badge" key={fl}>{fl}</span>
                    ))}
                    <div className="dim">{t('wz.refusesNote')}</div>
                  </dd>
                </>
              )}
            </dl>
          )}
        </div>
      )}

      {step === 1 && (
        <div className="panel">
          <h3 className="sub">{t('wz.pickModel')}</h3>
          {models.length === 0 ? (
            <div className="notice">{t('wz.noModels')}</div>
          ) : (
            <div className="tbl-wrap">
              <table className="tbl">
                <thead>
                  <tr>
                    <th>{t('col.model')}</th><th>{t('col.provider')}</th>
                    {!native && <th>{t('col.family')}</th>}<th>{t('col.tier')}</th><th>{t('col.action')}</th>
                  </tr>
                </thead>
                <tbody>
                  {models.map((m) => (
                    <tr key={`${m.model}-${m.tier}-${m.reasoning ?? ''}`}>
                      <td className="mono-s">{m.model}</td>
                      <td>{m.provider ?? (native ? t('nu.unknown') : null)}</td>
                      {!native && <td>{m.family}</td>}
                      <td>
                        <span className={`tierchip ${m.tier}`}>{m.tier}</span>
                        {m.reasoning && <span className="dim"> {t('wz.atReasoning', { r: m.reasoning })}</span>}
                      </td>
                      <td>
                        <button
                          className={`btn${draft.model === m.model && draft.reasoning === (m.reasoning ?? null) ? ' primary' : ''}`}
                          onClick={() => set({ model: m.model, provider: m.provider, reasoning: m.reasoning ?? null })}
                        >
                          {t('wz.pick')}
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      {step === 2 && (
        <div className="panel">
          <h3 className="sub">{t('wz.pickReasoning')}</h3>
          {reasonings.length === 0 ? (
            <div className="notice">{t('wz.noReasoning', { f: draft.framework ?? '' })}</div>
          ) : (
            <>
              <div className="prefs-row" role="group" aria-label={t('wz.pickReasoning')}>
                {reasonings.map((r) => (
                  <button
                    key={r ?? 'default'}
                    className="seg"
                    aria-pressed={draft.reasoning === r}
                    onClick={() => set({ reasoning: r })}
                  >
                    {r ?? t('nc.defaultReasoning')}
                  </button>
                ))}
              </div>
              {/* The consequence, not just the setting. Thinking level is what
                  decides the tier for Codex, so it decides which roles I can
                  offer — a fact a bare radio group hides. */}
              <div className="notice">{t('wz.reasoningDecidesTier')}</div>
            </>
          )}
        </div>
      )}

      {step === 3 && (
        <div className="panel">
          <h3 className="sub">{t('wz.setBudget')}</h3>
          <p className="notice">{t(native ? 'nc.localCatalog' : 'rs.definitionHelp')}</p>
          {!native && draft.framework === 'codex' && <ExecutionPolicyChoice resource yolo={draft.yolo} onChange={yolo => set({ yolo })} />}
          {/*
            * The preset's NAME, which the form never asked for.
            *
            * A preset is reusable across agents — that is its whole point — so it
            * needs a name a contributor recognises six weeks later. Without this
            * field the name was auto-derived as `codex · gpt-5.6-sol`, which is the
            * model configuration restated rather than a label: two presets on the
            * same model at different reasoning levels would be indistinguishable in
            * every list that shows them. The printed equivalent command already
            * referenced a `name`, which is how the omission surfaced.
            */}
          {!native && <>
          <div className="field-row">
            <label htmlFor="wz-name">{t('wz.presetName')}</label>
            <input
              id="wz-name" type="text" style={{ width: 240 }}
              value={draft.name}
              placeholder={`${draft.framework} · ${draft.model ?? ''}`}
              onChange={(e) => set({ name: e.target.value })}
            />
            <span className="dim">{t('wz.presetNameHint')}</span>
          </div>
          </>}
          {native && <div className="field"><label htmlFor="configuration-ceiling">{t('nc.ceilingChange')}</label><select id="configuration-ceiling" value={draft.ceilingKind} onChange={(event) => set({ ceilingKind: event.target.value })}>
            {['preserve', 'clear', 'monthly'].map((kind) => <option key={kind} value={kind}>{t(`nc.ceiling.${kind}`)}</option>)}
          </select><p>{t('nc.ceilingMeaning')}</p></div>}
          <div className={native ? "field" : "field-row"}>
            <label htmlFor="wz-tokens">{t('wz.monthlyTokens')}</label>
            <input
              id="wz-tokens" type={native ? "text" : "number"} inputMode={native ? "numeric" : undefined} pattern={native ? "[0-9]*" : undefined} aria-invalid={native && draft.ceilingKind === 'monthly' && !validNativeTokens(draft.tokens) ? true : undefined} min={native ? "0" : "100000"} step={native ? "1" : "100000"} disabled={native && draft.ceilingKind !== 'monthly'}
              value={draft.tokens}
              onChange={(e) => set({ tokens: native ? e.target.value : Number(e.target.value) })}
            />
            <span className="dim">{native && !validNativeTokens(draft.tokens) ? draft.ceilingKind === 'monthly' ? t('nc.invalidTokens') : '—' : fmtTokens(draft.tokens)}</span>
          </div>
          {!native && <>
          <div className="field-row">
            <label htmlFor="wz-rate">{t('wz.rateCap')}</label>
            <input
              id="wz-rate" type="number" min="0" step="10000"
              value={draft.rateCapPerDay}
              onChange={(e) => set({ rateCapPerDay: Number(e.target.value) })}
            />
            <span className="dim">{`${fmtTokens(draft.rateCapPerDay)}/d`}</span>
          </div>
          {/* The uncomfortable admission, made once and plainly. */}
          <div className="notice warn">{t('wz.budgetNotEnforced')}</div>
          </>}
          {native && <TechnicalDetails><p>{t('nc.gaps')}</p></TechnicalDetails>}
        </div>
      )}

      {/* The outcome travels with every step once a model is chosen, because
          "which roles does this let me offer" is the question the whole wizard is
          really answering, and finding out only at the end is too late to change
          a decision cheaply. */}
      {outcome.tier && (
        <div className="panel outcome">
          <h3 className="sub">{t(native ? 'nc.qualification' : 'wz.outcome')}</h3>{native && <p>{t('nc.qualificationMeaning')}</p>}
          <div className="prov-row">
            <span className="grow">{t('wz.qualifiesTier')}</span>
            <span className={`tierchip ${outcome.tier}`}>{outcome.tier}</span>
          </div>
          <div className="prov-row">
            <span className="grow">{t(native ? 'nc.matchesRoles' : 'wz.qualifiesRoles', { n: outcome.roles.length })}</span>
            <span>{outcome.roles.map((r) => <span className="chip-role" key={r.key}>{r.name}</span>)}</span>
          </div>
        </div>
      )}

      <div className="btn-row">
        <button className="btn" disabled={step === 0} onClick={() => setStep((s) => Math.max(0, s - 1))}>
          {t('wz.back')}
        </button>
        {step < STEPS.length - 1 ? (
          <button
            className="btn primary"
            disabled={step === 0 ? !draft.framework : step === 1 ? !draft.model : false}
            onClick={() => setStep((s) => s + 1)}
          >
            {t('wz.next')}
          </button>
        ) : (
          <button
            className="btn primary"
            disabled={blocked}
            onClick={async () => {
              if (native) {
                await native.configure(base, { profileChange: draft.profileKind === 'preserve' ? { kind: 'preserve' } : { kind: 'select', model: draft.model, reasoning: draft.reasoning }, ceilingChange: draft.ceilingKind === 'monthly' ? { kind: 'monthly', tokens: Number(draft.tokens) } : { kind: draft.ceilingKind } });
                return;
              }
              if (!live) return say('ok', t('wz.wouldCreate'));
              /*
               * The ceiling goes in the payload. It used to be omitted on purpose,
               * because POST /api/framework-presets built its record from a closed
               * field list and dropped it — sending it would have made the form
               * appear to save a budget the backend never held. It persists now, so
               * withholding it would silently discard the one field the contributor
               * actually came to set.
               */
              const res = await send('framework-presets', {
                body: {
                  name: draft.name?.trim() || `${draft.framework} · ${draft.model}`,
                  framework: draft.framework,
                  provider: draft.provider,
                  model: draft.model,
                  reasoning: draft.reasoning,
                  executionPolicy: { yolo: draft.framework === 'codex' && draft.yolo === true },
                  ceiling: {
                    tokens: draft.tokens,
                    period: 'monthly',
                    rateCapPerDay: draft.rateCapPerDay || null,
                  },
                },
              });
              if (!res.ok) return say('fail', res.error);
              await refresh();
              say('ok', t('wz.didCreate', { name: res.body?.preset?.name ?? draft.model }));
              /*
               * LEAVE THE FORM. It used to raise a toast and stay put, with every field still
               * filled — so the one visible difference between "saved" and "did nothing" was a
               * message that fades, and the honest reading of the screen was that nothing had
               * happened. Reported as a bug within a minute of someone using it.
               *
               * Return to the saved resource. Approval of a qualifying request
               * will provision its agent; no manual creation step follows this form.
               * The delay keeps the saved confirmation legible.
               */
              setTimeout(() => router.push('/resources'), 1200);
              return undefined;
            }}
          >
            {t(native ? native.editing ? 'nc.save' : 'nc.create' : 'wz.create')}
          </button>
        )}
      </div>

      {/* Shown, not hidden: a contributor must be able to reproduce and script
          what the form did, and seeing the command is how you notice the form
          built the wrong one. */}
      {!native && draft.framework && (
        <>
          <h2 className="sec">{t('wz.equivalent')}</h2>
          <pre className="cmd">{presetCommand(draft)}</pre>
          <div className="notice">{t(live ? 'wz.ceilingSaved' : 'wz.ceilingOmitted')}</div>
        </>
      )}

      <Toast toast={toast} />
    </>
  );
}
