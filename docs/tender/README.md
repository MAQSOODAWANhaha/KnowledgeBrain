# 招投标文档入口

分析走 `register → freeze → extract → verify → confirm`，确认后是 `analysis-ready`。编制是独立管道，先跑大纲执行，再跑正文执行，产出可编辑 DOCX 并经 ONLYOFFICE 编辑、保存与同版 PDF 出件。

| 内容 | 文档 |
| --- | --- |
| 流程节点、模型调用、编制架构与证据/模型层接口 | [业务工作流与架构](workflow.md) |
| 字段、类型、枚举与协议合同 | [字段字典](field-dictionary.md) |
| 工作台交互、来源格式进 Word 与 ONLYOFFICE 保存/PDF | [工作台与 Word 编辑](workbench.md) |
| 业务验收线与合成语义评测 | [验收与语义评测](acceptance.md)（语料：[semantic-cases.json](semantic-cases.json)） |
| 本机启动、CLI 运维与自动验收命令 | [运行手册](runbook.md) |
| 当前活跃未关闭缺陷清单 | [问题记录](bug-fix.md) |

## 当前状态（交接）

动态进展只维护在本节（不抄 run id、Token 或套件次数）；证据统一保存在仓库外 `/opt/workspace/kb-tender-evidence/`。

- **真实文件验收**：四份真实成品（Bidding、安服、111、222）的最低验收线仍是 **0 / 4**，离线单测不能代替。
- **分析主线**：`verify` 只发 `review`（`locate`、`associate`、`joint` 已退役），已在候选环境验证；缺页和漏报原子由宿主记待核。
- **安服 S1/D7**：当前分析候选，在隔离冷库通过 CLI 修订、幂等与冲突检查，定点核验首发全部通过宿主校验；待做整份核验根、人工确认与项目采用，P24/P36/P75–80 偏差待修（见 [BUG-318](bug-fix.md#bug-318履约交付的应急预案归入投标提交文件)）。
- **W2 一条要求链（`single_requirement_chain_v1`）**：新的自动大纲默认走这条链。已验证大纲、原式出件（含封面、P91 项目名称填格、P113 网格）、评分待核、工作台评标裁决抽屉、方案定点写作与显式项目背景绑定（写作使用合成材料与夹具回包）；项目背景选择目前仍只走保存接口。真实企业资料与整份业务验收待完成。旧目录合同何时仍会使用见[工作流](workflow.md#编制目标设计一条要求链)。
- **已知待复测项**：安服紧凑结构复核估算高于本地输入额度记软告警、尚未实发；`111` 待按 [BUG-250](bug-fix.md) 重放；`Bidding` 待按 [BUG-249](bug-fix.md) 重新导出。
