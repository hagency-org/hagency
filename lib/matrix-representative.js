/*
 * 代表 — Hagency's representative on a project side (ADR-016 decision 3).
 *
 * THE SPLIT THIS FILE EXISTS TO MAKE. Until now the representative and the working agent were one
 * thing: the agent itself had to be invited and had to join, so an agent identity had to exist
 * before any project was known — and its MXID was composed on Hagency's own server, which under
 * ADR-016 decision 2 (servers do not federate) is unusable for a project hosted elsewhere. That is
 * the circular dependency an operator named. The representative is what becomes known first.
 *
 * | | 代表 representative | agent instance |
 * |---|---|---|
 * | purpose | be registered on the project's server; receive requests | do the work |
 * | lifetime | as long as the project side is configured | one engagement |
 * | how many | one per project side | one per engagement |
 * | draws budget | no | yes |
 *
 * WHY THIS IS NOT IN bridge-matrix.js. Every Matrix call here takes its `baseUrl` as an argument.
 * That is ADR-016's third first-pass shape stated as code: the bridge's 44 `HOMESERVER` references
 * are a module constant, and a representative for a foreign homeserver cannot be built out of them.
 * Keeping it separate also makes it testable against an injected `fetch` — the bridge's own tests
 * assert against its SOURCE TEXT because the file cannot be exercised, and that is not a standard
 * worth extending.
 *
 * WHAT THE REPRESENTATIVE IS NOT. It is not Hagency's home bot. The home bot lives on the
 * contributor's own server, talks to the operator, holds the single crypto store and is the E2EE
 * sender; `MATRIX_HOMESERVER` remains its server. A representative is plaintext-only and does
 * intake — which is sound because ADR-016 settled that intake rooms are plaintext, and Hagency's
 * agents have never been E2EE participants anyway.
 */

import { createHash, randomBytes } from 'crypto';

/** Default localpart for the representative account. Lowercase: the spec requires it of localparts,
 * and `bridge-matrix.js` already lowercases agent localparts for the same reason. */
export const DEFAULT_REPRESENTATIVE_LOCALPART = 'hagency';

export class RepresentativeError extends Error {
  constructor(code, message) {
    super(message);
    this.name = 'RepresentativeError';
    this.code = code;
  }
}

/**
 * Did the homeserver REJECT us, or fail to answer?
 *
 * Same rule as `bridge-matrix.js`'s `isMatrixAuthFailure`, and restated here rather than imported
 * because that module cannot be loaded without its environment. Only 401 and 403 are a verdict on
 * the credential. Everything else — 5xx, a timeout, DNS, a rate limit — says nothing about it, and
 * reporting an outage as `rejected` sends an operator to ask a project side for a new credential
 * when the one they hold is fine. On this path that means a human doing account work on someone
 * else's homeserver, so the cost of the wrong answer is somebody else's afternoon.
 *
 * Unknown shapes answer `unreachable`, so an unrecognised failure is transient rather than a
 * verdict.
 */
export function classifyMatrixFailure(error) {
  const status = error?.status;
  if (status === 401 || status === 403) return 'rejected';
  /*
   * A DEFINITE STATE ON A WORKING SERVER, which neither of the other two answers can express. Found by walking
   * the flow: a side whose representative localpart was already taken reported `unreachable` — an outage
   * verdict on a homeserver that was answering perfectly. The errcode was already being surfaced by the
   * caller; nothing was reading it to classify.
   *
   * `blocked` rather than `rejected` because the credential may be perfectly good: what is in the way is an
   * account that already exists, and the fix is to claim it or choose another localpart. Calling it rejected
   * would send an operator to ask a project side for a new token they do not need.
   */
  if (error?.errcode === 'M_USER_IN_USE') return 'blocked';
  return 'unreachable';
}

function joinUrl(baseUrl, pathSuffix) {
  return `${String(baseUrl).replace(/\/+$/, '')}${pathSuffix}`;
}

/**
 * Parse a Matrix response body defensively, AFTER the status is in hand.
 *
 * `await res.json()` on a 401 whose body is empty or HTML — a proxy, a non-conforming homeserver —
 * throws a SyntaxError carrying no `.status`, which `classifyMatrixFailure` would then read as
 * `unreachable`: an outage verdict on a genuinely dead token. The status is the reliable signal; the
 * body only enriches the message. This is the same defence `getMatrixAccessTokenSession` documents.
 */
async function readBody(res) {
  try {
    return await res.json();
  } catch {
    return null;
  }
}

function matrixError(res, data, what) {
  const error = new RepresentativeError(
    'matrix_error',
    `${what} failed with HTTP ${res.status}: ${data?.errcode || data?.error || 'unknown error'}`,
  );
  error.status = res.status;
  error.errcode = typeof data?.errcode === 'string' ? data.errcode : undefined;
  return error;
}

/**
 * Ask a homeserver who a credential belongs to.
 *
 * `asUserId` masquerades, which is how an appservice acts as a user in its namespace: the query
 * parameter names the user and the `as_token` authorises it. For a plain access token it is omitted.
 *
 * THE MXID COMES BACK FROM HERE AND IS NEVER COMPOSED. ADR-014 decision 5: composing
 * `@localpart:server` is simply wrong for an account on a server we do not administer, and the
 * repository has a live example of the damage — the invite poll's owner-derivation filter matches a
 * CONSTRUCTED state_key, so for an agent on any other homeserver it never matches, no ownership
 * binding is written, and every later approval is denied `owner_binding_missing`. A silent
 * authorization failure produced by a string built from an assumption.
 */
export async function whoami({ baseUrl, token, asUserId = null, fetchImpl = fetch }) {
  if (!baseUrl) throw new RepresentativeError('bad_request', 'baseUrl is required');
  if (!token) throw new RepresentativeError('bad_request', 'token is required');
  const url = new URL(joinUrl(baseUrl, '/_matrix/client/v3/account/whoami'));
  if (asUserId) setMasqueradeUserParam(url, asUserId);
  const res = await fetchImpl(url.toString(), {
    headers: { Authorization: `Bearer ${token}` },
  });
  const data = await readBody(res);
  if (!res.ok) throw matrixError(res, data, 'whoami');
  const userId = typeof data?.user_id === 'string' ? data.user_id.trim() : '';
  if (!userId) {
    throw new RepresentativeError('matrix_error', 'whoami did not return a user_id');
  }
  return { userId, deviceId: typeof data?.device_id === 'string' ? data.device_id.trim() : null };
}

/**
 * Register the representative's account using a registration token the project side issued.
 *
 * The password is RANDOM and then DISCARDED. Random rather than derived is ADR-014 decision 3's
 * entire holding: a derived password cannot be rotated (every account's changes at once) and cannot
 * be revoked (it can always be re-derived), so `.env` compromise became permanent control of every
 * identity.
 *
 * Discarded rather than stored is a further choice worth stating, because it costs something. Keeping
 * it would give a login fallback if the access token ever dies. Not keeping it means a dead token
 * makes the project side unreachable until a human acts — which is exactly the state ADR-014
 * decision 6 says a dead credential must be, rather than something a retry loop papers over. The
 * trade is: one fewer long-lived secret at rest, against a recovery path that must go through a
 * person. If that turns out to be the wrong trade, adding the password is additive; removing it
 * later would not be.
 */
export async function registerRepresentative({
  baseUrl,
  registrationToken,
  localpart = DEFAULT_REPRESENTATIVE_LOCALPART,
  fetchImpl = fetch,
}) {
  if (!baseUrl) throw new RepresentativeError('bad_request', 'baseUrl is required');
  const username = String(localpart || '').toLowerCase();
  if (!username) throw new RepresentativeError('bad_request', 'localpart is required');
  const password = randomBytes(32).toString('base64url');
  const url = joinUrl(baseUrl, '/_matrix/client/v3/register');

  // Step 1 — probe for the UIA session and the flows this server offers. Some servers complete
  // registration outright and return a token here.
  const probeRes = await fetchImpl(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ username, password }),
  });
  const probe = await readBody(probeRes);
  if (probe?.access_token) {
    return { accessToken: probe.access_token, userId: probe.user_id || null };
  }
  /*
   * A 4xx that is not a UIA challenge is a real refusal and must be raised, not retried. M_USER_IN_USE
   * is the one an operator will actually meet: the localpart is taken, which happens when a
   * representative was registered before and its token was then lost. That needs a person to pick a
   * different localpart or reset the account — precisely the human-visible state above.
   */
  if (!probe?.session) {
    throw matrixError(probeRes, probe, 'representative registration probe');
  }

  // Step 2 — complete UIA with the project side's registration token. No `m.login.dummy` fallback:
  // this path exists because the project side issued us a credential, and silently registering
  // through open registration instead would create an account with a provenance nobody agreed to.
  const authRes = await fetchImpl(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      username,
      password,
      auth: { type: 'm.login.registration_token', token: registrationToken, session: probe.session },
    }),
  });
  const auth = await readBody(authRes);
  if (!authRes.ok || !auth?.access_token) {
    throw matrixError(authRes, auth, 'representative registration');
  }
  return { accessToken: auth.access_token, userId: auth.user_id || null };
}

