# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

delegated: Vite + TypeScript SPA in `web/`, served by the existing `api` process (`KNOWLEDGEBRAIN_WEB_ROOT`). No extra container. Chosen after the user asked to ship the full UI immediately; they did not name a framework.

## Users

公司内部投标人员（乙方）。LDAP 登录后处理招标文件：拆条款、勾产品、补图、出预览/定稿。办公桌、日光灯，白天看屏幕。

## Product Purpose

知识库回答「我们能卖什么、有哪些证」。投标台回答「这一标怎么拆、事实是什么、勾哪些产品、缺什么、如何报价、怎样形成①～⑥正式应答」。成功 = 登录用户能走完 建项 → 上传 → 事实/条款确认 → 两路匹配与1..N选择 → 人工报价定稿 → 程序材料处理 → ①～⑥组卷 → Word 过程稿 / PDF 正式定稿。

## Positioning

事实和条款由人确认；匹配是异步发布的不可变结果，不在确认 HTTP 里打全库。公司资料按条款找证据，不当产品排名；技术 supported 候选由人选择1..N。正式价格只由人录入并定稿。

## Operating Context

内网。同一 Compose：`api` / `worker` / `docreader`。过程中可反复生成 Word；正式 PDF 必须通过报价、有效期、程序材料、附件、part stale 和依赖门禁。正式 PDF 可打印线下签章；V1 不做 CA 电子签章或电子投标平台自动递交。

## Capabilities and Constraints

- 关注册。LDAP 或本地口令。登录即可投标与读写知识库。
- 招标文件不进 `documents`。公司资料走 Document 管线，看 `index_ready`。
- 未确认条款不进匹配。系统给 recommended，但保留全部 supported；人不宣布唯一最佳。
- V1 金额仅 CNY，价格由人确认；最高限价必须明确含税/未税口径。
- 业务时区固定 `Asia/Shanghai`。
- 不做包件、Org、多租户、成本/评标引擎、假截图、CA 电子签章或自动递交。
- clean-slate fresh redeploy；不保留旧 schema、旧 API、alias、双写或旧格式读取。

## Brand Commitments

- 名称：KnowledgeBrain / 投标。
- 视觉钉死：Apple / iCloud 浅雾灰工作台；侧栏 + 分栏；唯一强调色 Lake Blue `#1D6FD8`；系统字体栈（SF Pro / PingFang SC / Noto Sans SC）。规范见 `DESIGN.md`。
- 组件钉死：Mantine 7，不用原生控件当主交互。
- 禁止把招标文件或人补图写回产品库。

## Evidence on Hand

- `docs/bidding/domain.md`、`plans/bidding/README.md`
- `docs/knowledge-base/domain.md`（知识库领域与证据检索端口）
- `GET/POST /api/v1/bids…` 已实现
- `1.png`：Plannotator 三栏编辑器（结构参照）；外观改为 Cloudflare 白底
- 无客户照片、无真实标书样张可当品牌图

## Product Principles

1. 引导式工作台：进标先看本标文件。主流程为文件 → 事实/条款 → 匹配/选择 → 报价/材料 → ①～⑥成稿；侧栏跟随本标树。解析是文件状态，不是一步。可回头，但正式 PDF 受硬门禁。
2. 缺了就补，不锁死。
3. 系统排序，人勾选。
4. 过程 Word 可带 warning/placeholder；正式 PDF 读取冻结 manifest 并通过 SubmissionGateV1。
5. 登录即权限，不做 Workspace 门闩。

## Accessibility & Inclusion

中文界面。正文对比 ≥ 4.5:1。键盘可走完确认/勾选/导出。
