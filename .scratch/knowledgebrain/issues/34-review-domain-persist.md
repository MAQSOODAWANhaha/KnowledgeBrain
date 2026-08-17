# 34 — Review：0001 对照规格 §2.1

**What to build:** 对照 `docs/system-design.md` §2.1，确认表、约束、默认 library 没有做成 TAG 或知识空间。

**Blocked by:** 33 — 领域表 0001 与默认 library

**Status:** done

## Gate

命令见 `.scratch/knowledgebrain/review.md`。标 `done` 前必须跑通本票触及栈的 fmt / lint / test（CI 同命令）。未跑通不得标 done。


- [x] `products.kind` CHECK；`(workspace_id, slug)` 唯一；默认 library 由 persist 插入
- [x] documents UNIQUE (product_version_id, file_name, file_size, file_hash)
- [x] 无 quota / tenant_id / billing 列
- [x] 偏差已记明：HTTP 仍以内存 Store 为热路径；缺实体时 `hydrate_workspace` 灌回，检索/匹配内存空则走 PG

## Comments

- §2.1 表已齐（含 0002–0007）。chunk/embedding 可 `replace_document_chunks`；HNSW + GIN(tsv)。
- `hydrate_workspace` 灌 workspace / members / products / versions / documents，以及 tags、document_tags、chunks、embeddings、graph nodes/relations、wiki pages。
- `/search` 内存空则 `assembly_pg`；`/match` 内存无命中则 `matching_pg`。
