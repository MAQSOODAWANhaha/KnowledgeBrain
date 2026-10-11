# 招投标运行与故障处理

执行与配置以[运行时合同](../design/evidence-runtime-contract.md)为准，未解决缺陷见[实施计划](../design/check-execution-plan.md)。

运行前核对构建 SHA、冻结输入、模型/工具/提示/配置摘要及检查点合同。同合同恢复不能重置历史 usage；合同改变从新状态执行，不修改旧摘要冒充恢复。凭据由环境继承，不展示或归档完整环境。

- 解析：核对来源顺序、表格、图片及覆盖清单。解析完成不是大纲验收通过。
- Discover：区分 domain Failed、可纠正业务拒绝、worker terminal 和 pending transport 状态。默认六次的可配置拒绝计数阈值仍在已推基线中；拟替代代码未发布。不要手改 claim 或重启 terminal 掩盖失败。
- 传输：Prepared / Sending / Received / commit 分开核对；Received 重放不再调用服务。未知发送不能当零成本退款。
- Organize / Check：核对要求、章节、槽、绑定、履行关系及 fresh 读取/复核；仅有章节或 finished 标记不能替代最终产物验证。
- 容量与授权：完整请求准入与累计授权是两道门。不得改窗口、重置账本或自动换供应商绕过失败；没有旧 resume_budget 操作接口。
- 保存与导出：回调持久化才算保存，PDF 绑定同一 DOCX 版本；失败不能用旧正文冒充当前结果。

报告必须注明运行版本、停止原因、在途/未知请求、实际 usage 和未测阶段。私有证据按运行身份保存，不删除用户数据、不以排障名义部署或清库。
