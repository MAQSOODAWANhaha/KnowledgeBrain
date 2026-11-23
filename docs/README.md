# KnowledgeBrain 文档

招标分析与编制跑在 Python（`services/docreader`）。Rust 提供 API、知识库和共享服务，不再跑招标流程。维护规则见 [AGENTS.md](../AGENTS.md)「文档先行」：先改权威页再改代码，只写现在时禁打补丁，一件事只写一处，过期就删。

## 权威表

每类内容只在一页写。别处只放链接，冲突时改这一页。

| 内容 | 权威页 |
| --- | --- |
| 工作约定与全项目路由 | [AGENTS.md](../AGENTS.md) |
| 近期高频错题（活规则） | [LESSONS.md](../LESSONS.md) |
| 全项目工程实现规范、变更五问与 L0–L3 验收矩阵 | [工程与验收标准](code-standards.md) |
| 产品目标、用户、原则 | [PRODUCT.md](../PRODUCT.md) |
| 外壳视觉（颜色、字体、几何） | [DESIGN.md](../DESIGN.md) |
| 工作台交互与稿件行为 | [前端工作台](design/frontend-workbench-design.md) |
| Python 招标文档入口 | [python-tender-phase1/](design/python-tender-phase1/README.md) |
| 分析节点、放行条件、分析发送次数 | [业务工作流](design/python-tender-phase1/business-workflow.md) |
| 编制各阶段的模型调用次数 | [业务工作流 · 编制实际模型调用](design/python-tender-phase1/business-workflow.md#编制实际模型调用) |
| 编制职责、缺资料处理 | [编制架构](design/python-tender-phase1/authoring-architecture.md) |
| 字段、类型、枚举、编制合同 | [字段字典](design/python-tender-phase1/field-dictionary.md) |
| 分析模块接口和执行边界 | [领域模块合同](design/python-tender-phase1/domain-contract.md) |
| 业务验收口径与最低验收线 | [验收](design/python-tender-phase1/acceptance.md) |
| 语义评测语料与方法 | [语义样本](design/python-tender-phase1/semantic-cases.md)、[语义评测](design/python-tender-phase1/semantic-evaluation.md) |
| 现象、根因、改法和验证 | [问题记录](design/python-tender-phase1/bug-fix.md) |
| 部署状态与各样本当前基线 | [交接](design/python-tender-phase1/handoff.md) |
| Python 本机启动和运维命令 | [运行手册](design/python-tender-phase1/runbook.md) |
| Word 编辑、保存、版本、出件 | [bidding/](bidding/README.md) |
| 知识库领域、检索端口、crate | [knowledge-base/](knowledge-base/README.md) |
| 平台能力与归属 | [platform/](platform/README.md) |
| 数据库迁移与命名 | [数据库设计](design/database-initialization.md)、[命名约定](design/database-naming.md) |
| 部署、本机栈端口与发布 | [deploy/README.md](../deploy/README.md)、[发布入口](../deploy/release-entry.md) |

## 目录

```text
README.md  CLAUDE.md  AGENTS.md  LESSONS.md  PRODUCT.md  DESIGN.md
docs/
  README.md                     本页
  code-standards.md             实现规范、变更五问与 L0–L3 验收矩阵
  platform/README.md            共享平台
  knowledge-base/               README、domain、crate
  bidding/                      README（含来源格式进 Word）、onlyoffice（编辑、保存、版本、PDF）
  design/
    database-initialization.md  database-naming.md  frontend-workbench-design.md
    python-tender-phase1/       招标分析与编制
plans/README.md                 进行中的计划（做完即删）
deploy/README.md  release-entry.md
```

## 写法

1. 离线通过、正在跑的镜像版本、真实文件验收，三件分开写，不能用一件代替另一件。
2. 运行结果（run id、Token、镜像号、套件次数）只进 [交接](design/python-tender-phase1/handoff.md)；问题只追加原编号，不另写复盘。
3. 硬阻断只阻止确认 `analysis-ready`，不拒绝保存、预览或导出。
4. 修改任何模块的协议、流程、分包、Prompt 或 LLM 链路前，先答 [工程与验收标准 · 变更五问](code-standards.md#3-协议与-llm-链路变更五问全项目适用)：结论并入对应权威页，逐条答辩写进 Commit Body / PR 或 `plans/`。
5. 修改任何文档后运行 `python3 scripts/ci_check_docs.py`，确保 0 断链、0 孤儿文档、0 失效源码路径、无违规 `/tmp/` 或补丁措辞。
