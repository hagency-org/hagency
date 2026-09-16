/*
 * 项目方 — a project side: one homeserver, the credential Hagency holds there, one representative.
 *
 * ADR-016 decision 1. This is the record that dissolves the circular dependency an operator
 * identified: an agent's MXID contains a server name, so an agent identity cannot be minted before
 * the server is known — yet `agentUserId()` composed `@ac_<name>:<our server>` at startup, before
 * any project existed. The project side is what becomes known first, and the representative
 * (decision 3) is what makes it known while no agent exists.
 *
 * ONE SIDE PER HOMESERVER, and that is the store's key invariant. A homeserver issues accounts; a
 * room does not. Two project rooms on one server are reachable through one credential, so keying on
 * the project (ADR-014's amendment) multiplies credentials by rooms-per-server and still cannot
 * express the fact this record exists to hold: *we are registered here*. That fact is about the
 * server.
 *
 * WHAT THIS STORE DELIBERATELY DOES NOT KNOW. It does not know about engagements, bindings or
 * agents. Decision 7's cascade is an ORDERED sequence across several stores — end engagements,
 * release commitments, deactivate bindings, retire identities, forget the credential last — and
 * that ordering belongs to whoever owns all of them. A store that reached across would make the
 * order implicit, and the order is the part that matters: forgetting the credential first orphans
 * everything that needed it (leaving rooms, revoking tokens). `ApprovalStore` draws the same line.
 */

import { randomBytes } from 'crypto';
import { normalizeOutboundTransport } from './fleet-outbound-config.js';
import {
  chmodSync,
  closeSync,
  existsSync,
  fsyncSync,
  mkdirSync,
  openSync,
  readFileSync,
  renameSync,
  writeFileSync,
} from 'fs';
import path from 'path';

const STORE_VERSION = 1;
const DEFAULT_AUDIT_LIMIT = 2000;

/*
 * A Matrix server name: a DNS name, an IP literal, either optionally with `:port`. Deliberately not
 * a URL — the server name is an identity component (it is the part after the colon in every MXID
 * this side will mint), while the API base URL is a network location that `.well-known` delegation
 * may point somewhere else entirely. Conflating them is what makes a "homeserver" field ambiguous.
 */
const SERVER_NAME_RE = /^[a-z0-9][a-z0-9.\-]*(:\d{1,5})?$/;

/** The two credential kinds of ADR-016 decision 2. Both mint accounts; they differ in what the
 * project side installed, not in what Hagency does afterwards. */
export const CREDENTIAL_KINDS = ['appservice', 'registrationToken'];

/*
 * Access states. A verdict, never a value — the shape ADR-014 decision 6 established for agent
 * credentials, and for the same reason: "invalid" sends an operator to re-issue a token, while
 * "unreachable" sends them to check a network, and collapsing them wastes the wrong afternoon.
 */
/*
 * `blocked` IS THE THIRD ANSWER, and it exists because a walkthrough produced the wrong one of the other two.
 *
 * A registration-token side whose representative localpart was already taken reported `unreachable` — an
 * outage verdict on a homeserver that was answering perfectly. `M_USER_IN_USE` is neither a bad credential nor
 * a dead server: it is a definite, actionable state on a working server, and the two existing values force it
 * into a lie either way. `rejected` would send the operator to ask for a new token they do not need;
 * `unreachable` sends them to check a network that is fine.
 */
export const ACCESS_STATES = ['unverified', 'accepted', 'rejected', 'unreachable', 'blocked'];

export class ProjectSideStoreError extends Error {
  constructor(code, message) {
    super(message);
    this.name = 'ProjectSideStoreError';
    this.code = code;
  }
}

function text(value, field, max = 512) {
  const normalized = typeof value === 'string' ? value.trim() : '';
  if (!normalized || normalized.length > max) {
    throw new ProjectSideStoreError('bad_request', `${field} must be 1..${max} characters`);
  }
  return normalized;
}

/**
 * A server name, normalized to lowercase.
 *
 * Lowercased rather than rejected-if-uppercase because this value ends up inside MXIDs, whose
 * localparts the spec requires to be lowercase, and because an operator typing `Palpo.Test` has
 * made a typo rather than a different decision. The normalization is what makes `oneSidePerServer`
 * hold: `Palpo.test` and `palpo.test` must not become two sides with two credentials for one
 * homeserver.
 */
function serverName(value) {
  const normalized = text(value, 'server_name', 255).toLowerCase();
  if (!SERVER_NAME_RE.test(normalized)) {
    throw new ProjectSideStoreError(
      'bad_request',
      'server_name must be a Matrix server name such as example.org or 127.0.0.1:8008, not a URL',
    );
  }
  return normalized;
}

function apiBaseUrl(value) {
  const normalized = text(value, 'api_base_url', 1024);
  let parsed;
  try {
    parsed = new URL(normalized);
  } catch {
    throw new ProjectSideStoreError('bad_request', 'api_base_url must be an absolute URL');
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    throw new ProjectSideStoreError('bad_request', 'api_base_url must be http or https');
  }
  // Stored without a trailing slash so callers can concatenate `/_matrix/...` without producing
  // a double slash, which some homeservers 404 rather than normalize.
  return normalized.replace(/\/+$/, '');
}