/**
 * Bring a project side's representative into a known state, and say what that state is.
 *
 * Returns `{ accessState, detail, mxid, credentialPatch }`. It NEVER throws for a Matrix-side
 * problem: an unreachable homeserver and a rejected credential are both outcomes an operator needs
 * recorded, and a throw would make the caller choose between crashing a startup sweep and swallowing
 * the reason. It throws only for arguments that cannot be acted on at all.
 *
 * `credentialPatch` carries a newly minted representative token back to the caller for storage.
 * Returned rather than written here because this module holds no store: writing would give a network
 * helper the authority to mutate credentials, and the store's own audit trail should record who
 * changed what.
 */
export async function ensureRepresentative({
  side,
  credential,
  localpart = DEFAULT_REPRESENTATIVE_LOCALPART,
  fetchImpl = fetch,
}) {
  if (!side?.apiBaseUrl || !side?.serverName) {
    throw new RepresentativeError('bad_request', 'side must carry apiBaseUrl and serverName');
  }
  if (!credential) {
    return {
      accessState: 'unverified',
      detail: 'no credential configured for this project side',
      mxid: null,
      credentialPatch: null,
    };
  }
  const baseUrl = side.apiBaseUrl;

  if (credential.kind === 'appservice') {
    /*
     * An appservice representative is not registered — it already exists by virtue of the
     * registration the project side installed, as `sender_localpart`. Validation is a masqueraded
     * whoami: if the `as_token` is good and the namespace claim holds, the server answers with the
     * sender's own MXID.
     *
     * This is also where the agent-side payoff shows: nothing per-agent is created or stored here,
     * because on an appservice side agents have no credential at all.
     *
     * WHY MASQUERADED RATHER THAN A BARE `as_token` WHOAMI. A bare call proves only that the
     * homeserver knows the token. A masqueraded one additionally proves the namespace claim
     * functions, and it exercises the exact call shape every later agent operation uses — so a side
     * that validates here cannot fail on its first real request for a reason validation never
     * touched.
     *
     * Measured against the Palpo 0.4.0 build this deployment runs (a throwaway registration,
     * restarted, then removed): masquerading outside the claimed namespace is refused
     * `403 M_FORBIDDEN`, so the `as_token` is genuinely scoped rather than a superuser credential.
     * That is the property being checked.
     *
     * A CORRECTION KEPT DELIBERATELY. An earlier version of this comment claimed the masquerade was
     * load-bearing for a different reason: that nothing creates the `sender_localpart` account and a
     * bare call therefore fails until a masqueraded one bootstraps it. That was read out of Palpo's
     * source and symbol table, written down as "verified", and **did not reproduce** — on this build
     * a bare `as_token` whoami answers 200 with the sender's own MXID. The reason above is the one
     * that survived being tested. Noted here because the mistake is worth more than the fact: code
     * paths existing is not the same as code paths behaving, and only one of those had been checked.
     */
    const asUserId = side.representative?.mxid || `@${String(credential.senderLocalpart)}:${side.serverName}`;
    try {
      const { userId } = await whoami({ baseUrl, token: credential.asToken, asUserId, fetchImpl });
      return { accessState: 'accepted', detail: null, mxid: userId, credentialPatch: null };
    } catch (error) {
      return {
        accessState: classifyMatrixFailure(error),
        detail: error.message,
        mxid: null,
        credentialPatch: null,
      };
    }
  }

  if (credential.kind !== 'registrationToken') {
    throw new RepresentativeError('bad_request', `unsupported credential kind: ${credential.kind}`);
  }

  // An existing representative token is VALIDATED, never replaced on a failure.
  if (credential.representativeToken) {
    try {
      const { userId } = await whoami({ baseUrl, token: credential.representativeToken, fetchImpl });
      return { accessState: 'accepted', detail: null, mxid: userId, credentialPatch: null };
    } catch (error) {
      /*
       * A 401 must NOT trigger re-registration. Two reasons, and the second is the one that bites:
       * the localpart is already taken so registration would fail anyway, and attempting it would
       * turn a clear "your token was revoked" into a confusing M_USER_IN_USE from a different call.
       * ADR-014 decision 6 wants this surfaced, not healed.
       */
      return {
        accessState: classifyMatrixFailure(error),
        detail: error.message,
        mxid: null,
        credentialPatch: null,
      };
    }
  }

  // No token yet: register, using the project side's registration token.
  if (!credential.registrationToken) {
    return {
      accessState: 'unverified',
      detail: 'no representative token and no registration token to obtain one with',
      mxid: null,
      credentialPatch: null,
    };
  }
  try {
    const { accessToken, userId } = await registerRepresentative({
      baseUrl, registrationToken: credential.registrationToken, localpart, fetchImpl,
    });
    /*
     * Confirm the identity by asking, rather than trusting the register response alone. The register
     * reply usually carries `user_id`, but not every server includes it, and an MXID this side will
     * store must come from an answer rather than a default.
     */
    let mxid = userId || null;
    if (!mxid) {
      const confirmed = await whoami({ baseUrl, token: accessToken, fetchImpl });
      mxid = confirmed.userId;
    }
    return {
      accessState: 'accepted',
      detail: null,
      mxid,
      credentialPatch: { representativeToken: accessToken },
    };
  } catch (error) {
    return {
      accessState: classifyMatrixFailure(error),
      detail: error.message,
      mxid: null,
      credentialPatch: null,
    };
  }
}

/**
 * Mint an agent identity on a project side.
 *
 * ADR-016 decision 4: an agent instance is created when an engagement is accepted, from a durable
 * resource declaration, rather than by hand ahead of demand. This is the identity half — the part that
 * could not exist before a project side did, which is the circular dependency the operator identified.
 *
 * THE TWO KINDS DIVERGE COMPLETELY HERE, and that is the payoff of making appservice mandatory:
 *
 *   - `appservice`: NOTHING IS CREATED AND NOTHING IS STORED. The account exists by virtue of the
 *     namespace the project side installed, so minting is a claim rather than an act — Hagency will
 *     act as it later by masquerading with the side's one `as_token`. The identity is confirmed the
 *     same way the representative's is: a masqueraded whoami, which is also what makes the homeserver
 *     materialise the account.
 *   - `registrationToken`: a real account is registered and a real per-agent token comes back, which
 *     the caller must store or lose.
 *
 * The MXID is taken from the homeserver's answer in both cases, never composed (ADR-014 decision 5).
 * The localpart is what we ASK for; what we get is what the server says.
 *
 * Returns `{ minted, mxid, accessToken, kind }` and never throws for a Matrix-side outcome — the
 * caller is a provisioning path that must record a reason per agent rather than abort a sweep.
 */
export async function mintAgentIdentity({
  side,
  credential,
  localpart,
  fetchImpl = fetch,
}) {
  if (!side?.apiBaseUrl || !side?.serverName) {
    throw new RepresentativeError('bad_request', 'side must carry apiBaseUrl and serverName');
  }
  if (!credential) {
    throw new RepresentativeError('bad_request', 'a project side credential is required to mint an identity');
  }
  const wanted = String(localpart || '').trim().toLowerCase();
  if (!wanted) throw new RepresentativeError('bad_request', 'localpart is required');
  const baseUrl = side.apiBaseUrl;

  if (credential.kind === 'appservice') {
    /*
     * The namespace is checked BEFORE the call, because the homeserver's refusal for an out-of-namespace
     * masquerade is a 403 that looks identical to a bad `as_token` — and those send an operator to two
     * different places. `namespace` is the regex the registration claimed.
     */
    const claimed = `@${wanted}:${side.serverName}`;
    let pattern;
    try {
      pattern = new RegExp(`^${credential.namespace}$`);
    } catch {
      return {
        minted: false, kind: 'appservice', mxid: null, accessToken: null,
        reason: `this side's claimed namespace is not a usable regex: ${credential.namespace}`,
      };
    }
    if (!pattern.test(claimed)) {
      return {
        minted: false, kind: 'appservice', mxid: null, accessToken: null,
        reason: `${claimed} is outside the namespace this side claimed (${credential.namespace}), `
          + 'so the homeserver would refuse it — rename the agent or widen the registration',
      };
    }
    try {
      const { userId } = await whoami({ baseUrl, token: credential.asToken, asUserId: claimed, fetchImpl });
      /*
       * `accessToken: null` is the point, not an omission. An appservice agent has no per-agent
       * credential — Hagency acts as it with the side's `as_token` — which is why ADR-014 decision 4's
       * per-agent `{ homeserver, accessToken }` is unrepresentable for such a side.
       */
      return { minted: true, kind: 'appservice', mxid: userId, accessToken: null, reason: null };
    } catch (error) {
      return {
        minted: false, kind: 'appservice', mxid: null, accessToken: null,
        state: classifyMatrixFailure(error), reason: error.message,
      };
    }
  }

  if (credential.kind !== 'registrationToken') {
    throw new RepresentativeError('bad_request', `unsupported credential kind: ${credential.kind}`);
  }
  if (!credential.registrationToken) {
    return {
      minted: false, kind: 'registrationToken', mxid: null, accessToken: null,
      reason: 'this side has no registration token, so no agent account can be created on it',
    };
  }
  try {
    const { accessToken, userId } = await registerRepresentative({
      baseUrl, registrationToken: credential.registrationToken, localpart: wanted, fetchImpl,
    });
    let mxid = userId || null;
    if (!mxid) {
      const confirmed = await whoami({ baseUrl, token: accessToken, fetchImpl });
      mxid = confirmed.userId;
    }
    return { minted: true, kind: 'registrationToken', mxid, accessToken, reason: null };
  } catch (error) {
    /*
     * M_USER_IN_USE is the one a caller must be able to act on: the localpart is taken, which happens
     * when an agent of that name was minted before and its token was lost. Surfaced by errcode rather
     * than by matching the message, which the next reword would break.
     */
    return {
      minted: false, kind: 'registrationToken', mxid: null, accessToken: null,
      state: classifyMatrixFailure(error),
      errcode: error.errcode ?? null,
      reason: error.message,
    };
  }
}

