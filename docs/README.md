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
| 共享平台能力、数据库迁移与命名约定 | [platform/README.md](platform/README.md) |
| 知识库领域、检索端口与 `crates/knowledge` 边界 | [knowledge-base/README.md](knowledge-base/README.md) |
| 招投标总入口与当前交接状态 | [tender/README.md](tender/README.md) |
| 招投标分析流程、编制架构、实际模型调用与运行时接口 | [业务工作流与架构](tender/workflow.md) |
| 字段、类型、枚举与协议合同 | [字段字典](tender/field-dictionary.md) |
| 工作台交互、来源格式进 Word 与 ONLYOFFICE 保存/PDF | [工作台与 Word 编辑](tender/workbench.md) |
| 业务验收线与合成语义评测 | [验收与语义评测](tender/acceptance.md) |
| 招投标本机启动与 CLI 运维手册 | [运行手册](tender/runbook.md) |
| 当前活跃未关闭缺陷清单 | [问题记录](tender/bug-fix.md) |
| 部署、本机栈端口与发布工具 | [deploy/README.md](../deploy/README.md) |

## 目录结构

`docs/` 按业务领域扁平组织，不使用 `design/`、`bidding/`、`research/`、`validation/` 或带语言/期数后缀的目录：

```text
README.md  CLAUDE.md  AGENTS.md  LESSONS.md  PRODUCT.md  DESIGN.md
deploy/
  README.md                     部署、本机开发栈端口与宿主发布工具
plans/
  README.md                     进行中的计划入口（做完即删）
  improvement-roadmap.md        W1–W5 修复与改进路线（含 W2 一条要求链）
docs/
  README.md                     本页（总导航与权威表）
  code-standards.md             工程规范、目录命名、变更五问与 L0–L3 验收矩阵
  platform/README.md            共享平台、数据库迁移、归属与命名约定
  knowledge-base/README.md      知识库领域、检索端口与 crate 边界
  tender/                       招投标（README[含交接]、workflow、field-dictionary、
                                      workbench[含 Word/ONLYOFFICE]、acceptance[含语义评测]、
                                      runbook、bug-fix）
```

## 写法

1. 离线通过、正在跑的镜像版本、真实文件验收，三件分开写，不能用一件代替另一件。
2. 动态运行结果只进 [招投标入口 · 当前状态](tender/README.md#当前状态交接)；活跃缺陷登记在 [问题记录](tender/bug-fix.md)，闭环即清。
3. 硬阻断只阻止确认 `analysis-ready`，不拒绝保存、预览或导出。
4. 修改任何模块的协议、流程、分包、Prompt 或 LLM 链路前，先答 [工程与验收标准 · 变更五问](code-standards.md#3-协议与-llm-链路变更五问全项目适用)：结论并入对应权威页，逐条答辩写进 Commit Body / PR 或 `plans/`。
5. 修改任何文档后运行 `python3 scripts/ci_check_docs.py`，确保目录、命名、链接与源码路径全部合规。
