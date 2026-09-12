# Native outbound custody checkpoint

This isolated slice starts at `bf7a47d`. It adds custody schema 2 and a host-only
command API on the existing worker. It neither changes a service nor enables a
native HTTP collector.

Evidence read before implementation:

- Hagency `lib/fleet-outbound-client.js`, `lib/fleet-outbound-store.js`,
  `lib/fleet-outbound-config.js`, `bridge-matrix.js` and their outbound tests.
- Palpo source at `c7c400e04ab05479a63c30f14679ec0180457d85`, specifically
  `web-admin/lib/outbound.mjs` and `web-admin/deploy/outbound-v2.md`, inspected
  read-only. That source pin is not a live deployment-version observation.
- Accepted REQ-PALPO-OUTBOUND and ADR-095's distinct custody/domain owners.

The native lifecycle separates Matrix registration generation from rotating
machine generation, persists a stable consumer, retains full JSON before ACK,
and uses current opaque poll/lease tickets. It records processing claims, explicit
unknown outcomes and inspected result receipts. Exact completed retries preserve
arbitrary-ID tombstones. Publication bodies retain bytes, sequence and original
status observation times across lost responses. Rotation fences old proof and
publication authority without pretending an uncertain remote ACK succeeded.

Unlike the current JS store's lane/id-only ACK mutation, native ACK completion
also compares the exact current scope and token. Unlike simply marking all
retained work `acked` during rotation, it records `retired` lease authority while
preserving already owned registration work. Neither improvement claims that a
local request transport is an authenticated Matrix event.

Shared JavaScript canonical vectors include full device/ephemeral transaction
fields, Unicode, finite fractions, exponent spelling, numeric object keys and
opaque prototype-named data. Signed authority DTO encoders remain unchanged.

## Reproduce

```sh
cargo test -p hagency-store native_outbound_custody --locked --offline
cargo test -p hagency-store -p hagency-core --locked --offline
cargo clippy -p hagency-store -p hagency-core --all-targets --locked --offline -- -D warnings
cargo fmt --all -- --check
node native/scripts/check-rust-spec-bindings.mjs
```

Bound selectors cover exact intake, processing, rotation, publication, storage and
worker behavior. Test fixtures use fresh private directories and controlled host
observations only. Migration failure injection preserves prior schema/rows;
completion and rotation failure injection preserve pending state and receipts.
Limits reject excess record, payload and attempt use without deleting dedup data.
Actual results and lifecycle output are recorded in this change's progress entry.

## Remaining gates

The next adapter must implement HTTPS destination/redirect and body parsing rules,
scoped machine credentials, exact wire validation, authenticated Matrix source and
membership observations, request-domain handoff, bounded status/probe observation
and the actual network polling loop. Custody ACK is not domain admission. Local
probe result equality is not Matrix authentication. Host inspection must consult
the external owner's idempotent receipt before replaying unknown processing.

No model, homeserver, browser or live deployment was contacted. CI must still run
this new slice on the required native operating systems. The finite retained
history cap and continuous-operation maintenance remain explicit release gates.