/**
 * Create a room on a project side, as the representative.
 *
 * The capability ADR-016 needed and this module did not have. It is what lets an approval room live
 * where the DECIDER lives — the operator settled that an execution approval is the borrower's
 * (「答借用方，当然是借用方」), and the borrower is on the project side's homeserver, which the bot has
 * no account on.
 *
 * ENCRYPTION IS A REQUIRED ARGUMENT, AND `true` IS REFUSED. That is not a policy choice dressed as an
 * API; it is a capability fact. The representative is plaintext-only by construction — the single
 * crypto store belongs to the home bot on the contributor's server — so a room it created with
 * `m.room.encryption` would be a room it cannot read. Creating one anyway would produce an approval
 * channel that looks correct and delivers nothing, which is the failure mode this project has now hit
 * twice. Required rather than defaulted so the caller states it: an omission that silently produced a
 * plaintext approval room would be the same mistake from the other side.
 *
 * WHAT REPLACES ENCRYPTION HERE, and why it is defensible for this room specifically: the content at
 * risk is `input_preview`, the command the agent wants to run — and it runs against the BORROWER's own
 * repository. They are not a third party to it. What survives is narrower, and is an access-control
 * question rather than a confidentiality one: who else inside the borrower's organisation may see and
 * answer. That is what `preset: 'private_chat'` and an explicit invite list are for. ADR-003's
 * encryption requirement was written when the decider was assumed to be the contributor; the operator's
 * decision is what changes it, and this parameter is where that shows up in code.
 */
