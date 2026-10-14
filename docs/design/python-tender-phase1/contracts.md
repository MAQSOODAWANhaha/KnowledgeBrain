# 独立入口与持久合同

本合同先于实现提交。实现细节有必要变化时先更新本文件。

## 独立入口

CLI `python -m docreader.integrations.cli`，显式 PostgreSQL DSN、Redis URL、artifact root。
不新增 Web 框架，也不伪造已有 Rust HTTP API。

| 命令 | 输入 | 结果及约束 |
| --- | --- | --- |
| `init` | 运行环境 | 仅初始化 Python 专用业务表与官方 checkpoint 表 |
| `submit` | manifest JSON，幂等 request key | 复制原文件到不可变文件存储；事务创建 run/version/start 命令；返回 run_id |
| `status` | run_id | 当前版本/状态、摘要、产物引用、问题与用量；不输出秘密 |
| `confirm` | run_id/version/checklist_digest/idempotency_key | 只接受当前等待确认且清单一致的确认，事务创建 resume 命令；重复同确认幂等，过期拒绝 |
| `cancel` | run_id | 直接写意图并作废未执行命令；心跳/节点边界观察 |
| `revise` | run_id、新 manifest | 增加来源版本；旧确认无效；新 checkpoint thread；显式新 start |
| `dispatch` | 可选单轮 | token 领取待投命令、投 Redis、确认发布、对失联命令补投 |

manifest 含 documents（document_id、role=main/attachment/clarification/amendment、version、
path、可选 replaces/修改范围）及 expected_document_ids。登记问题持久返回，不能静默跳过。
文件路径仅本地独立验收使用；运行时只认冻结文件引用与内容摘要。

## 命令、租约和回执

队列 payload 仅 `run_id, command_id, version`，不携带正文/密钥/模型响应。
命令状态 pending → claimed(token, deadline) → published → running → completed；
claim 过期可重领；publish 失败恢复 pending；publish 后没有 execution receipt 需补投。
确认领取时同时核对 token，旧 dispatcher 不能确认新领取。
dispatcher 自身对 PostgreSQL 暂时断连保持进程存活、脱敏记录并退避重试（2 秒至 30 秒）；
基础设施恢复后继续领取。命令执行失败上限与 dispatcher 等待基础设施恢复不是同一计数。

worker 原子取得 run 执行租约与 epoch；同命令重复不重复生效，繁忙消息可结束由 dispatcher
补投。执行回执必须在 graph 完成或 interrupt 已持久后才写；进程强杀无回执，重放恢复。
epoch 围栏保护所有产物提交。checkpointer 写入需受同一个 run 排他执行保护，失去租约时
停止图并禁止旧 checkpoint 写入。取消/版本替换不依赖队列顺序。

产物唯一键 `(run_id, version, stage)` 或 `(run_id, version, stage, scope)`；内容寻址文件
先原子落盘后登记（允许无引用孤儿文件，不允许回执引用不存在文件）。提交成功 checkpoint
失败时节点恢复原回执。确认摘要覆盖冻结来源版本、要求版本与验证结果；计划附带该摘要。

实现选择：官方 AsyncPostgresSaver 的 cursor 边界使用同一 PostgreSQL 连接的短事务，
先 SELECT run FOR UPDATE 校验 fence，再执行 checkpoint SQL 并提交。持锁覆盖校验与写入，
没有先检查后写入的时间窗口；heartbeat/取消仅等这次短事务，不等模型请求。setup 使用
无执行租约的官方 saver，仅在显式 init 运行。该适配器随锁定版本作集成测试。

## 未来生产接入（本次未实现）

Rust 最终拥有业务 API、鉴权、业务版本、人工确认、提交事务；Python 拥有解析、OCR/VLM、
模型调用、队列和 LangGraph 执行。未来 Rust 事务写 run + outbox，Python dispatcher 通过
受控 API 领取小命令（claim token），投队列，再回报发布与执行回执。产物提交带幂等键、
源版本、epoch，由 Rust 事务核查。取消/心跳和确认恢复也走受控 API。

独立 Python 业务表/CLI 是可验证适配器，不能被称作已落地生产 API。生产接入需独立 PR、
授权、集成验收和切换方案；无历史 checkpoint 兼容或历史数据迁移框架。
