# 独立 Python 启动与验收

故障根因、解决方案及实际验证状态统一见[问题与修复记录](bug-fix.md)，新发现的问题先记录再修复。

## 当前 v2 主线与部署边界

本期目标已收敛为完整分析招标文件：义务、评分规则、不利后果和原文明示的填表、签章、
附件及证明材料提交要求。当前源码五节点为 `register → freeze → extract → verify → confirm`，
确认后到达 `analysis-ready`；条目统一为 `TenderItem(item_id, kind, text, evidence)`，四种
`kind` 为 `obligation`、`scoring_rule`、`consequence_rule`、`submission_requirement`。
业务含义交由 LLM 独立审阅，宿主保留结构、证据、覆盖、预算和执行保护。跨包引用一轮
补读后仍未解决的内容转人工证据裁决，不能以确认清除阻断。

本期不采集企业响应数据、不生成响应计划或材料准备待办、不选择是否争取评分机会。
“提交证明材料”是招标原文要求，分析只记录这项要求，不宣称企业已有材料或已经满足资格。

五节点、v2事项协议、默认3次重试、身份容错和定位压缩已部署，并发2，复用现有PG/Redis。
用户已授权真实验证且费用不设上限。旧run `eedcf1b0-40f4-42f2-b6c7-ef47ea723ec8`
在原文review17/17、locate49/202后首数据超时，uncertain及全部原回执保留。
用户明确选择用新版本重新付费跑同一文件，已通过正式submit建立独立运行
`6f0a29a2-8f45-4725-bebb-70485d8a75ba`；动态进度统一见[最新交接](handoff.md)。
不得通过改写命令状态、原回包、版本、摘要或清缓存来续跑；旧checkpoint/抽取/Plan不
冒充新协议产物。业务blocked使用已有证据裁决入口，确认不能清除未决问题。

源码的status输出新增`budget_observation`：计划请求数/输入估算与实际Token分列，按任务
统计输出max/p95、超过预留次数、实际总量与上下文容量对照以及缺失usage。
该报告已随05:07 UTC短引用修复版部署，区分计划估算和实际报告用量。
报告是观测，不自动调整容量、追加门限或把未知用量算作0；当前校准证据见BUG-021。

本轮源码增加合同拒收反馈、请求短引用及宿主核验执行覆盖；normalization_version=2，
预算builder为chat_json_object_stream_short_refs_v1。旧run已按用户要求正式取消，05:07 UTC修复版已部署。
不能在同一修订中更换Prompt/发送协议后直接retry。先完成或明确处置在途任务，再升级服务；
新协议真实验证使用独立新运行，保留旧运行和全部回执。模型纠正只针对未接受的合同失败，
不自动修正既有业务结论，不替用户确认。

本入口不会接入 Rust，也不会切换现有任务。Python 可与同一安装共用 PostgreSQL 数据库及
Redis，保留自己的运行表、checkpoint 和队列命名空间。所有 worker、dispatcher、CLI
必须看到同一 artifact root。文件存储必须持久化；
队列 Redis 结果 TTL 不承担业务持久性。

## 本机进程

在 `services/docreader` 下使用 Python 3.11：

```sh
uv sync --frozen --python 3.11
export PGHOST=127.0.0.1
export PGPORT=15432
export PGDATABASE=knowledgebrain
export PGUSER='<existing-application-user>'
# 从安全环境注入 PGPASSWORD，保留密码原字节；也可直接提供 DATABASE_URL。
export REDIS_URL='redis://127.0.0.1:16379/0'
export TENDER_ARTIFACT_ROOT='/absolute/path/to/persistent/python-tender'
# 共享 Rust 数据库须已完成主部署的 migrate，再初始化 Python 表。
uv run --frozen python -m docreader.integrations.cli init
```

