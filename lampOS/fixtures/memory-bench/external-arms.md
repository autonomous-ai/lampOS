# Out-of-tree memory adapters

These are integration notes, not completed adapters or benchmark results.
Keep third-party source, interpreters, environments and services outside
lampOS. Do not add Python to this repository. An external adapter is an
executable that speaks the version-2 JSON-lines contract in
[the benchmark plan](../../docs/memory-bench.md). The Rust runner remains the
owner of the fixture, answering model, judge, deadlines and cost ledger.

## Pin the implementation before running

Record the upstream repository and full commit, dependency lock, wrapper
revision, model identities, embedding dimensions, storage configuration and
all memory parameters. Give each attempt a fresh private state directory.
Configure every database, index, history file and cache under that directory;
SDK defaults that write into a user's home are unsuitable. The runner starts
adapters without inherited credentials. An adapter must finish pending work
before acknowledging a drain or snapshot and must retain its children within
its owned process group.

All extraction, update, reflection, retrieval and embedding requests must
cross the runner's model broker, including SDK-internal calls. A custom SDK
provider can translate its requests to the contract's model RPC. Direct
network calls with a second key bypass the shared budget and invalidate the
run. If an upstream API cannot use that boundary, report the adapter as
unsupported until an explicit bridge exists. Local embedding work must also
be declared and measured; different embedding models are a recorded arm
difference, not an invisible default.

## mem0

Use the open-source implementation rather than substituting a hosted account.
The [official integration reference](https://github.com/mem0ai/mem0/blob/main/LLM.md)
documents memory addition, search and deletion; the
[open-source overview](https://github.com/mem0ai/mem0/blob/main/docs/open-source/overview.mdx)
describes separate model, embedding, vector and history configuration.
Pin a revision before choosing exact APIs; do not assume an older extraction
loop or storage layout still describes the selected release.

The wrapper must map the fictional owner to a run-local entity, attach source
record identities to derived memories, and retain the SDK's native update
behavior. Ingest receives the observed owner/companion exchange, not the gold
key. Recall returns the SDK's selected memory text and provenance under the
common byte cap. Map deletion to all relevant raw, derived, indexed and history
state; deleting a single search result alone does not prove forgetting.
Snapshot and probe isolation must include every store and outstanding update,
not only the vector database. Register graph memory separately if enabled;
do not silently add it after seeing a result.

## MemoryOS

Use [BAI-LAB/MemoryOS](https://github.com/BAI-LAB/MemoryOS), the research project
with short-, mid- and long-term memory. Its documented interface includes
adding exchanges, retrieving memory and obtaining a user profile. Pin one
implementation variant and commit; the repository contains multiple storage
and interface variants. This name does not identify unrelated similarly
named projects.

Map profile text to the session snapshot and retrieved memory to recall.
Expose native consolidation triggers and any custom fixture-clock adaptation
in the manifest. A component that runs during ingestion remains ingestion
work; do not charge it as a free nightly pass. Snapshot all tiers, profile
state, indexes, heat/age metadata, update cursors and pending work for each
probe. Prove deletion through subsequent retrieval and consolidation, including
derived profiles; if this cannot be implemented faithfully, report that
capability as unsupported rather than replacing it with hidden test logic.

## Common admission checks

Before a month run, exercise actual forgetting effects, fresh snapshots after
mutations, unknown STT confidence, byte limits, both deadlines, a two-call
broker budget across probe restoration, and a multi-batch sleep. Probe exit
must discard its changes while preserving the main month's prior forgets.
No adapter may read questions, true-speaker labels, answer keys or other arms'
outputs. Apply the same admission, base character prompt and observed
transcript cache to every setup; document native instruction differences.

Use the runner's `smoke` and `plumbing-external` commands with a pinned wrapper
executable and full revision. A recorded successful month proves plumbing
only. Unsupported behavior, failed attempts and incomplete nights stay in the
report. Optional mem0 and MemoryOS runs add six runs per persona at three
repetitions; their extraction, embedding and storage costs remain unmeasured
until those adapters exist. Published scores or prices from other datasets
cannot fill those cells.
