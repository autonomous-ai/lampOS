# Target-scope wording correction

The original generated gates record retained a stale 42f comment-only description. gates.original.json preserves those original bytes. gates.json corrects only target-scope metadata to the actual efbf42a02/b05b1cd77 composition and records the old value. No command, source map, result, profile, timestamp, test count or head/base identity changes. The existing nine executable/dep-info digest records are producer-observed post-compile snapshots; omitted executable/dep-info bytes do not establish independent reader rehash or pre-exec attestation.
