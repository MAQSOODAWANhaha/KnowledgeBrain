# Review 规范

工单标 `done`、review 票勾「通过」之前，**本票改动触及的每一种语言 / 产物都必须验证通过**。不是只跑 Rust。CI 与本文件命令一致。

实现票做完立刻跑门禁，再开对应 review 票。review 票只核对仓库里已经为真的项；门禁没过不得标 `done`。

## 门禁命令

在仓库根目录执行。某栈本票未改到，仍建议全跑（CI 每次全跑）。

### Rust（`crates/`、`Cargo.toml`）

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

`dead_code` / clippy 警告禁止 `#[allow]` 糊过去：删未用代码，或改成真正被调用。

### Python（`services/docreader`）

```bash
# 仓库根：与编辑器同一套 pyrightconfig.json
uvx --from pyright pyright .
cd services/docreader
uvx ruff check .
uv run --with pyright pyright .
PYTHONPATH=.. uv run --with pytest pytest tests/
```

`ruff` 规则钉在该目录 `pyproject.toml`（`F` + `E` + `W`，行宽 `E501` 除外）。仓库根与 `services/docreader` 各有一份 pyright 配置，都指向 DocReader 的 `.venv`，`extraPaths` 解析 `docreader.*`。生成物 `proto/` 不扫。不要为了过门禁关掉实错规则，也不要关 rust-analyzer / pyright 来藏诊断。必须改源码或类型标注，禁止 `#[allow]` / `# noqa` / `type: ignore` 糊过真实问题。

### 其它栈

| 触及 | 命令 |
|---|---|
| `deploy/docker-compose.yml` / `docker-compose.yml` | `docker compose -f deploy/docker-compose.yml --env-file deploy/.env.example config -q` 且 `docker compose config -q` |
| `migrations/*.sql` | 对 compose 上的 Postgres 执行该迁移（至少 `apply` 测过） |
| `services/docreader/scripts/*.sh` | `bash -n <script>` |
| `services/docreader/proto/*.proto` | `bash -n scripts/generate_proto.sh`；生成的 `*_pb2*.py` 与 proto 一并提交 |
| `.scratch/knowledgebrain/spec.md` 或 `docs/system-design.md` | 两份必须逐字一致（先改正文再拷副本） |

仓库里**不要**再引入未进门禁的语言。若必须新增（例如临时 Go），先把 fmt / lint / test 写进本文件并接入 CI，再合代码。

当前 **不**保留 DocReader Go 客户端：解析进程是 Python，worker 走 Rust gRPC。

## Review 票核对顺序

1. 门禁：上表命令在本工作区已通过（或 CI 绿）。
2. 对照 brain / 规格：本票勾选项在仓库里为真。
3. 偏差：未做完的保持 `[ ]`，写进 Comments，不得把 Status 写成 `done`。

## 禁止

- 只跑 `cargo test` 就宣称任务完成。
- 用 `#[allow(dead_code)]` / `#[allow(clippy::…)]` 代替删除或修改。
- 只改 Rust、放过同票改过的 Python / SQL / compose / proto / 脚本。
- CI 没接上的检查却在工单里打钩。
