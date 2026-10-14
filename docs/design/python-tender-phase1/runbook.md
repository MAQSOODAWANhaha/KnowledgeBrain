# 独立 Python 启动与验收

本入口不会接入 Rust，也不会切换现有任务。只使用专用 PostgreSQL 数据库和共享 artifact
目录。所有 worker、dispatcher、CLI 必须看到同一 artifact root。文件存储必须持久化；
队列 Redis 结果 TTL 不承担业务持久性。

## 本机进程

在 `services/docreader` 下使用 Python 3.11：

```sh
uv sync --frozen --python 3.11
export TENDER_DATABASE_URL='postgresql://<user>:<password>@127.0.0.1:15432/<independent_database>'
export TENDER_REDIS_URL='redis://127.0.0.1:16379/1'
export TENDER_ARTIFACT_ROOT='/absolute/path/to/persistent/python-tender'
uv run --frozen python -m docreader.integrations.cli init
```

从既有安全环境注入 `KNOWLEDGEBRAIN_CHAT_BASE_URL/API_KEY/MODEL/MAX_CONTEXT_TOKENS`；
VLM 延续原配置继承规则，独立 endpoint 必须显式配置自己的 key 与 model。
不把秘密写入 manifest、git 或命令输出。CHAT 上下文容量必须为实际服务支持值。

分别启动两个进程：

```sh
uv run --frozen dramatiq docreader.jobs.tasks --processes 1 --threads 4
uv run --frozen python -m docreader.jobs.dispatcher
```

AsyncIO 每 worker 进程一个事件循环；4 threads 最多承载4条并发消息。原生解析隔离到线程/
现有解析器进程池，模型并发每 run 为4。取消通过数据库心跳/节点围栏观察，不等队列排队。
执行基础设施故障共享4次命令失败上限；语义未决不队列重试；原件改变走 revise，同来源要求补漏/纠正走 correct 并重新核查。
dispatcher 数据库断连会退避后继续，Redis 发布失败保留命令并按相同上限结算。

## 提交与确认

manifest 示例（路径相对 manifest 文件；首次提交会复制原件）：

```json
{
  "documents": [
    {"document_id": "main", "role": "main", "version": "v1", "path": "tender.docx"},
    {"document_id": "price-form", "role": "attachment", "version": "v1", "path": "price.xlsx"}
  ],
  "expected_document_ids": ["main", "price-form"]
}
```

```sh
uv run --frozen python -m docreader.integrations.cli submit manifest.json --request-key submission-001
uv run --frozen python -m docreader.integrations.cli status <run_id>
uv run --frozen python -m docreader.integrations.cli confirm <run_id> --version 1 --domain-revision 1 --digest <checklist_digest> --request-key confirm-001
uv run --frozen python -m docreader.integrations.cli cancel <run_id>
uv run --frozen python -m docreader.integrations.cli revise <run_id> revised-manifest.json --request-key revision-002
```

`waiting-confirmation` 时先阅读 status 的登记/冻结问题、要求、独立验证及对应原件，然后确认
精确摘要。`blocked` 不可强制确认。改版产生新的 thread 和摘要，旧确认拒绝。确认产生新
resume 命令，完成后 status=`response-plan-ready`，实际计划在 `results.plan`。
`results` 给出业务 JSON；artifacts 数组给出内容摘要和相对文件引用、阶段耗时。usage 中
每请求的真实用量缺失时保留 null；uncertain 表示可能已执行/计费，绝不伪造精确一次。

## 同来源业务纠错

`source_version` 在 CLI/旧字段中仍名 `version`；`domain_revision` 独立递增。当前结果只包含
本来源的共享冻结数据及当前修订，历史要求/确认/计划保留审计但不授权当前修订。
从 `results.extract` 的每个 pack ref 读取并整理完整纠错 JSON：

```json
{"packs": [{"pack_id": "<existing-pack-id>", "requirements": [], "dispositions": [], "issues": []}]}
```

这里数组仅示意结构，实际必须给出所有原包，并满足各来源范围的要求/非要求/未决处置合同；
不能直接用空数组清除义务。每包 requirements/dispositions 使用抽取产物同一 schema，
不提交 source_ranges、覆盖计数等宿主生成字段。未解决问题可保存后重新验证，但不得放行。

```sh
uv run --frozen python -m docreader.integrations.cli correct <run_id> corrected-requirements.json --version 1 --expected-revision 1 --base-requirements-digest <extract-artifact-sha256> --reason '补漏或纠正的实际依据' --request-key correction-002
uv run --frozen python -m docreader.integrations.cli status <run_id>
uv run --frozen python -m docreader.integrations.cli confirm <run_id> --version 1 --domain-revision 2 --digest <new-checklist-digest> --request-key confirm-002
```

correct 拒绝过期基线和 cancelled run，已冻结来源保持不变。旧确认立即失效；新修订通过
独立验证后再次等待确认。相同 request key/相同内容幂等；不能以 unchanged manifest
调用 revise 伪造来源变更。非秘密模型/提示配置恢复时必须与固定快照一致，密钥轮换允许。

## 同镜像不同角色

```sh
docker build -f deploy/Dockerfile.docreader -t knowledgebrain-docreader:python-tender-phase1 .
docker compose -f deploy/docker-compose.python-tender.yml run --rm python-tender-worker uv run --frozen --no-dev python -m docreader.integrations.cli init
docker compose -f deploy/docker-compose.python-tender.yml up -d
```

以上在仓库根执行；专用 Compose 不被现有默认 Compose 引用。容器 DSN/Redis 主机地址
必须是容器能访问的地址，不能把宿主 `127.0.0.1` 直接照搬。CLI 提交文件须显式挂载原件目录。
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

确定性模型在 `tests/acceptance/fixture_model.py`，只用于测试，没有生产 mock 开关。
它不发外部请求、不编造 paid usage。真实 LLM 验收需原文件、明确上下文容量，以及付费前确认。

生成一份持久合成实际产物供人工检查（仅测试模型，无外部调用）：

```sh
uv run --frozen python tests/acceptance/run_sample.py --root /tmp/tender-sample --evidence /tmp/tender-evidence
```

该命令使用 `TENDER_TEST_DATABASE_URL`；输出运行、验证、来源索引与响应计划 JSON。
运行文件原件/大型产物保留在 `--root` 下，JSON 引用可通过其 artifact root 校验 SHA256。