/*
 * Validate a credential and return it. THE RETURN VALUE IS A SECRET and must never reach a
 * projection — see `publicSide`.
 *
 * Per-kind fields rather than a bag, because the two kinds fail differently and an absent field
 * should be a refusal at write time instead of a 401 at use time. Note that `appservice` carries no
 * per-agent credential at all: Hagency masquerades with `asToken` via `?user_id=`, which is why
 * ADR-014 decision 4's per-agent `{ homeserver, accessToken }` is not merely misplaced for an
 * appservice side but unrepresentable.
 */
function credential(value, previous = null) {
  if (!value || typeof value !== 'object') {
    throw new ProjectSideStoreError('bad_request', 'credential must be an object');
  }
  const kind = text(value.kind, 'credential.kind', 64);
  if (!CREDENTIAL_KINDS.includes(kind)) {
    throw new ProjectSideStoreError(
      'bad_request',
      `credential.kind must be one of ${CREDENTIAL_KINDS.join(', ')}`,
    );
  }
  if (kind === 'appservice') {
    let transport;
    if (value.transport !== undefined) {
      try { transport = normalizeOutboundTransport(value.transport, /^(hf_[a-f0-9]{32})_representative$/.exec(value.senderLocalpart ?? '')?.[1]); }
      catch { throw new ProjectSideStoreError('bad_request', 'invalid outbound fleet configuration'); }
    }
    const asToken = text(value.asToken, 'credential.asToken', 4096);
    if (transport && [asToken, value.hsToken].includes(transport.token)) {
      throw new ProjectSideStoreError('bad_request', 'outbound credentials must be independent');
    }
    return {
      kind,
      ...(transport ? { transport } : {}),
      asToken,
      outboundGeneration: previous?.kind === kind && previous.asToken === asToken && previous.outboundGeneration
        ? previous.outboundGeneration
        : randomBytes(16).toString('hex'),
      hsToken: text(value.hsToken, 'credential.hsToken', 4096),
      /*
       * THE CALLBACK URL IS REMEMBERED, and it was not. It is chosen once when the registration is issued and
       * then lives only inside a YAML file on somebody else's machine — so a reissue had nothing to reuse, and
       * a reissue with the wrong url produces a registration that installs cleanly and receives nothing. That
       * is the documented silent failure, arriving from a button rather than from a typo.
       *
       * Optional, because a credential typed in by hand may not know it: a project side who generated their own
       * registration has the url in their file and never told us. Absent means "we do not know", which a reissue
       * must treat as a question rather than a default.
       */
      url: value.url === undefined || value.url === null || value.url === ''
        ? null
        : text(value.url, 'credential.url', 1024),
      // The namespace we claim. `@ac_.*` formalizes the existing MATRIX_AGENT_PREFIX default
      // rather than changing it (ADR-014 decision 2).
      namespace: text(value.namespace, 'credential.namespace', 255),
      senderLocalpart: text(value.senderLocalpart, 'credential.senderLocalpart', 255).toLowerCase(),
    };
  }
  const representativeToken = value.representativeToken === null || value.representativeToken === undefined
    ? null
    : text(value.representativeToken, 'credential.representativeToken', 4096);
  return {
    kind,
    registrationToken: text(value.registrationToken, 'credential.registrationToken', 4096),
    // The representative's own access token. Registration-token sides need one because the
    // representative is an ordinary account there; appservice sides do not, because `asToken`
    // authenticates as the sender_localpart directly.
    representativeToken,
    outboundGeneration: representativeToken && previous?.kind === kind
      && previous.representativeToken === representativeToken && previous.outboundGeneration
      ? previous.outboundGeneration
      : (representativeToken ? randomBytes(16).toString('hex') : null),
  };
}

/**
 * What may leave the store.
 *
 * `credential` IS ABSENT BY CONSTRUCTION, which is ADR-016 decision 8: a project side's credential
 * is settable and replaceable but never readable back. Built as an allow-list projection rather
 * than a `delete record.credential` on a copy, because a deletion has to be remembered every time a
 * field is added and a projection cannot forget — the same reason `ApprovalStore.publicRecord`
 * enumerates instead of stripping.
 *
 * `credentialKind` is safe and load-bearing: an operator needs to know a side is appservice-backed
 * to understand why its agents have no individual tokens.
 */
