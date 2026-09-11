# 本地完整闭环：招标文件 → DOCX 在线编辑 → 保存重开

## Context

编制 UI 已经按 ONLYOFFICE/DOCX 做（`DocxEditor` 加载 `DocsAPI`），后端打开/forcesave/回调也在 `crates/api/src/bid_v2_routes/docx/editor.rs`。本地 Compose **已包含** `onlyoffice` 服务；`deploy/.env.example`、`deploy/.env`、`docker-compose.yml` `x-app-env` 使用同一套 `KB_ONLYOFFICE_*` / `ONLYOFFICE_HOST_PORT`。缺项时打开编辑器 fail-closed。

**已拍板：** 本切片只做 **A**（上传并冻结解析 → 创建/打开 DOCX → ONLYOFFICE 编辑保存重开 → 下载 DOCX）。不做抽取 Agent、不做 Conversion PDF。本地用 **Community** `onlyoffice/documentserver`。浏览器用宿主机 origin。知识库 Markdown 预览保留。

## Approach

现有 `Config` 只有 `server` + `api`。`server` 同时用于：

- 浏览器 `api.js`（`api_script_url`）
- API → CommandService（forcesave）
- 回调 `url` 的 origin 校验与下载（`download_url` 要求 origin == `server`）

Compose 里浏览器要 `http://127.0.0.1:18081/`，API 容器访问 `127.0.0.1` 会打到自己。因此 **必须** 增加可选 `KB_ONLYOFFICE_COMMAND_ORIGIN`（默认等于 `SERVER_ORIGIN`，测试不设则行为不变）：

| 变量 | Compose 默认 | 谁用 |
| --- | --- | --- |
| `KB_ONLYOFFICE_SERVER_ORIGIN` | `https://127.0.0.1:18081/` | 浏览器加载 api.js。打开编辑器时 API 按请求 Host 把主机名改成当前访问的 IP/localhost，端口与 scheme 仍用该变量 |
| `KB_ONLYOFFICE_COMMAND_ORIGIN` | `http://onlyoffice/` | API 调 CommandService；回调 cache URL 校验/改写后下载 |
| `KB_ONLYOFFICE_API_ORIGIN` | `http://api:8080/` | DS 拉 source、POST callback |
| `KB_ONLYOFFICE_JWT_SECRET` | 独立 secret | 与 DS `JWT_SECRET` 相同；≠ 登录 `JWT_SECRET` |
| `KB_ONLYOFFICE_CAPABILITY_SECRET` | 另一个 secret | source/callback 用途绑定；必须 ≠ JWT secret |

`download_url`：允许 origin 为 `server` 或 `command`；若回调给出浏览器 origin，下载时改写为 `command` 的 host/port。forcesave POST 走 `command`。

DS 环境：`JWT_ENABLED=true`、`ALLOW_PRIVATE_IP_ADDRESS=true`（才能请求 `http://api:8080`）。不改 release-descriptor 五镜像合同。

## Reuse

- `crates/api/src/bid_v2_routes/docx/editor.rs` — `Config::load`、open/save/callback
- `crates/bidding/src/docx_round.rs`、`crates/api/src/bid_v2_routes/docx.rs`
- `web/src/bid/authoring/DocxEditor.tsx`、`docxSession.ts`、`web/src/bid/api/docx.ts`
- 解析：`convert_tender_source` + `TenderDocumentProcessV2Worker` + docreader
- HTTP 替身：`crates/api/tests/docx_round_http.rs`
- 协议说明：`docs/bidding/docx-editor.md`

## Files to modify

- `crates/api/src/bid_v2_routes/docx/editor.rs` — `command` origin；CommandService 与 `download_url`
- `docs/bidding/docx-editor.md` — 记录 `COMMAND_ORIGIN`
- `deploy/docker-compose.yml` — `onlyoffice` 服务、`x-app-env` 透传、volume
- `deploy/.env.example` — `KB_ONLYOFFICE_*`、`ONLYOFFICE_HOST_PORT`
- `deploy/README.md` — 本地闭环 vs 生产许可
- `web/src/bid/authoring/media.ts` — `.xlsm .doc .xls`
- `crates/api/tests/docx_round_http.rs` — 不设 COMMAND 时仍过；可选一条改写下载 origin

## Steps

- [x] **C1** `Config` 增加 `command`；`KB_ONLYOFFICE_COMMAND_ORIGIN` 空则 clone `server`。
- [x] **C2** `force_save` POST `command.join("coauthoring/CommandService.ashx")`。
- [x] **C3** `download_url` 接受 server 或 command origin；fetch 时若是 server origin 则改写到 command。
- [x] **D1** Compose `onlyoffice`：`onlyoffice/documentserver:8.3.3`；`18081:80`；JWT；私网 IP；`/healthcheck`；`start_period` 90s；runtime profile。
- [x] **D2** `x-app-env` 写入 KB_ONLYOFFICE_* 和 TTL/timeout（本地默认必填，不能空着）。
- [x] **D3** `.env.example` 填本地默认；两 secret 不同且 ≠ `JWT_SECRET`。
- [x] **D4** 前端上传扩展名对齐后端。
- [x] **D5** Document Server 已 `docker compose up`：`knowledgebrain-onlyoffice` healthy，`http://127.0.0.1:18081/healthcheck` 与 `/welcome/` 可达。完整「建项目→上传冻结→编辑保存重开」需 `--profile runtime` 全栈 + 浏览器，本环境未跑登录 UI。

## Verification

- `onlyoffice` healthy；浏览器打开 `http://127.0.0.1:18081/welcome/`。
- 未配 env：仍 `ONLYOFFICE_NOT_CONFIGURED`。
- 配齐：iframe、forcesave、回调落盘、`docx_sha256` 变、重开可见改动。
- `cargo test -p api --test docx_round_http`（需既有 ONLYOFFICE 替身环境时按现脚本）；不设 COMMAND 的替身测不过回归。
- 前端 `media.ts` 含 xlsm/doc/xls。
- 不宣称 Community = 生产嵌入许可。

## Out of scope

抽取 Agent、ONLYOFFICE 转 PDF、`render_v2` 撤除、release-descriptor、字体/并发采购。
