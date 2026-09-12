'use client';

import { createContext, useContext, useEffect, useRef, useState } from 'react';
import DataStatus from '@/components/DataStatus';
import { makeDerive } from '@/lib/derive';
import { fetchLive, CONTRACT_SLICES } from '@/lib/api';
import * as fixture from '@/lib/mock-data';
import { NATIVE_MODE, exchangeAccess, fetchNative, fetchResources, resourceView, publishResource, configurationView, configurationSelection, fetchConfiguration, configureResource, logoutNative, selection } from '@/lib/native-api';

/*
 * One data context for the console, with provenance attached.
 *
 * The fixture is the INITIAL value, not a fallback of last resort. Two reasons:
 * the static export has no proxy at all and must render something true about the
 * design, and a first paint that is blank-then-populated makes every assertion
 * racy. So pages always have data; what changes is where it came from.
 *
 * `useData()` returns the fixture's exact export names plus `provenance`, so a
 * page reads `presetOf`, `committed` and `capability` without knowing or caring
 * which source is behind them. The derivations come from one factory
 * (lib/derive.js) for the same reason.
 */

const FIXTURE_DATA = {
  roleCapacity: fixture.roleCapacity,
  agents: fixture.agents,
  presets: fixture.presets,
  offers: fixture.offers,
  whitelist: fixture.whitelist,
  engagements: fixture.engagements,
  projectSides: fixture.projectSides,
  alerts: fixture.alerts,
  usage: fixture.usage,
  frameworks: [],
  /*
   * No seat fixture. A seat is DERIVED from how agents were launched, so inventing
   * one would be inventing a launch topology — and the page's own point is that the
   * derivation is a fact rather than a choice. Fixture mode therefore shows the
   * seat section empty with its reason, which is true: with no backend there is no
   * host to derive a seat from.
   */
  seats: [],
  seatKeyed: null,
  /*
   * No contribution fixture either, and for the same kind of reason. A binding is
   * the record that a project can REACH an agent; inventing one would invent an
   * access grant, and the roster's access column exists precisely to reconcile that
   * record against the allocations. With no backend there is no binding store, so
   * fixture mode shows the column as unanswered — which is true.
   */
  contributions: [],
  /*
   * Never fixtured. A fabricated invitation would invite the contributor to accept a
   * project that does not exist, and an empty fixture would tell them nobody is waiting.
   */
  invites: [],
  detected: fixture.detected,
  detectedAt: null,
  detectCaveat: null,
  usageLive: [],
  metering: null,
  usageTotals: null,
};

/** Everything a page can read, assembled from a data object. */
function assemble(data, provenance, errors, refresh = async () => {}) {
  const derived = makeDerive(data);
  /*
   * When GET /api/capability answered, the SERVER's judgement replaces the local
   * one — same function name, same shape, one page code path. The server is where
   * role-capacity.json is authoritative and where a project-side client would read
   * it, so two answers to "can I fill Reviewer" is exactly the drift this layer
   * exists to prevent.
   */
  const capability = data.capabilityRows
    ? () => data.capabilityRows
    : derived.capability;
  return {
    ...data,
    ...derived,
    capability,
    provenance,
    errors,
    // Re-fetch after a write. A page that mutates and then re-renders from stale
    // local state shows the user their intention rather than the result, which is
    // exactly the failure a live console must not have.
    refresh,
    // True while the first fetch is in flight. Pages do not gate on it — they show
    // fixture data — but the banner uses it so "no backend" is not announced
    // before the attempt has finished.
    loading: provenance.__loading === true,
  };
}

const DataContext = createContext(
  assemble(FIXTURE_DATA, Object.fromEntries([
    ...['agents', 'presets', 'frameworks', 'alerts'].map((k) => [k, 'fixture']),
    ...CONTRACT_SLICES.map((k) => [k, 'contract']),
  ]), {}),
);

export function useData() {
  return useContext(DataContext);
}

// The build-time choice preserves the existing provider's callable contract.
export const DataProvider = NATIVE_MODE ? NativeDataProvider : LegacyDataProvider;