function publicSide(record) {
  if (!record) return null;
  return {
    id: record.id,
    label: record.label,
    serverName: record.serverName,
    apiBaseUrl: record.apiBaseUrl,
    credentialKind: record.credential ? record.credential.kind : null,
    hasCredential: Boolean(record.credential),
    /*
     * A STAGED CREDENTIAL IS A VISIBLE STATE, because the only thing standing between it and being live is an
     * action on somebody else's machine. Hidden, it would be a silent "why is my new token not working".
     *
     * NAMED WITHOUT THE WORD "credential", and not for style. The health writer's redaction guard silently
     * DROPS any key matching /credential/ — ADR-014 decision 6 found that the hard way, when a field vanished
     * from a health record and read as "never observed". A test in project-side-store.test.js enforces it, and
     * it caught this field on the way in.
     *
     * The name is also more accurate: the credential is already issued. What is pending is the INSTALL, which
     * happens on somebody else's machine.
     */
    awaitingInstall: Boolean(record.pendingCredential),
    awaitingInstallSince: record.pendingIssuedAt ?? null,
    /*
     * THE 接单员'S PUBLIC FACE, and it is public. `senderLocalpart` is the appservice's own account name
     * and `namespace` is the regex it claims — both appear verbatim in the registration file the project
     * side installs, and both are visible to anyone in a room with us. Projected because without them the
     * console cannot say WHO we sent, only that a credential exists, and "who is our order-taker on this
     * customer" is the first question the operator asks.
     *
     * The tokens are still absent, which is the point of this projection: the two fields a reader needs
     * are named, and the two that would matter if they leaked are not reachable from here at all.
     */
    senderLocalpart: record.credential?.senderLocalpart ?? null,
    /*
     * NOT A SECRET — an address, and the one a reissue must reuse. Exposed so the console can offer a reissue
     * without inventing a url, which is how a registration comes to install cleanly and hear nothing.
     */
    appserviceUrl: record.credential?.url ?? null,
    ...(record.credential?.transport ? { connectionMode: 'outbound', outboundEndpoint: record.credential.transport.url } : {}),
    namespace: record.credential?.namespace ?? null,
    /*
     * WHEN the credential was issued — which is what turns "unverified" into "waiting since Tuesday".
     * A Palpo registration is loaded once per registration id, so between issuing one and the project
     * side installing it there is a wait we do not control, and a wait with no age reads as a hang.
     * `hasCredential && accessState === 'unverified'` IS that state; this makes it legible.
     *
     * NOT `credentialIssuedAt`. The health writer's redaction guard drops any key matching /credential/
     * SILENTLY — the reason the verdict field is `accessState` and not `credentialState` — so that name
     * would have vanished from the health record and read as "never issued". A test guards the rule and
     * it caught this; the guard is why the rule is not folklore.
     */
    accessIssuedAt: record.accessIssuedAt ?? null,
    // NOT `credentialState`: the health writer's redaction guard refuses any key matching
    // /credential/, and ADR-014 decision 6 had to rename `agentsMissingCredential` to
    // `unprovisionedAgents` after discovering that a matching key is dropped SILENTLY. A field
    // named credentialState would vanish from the health record and read as "never observed".
    accessState: record.accessState,
    accessDetail: record.accessDetail || null,
    accessCheckedAt: record.accessCheckedAt,
    /*
     * The side's token allocation — ADR-016's settled question 2, where the operator chose a REAL
     * allocation over a derived slice of the deployment total. Two reasons were given and the second
     * is the operative one: a slice lets the first project side consume the whole pool, and it cannot
     * distinguish "this project exceeded its allocation" from "the deployment is out of budget" —
     * two sentences whose remedies differ.
     *
     * `null` means UNALLOCATED, not unlimited. Nothing may be minted against it.
     */
    allocatedTokens: record.allocatedTokens ?? null,
    representative: record.representative ? { ...record.representative } : null,
    /*
     * Projected as an ARRAY, sorted, because the console renders a list and an object's key order is
     * not a contract. `archived` is carried rather than filtered here: a caller that wants only live
     * projects can filter, but one that wants to show an archived project — which is the whole point
     * of archiving instead of deleting — cannot recover what this layer dropped.
     */
    projects: Object.values(record.projects ?? {})
      .map((pr) => ({ ...pr }))
      .sort((a, b) => a.id.localeCompare(b.id)),
    active: record.active,
    createdAt: record.createdAt,
    updatedAt: record.updatedAt,
  };
}

export class ProjectSideStore {
  constructor(filePath, options = {}) {
    this.filePath = path.resolve(filePath);
    this.now = typeof options.now === 'function' ? options.now : () => Date.now();
    this.auditLimit = Number.isFinite(options.auditLimit) && options.auditLimit > 0
      ? Math.floor(options.auditLimit)
      : DEFAULT_AUDIT_LIMIT;
    this.state = this._load();
    this._initializeOutboundGenerations();
  }

  // Older files predate publisher generations. Persist metadata before any
  // caller can use it; reissuing the credential would change existing authority.
  _initializeOutboundGenerations() {
    let changed = false;
    for (const record of Object.values(this.state.sides)) {
      for (const slot of ['credential', 'pendingCredential']) {
        const value = record?.[slot];
        if (!value || typeof value !== 'object') continue;
        const token = value.kind === 'appservice' ? value.asToken
          : value.kind === 'registrationToken' ? value.representativeToken : null;
        if (typeof token !== 'string' || !token.trim()) continue;
        if (value.outboundGeneration !== undefined && value.outboundGeneration !== null) {
          if (typeof value.outboundGeneration !== 'string' || !value.outboundGeneration.trim()) {
            throw new ProjectSideStoreError('persistence_failed', 'invalid stored outbound generation metadata');
          }
          continue;
        }
        value.outboundGeneration = randomBytes(16).toString('hex');
        changed = true;
      }
    }
    if (changed) this._save();
  }

