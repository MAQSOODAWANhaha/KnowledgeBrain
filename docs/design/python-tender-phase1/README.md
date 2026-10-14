# Python 招标工作流第一期（独立路径）

状态：设计先行，Draft PR；初版独立实现与确定性验收完成，追加请求成本/流式修复进行中，真实 LLM 语义验收待完成。
实施与验收事实见 [实施清单](implementation.md)、[证据](acceptance.md)，启动见 [runbook](runbook.md)。
基线：`e8b681a0997760ec3f567223fe7eb82b622f5ed7`（2026-10-14 拉取 origin/main 核对）。

## 工程原则与边界

**文档先行。** 先提交方案、接口合同、职责边界、实施清单和验收标准，再实现。
发现设计与实现偏差时，先判断是方案缺陷还是实现问题，更新对应文档，再继续。
本目录描述新增 Python 路径；旧 Rust 文档及历史验收证据保持原含义。

本阶段不得修改 Rust 源码、Cargo 依赖或现有 Rust 执行链路，不切流、不删除旧实现，
不自动合并或部署。新增入口仅显式启动。未来接入合同见 [contracts.md](contracts.md)，
不代表 Rust API 已存在。独立验收完成与最终生产集成完成是两个不同状态。

终点为 `response-plan-ready`：登记 → 解析/覆盖/冻结 → 并行要求抽取 → 独立验证 →
人工确认 → 响应计划。知识材料检索、正文生成、成稿审查、最终导出均不在本期。
这不是“投标文件最终验收”。无通用 supervisor/subagent 框架，也不引入通用运行时。

## 现有工程与新增职责

保留 `services/docreader/` 单一 Python 工程。复用 `parser/`、`models/`、`proto/`
及 `main.py` 原生 gRPC 接口，不改变默认命令。生产镜像 Python 3.11，新增依赖通过
`pyproject.toml` 与 `uv.lock` 锁定；显式 setuptools 包表必须覆盖新增包。

| 目录 | 职责 |
| --- | --- |
| `llm/` | CHAT/VLM 配置、请求、上下文校验、累计用量、失败/不确定状态；无业务编排 |
| `tender/evidence/` | 文档版本登记、原生解析、实际图片 OCR、冻结来源与包索引 |
| `tender/requirements/` | 并行抽取、证据及逐来源范围处置 |
| `tender/checks/` | 独立全来源查漏、证据与数字反校验、冲突/补遗问题 |
| `tender/planning/` | 每条适用要求到具体响应位置及材料需求 |
| `tender/prompts/` | 版本化领域提示，输入文档作为数据 |
| `workflow/` | 唯一业务阶段编排 LangGraph、状态与 PostgreSQL checkpointer |
| `jobs/` | Dramatiq Redis broker + AsyncIO、小消息、领取与回执、失联补投 |
| `integrations/` | 独立 CLI、持久业务表、文件引用；未来受控 API 边界 |

同镜像运行 gRPC、dispatcher、worker、CLI 等不同进程角色，不建多个微服务项目。
Dramatiq AsyncIO 每进程一个事件循环，消息并发仍受 worker 线程数约束；原生解析通过
线程/进程隔离执行，不占用事件循环。大文件不进队列、不进 checkpoint。

## 图和持久状态

LangGraph 的节点按六个阶段连接，人工节点使用持久 interrupt。checkpoint 只保存
run_id/source_version/domain_revision、阶段、六个固定产物引用与摘要、计数和状态；
字段以[唯一字典](field-dictionary.md)和`tender/schema.py`为准。
每阶段产物通过不可变内容寻址文件写入，再以数据库带围栏事务登记回执；重放节点先读
已有回执，因此“业务提交成功但 checkpoint 未保存”不会重复生成业务版本。

业务 PostgreSQL 表保存 run、版本、命令、执行租约/围栏、产物回执、确认、模型请求审计。
LangGraph checkpoint 表由官方 checkpointer 初始化。Redis 只是投递；业务结果不依赖 TTL。
初期独立提交将 run + start 命令在同一 PostgreSQL 事务落库；dispatcher 领取带 token，
投递后记录 published，只有 worker 执行回执才表示执行。过期 published/执行租约可补投。

