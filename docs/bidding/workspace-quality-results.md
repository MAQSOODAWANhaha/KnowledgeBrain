# 工作区质量检查与修复

> 历史验证记录：仅说明所记录版本的结果，不是现行设计或新流程验收。当前方案与任务见 [统一方案](../../plans/bidding/product-two-phase.md)。

无测试标记的出件工具切片：Python 语法与 diff 空白检查通过。使用实际 App/PG/Redis/ONLYOFFICE 和已有合成稿，目录更新、保存重开及同版本 PDF 的整理用例 **1/0/0/0**，随后原完整编辑/历史/PDF/四次停服回调重放 **1/0/0/0**；两轮脚本摘要等于当前源码，临时资源均清理零错误。首轮错误的无修改关闭 key 假设及失败日志保留，仅修正验收脚本。正文文字变更负向检查及两页 PDF 目视检查完成；这不是实际招标内容验收或 O2 产品导出。仅修改现有 Python 验收脚本，未重复未变化的 Rust/前端检查。见[无标记出件结果](onlyoffice-product-results.md#2026-09-09-只更新目录的-docxpdf-出件验证)。

SSE 完成边界切片：本地真实 HTTP 服务复现“完整 `[DONE]` 已送达但连接未关闭时仍超时”；异步 Agent 传输改为在完整终止事件后返回，复用原有工具解析器。逐字节分块（含中文/扩展汉字）、LF/CRLF、未完成终止事件和既有精确请求/HTTP 错误/时限回归 **11/11** 通过。fmt、全目标/全特性 check、严格 Clippy、样稿入口构建及完整默认 Rust 测试通过，日志统计 **606 passed、0 failed、16 ignored**；源码摘要在检查前后相同。首次沙箱监听拒绝与获准后旧代码红色复现均保留。证据见 [`sse-terminal`](../../artifacts/bid-full-sample/provider-diagnostics/sse-terminal/)。该修复消除已复现的传输误报，不证明旧真实提取超时的原因；新真实运行及语义验收单独跟踪。

Redis reset 只读前置切片：全工作区 fmt、全目标/全特性 check、Clippy `-D warnings` 全部通过；完整默认 Rust 测试日志统计 **604 passed、0 failed、16 ignored**。新增忽略的服务用例另在专属 Redis 中显式执行，**1 passed、0 failed、0 ignored、0 filtered**，验证实际数据库/服务身份、只读 ACL、权限不足拒绝及目标/其他部署/旧键不变；临时服务清理零错误。首次编译参数借用错误与修正后日志均保留。证据见 [`namespace-reset-redis`](../../artifacts/namespace-reset-redis/)。未修改 Python/前端、增加 migration 或实现删除入口；R2 及真实招标整稿验收仍未完成。

本地对象 reset 只读前置切片：全工作区 fmt、全目标/全特性 check、Clippy `-D warnings` 全部通过，完整默认 Rust 测试日志统计 **604 passed、0 failed、15 ignored**。新增三项实际文件系统回归与既有确认校验和用例定向 4/4；验证目录替换、符号链接拒绝、缺失子目录不创建和外部/其他 namespace 数据保留。复用共享对象访问代码后执行了完整 Rust 默认回归，未修改/重跑 Python 或前端；未新增 migration、删除入口或部署。证据见 [`namespace-reset-objects`](../../artifacts/namespace-reset-objects/)。真实招标样稿仍受大请求超时影响；最新 Rust 小型工具诊断成功不构成恢复或完整验收，详见[样稿记录](full-sample-results.md)。不能以本轮质量结果代替完整验收。

目录页码验收工具切片：Python 入口语法检查通过；最终完整真实目标 **1/0/0/0、exit 0**，所有临时资源清理 exit 0。正常界面更新目录后，保存的标题、页码、书签和实际 PDF 页核对通过；原始引擎文件的缺页码/错书签/改标题破坏检查全部拒绝。仅修改验收脚本，未修改产品 Rust、前端、生成器或解析服务。两页合成验证不构成实际招标完整验收。证据见[目录计算与保存结果](onlyoffice-product-results.md#2026-09-09-目录计算保存书签与实际-pdf-页码)。

同稿 PDF 验收工具切片：现有 Python 入口语法检查通过；最终脚本的真实 App/PG/Redis/ONLYOFFICE 目标 **1 passed、0 failed、0 ignored、0 filtered**，保存、同稿转换、重开编辑及历史文件检查通过，临时服务清理 exit 0。仅修改验收脚本，未修改产品 Rust 或前端；不重复未变化的全工作区测试。合成 PDF 与来源 DOCX 摘要绑定，统一 Python 服务回读及两页目视证据已归档；真实招标完整模板和产品导出仍未验收。见[同稿 PDF 工具结果](onlyoffice-product-results.md#2026-09-09-同一保存版本的-pdf-验收工具)。

共享 HTTP 错误诊断切片：400/413/429/503 的截断错误正文回归通过；完整默认 Rust 测试日志统计 **601 passed、0 failed、15 ignored**，fmt、严格 Clippy 及样稿入口构建通过。未声明共享工作区在其他写入下整体摘要不变。随后仅修改本地验收 Journal，修复累计调用数误作单轮尝试次数，并保留重启后的三次上限；专属测试 1/1、全工作区 fmt、入口严格 Clippy/构建通过。证据见 [`provider-diagnostics`](../../artifacts/bid-full-sample/provider-diagnostics/)。这些是代码质量证据，真实招标提取及整稿验收仍未通过。

本次针对当前 main 工作树复现并修复 fmt、Clippy、测试接线及生产镜像构建问题。保留继承改动，未提交；结果是本地开发检查，不等于 hosted CI、完整基础设施验收或真实 Agent 整稿验收。

O1 产品整链补充：已有生成验收脚本接入实际 App 与 ONLYOFFICE，最终隔离运行通过 3 轮编制、45 次本机脚本工具调用，以及浏览器生成后的两次保存、关闭重开和实际单元格编辑。修复测试按旧对象目录读取的问题，目录由 Rust 平台 `blob_path` 提供，不在 Python 复制 namespace 规则。fmt、全工作区全目标/全特性 check/严格 Clippy、二进制构建及表格校验器 2/2 通过；真实数据库夹具 1/1。此次未改产品业务逻辑，未重复完整默认测试；下方 598/0/15 是前一轮结果。完整证据及语义边界见[同次产品验收](docx-composition.md#浏览器生成至实际-office-保存的同次验收)。

最新 O1 模板校验切片：fmt、全工作区全目标/全特性 check、Clippy `-D warnings` 通过；完整 Rust 默认测试 **598 passed、0 failed、15 ignored**。首次沙箱测试因本地 HTTP/gRPC 监听权限失败，获准本地监听后原范围重跑通过，未跳过失败测试或放宽断言。验证前后 Rust 源码及 Cargo 清单/锁文件摘要一致；本轮未改动或重跑 Python/前端。证据见 [`artifacts/bid-full-sample/template-policy/`](../../artifacts/bid-full-sample/template-policy/)；真实 Agent 整稿仍未验收。

最新 PostgreSQL reset 只读前置检查切片：相同范围的 fmt/check/Clippy 全部通过，完整 Rust 默认测试 595 passed、0 failed、15 ignored；新增专属 PostgreSQL 必跑用例 1/1 通过，覆盖真实 baseline/receipt、映射、其他连接及漂移拒绝，临时服务清理零残留。详见[PostgreSQL 前置校验](../platform/namespace-isolation.md#postgresql-reset-只读前置校验)。该结果不表示 reset 执行器或真实 Agent 整稿验收已完成。

最新 Redis namespace 切片已重新通过同样的 fmt/check/Clippy 范围及完整 Rust 测试：594 passed、0 failed、14 ignored。新增两项隔离用例已在专属 Redis 中显式执行，另有 4 项原生故障和 14 项 producer 契约通过；两轮临时服务均清理零残留。详情及当前源码摘要见[Redis 隔离补充](../platform/namespace-isolation.md#redisoxana-与多模态计数器隔离补充)。本轮未重跑未修改的 Python/前端。

后续 OBJECT_DIR/MinIO 隔离改动已重新验证：fmt、全工作区全目标/全特性 check、Clippy `-D warnings` 通过；完整 Rust 测试 594 passed、0 failed、12 ignored，专属 MinIO 必跑 3/3。最后一轮完整测试与 Clippy 期间源码摘要无漂移。早先沙箱监听失败及共享源码变动产生的格式差异均保留证据，详见[对象隔离补充](../platform/namespace-isolation.md#object_dir-与-minio-隔离补充)。下表保留原轮次结果；本轮未修改或重跑 Python/前端，也不将此前运行镜像作为新代码的验收证据。

## 修复原因与实现

- Rust 格式未统一：执行工作区 rustfmt。
- Clippy：对象并发测试将相关对象参数归为同一结构；worker 启动测试在失败路径终止并等待子进程，避免遗留进程。未新增 lint 豁免。
- 默认 Rust 测试曾强制要求未启动的真实 DocReader 和本机私有文件：将该契约放入显式 `docreader-contract-tests` feature，CI Python job 必跑 `scripts/docreader_replay_acceptance.py`。脚本启动已有 `DocReaderServicer`，生成本地测试文件并执行 Rust gRPC 契约，缺前置或没有实际执行一个成功测试均失败。真实文件路径通过参数传入。
- Python SSRF 单测原先依赖外部 DNS：通过标准 DNS mock 验证公开地址、解析失败、混合内外网地址的处理，生产 SSRF 校验保持原样。沙箱缓存目录及监听权限造成的失败按测试环境问题处理。
- Node 22/npm 10 的生产构建发现 lockfile 缺失 esbuild optional peer 包：用同版本 npm 补齐缺项。此项修复仅新增 27 个 lock entry，未改已有 entry 或 package.json；全工作树还包含此前的前端撤旧改动。
- Rust Docker 构建加入 `--locked`。真实 R1 启动发现没有显式 `environment` 的依赖服务被错误拒绝：仅在 Compose 声明环境映射时逐项核对，同时保留 Rust 发布身份的独立必填检查；增加无显式环境的依赖启动及重放回归。

## 不硬编码的落实范围

已删除按固定签名词、角色排序及机械切分来源区间的 `repair_template_overlaps.rs` 辅助程序。这类规则无法判断真实招标要求，也不能替 Agent 修补证据后重绑审查摘要。原始真实提取结果及独立拒绝结论继续保留。

另删除 `repair_grid_policies.rs`：它将未分配的空单元格自动设为投标人填写区，并生成“无内容填 /”指令，再机械重绑关系和复核摘要。空格角色及填写规则必须由 Agent 依据原文判断。现有 Template 校验增加两处与 DOCX 编译器一致的检查：同一网格的 regions 必须连续，非网格正文必须有可编辑解析文字。错误在 `put_record` 返回且不改变分析/覆盖状态；不自动排序、拆表或补写正文。新增三项回归覆盖这两处缺口及空格显式保留角色；投标模块 100 passed、0 failed、1 ignored。原页图像仍可作为证据，缺少可编辑文字时保留 unresolved，继续复用统一 Python 解析服务。未新增框架、配置项或 migration。

招标章节、顺序、附表关系和适用条件仍须来自源文件及 Agent 提取、复核；模型运行配置继续读取 `deploy/.env`，解析继续复用 Python DocReader。本次定向检查覆盖提取/编制源码、样稿 examples 与运行入口，未发现被删除脚本的调用或固定“商务、技术、报价”生成序列。该搜索不构成所有语义逻辑的全面审查。CI 合成文档仅验证解析重放，不进入业务生成路径。

## 本地验证结果

失败与最终日志的脱敏副本保存于 [`artifacts/workspace-quality/`](../../artifacts/workspace-quality/)，原文件及归档副本摘要、脱敏匹配次数见 `manifest.json`。原始日志保留在各条目记录的本机 `/tmp` 路径；真实配置及原始 Compose 未归档。

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo check --locked --workspace --all-targets --all-features` | 通过 |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | 通过；CI 已使用同样的目标及 feature 范围 |
| `cargo test --locked --workspace --no-fail-fast` | 586 passed、0 failed、9 ignored |
| Python `pytest tests/` | 150 passed、13 skipped、5 warnings |
| 必跑 DocReader gRPC 重放 | 合成 DOCX/PDF 1/1；真实 DOCX/PDF 1/1；未调用外部模型 |
| 前端 lint、Node 22 干净容器 build/test | 通过；33 passed、0 failed |
| Draft 2020-12 Schema | 10 份 Schema，正反例通过 |

Rust 使用 1.97.0，本机附加 `--offline --target-dir /tmp/kb-retire-target`。默认 Rust 测试中，部分基础设施契约在未提供专用数据库/Redis 配置时提前返回；上述计数不能用来证明这些集成断言已执行。Python 跳过项涉及额外测试文件及 LibreOffice/ImageMagick 等未具备的依赖，警告来自已有多线程进程中的 fork。Rust 依赖 `proc-macro-error2` 仍有未来版本兼容提示，当前 Clippy 检查通过。

DocReader 必跑入口（先按现有服务说明安装锁定依赖）：

```bash
cd services/docreader
uv run --frozen python ../../scripts/docreader_replay_acceptance.py
# 本机真实验收时，两个参数都必须提供，并使用实际文件路径：
uv run --frozen python ../../scripts/docreader_replay_acceptance.py --docx "$DOCX_INPUT" --pdf "$PDF_INPUT"
```

真实输入摘要分别为 `4a7c68d8c20688a12f7e978e97abbbd5b2cec116728ad3f3f639558c8091b47d` 和 `4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`。契约验证相同 DOCX 成功重放不增加实际转换次数，替换成另一 PDF 才发生第二次转换；其中视觉调用使用测试替身，未验证模型识别质量。

质量修复时 R1 首次隔离启动失败；后续已完成修复后的真实启动、只读重放及对象生命周期验收，详见[真实隔离记录](release-live-results.md)，干净发布候选仍待落实。完整 Agent DOCX 及同版本 PDF 仍未生成，原始提取仍被拒绝，外发已获授权；最新 v5 因 HTTP 503 终止，详见[样稿记录](full-sample-results.md)。

## R3 queue-faults CI 接线

`.github/workflows/ci.yml` 新增计划中的独立 `queue-faults` 岗位，复用现有 `oxana_native_faults` 四项原生测试，不增加队列实现。岗位使用独立 Redis service 和动态本机端口，强制 `KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS=1`，清除子进程专用变量后执行完整测试目标。`pipefail` 保留 cargo 失败，日志必须有非零通过数且 failed/ignored/filtered 均为零；出现测试的两类 skip 消息直接失败。日志无论成功失败均归档，缺日志使归档步骤失败。镜像 job 的 `needs` 增加该岗位；没有提交或触发发布。

从实际 YAML 提取同一段 shell，9 项控制流验证覆盖成功、cargo 失败、零用例、ignored、filtered、failed、缺结果及两类 skip。随后在专属 Redis 容器执行这段 shell，真实 **4 passed、0 failed、0 ignored、0 filtered**，step exit 0；覆盖唯一任务重复提交、崩溃复活、Housekeep 不复活和 dead revive。临时容器 `/data` 使用 tmpfs，无持久卷；删除前校验 owner，删除后查询本轮标签无残留。原有各 job 的 YAML 结构保持不变，仅增加镜像依赖。

证据见 [`artifacts/queue-faults-ci/`](../../artifacts/queue-faults-ci/)。这证明本地原生故障执行与 CI 接线，不能宣称 hosted job 或分支保护已通过。R3 其余 named jobs、同一干净候选与 hosted 验收仍缺；真实招标整稿的外发授权、提取补正及高质量验收状态不变。本轮仅改 CI 和台账，未重复此前已经通过的全工作区构建测试。
