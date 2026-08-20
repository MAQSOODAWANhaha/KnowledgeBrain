# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

delegated: Vite + TypeScript SPA in `web/`, served by the existing `api` process (`KNOWLEDGEBRAIN_WEB_ROOT`). No extra container. Chosen after the user asked to ship the full UI immediately; they did not name a framework.

## Users

公司内部投标人员（乙方）。LDAP 登录后处理招标文件：拆条款、勾产品、补图、出预览/定稿。办公桌、日光灯，白天看屏幕。

## Product Purpose

知识库回答「我们能卖什么、有哪些证」。投标台回答「这一标怎么拆、勾哪些产品、缺什么、导出 ①～⑤」。成功 = 登录用户能走完 建项 → 上传 → 先商务再技术段 → 表里确认/勾选/补图 → 文稿成稿 → Word 过程稿 / PDF 定稿。

## Positioning

条款是人确认的；匹配是项目级作业，不在确认 HTTP 里打全库。公司资料按条款找文档，不当产品排名。

## Operating Context

内网。同一 Compose：`api` / `worker` / `docreader`。过程中反复下 Word；定稿 PDF。⑥ 函/报价模板不做。

## Capabilities and Constraints

- 关注册。LDAP 或本地口令。登录即可投标与读写知识库。
- 招标文件不进 `documents`。公司资料走 Document 管线，看 `index_ready`。
- 未确认条款不进匹配。系统排序，人不宣布唯一最佳。
- 第一期不做包件、Org、⑥、假截图。

## Brand Commitments

- 名称：KnowledgeBrain / 投标。
- 视觉钉死：Apple / iCloud 浅雾灰工作台；侧栏 + 分栏；唯一强调色 Lake Blue `#1D6FD8`；系统字体栈（SF Pro / PingFang SC / Noto Sans SC）。规范见 `DESIGN.md`。
- 组件钉死：Mantine 7，不用原生控件当主交互。
- 禁止把招标文件或人补图写回产品库。

## Evidence on Hand

- `docs/bid-platform-domain.md`、`docs/system-design.md`
- `GET/POST /api/v1/bids…` 已实现
- `1.png`：Plannotator 三栏编辑器（结构参照）；外观改为 Cloudflare 白底
- 无客户照片、无真实标书样张可当品牌图

## Product Principles

1. 一张工作台：侧栏选商务/勾选段/成稿，主列是表或文稿，不是三套页面。
2. 缺了就补，不锁死。
3. 系统排序，人勾选。
4. 过程 Word，定稿 PDF。
5. 登录即权限，不做 Workspace 门闩。

## Accessibility & Inclusion

中文界面。正文对比 ≥ 4.5:1。键盘可走完确认/勾选/导出。