export async function createRoomOnSide({
  side,
  credential,
  name,
  topic = null,
  invite = [],
  encrypted,
  powerLevelOverride = null,
  isDirect = false,
  fetchImpl = fetch,
}) {
  if (!side?.apiBaseUrl || !side?.serverName) {
    throw new RepresentativeError('bad_request', 'side must carry apiBaseUrl and serverName');
  }
  if (!credential) throw new RepresentativeError('bad_request', 'a project side credential is required');
  if (typeof encrypted !== 'boolean') {
    throw new RepresentativeError(
      'bad_request',
      'encrypted must be stated explicitly: the representative has no crypto store, so this is a '
      + 'decision the caller has to make rather than inherit',
    );
  }
  if (encrypted) {
    throw new RepresentativeError(
      'unsupported',
      'a representative cannot create an encrypted room: it holds no crypto store, so it could not '
      + 'read the room it created. ADR-016 settles intake as plaintext; use an invite-only room and '
      + 'control who is in it.',
    );
  }
  if (!name || typeof name !== 'string') {
    throw new RepresentativeError('bad_request', 'name is required');
  }

  /*
   * An appservice side acts AS its sender_localpart, so the create must be masqueraded; a
   * registration-token side acts as the representative account it registered, with its own token.
   * Both end up creating the room as the same logical actor — the difference is only which credential
   * carries it.
   */
  let token;
  let asUserId = null;
  if (credential.kind === 'appservice') {
    token = credential.asToken;
    asUserId = side.representative?.mxid || `@${String(credential.senderLocalpart)}:${side.serverName}`;
  } else if (credential.kind === 'registrationToken') {
    token = credential.representativeToken;
    if (!token) {
      return {
        created: false, roomId: null,
        reason: 'this side has no representative token yet — verify the side before creating rooms on it',
      };
    }
  } else {
    throw new RepresentativeError('bad_request', `unsupported credential kind: ${credential.kind}`);
  }

  const url = new URL(`${String(side.apiBaseUrl).replace(/\/+$/, '')}/_matrix/client/v3/createRoom`);
  if (asUserId) setMasqueradeUserParam(url, asUserId);

  const body = {
    preset: 'private_chat',
    name,
    ...(topic ? { topic } : {}),
    ...(invite.length ? { invite } : {}),
    ...(isDirect ? { is_direct: true } : {}),
    ...(powerLevelOverride ? { power_level_content_override: powerLevelOverride } : {}),
    /*
     * EXPLICITLY EMPTY, not omitted. A homeserver may be configured to encrypt private rooms by
     * default, and inheriting that would hand us a room we cannot read — the exact outcome the refusal
     * above exists to prevent. Stating an empty initial state is the only way to say "not that".
     */
    initial_state: [],
  };

  try {
    const res = await fetchImpl(url.toString(), {
      method: 'POST',
      headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    const data = await readBody(res);
    if (!res.ok) throw matrixError(res, data, 'createRoom');
    const roomId = typeof data?.room_id === 'string' ? data.room_id.trim() : '';
    if (!roomId) throw new RepresentativeError('matrix_error', 'createRoom did not return a room_id');
    /*
     * The room id names the server that owns it, and that must be THIS side. A homeserver returning a
     * room on another origin would mean we created it somewhere we did not intend — worth catching
     * here rather than discovering when the borrower cannot find it.
     */
    const origin = roomId.slice(roomId.indexOf(':') + 1).toLowerCase();
    if (origin !== side.serverName) {
      return {
        created: false, roomId: null,
        reason: `createRoom returned a room on ${origin}, not on ${side.serverName}`,
      };
    }
    return { created: true, roomId, encrypted: false, reason: null };
  } catch (error) {
    return {
      created: false, roomId: null,
      state: classifyMatrixFailure(error),
      reason: error.message,
    };
  }
}

/*
 * Resolve which token acts, and as whom, for a side + credential pair.
 *
 * Extracted after the third copy: `createRoomOnSide` and `sendToRoomOnSide` each carried this ladder
 * inline, and the invite/join pair below needs the SAME answer. Three transcriptions of one rule is
 * how the appservice branch ends up masquerading in two places and not the third.
 */
function actorFor(side, credential, whatFor) {
  if (credential.kind === 'appservice') {
    return {
      token: credential.asToken,
      asUserId: side.representative?.mxid || `@${String(credential.senderLocalpart)}:${side.serverName}`,
    };
  }
  if (credential.kind === 'registrationToken') {
    if (!credential.representativeToken) {
      return { reason: `this side has no representative token yet — verify the side before ${whatFor}` };
    }
    return { token: credential.representativeToken, asUserId: null };
  }
  throw new RepresentativeError('bad_request', `unsupported credential kind: ${credential.kind}`);
}

/**
 * Does this appservice's claimed namespace admit `mxid`? Returns `true`, or the reason it does not.
 *
 * An as_token may act as any user INSIDE its namespace and must never be talked into acting outside
 * it, so this answers the one question that makes a masquerade legitimate.
 */
/*
 * Exported because the bridge's credential CHECK needs the same rule its send path uses. It was private
 * while only one caller existed; a second copy would have been a second copy that drifts, and the one
 * that drifts first is the check, since nothing sends through it.
 */
/*
 * F10: THE single exit point for masquerade user_id values. Every ?user_id=
 * set on a request that acts as a user through an appservice credential goes
 * through here: the id must (a) be syntactically an MXID, (b) name an agent
 * REGISTERED in this fleet (the roster is supplied by the caller — the
 * bridge's known-agent set), and (c) fall inside the side's registered
 * namespace. A masquerade that fails any check is refused BEFORE the request
 * is built: sending it anyway would have the homeserver execute a spoofed
 * identity, which is the one thing an as_token must never be able to do.
 *
 * F10 (17-r1): EXPORTED, and the namespace check OPTS IN. The board's refusal
 * was that "every exit point goes through one validator" was only true of this
 * file: `bridge-matrix.js` built the representative's join URL and set
 * `user_id` on it directly, so the claim held nowhere but here. Now the bridge
 * sends through the same function — which means it must be importable — and the
 * checks are passed through as options rather than hard-coded, because the two
 * callers police different populations. A representative join masquerades as
 * `sender_localpart`, which is BY DEFINITION outside `@ac_.*` — the namespace
 * and roster that make an agent masquerade legitimate would forbid the one
 * user an appservice always controls. So `joinRoomOnSideAsRepresentative`
 * calls this with NO namespace and NO roster (the MXID shape check still
 * applies), and the agent paths keep passing both.
 */
export function setMasqueradeUserParam(url, userId, { namespace = null, isRegisteredAgent = null, label = 'representative' } = {}) {
  const verdict = validateMasqueradeUserId({ userId, namespace, isRegisteredAgent, label });
  if (!verdict.ok) throw new RepresentativeError('masquerade_refused', verdict.reason);
  url.searchParams.set('user_id', userId);
}

export function validateMasqueradeUserId({ userId, namespace, isRegisteredAgent, label = 'masquerade' }) {
  const id = typeof userId === 'string' ? userId.trim() : '';
  if (!/^@[^:\s@]+:[^\s@]+$/.test(id)) {
    return { ok: false, reason: `${label}: "${id}" is not an MXID` };
  }
  /*
   * F10 (17-r3): AGENT labels MUST have a roster to ask. An agent masquerade
   * that skips the roster check because no callback was supplied is exactly
   * the silent downgrade the board refused: namespace-only answers "the
   * prefix looks right", which a ghost satisfies by construction. Refuse
   * outright — REFUSED: roster check unavailable — rather than degrade.
   * Representative labels are the exception BY DEFINITION: the representative
   * is sender_localpart, not a roster member at all.
   */
  if (typeof isRegisteredAgent !== 'function' && /^agent-/.test(label)) {
    return { ok: false, reason: `${label}: roster check unavailable — an agent masquerade is refused rather than degraded to namespace-only` };
  }
  if (typeof isRegisteredAgent === 'function' && !isRegisteredAgent(id)) {
    return { ok: false, reason: `${label}: ${id} is not a registered agent of this fleet` };
  }
  /*
   * namespaceAdmits returns TRUE on admission and a TRUTHY STRING (the
   * refusal explanation) on rejection — `!admits` would never fire. Compare
   * against true explicitly; the string doubles as the reason.
   */
  if (namespace) {
    const admits = namespaceAdmits(namespace, id);
    if (admits !== true) {
      return { ok: false, reason: `${label}: ${typeof admits === 'string' ? admits : `${id} is outside the registered namespace ${namespace}`}` };
    }
  }
  return { ok: true };
}

export function namespaceAdmits(namespace, mxid) {
  if (!namespace) {
    return 'this appservice credential states no user namespace, so nothing can be verified as ours to '
      + 'act for';
  }
  let pattern;
  try {
    pattern = new RegExp(`^${namespace}$`);
  } catch {
    return `this side's claimed namespace is not a usable regex: ${namespace}`;
  }
  if (!pattern.test(mxid)) {
    return `${mxid} is outside the namespace this side claimed (${namespace}), and an as_token must `
      + 'not act as a user it does not own';
  }
  return true;
}

/** A room id names its origin server; anything on another origin is not this side's to touch. */
function roomBelongsToSide(roomId, side) {
  const at = roomId.indexOf(':');
  const origin = at > 0 ? roomId.slice(at + 1).toLowerCase() : null;
  return origin === side.serverName;
}

/**
 * Does this project's homeserver federate with ours? — ADR-016 decision 2's optimization.
 *
 * The decision says federation is "a single permission inside" the one model: where the project's server
 * federates with an existing agent's server, that identity MAY be reused instead of minting a new one.
 * Answering that needs a fact about somebody else's homeserver, so it is probed rather than configured.
 *
 * THE PROBE ASKS THE SIDE TO LOOK UP A USER WE KNOW EXISTS ON OUR SERVER — the bot. That choice is what
 * makes the answer readable:
 *
 *   - a profile comes back → their server reached ours, so it federates with us;
 *   - `M_NOT_FOUND` → it reached ours and ours said no such user, which still proves federation. Only
 *     possible if `probeMxid` is wrong, so it is reported as `unknown` rather than counted either way;
 *   - anything else — a timeout, 502, `M_UNKNOWN`, a federation-disabled refusal — is `isolated`.
 *
 * ISOLATED IS THE SAFE ANSWER AND THE DEFAULT. Guessing `federates` wrongly means reusing an identity the
 * project cannot see, which produces an agent that is addressable only in theory; guessing `isolated`
 * wrongly means minting an account that was not strictly needed. The operator's 2026-08-13 ruling —
 * assume the project's Matrix does not federate — is the same asymmetry stated as a rule.
 */
export async function probeFederationFromSide({ side, credential, probeMxid, fetchImpl = fetch }) {
  if (!side?.apiBaseUrl || !side?.serverName) {
    throw new RepresentativeError('bad_request', 'side must carry apiBaseUrl and serverName');
  }
  if (!credential) throw new RepresentativeError('bad_request', 'a project side credential is required');
  if (typeof probeMxid !== 'string' || !probeMxid.startsWith('@') || !probeMxid.includes(':')) {
    throw new RepresentativeError('bad_request', 'probeMxid must be a full MXID on OUR server');
  }
  const probeServer = probeMxid.slice(probeMxid.indexOf(':') + 1).toLowerCase();
  if (probeServer === side.serverName) {
    /*
     * The same server is not federation, and calling it that would make every single-server deployment
     * look federated — including the one this was developed against.
     */
    return { federation: 'unknown', reason: `${probeMxid} is on ${side.serverName} itself, so nothing is proven` };
  }

  const actor = actorFor(side, credential, 'probing federation from it');
  if (actor.reason) return { federation: 'unknown', reason: actor.reason };

  const url = new URL(
    `${String(side.apiBaseUrl).replace(/\/+$/, '')}/_matrix/client/v3/profile/`
    + `${encodeURIComponent(probeMxid)}`,
  );
  if (actor.asUserId) setMasqueradeUserParam(url, actor.asUserId);
  try {
    const res = await fetchImpl(url.toString(), {
      method: 'GET',
      headers: { Authorization: `Bearer ${actor.token}` },
    });
    const data = await readBody(res);
    if (res.ok) return { federation: 'federates', reason: null };
    if (data?.errcode === 'M_NOT_FOUND') {
      return {
        federation: 'unknown',
        reason: `${side.serverName} answered M_NOT_FOUND for ${probeMxid}: it may federate with us and not `
          + 'know that user, or not federate at all — probe with an mxid that certainly exists',
      };
    }
    return {
      federation: 'isolated',
      reason: `${side.serverName} could not resolve ${probeMxid}: ${data?.errcode || `HTTP ${res.status}`}`,
    };
  } catch (error) {
    return { federation: 'isolated', reason: `${side.serverName} could not be asked: ${error.message}` };
  }
}

/**
 * Resolve `#project:server` to a room id, on the side that publishes it.
 *
 * The first half of ADR-016 decision 5: an alias is the operator's 「邀请码」 in native Matrix terms —
 * shareable, human-readable, and scoped to the server that owns it. Resolution is a plain directory
 * read and needs no masquerade; it is separated from the knock because the two fail for different
 * reasons and an operator fixes them differently. An unknown alias is a typo or an unpublished room;
 * a refused knock is a policy.
 *
 * THE ALIAS MUST NAME THIS SIDE'S SERVER. `#p:other.example` resolved through this side's credential
 * would either fail or — if the homeserver happened to know it over federation — hand back a room this
 * side has no account in, which is the cross-side confusion `sendToRoomOnSide` refuses for room ids.
 */
export async function resolveAliasOnSide({ side, credential, alias, fetchImpl = fetch }) {
  if (!side?.apiBaseUrl || !side?.serverName) {
    throw new RepresentativeError('bad_request', 'side must carry apiBaseUrl and serverName');
  }
  if (!credential) throw new RepresentativeError('bad_request', 'a project side credential is required');
  if (typeof alias !== 'string' || !alias.startsWith('#') || !alias.includes(':')) {
    throw new RepresentativeError('bad_request', 'alias must look like #room:server');
  }
  const aliasServer = alias.slice(alias.indexOf(':') + 1).toLowerCase();
  if (aliasServer !== side.serverName) {
    return {
      resolved: false, roomId: null,
      reason: `${alias} is published by ${aliasServer}, not by ${side.serverName}`,
    };
  }

  const actor = actorFor(side, credential, 'resolving aliases on it');
  if (actor.reason) return { resolved: false, roomId: null, reason: actor.reason };

  const url = new URL(
    `${String(side.apiBaseUrl).replace(/\/+$/, '')}/_matrix/client/v3/directory/room/`
    + `${encodeURIComponent(alias)}`,
  );
  if (actor.asUserId) setMasqueradeUserParam(url, actor.asUserId);
  try {
    const res = await fetchImpl(url.toString(), {
      method: 'GET',
      headers: { Authorization: `Bearer ${actor.token}` },
    });
    const data = await readBody(res);
    if (!res.ok) throw matrixError(res, data, 'directory lookup');
    const roomId = typeof data?.room_id === 'string' ? data.room_id.trim() : '';
    if (!roomId) throw new RepresentativeError('matrix_error', 'directory returned no room_id');
    return { resolved: true, roomId, servers: Array.isArray(data?.servers) ? data.servers : [], reason: null };
  } catch (error) {
    return {
      resolved: false, roomId: null,
      state: classifyMatrixFailure(error),
      reason: error.message,
    };
  }
}

/**
 * Knock on a project's room, as the representative — ADR-016 decision 5's second half.
 *
 * A knock is a PULL: we ask, the project accepts. That matches the intake direction the rest of this
 * design already has, and it is why the decision chose it over an invite token we would have to issue.
 *
 * IT DOES NOT JOIN, AND IT DOES NOT REPLACE APPROVAL. `knock` leaves membership in `knock` state until
 * somebody on the project's side invites us; and the contributor's approval of any lending remains a
 * separate, audited act — ADR-014's ruling stands: 「joining a Discord costs the joiner nothing.
 * Lending an agent spends tokens.」 So a successful knock means findable-and-asked, nothing more, and
 * this function returns `knocked` rather than anything that reads like access.
 *
 * ALREADY IN THE ROOM IS A SUCCESS WITH NOTHING DONE, for the same reason it is in
 * `inviteToRoomOnSide`: a caller that cannot tell it from a refusal either retries forever or reports a
 * working room as broken. A room whose join rule is not `knock` is a REFUSAL and says so, because the
 * remedy belongs to the project (set the rule) and not to us.
 */
export async function knockOnRoomOnSide({
  side,
  credential,
  aliasOrRoomId,
  reason = null,
  fetchImpl = fetch,
}) {
  if (!side?.apiBaseUrl || !side?.serverName) {
    throw new RepresentativeError('bad_request', 'side must carry apiBaseUrl and serverName');
  }
  if (!credential) throw new RepresentativeError('bad_request', 'a project side credential is required');
  if (typeof aliasOrRoomId !== 'string' || !/^[#!]/.test(aliasOrRoomId) || !aliasOrRoomId.includes(':')) {
    throw new RepresentativeError('bad_request', 'aliasOrRoomId must look like #room:server or !id:server');
  }
  const target = aliasOrRoomId.slice(aliasOrRoomId.indexOf(':') + 1).toLowerCase();
  if (target !== side.serverName) {
    return {
      knocked: false, already: false, roomId: null,
      reason: `${aliasOrRoomId} is on ${target}, not on ${side.serverName}`,
    };
  }

  const actor = actorFor(side, credential, 'knocking on rooms on it');
  if (actor.reason) return { knocked: false, already: false, roomId: null, reason: actor.reason };

  const url = new URL(
    `${String(side.apiBaseUrl).replace(/\/+$/, '')}/_matrix/client/v3/knock/`
    + `${encodeURIComponent(aliasOrRoomId)}`,
  );
  if (actor.asUserId) setMasqueradeUserParam(url, actor.asUserId);
  try {
    const res = await fetchImpl(url.toString(), {
      method: 'POST',
      headers: { Authorization: `Bearer ${actor.token}`, 'Content-Type': 'application/json' },
      body: JSON.stringify(reason ? { reason } : {}),
    });
    const data = await readBody(res);
    if (!res.ok) {
      const message = String(data?.error ?? '');
      if (res.status === 403 && /already (in|a member of) the room|already joined/i.test(message)) {
        return { knocked: false, already: true, roomId: null, reason: null };
      }
      /*
       * A homeserver that does not implement knocking at all answers 404 with M_UNRECOGNIZED, and an
       * operator reading "HTTP 404" would go looking for a wrong alias. Named, because the remedy is a
       * homeserver upgrade rather than anything about this room.
       */
      if (data?.errcode === 'M_UNRECOGNIZED') {
        return {
          knocked: false, already: false, roomId: null, state: 'unsupported',
          reason: `${side.serverName} does not implement knocking (M_UNRECOGNIZED), so an alias cannot be `
            + 'the invite object on this homeserver — it has to invite the representative directly',
        };
      }
      throw matrixError(res, data, 'knock');
    }
    return {
      knocked: true, already: false,
      roomId: typeof data?.room_id === 'string' ? data.room_id : null,
      reason: null,
    };
  } catch (error) {
    return {
      knocked: false, already: false, roomId: null,
      state: classifyMatrixFailure(error),
      reason: error.message,
    };
  }
}

/**
 * Invite a user into a room on a project side, as the representative.
 *
 * ADR-016 decision 3 says the representative is the one who brings a dispatched agent into a project
 * room. Until this, the agent joined on its own — which works only where the agent HAS a token of its
 * own. An appservice side mints no per-agent token at all (the namespace makes the agent addressable,
 * not able to act), so on exactly the sides ADR-016 treats as the normal case, nobody could put the
 * agent in the room.
 *
 * ALREADY-A-MEMBER IS NOT A FAILURE. Matrix answers 403 `M_FORBIDDEN` for "already in the room" and
 * for "you may not invite here", and a caller that cannot tell them apart either retries forever or
 * treats a working room as broken. The message is matched to separate them, and `already: true` is
 * reported as a success with nothing done — so this is safe to call on every dispatch rather than
 * only on the first.
 */
export async function inviteToRoomOnSide({
  side,
  credential,
  roomId,
  userId,
  reason = null,
  fetchImpl = fetch,
}) {
  if (!side?.apiBaseUrl || !side?.serverName) {
    throw new RepresentativeError('bad_request', 'side must carry apiBaseUrl and serverName');
  }
  if (!credential) throw new RepresentativeError('bad_request', 'a project side credential is required');
  if (!roomId || typeof roomId !== 'string') throw new RepresentativeError('bad_request', 'roomId is required');
  if (!userId || typeof userId !== 'string' || !userId.startsWith('@')) {
    throw new RepresentativeError('bad_request', 'userId must be a full MXID');
  }
  if (!roomBelongsToSide(roomId, side)) {
    return {
      invited: false, already: false,
      reason: `${roomId} is not on ${side.serverName}, so this side's credential does not belong there`,
    };
  }

  const actor = actorFor(side, credential, 'inviting into rooms on it');
  if (actor.reason) return { invited: false, already: false, reason: actor.reason };

  const url = new URL(
    `${String(side.apiBaseUrl).replace(/\/+$/, '')}/_matrix/client/v3/rooms/`
    + `${encodeURIComponent(roomId)}/invite`,
  );
  if (actor.asUserId) setMasqueradeUserParam(url, actor.asUserId);

  try {
    const res = await fetchImpl(url.toString(), {
      method: 'POST',
      headers: { Authorization: `Bearer ${actor.token}`, 'Content-Type': 'application/json' },
      body: JSON.stringify({ user_id: userId, ...(reason ? { reason } : {}) }),
    });
    const data = await readBody(res);
    if (!res.ok) {
      /*
       * The one 403 that means "nothing to do". Matched on the message because Matrix gives it no
       * error code of its own — `M_FORBIDDEN` covers both this and a genuine refusal.
       */
      if (res.status === 403 && /already (in|a member of) the room|already joined/i.test(String(data?.error ?? ''))) {
        return { invited: false, already: true, reason: null };
      }
      throw matrixError(res, data, 'invite');
    }
    return { invited: true, already: false, reason: null };
  } catch (error) {
    return {
      invited: false, already: false,
      state: classifyMatrixFailure(error),
      reason: error.message,
    };
  }
}

/**
 * Can our representative invite into this room? — the diagnosis for a 403 nobody can read.
 *
 * A LIVE RUN FOUND THIS, and it is an interaction between two decisions rather than a bug in either.
 * Decision 5 lets us enter a project's room by knocking; decision 3 has the representative invite our
 * agents into it. In a room the representative CREATED it holds PL 100 and both work. In a room the
 * PROJECT created and we knocked into, it holds `users_default` — 0 on a default Palpo room, against an
 * `invite` requirement of 50. So we get in, and cannot bring anyone with us.
 *
 * The invite fails with a bare `M_FORBIDDEN`, which is indistinguishable from a dozen other refusals. So
 * this is read from the room's own power levels rather than guessed from an error string: a definite
 * answer, on the failure path only, at the cost of one state read when something has already gone wrong.
 */
export async function canRepresentativeInvite({ side, credential, roomId, fetchImpl = fetch }) {
  const actor = actorFor(side, credential, 'reading power levels on it');
  if (actor.reason) return { known: false, reason: actor.reason };
  const url = new URL(
    `${String(side.apiBaseUrl).replace(/\/+$/, '')}/_matrix/client/v3/rooms/`
    + `${encodeURIComponent(roomId)}/state/m.room.power_levels/`,
  );
  if (actor.asUserId) setMasqueradeUserParam(url, actor.asUserId);
  try {
    const res = await fetchImpl(url.toString(), {
      method: 'GET',
      headers: { Authorization: `Bearer ${actor.token}` },
    });
    const data = await readBody(res);
    if (!res.ok) return { known: false, reason: `power levels unreadable: ${data?.errcode || res.status}` };
    const required = Number.isFinite(data?.invite) ? data.invite : 0;
    const usersDefault = Number.isFinite(data?.users_default) ? data.users_default : 0;
    const mine = Number.isFinite(data?.users?.[actor.asUserId]) ? data.users[actor.asUserId] : usersDefault;
    return { known: true, can: mine >= required, required, mine };
  } catch (error) {
    return { known: false, reason: `power levels unreadable: ${error.message}` };
  }
}

/** Read a room label using the recorded representative; it grants no room authority. */
export async function roomNameOnSide({ side, credential, roomId, fetchImpl = fetch }) {
  if (!roomBelongsToSide(roomId, side)) return null;
  const actor = actorFor(side, credential, 'reading its room name');
  if (actor.reason) return null;
  const url = new URL(`${String(side.apiBaseUrl).replace(/\/+$/, '')}/_matrix/client/v3/rooms/`
    + `${encodeURIComponent(roomId)}/state/m.room.name/`);
  if (actor.asUserId) setMasqueradeUserParam(url, actor.asUserId);
  const res = await fetchImpl(url.toString(), {
    method: 'GET', headers: { Authorization: `Bearer ${actor.token}` }, signal: AbortSignal.timeout(12000),
  });
  const data = await readBody(res);
  return res.ok && typeof data?.name === 'string' ? data.name : null;
}

/**
 * Who is joined to a room on a project side, read with the side's own credential.
 *
 * WHY IT EXISTS. The bridge classifies an inbound room by its membership — one agent plus one human is an
 * agent DM, one human alone is a bot DM, otherwise it is a group — and it read that membership with Hagency's
 * own bot. On a project side the bot is not in the room, so the read fails and the classification with it.
 * The representative IS in the room, and this is what it asks with.
 *
 * SAME ACTOR RULE AS THE POWER-LEVEL READ beside it: `actorFor` decides who asks and refuses a side whose
 * credential cannot act, so a registrationToken side without a representative token is a reason rather than
 * a failed request.
 *
 * `known: false` RATHER THAN AN EXCEPTION, because a caller mid-message must be able to carry on without
 * this. An unreadable membership means "classify by what else you know", not "drop the message".
 */
export async function joinedMembersOnSide({ side, credential, roomId, fetchImpl = fetch }) {
  const actor = actorFor(side, credential, 'reading its membership');
  if (actor.reason) return { known: false, members: [], reason: actor.reason };
  const url = new URL(
    `${String(side.apiBaseUrl).replace(/\/+$/, '')}/_matrix/client/v3/rooms/`
    + `${encodeURIComponent(roomId)}/joined_members`,
  );
  if (actor.asUserId) setMasqueradeUserParam(url, actor.asUserId);
  try {
    const res = await fetchImpl(url.toString(), {
      method: 'GET',
      headers: { Authorization: `Bearer ${actor.token}` },
    });
    const data = await readBody(res);
    if (!res.ok) {
      let retryAfterMs = Number(data?.retry_after_ms);
      if (!Number.isFinite(retryAfterMs) || retryAfterMs < 0) {
        const raw = res.headers?.get?.('retry-after');
        const seconds = Number(raw);
        if (Number.isFinite(seconds) && seconds >= 0) retryAfterMs = seconds * 1_000;
        else {
          const retryAt = Date.parse(raw);
          retryAfterMs = Number.isFinite(retryAt) ? Math.max(0, retryAt - Date.now()) : null;
        }
      }
      return {
        known: false,
        members: [],
        reason: `membership unreadable: ${data?.errcode || res.status}`,
        ...(Number.isFinite(retryAfterMs) ? { retryAfterMs } : {}),
      };
    }
    const joined = data?.joined && typeof data.joined === 'object' ? Object.keys(data.joined) : [];
    return { known: true, members: joined, reason: null };
  } catch (error) {
    return { known: false, members: [], reason: `membership unreadable: ${error.message}` };
  }
}

export async function roomStateOnSide({ side, credential, roomId, fetchImpl = fetch }) {
  const actor = actorFor(side, credential, 'reading room admission');
  if (actor.reason) throw new Error(actor.reason);
  const url = new URL(`${String(side.apiBaseUrl).replace(/\/+$/, '')}/_matrix/client/v3/rooms/${encodeURIComponent(roomId)}/state`);
  if (actor.asUserId) setMasqueradeUserParam(url, actor.asUserId);
  const response = await fetchImpl(url, { headers: { Authorization: `Bearer ${actor.token}` }, signal: AbortSignal.timeout(15_000) });
  const state = await readBody(response);
  if (!response.ok || !Array.isArray(state)) throw new Error('room admission state is unavailable');
  return state;
}

/**
 * One page of a room's timeline on a project side, read with the side's own credential.
 *
 * WHY IT EXISTS. The bridge already backfills the invite→join window for a room its BOT joins, because
 * sync delivers nothing from before a join and anything said in that window is lost otherwise. The
 * representative has exactly the same window and no way to read it: `backfillJoinedRoom` paginates with
 * `this.botClient`, and on a project side the bot is not in the room. Proven on a live pair of machines
 * — a customer inviting the representative and asking in the same breath got no engagement, no reply and
 * no error, while the same ask one second after the join worked.
 *
 * SAME ACTOR RULE as the two reads above it: `actorFor` decides who asks, so a registrationToken side
 * with no representative token yet is a reason rather than a failed request.
 *
 * `known: false` RATHER THAN AN EXCEPTION, matching `joinedMembersOnSide`. A caller mid-join must be able
 * to carry on without this: failing to read the window is a missed message, and turning it into a failed
 * join would take the representative out of a room it is legitimately in.
 *
 * NO ROOM-ORIGIN PRE-FLIGHT, deliberately, and for the same reason `joinedMembersOnSide` has none: a read
 * admits nobody to anything, and the one caller has already refused a room on another origin before it
 * ever joined. The checks belong on the acts — `joinRoomOnSideAsAgent`, `sendToRoomOnSide` — which have them.
 */
export async function roomMessagesOnSide({
  side,
  credential,
  roomId,
  from = null,
  to = null,
  limit = 50,
  dir = 'b',
  fetchImpl = fetch,
}) {
  const actor = actorFor(side, credential, 'reading its history');
  if (actor.reason) return { known: false, chunk: [], end: null, reason: actor.reason };
  const url = new URL(
    `${String(side.apiBaseUrl).replace(/\/+$/, '')}/_matrix/client/v3/rooms/`
    + `${encodeURIComponent(roomId)}/messages`,
  );
  url.searchParams.set('dir', dir);
  url.searchParams.set('limit', String(limit));
  if (from) url.searchParams.set('from', from);
  if (to) url.searchParams.set('to', to);
  if (actor.asUserId) setMasqueradeUserParam(url, actor.asUserId);
  try {
    const res = await fetchImpl(url.toString(), {
      method: 'GET',
      headers: { Authorization: `Bearer ${actor.token}` },
    });
    const data = await readBody(res);
    if (!res.ok) {
      return { known: false, chunk: [], end: null, reason: `history unreadable: ${data?.errcode || res.status}` };
    }
    if (!Array.isArray(data?.chunk) || (data.end != null && typeof data.end !== 'string')) {
      return { known: false, chunk: [], end: null, reason: 'history unreadable: malformed messages page' };
    }
    return {
      known: true,
      chunk: Array.isArray(data?.chunk) ? data.chunk : [],
      end: typeof data?.end === 'string' ? data.end : null,
      reason: null,
    };
  } catch (error) {
    return { known: false, chunk: [], end: null, reason: `history unreadable: ${error.message}` };
  }
}

/**
 * Join a room on a project side AS a dispatched agent, using the side's appservice credential.
 *
 * The other half of the invite above, and the half only an appservice can do: the as_token may act as
 * any user inside its namespace, so the agent enters the room without ever holding a credential. That
 * is the whole point of the namespace — `@ac_.*` means "these users are ours to act for" — and it is
 * why a project-side agent needs no registration.
 *
 * REFUSED, NOT ATTEMPTED, on a registrationToken side. There the agent has a real account and a real
 * token, and joining on its behalf with the REPRESENTATIVE's token would put the representative in the
 * room while reporting that the agent joined. The caller is told to use the agent's own token, which
 * is the path that already exists in the bridge.
 *
 * REFUSED for a user outside the namespace. Masquerading as an arbitrary user is exactly the
 * cross-account action an as_token must never be talked into: the check is here, at the only place
 * that composes the parameter, rather than trusted to every caller.
 */
export async function joinRoomOnSideAsAgent({
  side,
  credential,
  roomId,
  agentUserId,
  /*
   * F10 (17-r3): REQUIRED. The join masquerades as an agent, and an agent
   * masquerade without the fleet's roster answer is refused at the exit
   * rather than degraded to namespace-only.
   */
  isRegisteredAgent,
  fetchImpl = fetch,
}) {
  if (!side?.apiBaseUrl || !side?.serverName) {
    throw new RepresentativeError('bad_request', 'side must carry apiBaseUrl and serverName');
  }
  if (!credential) throw new RepresentativeError('bad_request', 'a project side credential is required');
  if (!roomId || typeof roomId !== 'string') throw new RepresentativeError('bad_request', 'roomId is required');
  if (!agentUserId || typeof agentUserId !== 'string' || !agentUserId.startsWith('@')) {
    throw new RepresentativeError('bad_request', 'agentUserId must be a full MXID');
  }
  if (credential.kind !== 'appservice') {
    return {
      joined: false, already: false,
      reason: 'only an appservice side can join as an agent; a registrationToken side has a per-agent '
        + "token and must use it, or the representative would join under the agent's name",
    };
  }
  if (!roomBelongsToSide(roomId, side)) {
    return {
      joined: false, already: false,
      reason: `${roomId} is not on ${side.serverName}, so this side's credential does not belong there`,
    };
  }
  const url = new URL(
    `${String(side.apiBaseUrl).replace(/\/+$/, '')}/_matrix/client/v3/join/${encodeURIComponent(roomId)}`,
  );
  /*
   * F10 (17-r2): namespace AND roster go to the SINGLE EXIT — the exit either
   * authorizes the masquerade or refuses it before the request exists. The
   * namespace check no longer lives outside the setter: one check, one place.
   * `isRegisteredAgent` is the fleet's authoritative roster (the bridge's
   * `isKnownAgentMxid`); NULL means the caller has no roster to ask, and the
   * exit degrades to namespace-only rather than guessing.
   */
  try {
    setMasqueradeUserParam(url, agentUserId, {
      namespace: credential.namespace ?? null,
      isRegisteredAgent,
      label: 'agent-join',
    });
  } catch (error) {
    return { joined: false, already: false, reason: error.message };
  }

  try {
    const res = await fetchImpl(url.toString(), {
      method: 'POST',
      headers: { Authorization: `Bearer ${credential.asToken}`, 'Content-Type': 'application/json' },
      body: '{}',
    });
    const data = await readBody(res);
    if (!res.ok) throw matrixError(res, data, 'join');
    return { joined: true, already: false, roomId: data?.room_id ?? roomId, reason: null };
  } catch (error) {
    return {
      joined: false, already: false,
      state: classifyMatrixFailure(error),
      reason: error.message,
    };
  }
}

/**
 * F10 (17-r1): Answer a project-side invite by joining AS THE REPRESENTATIVE, through the single
 * masquerade exit.
 *
 * WHY THIS EXISTS. The bridge used to build this join inline — one more `searchParams.set('user_id',
 * …)` outside the validator — and the outer loop refused F10 for exactly that: the "one exit point"
 * claim was only true inside this file. This helper is the bridge's replacement path, so the
 * representative's join is composed in the same place every other masquerade is.
 * Registration-token sides use their recorded representative access token directly,
 * with no user_id parameter and no attempt to register or log in again.
 *
 * WHY NO NAMESPACE AND NO ROSTER. The representative is the registration's `sender_localpart`, not
 * an `@ac_` user: the namespace regex and the fleet roster that make an AGENT masquerade legitimate
 * would refuse the one user this appservice unconditionally controls. What still applies is the
 * shape check (an MXID that is not an MXID is never something to act as) and the room's origin — a
 * room id names its server, and an invite naming a foreign room is not this side's to accept.
 *
 * MASQUERADE REFUSALS THROW, because the caller's catch lanes distinguish permanent from retryable,
 * and a masquerade refusal is as permanent as permissions get: retrying it would re-send a spoof the
 * homeserver was already told to refuse. `code: 'masquerade_refused'` lets the bridge match it.
 */
export async function joinRoomOnSideAsRepresentative({
  side,
  credential,
  roomId,
  fetchImpl = fetch,
}) {
  if (!side?.apiBaseUrl || !side?.serverName) {
    throw new RepresentativeError('bad_request', 'side must carry apiBaseUrl and serverName');
  }
  if (!credential) throw new RepresentativeError('bad_request', 'a project side credential is required');
  if (!roomId || typeof roomId !== 'string') throw new RepresentativeError('bad_request', 'roomId is required');
  if (!roomBelongsToSide(roomId, side)) {
    return {
      joined: false,
      code: 'foreign_room',
      reason: `${roomId} is not on ${side.serverName}, so this side's credential does not belong there`,
    };
  }
  const actor = actorFor(side, credential, 'answering its representative invite');
  if (actor.reason) return { joined: false, reason: actor.reason };
  const representative = side.representative?.mxid || actor.asUserId;
  if (!representative) return { joined: false, reason: 'representative identity has not been verified' };
  const url = new URL(
    `${String(side.apiBaseUrl).replace(/\/+$/, '')}/_matrix/client/v3/join/${encodeURIComponent(roomId)}`,
  );
  if (actor.asUserId) setMasqueradeUserParam(url, representative, { label: 'representative' });

  try {
    const res = await fetchImpl(url.toString(), {
      method: 'POST',
      headers: { Authorization: `Bearer ${actor.token}`, 'Content-Type': 'application/json' },
      body: '{}',
    });
    const data = await readBody(res);
    if (!res.ok) throw matrixError(res, data, 'join');
    return { joined: true, representative, roomId: data?.room_id ?? roomId, reason: null };
  } catch (error) {
    return {
      joined: false,
      representative,
      state: classifyMatrixFailure(error),
      reason: error.message,
    };
  }
}

/**
 * Take an agent OUT of a room on a project side — the counterpart to `joinRoomOnSideAsAgent`.
 *
 * WHY IT HAS TO EXIST. Deleting an agent removed its record, its scopes, its commitments and its group
 * memberships, and left its Matrix identity sitting in the customer's room. Read off a live homeserver:
 * four `@ac_e2e-probe-*` accounts still joined to a project room, every one of them an agent Hagency had
 * deleted. In the 施工队 model every Matrix server belongs to a customer, so those are seats occupied in
 * somebody else's house by contractors who left — visible in their member list, and still inside an
 * appservice namespace that makes them usable.
 *
 * THE SAME TWO CHECKS AS THE JOIN, and for the same reasons: `roomBelongsToSide`, because this side's
 * credential has no business acting on another server's room, and `namespaceAdmits`, because a masquerade
 * outside the claimed namespace is a 403 that reads identically to a bad as_token.
 *
 * IDEMPOTENT AT THE HOMESERVER, verified rather than assumed. A first version special-cased 403
 * `M_FORBIDDEN` as "already out" on the reasoning that Matrix refuses a leave you cannot perform. Run
 * against a real Palpo: leaving twice answers `200 {}` both times, and the 403 branch was dead code
 * written from imagination. What a non-existent user actually produces there is `500 M_UNKNOWN`, which is
 * a failure and is reported as one.
 */
export async function leaveRoomOnSideAsAgent({
  side,
  credential,
  roomId,
  agentUserId,
  // F10 (17-r3): REQUIRED, same contract as the join
  isRegisteredAgent,
  fetchImpl = fetch,
}) {
  if (!side?.apiBaseUrl || !side?.serverName) {
    throw new RepresentativeError('bad_request', 'side must carry apiBaseUrl and serverName');
  }
  if (!credential) throw new RepresentativeError('bad_request', 'a project side credential is required');
  if (!roomId || typeof roomId !== 'string') throw new RepresentativeError('bad_request', 'roomId is required');
  if (!agentUserId || typeof agentUserId !== 'string' || !agentUserId.startsWith('@')) {
    throw new RepresentativeError('bad_request', 'agentUserId must be a full MXID');
  }
  if (credential.kind !== 'appservice') {
    return {
      left: false,
      reason: 'only an appservice side can act as an agent; a registrationToken side holds a per-agent '
        + 'token and must use it',
    };
  }
  if (!roomBelongsToSide(roomId, side)) {
    return {
      left: false,
      reason: `${roomId} is not on ${side.serverName}, so this side's credential does not belong there`,
    };
  }

  const url = new URL(
    `${String(side.apiBaseUrl).replace(/\/+$/, '')}/_matrix/client/v3/rooms/${encodeURIComponent(roomId)}/leave`,
  );
  /*
   * F10 (17-r2): same single-exit contract as the join — namespace AND roster
   * checked at the exit, nowhere else. A ghost (in-namespace but not in the
   * fleet roster) is refused BEFORE the request is built.
   */
  try {
    setMasqueradeUserParam(url, agentUserId, {
      namespace: credential.namespace ?? null,
      isRegisteredAgent,
      label: 'agent-leave',
    });
  } catch (error) {
    return { left: false, reason: error.message };
  }

  try {
    const res = await fetchImpl(url.toString(), {
      method: 'POST',
      headers: { Authorization: `Bearer ${credential.asToken}`, 'Content-Type': 'application/json' },
      signal: AbortSignal.timeout(15_000),
      body: '{}',
    });
    const data = await readBody(res);
    if (!res.ok) throw matrixError(res, data, 'leave');
    return { left: true, reason: null };
  } catch (error) {
    return {
      left: false,
      state: classifyMatrixFailure(error),
      reason: error.message,
    };
  }
}

/** Leave known memberships before deleting the credential that owns them. */
export async function leaveRoomOnSideAsRepresentative({ side, credential, roomId, fetchImpl = fetch }) {
  if (!credential || !roomBelongsToSide(roomId, side)) return { left: false, reason: 'no credential for this room' };
  const actor = actorFor(side, credential, 'leaving a room');
  if (actor.reason) return { left: false, reason: actor.reason };
  try {
    const url = new URL(`${String(side.apiBaseUrl).replace(/\/+$/, '')}/_matrix/client/v3/rooms/${encodeURIComponent(roomId)}/leave`);
    if (actor.asUserId) setMasqueradeUserParam(url, actor.asUserId, { label: 'representative' });
    const res = await fetchImpl(url.toString(), {
      method: 'POST', headers: { Authorization: `Bearer ${actor.token}`, 'Content-Type': 'application/json' },
      body: '{}', signal: AbortSignal.timeout(15_000),
    });
    const data = await readBody(res);
    if (!res.ok) throw matrixError(res, data, 'leave');
    return { left: true, reason: null };
  } catch (error) { return { left: false, reason: error.message }; }
}

/**
 * Send a message into a room on a project side, as the representative.
 *
 * The counterpart to `createRoomOnSide`, and the reason the approval request can reach a decider who
 * is not on the contributor's homeserver. The bot cannot: it holds an account on one server only
 * (ADR-014 decision 4's split), so a room on a project side is unreachable to it.
 *
 * IDEMPOTENT BY TRANSACTION ID, and the caller supplies the seed rather than the clock. Matrix's
 * `PUT .../send/{type}/{txnId}` deduplicates on that id, so a retry after a timeout — where the send
 * may or may not have landed — must reuse it or post the message twice. A timestamp would make every
 * retry a new message, which is exactly the failure it looks like it prevents. `sendAsAgentContent`
 * derives its id the same way for the same reason.
 */
function projectionRepresentativeActor(side, credential) {
  if (credential.kind === 'appservice') {
    const publisherMxid = `@${String(credential.senderLocalpart).toLowerCase()}:${side.serverName}`;
    return { token: credential.asToken, asUserId: publisherMxid, publisherMxid };
  }
  if (credential.kind === 'registrationToken') {
    return {
      token: credential.representativeToken,
      asUserId: null,
      publisherMxid: typeof credential.representativeMxid === 'string'
        ? credential.representativeMxid.trim()
        : null,
    };
  }
  throw new RepresentativeError('bad_request', `unsupported credential kind: ${credential.kind}`);
}

function requireProjectionPublisher(actor, expectedPublisherMxid) {
  if (!actor.token) {
    throw new RepresentativeError('bad_request', 'authenticated representative credential is incomplete');
  }
  if (!actor.publisherMxid || actor.publisherMxid !== expectedPublisherMxid) {
    throw new RepresentativeError(
      'bad_request',
      'expected publisher does not match authenticated representative',
    );
  }
}

export async function sendToRoomOnSide({
  side,
  credential,
  roomId,
  content,
  txnSeed,
  msgType = 'm.room.message',
  finalTxnId,
  preparedEventType,
  expectedPublisherMxid,
  fetchImpl = fetch,
}) {
  if (!side?.apiBaseUrl || !side?.serverName) {
    throw new RepresentativeError('bad_request', 'side must carry apiBaseUrl and serverName');
  }
  if (!credential) throw new RepresentativeError('bad_request', 'a project side credential is required');
  if (!roomId || typeof roomId !== 'string') throw new RepresentativeError('bad_request', 'roomId is required');
  if (!content || typeof content !== 'object' || Array.isArray(content)) throw new RepresentativeError('bad_request', 'content is required');
  const projectionFields = [finalTxnId, preparedEventType, expectedPublisherMxid];
  const isProjection = projectionFields.some((value) => value !== undefined);
  if (isProjection && projectionFields.some((value) => value === undefined)) {
    throw new RepresentativeError('bad_request', 'projection send requires finalTxnId, preparedEventType, and expectedPublisherMxid');
  }
  if (isProjection
      && (typeof finalTxnId !== 'string'
        || !/^[A-Za-z0-9._~-]{1,128}$/.test(finalTxnId)
        || finalTxnId === '.'
        || finalTxnId === '..')) {
    throw new RepresentativeError('bad_request', 'invalid final Matrix transaction id');
  }
  if (isProjection && !['m.room.message', 'm.room.encrypted'].includes(preparedEventType)) {
    throw new RepresentativeError('bad_request', 'unsupported prepared Matrix event type');
  }
  if (!isProjection && (!txnSeed || typeof txnSeed !== 'string')) {
    throw new RepresentativeError(
      'bad_request',
      'txnSeed is required: a retry must reuse it, so it cannot be derived from a clock here',
    );
  }

  /*
   * THE ROOM MUST BELONG TO THIS SIDE. A room id names its origin server, and sending to one that does
   * not match means presenting this side's credential to a room it has no account in — at best a 403,
   * at worst the cross-side disclosure that `setRoomAvatar`'s retry ladder used to produce.
   */
  const at = roomId.indexOf(':');
  const origin = at > 0 ? roomId.slice(at + 1).toLowerCase() : null;
  if (origin !== side.serverName) {
    return {
      sent: false, eventId: null,
      reason: `${roomId} is not on ${side.serverName}, so this side's credential does not belong there`,
    };
  }

  let token;
  let asUserId = null;
  if (credential.kind === 'appservice') {
    token = credential.asToken;
    asUserId = side.representative?.mxid || `@${String(credential.senderLocalpart)}:${side.serverName}`;
  } else if (credential.kind === 'registrationToken') {
    token = credential.representativeToken;
    if (!token) {
      return { sent: false, eventId: null, reason: 'this side has no representative token yet' };
    }
  } else {
    throw new RepresentativeError('bad_request', `unsupported credential kind: ${credential.kind}`);
  }

  const projectionActor = isProjection ? projectionRepresentativeActor(side, credential) : null;
  if (projectionActor) requireProjectionPublisher(projectionActor, expectedPublisherMxid);
  const txnId = isProjection
    ? finalTxnId
    : `hagency_${createHash('sha256').update(txnSeed).digest('hex').slice(0, 32)}`;
  const eventType = isProjection ? preparedEventType : msgType;
  const url = new URL(
    `${String(side.apiBaseUrl).replace(/\/+$/, '')}/_matrix/client/v3/rooms/`
    + `${encodeURIComponent(roomId)}/send/${encodeURIComponent(eventType)}/${txnId}`,
  );
  if (asUserId) setMasqueradeUserParam(url, asUserId);

  try {
    const res = await fetchImpl(url.toString(), {
      method: 'PUT',
      headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
      body: JSON.stringify(content),
    });
    const data = await readBody(res);
    if (!res.ok) throw matrixError(res, data, 'send');
    const eventId = typeof data?.event_id === 'string' ? data.event_id.trim() : '';
    if (!eventId) throw new RepresentativeError('matrix_error', 'send did not return an event_id');
    return { sent: true, eventId, reason: null };
  } catch (error) {
    return { sent: false, eventId: null, state: classifyMatrixFailure(error), reason: error.message };
  }
}


export async function sendEmptyStateToRoomOnSide({
  side, credential, roomId, eventType, stateKey, content, expectedPublisherMxid, fetchImpl = fetch,
}) {
  if (!side?.apiBaseUrl || !side?.serverName) throw new RepresentativeError('bad_request', 'side must carry apiBaseUrl and serverName');
  if (!credential) throw new RepresentativeError('bad_request', 'a project side credential is required');
  if (!roomId || typeof roomId !== 'string') throw new RepresentativeError('bad_request', 'roomId is required');
  if (!content || typeof content !== 'object' || Array.isArray(content)) throw new RepresentativeError('bad_request', 'content is required');
  if (stateKey !== '') throw new RepresentativeError('bad_request', 'stateKey must be empty');
  if (typeof eventType !== 'string' || !/^[A-Za-z][A-Za-z0-9._-]{0,254}$/.test(eventType) || eventType.startsWith('m.room.')) {
    throw new RepresentativeError('bad_request', 'eventType must be a custom Matrix event type');
  }
  const at = roomId.indexOf(':');
  if (at <= 0 || roomId.slice(at + 1).toLowerCase() !== side.serverName) {
    return { sent: false, eventId: null, reason: `${roomId} is not on ${side.serverName}, so this side's credential does not belong there` };
  }
  const actor = projectionRepresentativeActor(side, credential);
  if (!actor.token) {
    return { sent: false, eventId: null, reason: 'this side has no representative token yet' };
  }
  requireProjectionPublisher(actor, expectedPublisherMxid);
  const url = new URL(`${String(side.apiBaseUrl).replace(/\/+$/, '')}/_matrix/client/v3/rooms/${encodeURIComponent(roomId)}/state/${encodeURIComponent(eventType)}/`);
  if (actor.asUserId) setMasqueradeUserParam(url, actor.asUserId);
  let httpStatus;
  try {
    const res = await fetchImpl(url.toString(), { method: 'PUT', headers: { Authorization: `Bearer ${actor.token}`, 'Content-Type': 'application/json' }, body: JSON.stringify(content) });
    // Unlike readBody, do not swallow a body timeout and misreport headers as
    // a completed failure. Thrown transport errors cannot supply this status.
    const data = await res.json();
    if (!res.ok) {
      if (Number.isInteger(res.status) && res.status >= 100 && res.status <= 599) httpStatus = res.status;
      throw matrixError(res, data, 'send state');
    }
    const eventId = typeof data?.event_id === 'string' ? data.event_id.trim() : '';
    if (!eventId) throw new RepresentativeError('matrix_error', 'state send did not return an event_id');
    return { sent: true, eventId, reason: null };
  } catch (error) {
    return { sent: false, eventId: null, state: classifyMatrixFailure(error), reason: error.message,
      ...(httpStatus === undefined ? {} : { httpStatus }) };
  }
}
