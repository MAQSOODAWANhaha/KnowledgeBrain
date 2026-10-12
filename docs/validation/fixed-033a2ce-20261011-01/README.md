# 固定版本真实运行验证归档

本轮未验收通过：Discover 4/4 包提交，109 条要求已分配到 12 个章节；Organize 的 slots、bindings、fulfillments 均为 0，Check 调用为 0。运行在下一请求发送前以 `PRIVATE_WINDOW_BLOCKED` 结束。

实际运行源码固定为 `033a2ce513f7a6f0e90bbb6f50c259f79547f386`，运行身份 `fixed-033a2ce-20261011-01`；2026-10-11 11:45:13 UTC 启动、11:59:34 UTC 结束。加入本归档后的文档 HEAD 是后续提交，不能视为新的运行源码或新一次验收。

[工程交接、任务分配和测试证据](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/pull/22#issuecomment-6108874860)；设计仍以[现有合同](../../design/evidence-runtime-contract.md)为入口，本归档不另立合同。

| 文件 | 职责与定位字段 |
| --- | --- |
| [中文报告](033a2ce_私有中文真实验证报告.html) | 汇总真实执行结果、原文核对与未通过项；文件名保留原名，“私有”不再表示本副本的访问权限 |
| [实际中间产物](033a2ce_实际产物_未完成独立验收.json) | `requirements_with_original_evidence` 保留 109 条要求及原文证据；`draft.chapters[].requirement_ids` 可核对章节归属；`draft.slots/bindings/fulfillments` 与 `host_review_report` 可核对未完成门槛 |
| [调用过程与预算结算](033a2ce_调用过程与预算结算.json) | `calls[]` 的阶段、起始时间、耗时、usage、`actions[].tool/mode/requested_refs/host_receipt`；第 19–27 次为 Organize，第 27 次请求 165 refs；`remaining`、`terminal_code`、`assignment` 记录终态 |
| [已有 CPU 只读采样](readonly-performance-sample.json) | 两次采样显示约 1 秒墙钟时间消耗 1.01 CPU 秒；没有函数栈，不能归因为某个函数、tokenizer 或序列化步骤 |
| [文件校验清单](manifest.json) | 实际文件名、字节数、SHA256，以及与原始交付文件逐字节一致的核验结果 |

27 次请求全部返回，0 在途、0 未知 usage。总 input 1,100,856、output 22,500；余额 input 60,828、output 251,528。剩余额度不足以放行当时的下一请求；其精确请求差额未单独持久化。不得重置预算或自动重启，追加预算需要授权。宿主长间隔与 Organize 无响应槽位应先只读诊断；已有资料不含函数级耗时或完整可重放请求。

用户在获知公开 GitHub PR 可被他人读取后，明确批准公开三份指定文件及必要过程信息。本目录三份文件按原内容复制，原私有文件未改动；认证信息复核未检出匹配。本次范围不包括原始 PDF、其他旧私有材料或全量请求响应。先前 Library 401 不影响本目录文件的 GitHub 获取。
