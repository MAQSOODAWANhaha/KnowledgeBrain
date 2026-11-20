# KnowledgeBrain 文档

招标分析与编制跑在 Python（`services/docreader`）。Rust 提供 API、知识库和共享服务，不再跑招标流程。维护规则见 [AGENTS.md](../AGENTS.md)「文档先行」：先改权威页再改代码，一件事只写一处，过期就删。

## 权威表

每类内容只在一页写。别处只放链接，冲突时改这一页。

| 内容 | 权威页 |
| --- | --- |
| 产品目标、用户、原则 | [PRODUCT.md](../PRODUCT.md) |
| 外壳视觉（颜色、字体、几何） | [DESIGN.md](../DESIGN.md) |
| 工作台交互与稿件行为 | [前端工作台](design/frontend-workbench-design.md) |
| 分析节点、放行条件、分析发送次数 | [业务工作流](design/python-tender-phase1/business-workflow.md) |
| 编制各阶段的模型调用次数 | [业务工作流 · 编制实际模型调用](design/python-tender-phase1/business-workflow.md#编制实际模型调用) |
| 编制职责、缺资料处理 | [编制架构](design/python-tender-phase1/authoring-architecture.md) |
| 字段、类型、枚举、编制合同 | [字段字典](design/python-tender-phase1/field-dictionary.md) |
| 分析模块接口和执行边界 | [领域模块合同](design/python-tender-phase1/domain-contract.md) |
| 验收口径 | [验收](design/python-tender-phase1/acceptance.md) |
| 语义评测语料与方法 | [语义样本](design/python-tender-phase1/semantic-cases.md)、[语义评测](design/python-tender-phase1/semantic-evaluation.md) |
| 现象、根因、改法和验证 | [问题记录](design/python-tender-phase1/bug-fix.md) |
| 部署了没有、真实文件验到哪 | [交接](design/python-tender-phase1/handoff.md) |
| Python 本机启动和运维命令 | [运行手册](design/python-tender-phase1/runbook.md) |
| Word 编辑、保存、版本、出件 | [bidding/](bidding/README.md) |
| 知识库领域、检索端口、crate | [knowledge-base/](knowledge-base/README.md) |
| 平台能力与归属 | [platform/](platform/README.md) |
| 数据库迁移与命名 | [数据库设计](design/database-initialization.md)、[命名约定](design/database-naming.md) |
| 部署与发布 | [deploy/README.md](../deploy/README.md)、[发布入口](../deploy/release-entry.md) |

## 目录

```text
README.md  AGENTS.md  PRODUCT.md  DESIGN.md
docs/
  README.md                     本页
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
2. 运行结果（run id、Token、镜像号、套件次数）只进交接。问题只追加原编号，不另写复盘。
3. 硬阻断只阻止确认 `analysis-ready`，不拒绝保存、预览或导出。
4. 改招标协议、流程、分包或 Prompt 前，先回答 [AGENTS.md](../AGENTS.md) 里的五个问题，并写进业务工作流。