  /*
   * A parse failure THROWS. It must not return an empty state.
   *
   * This is ADR-014's most expensive lesson, and it applies here more sharply than it did there.
   * `loadState` treated every read failure as "start empty", and startup then persisted that over
   * the file — which was survivable only while credentials could be re-derived from a master
   * secret. A project side's credential was issued by someone else's homeserver: nothing in this
   * software can re-mint it, so silently starting empty and saving over the file destroys a
   * credential that only the project side can replace.
   */
  _load() {
    if (!existsSync(this.filePath)) {
      return { version: STORE_VERSION, sides: {}, audit: [] };
    }
    try {
      const parsed = JSON.parse(readFileSync(this.filePath, 'utf8'));
      return {
        version: STORE_VERSION,
        sides: parsed?.sides && typeof parsed.sides === 'object' ? parsed.sides : {},
        audit: Array.isArray(parsed?.audit) ? parsed.audit.slice(-this.auditLimit) : [],
      };
    } catch (error) {
      throw new ProjectSideStoreError(
        'persistence_failed',
        `failed to load project side store: ${error.message}`,
      );
    }
  }

  _save() {
    const dir = path.dirname(this.filePath);
    mkdirSync(dir, { recursive: true, mode: 0o700 });
    const tmp = `${this.filePath}.${process.pid}.${randomBytes(6).toString('hex')}.tmp`;
    const bytes = JSON.stringify(this.state, null, 2) + '\n';
    let fd = null;
    try {
      // 0600 before any content is written and again after the rename: this file holds an
      // `as_token` that grants a whole namespace on someone else's homeserver.
      writeFileSync(tmp, bytes, { mode: 0o600 });
      chmodSync(tmp, 0o600);
      fd = openSync(tmp, 'r');
      fsyncSync(fd);
      closeSync(fd);
      fd = null;
      renameSync(tmp, this.filePath);
      chmodSync(this.filePath, 0o600);
    } catch (error) {
      if (fd !== null) {
        try { closeSync(fd); } catch {}
      }
      throw new ProjectSideStoreError(
        'persistence_failed',
        `failed to persist project side store: ${error.message}`,
      );
    }
  }

  /*
   * Audit entries record that a credential CHANGED, never what it changed to. An audit trail that
   * quotes secrets is a second copy of them with a longer retention than the record itself.
   */
  _audit(type, detail = {}) {
    this.state.audit.push({ type, at: this.now(), ...detail });
    if (this.state.audit.length > this.auditLimit) {
      this.state.audit.splice(0, this.state.audit.length - this.auditLimit);
    }
  }

  /**
   * Create or update a project side, keyed by its server name.
   *
   * The id IS the server name. Not a random id with a server-name field beside it: a generated id
   * would let two records claim one homeserver, and the invariant that makes this design coherent
   * is that a homeserver has exactly one credential and one representative.
   */
  upsertSide(input = {}) {
    const server = serverName(input.server_name ?? input.serverName);
    const existing = this.state.sides[server] || null;
    const now = this.now();

    const record = {
      id: server,
      serverName: server,
      label: input.label === undefined && existing ? existing.label : text(input.label ?? server, 'label', 255),
      apiBaseUrl: input.api_base_url === undefined && input.apiBaseUrl === undefined && existing
        ? existing.apiBaseUrl
        : apiBaseUrl(input.api_base_url ?? input.apiBaseUrl),
      // A credential omitted on update is CARRIED FORWARD, never cleared. The console can only
      // write this field, so an update that echoed back what it could not read would erase it —
      // and re-obtaining it needs an action on the project side. Use `clearCredential` to mean it.
      credential: input.credential === undefined
        ? (existing ? existing.credential : null)
        : (input.credential === null ? null : credential(input.credential, existing?.credential)),
      representative: existing ? existing.representative : null,
      active: existing ? existing.active : true,
      createdAt: existing ? existing.createdAt : now,
      updatedAt: now,
      // Access state is carried forward, then invalidated below if the credential changed.
      accessState: existing ? existing.accessState : 'unverified',
      accessDetail: existing ? existing.accessDetail : null,
      accessCheckedAt: existing ? existing.accessCheckedAt : null,
      /*
       * Carried forward, never reset by an unrelated save. An allocation is an operator's budgeting
       * decision, and an upsert that saved a label must not silently un-budget a side — which would
       * present as every mint on it being refused for no visible reason.
       */
      allocatedTokens: existing ? (existing.allocatedTokens ?? null) : null,
      /*
       * THE PROJECTS UNDER THIS SIDE, keyed by their own id. 项目方 → 项目 → 外派员工: one customer
       * has several projects, each with its own room, and the operator's directive was that the
       * 接单员 is one per SIDE while each project names its own room ("项目方一个,但每个项目可以单独
       * 指定房间"). So a project is a name and a room, not a second credential.
       *
       * Held on the side rather than in a store of their own, for one reason: a project cannot exist
       * without a side. Its room lives on that side's homeserver and is reachable only through that
       * side's credential, so a separate store would permit an orphan the domain does not have.
       *
       * The BUDGET stays on the side, per the same directive ("项目方一份总额度"). A project therefore
       * has no allocation of its own — deliberately, and it means one project can consume what
       * another was going to use. That is the operator's choice and the refusal names the side, so
       * the figure and the message agree.
       */
      projects: existing ? (existing.projects ?? {}) : {},
    };

    /*
     * A NEW credential resets the verdict to unverified. Carrying `accepted` forward across a
     * credential change would report a verdict about a value that is no longer stored — the same
     * defect as a stale membership flag, and it would hide a paste error behind an old success.
     */
    const credentialChanged = JSON.stringify(existing?.credential ?? null)
      !== JSON.stringify(record.credential ?? null);
    if (credentialChanged) {
      record.accessState = 'unverified';
      record.accessDetail = null;
      record.accessCheckedAt = null;
    }

    this.state.sides[server] = record;
    this._audit(existing ? 'side_updated' : 'side_created', {
      side: server,
      credentialKind: record.credential ? record.credential.kind : null,
      credentialChanged,
    });
    this._save();
    return publicSide(record);
  }