从既有安全环境注入 `KNOWLEDGEBRAIN_CHAT_BASE_URL/API_KEY/MODEL/MAX_CONTEXT_TOKENS`；
`TENDER_DATABASE_URL`/`TENDER_REDIS_URL` 仍供显式隔离测试或运维覆盖，Compose 不要求它们。
VLM 独立配置 `KNOWLEDGEBRAIN_VLM_BASE_URL/API_KEY/MODEL`，不继承 Chat 的地址、密钥或模型；
即使使用同一服务，也须显式填写三项。缺少 VLM 配置仅在需要图片处理时阻断。
不把秘密写入 manifest、git 或命令输出。CHAT 上下文容量必须为实际服务支持值。
本轮私有真实 PDF 验收使用父任务已明确的 `KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS=131072`；
它是单请求窗口，不能替代累计实际 usage，也不允许纠错自行翻倍。

分别启动两个进程：

```sh
uv run --frozen dramatiq docreader.jobs.tasks --processes 1 --threads 4
uv run --frozen python -m docreader.jobs.dispatcher
```

AsyncIO 每 worker 进程一个事件循环；4 threads 最多承载4条并发消息。原生解析隔离到线程/
现有解析器进程池，分包调度与模型默认并发每 run 为2；4 threads 是消息并发，多个 run
以及其他应用仍会共享同一模型 Key 的服务端额度。取消通过数据库心跳/节点围栏观察，不等队列排队。
执行失败默认首次加最多3次重试，连接、服务拒绝、JSON/宿主合同及普通异常共享4次命令
失败上限，不叠加SDK重试；失租由接管方一次结算进同一预算。旧Worker在取消或版本/租约
失效后停止写入，sent/uncertain停止付费重放。业务未决不队列重试；
原件改变走 revise，同来源事项补漏/纠正走 correct 并重新核查。
dispatcher 数据库断连会退避后继续，Redis 发布失败保留命令并按相同上限结算。
核验分包和完整请求预算测量在线程中运行，避免同步规划阻塞事件循环与租约心跳。

## 提交与确认（当前 v2）

manifest 示例（路径相对 manifest 文件；首次提交会复制原件）：

```json
{
  "documents": [
    {"document_id": "main", "role": "main", "document_version": "v1", "path": "tender.docx"},
    {"document_id": "price-form", "role": "attachment", "document_version": "v1", "path": "price.xlsx"}
  ],
  "expected_document_ids": ["main", "price-form"]
}
```

以下 CLI 对当前 v2 源码或已更新为 v2 的镜像执行；`confirm` 不生成响应计划。

```sh
uv run --frozen python -m docreader.integrations.cli submit manifest.json --request-key submission-001
uv run --frozen python -m docreader.integrations.cli status <run_id>
uv run --frozen python -m docreader.integrations.cli confirm <run_id> --source-version 1 --domain-revision 1 --digest <checklist_digest> --request-key confirm-001
uv run --frozen python -m docreader.integrations.cli cancel <run_id>
uv run --frozen python -m docreader.integrations.cli revise <run_id> revised-manifest.json --request-key revision-002
```

`waiting-confirmation` 时先阅读 status 的登记/冻结问题、事项、独立验证及对应原件，然后确认
精确摘要。`blocked` 不可强制确认。改版产生新的 thread 和摘要，旧确认拒绝。确认产生新
resume 命令，完成后 status=`analysis-ready`。`results.current_items` 从当前抽取、核验及
已接受决议投影有效事项，`results.verify` 保留核验问题、关系及决议回执；不输出 `results.plan`。
`results` 给出业务 JSON；artifacts 数组给出内容摘要和相对文件引用、阶段耗时。usage 中
每请求的真实用量缺失时保留 null；uncertain 表示可能已执行/计费，绝不伪造精确一次。

## 失败执行恢复

默认3次重试仍未成功后，先用 status 核对当前版本、最新失败 command 和模型用量。对已结束、明确失败的执行，
显式新建恢复命令；保持原文件、执行快照和领域修订不变：

```sh
uv run --frozen python -m docreader.integrations.cli retry <run_id> --source-version 1 --domain-revision 1 --failed-command-id <latest-failed-command-id> --reason '重试已完整返回但JSON无效的抽取包' --request-key retry-001
```

dispatcher 按原入口调度，新命令继续同一个 checkpoint，复用成功产物，只对明确 failed 的
请求允许再次付费。原失败命令、回包及用量保留；相同 request-key 和参数重复提交返回
同一恢复命令，不产生额外调用。恢复已提交确认的失败执行时，保留原 resume 和确认参数。
当前修订含 sent/uncertain、已取消、版本过期或目标不是最新失败命令时拒绝恢复。
blocked 的业务未决走证据裁决或 correct，不用 retry 清除。若请求审计 succeeded 而宿主
业务合同拒收，仍复用原响应；本入口不改回包、不改摘要，也不自动修复任意语义错误。

