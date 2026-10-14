# 验收标准与证据分层

状态：基础设施骨架已验证，领域实现进行中。结果必须写命令、环境、提交、实际输出，不将计划写成结果。

## 已执行：骨架（2026-10-14）

Python 3.11.16，PostgreSQL `127.0.0.1:15432` 隔离数据库 `tender_phase1_test`，
Redis `127.0.0.1:16379/1`；不使用原业务表。

- `.venv311/bin/python -m pytest tests/acceptance/test_skeleton.py -q -x`：**4 passed in 5.89s**。
  使用真实 Dramatiq 子进程和 Redis：SIGKILL 后重新投递恢复同 thread；人工暂停后强杀并重启确认；
  并发重复命令、取消心跳、Redis 投递失败、业务回执已提交而图无 checkpoint 时重放。
- `.venv311/bin/python -m pytest tests/unit/test_artifacts.py tests/workflow/test_store.py -q`：
  **12 passed in 1.11s**（持久模块测试使用同真实 PG 上的独立临时 schema）。
  含 claim token、发布回执与执行回执区分、租约围栏、摘要确认/改版、uncertain 与基础设施失败上限。

这些测试的 SkeletonNodes 仅证明执行可靠性，不含招标语义，不能计为阶段 1–6 领域完成。
依赖锁定：LangGraph 1.2.15、Postgres checkpointer 3.1.3、Dramatiq 1.18.0、
psycopg 3.3.6、OpenAI 2.54.0、httpx 0.28.1。

## A. 真实基础设施骨架（先于领域实现）

- Python 3.11 + 锁定依赖可导入；真实 PostgreSQL 初始化官方 checkpoint 与独立业务表。
- 真实 Redis + Dramatiq AsyncIO 启动/恢复；同 run 并发重复消息只有一个有效执行者。
- worker 强杀后，租约/领取过期、dispatcher 补投，恢复同一个持久 thread。
- 人工 interrupt 已保存后 actor 结束释放线程；重启进程后 confirm 新命令恢复。
- Redis enqueue 失败不丢命令；published 无执行回执可补投；旧 claim token 不可确认发布。
- 旧 epoch/租约不可提交；取消长任务时无需排在队列尾；checkpoint 写入也禁止旧执行者。
- 业务提交成功后注入 checkpoint 失败，重跑复用同产物回执，不重复业务提交。

## B. 确定性模型 + 真实解析器

- 混合 PDF/Word/Excel/图片/附件；重复/缺失/不可读与 unsupported/partial 明确记录。
- Excel 嵌入图和跨工作表计数；章节/段落/表格/合并关系及坐标不丢。
- 扫描件/实际图 OCR；文本 PDF 不逐页 OCR。冻结不可变；重复执行复用。
- 小章节合并/大章节拆包，跨包导语、条件和表头带来源；supporting 不重复计正文覆盖。
- 全空行压缩发送视图；原始网格和部分空字段保留。
- 抽取证据/原文/条件/例外/数字单位/评分否决风险/交叉引用；每范围明确处置。
- 独立全来源查漏、数字反查、非 100 分规则、补遗明确作用域与未决冲突。
- 部分包失败保留独立成功包，失败依赖不放行、全局不假成功。
- 重复/过期确认、运行取消、来源改版、旧确认失效。
- 响应计划逐要求具体落点和材料需求，空标题/空槽位拒绝。
- 各阶段耗时、调用/包数、实际 usage、uncertain 请求、问题与产物可读取。

## C. 分发与包装

- 原有 docreader 测试按改动范围运行，新 tests/unit、tests/workflow、tests/acceptance。
- 构建 wheel 并验证新包/提示文件完整；镜像新模块 import smoke，默认仍是 gRPC。
- Python 专用启动说明与可选部署配置完整；现有 Rust 默认运行链路 diff 为空。

## D. 真实 LLM 语义验收（单独记录）

先检查环境输入和模型是否存在，只报告 endpoint/model/文件清单，不输出 key。
付费全流程调用须先报告估计请求范围并由父任务确认。真实输出须人工语义核对遗漏、
数字、补遗和计划；mock/CI 通过不等于语义验收。不具备输入/访问则明确剩余 blocker。

## E. 最终生产集成（本 PR 不完成）

Rust run/outbox/API/鉴权/事务围栏、流量切换、旧实现移除、部署及历史数据迁移均未实施。
不把独立 Python 验收勾选为最终生产集成。
