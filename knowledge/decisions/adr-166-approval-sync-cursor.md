# ADR-166: Approval receipt identity includes the response digest

Status: Accepted for implementation and isolated qualification, 2026-09-17.

The authorized isolated Codex file request reached private card delivery, then
the approval pump refused a sync response because an earlier receipt had the
same next_batch cursor but a different body. Ordinary Matrix intake already
distinguishes these values. Approval intake must do the same: duplicate receipt
identity means the exact `(token, digest)` pair, including reopen validation.

Distinct responses keep their own bounded protected custody. Exact replay
cannot reinterpret previously rejected events under a new approval target.
The actual SDK still owns decryption and authentication. If it cannot account
for a changed timeline, the retained batch refuses instead of fabricating proof.
No synthetic sync token, journal clearing, new owner, or automatic send retry is
introduced. Existing 64-batch/256-source bounds remain.

The same live attempt also exposed Robrix's legacy-only 32-hex request parser;
native IDs remain their original 40-hex values. The client separately admits the
two exact syntax profiles without changing server authorization or verdict bytes.
