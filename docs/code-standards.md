# 工程实现与验收标准

本页收口跨 Rust、Python 和 Web 的强制实现纪律、文档对齐检查与 L0–L3 验收口径。
所有新分支均以本页与 [AGENTS.md](../AGENTS.md)、[LESSONS.md](../LESSONS.md) 为基线；业务验收线见 [招标验收](design/python-tender-phase1/acceptance.md)。

## 1. 文档与实现强对齐（每次改动必查）

1. **先改权威页再改代码，同 Commit 提交**：修改协议、字段、接口、CLI、环境变量、端口、UI 交互或部署方式时，在 [docs/README.md](README.md) 对应的权威页同步更新。
2. **只写现在时，禁打补丁**：直接改写或删除旧句，禁止写「若旧句要求 X，以本句为准」「本轮尚未部署」「旧方案如下」。
3. **核对源码事实**：文档中出现的仓库文件路径、环境变量名、端口、CLI 子命令、Python/Rust/Node 版本必须与代码和 CI 一致；不写易过期的行号。
4. **招标五问归宿**：改协议、流程、分包或 Prompt 前必答 [AGENTS.md](../AGENTS.md) 五问；**最终生效的规则与调用公式**写入 [业务工作流](design/python-tender-phase1/business-workflow.md)，**逐条五问答辩**写入 Commit Body / PR 或 `plans/`，不永久堆积在权威设计页。
5. **机器守卫**：所有提交必须通过 `python3 scripts/ci_check_docs.py`。

## 2. 实现与提交纪律

- **单 Commit 单目标**：不同主题或顺手修的无关缺陷拆成独立 Commit，便于审计和回滚。
- **兄弟点清查**：修缺陷前全仓 `rg` 同类模式、调用方、路由、表或视口，同一次改动修齐。
- **失败即未知**：解析失败、读文件失败、映射缺失或模型回包不完整必须显式报错或留 `unresolved`，绝不静默降级为空列表或 0 项。
- **独立 Worktree + 边界自检**：多会话并行时每个任务使用独立 git worktree；每次 `git commit` 后立刻检查 `git show --stat HEAD`，确认只包含本任务文件。
- **棘轮只降不升**：仓库内已建立的静态检查基线与迁移清单只允许缩减或清零，不允许新增豁免或放宽预算。

## 3. 分栈红线

| 技术栈 | 强制要求 |
| :--- | :--- |
| **Rust (`crates/`, `migrations/`)** | API/Worker/Retention 严禁执行业务 DDL；已提交迁移 SQL 不可改写；资源按 `owner_user_id` 严格隔离；不吞错误。 |
| **Python (`services/docreader/`)** | 基准版本锁定 **Python 3.11**（对齐 `deploy/Dockerfile.docreader`）；阻塞解析/规划不得占异步事件循环；严禁模型誊写宿主已知元数据；Prompt 与宿主校验口径必须双向测试对齐。 |
| **Web (`web/`)** | 遵循 [DESIGN.md](../DESIGN.md)；不破坏保留 `testid`；未保存编辑不得被后台轮询静默覆盖；布局改动须验证窄屏与宽屏断点。 |

## 4. 四级验收矩阵（严禁互相替代）

| 级别 | 必跑命令与范围 | 触发条件 |
| :--- | :--- | :--- |
| **L0 静态与守卫** | `python3 scripts/ci_check_docs.py`、`bash scripts/ci_check_sources.sh`（加载 `.github/ci.env` 与 `DEFAULT_BRANCH=main`）；Rust `cargo fmt --all -- --check` + `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`；Python `ruff` / 类型检查；Web `npm --prefix web run lint` + `npm --prefix web run build` | **每次提交必跑**（按触碰栈执行 + 文档/CI 守卫） |
| **L1 离线单测 + 反证** | Rust `cargo test --locked --workspace`；Python `uv run --frozen pytest tests/unit -q`；Web `npm --prefix web test` | **每次提交必跑**（触碰哪一栈跑该栈**全量**单测，禁止只跑 `-k` 代替） |
| **L2 隔离集成与 E2E** | Rust 隔离 PG/Redis 验收（见 `.github/workflows/ci.yml`）；Python `uv run --frozen pytest tests/workflow tests/acceptance -q`；Web `npm --prefix web run test:e2e`；Word/PDF 变更跑隔离 ONLYOFFICE 探针 | 改 DB/队列/API/跨进程桥接/前端页面/Word 导出时**合并前必跑** |
| **L3 真实业务验收** | 真实招标文件走 [验收](design/python-tender-phase1/acceptance.md) 与人工原文抽查，证据落 `/opt/workspace/kb-tender-evidence/` | 协议/Prompt/核心导出变更、里程碑放行前 |

**验收铁律**：
1. **Red → Green 反证测试**：所有 Bug 修复和守卫规则必须提供「回退改动会真实 `FAIL`（编译错或 skip 不算）」的测试，并在 Commit Body 记录失败表现。
2. **禁止借口跳过**：`main` 上本来就红或“单跑过、全跑挂”是当前要修的缺陷，不得跳过；离线测试必须经 `python3 scripts/run_local_tests.py -- <cmd>` 剥离真实模型凭证，严禁触网或读真实账号；同机只串行跑一个全量套件。
3. **跑完再记**：验证结果必须是**当前 HEAD 跑完后的最终结果**，不得记录中途进度或旧树结果；未跑的检查必须显式列出。

## 5. Commit Body 强制验证模板

每个非纯拼写修正的 Commit，body 末尾必须包含：

```text
Verification:
- Red -> Green: <测试名> (reverted failed with <关键断言/报错；新功能/文档整理写 N/A 及原因>)
- Ran [L0/L1/L2]: <当前 HEAD 实际跑完的命令与结果摘要>
- Not run: <未跑的检查及原因，如 L3 live tender run>
```

## 6. 复盘与规则晋升

每个里程碑收口或同类缺陷第二次出现时，按 [LESSONS.md](../LESSONS.md) 顶部 `reviewed-through` 游标增量审查期间提交与 `BUG-XXX`：
- 返工提交、验证缺口、文档漂移 → 记入 `LESSONS.md` 并递增 `Seen`；
- `Seen ≥ 3` → 晋升进本页，或直接写成自动化 Guard Test 挂进 CI，同时从 `LESSONS.md` 移出。
