# 招投标 V2 后端运行手册

本文只覆盖 `docs/bidding/authoring.md` 与
`plans/bidding/authoring-clean-slate-execution.md` 的 Phase 0–7 后端交付。
Web `files | authoring | export` 工作台按 `plans/bidding/frontend-authoring.md` 独立交付。

## 1. 不可变运行边界

- 只支持 clean-slate fresh database；不迁移、不双写、不运行兼容 DDL。
- migrator 依次创建 `knowledge_base_baseline.sql`、`shared_platform_baseline.sql`、
  `bidding_v2_baseline.sql`。API、Worker、Retention 启动时不执行 DDL。
- 招投标 HTTP 只使用 `/api/v2`。`/api/v1/ops/*` 是共享平台运维 API，不是招投标 V1。
- 每个 project 只有一个 project-wide Workspace 和一个 WorkspaceHead。
- 业务 warning 不阻断人工编辑或导出；CAS、Schema、资产、事务、权限和渲染错误 fail-closed。
- AI 任务只由用户请求创建；Worker 不得自动创建 Outline/Content request。

## 2. Fresh 部署

```bash
cp deploy/.env.example deploy/.env
# 必须替换 JWT、数据库、对象存储和模型凭证。
docker compose -f deploy/docker-compose.yml --env-file deploy/.env \
  --profile runtime up -d --build
```

依赖顺序是 PostgreSQL/Redis/Object Store → migrator → API/Worker/Retention。运行进程必须使用各自
runtime role；只有 migrator 可以创建 Schema。若 catalog 处于 partial/stale 状态，停止运行进程并在
明确允许丢弃全部数据后执行：

```bash
docker compose -f deploy/docker-compose.yml down -v
docker compose -f deploy/docker-compose.yml --env-file deploy/.env \
  --profile runtime up -d --build
```

不得用在线 `ALTER`、first-launch verifier 或 catalog allowlist 修复 partial schema。

## 3. 队列与进程

`deploy/queue-registry.toml` 中 `bid-authoring-v2` 只允许五类 `required_enabled` Job：

1. `bid:tender_document_process:v2`
2. `bid:requirement_set_compile:v2`
3. `bid:outline_generate:v2`
4. `bid:content_generate:v2`
5. `bid:submission_export:v2`

API 事务提交后 enqueue。若enqueue结果不确定，API返回503并在`error.details`中携带已提交的request artifact ID、revision、request digest和frozen input digest；客户端必须用同一`Idempotency-Key`重试。Worker只读取冻结request identity，发布immutable artifact/stage receipt，不自动推进WorkspaceHead；
相同 frozen input redelivery 必须 replay。同一功能不得拆出 EvidenceMatch continuation 或旧 Part/Gate Job。

## 4. 常见恢复流程

### 4.1 TenderDocument 失败

1. 读取 V2 document status 和技术错误。
2. 修复 DocReader gRPC、OCR/VLM、文件 magic/container、ObjectRegistry 或对象可用性问题。DocReader只在Tender job执行时连接；不可用不得阻止export/content/maintenance lane启动或运行。
3. 由 owner 调用 `POST /api/v2/bid-projects/{project_id}/tender-documents/{document_id}/retry`，携带新的
   `Idempotency-Key`。
4. 观察 TenderDocumentProcess 和后续 RequirementSetCompile receipt。

pending/failed/unresolved 是可冻结的业务状态；它们不得让其他已成功文件失效。

### 4.2 CAS 或过期后台结果

- Workspace mutation/candidate acceptance 的 `If-Match` 冲突返回 409 和当前 head。重新 GET，向用户展示
  冲突后再显式重放操作。
- superseded RequirementSet delivery仍将Request记录为`succeeded`，result identity标注`published_current=false`；不得写Request `obsolete`，也不得推进RequirementSet current、RequirementProjection或WorkspaceHead。
- Requirement supersession 按 `effective_applicability.fragments` 做局部 DAG 重放；边冻结 old/new
  SourceUnit revision 和 amendment DocumentRelation identity。范围重叠、未知来源或不兼容 DocumentSet 必须失败。
- stale Candidate保持存储状态`proposed`，由base Workspace与当前Head派生有效`obsolete`；不得覆盖后来人工编辑。已接受Candidate的重复请求返回原decision receipt，reject不要求Workspace `If-Match`。
- RequirementProjection和QuoteSnapshot发布只移动各自current pointer。用户必须携带Workspace revision ID+digest和`If-Match`调用显式apply；CAS失败返回409并回滚全部Workspace artifacts。
- `POST /api/v2/bid-projects/{project_id}/document-set-revisions`返回冻结DocumentSet和专属RequirementSetCompile request identity。Web只可通过`GET /api/v2/bid-projects/{project_id}/requirement-set-compilations/{request_id}`轮询该绑定请求；`pending`、`failed`或`published_current=false`均不得apply或生成大纲。仅用户当前发起的生成动作可在校验冻结DocumentSet和Projection identity后显式apply，后台刷新不得自动apply。