## 同来源业务纠错

`source_version`、单文件`document_version`和`domain_revision`分别按[字段字典](field-dictionary.md)命名，无旧字段别名。当前结果只包含
本来源的共享冻结数据及当前修订，历史事项/核验/确认保留审计但不授权当前修订。
从 `results.extract` 的每个 pack artifact_ref 读取并整理完整纠错 JSON：

```json
{
  "schema_version": 2,
  "packs": [{
    "pack_id": "<existing-pack-id>",
    "schema_version": 2,
    "items": [],
    "relations": [],
    "dispositions": [],
    "issues": []
  }]
}
```

这里数组仅示意结构，实际必须给出所有原包，并满足各来源范围的 `items/non_item/unresolved`
处置合同及完整事项引用；不能直接用空数组清除义务。每包使用 v2 抽取合同的
`schema_version/items/relations/dispositions/issues`，最小事项为 `item_id/kind/text/evidence`，
不提交 source_ranges、覆盖计数等宿主生成字段。未解决问题可保存后重新验证，但不得放行。
`--base-items-digest` 取当前无 scope 的 `extract` 汇总产物 `artifact_ref.sha256`，
不是某个分包产物的摘要。

```sh
uv run --frozen python -m docreader.integrations.cli correct <run_id> corrected-items.json --source-version 1 --expected-revision 1 --base-items-digest <extract-artifact-sha256> --reason '补漏或纠正的实际依据' --request-key correction-002
uv run --frozen python -m docreader.integrations.cli status <run_id>
uv run --frozen python -m docreader.integrations.cli confirm <run_id> --source-version 1 --domain-revision 2 --digest <new-checklist-digest> --request-key confirm-002
```

作用域问题使用有限的独立入口，不编辑原文件或删除问题。从 `status` 的当前
`results.verify.issues` 读取待审议问题及具体 `range_id`，结合冻结原文核对；
`results.pack-plan.plan_digest` 提供本次审议的当前分包基线。提交JSON顶层只有`scope_decisions`：
每项包含`range_id`、`decision`（`bind`或`local_only`）、精确原文`evidence`（range_id/quote）、
具体`rationale`和`target_range_ids`。bind目标必须为同文档完整规范范围；local_only目标为空，
理由须非空并说明局部边界和依据，不要求命中特定词表或固定措辞；业务含义仍需结合
证据独立核验，不能仅凭理由字符串宣称问题已解决。其他未审问题继续阻断。

```sh
uv run --frozen python -m docreader.integrations.cli review-scope <run_id> scope-decisions.json --source-version 1 --expected-revision 2 --base-plan-digest <current-plan-digest> --reason '确认具体条件适用范围的依据' --request-key scope-003
```

该命令将新增领域修订，复用规范来源、重建分包支持、重新抽取和验证；已有作用域决议保留，
旧确认失效。来源/预算变更不得复用旧分包回执。无有效证据、空理由、错误目标、非当前
问题或过期基线拒绝；人工作用域决议还须连同原文及目标进入独立核验。
自动化验收仍使用隔离测试数据库/schema，不能在共享业务表上运行测试清理。
新表结构尚未发布，不对旧未发布 checkpoint 建立迁移兼容。

correct 拒绝过期基线和 cancelled run，已冻结来源保持不变。旧确认立即失效；新修订通过
独立验证后再次等待确认。相同 request key/相同内容幂等；不能以 unchanged manifest
调用 revise 伪造来源变更。非秘密模型/提示配置恢复时必须与固定快照一致，密钥轮换允许。
模型并发也属于固定执行快照；默认值从4改为2后，已有修订不会被静默改写。
`uncertain` 回执继续阻止自动重发；修改并发不能解除该围栏或授权再次付费。

## 同镜像不同角色