每个 run 同时只有一个有效执行租约；epoch 单调增加，所有业务提交检查 epoch、租约和
取消意图。heartbeat 在独立协程观察取消；节点边界再检查。旧执行者不能提交或覆盖
checkpoint；实现需同时保护图执行与业务产物，不能只给产物加锁。
人工 interrupt 持久化后结束当前 actor，释放 worker；confirm 事务产生独立 resume 命令。
运行取消直接写取消意图，不排队等待长任务结束。来源变更产生新版本和新 thread_id，
旧确认失效；旧执行结果不会流入新版本。

## 领域合同

1. 文档集含所有主件、附件、澄清补遗和明确版本/修改目标。重复、缺失、不可读是可追溯问题。
2. 冻结保留章节、段落、表格/单元格/合并关系、实际图片、附件位置及 parser 的 partial/
   unsupported 信息。文本 PDF 不逐页渲染给模型。扫描页和实际图片才调用 OCR/VLM。
   Python literal OCR 是本次新增领域能力，不假定 Rust OCR 已移植。
3. 包只引用冻结单位。小章节合并、大章节切分；导语、条件、表头作为带来源的 supporting
   context，不重复计入正文覆盖。发送视图可压缩全空表格行，保留原始网格与部分空字段语义。
4. 每个正文范围必须有要求、带依据的非要求处置或未决。要求含原文证据、对象、条件、例外、
   数字/单位、评分、否决风险、交叉引用；不得以要求数量替代覆盖核对。
5. 独立验证既核对已有要求，又重读全部来源找遗漏；数字反校验是规则检查。评分只有来源
   明示合计时检查合计，不假设均为 100。补遗只影响明确目标，不以后发时间全局覆盖。
   局部包语义失败隔离保留成功包；依赖失败范围的结果不得放行，全局不得假成功。
6. 人工确认绑定来源/领域修订、冻结+分包计划+要求+验证清单摘要；变更后旧确认失效。未决阻断项不能靠确认强制
   通过。响应计划逐条落实正文、具体表格字段、附件或人工动作并列材料需求；空标题/空槽位失败。

## 模型、失败与观测

配置仅从环境读取，延续 `KNOWLEDGEBRAIN_CHAT_*`（含 `LLM_*` 既有别名）与 VLM 语义。
独立 VLM endpoint 不转发 Chat key；继承 Chat 图像调用需明确支持图像。key 不写文件或日志。
单次上下文容量与累计预算分开；不自加 `max_tokens` 输出硬上限。纠错仍受同一服务容量限制。
合理默认并发/超时/退避；SDK 内建重试关闭，队列仅重试基础设施失败，语义问题在图内返回
领域问题/受限修复；不能 SDK、图、队列叠加无限重试。

每模型请求在发送前登记，成功记录真实 usage，已发送但响应丢失标记 uncertain，重试可能
重复计费不可伪称 exactly-once。阶段耗时、请求/包数、usage、未决项与实际产物均持久记录。
当前追加的[请求必要性与流式合同](request-review.md)要求所有新Python模型交互streaming、
连续180秒无数据才超时，且uncertain不自动重付费；实现与验证进度在追加评审中单列。
确定性 mock 模型只验证合同和恢复，真实模型语义验收另列。付费全流程之前报告输入、endpoint
与估计范围，等待父任务确认；离线实现继续。

## 验收顺序

首先用真实 Redis/PostgreSQL 建立最小骨架并验证强杀恢复、重复投递、人工暂停后重启。
随后接领域实现和真实 parser，再做故障注入与混合文档验收。详细标准及证据见
[acceptance.md](acceptance.md)。基线已知 Rust bidding CI 为 500 passed / 2 failed /
24 ignored（前轮记录），不是本 PR 引入；本阶段不修改该 Rust 测试。

用户重点补充：[分包设计评估与Rust→Python规则对照](packing-review.md)。固定字符分包的旧21包仅作诊断基线，不计新合同验收。
