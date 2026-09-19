spec: task
name: "Own native Palpo resource publication in the executable service"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, palpo, publication, custody]
---

## Intent

Wire accepted ADR-109 "Publish coherent native resource catalogs through original
outbound custody" into the real native serve command under ADR-096 "Bootstrap
one owned development attempt after authenticated Matrix refresh" ownership.
The independent opt-in publishes current resources while retaining original
configuration, publication uncertainty and joined worker custody.

## Constraints

### Must
- Require explicit --palpo-transport independently of --development-driver.
- Load one closed private palpo-transport.json of at most 16 KiB plus the fixed private machine token and optional CA before starting store owners.
- Require a canonical HTTPS endpoint and validated full expected Registration with its exact canonical digest.
- Check the existing original DomainStore registration before Adapter attachment and refuse missing or different registration without outbound activation or HTTP.
- Reuse Bootstrap's original custody Store and DomainStore with the library's unchanged limits and one original Adapter::run_with_resources future.
- Retain the original worker join synchronously before any await and cancel its token without discarding or reconstructing the running future.
- Retain an incomplete join through the existing two-second close observation bound and settle the original worker before closing either writer.
- Store the consumed terminal join result before another await and keep failure or unknown observations explicit.
- Preserve ADR109's original pending body sequence and digest through edits and acknowledgment loss.
- Preserve the fresh original-domain check after custody waits and before HTTP admission while recording that already admitted request bytes cannot be recalled during registration rotation.
- Expose only bounded closed operator status labels without credentials private paths request bodies or raw transport errors.
- Qualify actual executable HTTPS behavior separately from in-process shutdown cross-compilation and unimplemented consumers.

### Must Not
- Do not automatically register rotate upgrade or recreate domain registration transport generations credentials or outbound custody.
- Do not require Codex an owned runtime Matrix SDK credentials or a development driver to publish resources.
- Do not add a second store listener collector adapter publication lane or hidden worker retry loop.
- Do not change library retries deadlines HTTP acceptance rules authorization or executable child cleanup semantics.
- Do not fabricate catalog acceptance Matrix readiness work consumption execution permission or production parity from a heartbeat or a running task.
- Do not contact live Palpo or change existing deployment state configuration or credentials.

## Boundaries

### Allowed Changes
- ./Cargo.lock
- native/hagency/Cargo.toml
- native/hagency/src/main.rs
- native/hagency/src/lib.rs
- native/hagency/src/bootstrap.rs
- native/hagency/src/bootstrap/config.rs
- native/hagency/src/bootstrap/palpo.rs
- native/hagency/tests/palpo_service.rs
- native/hagency/tests/palpo_service/fixture.rs
- knowledge/decisions/adr-096-native-development-bootstrap.md
- knowledge/decisions/adr-109-native-catalog-publication.md
- knowledge/context/native-palpo-service.md
- specs/task-rust-native-palpo-service.spec.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: The executable publishes the current resource catalog independently
  Test: native_palpo_service_executable_catalog
  Given a real native executable with explicit --palpo-transport independently of --development-driver and private configuration with an existing exact canonical registration
  When its original service runs against a local HTTPS peer and resources or role publication change through authenticated native HTTP
  Then the next accepted catalog has the coherent current offers including withdrawal without starting the development runner or claiming consumer completion

Scenario: Lost acknowledgment preserves the original publication
  Test: native_palpo_service_original_publication
  Given the real service has frozen a publication whose HTTPS acknowledgment is lost
  When the resource changes before that original pending update is accepted
  Then its original retry preserves exact bytes sequence and digest before a later catalog can appear

Scenario: Invalid configuration fails before outbound requests while disabled stays passive
  Test: native_palpo_service_configuration_refusal
  Given disabled missing malformed insecure private-file or mismatched-registration configuration
  When the actual executable loads its selected fixed profile
  Then disabled startup stays passive and enabled refusal sends no poll or update without changing registration

Scenario: Cancellation retains original worker and writer custody
  Test: native_palpo_service_cancel_custody
  Given the production Bootstrap owner is serving through real HTTPS with an in-flight original request
  When its existing cancellation token fires and non-consuming close observes completion or a retained incomplete join
  Then the original task settles before both writers close and no uncertain publication is reported accepted

## Out of Scope

Registration onboarding migration and rotation Matrix event or work consumers
allocation readiness native execution production cutover browser flows and live
provider qualification. Windows cross-compilation is compile evidence only;
graceful executable signal evidence must name the platform actually exercised.
