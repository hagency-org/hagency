'use client';

/*
 * The readiness and version strip (ADR-145): read-only presentation of
 * facts the console already has. The readiness cell reads the existing
 * unauthenticated `GET /ready` same-origin and consumes the payload
 * as-is — never `/health` (200-while-live is the wrong boundary), never a
 * console route, never proxied. The version and store-head cells are
 * build-time constants from the generated status-constants module, so the
 * store head is the binary's EXPECTED schema head, never a live query.
 *
 * No state-word enumeration lives here — one vocabulary, the server's:
 * every state word renders as text, and only the sweep component's own
 * liveness words participate in colouring, mirroring the server's single
 * ready predicate. The strip has no controls: restart/stop is the service
 * wrapper's slice, not this one.
 */
import { useEffect, useState } from 'react';
import { useT } from '@/components/Prefs';
import { fetchReadiness } from '@/lib/native-api';
import { HAGENCY_NATIVE_VERSION, HAGENCY_NATIVE_SCHEMA_HEAD } from '@/lib/generated/status-constants';

/* The settled not-serving words of ADR-145's render rules; refused tick
 * words are READY words and must never colour a cell not-ready. */
const NOT_READY_STATES = ['closed', 'stopped', 'unavailable', 'outcome_unknown', 'not_started'];

export default function NativeStatusStrip() {
  const t = useT();
  const [readiness, setReadiness] = useState(null);
  useEffect(() => {
    const controller = new AbortController();
    fetchReadiness(controller.signal)
      .then((v) => setReadiness({ kind: 'answer', value: v }))
      .catch((e) => { if (e?.name !== 'AbortError') setReadiness({ kind: 'unreachable' }); });
    return () => controller.abort();
  }, []);
  let state = 'unknown';
  let failing = [];
  if (readiness?.kind === 'answer') {
    const value = readiness.value;
    failing = value.components.filter((c) => NOT_READY_STATES.includes(c.state));
    state = value.status === 'ok' && failing.length === 0 ? 'ready' : 'not-ready';
  }
  return <section
    className="panel"
    style={{ marginTop: 0, padding: '8px 14px' }}
    data-native-status={state}
    aria-label={t('ns.title')}
  >
    <div className="page-head" style={{ alignItems: 'baseline', gap: 14 }}>
      <span data-native-status-cell="readiness" className={state === 'ready' ? '' : ' warn'}>
        {t('ns.title')}: {t(state === 'ready' ? 'ns.ready' : state === 'not-ready' ? 'ns.notReady' : 'ns.unknown')}
      </span>
      {state === 'not-ready' && failing.length > 0 && <span data-native-status-cell="components" className="note">
        {failing.map((c) => `${c.name}=${c.state}`).join(' ')}
      </span>}
      <span className="spacer" />
      <span data-native-status-cell="version" className="note">{t('ns.version')}: {HAGENCY_NATIVE_VERSION}</span>
      <span data-native-status-cell="schema-head" className="note">{t('ns.schemaHead')}: {HAGENCY_NATIVE_SCHEMA_HEAD}</span>
    </div>
  </section>;
}
