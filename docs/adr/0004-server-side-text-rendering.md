# 0004 — Values are rendered to text by the server on the read path

## Status

Accepted. Implemented for table browsing.

Update 2026-11-16: editing now sends values as text parameters as anticipated below — cast to the column's bare type on PostgreSQL, uncast on MySQL (except `BIT`) — and compares these text renderings for optimistic concurrency; the per-type validation before sending was not built (the server validates). See [ADR 0013](0013-staged-edits-and-optimistic-saves.md).

Update 2026-11-16 (SQL console): the open question under "Consequences" about arbitrary SQL results is answered. Query tabs send statements over the simple/text protocol on both engines, MySQL included, so the server renders every value as text there too; no driver-native decoding was added. Differences from the browse path: nothing is truncated on the server (the backend keeps full values for export and cuts cells to 1,000 characters for display only), binary values are shown in full as `0x` plus lowercase hex on both engines, and MySQL spatial columns are classified as binary, so they are shown as hex rather than WKT (from the code; not run against a server). See [ADR 0015](0015-sql-console.md).

## Context

A data grid must show what is stored, exactly. Decoding every database type into a Rust type and then into JSON for the web view loses information: JavaScript numbers cannot hold 64-bit integers or high-precision decimals, and each engine has a long tail of types (arrays, enums, ranges, spatial, intervals) that would each need a decoder.

## Decision

When browsing a table, every projected column is converted to text in the `SELECT` itself:

| Engine | Ordinary column | Binary | Special cases |
|---|---|---|---|
| MySQL | `LEFT(CAST(col AS CHAR), 1001)` | `HEX(LEFT(col, 25))` | `BIT`: `CAST(col + 0 AS CHAR)`; spatial types: `LEFT(ST_AsText(col), 1001)` |
| PostgreSQL | `left(col::text, 1001)` | `encode(substring(col from 1 for 25), 'hex')` | — |

- Cells travel as `string | null`; `null` is SQL NULL, so NULL and the empty string stay distinct.
- Previews are limited to 1000 characters of text and 24 bytes of binary. The query asks for one unit more, which is how truncation is detected and flagged.
- Column metadata carries a coarse `ValueKind` (number, text, bool, temporal, binary, json, other) for alignment and, later, editors.

## Alternatives considered

| Option | Why not |
|---|---|
| Driver-native decoding into Rust types | Large per-engine type matrix; lossy or failing on unsupported types; numbers still need string transport to the web view |
| Server-side JSON (`row_to_json`, `JSON_OBJECT`) | Engine-specific quirks, numeric precision issues in JSON, awkward binary handling |
| Fetch full values and truncate in Rust | Transfers large text and BLOB values only to discard them |

## Consequences

- Display is exact for the tested types: `DECIMAL(24,6)`, `BIGINT` beyond 2^53, `datetime(6)` / `timestamptz`, JSON / `jsonb`, BLOB / `bytea`, `BIT`, arrays, `uuid`, enums, Thai text.
- Formatting follows the session: time zone and date style come from the server or role defaults, and the two engines differ (hex case, `1` vs `true` for booleans).
- A cast in the projection is cheap, but the full value of a truncated cell is not available; a cell inspector must issue a second query (Phase 2b).
- **Editing (Phase 3)** will send values as text parameters with an explicit cast to the column type, and needs per-type validation before sending.
- **Arbitrary SQL results (Phase 4)** cannot be rewritten this way. That path needs driver-native decoding or, for PostgreSQL, the simple-query text protocol. Open question.

## Licensing

No third-party component is introduced by this decision.
