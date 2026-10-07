# 模块归属

生成合同以 [PRD](prd.md) 和 [大纲运行时](outline.md) 为准。本页只写三个包各自拥有什么。

`crates/bidding/src/lib.rs` 的边界：

- `analysis` 读冻结招标文件，产出记录、关系和修复。它不发布章节，也不写知识库响应。共享检查点的回合循环仍放在这里；大纲和响应各自应用自己的工具。
- `outline` 拥有章节树和规定模板槽，收尾后应发布成 `OutlineArtifact`。投影和出场还没接到 `finish_outline` 上，见 [缺口 3](../../plans/bidding/outline-gaps.md)。它不评审分析记录，不读企业知识库。
- `response` 只消费冻结的 `OutlineArtifact`，按响应槽写证据。它不解析招标文件，不改章节，不改模板。对不上证据的槽文本是 `【待人工补充】`。

章节身份是 id。标题可以改，附件绑定跟着章节 id，不跟着标题。一个附件表的 `form_id` 只能出现在一个章节上。

Word 正文是用户保存的文件。大纲产物是冻结的 `OutlineArtifact`，不是另一份会随编辑回写的正文主源。响应结果用 `outline_sha256` 绑回这一份大纲。

检查点保存 `DiscoverWork`、`tool_draft` 和 `phase`。Journal 负责恢复、请求预约和提交。不另建一套随对话增长的语义记忆。
