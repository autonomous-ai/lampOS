# Release 6786 public-text consistency read

The candidate named by First at Queue 6393960834 is `lampos-lamp-voice-6786`, commit `056a76132354f40f97eb5fbc519778c04f1ccb19`. Read only: downloaded the three public manifest/checksum text assets and read release API metadata. No binary or device was executed.

The envelope decodes to the exact published manifest bytes and names key ID `54213f8d3aa2f241`; its signature decodes to 64 bytes. This is a shape/consistency check, **not signature verification**. The trusted public key has not been supplied.

All 31 SHA256SUMS binary entries agree with the manifest. The 32nd payload, `lamp-product-skills`, is made and hashed separately by `crates/update/src/publish.rs:make_with`, so it is intentionally absent from that binary list. The initial overstrong assertion that every payload must be in SHA256SUMS is retained separately; contract inspection corrected it without changing inputs.

All 32 payload sizes and 28 unit sizes agree with API asset metadata; all 28 unit hashes and sizes match the exact committed Git blobs. The 63 published assets are exactly those 60 declarations and three manifest/checksum text assets. The receipt binds downloaded text hashes and each source unit.

No payload-binary hash was computed from a download. No trusted-key verification, signer, install, lamp run, power cut, rollback or release acceptance is claimed. Those remain #1812/#2810 handoffs to Migrate and the lamp holders.
