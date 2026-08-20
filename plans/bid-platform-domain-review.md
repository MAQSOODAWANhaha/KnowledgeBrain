# 投标平台领域草案审查

## Context

审查对象：`docs/bid-platform-domain.md`（草案）。问题是方案是否自洽、对着本仓库实现会不会打架。不改业务代码。

已拍板（本轮）：

| 项 | 结论 |
|---|---|
| 仓库 | **同仓**。部署沿用现有 Compose 拆分：`api` / `worker` / `docreader` + postgres/redis/minio |
| 公司资料里有什么 | 招标书会要的**公司侧材料**：资质、体系证、服务能力、业绩/案例等。用来答商务/资格条款，不是拿来跟型号抢排序 |
| 鉴权 | **关掉注册**，登录走公司 **LDAP**，发现有 JWT。登录用户都能打标、都能打两种 `scope` 的 `/match` |

本仓现状仍与草案 §8 冲突：Workspace 无 `kind`；`/match` 必须落到一个仓且只评 `kind=product`、静默顶 50；`use_library` 把证记到产品分上；检索看 `enable_status` 不看 multimodal 是否完成；`require_ws` 罩住几乎所有读写；`vlm_configured()` 会落到 chat URL。

## Approach

**领域叙事成立，按字面开工会撞实现。** 先改两份规格，再改知识库契约，再加投标实体。投标不是新 Git 仓，也不是再调一层 HTTP convert：HTTP 仍进 `api`，解析/抽取进 `worker` 新队列，进程内复用 `docparser` / `models::chat_sse` / 对象存储。

拍板后的模型：

```
（一家公司，LDAP 登录即可）
  ├── Workspace kind=product_line     产品线（型号手册、界面图）
  ├── Workspace kind=company          恰好一条；资质 / 体系 / 业绩案例 / 服务能力
  │     └── Product（分类夹，kind=library）→ Document → 检索
  └── BidProject                      与 Workspace 平级
        └── 招标文件只活在项目里，不进产品索引
```

三处必须改草案，否则实现必吵：

1. **商务检索按条款找文档，不走产品排序。** `/match?scope=company` 的契约是 `requirement.id → hit|miss + 最佳文档`，不是 `candidates[]` 排行榜。公司侧 Product 只是分类夹（执照、ISO、业绩案例、服务能力），用 `kind=library` + Tag。同一条款多份材料：取分数最高的一条写入 `BidCommercialHit`，其余可进可选 `alts[]`，④ 先展示最佳文件名。
2. **招标 convert 与公司资料入库分开。** 公司资料走现有 Document 管线（必须进向量，否则商务打不到）。招标文件走抽出来的 `convert + multimodal → markdown` 库，写入 `BidDocument`，**禁止** `INSERT documents`。两边共用 DocReader / 同一 VLM，不共用 `parse_status` / `enable_status`。
3. **鉴权改成「LDAP 登录 = 全库通」.** 关 `POST /auth/register`；`/auth/login` 走 LDAP bind，用户不存在则插入。`require_ws` 不再挡列仓、传手册/证、读原件、`/match`、投标。API key 只要能认证就可以打标和两种 `scope`（不按 workspace/product scope 挡）。`workspace_members` 第一期留表、不当门闩。

知识库再补两道门闩：

- `index_ready`：convert 完成 ∧（无图 ∨ multimodal 完成）。商务自动重搜、招标切条都看这个，不借用 `parse_status=completed`（那会等到 wiki/graph），也不看单独的 `enable_status`（那时图可能还是 `![]`）。
- 真 VLM：`vlm_configured()` 只认 `KNOWLEDGEBRAIN_VLM_*`，禁止落到 `LLM_BASE_URL`。产品线 + company 的 current 默认 `enable_multimodel=true`。

产品线里那份默认 library「公司资料」：**冻结写入并迁到 company**，商务不扫。禁止双写。

确认后的两路匹配做成**项目级异步作业**（同条款集 debounce），禁止每点一条确认就同步打完全部产品线。`scope=product_lines` 只走 PG；跨线 embedding 或 `retrieval_config` 不一致 → 400；产品数超过今日 50 帽改为真评完或显式 400，**禁止静默截断**。

## Files to modify

规格（先做，代码不动直到规格改完）：

- `docs/bid-platform-domain.md` — 写入本轮拍板；改 §5.1 商务响应；补 `index_ready`、superseded、鉴权三句、匹配作业
- `docs/system-design.md` — Workspace.kind、`/match`.scope、鉴权、VLM、Hit 出图、GET document/files、默认 library 策略

规格定稿后的实现面（现在不写代码）：

- `deploy/docker-compose.yml` — 不新增业务容器；`api`/`worker` 加 LDAP / `BID_EXTRACT_MODEL_ID` / `KNOWLEDGEBRAIN_VLM_*` 环境变量
- `crates/auth` — LDAP bind；关掉 register
- `crates/api/src/routes.rs` — 登录即可；`create_workspace` 的 kind / 禁止第二条 company；company 不插 library；`DocView.object_key`；files 登录可读
- `crates/search/src/lib.rs` — `scope`；company 按条款展平；跨仓；去静默 50；Hit.`image_object_key`
- `crates/domain` / `crates/enrichment` — 多模态默认开；VLM 不回落到 chat
- `crates/docparser` + `crates/worker` — 抽出无 Document 的 convert；招标 `bid:convert` / `bid:extract` 队列
- 新迁移 — `workspaces.kind`（company 部分唯一）；Bid* 表
- 新模块（建议 `crates/bid`）— 项目 / 条款 / Pick / 预览；由 `api`、`worker` 链接

## Reuse

