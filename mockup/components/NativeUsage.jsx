'use client';
import PageHead from '@/components/PageHead';
import TechnicalDetails from '@/components/TechnicalDetails';
import { useData } from '@/components/Data';
import { useT } from '@/components/Prefs';

const KINDS = ['input', 'output', 'cacheWrite', 'cacheRead'];
export function NativeAccessNotice() {
  const data = useData(); const t = useT();
  if (data.phase !== 'access') return null;
  const status = data.logoutStatus;
  return <section className="panel" data-native-state="access" data-logout-state={status ?? undefined}>
    <h2>{t(status ? `nr.logout.${status}` : 'nu.access')}</h2>
    {['busy', 'unknown'].includes(status) && <><p role="alert">{t('nr.logoutUnresolved')}</p><button className="btn" onClick={data.logout}>{t('nr.retryLogout')}</button></>}
    {status !== 'pending' && <><p>{t('nu.accessHelp')}</p><code>hagency console-access --state-dir &lt;state&gt; --listen &lt;address&gt;</code></>}
  </section>;
}
function Counts({ value, label }) {
  const t = useT();
  return <section className="panel" style={{ marginTop: 0 }}><h3>{label}</h3><dl>{KINDS.map((kind) => <div key={kind} className="kv">
    <dt>{t(`nu.${kind}`)}</dt><dd data-kind={kind}>{value?.[kind] == null ? t('nu.unknown') : value[kind].toLocaleString()}</dd>
  </div>)}</dl></section>;
}
function Period({ period, title }) {
  const t = useT();
  return <section className="panel"><h2 className="sec" style={{ marginTop: 0 }}>{title}</h2>{period === null ? <p>{t('nu.noPeriod')}</p> : <>
    <p>{period.key} · {t(period.incomplete ? 'nu.incomplete' : 'nu.complete')} · {t('nu.observations', { n: period.observations })}</p>
    <div className="split even"><Counts value={period.observed_growth} label={t('nu.growth')} />
    <Counts value={period.known_growth_lower_bound} label={t('nu.growthLower')} /></div>
  </>}</section>;
}
export default function NativeUsage() {
  const t = useT();
  const data = useData();
  const { phase, report, engagements, selected, error } = data;
  return <>
    <PageHead title={t('us.title')} sub={t('nu.sub')} />
    <p className="muted">{t('nu.evidence')}</p>
    {phase === 'loading' && <p role="status">{t('nu.loading')}</p>}
    <NativeAccessNotice />
    {phase === 'error' && <section className="panel" role="alert"><h2>{t('nu.failed')}</h2><p>{t(error === 'not_found' ? 'nu.notFound' : 'nu.retryHelp')}</p><button className="btn" onClick={data.refresh}>{t('nu.refresh')}</button></section>}
    {['ready', 'stale'].includes(phase) && <div data-native-state={phase} aria-busy={data.refreshing === true}>
      {data.refreshing && <p role="status">{t('nu.refreshing')}</p>}
      {phase === 'stale' && <p role="alert">{t('nu.stale')}</p>}
      <section className="panel"><div className="field"><label htmlFor="native-engagement">{t('nu.engagement')}</label>{engagements.length || selected ? <select id="native-engagement" value={selected ?? ''} onChange={(event) => data.choose(event.target.value)}>
        {selected && !engagements.some((e) => e.id === selected) && <option value={selected}>{t('nu.outsidePage')}</option>}
        {engagements.map((e) => <option key={e.id} value={e.id}>{e.agentName} · {e.projectName ?? t('nu.unnamedProject')} · {e.role}</option>)}
      </select> : <p>{t('nu.empty')}</p>}</div>
      <div className="btn-row"><button className="btn" onClick={data.refresh}>{t('nu.refresh')}</button><button className="btn" onClick={data.firstPage}>{t('nu.firstPage')}</button>
      <button className="btn" onClick={data.nextPage} disabled={!data.next_after}>{t('nu.nextPage')}</button><button className="btn" onClick={data.logout}>{t('nu.logout')}</button></div>
      </section>
      {report && <div data-engagement-id={report.engagement_id}>
        <section className="panel"><h2 className="sec" style={{ marginTop: 0 }}>{t('nu.summary')}</h2><p>{t('nu.sources', { n: report.summary.sources })}</p>
          <p>{t('nu.incompleteSources', { latest: report.summary.latest_incomplete_sources, history: report.summary.historically_incomplete_sources })}</p>
          <p>{t('nu.regressions', { n: report.summary.regression_observations })}</p>
          <div className="split even"><Counts value={report.summary.latest_counts} label={t('nu.latest')} />
          <Counts value={report.summary.known_high_water_lower_bound} label={t('nu.highWater')} /></div>
        </section>
        <section className="panel"><h2 className="sec" style={{ marginTop: 0 }}>{t('nu.ceiling')}</h2><p>{t('nu.ceilingHelp')}</p><dl>
          {['tokens_drawn', 'tokens_used', 'remaining_tokens'].map((key) => <div key={key} className="kv">
            <dt>{t(`nu.${key}`)}</dt><dd data-ceiling={key}>{report.ceiling[key] == null ? t('nu.unknown') : report.ceiling[key].toLocaleString()}</dd>
          </div>)}
        </dl></section>
        <Period period={report.daily} title={t('nu.daily')} /><Period period={report.monthly} title={t('nu.monthly')} />
      </div>}
    </div>}
    <TechnicalDetails><p>{t('nu.limitations')}</p>{selected && <p>{t('nu.engagementId')}: <code>{selected}</code></p>}{error && <code>{error}</code>}</TechnicalDetails>
  </>;
}
