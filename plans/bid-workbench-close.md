# 投标工作台一次收口

| 项 | 值 |
|---|---|
| 状态 | 已落地 |
| 对照 | `plans/bid-booklet-preview.md` + 第一轮审阅 |
| 不做 | ⑥、买方套红、一条参数一个产品、解析 MD 反推勾选、新编辑器 npm 包、把旧全书 Pick 复制到每一段 |

## Context

第一轮审阅：分段匹配、先商务、成稿分册这些骨架在。落地把批准的「一张工作台」拆成条款 / 勾选 / 预览三页，并且导出、人评、漏锚、未归段和商务 hit 都和拍板拧着。

这次按批准过程一次改完：后端契约先正，再换工作台，不再叠补丁页。

## Approach

一张标一个工作台。侧栏是商务 / 勾选段 / 未归段 / 成稿；主列是 `[表 | 文稿]`；检查器是本段候选或去补证。导出默认渲人稿。建议列只现算检索结果。must 锚只认各篇②。

手补跟当前侧栏：商务页 → `family=commercial`；技术段 → 该 `section_id`；未归段 → `section_id=NULL`。

旧 0014 把全书 Pick 摊到第一个锚段。不再做 0015 复制到各段（会让存储段误 cover）。已有标靠该段重勾。

## Files to modify

- `crates/bid/src/lib.rs` — `coverage_for`、debounce、`ClauseView` 建议列、match stale
- `crates/bid/src/booklet.rs` — ②-only 锚、③ 去锚、产品名、侧栏序、① `expires_at`、ended 锁重生成、禁 HTML/YAML
- `crates/bid/src/export.rs` — `pulldown-cmark` 渲 MD；锚旁 BidShot + `objects/…`
- `crates/bid/Cargo.toml` — 加 `pulldown-cmark`
- `crates/api/src/routes.rs` — meet 门闩、手补 `section_id`、units/未归段、pick 标 ③ stale、picks 必须带 `unit_id`、ended 锁 regen
- `crates/storage/src/bid.rs` — `insert_clause` 已有 `section_id`；units/hits 查询按需补
- `web/src/App.tsx` — 路由只留 `#/bids/:id`（兼容旧 hash 跳转）
- `web/src/bid/` — 新建工作台（Workbench / Sidebar / ClauseTable / Inspector / BookletPane）
- `web/src/api.ts` — hits、merge、endBid、导出文件名、删死 `preview`
- `web/src/hash.ts` — `#/bids/:id` + `?view=` / `#/bids/:id/booklet/:key`
- `DESIGN.md` / `PRODUCT.md` — 改成批准 IA
- `docs/bid-platform-domain.md` + `.scratch/knowledgebrain/spec.md` — ① 含截止日；`cmp` 仍过

## Reuse

- `bid::resolve_unit` / `section_merge_map` / `unsectioned_unit`（`crates/bid/src/lib.rs`）
- `storage::bid::list_commercial_hits`、`list_picks_for_unit`、`latest_match_debounce`、`mark_booklet_stale`、`set_section_merge`
- `storage::product_name`（① 已用，② 同样用）
- `booklet::clause_anchor` / `missing_must_anchors` / `ensure_all_parts` / `save_part`
- `export::pic_from_bytes` / `build_export_docx` 的嵌图路径
- `POST /sections/{sid}/merge`（已有环检查）
- `api.endBid`、`downloadExport` 的 `{ error.message }` 解析
- `Shell` 窄屏抽屉

## Steps

### 1. 建议列与人评门闩

- `coverage_for` 只返回 `pending | need_rematch | cover | unmet | uncovered`。人评不进建议列。
- `ClauseView` 加只读 `suggestion`。商务条款 `suggestion` 空，另加 `hit_outcome` / `hit_file`（来自 `list_commercial_hits`）。
- `GET /clauses` 一次带上建议和商务 hit，工作台不再打 `/preview`。
- `PATCH`：建议 `unmet` 时拒绝 `assessment=meet`（409）。允许 `partial|deviate|fail`。人评变仍标该段② + ③ stale。

### 2. 勾选段清单

`GET /units` 固定三类，顺序：

1. `{ kind: "commercial" }` 商务
2. 仅含**技术条款**的未并 `BidSection`（`technical_count > 0` 或作为锚被并入）。纯商务标题段不进技术树。
3. `{ kind: "unsectioned", id: "00000000-0000-0000-0000-000000000000" }` 未归段（始终有，手补入口）

每条技术单元带 `prev_id`（侧栏上一条锚），给「并入上一段」。

`expected_part_keys`：只为「有已确认技术条款」的单元生成②；序 = 上列侧栏序，禁止 `units.sort()` UUID。无确认技术则没有空②，仍有 ①③④⑤。

### 3. 手补进当前段

`POST /clauses` 收 `section_id?` + `family`。

- 商务页：`family=commercial`，`section_id=null`
- 技术段：`family=technical`，`section_id=当前锚`
- 未归段：`family=technical`，`section_id=null` → `unit_id = nil`

插入后确认，并入该单元 debounce。

### 4. 匹配 debounce 与 stale

- `enqueue_one_match`：上一 job `debounce_key` 相同且状态为 `pending|running|done` → 不入队。确认集变（key 变）才新 job。
- `run_match_job` **不再**仅因 job 跑完就把①/②标 stale。①/② stale 只来自：确认集变、勾/去掉、人评。商务 hit 仍标④⑤。
- `upsert_pick` / `delete_pick` 额外标 ③ stale（unmet 会变）。

