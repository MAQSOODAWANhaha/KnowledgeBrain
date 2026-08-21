# 投标工作台：引导式 Wizard 壳

## Context

左侧「作业」把阶段入口（评估 / 成稿 / 文件）和内容导航混在一起，用户要求去掉这三项。阶段应走顶栏 Wizard；抽出的商务/技术内容才进侧栏。

硬约束（不改后端语义）：先商务再技术、缺了就补、不锁死、自动条款 draft、仅确认后匹配。因此 **禁止不可回头的强制 Wizard**。

已拍板：

1. 顶栏四步：`文件 → 解析 → 评估 → 成稿`
2. 评估仍是一步，不拆成商务 Wizard / 技术 Wizard
3. 侧栏只放抽出内容：评估时商务 / 技术段 / 未归段；成稿时 ①～⑤。文件列表不再进侧栏

## Approach

只改工作台壳（`web/src/bid` + hash + Shell 空侧栏），不改 API / 抽取 / 匹配。

**顶栏阶段条（可点、可回头）**

| 步 | 主区 | 侧栏 | 下一步就绪 |
|---|---|---|---|
| 文件 | `FilesPane` 上传 + 全量状态 | 无（抽出内容还不存在） | 至少一个文件入库 |
| 解析 | 同一文件列表 + 解析/抽取进度与失败重试 | 无；若已有条款可显示评估树但不作为作业入口 | 无 processing/pending 文件，且抽取不在跑（允许部分失败，用文案提示） |
| 评估 | 现有条款表 + Inspector | **只要** 商务 / 各技术段 / 未归段（带待确认数） | 不强制确认完；主按钮仍可进成稿，提示未确认数 |
| 成稿 | `BookletPane` + 导出 | **只要** ①～⑤ 分册 | — |

- 主按钮：文件「开始解析/下一步」；解析完成后「去评估」；评估「去成稿」；成稿无下一步，保留 Word/PDF
- 上一步始终可点
- 未就绪步骤可点进去（例如评估在抽取中），主区显示等待/失败，不 404
- 新建标、无文件时落地 **文件** 步（已有 `view=commercial && 无文件 → files` 逻辑，改为按 `step`）
- 上传完成后 **不自动跳评估**（现已 toast 停下）；解析步看完状态再点下一步

**Hash**

- 增加 `step=files|parse|eval|booklet`
- `view` 仍表示评估内分段（`commercial` / unit id / `unsectioned`）或兼容旧书签
- 兼容：`?view=files` → `step=files`；`?view=booklet` → `step=booklet`；无 step 时按派生状态落到当前该去的步

**Shell**

- `tree` 可空：文件/解析步不渲染侧栏列（或 CSS 收掉 `.side`），主区拉满
- 评估/成稿才传 tree

## Files to modify

- `web/src/hash.ts` — `BidRoute.step`、`bidHref`、`parseBidRoute` 兼容
- `web/src/bid/Workbench.tsx` — 用可点击 Wizard 替换装饰用 `BidSteps`；主按钮；去掉作业栏；按 step 切主区
- `web/src/bid/Sidebar.tsx` — 删除「作业」三项和文件名列表；评估只留分段，成稿只留分册
- `web/src/Shell.tsx` — 无 `tree` 时不占 260px 侧栏
- `web/src/app.css` — 阶段条、空侧栏、主区拉满
- `web/src/bid/FilesPane.tsx` — 作为文件/解析步主区（状态已有，解析步可强调进度）
- `PRODUCT.md` — 原则 1 改为 Wizard 壳；侧栏 = 抽出内容
- `docs/bid-platform-domain.md` — 过程顺序补一句工作台四步（不改领域对象）

## Reuse

- `bidStage` / 现有 `BidSteps` 结构（`Workbench.tsx`）升为可点击条
- `FilesPane` + `fileStage`（`helpers.ts`）
- `ClauseTable`、`ClauseDetail`、`Inspector`、`BookletPane`
- `liveClauses`、`unitIdForView`、`catalogKeys`、`partTitle`
- `derived.files_ready` / `extract_running` / `fileStage` 作为步骤状态

## Steps

- [x] Hash：`step` 四态 + 旧 `view=files|booklet` 映射；默认落地规则
- [x] `BidSteps` 改为可点击顶栏（当前/完成/失败角标）；主区底部或页头「上一步 / 下一步」
- [x] `Sidebar` 删除作业三项和文件清单；评估分段、成稿分册保留
- [x] `Shell` 支持无侧栏
- [x] `Workbench` 按 `step` 渲染 Files / 解析进度 / 评估表 / 成稿；不再用作业入口切页
- [x] 解析失败、部分抽取失败时步骤条和主区都可见，不跳走
- [x] 同步 `PRODUCT.md` 与领域文档一句
- [x] 手动走一遍：建标 → 上传 → 解析 → 评估（切商务/技术）→ 成稿 → 回文件补传

## Verification

- 侧栏再也看不到「评估 / 成稿 / 文件」和文件名清单
- 新建标进入文件步，主区拉满
- 多文件状态都能在文件/解析步完整看见
- 解析完成后点下一步到评估商务；左边只有商务/技术段
- 评估未确认完仍可去成稿，可回评估
- 成稿左边只有 ①～⑤
- `#/bids/{id}?view=files`、`?view=booklet` 仍打开对应步
- 结束本标后 Wizard 可看、不可改

## 非目标

- 不改抽取引擎、匹配、lease、LDAP
- 不把评估拆成两个 Wizard 步
- 不做强制门闩（未确认不能成稿等）
