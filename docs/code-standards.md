# 工程实现与验收标准

本页收口**全项目**（共享平台、知识库、招投标、DocReader 与 Web）统一的强制实现纪律、文档对齐检查、变更五问与 L0–L3 验收口径。
所有分支均以本页与 [AGENTS.md](../AGENTS.md)、[LESSONS.md](../LESSONS.md) 为基线；招标业务验收线见 [招标验收](design/python-tender-phase1/acceptance.md)，本机栈与部署配置见 [deploy/README.md](../deploy/README.md)。

## 1. 文档与实现强对齐（每次改动必查）

1. **先改权威页再改代码，同 Commit 提交**：修改协议、字段、接口、CLI、环境变量、端口、UI 交互或部署方式时，在 [docs/README.md](README.md) 对应的权威页同步更新。
2. **只写现在时，禁打补丁**：直接改写或删除旧句，禁止写「若旧句要求 X，以本句为准」「本轮尚未部署」「旧方案如下」。
3. **核对源码事实**：文档中出现的仓库文件路径、环境变量名、端口、CLI 子命令、Python/Rust/Node 版本必须与代码和 CI 一致；不写易过期的行号。
4. **机器守卫**：所有提交必须通过 `python3 scripts/ci_check_docs.py`。

## 2. 通用实现、缺陷与安全纪律（全项目适用）

- **单 Commit 单目标**：不同主题或顺手修的无关缺陷拆成独立 Commit，便于审计和回滚。
- **兄弟点清查**：修缺陷前全仓 `rg` 同类模式、调用方、路由、表或视口，同一次改动修齐。
- **失败即未知**：解析失败、读文件失败、映射缺失或模型回包不完整必须显式报错或留 `unresolved`，绝不静默降级为空列表或 0 项。
- **先记问题再修**：发现缺陷先写明现象、影响、根因和改法（根因未查实明确标待查），修复后补齐实际测试、部署与真实流程验收状态；招标缺陷记录在 [bug-fix.md](design/python-tender-phase1/bug-fix.md)，动态运行状态只维护在对应交接页（如 [handoff.md](design/python-tender-phase1/handoff.md)）。
- **证据与密钥边界**：运行证据、抓包与回放素材统一放在仓库外目录（如 `/opt/workspace/kb-tender-evidence/`），禁止放 `/tmp`；严禁提交业务原文、完整请求或密钥。
- **独立 Worktree + 树完整性自检**：多会话并行时每个任务使用独立 git worktree；每次 `git commit` 后立刻核对 `git show --stat HEAD` 与 `git diff HEAD^ HEAD --stat`，确认改动列表与提交主题一一对应，没有混入无关文件或因旧树覆盖掉他人提交与测试。
- **管道防吞错**：所有串联测试或日志截断命令必须开启 `set -o pipefail`（如 `set -euo pipefail`），禁止 `test | tail && git push` 掩盖非零退出码。
- **棘轮只降不升**：仓库内已建立的静态检查基线、断链白名单与迁移清单只允许缩减或清零，不允许新增豁免或放宽预算。

## 3. 协议与 LLM 链路变更五问（全项目适用）

修改**任何模块**（知识库解析/摘要/问答/Wiki/图谱、招投标分析/核验/编制等）的协议、流程、分包、Prompt 或模型调用链路前，必须先回答以下五个问题。**最终生效的规则与调用公式**写入对应领域权威页（如招标写入 [业务工作流](design/python-tender-phase1/business-workflow.md)），**逐条五问答辩**写入 Commit Body / PR 或 `plans/`，不把历史答辩日志永久堆积在权威设计页：

1. 全流程模型调用次数的公式是什么，随文件或数据规模如何增长？
2. 单次请求最坏情况下的输入和输出边界是什么？
3. 会新增哪些失败或未决问题，各由谁通过什么出口解决（不能只有阻断没有出口）？
4. 修订或局部变更一次需要重跑多少调用？
5. 是否要求模型誊写宿主已经知道或可推导的内容？

## 4. 分栈红线

| 技术栈 | 强制要求 |
| :--- | :--- |
| **Rust (`crates/`, `migrations/`)** | API/Worker/Retention 严禁执行业务 DDL；已提交迁移 SQL 不可改写；资源按 `owner_user_id` 严格隔离；不吞错误。 |
| **Python (`services/docreader/`)** | 基准版本锁定 **Python 3.11**（对齐 `deploy/Dockerfile.docreader`）；阻塞解析/规划不得占异步事件循环；严禁模型誊写宿主已知元数据；Prompt 与宿主校验口径必须双向测试对齐。 |
| **Web (`web/`)** | 遵循 [DESIGN.md](../DESIGN.md)；不破坏保留 `testid`；未保存编辑不得被后台轮询静默覆盖；布局改动须验证窄屏与宽屏断点。 |

