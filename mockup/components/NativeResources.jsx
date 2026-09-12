'use client';
import PageHead from '@/components/PageHead';
import TechnicalDetails from '@/components/TechnicalDetails';
import ResourceAgents from '@/components/ResourceAgents';
import { NativeAccessNotice } from '@/components/NativeUsage';
import { useData } from '@/components/Data';
import { useT } from '@/components/Prefs';

const label = (r) => [r.framework, r.model, r.reasoning].filter(Boolean).join(' · ');
function BudgetPart({ value, account = false }) {
  const t = useT(); const number = (n) => n == null ? t('nu.unknown') : n.toLocaleString();
  return <section className="panel" style={{ marginTop: 0 }}><h3>{t(account ? 'nr.account' : 'nr.pool')}</h3><dl className="kv">
    <dt>{t(account ? 'nr.quota' : 'col.ceiling')}</dt><dd data-budget={account ? 'quota' : 'ceiling'}>{number(account ? value.quota : value.ceiling)}</dd>
    <dt>{t('nr.period')}</dt><dd>{value.period === 'monthly' ? t('rs.monthly') : value.period ?? t('nu.unknown')}</dd>
    <dt>{t('nr.committed')}</dt><dd>{number(value.committed)}</dd>
    <dt>{t('nr.remaining')}</dt><dd>{number(value.remaining)}</dd>
    {account && <><dt>{t('nr.declaration')}</dt><dd>{t(`nr.seat.${value.status}`)}</dd></>}
  </dl></section>;
}
export default function NativeResources() {
  const t = useT(); const data = useData();
  const { phase, selected, resources = [], roles = [], budget, action } = data;
  const number = (n) => n == null ? t('nu.unknown') : n.toLocaleString();
  return <>
    <PageHead title={t('rs.title')} sub={t('nr.sub')} />
    <p className="muted">{t('nr.localOnly')}</p>
    {['ready', 'stale'].includes(phase) && <div className="btn-row"><a className="btn primary" href={`/console/resources/new/${selected ? `?source_resource_id=${selected}` : ''}`}>{t('nc.create')}</a></div>}
    {phase === 'loading' && <p role="status">{t('nr.loading')}</p>}
    <NativeAccessNotice />
    {action && <section className="notice" data-resource-action={action.kind} role={action.kind === 'pending' || action.kind === 'saved' ? 'status' : 'alert'}>
      <p><b>{action.label}</b> · {t(`nr.action.${action.kind}`)}</p>
      {['conflict', 'unknown', 'busy', 'refused'].includes(action.kind) && <button className="btn" disabled={phase === 'access'} onClick={data.refresh}>{t('nr.reconcile')}</button>}
    </section>}
    {phase === 'error' && <section className="panel" role="alert"><h2>{t('nr.failed')}</h2><p>{t(data.error === 'not_found' ? 'nr.notFound' : 'nu.retryHelp')}</p><button className="btn" onClick={data.refresh}>{t('nu.refresh')}</button></section>}
    {['ready', 'stale'].includes(phase) && <div data-native-resource-state={phase} aria-busy={data.refreshing === true}>
      {data.refreshing && <p role="status">{t('nr.refreshing')}</p>}
      {phase === 'stale' && <p role="alert">{t('nr.stale')}</p>}
      <section className="panel"><h2 className="sec" style={{ marginTop: 0 }}>{t('nr.profile')}</h2>
        <div className="field"><label htmlFor="native-resource">{t('nr.selected')}</label>
          {resources.length || selected ? <select id="native-resource" value={selected ?? ''} onChange={(event) => data.choose(event.target.value)}>
            {selected && !resources.some((r) => r.id === selected) && <option value={selected}>{t('nr.outsidePage')}</option>}
            {resources.map((r) => <option key={r.id} value={r.id}>{label(r)}</option>)}
          </select> : <p>{t('nr.empty')}</p>}
        </div>
        <div className="btn-row"><button className="btn" onClick={data.refresh}>{t('nu.refresh')}</button><button className="btn" onClick={data.firstPage}>{t('nu.firstPage')}</button><button className="btn" disabled={!data.next_after} onClick={data.nextPage}>{t('nu.nextPage')}</button><button className="btn" onClick={data.logout}>{t('nu.logout')}</button></div>
        {!data.permissions?.publishResource && <div className="notice"><p>{t(data.permissions?.configureResource ? 'nc.publicationSeparate' : 'nr.readOnly')}</p><code>hagency console-access --state-dir &lt;state&gt; --listen &lt;address&gt; --manage-resource-publication</code></div>}
        <div className="tbl-wrap"><table className="tbl"><thead><tr><th>{t('nr.profile')}</th><th>{t('col.ceiling')}</th><th>{t('nr.catalog')}</th><th>{t('col.action')}</th></tr></thead><tbody>
          {resources.map((r) => <tr key={r.id} data-resource-row={r.id}><td>{label(r)}{r.provider && <div className="dim">{r.provider}</div>}<TechnicalDetails><code>{r.id}</code></TechnicalDetails></td><td>{number(r.ceiling?.tokens)}</td><td data-publication={r.published}>{t(r.published ? 'nr.included' : 'nr.withdrawn')}</td><td><a className="btn" href={`/console/resources/new/?resource_id=${r.id}`}>{t('nc.edit')}</a> <ResourceAgents preset={r} native={{ allowed: data.permissions?.publishResource && phase === 'ready', busy: action?.kind === 'pending', publish: data.publish }} /></td></tr>)}
        </tbody></table></div><p className="dim">{t('nr.pageOnly')}</p>
      </section>
      {budget && <section className="panel" data-resource-id={selected}><h2 className="sec" style={{ marginTop: 0 }}>{t('nr.budget')}</h2><p>{t('nr.budgetMeaning')}</p><div className="split even"><BudgetPart value={budget.pool} /><BudgetPart value={budget.seat} account /></div><p>{t('nr.effectiveRemaining')}: <b>{number(budget.remainingTokens)}</b></p></section>}
      <section className="panel"><h2 className="sec" style={{ marginTop: 0 }}>{t('nr.roles')}</h2><div className="tbl-wrap"><table className="tbl"><thead><tr><th>{t('nr.role')}</th><th>{t('nr.choice')}</th><th>{t('nr.eligible')}</th><th>{t('nr.crossFamily')}</th></tr></thead><tbody>{roles.map((r) => <tr key={r.role}><td>{r.role}</td><td>{t(r.explicitPublication === null ? 'nr.automatic' : r.explicitPublication ? 'nr.enabled' : 'nr.disabled')}</td><td>{t(r.available ? 'nr.yes' : 'nr.no')}</td><td>{t(r.crossFamily ? 'nr.yes' : 'nr.no')}</td></tr>)}</tbody></table></div></section>
    </div>}
    <TechnicalDetails><p>{t('nr.roleMeaning')}</p><p>{t('nr.gaps')}</p>{selected && <p>{t('nr.identifier')}: <code>{selected}</code></p>}{data.error && <code>{data.error}</code>}{action?.error && <code>{action.error}</code>}</TechnicalDetails>
  </>;
}
