# Native outbound transport

This crate is a host embedding API, not a running service or a replacement for
domain request admission. See [ADR-042](../../knowledge/decisions/adr-042-native-outbound-http.md).

The trusted host opens the existing private custody Repository/Store, constructs
`HostConfig` from its installed Matrix registration and Palpo machine credentials,
then calls `Adapter::attach`. Registering the transport locally is not a connection
proof. `Adapter::run` uses three joined cancellable loops; a host that schedules
its own work can instead call `poll_once` for each lane and `publish_once`.

After receipt, a trusted consumer reads the lane Head and uses Claim with a stable
**attempt ID**, then Start to obtain its payload once. It validates Matrix source
authority or performs idempotent domain admission before recording its exact
external result using Complete. On unknown outcomes it must inspect that owner's
receipt and use Inspect; a new HTTP response is not inspection evidence. The
opaque transport scope is not a native domain capability. Machine generation,
Matrix registration generation and Matrix device incarnation are separate.

Host observations go through `freeze_update({heartbeat:true,...})`; custody owns
v2/generation/sequence. Retry pending data before producing replacement content.
Do not refresh observedAt on retry. The built-in empty heartbeat does not publish
a resource catalog, approve requests or make a connection ready.

```sh
cargo test -p hagency-palpo --offline
cargo clippy -p hagency-palpo -p hagency-store --all-targets --offline -- -D warnings
```

An additional optional reference check needs a read-only checkout of Palpo commit
`c7c400e04ab05479a63c30f14679ec0180457d85` and Node >=24. It starts only a local
fixture with an in-memory database; homeserver I/O is forbidden. Run at repository
root:

```sh
node native/hagency-palpo/tests/fixtures/palpo-reference.mjs /path/to/pinned-palpo
```

The script verifies the commit and relevant source cleanliness before importing
it. The example accepts only literal loopback HTTP and uses fixed synthetic
credentials. It must not be pointed at a deployed service. Normal Cargo tests
need neither Node nor another checkout.
