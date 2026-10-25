# Plumb operations notes

## Store durability profile (Fable #199, architecture §33.9)

Every read-write store connection (revision, workflow, stage and artifact stores over the one
pilot SQLite database) is configured with:

| Pragma | Value | Why |
|---|---|---|
| `journal_mode` | `WAL` | Readers never block the single writer; set once by the first read-write open. |
| `synchronous` | `FULL` (2) | The WAL is synced on every commit, so a committed revision, workflow transition or artifact survives an OS crash or power loss. There is no power-loss window. |
| `foreign_keys` | `ON` | Referential integrity of every store table. |
| `busy_timeout` | 5 s (revision, workflow and stage stores) | Concurrent writers wait for each other; a writer still blocked after the timeout fails with `E_DATABASE_BUSY` (503). |

The profile is frozen: `plumb-store` (`SYNCHRONOUS_FULL`, `Durability`) and `plumb-artifacts`
(`SqliteArtifactStore::synchronous`) tests assert the configured pragmas.

## Open modes (architecture §33.9)

- Only `plumb init` creates a database and its schema. Every other command opens an existing
  database; a missing, zero-byte, non-SQLite or foreign database, or one without the PLUMB
  tables, is `E_DATABASE_NOT_INITIALIZED` and is never modified.
- An older store schema is migrated in place by the first read-write open (store schema v4 keys
  workflow proposals by occurrence, H0.10; store schema v5 adds the terminal proposal state
  `abandoned`, H0.11).
- `vocab`, `questions`, `rounds show`, `export`, `report`, `proposals` (without `--refresh`) and
  `findings list` open the database read-only: SQLite read-only access, no journal-mode change,
  no worker, no recovery or reconciliation writes. Gate reports these commands evaluate on demand
  are computed in memory and not persisted. They work on a read-only database file; a write
  command on such a file fails with `E_DATABASE_READ_ONLY`.