  /**
   * Set or clear the side's token allocation.
   *
   * UNALLOCATED IS NOT UNLIMITED, and choosing that default while it is still free is the point. A
   * side with no allocation can have nothing minted against it — so a project side that has been
   * configured and not yet budgeted refuses work rather than drawing on whatever happens to be left.
   * The alternative default, treating absent as unbounded, is the shape that makes an alarm arrive
   * after the tokens are gone.
   *
   * Zero is a legitimate value and is NOT the same as null: it says "this side is allocated nothing",
   * which an operator may want as a way to close a side to new work without deactivating it.
   */
  setAllocation(idValue, tokens) {
    const id = serverName(idValue);
    const record = this.state.sides[id];
    if (!record) return null;
    if (tokens === null || tokens === undefined) {
      record.allocatedTokens = null;
    } else {
      const value = Number(tokens);
      if (!Number.isInteger(value) || value < 0) {
        throw new ProjectSideStoreError('bad_request', 'allocated_tokens must be a non-negative integer or null');
      }
      record.allocatedTokens = value;
    }
    record.updatedAt = this.now();
    this._audit('allocation_set', { side: id, allocatedTokens: record.allocatedTokens });
    this._save();
    return publicSide(record);
  }

  /** Replace a side's credential without touching anything else. Write-only by design. */
  /**
   * @param {object} [options]
   * @param {boolean} [options.stage] Keep the working credential live and hold this one until it is proven.
   *
   * WHY STAGING EXISTS. Issuing replaced the live credential immediately, which broke Hagency's own outbound
   * auth the instant an operator clicked "generate" — before they had any chance to install the new file.
   * That happened for real: the fleet held one token, the homeserver still had the previous one, `verify`
   * correctly said `rejected`, and the operator was handed a repair job for a state Hagency had created.
   *
   * The chore was never the file copy. It was that a working thing was broken first and the operator was
   * asked to catch up. Staged, an unwanted click costs nothing: the old credential keeps working, the new one
   * waits, and it is promoted only when a `verify` proves the homeserver accepts it.
   */
  setCredential(idValue, value, { stage = false } = {}) {
    const id = serverName(idValue);
    const record = this.state.sides[id];
    if (!record) return null;

    /*
     * `record.credential &&` IS UNREACHABLE FROM THE CURRENT CALLER and stays anyway. The registration
     * endpoint only passes `stage: true` when a live credential exists, so removing this condition changes
     * nothing today — recorded honestly as an equivalent mutant rather than claimed as tested.
     *
     * It stays because this is a public store method and the guard is what it protects against: staging a
     * FIRST credential would leave the side holding one it will not use, unable to act while `hasCredential`
     * says otherwise. A caller that gets that wrong should not be able to produce that state.
     */
    if (stage && record.credential && value !== null) {
      record.pendingCredential = credential(value);
      record.pendingIssuedAt = this.now();
      /*
       * `accessState` IS NOT TOUCHED. It describes the credential that is live and still working, and
       * resetting it would report a healthy side as unverified because somebody generated a spare.
       */
      record.updatedAt = this.now();
      this._audit('credential_staged', { side: id, credentialKind: record.pendingCredential.kind });
      this._save();
      return publicSide(record);
    }

    record.credential = value === null ? null : credential(value, record.credential);
    record.pendingCredential = null;
    record.pendingIssuedAt = null;
    record.accessState = 'unverified';
    record.accessDetail = null;
    record.accessCheckedAt = null;
    /*
     * Stamped here, and CLEARED when the credential is withdrawn. A stale issue date on a side with no
     * credential would read as "waiting" for something nobody is going to install.
     *
     * Resetting `accessState` to unverified on every credential change is what makes this figure mean
     * something: a Palpo registration only loads on restart, so a freshly issued credential is
     * unverifiable until the project side acts, and the pair (hasCredential, unverified) is precisely
     * "issued, waiting on them" — a wait that is theirs, not a failure that is ours.
     */
    record.accessIssuedAt = record.credential ? this.now() : null;
    record.updatedAt = this.now();
    this._audit('credential_set', { side: id, credentialKind: record.credential?.kind ?? null });
    this._save();
    return publicSide(record);
  }