function NativeDataProvider({ children }) {
  const initial = { nativeConsole: true, resourceConsole: false, configurationConsole: false, editor: null, editing: false, phase: 'loading', refreshing: false, requestKey: null, engagements: [], resources: [], roles: [], permissions: { publishResource: false, configureResource: false }, selected: null, report: null, budget: null, next_after: null, error: null };
  const [state, setState] = useState(initial);
  const [logoutStatus, setLogoutStatus] = useState(null);
  const [action, setAction] = useState(null);
  const mutation = useRef(false);
  const admissionEpoch = useRef(0);
  const generation = useRef(0);
  const inFlight = useRef(0);
  const admitted = useRef(false);
  const logoutPending = useRef(Promise.resolve());
  const endingAccess = useRef(false);
  const cursor = useRef('');
  const load = async (after = cursor.current) => {
    if (!admitted.current) return;
    const mine = ++generation.current;
    inFlight.current += 1;
    let requestKey = null;
    try {
      const entry = configurationView(window.location) ? configurationSelection(window.location) : null;
      const resources = entry !== null || resourceView(window.location);
      const requested = entry ? entry.id : selection(window.location, resources ? 'resource_id' : 'engagement_id');
      requestKey = JSON.stringify([entry?.mode ?? resources, requested, after]);
      setState((s) => s.requestKey === requestKey && ['ready', 'stale'].includes(s.phase)
        ? { ...s, refreshing: true, error: null }
        : { ...initial });
      const value = await (entry ? fetchConfiguration(entry, after) : resources ? fetchResources(requested, after) : fetchNative(requested, after));
      if (mine !== generation.current || !admitted.current) return;
      cursor.current = after;
      setState({ ...initial, ...value, phase: 'ready', requestKey });
      return value;
    } catch (error) {
      if (mine !== generation.current) return;
      if (error.message === 'console_access_required') admitted.current = false;
      setState((s) => ['native_unavailable', 'busy'].includes(error.message) && s.requestKey === requestKey && ['ready', 'stale'].includes(s.phase)
        ? { ...s, phase: 'stale', refreshing: false, error: error.message }
        : { ...initial, phase: error.message === 'console_access_required' ? 'access' : 'error', error: error.message });
    } finally { inFlight.current -= 1; }
  };
  useEffect(() => {
    let stopped = false;
    const enter = async () => {
      const mine = ++generation.current;
      admissionEpoch.current += 1;
      admitted.current = false;
      setLogoutStatus(null); setAction(null);
      setState({ ...initial });
      try {
        await exchangeAccess(window.location, window.history, logoutPending.current);
        if (!stopped && mine === generation.current) { admitted.current = true; await load(''); }
      } catch (error) { if (!stopped && mine === generation.current) setState({ ...initial, phase: 'access', error: error.message }); }
    };
    void enter();
    const refresh = () => { if (!stopped && inFlight.current === 0 && document.visibilityState === 'visible') void load(); };
    const navigate = () => { if (!stopped) void load(); };
    const renew = () => { if (!stopped && window.location.hash) void enter(); };
    const timer = setInterval(refresh, 15000);
    window.addEventListener('focus', refresh);
    window.addEventListener('popstate', navigate);
    window.addEventListener('hashchange', renew);
    document.addEventListener('visibilitychange', refresh);
    return () => { stopped = true; admitted.current = false; generation.current += 1; clearInterval(timer); window.removeEventListener('focus', refresh); window.removeEventListener('popstate', navigate); window.removeEventListener('hashchange', renew); document.removeEventListener('visibilitychange', refresh); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const choose = (value) => {
    if (configurationView(window.location)) {
      window.history.pushState(window.history.state, '', `/console/resources/new/?source_resource_id=${encodeURIComponent(value)}`);
      void load(''); return;
    }
    const resources = resourceView(window.location);
    window.history.pushState(window.history.state, '', `/console/${resources ? 'resources/?resource_id' : 'usage/?engagement_id'}=${encodeURIComponent(value)}`);
    void load();
  };
  const logout = async () => {
    if (endingAccess.current) return;
    endingAccess.current = true;
    admitted.current = false;
    admissionEpoch.current += 1;
    const mine = ++generation.current;
    setLogoutStatus('pending');
    if (mutation.current) setAction((a) => a ? { ...a, kind: 'unknown' } : a);
    setState({ ...initial, phase: 'access' });
    const pending = logoutNative();
    logoutPending.current = pending;
    try { await pending; if (mine === generation.current) setLogoutStatus('ended'); }
    catch (error) { if (mine === generation.current) { setLogoutStatus(error.message === 'busy' ? 'busy' : 'unknown'); setState({ ...initial, phase: 'access', error: error.message }); } }
    finally { endingAccess.current = false; if (logoutPending.current === pending) logoutPending.current = Promise.resolve(); }
  };
  const publish = async (resource, published) => {
    if (!admitted.current || mutation.current) return;
    mutation.current = true;
    const epoch = admissionEpoch.current;
    const identity = { id: resource.id, label: `${resource.framework} · ${resource.model}` };
    setAction({ ...identity, kind: 'pending' });
    try {
      await publishResource(resource, published);
      if (epoch !== admissionEpoch.current) return;
      setAction({ ...identity, kind: 'saved' });
      await load();
    } catch (error) {
      if (epoch !== admissionEpoch.current) return;
      const kind = error.message === 'resource_revision_conflict' ? 'conflict' : error.message === 'busy' ? 'busy' : ['outcome_unknown', 'native_unavailable', 'invalid_native_response'].includes(error.message) ? 'unknown' : 'refused';
      setAction({ ...identity, kind, error: error.message });
      if (error.message === 'console_access_required') { admitted.current = false; generation.current += 1; setState({ ...initial, phase: 'access', error: error.message }); }
    } finally { mutation.current = false; }
  };
  const configure = async (resource, changes) => {
    if (!admitted.current || mutation.current) return;
    mutation.current = true;
    const epoch = admissionEpoch.current;
    const identity = { id: resource.id, label: `${resource.framework} · ${resource.model}`, configuration: true };
    setAction({ ...identity, kind: 'pending' });
    try {
      const result = await configureResource(resource, !state.editing, changes);
      if (epoch !== admissionEpoch.current) return;
      setAction({ ...identity, kind: 'saved', result });
      await load();
    } catch (error) {
      if (epoch !== admissionEpoch.current) return;
      const kind = error.message === 'resource_revision_conflict' ? 'conflict' : error.message === 'busy' ? 'busy' : ['outcome_unknown', 'native_unavailable', 'invalid_native_response'].includes(error.message) ? 'unknown' : 'refused';
      setAction({ ...identity, kind, error: error.message });
      if (error.message === 'console_access_required') { admitted.current = false; generation.current += 1; setState({ ...initial, phase: 'access', error: error.message }); }
    } finally { mutation.current = false; }
  };
  return <DataContext.Provider value={{ ...state, action, publish, configure, logoutStatus, choose, refresh: () => load(), nextPage: () => load(state.next_after), firstPage: () => load(''), logout }}>{children}</DataContext.Provider>;
}

function LegacyDataProvider({ children }) {
  const [state, setState] = useState(() => assemble(
    FIXTURE_DATA,
    {
      __loading: true,
      ...Object.fromEntries(['agents', 'presets', 'frameworks', 'alerts'].map((k) => [k, 'fixture'])),
      ...Object.fromEntries(CONTRACT_SLICES.map((k) => [k, 'contract'])),
    },
    {},
  ));

  /*
   * A generation counter, because an older response must never overwrite a newer one.
   *
   * `refresh()` is called after every write, and the effect calls `load()` on mount.
   * With no ordering, two overlapping fetches resolve in whatever order the network
   * gives them: press Revoke, the refresh renders the ended engagement, then the
   * initial fetch completes and puts it back on screen as active. The user sees their
   * change undo itself.
   */
  const generation = useRef(0);
  const inFlight = useRef(0);

  const load = async () => {
    const mine = ++generation.current;
    /*
     * `?data=fixture` forces fixture mode without stopping the backend.
     *
     * Needed because the two suites want opposite things from one dev server:
     * check-switches asserts against fixture rows (contract slices are empty
     * against a live backend, so its cell-layout checks had nothing to inspect and
     * failed for the right reason), while live-ux asserts against real payloads.
     * Also useful by hand: it makes the provenance claim falsifiable in both
     * directions rather than only when a backend happens to be down.
     */
    const stale = () => mine !== generation.current;
    if (typeof window !== 'undefined'
      && new URLSearchParams(window.location.search).get('data') === 'fixture') {
      setState(assemble(
        FIXTURE_DATA,
        {
          ...Object.fromEntries(['agents', 'presets', 'frameworks', 'alerts'].map((k) => [k, 'fixture'])),
          ...Object.fromEntries(CONTRACT_SLICES.map((k) => [k, 'contract'])),
        },
        {},
        load,
      ));
      return;
    }
    inFlight.current += 1;
    try {
      const { data, provenance, errors } = await fetchLive();
      if (stale()) return;
      setState(assemble({ ...FIXTURE_DATA, ...data }, provenance, errors, load));
    } catch (e) {
      // fetchLive already degrades per slice, so reaching here means something
      // structural. Keep the fixture and say why rather than rendering nothing.
      if (stale()) return;
      setState((prev) => assemble(FIXTURE_DATA, { ...prev.provenance, __loading: false }, { all: e.message }, load));
    } finally {
      inFlight.current -= 1;
    }
  };

  useEffect(() => {
    let cancelled = false;
    void load();
    // Automatic observations never overlap a load. Explicit refresh() still
    // starts a newer generation, so a completed write can supersede old reads.
    const refreshVisible = () => {
      if (!cancelled && document.visibilityState === 'visible' && inFlight.current === 0) void load();
    };
    const timer = setInterval(refreshVisible, 15_000);
    window.addEventListener('focus', refreshVisible);
    document.addEventListener('visibilitychange', refreshVisible);
    return () => {
      cancelled = true;
      clearInterval(timer);
      window.removeEventListener('focus', refreshVisible);
      document.removeEventListener('visibilitychange', refreshVisible);
      generation.current += 1;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return <DataContext.Provider value={state}>{children}</DataContext.Provider>;
}

/**
 * The banner that names what is real on this page.
 *
 * Not decoration. Four slices have endpoints and five do not, so a console that
 * looked uniformly live would misrepresent five of its own numbers. `slices` is
 * what this page actually reads, so the banner is specific rather than global.
 */
export function Provenance({ slices }) {
  const { provenance, errors, loading } = useData();
  return <DataStatus slices={slices} provenance={provenance} errors={errors} loading={loading} />;
}