### 5. 成稿与导出

- ③ 生成**不写** `<!-- clause:{id} -->`。`missing_must_anchors` 只扫各篇②拼接，不扫①③④⑤。
- ② 产品列用 `product_name`，不用 UUID。
- ① 补 `expires_at`。
- `save_part` / `ensure_part(force)` / `export_project_opts(..., true)`：`ended` 则拒写；导出仍可读当前稿。
- 保存时剥 HTML 标签和 `---` YAML 围栏；保留 GFM 段/强调/列表/表和注释锚。
- 导出默认 `regenerate_stale=false`。UI 勾选才带 true。
- `pulldown-cmark` 走标题/段落/列表/表。渲完②后按锚插该条 `BidShot`；MD 里的 `objects/{sha}` 也嵌图。导出段落去掉注释锚。
- `Content-Disposition` 文件名：`{title}-应答卷.docx` / `{title}-定稿.pdf`。前端读该头。

### 6. 一张工作台（替换三页）

拆 `web/src/bid/`，`App.tsx` 只做顶层路由。

```
#/bids/:id                      默认 view=commercial、pane=table
#/bids/:id?view=commercial
#/bids/:id?view=<unitUuid>
#/bids/:id?view=unsectioned
#/bids/:id?view=booklet&part=1|2:…|3|4|5
#/bids/:id/picks|/preview       301 到上式
```

侧栏：本标 → 商务（默认）→ 勾选段（可「并入上一段」）→ 未归段 → 成稿（①③④⑤ + 各②，stale 黄点）。

主列 `[表 | 文稿]`：

| 侧栏 | 表 | 文稿 |
|---|---|---|
| 商务 | 确认 / hit·缺件 | ④ 或 ⑤（按当前条 hit/miss） |
| 勾选段 / 未归段 | 确认 / 建议 / 人评 | 该段② |
| 成稿 | 分册目录 | 该 part |

检查器：

- 商务：去补证（链资料夹）、人评偏离、当前条正文
- 技术：本段候选 + 勾入/去掉（必须带 `unit_id`）、人评、补图（产品 = **本段**已勾，禁止全书 `picks[0]`）
- 无第二套 MD 编辑器

其它 UX 一并收：

- 确认在当前表行，不靠全书第一 draft
- 商务空表不再出上传 Dropzone（上传只在「本标 / 文件」）
- 匹配条：商务「正在按资格条款检索资料」；技术「正在按本段参数匹配产品」
- toast 失败用红；`patch`/`pick` 包 try/catch
- 导出前：有 stale 警告 + 可选重生成；漏锚 409 红字
- 文稿 `focus` 不覆盖未保存 draft
- `ended`：表/文稿只读，仍可下载；顶栏「结束本标」
- 有界编辑器：Textarea + 粗/斜/列表/表工具条 + 只读预览。不加 Milkdown / `@uiw/react-md-editor`

### 7. 文档

`DESIGN.md` §6/§9、`PRODUCT.md` Brand/流程改成上表。领域文档①写截止日。scratch spec 同步。

## Verification

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p bid --lib` 至少覆盖：
  - `coverage_for` 不含 `deviate`；人评 deviate 仍建议 `unmet|cover|…`
  - meet + 建议 unmet → 拒
  - ② 有锚、③ 无锚；删②锚 → `MISSING_MUST`
  - `expected_part_keys` 侧栏序、不含纯商务段
  - 同 key 且 last=`done` → 不再入队
  - 勾选标 ③ stale
  - 导出默认不重写 stale 人稿
- `web` `tsc --noEmit`
- 手走：登录 → 默认商务 → 确认看 hit/缺件 → 去补证 → 进技术段确认/匹配/勾选/人评/编② → 并段后只勾一次 → 手补进未归段仍可见 → 改勾选②变黄、导出默认保留人句 → 删② must 锚拦导出 → ended 只读仍能下 PDF

## 审阅项对照

| 审阅 | 收口 |
|---|---|
| 导出默认重生成 | 默认 false；勾选才 true |
| ③ 锚放行漏 must | 只扫②；③ 不写锚 |
| meet 盖 unmet | PATCH 409 |
| 手补消失 | 未归段 + 手补带 section |
| 商务无 hit/去补证 | clauses 带 hit；检查器去补证 |
| 技术无建议列 | `suggestion` 上表 |
| 勾选不标③ | pick/unpick 标 ③ |
| `coverage_for` 写 deviate | 删除该分支 |
| 商务章当技术段 | units 只收有技术条款的锚 |
| 并无 UI | 侧栏「并入上一段」 |
| 补图 `picks[0]` | 本段 picks |
| debounce 假 stale | done+同 key 跳过；match 完不标② |
| 导出无图 / 产品 UUID | pulldown-cmark + BidShot；`product_name` |
| 三页 IA | 一张工作台 |
| 空商务当未上传 | 空表文案，Dropzone 挪走 |
| 确认不在段内 | 行内确认 |
| toast 当成功 | 失败红 |
| 匹配文案只说产品 | 分商务/技术 |
| Picks 先打全书 | 禁止无 `unit_id` |
| hash 丢分册 | `?view=booklet&part=` |
| ended 无入口 | 顶栏结束；锁写 |
| 死 `api.preview` | 删除客户端 |
| 0014 摊 Pick | 不迁；该段重勾 |