### 4.3 Evidence 与生成

- `NO_EVIDENCE` 是可审计 warning，不是 Gate。
- `image_ocr` 必须有 Knowledge 域发布的 immutable media mapping 和 attestation；缺失或不一致时修复知识
  摄取后重新由用户发起匹配/生成。
- user PickSet 由冻结 request 加载；不要授予 Worker 直接读取 bidding tables 的宽权限。
- 不得手工改 EvidenceBundle、Candidate 或 accepted selection。

### 4.4 Export 与对象清理

SubmissionExport 顺序固定为 Assessment → AttachmentPreparation/verify → RenderSnapshot/Manifest →
manifest-only DOCX/PDF render → output publication。Worker 运行镜像必须提供 `pdftoppm`；启动时会执行preflight。`embedded_pages` PDF只允许Worker从冻结source digest以固定144 DPI准备，客户端准备接口不得提交替代页面图片。未准备时Preview只显示元数据占位且不读取source PDF bytes；准备后Preview、DOCX和PDF用同一可打印区域拟合冻结页面。失败时：

- prepared snapshot/manifest 可按原 request replay；renderer 只以 `(manifest_id, manifest_sha256)` 加载输入，
  不得从 request 或 live Workspace 重建同一 manifest。
- staging output 未转为 owner 时由 Retention 回收。
- 已发布 output、字体、图片、附件页由 ObjectRegistry owner reference 保留；禁止直接删物理对象。
- export请求只接受受控`watermark`选项；submission必须为null。Assessment提示只进入独立报告，不提供Knowledge provenance或review notice appendix开关。
- 正式render对缺失/空图片、digest不匹配、缺失embedded-pages preparation失败关闭。固定上限为：冻结export input 64 MiB、单图64 MiB/20,000px/100MP、PDF source 128 MiB、1,000页、raster总量256 MiB、render输出256 MiB、raster/render各120秒；临时目录在所有退出路径清理。

## 5. 当前 checkout 的强验证

以下命令都必须针对 fresh database；历史日志不能替代重跑。

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=1
scripts/bidding_v2_deletion_scan.sh
python3 -m py_compile \
  scripts/bidding_v2_phase2_api_e2e.py \
  scripts/bidding_v2_evidence_api_worker_e2e.py \
  scripts/bidding_v2_export_api_worker_e2e.py
git diff --check
```

Fresh SQL acceptance：

```bash
DATABASE_URL=postgres://... scripts/fresh_schema_acceptance.sh
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f scripts/bidding_v2_phase0_live.sql
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f scripts/bidding_v2_phase1_live.sql
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f scripts/bidding_v2_phase1_supersession_live.sql
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f scripts/bidding_v2_phase3_live.sql
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f scripts/bidding_v2_phase6_live.sql
```

真实 API→Redis→Worker 验收需要启动 API、Redis、Worker，配置 writable `OBJECT_DIR` 和正常认证得到的
owner JWT：

```bash
export BID_V2_API_URL=http://127.0.0.1:58080
export BID_V2_JWT='<owner jwt>'
python3 scripts/bidding_v2_phase2_api_e2e.py
python3 scripts/bidding_v2_evidence_api_worker_e2e.py
python3 scripts/bidding_v2_export_api_worker_e2e.py
```

E2E 必须实际观察 request terminal status、Candidate/PickSet/accept receipt、DOCX/PDF bytes、Manifest、download
和 Assessment report；HTTP 200 或 mock response 本身不是完成证据。

## 6. 审计清单

- 招投标生产源码不含 V1 endpoint、PartSet、Gate、first-launch、compat/migration/dual-write。
- Schema、Rust payload、HTTP DTO、QueueRegistry 和 Worker handler 使用同一 closed V2 identity。
- runtime API/Worker 无 bidding table write；Knowledge image verifier 归 Knowledge 域所有。
- Workspace/Candidate/Export 的 immutable dependency 和 object owner identity 可从 API receipt 回溯。
- 六格式 Tender fixture、纯人工编制、动态 Outline、Evidence/PickSet、部分 Candidate acceptance、DOCX/PDF
  replay 和技术失败清理均有当前运行证据。
