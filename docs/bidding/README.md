# 招投标文档

现行生成合同只有一条：招标解析完成后，一次大纲运行写出**章节大纲和规定模板**（含附件表）。代码在 `crates/bidding/src/outline`。

历史项目专属证据已移至授权私有验收档案。公开验证使用合成输入；本段不声明真实模型语义验收通过。

编辑、保存和下载不定义生成合同：

- [ONLYOFFICE](onlyoffice.md)
- [保存协议](docx-editor.md)
- [版本](docx-rounds.md)
- [运行手册](backend-runbook.md)

[archive/](archive/README.md) 里的 results、acceptance、review、diagnosis、regression 以及旧骨架编译说明，只证明当时那一版做了什么。不要把它们读成第二套大纲设计。

`crates/bidding/src/analysis/outline_flow.rs` 和 `submit_outline_scan`、`put_outline_items`、`submit_outline_check` 是遗留抽取路径，不是这条产品合同。
