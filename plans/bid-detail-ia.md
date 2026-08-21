# 投标项目详情：上导航 + 侧栏状态交互

## Context

详情页乱，核心是 **Wizard 点击和侧栏点击抢同一套主区，栏目又不完全对应**。

硬约束不改：先商务再技术、缺了就补、不锁死、不改抽取/匹配 API。只梳理详情壳的点击 → 主区映射。

## 现在有哪些栏目、点了怎样

状态实际是 `step`（Wizard）+ `view`/`part`（侧栏叶）。Wizard 一换，侧栏整棵树换掉。

### 上导航 Wizard（始终 4 项）

| 点 | 路由 | 侧栏变成 | 主区 |
|---|---|---|---|
| 1 文件 | `step=files` | 本标文件名列表 | 上传 + 全量文件状态（`FilesPane`） |
| 2 解析 | `step=parse` | **同一份**文件名列表 | **几乎同一块** `FilesPane`，多一条抽取提示 |
| 3 评估 | `step=eval`，`view` 沿用（从成稿来则商务） | 换成商务 / 技术段 / 未归段 | 该 `view` 的条款表 + 右侧检查器 |
| 4 成稿 | `step=booklet` | 换成 ①～⑤ | 当前 `part` 的文稿 |
| 上一步 / 下一步 | 同上四态顺序 | 同上 | 同上 |

乱点：文件≈解析；点评估丢掉文件树；点文件名主区不变。

## Approach

**Wizard 三步，侧栏跟步走一棵本标树。**

理由：解析/抽取是文件上的状态，不是一个去处；四步会让两个按钮打开同一块主区。三棵树同时铺开会再次「进去把抽出内容全摊开」。一步一棵树，栏目和主区 1:1。

### 点击表（已锁定）

**Wizard（3）**

| 点 | `step` | 侧栏只出现 | 主区默认叶 |
|---|---|---|---|
| 1 文件 | `files` | 本标招标文件 | 上传 + 每份排队/解析中/失败/已抽出 |
| 2 评估 | `eval` | 商务、已抽出技术段、未归段 | 商务表 + 检查器 |
| 3 成稿 | `booklet` | ①～⑤（② 有段才列出） | ① 扉页 |

点 Wizard 只换模式。可回头、不锁死。未抽出时点评估：空表 +「再抽一次」，不 404。去掉「上一步 / 下一步」。

**侧栏（本标，一次一棵）**

| 当前 Wizard | 点 | 主区 |
|---|---|---|
| 文件 | 某文件 | 滚到/展开该文件行（有状态、重试、删除）。不是单文件条款页 |
| 评估 | 商务 | 本标商务条款表 |
| 评估 | 某技术段 / 未归段 | 该叶技术条款表 |
| 成稿 | ①～⑤ 某一册 | 该册文稿；预览/编辑留页头 |

侧栏固定还有：在办的标（回列表）、本标标题（不可点）。

**进标：** `#/bids/{id}` 与列表点击 → `step=files`。旧 `step=parse` → `files`。

## Files to modify

- `web/src/hash.ts` — `BidStep = files|eval|booklet`；`parse` 映射 `files`；进标默认 files
- `web/src/bid/Workbench.tsx` — Wizard 三项；主区按上表；删除 `WizardNav`；上传后留在文件步（不跳 parse）
- `web/src/bid/Sidebar.tsx` — 按 `step` 一棵树；文件行 `doc` 锚点
- `web/src/bid/FilesPane.tsx` — 行 `id` 锚点，供侧栏滚入
- `web/src/app.css` — 仅必要时
- `PRODUCT.md` / `DESIGN.md` — 导航分工一句

不改 `Shell.tsx` 结构（仍要侧栏 + 上导航槽）。不改 API。

## Reuse

`FilesPane`、`ClauseTable`、`ClauseDetail`、`Inspector`、`BookletPane`、`liveClauses`、`unitIdForView`、`partTitle`、`fileStage`、`bidHref` / `parseBidRoute`。

## Steps

- [x] Hash：三态 + `parse`→`files`；无 step 落 files
- [x] Wizard 三项；删上一步/下一步；上传后不跳 parse
- [x] 侧栏按 step 一棵本标树；文件点击滚到该行
- [x] FilesPane 行锚点
- [x] PRODUCT / DESIGN 一句
- [x] 手动：进标文件 → 点文件名滚到行 → 评估商务/技术 → 成稿① → 回文件；未抽出点评估不 404

## Verification

- 进标主区是本标文件，不是评估总表
- Wizard 没有「解析」；文件步能看见每份解析/抽取状态
- 评估侧栏无 ①～⑤；成稿侧栏无商务；文件侧栏无条款段
- 点文件名主区滚到该行，不再空点
- 华电文件/条款不出现在中煤
- 可回头、不锁死
