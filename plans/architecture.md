# KnowledgeBrain 目标架构

| 项 | 值 |
| --- | --- |
| 状态 | **已确认，实施中（D 未完成项见 [`d5-remaining.md`](d5-remaining.md)）** |
| 产品契约 | [`../docs/bidding/authoring.md`](../docs/bidding/authoring.md) |
| 编制 Web | [`bidding/frontend-authoring.md`](bidding/frontend-authoring.md) |
| 后端到达 | [`bidding/tender-to-submission-v2.md`](bidding/tender-to-submission-v2.md) |

本文是仓库级目标架构。它不改编制契约里的编辑/导出规则，只规定：**一个项目怎么分层、东西放哪、资料和投标怎么共用一个库。**

---

## 1. 产品（已经确认，本方案不改）

用户主路径：

```text
文件 → 编制 → 导出
```

编制是 Word 式三栏：独立大纲树 + Tiptap 连续画布 + 检查器。Assessment 只提示，没有业务 Gate。**不做**匹配向导、报价向导、①～⑥。检查器里「匹配资料」最多是编制内现查刷新，不是一步。

- 招标文件驱动大纲和「招标要求」结构；招标方图片不得自动当投标方证据。
- 投标正文来自：人在画布里写的、从资料填进来并被接受的、工作区里手插的资产、可选的 QuoteSnapshot。
- V1 不是整份作废。转换、SourceSpan、QuoteSnapshot、CAS、对象、队列仍要。因目标变更而换掉的是固定 PartSet、业务 Gate、向导式匹配/组卷。

---

## 2. 一个项目、一套运行时

不是拆库、不是拆部署。

```text
一个 Compose
  postgres    库名 knowledgebrain（一张 catalog）
  redis       Oxana
  minio       对象字节
  neo4j       可选图谱双写
  api / worker / retention / docreader
```

一个 ObjectRegistry、一个对象桶、一套 Oxana。  
`api` / `worker` / `retention` 用不同 DB 角色连**同一库**。

现码还有 `knowledge_base_baseline` / `shared_platform_baseline` / `bidding_v1_baseline` 三刀 first-launch。**那是现状，不是前期落地方式。** 前期在同一个库上直接加表、改表、加 SQL，跟 crate 一起交，不必先改 4000 行 baseline、不必过 checksum manifest 才能编一章。生产级 fresh-launch 仪式往后放，不挡编制主路径。

`knowledge::Store`（内存 HashMap）**不是**第二套生产库。生产已经 `platform::connect()` → Postgres。目标 D：知识管线正式 API 也只走这个 pool，不再 hydrate Store。

---

## 3. 七个 crate（已拍板）

```text
docparser              解析（资料和招标都用，独立 crate）
     ↑                    ↑
knowledge              bidding
     ↘                    ↙
              platform
           ↗     ↖     ↖
         api   worker  retention
```

| crate | 拥有 | 不拥有 |
| --- | --- | --- |
| **platform** | 鉴权、观测、Oxana、**打开唯一 PgPool**、fresh migrate、ObjectRegistry | BidProject、大纲、Document、检索算法 |
| **docparser** | PDF/DOCX/XLSX/图片 → 结构化源 | 知识索引、投标大纲、自己的表 |
| **knowledge** | 资料 Workspace/Product/Document、chunk/index/wiki/graph、检索、LLM HTTP | 招标文件、BidWorkspace、组卷 |
| **bidding** | BidProject、BidWorkspace、大纲、ContentBlock、Candidate、Assessment、QuoteSnapshot、导出 | 知识库表结构的第二份实现、Oxana 内部状态 |
| **api** | HTTP 组装 + 静态 Web | 领域规则 |
| **worker** | Oxana handler 组装 | 领域规则 |
| **retention** | 物理删对象 | 业务表 |

已从 20 个 crate 归并（旧包名已删除）：

