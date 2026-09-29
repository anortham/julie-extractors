# Artifact identity and force rebuilds

## Decision

Extraction writes validate the artifact ID inside their SQLite transaction,
in addition to the producer generation. A changed ID produces recoverable
`artifact_changed`, exit code `1`. A completed rebind cannot be overwritten by
a write prepared against the previous identity.

Different-root force scans reset the existing artifact inside that same
transaction. They assign a new artifact ID, clear old facts, revision history,
capabilities, extraction level, and rebind metadata, and insert the new scan.
They preserve the database file identity. A failed replacement rolls back to
the original artifact.

Metadata initialization checks for an empty metadata table while holding an
immediate SQLite write transaction. A populated artifact is never initialized
from a stale caller snapshot.

## Alternatives

Checking producer fingerprints alone misses rebinds because rebind changes
identity without changing the producer. Reporting this as a fingerprint
mismatch would incorrectly recommend a force rebuild.

Checking identity before unlinking still permits a concurrent change between
the check and deletion. Transactional replacement removes that gap without
adding a lock file or a platform-specific locking mechanism.

## Verification

Writer regressions cover stale metadata before and after opening, including
force, spooled, incremental, delete, and capability writes. CLI regressions
verify rollback after an injected metadata failure and successful rebuild
through a hard-linked database alias. Linux and native NTFS Windows release
gates verify the same contract.
