# Native CTR equality evidence

Original Native run34582323420 at955463a failed the Windows Cargo test
`native_media_interoperability`: ciphertext `[17]` equaled plaintext `[17]` at
the nonempty ciphertext inequality assertion. The original result is failure;
its separately successful default-Store qualification cannot replace it.
Full original logs, exact Cargo slices, artifact bodies and SHA256 custody are
preserved in the external 2026-09-10 migration cache.

[NIST SP800-38A section6.5](https://nvlpubs.nist.gov/nistpubs/legacy/sp/nistspecialpublication800-38a.pdf)
defines CTR as plaintext XOR the encrypted counter stream. Consequently a zero
keystream byte preserves that plaintext byte. Random ciphertext inequality for
every nonempty message is not a valid invariant, especially for one byte.

The regression uses a public fixed Node/OpenSSL AES256-CTR vector: a32-byte key
containing30zero bytes followed by0x016a, a16-byte zero IV, and plaintext0x11.
Node `createCipheriv('aes-256-ctr', key, iv)` produces ciphertext0x11. Its fixed
SHA256 is checked by the actual native codec, and the actual Matrix SDK independently
decrypts it. Corrupt ciphertext still fails the unchanged hash check. No cipher,
production key source or dependency changes are involved.

The original interoperability selector retains all original vectors, generated
encryption and native/SDK round trips. It checks actual ciphertext hash metadata
and independent keys and IVs across separate encryptions instead of an impossible
per-message ciphertext inequality requirement. Descriptor freshness checks are
not a proof of randomness quality or descriptor sender provenance.

This repairs one source-proven test defect. It does not explain the independent
Linux startup stall, Windows FileService recovery refusal, earlier SQLite
shutdown timeouts, or qualify complete file delivery or migration cutover.
