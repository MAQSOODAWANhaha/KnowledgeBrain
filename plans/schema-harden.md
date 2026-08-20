# Schema harden (CREATE-only)

## Context

Migrations are already squashed into CREATE (`0001`–`0007`) plus one-shot `0008` backfill. User confirmed **no upgrade path** — compose volumes can be wiped and redeployed.

Review leftovers that we will actually change:

1. `product_versions` hard `UNIQUE (product_id, label)` vs soft-delete (API 201 then PG `23505`).
2. A small set of FK / CHECK / NOT NULL holes that already bite existing delete/insert paths.

Out of scope (already decided): ALTER shim, live-column renames, drop `deviate`, `unit_id` → `bid_sections` FK, GIN/perf indexes, dead columns (`vlm_model_id`, `parent_slug`).

## Approach

**#2 — yes, live partial unique, and make the API actually able to reuse the label.**

Documents already use `documents_live_file_uidx … WHERE deleted_at IS NULL`. Versions should match.

SQL alone is not enough: `create_version` 409s on any in-memory row with that label (`crates/api/src/routes.rs` ~1426), and `delete_version` never marks the in-memory version archived (worker only sets `deleted_at` in PG). Same process would still 409 after delete. So:

- DROP table `UNIQUE (product_id, label)`.
- Add `product_versions_live_label_uidx` on `(product_id, label) WHERE deleted_at IS NULL`.
- `delete_version`: set `status = Archived` in memory; if it was `current_version_id`, clear it.
- `create_version` conflict: only live (non-`Archived`) labels.

**#3 — harden deletes and enums; bid→KB FKs follow existing delete order.**

`delete_empty_product` hard-deletes archived documents, then archived versions, then the product. Bid rows that point at those KB ids must not block that path.

| Change | Why | ON DELETE |
|---|---|---|
| `document_processing_spans.document_id` → `documents` | housekeep joins by bare id; hard-delete leaves orphans | CASCADE |
| `document_tags` both FKs | tagged hard-delete fails; caller swallows | CASCADE |
| `documents.type` CHECK `file\|url\|passage\|manual` | API writes exactly these four | — |
| `enable_status` CHECK `disabled\|enabled` | only values written | — |
| `summary_status` CHECK `none\|pending\|processing\|completed\|failed` | `SummaryStatus` | — |
| `file_name/size/hash/object_key` NOT NULL | constructors + `insert_document` always set them; hydrate already `try_get::<String>` | no default `''` (would collapse the live-file unique) |
| `bid_picks.product_id` / `version_id` | required uuids, currently unconstrained | CASCADE (so empty-product cleanup still works) |
| `bid_shots.product_id` / `version_id` | same | CASCADE |
| `bid_shots.kb_document_id` | optional KB ref | SET NULL |
| `bid_commercial_hits.{document_id,version_id,product_id}` | optional hit payload | SET NULL |

Skip `unit_id` FK (nil / NULL sentinels). Skip `cloned_from_version_id` / wiki folder FKs / `chunk_type` CHECK (no current delete bug).

**`0008` backfill:** leave it. Empty DB is a no-op (`schema_flags` insert only). Not worth a second apply-path edit.

## Files to modify

- `migrations/0001_domain.sql` — version unique, spans/tags FKs, document CHECKs + NOT NULL
- `migrations/0007_bid.sql` — bid→KB FKs
- `crates/api/src/routes.rs` — `delete_version` archive in memory; `create_version` live-label check
- `crates/storage/src/persist.rs` — add persist test mirroring `summary_append_keeps_text_and_soft_delete_frees_unique` for version labels
- `docs/system-design.md` — version uniqueness sentence: live partial unique, same as documents

## Reuse

- Live unique pattern: `documents_live_file_uidx` in `migrations/0001_domain.sql`
- Enums: `crates/domain/src/status.rs` (`SummaryStatus`, `ParseStatus` already CHECK'd)
- Doc types written in `crates/api/src/routes.rs` (`file` default, `url` / `passage` / `manual`)
- Hard-delete order: `delete_empty_product` in `crates/storage/src/persist.rs`
- Version archive in PG: `process_kb_delete_pg` in `crates/worker/src/consume.rs` (leave as-is)
- Bid inserts: `upsert_pick` / shot insert in `crates/storage/src/bid.rs` (no SQL text change if CREATE FKs match existing columns)

## Steps

- [ ] `0001`: remove `UNIQUE (product_id, label)`; add `product_versions_live_label_uidx`
- [ ] `0001`: `document_processing_spans.document_id REFERENCES documents (id) ON DELETE CASCADE`
- [ ] `0001`: `document_tags` both FKs `ON DELETE CASCADE`
- [ ] `0001`: `documents.type` / `enable_status` / `summary_status` CHECKs; four file identity columns `NOT NULL`
- [ ] `0007`: bid_picks / bid_shots CASCADE to products + product_versions; optional KB refs SET NULL
- [ ] `delete_version`: `v.status = Archived`; clear `current_version_id` if it pointed here
- [ ] `create_version`: conflict only when `v.status != Archived`
- [ ] Persist test: insert version → set `deleted_at` → insert same `(product_id, label)` succeeds
- [ ] Docs: `product_versions` uniqueness line

## Verification

- `cargo check -p storage -p api`
- `cargo test -p storage --lib summary_append_keeps_text_and_soft_delete_frees_unique <new_version_label_test> -- --exact` against a **wiped or throwaway** DB (do not DROP the live 15432 volume unless user already redeployed)
- Manual after wipe: create version `v1` → delete → create `v1` again → 201, not 409/`23505`
- Manual: upload tagged file, archive last version so `delete_empty_product` runs — must not fail on `document_tags` / spans
- Manual: pick a product on a bid, then delete that product’s last version — picks/shots for that version disappear (CASCADE), project remains
