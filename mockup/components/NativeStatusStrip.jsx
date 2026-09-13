'use client';

/*
 * The readiness and version strip (ADR-145): read-only presentation of
 * facts the console already has. The readiness cell reads the existing
 * unauthenticated `GET /ready` same-origin and consumes the payload
 * as-is — never `/health` (200-while-live is the wrong boundary), never a
 * console route, never proxied. The overall word derives from the
 * payload's own `status` rollup and nothing else: there is NO second
 * state-word vocabulary in the client. When the rollup says not ready,
 * every component's raw `name=state` pair is listed verbatim — which
 * necessarily includes each failing component's own name and word, and
 * keeps tick outcome words (`refused_busy` &c) as the ready words they
 * are, rendered without outcome styling. An unreachable `/ready` renders
 * unknown, never ready, with no component list.
 *
 * The version and store-head cells are build-time constants re-assigned
 * by build-native-console.mjs in the staged tree: the store head is the
 * binary's EXPECTED schema head, never a live query. No controls live
 * here — restart/stop is the service wrapper's slice, not this one.
 */
import { useEffect, useState } from 'react';
import { useT } from '@/components/Prefs';
import { fetchReadiness, HAGENCY_NATIVE_SCHEMA_HEAD, HAGENCY_NATIVE_VERSION } from '@/lib/native-api';

export default function NativeStatusStrip() {
  const t = useT();
  // null until the fetch settles; the string marks a network-level failure.
  const [answer, setAnswer] = useState(null);
  const [unreachable, setUnreachable] = useState(false);
  useEffect(() => {
    const controller = new AbortController();
    fetchReadiness(controller.signal)
      .then((value) => setAnswer(value))
      .catch((error) => { if (error?.name !== 'AbortError') setUnreachable(true); });
    return () => controller.abort();
  }, []);
  const state = unreachable || (answer !== null && answer.status !== 'ok')
    ? (unreachable ? 'unknown' : 'not-ready')
    : answer === null ? 'checking' : 'ready';
  const word = state === 'ready' ? t('ns.ready') : state === 'not-ready' ? t('ns.notReady')
    : state === 'unknown' ? t('ns.unknown') : t('ns.checking');
  return <>
    <span
      data-native-status={state}
      data-native-status-cell="readiness"
      style={{ color: state === 'ready' ? 'var(--ok)' : state === 'not-ready' ? 'var(--warn)' : 'var(--ink-dim)' }}
    >{t('ns.title')}: {word}</span>
    {state === 'not-ready' && answer !== null && <span data-native-status-cell="components" className="note">
      {answer.components.map((c) => `${c.name}=${c.state}`).join(' ')}
    </span>}
    <span data-native-status-cell="version" className="note">{t('ns.version')}: {HAGENCY_NATIVE_VERSION}</span>
    <span data-native-status-cell="schema-head" className="note">{t('ns.schemaHead')}: {HAGENCY_NATIVE_SCHEMA_HEAD}</span>
  </>;
}