  /**
   * The credential, for the one caller that needs it: whoever talks to that homeserver.
   *
   * Named so it cannot be reached by accident and so a reviewer grepping for it finds every use.
   * Deliberately NOT part of `publicSide`, and therefore never returned by an API handler that
   * projects records.
   */
  credentialFor(idValue) {
    const id = serverName(idValue);
    const record = this.state.sides[id];
    if (!record || !record.credential) return null;
    return { ...record.credential };
  }

  /** The credential waiting to be installed, if one was staged. Same access rules as the live one. */
  pendingCredentialFor(idValue) {
    const id = serverName(idValue);
    const record = this.state.sides[id];
    if (!record || !record.pendingCredential) return null;
    return { ...record.pendingCredential };
  }

  /**
   * Make the staged credential the live one — called only after something PROVED the homeserver accepts it.
   *
   * The old credential is dropped here rather than kept as a fallback. Two live credentials for one side is
   * two things that can each be revoked without the other noticing, and the homeserver only honours one
   * registration per id anyway: keeping the loser would be keeping a token that authorises nothing.
   */
  promotePendingCredential(idValue) {
    const id = serverName(idValue);
    const record = this.state.sides[id];
    if (!record || !record.pendingCredential) return null;
    record.credential = record.pendingCredential;
    record.pendingCredential = null;
    record.pendingIssuedAt = null;
    record.accessIssuedAt = this.now();
    record.updatedAt = this.now();
    this._audit('credential_promoted', { side: id, credentialKind: record.credential.kind });
    this._save();
    return publicSide(record);
  }

  /**
   * Record what the homeserver said about our credential.
   *
   * A verdict with the time it was taken, because "accepted" with no age is indistinguishable from
   * "accepted once, months ago, by a bridge that has since stopped running" — the same argument
   * that put `membershipCheckedAt` beside `agentJoined`.
   */
  observeAccess(idValue, input = {}) {
    const id = serverName(idValue);
    const record = this.state.sides[id];
    if (!record) return null;
    const state = text(input.state, 'state', 32);
    if (!ACCESS_STATES.includes(state)) {
      throw new ProjectSideStoreError(
        'bad_request',
        `state must be one of ${ACCESS_STATES.join(', ')}`,
      );
    }
    record.accessState = state;
    record.accessDetail = input.detail === null || input.detail === undefined
      ? null
      : text(input.detail, 'detail', 1024);
    record.accessCheckedAt = this.now();
    record.updatedAt = record.accessCheckedAt;
    this._audit('access_observed', { side: id, state });
    this._save();
    return publicSide(record);
  }

  /**
   * Record the representative's identity on this side (ADR-016 decision 3).
   *
   * The MXID is SUPPLIED rather than composed. ADR-014 decision 5 is the reason: composing
   * `@localpart:server` is wrong the moment the account lives somewhere we do not control, and the
   * authoritative answer comes from `/whoami` on that homeserver. This store takes the discovered
   * value and refuses to guess one.
   */
  setRepresentative(idValue, input = {}) {
    const id = serverName(idValue);
    const record = this.state.sides[id];
    if (!record) return null;
    const mxid = text(input.mxid, 'mxid', 255);
    if (!mxid.startsWith('@') || !mxid.includes(':')) {
      throw new ProjectSideStoreError('bad_request', 'mxid must be a full Matrix MXID');
    }
    const host = mxid.slice(mxid.indexOf(':') + 1).toLowerCase();
    /*
     * The MXID's server must BE this side's server. A representative whose identity lives elsewhere
     * is the federation assumption smuggled back in (ADR-016 decision 2 assumes no federation), and
     * it would silently produce a side that cannot act locally on the server it claims to be
     * registered with.
     */
    if (host !== record.serverName) {
      throw new ProjectSideStoreError(
        'bad_request',
        `representative mxid must live on ${record.serverName}, got ${host}`,
      );
    }
    record.representative = {
      mxid,
      localpart: mxid.slice(1, mxid.indexOf(':')),
      observedAt: this.now(),
    };
    record.updatedAt = this.now();
    this._audit('representative_set', { side: id, mxid });
    this._save();
    return publicSide(record);
  }

  getSide(idValue) {
    let id;
    try {
      id = serverName(idValue);
    } catch {
      return null;
    }
    return publicSide(this.state.sides[id] || null);
  }

  listSides(filters = {}) {
    const all = Object.values(this.state.sides).map(publicSide);
    if (filters.activeOnly) return all.filter((s) => s.active);
    return all;
  }

