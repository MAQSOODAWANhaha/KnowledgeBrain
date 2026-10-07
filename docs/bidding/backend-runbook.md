# 招投标运行与故障处理

生成合同见 [大纲运行时](outline.md)。本页只写怎么看一次大纲运行，不保留旧扫描工具的操作步骤。

## 运行前

核对当前构建、数据库基线、冻结输入，以及大纲工具合同摘要（六个工具，`outline-tools-v1.schema.json`）。输入摘要或运行合同变了，检查点不能恢复。模型与凭据来自 `deploy/.env`。大纲运行只有一次成稿。

## 故障定位

- 解析：按文件看来源是否按解析器顺序发布齐全。解析完成不等于大纲已经生成。
- 发现：看 `reading_packs` 里各包是 `pending`、`running`、`failed` 还是 `committed`。`failed` 包的 `PackFeedback` 给出字段路径和错误码；同一包把 `repair` 设为 true 再交。不要用旧的扫描游标判断发现是否做完。
- 模型：区分请求已预约、响应已保存、工具已提交。已保存的响应不能再向供应商发一次。
- 草稿：看 `tool_draft` 的章节、附件绑定、`slots_submitted` 和 `finished`。未绑定附件表时 `finish_outline` 会拒绝。`finished` 为真仍报 `outline is not ready to publish` 时，投影没有通过 `validate_artifact`（例如章节或槽不合法）。空的 `analysis.outline.checks` 不再挡住出场。
- 预算：重试不把已用轮次清零，也不改已冻结的请求去绕过预算。
- 保存与导出：保存以回调落库为准。PDF 从同一份已保存 DOCX 转换。失败要明确报告，不能用旧正文冒充当前版本。

## 记录

历史提取和验收轨迹在 [archive](archive/README.md)。那些记录里的旧工具名、调用次数和继续运行命令不能当作现在的操作依据。不要自动重启已耗尽的旧运行，不要为了文档核对去清库或删用户文件。
