# 0006 — Data grid: custom grid on TanStack Virtual

## Status

Accepted for the read-only grid (implemented). **To be re-validated in Phase 3** before any editing work builds on it.

Update 2026-11-16: the custom grid now supports editing, single-range selection and paste ([ADR 0013](0013-staged-edits-and-optimistic-saves.md)); the performance and multi-range validation described under "Phase 3 gate" is still open.

## Context

The grid is the flagship surface. Phase 2 needs fast read-only browsing. Phase 3 needs editable cells, multi-range selection, clipboard paste from spreadsheets, staged-change highlighting, column reorder/hide and pinned key columns. The brief forbids relying on paid-only features without a licensing decision.

## Decision

Build the grid in the repository on `@tanstack/react-virtual` 3 (row virtualization) with ARIA grid roles.

Implemented now: sticky header, sort by header click (ascending, descending, default), column resize, single-cell selection with arrow / Page / Home / End keys, copy cell and copy row, NULL vs empty rendering, truncated-cell markers.

Not implemented: column virtualization, multi-range selection, paste, editing, reorder, hide, pinning.

## Alternatives considered

| Option | Assessment |
|---|---|
| AG Grid Community (MIT) | Mature, but range selection and clipboard paste are Enterprise-only features, which are exactly what Phase 3 needs. Using it would require a product decision to buy an Enterprise licence |
| Glide Data Grid 6.0.3 (MIT) | Canvas rendering, fast, built-in range selection and paste. Its `peerDependencies` declare React 16–18 only; PuriCat uses React 19 |
| TanStack Table + Virtual | Adds a table model, not selection or clipboard; little gain over the current code |

## Consequences

- No licence cost and full control over NULL handling, exact text values and staged-change rendering.
- Selection, clipboard and editing must be written and tested in-house; this is the main schedule risk of Phase 3.
- DOM rendering with row-only virtualization is unproven for very wide tables and large pages. No performance numbers exist yet.

## Phase 3 gate

Before building editing, run a time-boxed spike that measures the custom grid on: multi-range selection, rectangular paste, column virtualization, and scrolling with 100k+ rows loaded.

- Pass: continue with the custom grid.
- Fail: switch to Glide Data Grid, either by pinning React 18 or by waiting for React 19 support; or take the AG Grid Enterprise question to the product owner ([ROADMAP.md](../../ROADMAP.md), blockers).

## Licensing

TanStack Virtual MIT. AG Grid Community MIT, Enterprise commercial. Glide Data Grid MIT.