  /**
   * Add or update a project under a side.
   *
   * The id is derived from the NAME rather than supplied, so two projects cannot claim one id and the
   * operator never has to invent one. The room is optional at creation: a project is often named
   * before its room exists, and refusing to record it until then would push the operator back to a
   * notebook — which is where project names currently live, since nothing in this system stores one
   * (`groupForRoom(projectRoomId) || meta.group || projectRoomId` degrades to a raw room id).
   *
   * A ROOM MAY BELONG TO ONE PROJECT ONLY, and the check is inside the side because that is the scope
   * where it can be made: two sides may legitimately hold the same-looking local part, since a room id
   * is only unique with its server. Rejecting a duplicate rather than reassigning it, because a room
   * that silently moved between projects would move its engagements' attribution with it.
   */
  upsertProject(sideIdValue, input = {}) {
    const sideId = serverName(sideIdValue);
    const record = this.state.sides[sideId];
    if (!record) return null;
    if (!record.projects) record.projects = {};

    const name = text(input.name, 'project.name', 255);
    /*
     * THE SLUG KEEPS LETTERS IN ANY SCRIPT, and the first version did not. `[^a-z0-9._-]` turned
     * "BigLittle 重构" into "biglittle" and a wholly Chinese name into the EMPTY STRING, which then
     * failed as "project.name must contain a usable character" — about a name that plainly had several.
     * Caught by running it rather than by reading it: this product's operator writes Chinese, and an
     * ASCII slug quietly refuses their vocabulary.
     *
     * `\p{L}\p{N}` with the `u` flag keeps CJK, Cyrillic, accented Latin — anything a name is made of.
     * The id lands in a URL path, where `encodeURIComponent` handles it, and in the store's keys, which
     * are strings. Neither cares which script.
     */
    /*
     * `room` IS ACCEPTED TOO, and it was not. The field is `room_id`/`roomId`, and a caller sending `room` —
     * which is what the projects listing calls it when it reads back — got `ok: true` and a project with no
     * room. Found by walking the flow, and it is the fourth write found this way that reported success while
     * dropping what it was sent.
     *
     * Added rather than documented, because the spelling that a reader of the OUTPUT would reach for is the
     * spelling they will send.
     */
    const id = String(input.id ?? name).trim().toLowerCase()
      .replace(/[^\p{L}\p{N}._-]+/gu, '-')
      .replace(/^-+|-+$/g, '');
    if (!id) throw new ProjectSideStoreError('bad_request', 'project.name must contain a letter or digit');

    const existing = record.projects[id] ?? null;
    /*
     * A SPELLING WE DO NOT ACCEPT IS REFUSED, NOT DROPPED — and I am the fifth instance the note above
     * counts. Adding `room` fixed one spelling; `project_room_id` then did the same thing, silently, and
     * that spelling is not a wild guess: it is what `upsertBinding` takes and what the engagement API
     * carries as `projectRoomId`. So the same concept has one name per route and one of them lies.
     *
     * ALIASES DO NOT CONVERGE. Each one added makes the next miss more surprising, and the failure is
     * `ok: true` with a project that has no room — discoverable only by reading the record back, which is
     * how all five were found. Refusing names the right spelling at the moment of the mistake instead.
     *
     * NARROW ON PURPOSE: only keys that clearly MEAN a room. An unrelated extra field is still ignored,
     * because rejecting every unknown key would break callers that send more than this function reads.
     */
    for (const key of Object.keys(input)) {
      const looksLikeRoom = /room/i.test(key);
      if (looksLikeRoom && !['room_id', 'roomId', 'room'].includes(key)) {
        throw new ProjectSideStoreError('bad_request',
          `project room must be sent as room_id, roomId or room — "${key}" would be ignored and the project `
          + 'would be created with no room');
      }
    }
    const roomRaw = input.room_id ?? input.roomId ?? input.room;
    const roomId = roomRaw === undefined
      ? (existing ? existing.roomId : null)
      : (roomRaw === null || roomRaw === '' ? null : text(roomRaw, 'project.roomId', 255));

    if (roomId) {
      if (!roomId.startsWith('!')) {
        throw new ProjectSideStoreError('bad_request', 'project.roomId must be a room id starting with !');
      }
      /*
       * The room's server must BE this side. A room on another homeserver cannot be reached with this
       * side's credential, so recording it here would produce a project the 接单员 can never enter —
       * and the budget attribution, which reads the server out of the room id, would charge the wrong
       * customer.
       */
      const at = roomId.indexOf(':');
      const host = at > 0 ? roomId.slice(at + 1).toLowerCase() : '';
      if (host !== sideId) {
        throw new ProjectSideStoreError('bad_request',
          `project.roomId is on ${host || 'no server'}, but this project side is ${sideId}`);
      }
      const clash = Object.values(record.projects).find((pr) => pr.roomId === roomId && pr.id !== id);
      if (clash) {
        throw new ProjectSideStoreError('conflict',
          `room ${roomId} already belongs to project ${clash.id}`);
      }
    }

    const now = this.now();
    record.projects[id] = {
      id,
      name,
      roomId,
      note: input.note === undefined ? (existing ? existing.note : null) : (input.note === null ? null : text(input.note, 'project.note', 1024)),
      archived: existing ? existing.archived === true : false,
      archivedAt: existing ? (existing.archivedAt ?? null) : null,
      createdAt: existing ? existing.createdAt : now,
      updatedAt: now,
    };
    record.updatedAt = now;
    this._audit(existing ? 'project_updated' : 'project_added', { side: sideId, project: id, roomId });
    this._save();
    return { ...record.projects[id] };
  }

