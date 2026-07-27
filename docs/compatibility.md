# Compatibility and migration policy

Crownlines versions each compatibility boundary independently. The application
release number identifies a build; it does not imply that a protocol, save,
scenario, journal, snapshot, or database is compatible.

## Current compatibility matrix

| Boundary | Current | Accepted by this build | Failure behavior |
| --- | ---: | --- | --- |
| Client/server protocol | 2 | Exactly 2 | HTTP 426 or WebSocket `incompatible_protocol` before seat creation/authentication. |
| Local save wrapper | 2 | 1 and 2 | Format 1 is migrated in memory; failures preserve the existing slot. |
| Core save envelope | 2 | 1 and 2 through a scenario-aware reader | Format 1 is migrated in memory; unsupported versions return a recoverable error. |
| Authoritative snapshot envelope | 2 | 1 and 2 through a scenario-aware reader | Format 1 is migrated in memory; unsupported versions return a recoverable error. |
| Replay journal | 2 | 1 and 2 through a scenario-aware reader | Format 1 is rebuilt deterministically; incompatible actions stop migration. |
| Scenario schema | 2 | 1 and 2 | Schema 1 receives the implicit 2/4/8 promotion ladder; newer schemas are rejected. |
| Fog rules block | 1 | Exactly 1 when present | Omission or explicit `None` disables fog; unsupported nested versions fail scenario validation. |
| Server database schema | 2 | Fresh/0, 1, or 2 | Forward migration to 2; any version above 2 aborts startup without migration. |

“Exactly” on the wire is intentional: protocol 1 peers cannot interpret frozen
promotion eligibility and are rejected rather than downgraded. Persisted format
1 is now a supported product input because its migration paths have permanent
tests.

The `application_version` stored in files is provenance. It must be present, but
readers decide compatibility from the relevant independent format/schema field.

## Protocol negotiation

Every HTTP request and WebSocket message carries `protocol_version`. A mismatch
is rejected before room creation, seat join, or WebSocket seat authentication,
using the server's current protocol in the response and actionable client copy.
There is no mixed-version session and no capability downgrade within a protocol
version.

Any incompatible change to message meaning, required fields, authority rules,
or canonical synchronization increments `PROTOCOL_VERSION`. Additive changes
may retain the version only when old and new peers demonstrably interpret them
the same way; this must be covered in both protocol and real-server tests.

## Saves, snapshots, journals, and scenarios

These boundaries use `SAVE_FORMAT_VERSION`, `SNAPSHOT_FORMAT_VERSION`,
`JOURNAL_FORMAT_VERSION`, `SCENARIO_SCHEMA_VERSION`, and the client-local wrapper
version. Change the affected number whenever an existing reader could
misinterpret the same bytes, even if the application release number also
changes.

A newly supported source version requires checked-in fixtures that cover every
supported source-to-current path, canonical hash validation after migration,
pending choices/clocks/draw/terminal variants where applicable, corrupt and
future-version rejection, and atomic preservation of the original file on
failure. Migration functions must select by the declared source version; they
must not guess from missing fields. Scenario migration additionally requires all
authored scenarios and golden journals to validate under the new schema.

Unsupported files remain user data. Loading reports a recoverable error and
must not overwrite, partially migrate, or silently discard them.

Format 1 saves and snapshots did not store promotion eligibility. If restored
while a promotion batch is pending, migration calculates realm control once
from the restored canonical state and assigns that same frozen snapshot to
every queued promotion. Format 1 scenarios use Bishop/Rook/Queen thresholds
2/4/8. Format 1 journals are rebuilt by replaying their actions under the
current scenario and regenerating events and hashes; if a formerly accepted
action is no longer legal, migration fails with its source version and reason
instead of partially replaying it.

Fog configuration is an optional nested scenario boundary. Disabled fog is
omitted from canonical serialization, so the three shipped perfect-information
scenario hashes remain unchanged. Enabled fog currently requires nested schema
1 and a board-compatible radius; later line-of-sight semantics must increment or
replace that nested version rather than reinterpret existing scenario bytes.

## Database upgrades and rollback

Database migrations are ordered, forward-only, and transactional. Before
starting a build whose current database schema is newer than the deployed
build's, an operator must create and integrity-check an online backup and record
the old image digest. Startup completes migrations and active-match recovery
before binding the listener. A schema newer than the binary supports aborts
startup and readiness; it is never automatically downgraded.

Rollback within an unchanged database schema may reuse the volume only when the
release notes explicitly say the old build is compatible. Rollback across a
schema increment means stopping the candidate and restoring the pre-upgrade
backup before starting the old image. Never run two versions against one SQLite
volume. The exact operational commands are in
[`server-operations.md`](server-operations.md).

## Deprecation and release requirements

- Compatibility promises are conservative and listed in the matrix, not
  inferred from semantic application versions.
- Removing a supported source reader requires advance release-note notice and a
  migration path to a still-supported version. Protocol versions are not
  served concurrently unless that support is explicitly implemented and tested.
- Each release note lists all six independent file/wire versions plus the
  database schema, supported upgrade sources, required backup, and rollback
  constraints.
- Release-candidate verification tests the exact packaged client and server
  image, all supported migration fixtures, wrong-version failures, database
  backup/upgrade/restore, and canonical hashes after recovery.
