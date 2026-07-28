<!--
date: 2026-07-28 | repo-type: module | unit: backbone-buying | focus: bounded-context-cleanliness
roster: chair, skeptic, steelman, yagni-business (standing) · ddd-bounded-context, contract-seat (module context) · domain-expert (invited — buying encodes real P2P domain rules)
trigger: post-0.3.x schema regeneration review; regen folded ADR-0010 child-table company_id + RLS back into original April migrations and deleted the July .down.sql
-->

# Council — module:backbone-buying — focus: bounded-context-cleanliness

## Best call
Unwind the fold: drop the regen's content changes to the 6 already-shipped April migrations (20260426220002/04/06/07/09 + the 5 policy appends to 20260426220010), keep July's `202607…` migration as the discrete forward step that actually lands ADR-0010's child RLS, and pair its missing `.down.sql`. Leave the April files immutable from here on; schema YAML governs FUTURE regens only. Treat the BuyingCrudServices type-narrowing and the local-ADR pointer as separate, smaller follow-ups (below), not part of this move.

- Residual negative value: ~0 functional. The 6 April files retain pre-regen wording and stay cosmetically out of sync with the YAML SSoT; July already materialized the correct schema on every DB that ran it, so no query/index/RLS shape changes. Cost = a future dev reading an April file sees pre-company_id wording; bounded by ADR-0010 (in the framework handbook) and by YAML-as-SSoT. Unwind itself is minutes of work (working tree is uncommitted — just don't commit the April-rewriting hunks).
- Reversibility: easy. The regen is uncommitted; nothing to revert in history. If fresh-DB-only is later proven, re-applying the April rewrites is a one-command cherry-pick.
- What would flip this: verified evidence that zero non-fresh DBs exist (pre-launch, fresh-schema-only project) — in that world the fold is free SSoT alignment. Note the cheap probe (git stash regen → throwaway pg with HEAD migrations applied → stash pop → `sqlx migrate run`; ChecksumMismatch on 20260426220002 = non-fresh-DB hazard confirmed) would resolve it, but cannot flip the recommendation: unwind dominates under every state of the world (worst case ~0 cosmetic drift vs. the fold's worst case = boot brick on every non-fresh DB).

## Disagreement map
1. The fold (steelman FOR vs. skeptic + yagni-business AGAINST). Crux: do non-fresh DBs exist? Resolved from the seats — yes. yagni-business concedes "July already produced correct schema everywhere" (July cannot apply without April having applied first), and the April files are 3 months old at the 2026-07-28 date. Non-fresh DBs are the base-rate reality. The steelman's safe-fold assumption is false.
2. "Guarded/idempotent SQL makes the fold safe" (steelman) vs. "checksum mismatch halts before any SQL executes" (skeptic). Crux: sqlx 0.8 checksum semantics. Resolved — `sqlx 0.8` with the `migrate` feature (Cargo.toml:37) stores a SHA-384 per applied migration and errors `ChecksumMismatch` on file-content change; it does not skip. The steelman defended against the wrong threat (statement-level idempotency) — irrelevant once the runner refuses to execute.
3. "BuyingCrudServices cleanly relabels raw services as read/seed" (steelman) vs. "the type name lies — it exposes full CUD handles that let siblings bypass BuyingWriteService invariants" (contract-seat). Crux: the exposed type surface. Resolved — `Arc<GenericCrudService<…>>` carries create/update/delete; a sibling can mutate documents around the 3-way-match watermarks and mark_received/mark_billed scoping. This is a context-boundary leak via the type system, not a naming nit.

## Recommendations (ranked by leverage)
| # | Move | Leverage | Residual negative | Reversibility | Evidence to flip |
|---|------|----------|-------------------|---------------|------------------|
| 1 | Unwind the fold: revert the 6 April migration content rewrites; keep July as the discrete forward step; pair July's `.down.sql`. | high — removes a boot brick on every non-fresh DB (dev/staging/prod) and removes the undocumented per-env `UPDATE _sqlx_migrations SET checksum=…` surgery on 6 rows. | ~0 functional; April files stay cosmetically drifted from YAML SSoT. | easy (uncommitted) | Proof of fresh-DB-only project; then fold is free to re-apply. |
| 2 | Re-pair the July migration with a symmetric `.down.sql` (drop child policies / company_id columns in reverse order). | high per effort — restores `sqlx migrate revert` on fresh AND non-fresh DBs; today the orphan `.up.sql` makes revert impossible and silently masks ADR-0010 failing to land on existing DBs. | tiny — authoring one `.down.sql`. | easy | None — the unpaired `.up.sql` is an objective defect. |
| 3 | Type-narrow `BuyingCrudServices`: expose read repositories / a `ReadServices` view (or type-narrow to a read trait) instead of `Arc<GenericCrudService>` handles; alternatively rename the type to match its real full-mutation surface. | medium-high — closes the bypass path around 3-way-match watermarks, mark_received/mark_billed scoping, and event emission. | small — either a read-trait wiring pass or an honest rename; behavior-preserving. | easy | A demonstrated legitimate external need for sibling-side mutation of these documents (none surfaced). |
| 4 | Mirror ADR-0008 / ADR-0010 (or a local ADR-003 pointer) into `backbone-buying/docs/adr/` so in-module readers see WHY `company_id` is NOT NULL on every child table. | low-medium — doc-only; closes the "reader can't see the tenancy rationale" gap the context seat flagged. Model is already correct. | trivial. | easy | None — pure documentation addition. |

## Parking lot
- Purchase RETURN (reduce `received_qty`) and CREDIT NOTE (reduce `billed_qty`) — the monotonic-watermark model cannot represent them. Raised by domain-expert. Scope: future P2P domain-model enhancement at root, not a bounded-context-cleanliness item. Already deferred per "council 2026-07-05"; keep as a named, intentional scope cut. **→ RESOLVED 2026-07-28: implemented in [ADR-004](../../adr/ADR-004-reverse-watermarks.md) (`mark_returned` / `mark_credited`, shipped in backbone-buying 0.3.3).**
- Over-receipt tolerance — raised by domain-expert; same deferral bucket as above.
- SSoT-propagation / cross-module cargo-dependency posture — touched by steelman (zero hard deps in normal build, proven by `tests/receipt_seam.rs`); already verified clean by the orchestrator, no action under this lens.

## Resolution log (2026-07-28)

- **#1 Unwind the fold — DONE.** Reverted the regen's content rewrites to the 6 April migrations
  (`20260426220002/04/06/07/09` child CREATEs + `20260426220010` up/down) and the seed templates via
  `git checkout HEAD -- migrations/`. April migrations are immutable again; no sqlx `ChecksumMismatch`
  hazard on non-fresh DBs.
- **#2 Re-pair the July migration — DONE (via the unwind).** `20260722000000_child_tables_company_rls.down.sql`
  was restored from HEAD; the July pair (`.up.sql` + `.down.sql`) is intact and `sqlx migrate revert` works.
- **#3 Encapsulate the generated CRUD surface — DONE (generator-level, follow-up session 2026-07-28).**
  The active generator `metaphor-plugin-schema/src/generators/module.rs` (the `module` target that emits each
  `{Domain}Module`) was emitting `pub {snake}_service: Arc<GenericCrudService>` fields **and** a full-mutation
  `{Domain}CrudServices` / `crud_services()` accessor — both reachable by sibling crates, bypassing the
  validated write service. Fix landed: (a) service fields are now `pub(crate)` (sibling crates cannot reach
  them; same-crate handlers / write-service still can via `self.`); (b) the `{Domain}CrudServices` struct and
  `crud_services()` accessor were removed entirely (unconsumed, full-mutation, mislabeled as read/seed). The
  sanctioned surfaces are now: cross-module mutation → a hand-authored `{Base}WriteService`; unguarded CRUD →
  `all_crud_routes()` (HTTP only), never a programmatic CUD handle. Verified: full generator suite **369 tests
  green**; backbone-buying regenerated (only `src/lib.rs` changed, CUSTOM blocks preserved); buying integration
  suite **19 tests green** against the encapsulated lib. NOTE: the fix is in the generator **source**; the
  shipped `~/.metaphor/bin/metaphor-schema` binary must be rebuilt + released (then `metaphor plugin add`) for
  the change to reach other modules' future regens.
- **#4 Local ADR pointer — DONE.** Added `docs/adr/ADR-003-child-table-company-rls-pointer.md` signposting
  framework ADR-0008/0010 so the `company_id`-on-every-child tenancy model is legible from inside the module.
