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
| `confirm` | run_id/source_version/domain_revision/checklist_digest/idempotency_key | 只接受当前等待确认且清单一致的确认，事务创建 resume 命令；重复同确认幂等，过期拒绝 |
| `cancel` | run_id | 直接写意图并作废未执行命令；心跳/节点边界观察 |
| `revise` | run_id、新 manifest | 增加来源版本；旧确认无效；新 checkpoint thread；显式新 start |
| `correct` | run_id/source_version/expected_revision/base_requirements_digest、完整纠错要求清单、reason、request key | 同来源产生新领域修订；重新独立验证与确认；不伪造来源改版 |
| `dispatch` | 可选单轮 | token 领取待投命令、投 Redis、确认发布、对失联命令补投 |

manifest 含 documents（document_id、role=main/attachment/clarification/amendment、version、
path、可选 replaces/修改范围）及 expected_document_ids。登记问题持久返回，不能静默跳过。
文件路径仅本地独立验收使用；运行时只认冻结文件引用与内容摘要。

## 命令、租约和回执

队列 payload 仅 `run_id, command_id, version`，不携带正文/密钥/模型响应。
命令状态 pending → claimed(token, deadline) → published → running → completed；
claim 过期可重领；publish 失败恢复 pending。仅未执行超时，或执行租约失效且命令仍可执行时补投；有效 run 租约期间不补投。completed/paused(blocked/waiting)/failed/cancelled 均持久记录处置，取消或终态失败不重新执行。
确认领取时同时核对 token，旧 dispatcher 不能确认新领取。
dispatcher 自身对 PostgreSQL 暂时断连保持进程存活、脱敏记录并退避重试（2 秒至 30 秒）；
基础设施恢复后继续领取。命令执行失败上限与 dispatcher 等待基础设施恢复不是同一计数。

worker 原子取得 run 执行租约与 epoch；同命令重复不重复生效，繁忙消息可结束由 dispatcher
补投。执行回执必须在 graph 完成或 interrupt 已持久后才写；进程强杀无回执，重放恢复。
epoch 围栏保护所有产物提交。checkpointer 写入需受同一个 run 排他执行保护，失去租约时
停止图并禁止旧 checkpoint 写入。取消/版本替换不依赖队列顺序。

### 来源版本与领域修订（2026-10-14 评审修正，先于代码）

`version` 字段仅表示 `source_version`：原文件/附件集合改变才增加。新增 `domain_revision`
表示同一冻结来源上的要求补漏/纠正，每来源从 1 开始。`correct` 必须带预期当前修订、
基线要求内容摘要和原因；提交完整分包要求清单（每包身份不变，可增删或更正要求/处置），
事务建立新修订与 start 命令，不更改 manifest 或冻结文件。重复同 request key 返回同一修订；
并发旧基线拒绝；cancelled run 禁止纠错而隐式复活。纠错也允许从 blocked、等待确认或已完成计划发起；不能在来源尚未冻结/抽取时使用。

产物身份为 `(run_id, source_version, domain_revision, stage, scope)`；每项另存
`input_digest`（直接上游文件摘要与操作相关配置摘要）。register/freeze/ocr 等来源产物使用
领域修订 0，在同来源内共享；抽取/验证/确认/计划使用当前修订。相同身份与输入重放返回原回执；
同身份换输入或不同内容拒绝；合法纠错建立新修订。文件先原子落盘后登记，checkpoint 失败
可复用已提交回执。不能把旧验证/计划当作新修订有效结果；status 的当前结果只列当前修订
及共享来源，历史回执保留审计。新 checkpoint thread 包含来源版本与领域修订。

确认绑定 `(source_version, domain_revision, checklist_digest)`；摘要包含冻结来源、要求、
独立验证内容。纠错事务清空当前确认摘要、提升 epoch、作废旧未执行命令，保留历史确认但
不再授权新修订；新修订必须重新独立验证并人工确认后生成计划。旧 worker 与旧确认均拒绝。
原来源改版仍用 `revise`，并重置领域修订为 1；完全相同 manifest 不构成来源改版。

来源冻结前另以共享来源回执固定实际解析/分包配置；部分文档恢复不得混用新配置。
已固定配置的恢复在读取 checkpoint/重放终态回执之前核对，不依赖节点恰好重跑。
首次模型调用前按当前修订持久固定非秘密执行配置（规范 endpoint、model、单请求容量、
实际提示内容 SHA256）。恢复必须匹配；不存密钥，不因 key 轮换产生新修订。环境或提示
改变时明确失败，不混用新旧配置续跑。纠错可以固定新配置但必须重新验证。普通 CHAT
单请求容量本次验收取已确认的 `C=131072`；与累计实际 usage 分开，纠错不得超服务容量。

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

### 真实原件预检后的有限修正（先于代码）

同一来源内的 literal OCR 只读取图片像素，不把出现位置当作模型语义输入；按图片内容 SHA256
和固定模型/提示配置复用回执。每个图片出现的文档/页/锚点仍独立保留，不能去重删除来源。
同 worker 内相同图片请求加小范围异步互斥，跨重启使用围栏回执，不另建通用缓存服务。
这将真实 PDF 的 98 个图片位置保留为 98 个来源，但仅需 2 份不同像素内容的初次 OCR 请求。

最终分包规则以 [结构优先分包评估](packing-review.md) 为准；废止固定6000/12000字符驱动请求边界。
