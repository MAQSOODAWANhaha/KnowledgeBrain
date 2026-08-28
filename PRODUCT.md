# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

delegated: Vite + TypeScript SPA in `web/`, served by the existing `api` process (`KNOWLEDGEBRAIN_WEB_ROOT`). No extra container. Chosen after the user asked to ship the full UI immediately; they did not name a framework.

编制正文钉死 **Tiptap WYSIWYG**。不是 Markdown 源码，也不是 textarea + GFM 预览。Tiptap JSON 只活在浏览器内存，持久真源是 `ContentBlockV1`。

## Users

公司内部投标人员（乙方）。LDAP 登录后处理招标文件：上传、生成大纲、在 Word 式画布里改树和正文、从知识库填充、导出 DOCX/PDF。办公桌、日光灯，白天看屏幕。

## Product Purpose

知识库回答「我们能卖什么、有哪些证」。投标台回答「这一标要求什么、大纲怎么长、正文怎么写、缺什么、怎样导出一份可交的稿」。成功 = 登录用户能走完 建项 → 上传招标文件 → 解析 → 生成大纲 → 在同一张画布改树 / 改字 / 插表插图 / 知识库填充 → 导出 Word 过程稿或 PDF 正式稿。改完再导出是新文件，不是改旧文件。

## Positioning

用户拥有最终编辑与导出决定权。系统只生成大纲 / 内容候选、提示风险、保存可追踪证据。知识检索用来给「填充」提出证据和图片候选，不做成必须勾选产品才能往下走的向导。正式价格若使用报价，只由人录入并定稿为 `QuoteSnapshot`。编制过程没有业务锁；Assessment 只提示。

## Operating Context

内网。同一 Compose：`api` / `worker` / `docreader`。过程中可反复生成 Word / PDF。业务提示不阻止导出。只有技术失败（schema / 资产 / CAS / renderer）才停止对应 mutation 或 render。正式 PDF 可打印线下签章；V1 不做 CA 电子签章或电子投标平台自动递交。

## Capabilities and Constraints

- 关注册。LDAP 或本地口令。登录即可投标与读写知识库。
- 招标文件不进 `documents`。公司资料走 Document 管线，看 `index_ready`。
- Web 编制面是 Word 式三栏：左侧大纲导航、中间连续画布、右侧知识 / 提示。用户看见的黄金路径只有 **文件 → 编制 → 导出**。解析是文件状态；freeze / checkpoint / 台账不是「确认后才能改」的界面。
- 大纲是独立树（lineage / 父子 / ordinal）。章标题来自树，不嵌进正文 heading，也不从 Markdown `#` 抽取。点击跳转、拖拽调序 / 层级、增删改名拆合第一期就做完整。
- 正文 Tiptap 关闭 heading。只给聚焦章挂活编辑器，其余章静态渲染，点哪编哪。
- AI 只出 Candidate overlay（大纲默认全选、可取消节点；正文可部分接受），永不直接换树或覆盖并发人工编辑。
- 未覆盖要求、stale、缺件只提示，不禁止编辑或导出。导出当前 `WorkspaceRevision`。
- V1 金额仅 CNY，价格由人确认；最高限价必须明确含税 / 未税口径。报价精编、台账精编、文档设置面板不是黄金路径必经步。
- 业务时区固定 `Asia/Shanghai`。
- 不做包件、Org、多租户、成本 / 评标引擎、假截图、CA 电子签章或自动递交。
- 禁止把招标文件图片 / 附件自动当作投标方证据。
- clean-slate fresh redeploy；不保留旧 schema、旧 API、①～⑥ PartSet、SubmissionGateV1、alias、双写或旧格式读取。

## Brand Commitments

- 名称：KnowledgeBrain / 投标。
- 视觉钉死：Apple / iCloud 浅雾灰工作台；侧栏 + 分栏；唯一强调色 Lake Blue `#1D6FD8`；系统字体栈（SF Pro / PingFang SC / Noto Sans SC）。规范见 `DESIGN.md`。
- 组件钉死：Mantine 7 做壳，不用原生控件当主交互。编制画布用 Tiptap。
- 禁止把招标文件或人补图写回产品库。

## Evidence on Hand

- `docs/bidding/authoring.md`（编制工作区产品、领域与 Web 交互契约）
- `docs/knowledge-base/domain.md`（知识库领域与证据检索端口）
- `GET/POST /api/v1/bids…` 已实现部分 V2 壳；Word 式整篇画布仍按契约待落地
- `1.png`：Plannotator 三栏编辑器（结构参照：左树 / 中文稿 / 右检查器）；外观仍走 Apple / iCloud，不走 Cloudflare 锌灰
- 无客户照片、无真实标书样张可当品牌图

## Product Principles

1. 引导式工作台，但不是锁死向导。进标先看本标文件。用户主路径是文件 → 编制 → 导出；侧栏在编制步跟随本标大纲树。解析、冻结、检查都不是另一步。
2. 一张 Word 式文稿。左侧独立大纲，中间按树前序叠成连续画布，右侧是当前章证据和生成状态。点击跳转、滚动反标、hash 同步到当前章 lineage。
3. 缺了就补，不锁死。生成中、有候选、有检查提示时，用户仍可改树、改字、导出当前稿。
4. 系统建议，人改写。大纲候选和内容候选都是 overlay；人改的留下，过期候选不得覆盖。
5. 过程稿与正式稿都可以在有业务提示时导出；正式 submission 不写水印、风险声明或知识来源。技术损坏 fail-closed。
6. 登录即权限，不做 Workspace 门闩。

## Accessibility & Inclusion

中文界面。正文对比 ≥ 4.5:1。键盘可走完上传、大纲跳转 / 调序（拖拽的按钮备份）、正文编辑、导出。