| 现在 | 并入 |
| --- | --- |
| `auth` `obs` `runtime` `migrator` `first-launch-verifier` + storage 的 pool/对象/迁移 | `platform`（migrator/verifier 改为这个 crate 的 bin） |
| `chunker` `clone` `index` `enrichment` `graph` `wiki` `search` `models` + storage 的 knowledge_* | `knowledge` |
| `docparser` | 仍独立 |
| `bid` + storage 的 `bid_*` + runtime 里的 bid job 合同 | `bidding` |
| `api` `worker` `retention` | 保留，变薄 |
| `domain` | **撤销。** 见下面落点，避免 knowledge ↔ docparser 循环 |
| `storage` | **撤销包名。** `persist.rs` 必须切开，禁止整文件并进 platform |

**`persist.rs` 切开（必须写进搬家清单，不能靠文件名 glob）：**

- pool / migrate / schema identity → `platform::db` / `migrate`
- `users` / `api_keys` → `platform::auth`
- workspace / document / chunk / wiki / graph / search SQL 和 `hydrate_workspace` → `knowledge`（生产路径随后删掉 Store hydration）
- 投标相关若还写在 persist 里 → `bidding`

**`domain` 撤销落点：**

- 文件类型、默认解析引擎、`sha256_hex`、URL 拦截 → **`docparser`**（知识库和招标都用；docparser **不**依赖 knowledge 的 `ProcessOverrides`）
- `ProcessOverrides`、资料 Workspace/Product/Document 类型 → `knowledge`
- Bid 类型 → `bidding`
- 队列名、错误码、first-launch 拓扑 → `platform`

crate 内部用模块，**禁止**再给 Outline/Assessment/Render 各开一个 crate。

### 3.1 bidding 内部分组（可选，不是落地门槛）

前期按功能往现有 `bid` 里加即可。下面只是以后整理时的参考，不挡「先跑通三步」。

```text
tender_set / workspace / outline / authoring / candidate / render
quote          可复用，非黄金路径
requirement    后台，不是顶栏、不是 Workbench 一步
```

不要单独建名为 `evidence/` 的模块当落地项，以免又做成 EvidenceBundle。

### 3.2 knowledge 内模块

```text
ingest/     上传后管线
retrieve/   给编制用的检索（就是查本库资料表）
wiki/ graph/ search/
models/
```

### 3.3 platform 内模块

```text
auth/ queue/ objects/ db/ migrate/ first_launch/ obs/
```

`db` 只负责：连接、事务帮手、执行 SQL。前期不把「一份 first-launch manifest」当成改表的唯一门。

---

## 4. 资料和投标怎么共用一个库

拆 crate ≠ 拆库。两边的 SQL **源码**分文件，**执行**打同一个 PgPool。

```text
platform::db::pool()
        │
        ├─ knowledge 的 SQL   → 资料/索引表
        ├─ platform 的 SQL    → 对象/幂等/迁移账本
        └─ bidding 的 SQL     → bid_* / 工作区
              全部在 catalog knowledgebrain
```

**前期 schema 跟着功能走。** 编制缺表就在本库加表（SQL 可以放在 bidding/knowledge/platform 自己的目录），本地和开发 compose 直接 apply。不要把每次改动塞回巨型 baseline，也不要「谁改哪一刀切片」当流程。

仍是同一个 `knowledgebrain` catalog。以后若要干净安装，再从当时真实 schema 收一份 baseline——那是后置打包，不是日常开发约束。

### 4.1 填充：可以现查资料（已拍板，取消旧端口法律）

**取消**这些目标约束：

- bidding 禁止 JOIN knowledge 表
- 必须经 `KnowledgeRetrievalPort` 才允许读资料
- 检索命中立刻冻进投标 EvidenceBundle / MatchingReport
- knowledge-owned attestation 才能证明一次填充

填充就是同一库里查当前资料，生成 ContentCandidate；人没接受就不必写成投标证据仓。

仍要的很少：

1. **招标文件不进资料表。** 招标 PDF 走 bidding 文件表 + docparser，不进 `documents`，也不能自动当投标方证据。
2. **接受进稿才跟稿走。** 人接受一块文字/一张图时，把当时文本或对象 digest 写入 ContentBlock / 工作区资产。之后资料改删，这份稿还能导。稳住的是 `WorkspaceRevision`，不是每次检索的平行证据仓。
3. **不要复制一套分块/向量。** bidding 调 `knowledge` 的检索函数（同一 pool）。这是代码复用，不是「禁止 SQL」。

