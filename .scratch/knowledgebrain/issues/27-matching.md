# 27 — Matching 按条款荐产品

**What to build:** 传入一组招标条款，得到按覆盖率排序的产品候选，每条条款带证据 hits；没有「唯一推荐产品」字段。

**Blocked by:** 12 — Review：分块与向量对照 brain；24 — Review：library 与 TAG

**Status:** done

- [x] `mode=matching` 或 `POST /match` 接受 `requirements[]`
- [x] `version_scope=current|all_active`；`include_library` / 条款 `use_library`
- [x] 输出 candidates：score、coverage、unmet_must、matched_version_id、每条款 hits
- [x] 无 `best_product_id`；hit 带 product_id+version_id
- [x] embedding 不一致 → EMBEDDING_MISMATCH
- [x] 条款 hit 须过向量 0.15 / 关键词 0.3

## Comments

- reality: 内存 `matching` 仍可用。内存没有条款命中时，`POST /match` 与 `mode=matching` 走 `matching_pg`（同一套 `hybrid_search_pg` + 0.15/0.3）。JSON 无 `best_product_id`。
- 召回向量仍是 `stub_embed`。http_flow 覆盖内存路径；PG 路径有 `matching_pg_returns_candidates_without_best_product`。
- review 2026-08-15: 无 best_product_id 通过。graph expand 固定 0.2 曾可绕过阈值——代码侧已收口。
- 7 检索收口：查询向量走 `embed_index(query, version.embedding_model_id)`（HTTP 失败不再回 stub）。PG assembly 校验目标 embedding 一致；`version_id=current` 且无 current → VALIDATION。内存 graph 分用关键词分不再写死 0.2。同 `chunk_id` 融合取高分。PG 关键词通道用 `ts_rank_cd`。Hit 回 `tag_slugs`。matching 硬顶 50 **产品**（不是 50 版本）；library 目标也进 embedding 一致性检查。
