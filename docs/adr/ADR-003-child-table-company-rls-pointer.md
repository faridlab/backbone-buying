# ADR-003: Why `company_id` is NOT NULL on every buying child table (pointer)

**Status**: Accepted — Added 2026-07-28 (in-module pointer; the decisions themselves live in the framework handbook)
**Deciders**: Farid (owner), council run 2026-07-28 (bounded-context-cleanliness)
**Related**: framework handbook [ADR-0008 — company read-fence via RLS](../../../docs/handbook/adr/0008-company-read-fence-via-rls.md), [ADR-0010 — child-table RLS and catalog tenancy](../../../docs/handbook/adr/0010-child-table-rls-and-catalog-tenancy.md), buying [ADR-001](ADR-001-buying-boundary.md), council [2026-07-28-module-backbone-buying-bounded-context-cleanliness](../council/2026-07-28-module-backbone-buying-bounded-context-cleanliness.md)

## Context

A reader inside this module sees `company_id UUID NOT NULL` on every child table
(`material_request_items`, `purchase_order_items`, `rfq_items`, `rfq_suppliers`,
`supplier_quotation_items`) plus a `company_id = NULLIF(current_setting('app.company_id', true),...)::uuid`
RLS policy on each — but the **decisions that put it there are not held in this module's `docs/adr/`**
(only ADR-001 boundary and ADR-002 receipt-seam live here). They are cross-module framework ADRs. This
file is the local signpost so the tenancy model is legible without leaving the module.

## Decision

This module follows — it does not re-decide — the two framework tenancy ADRs:

1. **ADR-0008 (parent fence).** Every *parent* buying table (`material_requests`, `purchase_orders`,
   `request_for_quotations`, `supplier_quotations`) carries `company_id` + a `FORCE ROW LEVEL SECURITY`
   policy scoped via `set_config('app.company_id', <uuid>, true)`. An unset var sees zero rows.
2. **ADR-0010 (child collapse).** The five child tables get a **direct** `company_id` (not a JOIN to the
   parent) so each child row is tenant-isolated by construction. Like every other catalog/billing/selling
   child, `company_id` is a **logical** FK only — no hard FK to `organization.companies` — so modules stay
   independently deployable. RLS is the fence, not the FK.

**Single source of truth:** `schema/models/*.model.yaml` declares `company_id` on each child entity
(verified for all five on 2026-07-28); the codegen pipeline materializes it. Migration
`20260722000000_child_tables_company_rls` is the hand-written forward step that adds the column, backfills
from the parent, sets `NOT NULL`, and fences it (paired `.up.sql` / `.down.sql`).

## Consequences

- **Read the handbook for the rationale**, not this file. This pointer exists only so the tenancy model
  is discoverable from inside the module.
- **Do not edit `company_id` out of a child table without updating the schema YAML first** — the next
  regeneration re-emits the entity, DTOs, and (for new tables) the CREATE migration from the YAML.
- **Historical migrations are immutable.** A 2026-07-28 council found that folding ADR-0010 *backward*
  into the original April CREATE migrations risks a sqlx `ChecksumMismatch` boot-brick on any DB that
  already applied them; ADR-0010 stays a discrete forward step. See the linked council report.
- **Migrations are immutable under `schema generate` (metaphor-schema ≥ 0.5.6).** The generator now
  never overwrites or deletes an existing migration — not even under `--force` — so a schema change to
  an existing table lands as a NEW forward migration (`migration generate`), as the July
  `child_tables_company_rls` migration does for `company_id`. To regenerate migrations from scratch,
  delete `migrations/` first. (Before 0.5.6, a full `schema generate --force` would fold schema changes
  back into the original CREATE and delete "stale" `.down.sql`s, breaking sqlx's checksum contract —
  the 2026-07-28 council caught this; fixed in `metaphor-plugin-schema` 0.5.6.)