```text
点「填充」
  → knowledge 按当前资料检索（可直接查资料表）
  → ContentCandidate
  → 人接受 → 写入本章 ContentBlock / 工作区图
  → 导出只读 WorkspaceRevision，不回查 live 资料拼正文
```

`KnowledgeRetrievalPort`、Matching attestation、命中即冻投标仓：从**目标**拿掉；现码对照里标明因目标变更待删。

### 4.2 事务（单库才说这个）

技术上可以一个事务横跨资料和投标，**业务上不要。**

| 场景 | 动哪些表 |
| --- | --- |
| 编一章、接受候选、导出 | bidding + 必要时 ObjectRegistry |
| 资料上传、分块、索引 | knowledge + ObjectRegistry |
| 填充 | knowledge 只读检索；接受时 bidding 写块/资产 |
| 上传招标文件 | bidding 文件行 + 对象；不写 knowledge documents |

---

## 5. 词

| 词 | 拥有方 | 含义 |
| --- | --- | --- |
| `Workspace` | knowledge | 产品线 / 公司资料空间 |
| `BidProject` | bidding | 一个标 |
| `BidWorkspace` | bidding（对外文档） | 该标唯一输出工作区。代码/SQL 可暂留 `SubmissionWorkspace` |
| 资料检索 | knowledge 提供函数 | 编制调用来填充，不是匹配向导 |

平台不拥有上面任何业务聚合。`runtime` 里现有的 `bid_authoring_contract` 迁到 `bidding`。并 crate 时 **不要改 SQL 表名**；`SubmissionWorkspace` 可留在库里，文档对外叫 BidWorkspace。

---

## 6. 文档树（落地后）

```text
PRODUCT.md / DESIGN.md

docs/platform/                 只对应 platform
docs/knowledge-base/           只对应 knowledge
docs/bidding/authoring.md      ← 现 docs/bidding/authoring.md
docs/bidding/current-code.md   ← 现 docs/bidding/current-code.md
docs/research/                 非规范

plans/architecture.md          本文（仓库级）
plans/platform/
plans/knowledge-base/
plans/bidding/
  README.md                    只指向 V2 两份计划
  tender-to-submission-v2.md
  frontend-authoring.md
  current-code/                现码复用/待换
```

旧契约路径留一页 stub，避免外链碎。  
权威：用户路径和画布看 `authoring.md` §2.4；实现看 V2 两份计划；现码看 `current-code`。

---

## 7. Web

**步导航**（打开标之后）只有：`files | authoring | export`。  
`AuthoringStep` / `parseAuthoringRoute` / Workbench 主列 **只认这三步**。要求台账、报价页、独立预览 **不是** Workbench 一步，也不再保留六步 `AUTHORING_STEPS` 当正式路由联合类型。

**编制画布工具条**（编制步里面）才是：生成大纲 · 填充本章/全部空章 · 导出。不要和步导航都叫「顶栏」。

报价若还要入口：编制检查器或设置里的次链，不进三步导航。  
`gfm.tsx` 不是编辑真源。

---

## 8. 明确不做

- 两个 Postgres、两套 Redis、两套对象桶
- 给每个投标聚合再开 crate
- 保留 `domain::Store` 当生产领域层
- 用 MatchingReport / attestation 当填充合同
- 改知识库 `Workspace` 表名
- 前期用 first-launch checksum / 三刀 baseline 当改表门禁

---

## 9. 落地顺序（确认后才执行）

**A 文档** 契约搬家、计划索引、链接。  
**B 编制主路径可跑** 三步顶栏 + 工作区表按需加在本库；缺什么表加什么，不走 manifest 仪式。  
**C 并 crate** 同一 `DATABASE_URL`，`pub use` 过渡。验收：仍一个 postgres。  
**D 知识管用 PgPool** 去掉生产路径上的 `Store`。  
**E 后置** 需要干净安装时再收 baseline；删 Gate/PartSet 随编制主路径替换，不单独等「Phase 7 仪式」。

---

## 10. 现码对照（不是目标）

crate 已并成 7 个。Web 已是三步导航。`knowledge::Store` hydrate、Matching attestation、Gate/PartSet 仍是现码/待删，不写回目标。