  /**
   * ARCHIVE a project. There is no delete, on the operator's instruction — 「项目方暂时不可以删除,可以
   * archive 掉」 — and the reason is the one they gave earlier about agents: 「删除会有合规问题」. A
   * project's room carries the engagements that were served through it, so forgetting the project
   * would leave that history attributable to nothing.
   *
   * Reversible, because an archive that cannot be undone is a delete with a gentler name.
   */
  setProjectArchived(sideIdValue, projectIdValue, archived) {
    const sideId = serverName(sideIdValue);
    const record = this.state.sides[sideId];
    if (!record?.projects) return null;
    const id = String(projectIdValue ?? '').trim().toLowerCase();
    const pr = record.projects[id];
    if (!pr) return null;
    const now = this.now();
    pr.archived = archived === true;
    pr.archivedAt = pr.archived ? now : null;
    pr.updatedAt = now;
    record.updatedAt = now;
    this._audit(pr.archived ? 'project_archived' : 'project_unarchived', { side: sideId, project: id });
    this._save();
    return { ...pr };
  }

  /** The project a room belongs to, or null. How a room id becomes a NAME anywhere in the product. */
  projectForRoom(roomIdValue) {
    const roomId = String(roomIdValue ?? '').trim();
    const at = roomId.indexOf(':');
    if (at <= 0) return null;
    const sideId = roomId.slice(at + 1).toLowerCase();
    const record = this.state.sides[sideId];
    if (!record?.projects) return null;
    const pr = Object.values(record.projects).find((x) => x.roomId === roomId);
    return pr ? { sideId, ...pr } : null;
  }

  /**
   * ARCHIVE a side — deactivate it without forgetting it.
   *
   * THIS IS THE ONLY WAY OUT, on the operator's instruction: 「项目方暂时不可以删除,可以 archive 掉」.
   * `removeSide` below still exists because the store cannot be the thing that decides product policy,
   * but no console surface offers it and the reason is the operator's own compliance rule about agents,
   * which applies with more force here: a side is the attribution for every engagement, binding and
   * token ever spent on that customer.
   *
   * It is also the reversible half of decision 7's cascade. Archiving stops a side being used for new
   * work while leaving the credential in place, which is what makes an ordered cascade possible at
   * all: close it to new engagements, then do the work that still needs the credential — leaving
   * rooms, revoking tokens — and only then, if policy ever allows it, forget it.
   */
  deactivateSide(idValue) {
    const id = serverName(idValue);
    const record = this.state.sides[id];
    if (!record) return null;
    record.active = false;
    record.updatedAt = this.now();
    this._audit('side_deactivated', { side: id });
    this._save();
    return publicSide(record);
  }

  reactivateSide(idValue) {
    const id = serverName(idValue);
    const record = this.state.sides[id];
    if (!record) return null;
    record.active = true;
    record.updatedAt = this.now();
    this._audit('side_reactivated', { side: id });
    this._save();
    return publicSide(record);
  }

  /**
   * Forget a side. THE LAST STEP of decision 7's cascade, never the first.
   *
   * This store cannot verify that the earlier steps ran — it does not know about engagements or
   * bindings (see the file header). What it can do is refuse to be the accidental first step: a
   * side that is still active has not been closed to new work, so removing it here would drop the
   * credential while engagements were still live. Deactivate first, or pass `force`.
   *
   * The refuse-by-default-with-explicit-force shape is taken from `DELETE /api/agents/:name`, which
   * already answers "unregister is disabled; agent marked inactive. Use ?force=true" — a pattern
   * the codebase had before this ADR proposed it.
   */
  removeSide(idValue, options = {}) {
    const id = serverName(idValue);
    const record = this.state.sides[id];
    if (!record) return null;
    if (record.active && options.force !== true) {
      throw new ProjectSideStoreError(
        'side_active',
        `project side ${id} is still active — deactivate it (and end its engagements) before removing, or pass force`,
      );
    }
    const beforeAudit = this.state.audit.slice();
    delete this.state.sides[id];
    this._audit('side_removed', { side: id, forced: options.force === true,
      ...(options.cleanup ? { cleanup: structuredClone(options.cleanup) } : {}) });
    try { this._save(); } catch (error) {
      this.state.sides[id] = record;
      this.state.audit = beforeAudit;
      throw error;
    }
    return publicSide(record);
  }

  listAudit() {
    return this.state.audit.slice();
  }
}

export function createProjectSideStore(filePath, options = {}) {
  return new ProjectSideStore(filePath, options);
}