## 5. 四级验收矩阵（全项目适用，严禁互相替代）

| 级别 | 必跑命令与范围 | 触发条件 |
| :--- | :--- | :--- |
| **L0 静态与守卫** | `python3 scripts/ci_check_docs.py`、`bash scripts/ci_check_sources.sh`（加载 `.github/ci.env` 与 `DEFAULT_BRANCH=main`）；Rust `cargo fmt --all -- --check` + `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`；Python `ruff` / 类型检查；Web `npm --prefix web run lint` + `npm --prefix web run build` | **每次提交必跑**（按触碰栈执行 + 文档/CI 守卫） |
| **L1 离线单测 + 反证** | Rust `cargo test --locked --workspace`；Python `uv run --frozen pytest tests/unit -q`；Web `npm --prefix web test` | **每次提交必跑**（触碰哪一栈跑该栈**全量**单测，禁止只跑 `-k` 代替） |
| **L2 隔离集成与 E2E** | Rust 隔离 PG/Redis 验收（见 `.github/workflows/ci.yml`）；Python `uv run --frozen pytest tests/workflow tests/acceptance -q`；Web `npm --prefix web run test:e2e`；Word/PDF 变更跑隔离 ONLYOFFICE 探针 | 改 DB/队列/API/跨进程桥接/前端页面/Word 导出时**合并前必跑** |
| **L3 真实业务验收** | 真实文档与业务链路验收 + 人工原文抽查（招标线见 [验收](design/python-tender-phase1/acceptance.md)），证据落仓库外目录 | 协议/Prompt/检索/核心导出变更、里程碑放行前 |

**验收铁律**：
1. **Red → Green 反证测试**：所有 Bug 修复和守卫规则必须提供「回退改动会真实 `FAIL`（编译错或 skip 不算）」的测试，并在 Commit Body 记录失败表现。
2. **禁止借口跳过**：`main` 上本来就红或“单跑过、全跑挂”是当前要修的缺陷，不得跳过；离线测试必须经 `python3 scripts/run_local_tests.py -- <cmd>` 剥离真实模型凭证，严禁触网或读真实账号；同机只串行跑一个全量套件。
3. **跑完再记**：验证结果必须是**当前 HEAD 跑完后的最终结果**，不得记录中途进度或旧树结果；未跑的检查必须显式列出。

## 6. PR 评审、锁头合入与发布判定

1. **PR 语义变更说明**：PR 描述须写清触发条件、Before/After 可观测行为变化、同步更新的权威页链接、实际验证证据及明确未跑项。
2. **评审必实跑并反证**：只看 diff 不算评审。评审人（含 Agent 评审）须拉取分支在当前 `main` 上实跑对应 L0–L2 检查，并故意回退或破坏关键逻辑（Break it on purpose）确认新测试真实报错。
3. **锁头合入与留痕**：禁止合入 Draft PR；合入前在 PR 评论或提交记录写明已评审的 HEAD SHA 与实跑结果，并用 `gh pr merge --match-head-commit <reviewed-sha>`（或等价锁头校验）合入；评审后若有新 push 须重审新 HEAD。
4. **CI 与发布判定**：被后续 push 取消的 CI run 不算通过；`main` 上的 CI run 禁止被取消（由 `.github/workflows/ci.yml` 强制）。仅打 tag、推送镜像或运行 `knowledgebrain-release --check-only` 不算发布完成，须按 [deploy/README.md](../deploy/README.md) 核验运行镜像 RepoDigest、迁移退出 0 与 `/ready` 健康探针。

## 7. Commit Body 强制验证模板

每个非纯拼写修正的 Commit，body 末尾必须包含：

```text
Verification:
- Red -> Green: <测试名> (reverted failed with <关键断言/报错；新功能/文档整理写 N/A 及原因>)
- Ran [L0/L1/L2]: <当前 HEAD 实际跑完的命令与结果摘要>
- Not run: <未跑的检查及原因，如 L3 live run>
```

## 8. 复盘与规则晋升

每个里程碑收口或同类缺陷第二次出现时，按 [LESSONS.md](../LESSONS.md) 顶部 `reviewed-through` 游标增量审查期间提交与缺陷记录：
- 返工提交、验证缺口、文档漂移 → 记入 `LESSONS.md` 并递增 `Seen`；
- `Seen ≥ 3` → 晋升进本页，或直接写成自动化 Guard Test 挂进 CI，同时从 `LESSONS.md` 移出。
