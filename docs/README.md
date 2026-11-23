# KnowledgeBrain 文档

招标分析与编制跑在 Python（`services/docreader`）。Rust 提供 API、知识库和共享服务，不再跑招标流程。维护规则见 [AGENTS.md](../AGENTS.md)「文档先行」：先改权威页再改代码，只写现在时禁打补丁，一件事只写一处，过期就删。

## 权威表

每类内容只在一页写。别处只放链接，冲突时改这一页。

| 内容 | 权威页 |
| --- | --- |
| 工作约定与全项目路由 | [AGENTS.md](../AGENTS.md) |
| 近期高频错题（活规则） | [LESSONS.md](../LESSONS.md) |
| 全项目工程规范、目录命名、变更五问、评审与验收矩阵 | [工程与验收标准](code-standards.md) |
| 产品目标、用户、原则 | [PRODUCT.md](../PRODUCT.md) |
| 外壳视觉（颜色、字体、几何） | [DESIGN.md](../DESIGN.md) |
| 共享平台能力与归属 | [platform/README.md](platform/README.md) |
| 数据库迁移与命名 | [数据库初始化](platform/database-initialization.md)、[数据库命名](platform/database-naming.md) |
| 知识库领域、检索端口、crate | [knowledge-base/README.md](knowledge-base/README.md)（[domain](knowledge-base/domain.md)、[crate](knowledge-base/crate.md)） |
| 招投标文档总入口 | [tender/README.md](tender/README.md) |
| 分析节点、放行条件、分析与编制发送次数 | [业务工作流](tender/workflow.md) |
| 编制职责、缺资料处理 | [编制架构](tender/authoring.md) |
| 字段、类型、枚举、编制合同 | [字段字典](tender/field-dictionary.md) |
| 分析模块接口和执行边界 | [领域模块合同](tender/domain-contract.md) |
| 工作台交互与稿件行为 | [投标工作台](tender/workbench.md) |
| Word 编辑、保存、版本与同版 PDF 出件 | [ONLYOFFICE](tender/onlyoffice.md) |
| 来源格式进 Word 规则 | [Word 导出格式](tender/word-export.md) |
| 业务验收口径与最低验收线 | [验收](tender/acceptance.md) |
| 语义评测语料与方法 | [语义样本](tender/semantic-cases.md)、[语义评测](tender/semantic-evaluation.md) |
| 现象、根因、改法和验证 | [问题记录](tender/bug-fix.md) |
| 部署状态与各样本当前基线 | [交接](tender/handoff.md) |
| 招投标本机启动与运维命令 | [运行手册](tender/runbook.md) |
| 部署、本机栈端口与发布 | [deploy/README.md](../deploy/README.md)、[发布入口](../deploy/release-entry.md) |

## 目录结构

`docs/` 按业务领域扁平组织，不再使用 `design/`、`bidding/`、`research/`、`validation/` 或带语言/期数后缀的目录：

```text
README.md  CLAUDE.md  AGENTS.md  LESSONS.md  PRODUCT.md  DESIGN.md
deploy/
  README.md                     部署与本机开发栈端口
  release-entry.md              宿主发布工具边界与验收
plans/
  README.md                     进行中的计划（做完即删）
docs/
  README.md                     本页（总导航与权威表）
  code-standards.md             工程规范、目录命名、变更五问与 L0–L3 验收矩阵
  platform/                     共享平台与数据库（README、database-initialization、database-naming）
  knowledge-base/               知识库（README、domain、crate）
  tender/                       招投标（README、workflow、authoring、domain-contract、field-dictionary、
                                      workbench、onlyoffice、word-export、acceptance、semantic-*、
                                      runbook、bug-fix、handoff）
```

## 写法

1. 离线通过、正在跑的镜像版本、真实文件验收，三件分开写，不能用一件代替另一件。
2. 运行结果（run id、Token、镜像号、套件次数）只进 [交接](tender/handoff.md)；问题只追加原编号，不另写复盘。
3. 硬阻断只阻止确认 `analysis-ready`，不拒绝保存、预览或导出。
4. 修改任何模块的协议、流程、分包、Prompt 或 LLM 链路前，先答 [工程与验收标准 · 变更五问](code-standards.md#3-协议与-llm-链路变更五问全项目适用)：结论并入对应权威页，逐条答辩写进 Commit Body / PR 或 `plans/`。
5. 修改任何文档后运行 `python3 scripts/ci_check_docs.py`，确保目录、命名、链接与源码路径全部合规。
