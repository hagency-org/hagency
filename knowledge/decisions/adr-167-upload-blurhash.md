# ADR-167: Optional Palpo upload metadata preserves exact response custody

Status: Accepted for implementation and isolated qualification, 2026-09-17.

An independent 34-byte opaque diagnostic upload to the authorized test fleet
returned HTTP200 with content_uri and blurhash:null. Local Palpo source declares
blurhash as Option<String> and serializes None. ADR072/083's single-member parser
rejects that response; protected upload response validation rejects it as well.
The earlier live file002 attempt retains WritePossible without accepted response
custody. The probe demonstrates a compatibility failure, not the missing original
response or authority to settle that attempt.

Both HTTP acceptance and protected reopen now use one strict parser that admits
content_uri plus optional null/string blurhash. The existing 4096-byte body bound
also bounds the metadata. Other members, duplicate keys (including duplicate null
metadata), invalid types and invalid MXC identities remain refused. Metadata is
ignored for authority; exact original bytes and digests include it. Full EOF and
header validation remain required at the actual HTTP boundary. No old receipt is
rewritten, no synthetic response is accepted and no uncertain upload is retried.

The file service logs the fixed Matrix error variant when original upload fails,
before the unchanged single historical settlement inspection. This avoids losing
the useful failure reason without logging response bodies, tokens or file bytes.
