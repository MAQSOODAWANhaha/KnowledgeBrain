# D：知识管线去掉生产 `Store` — 未完成清单

停在 2026-04-11。不要从内存 `drain` / `http_flow` 续；那些已经删了。

目标仍是 `plans/architecture.md` §9 **D**：生产知识 API 只走 `platform::connect()` → Postgres，不再整仓 `hydrate_workspace`。

## 已落地（不要回退）

| 段 | 内容 |
| --- | --- |
| A+B+C | 文档对齐、三步 Web、7 crate |
| D1 | worker consume 只走 `knowledge::pipeline(pool, …)` |
| D2 | 生产 `AppState` 无进程级 Store；**`test_catalog` / `http_flow` / `worker::drain` 已删**（测假 catalog 没用） |
| D3 | job hydrate 收成 `hydrate_document` / `hydrate_version` |
| D4 | 算法走 `DocJob` / `WikiJob`（summary/questions/image/graph/wiki） |
| D5 读路径 | 工作区/产品/文档列表、检索 `assembly_pg` / `matching_pg` |
| D5 写路径（部分） | 改/删工作区、成员列表、建/读/改/删产品、版本列表/详情/创建/删除/`set_current`、文档详情/删/重解析/取消 |

已有 persist 帮手（接着用，别再整仓 hydrate）：

- `list_workspaces` / `load_workspace` / `update_workspace_name` / `retire_workspace`
- `list_products_in_workspace` / `load_product` / `product_slug_taken` / `insert_product` / `update_product_name`
- `list_documents_in_version` / `load_document` / `insert_document` / `set_parse_status` / `mark_reparse_queued`
- `list_versions_for_product` / `load_version` / `resolve_product_version_id` / `insert_version` / `insert_version_cloning` / `set_version_status` / `set_product_current` / `clear_product_current_if` / `version_label_taken` / `workspace_embedding_conflict`
- `knowledge::put_bytes`（对象不必再写 `Store.objects`）

`require_ws` 目前永远 `Owner`。SQL 化时继续用 `Store::default()` + `require_ws(&dummy, …)` 即可，不要为 ACL 重新 hydrate。

## 还没改（`crates/api/src/routes.rs`）

`ensure_workspace` 约 17 处、`ensure_product` 约 13 处、`ensure_document` 2 处（含帮手自己）。`hydrate_workspace` 只剩 `ensure_workspace` 内部那一次。

按优先级：

### 1. ingest（进行到一半）

`ingest_file` / `ingest_url` / `ingest_passage` / `ingest_manual` 仍 `ensure_product` + `insert_pending(&mut Store)`。

做法：抽 `ingest_prepared(actor, product_id, version_id, title, file_name, bytes, tags, overrides, doc_type)`：

1. `pg()` → `load_product` → `require_ws` → `resolve_product_version_id` → `load_version`
2. `status == Active`；`is_frozen_default_library`
3. `resolve_process_config`（已 `pub`）做图/ASR 校验
4. `put_bytes`；`find_duplicate_document`；tag 校验（需补 `tags_belong_to_workspace`）
5. `Document::new` + `persist_ingest_row`
6. csv/xlsx → `enqueue_datatable`；file → `enqueue_document_process`；passage → `enqueue_document_process_with`；manual → `enqueue_manual_process`
7. 失败只 `set_parse_status(..., "failed")` + `persist_failed_row`，不要 `lock()` 回写内存 Store

然后 `insert_pending` / `rollback_ingest` / `push_document_job` 可以删。

### 2. `patch_version`

仍 `ensure_product` + 改内存 `ProductVersion` 再 `update_version_config`。改成 `load_version` + 改字段 + 现成 `update_version_config`。

### 3. 成员 / retrieval

`add_member` / `patch_member` / `remove_member` / `get_retrieval` / `patch_retrieval`：已有 `insert_member` / `delete_member`。补 list/update retrieval SQL，不要 hydrate。

### 4. 文档周边

| handler | 改法 |
| --- | --- |
| `document_content` | `load_document` + 对象字节（不要整仓） |
| `timeline` | 已有 `list_spans` / `list_spans_attempt` |
| `list_tags` / `create_tag` / `delete_tag` / `put_tags` | 已有 `insert_tag` / `delete_tag` / `replace_document_tags`；补 list |
| `wiki_pages` / `wiki_page` / `wiki_folders` | persist 已有 wiki list；按 version 查 |
| `version_file` | `load_product` + `resolve_product_version_id` + 对象 |

### 5. 检索残留

- `hydrate_search_workspace`：`do_search` 已走 PG，这个函数若无引用就删
- `do_answer`：对照 `assembly_pg`，改 PG，不要 `ensure_product`

### 6. 收尾才删

全部 handler 不再调用后再删：

- `ensure_workspace` / `ensure_user_workspaces` / `ensure_product` / `ensure_document`
- `Catalog` / `lock()` / `merge_catalog`
- `knowledge::hydrate_workspace`（`hydrate_workspace_index` 可留给 `hydrate_document`/`hydrate_version`）
- 生产路径上的 `Store::default()` 请求袋

pipeline 里 job 仍可 `hydrate_document` → `DocJob` → persist，那不是整仓。

## 不要做

- 不要恢复 `test_catalog` / `http_flow` / `worker::drain`
- 不要为了单测再给 wiki/enrichment 加 `&mut Store` 包装
- 不要一次改 60 个 handler；按上面 1→6
- 忽略 pi-lens 60s `cargo test` 超时（编辑器自动跑，不是产品失败）

## 验收

```text
cargo check --workspace
```

D 完成当：`routes.rs` 生产路径 0 次 `hydrate_workspace` / `ensure_workspace`，ingest 不碰 `insert_pending(&mut Store)`。
