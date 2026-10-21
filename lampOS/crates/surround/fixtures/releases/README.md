# Release fixtures

The integration tests in `tests/it/release.rs`, `tests/it/selection.rs`, and
`tests/it/worker.rs` share
`support.rs`, which creates an isolated TUF repository
from text at runtime. Its Ed25519 signing seeds are the public test values
`[1; 32]` and `[2; 32]`. They are not production keys. Targets contain short
plain-text strings and are never executed; no binary artifact is committed.

Metadata follows the TUF top-level root/timestamp/snapshot/targets roles and
is signed using `ed25519-dalek`. Canonical bytes and verification use the
maintained Tough 0.24.0 schema and client. Tests cover signatures, expiration,
root rotation, persistent rollback floors, compatibility, bounded downloads,
interrupted staging, private paths, corruption, and offline revalidation.

Primary sources inspected:

- <https://docs.rs/tough/0.24.0/tough/struct.RepositoryLoader.html>
- <https://docs.rs/tough/0.24.0/tough/struct.Repository.html>
- <https://github.com/awslabs/tough/tree/98d8eb8b2ce63515d9b4981c938ef6453c5b5771/tough/src>
- <https://theupdateframework.github.io/specification/latest/>

Tough uses actual host UTC for expiry. Tests supply an explicit synchronized
time assertion matched to that clock; this does not verify any lamp clock,
production trust provisioning, OrangePi filesystem, or ARM64 executable.

The selection cases validate one atomic record for all three binary hashes,
stale-generation rejection, rollback after a damaged current binary, fresh
candidate rehashing, disabled selection, and interrupted writes. Disk-error
tests use a directory at the expected temporary-file path; they do not simulate
real power loss, a full SD card, or kernel/filesystem durability on the lamp.

Collection tests stage and select 18 releases through a four-set retention
bound, protect the current/previous/candidate/highest sets, reject foreign files
and links, and retry after removing directory write permission interrupts
deletion. Trust roots and signed version/UTC floors are not collected. The
worker uses a private test Unix supervisor for durable intent, failed manual
rollback and original-release recovery; fixture targets are never launched.
