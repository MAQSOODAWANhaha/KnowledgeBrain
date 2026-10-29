# 招投标文档入口

当前招标分析与编制统一使用 [Python 工作流](../design/python-tender-phase1/business-workflow.md)，字段以 [字段字典](../design/python-tender-phase1/field-dictionary.md) 为准，实施与实际验证见 [交接文档](../design/python-tender-phase1/handoff.md)。

旧 Rust 招标工作流、专用 API、worker、表和存储过程已删除，不保留兼容运行入口。Rust 的 API 桥接、知识库、对象存储和通用后台服务继续使用。全新初始化及创建人权限见 [数据库设计](../design/database-initialization.md)。

本目录中的旧 analysis / outline / response、冻结大纲、旧 API 和旧 SQL 文档仅供历史追溯，不再作为执行说明。发布工具边界仍见 [发布入口](release-entry.md)；当前 Python 与 Word 接入的部署状态以交接文档为准。
