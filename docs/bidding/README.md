# 招投标文档

现行生成合同只有一条：招标解析完成后，一次大纲运行写出**章节大纲和规定模板**（含附件表）。代码在 `crates/bidding/src/outline`。

| 文档 | 写什么 |
| --- | --- |
| [产品需求](prd.md) | 用户流程、三个包的边界、冻结的 `OutlineArtifact` |
| [大纲运行时](outline.md) | 阅读包目标载荷、六个工具、一轮顺序、上下文窗口、模板已并入组织 |
| [模块归属](authoring.md) | `analysis` / `outline` / `response` 各写什么 |
| [未完成与旧路径](../../plans/bidding/outline-gaps.md) | 旧路径已退出产品请求；模板已并入组织；一次成稿验收仍是回归合同 |

编辑、保存和下载不定义生成合同：

- [ONLYOFFICE](onlyoffice.md)
- [保存协议](docx-editor.md)
- [版本](docx-rounds.md)
- [运行手册](backend-runbook.md)

[archive/](archive/README.md) 里的 results、acceptance、review、diagnosis、regression 以及旧骨架编译说明，只证明当时那一版做了什么。不要把它们读成第二套大纲设计。

`crates/bidding/src/analysis/outline_flow.rs` 和 `submit_outline_scan`、`put_outline_items`、`submit_outline_check` 是遗留抽取路径，不是这条产品合同。