以下为已有 PostgreSQL/Redis 且主迁移已完成时的 Python 角色更新入口；须保留当前安装实际
使用的私有网络 overlay，并先核对[交接中的网络与发送围栏](handoff.md)，不因更新镜像而
重发旧 uncertain 请求。构建只选择 Python target，启动使用 `--no-deps`，不重建或重启
Rust、数据库和原 DocReader 服务。当前 overlay 路径如下，换安装环境时须对应核实。

```sh
tender_compose() {
  docker compose --env-file deploy/.env \
    -f deploy/docker-compose.yml -f deploy/docker-compose.local.yml \
    -f /tmp/kb-workflow-start-g8vpogcy/model-relay.yml \
    --profile python-tender "$@"
}

kb_tender_build_dir=$(mktemp -d /tmp/kb-tender-build-XXXXXX)
tender_compose build --print --build-arg APT_MIRROR=debian \
  python-tender-worker > "$kb_tender_build_dir/build.json"
docker buildx bake --allow=network.host -f "$kb_tender_build_dir/build.json" \
  --load python-tender-worker &&
tender_compose stop python-tender-worker python-tender-dispatcher &&
tender_compose up --no-deps --no-build --force-recreate \
  --abort-on-container-exit --exit-code-from python-tender-init python-tender-init &&
tender_compose up -d --no-deps --no-build --force-recreate \
  python-tender-worker python-tender-dispatcher
```

以上在仓库根执行，使用主 Compose 的可选 profile、已有 `.env` 及同一 PostgreSQL/Redis。
先完成主部署的 `migrate`，再由一次性 `python-tender-init` 初始化 Python 自己的表及 checkpoint；worker/dispatcher
复用 DocReader 镜像和共享 artifact 卷，无新增宿主端口。容器使用 `postgres`、`redis`
服务名，不能把宿主 `127.0.0.1` 直接照搬。CLI 提交文件须显式挂载原件目录，例如：

```sh
docker compose --env-file deploy/.env \
  -f deploy/docker-compose.yml -f deploy/docker-compose.local.yml \
  --profile python-tender run --rm --no-deps \
  -v /absolute/path/to/originals:/input:ro python-tender-worker \
  uv run --frozen --no-dev --no-sync python -m docreader.integrations.cli submit /input/manifest.json \
  --request-key submission-001
```

CLI 的 status、review-scope、correct、confirm 使用同一个服务入口和 artifact 卷。
镜像构建时已安装锁定依赖，容器角色及 CLI 使用 `--no-sync`，运行时无需访问包索引。
默认 docreader 镜像 CMD 仍是原 `main.py` gRPC。使用私有网络/凭证控制独立入口，尚无 Rust 权限 API。

## 自动验收

```sh
export TENDER_TEST_DATABASE_URL='<dedicated test PostgreSQL DSN>'
export TENDER_TEST_DSN="$TENDER_TEST_DATABASE_URL"
export TENDER_TEST_REDIS_URL='redis://127.0.0.1:16379/1'
export TENDER_REDIS_URL="$TENDER_TEST_REDIS_URL"
uv run --frozen pytest tests/unit tests/workflow tests/acceptance -q
uv build --wheel
```

接受测试会启动并 SIGKILL 自己的 Dramatiq 子进程，不碰现有 Rust worker。数据库账号需有
测试 schema 创建权限。未配置真实服务时 integration 测试会显式 skip；skip 不能当通过。
新 `.github/workflows/python-tender.yml` 显式启动 PostgreSQL/Redis 服务运行这些测试。

当前五节点确定性模型在 `tests/acceptance/fixture_analysis_model.py`，只用于测试，没有生产 mock 开关。
它不发外部请求、不编造 paid usage。真实 LLM 验收需原文件、明确上下文容量，以及付费前确认。

生成一份持久合成实际产物供人工检查（仅测试模型，无外部调用）：

```sh
uv run --frozen python tests/acceptance/run_sample.py --root /tmp/tender-sample --evidence /tmp/tender-evidence
```

该命令使用 `TENDER_TEST_DATABASE_URL`；确认后终态为 `analysis-ready`，输出运行、事项索引、
独立核验及来源 JSON，不生成响应计划。
运行文件原件/大型产物保留在 `--root` 下，JSON 引用可通过其 artifact root 校验 SHA256。