| 复用 | 路径 | 用法 |
|---|---|---|
| 现有三进程 | `deploy/docker-compose.yml` 的 `api` / `worker` / `docreader` | 投标 HTTP 挂 `api`，作业挂 `worker`，不新开 Git 仓、第一期不新开业务容器 |
| 产品排序 | `crates/search/src/lib.rs` `matching_pg` | **只**给 `scope=product_lines` |
| 混合检索 | `hybrid_search` / `pg_version_hits` | company 内部按条款扫全部 company current，再展平 |
| 图 object key | `enrichment` 写在 `chunks.context_header` | 仅 `image_ocr` / `image_caption`；补到 Hit |
| 模型 SSE | `crates/models/src/http.rs` `chat_sse` | 投标抽取进程内调用，不新开 HTTP |
| DocReader | `crates/docparser`、`services/docreader` | 招标与产品共用 gRPC；招标结果写入 BidDocument |
| JWT | `crates/auth` | LDAP 成功后仍发这张票 |
| 对象键 | `objects/{sha256}` | 招标原件、人补图、手册抽图同一桶；人补图用登录可读的全局 files，不挂 ProductVersion |
| `tender_text` | `requirements_from_tender` | **不用**（无 family；先 chat 再按行兜底） |
| chunker heading | `crates/chunker/src/heading.rs` | **不当** BidSection；招标切段按「第 X 章 / 3.2」另写 |

## Steps

- [ ] **改草案** `docs/bid-platform-domain.md`：同仓分容器；公司资料 = 资质/服务/案例等分类夹；商务 `/match` 按条款 hit/miss；鉴权 = 关注册 + LDAP + 登录通吃；`index_ready`；`BidClause` 增加 superseded；确认后匹配改为项目作业
- [ ] **改** `docs/system-design.md` §2 / §4.2 / §7.2，与上表对齐（以领域草案为准）
- [ ] 知识库：`workspaces.kind` + bootstrap `slug=company`；存量回填 `product_line`；冻结并迁移默认 library
- [ ] `/match` 加 `scope`；登录即可；company 响应按条款；product_lines 跨仓 + embedding/阈值校验；Hit 出图
- [ ] 真 VLM + 两种 Workspace 的 current 默认开多模态；`GET document` / `files` 登录可读
- [ ] 抽出 `convert_to_markdown`（无 Document）；招标 `BidDocument` 四态只表示 convert+multimodal
- [ ] LDAP 登录、删除/关闭 register；API key 可打标
- [ ] BidProject / 多文件上传 / 按文件自动抽 / 人确认
- [ ] 两路匹配作业 → BidPick / BidShot / 预览 ①～⑤

## Verification

规格改完后用这些场景对：

1. Compose 仍是 `api`+`worker`+`docreader`，投标不另起一个要 HTTP 调 convert 的容器。
2. 未登录 401；LDAP 登录后可建产品线、往 company 传案例/证、建标、确认、勾选、预览。`POST /auth/register` 4xx。
3. 产品线旧 library 里的 ISO **不**进 ④；只有 company 下那份进。
4. 条款「具备 ISO9001」+「近三年类似案例」→ 两条 `BidCommercialHit`，分别落到证和案例文档，不出现「ISO 产品 vs 案例产品」排行。
5. 扫描件 `enable_status=enabled` 但 multimodal 未完 → 不写 `miss`，预览待检索。
6. 两个产品线 embedding 不同 → `scope=product_lines` 400，不静默评 50 个。
7. 确认第 1 条技术条款 → 入队一次匹配作业，不把 HTTP 打满全库；已有 Pick 时新条款是 `need_rematch` 不是 `unmet`。
8. 后传补遗只追加 draft；旧 draft 有 superseded；已确认不动。
9. 人补界面图不进产品索引，预览 ② 能读到。
10. 招标 PDF 解析失败不影响已完成文件出条款。

## 审查摘要（对照原草案）

仍成立：应标侧、无包件、人确认、技术/商务两路、勾选即方案、覆盖现算、缺了就补、招标不进产品索引。

必须改掉的实现矛盾：

| 原草案 | 实现对上会怎样 | 改为 |
|---|---|---|
| 商务也走产品排序 `/match` | 执照/案例变成互相竞争的「产品」 | `scope=company` 按条款展平文档命中 |
| 表结构不变 + 商务资产是 Product | 默认 library 与 company 双写 | company 用 `kind=library` 分类夹；旧 library 迁走 |
| 同一套 convert 且不 INSERT documents | convert 绑在 Document 生命周期上 | 公司资料入库；招标用抽出的 convert 库 |
| `parse_status=completed` 后再检索 | 会等到 wiki/graph，或过早在 enabled 时误 miss | 新门闩 `index_ready` |
| 登录即可（未写注册） | 开放 register + 全库通 | 关注册 + LDAP |
| 确认后立刻两路 `/match` | 同步打全库超时 | 项目级异步作业 |
| 产品 >50「一次评完」 | 今日静默截断 | 禁止截断；400 或真评完 |
| 「已被新抽取替代」 | `status` 只有 draft/confirmed/rejected | 增加 superseded |
| 同仓未决 | convert / SSE 无法复用 | 已决：同仓、进程内复用、Compose 三进程 |

## 仍未写入代码的小缝（不挡改规格）

- LDAP 具体协议（ldap/ldaps、组过滤）进 `deploy/.env.example`，不进领域实体。
- 案卷对象前缀：建议招标原件 / 人补图仍用 `objects/{sha256}`，与产品共用去重。
- Tag `界面` 第一期仍建议、非门闩；Tag 按 Workspace 隔离。
- `BidPreview.sections`：**读时现算**，与 5.3 同一套谓词，不固化第二真相。
