# 跨领域待验收事项

招投标生成的唯一当前合同入口是[招投标文档](../docs/bidding/README.md)，整改仅在[实施计划](../docs/design/check-execution-plan.md)维护。本页不再保留旧 Agent 轮次、私有材料摘要、临时路径或旧工具/兼容方案。

以下保留未解决的跨领域验收要求；不是本轮授权实施清单，也不是当前候选已通过声明。知识库与插件填充延期；编辑/导出规范见[ONLYOFFICE](bidding/onlyoffice-integration.md)。平台/部署要求仍由[平台计划](platform/README.md)及[部署说明](../deploy/README.md)定义。历史结果在 Git 和原私有审计中保留，不改写成新版本通过。

## 平台与发布

### P1 — 对象清理交接与真实消费回归闭环

- **目标 / 不做：** 按[平台 §6](platform/runtime-foundation.md#6-retention-consumer)纠正交接与异步完成的错误验收，补齐真实消费结果；handler 在原绝对 deadline 内只确认交接，不同步删 blob、不等待物理回收。不新增 retry budget、扫描器、通用 wrapper、清理任务表或第二状态机。
- **产出：** 最小修复、真实 consumer 回归、原 Request 用例逐项覆盖映射、红/绿退出码与数据库/blob/receipt 证据、P1 必跑入口及隔离资源清理收据。
- **可证实验收：**
  1. 先用最窄可用 fixture 留下旧失败的真实退出码，区分缺 Redis 与错误同步删除语义；原 fixture 改用真实写入的测试 blob，不只登记字节。
  2. enqueue 未确认/失败/未知结果时保留 staging 与可恢复 identity，显式失败；仅官方 enqueue 的确认才 disarm tracker，`Skip` 不冒充 delivery receipt。确认交接不宣称 staging 或 blob 已删除。
  3. 真实 typed consumer 执行后证明 staging 释放；无引用应回收对象在有界期限内观测到 blob 消失、`deleted`、tombstone/receipt；有引用拒绝物理删除，重复消费幂等。沿 Oxana 原生 retry/resurrection 验证失败保留与恢复，不复制测试 consumer、不用直接 SQL 函数调用替代队列消费。
  4. 原 Request 各错误码、失败终态原子性、owner/revision/digest 围栏全部保留。必须调整归属时提交逐项覆盖映射与真实结果；不能只反转/删除 cleanup 断言、固定 sleep、skip 或削弱最终回收条件。
  5. `runtime.rs` / `bidding.rs` adapter 仍共享既定 absolute cleanup deadline，terminal write/tracker/取消与 join 不获新预算；失败/取消不遗留超期等待或 late write。
  6. CI 实际执行相关 consumer；缺 DSN/Redis/对象资源/role、ignored/skipped/filtered/零用例或 cleanup 失败必须失败。保留 `schema-contract`、Content/Outline/Request 三 suite guard、`--locked`，不趁机实现整套发布流水线。
  7. 仅使用已存在本地镜像创建本轮唯一 label/ID 的专用资源，不 pull/升级依赖，不读真实 `.env`/secret；创建前记录既有容器/卷/网络/镜像身份及端口（25433 须先空闲），绑定 127.0.0.1、`knowledgebrain_test_*`；退出仅清本轮登记且 label 匹配资源，零残留且既有资源身份不变。缺安全隔离条件即报告阻断。

### P2 — 完整数据库/队列正确性

- **目标 / 不做：** 在 P1 闭环后复跑完整数据库相关 suite 与平台/队列故障矩阵，区分完整 suite 和聚焦用例证据；不重做已有诊断修复，不以跳过用例获得绿色。hosted CI 与 named job `queue-faults` **不是本任务重点**，改挂 R3，不在 P2 剩余清单里追接线。
- **产出：** 逐 suite 真实执行/退出码、数据库和队列故障覆盖表、必要的最小修复。本地分轮证据即可关闭本切片；不要求 hosted workflow 或新增 `queue-faults` job。
- **可证实验收：** fresh DDL、单账号权限边界及真实登录；五 writer 字节/NULL/错误码、AgentRun 锁后时钟、调用预算和原子终态；ObjectRegistry 引用竞态、queue 原生 retry/delay/reconnect/resurrection/dead revive、六 handler deadline、SIGINT/SIGTERM/fatal 与 helper cancel/kill/reap。完整 suite 不得 ignored/skipped/filtered/零执行；缺依赖与清理失败非绿。数据库行为验收与完整应用启动分别记证据。

### O0 — 许可、版本、字体与真实样稿 PoC

- **目标 / 不做：** 按[接入计划 O0](bidding/onlyoffice-integration.md)取得可保存的真实 Word 实证；不以 iframe/截图代替，不默认免费无限制，不作采购决定。
- **产出：** 服务版本/字体/样稿摘要、人工编辑后保存重开 DOCX 与同一文件 PDF、格式差异和许可核实记录。
- **可证实验收：** 合并单元格、非等宽列、重复表头、页眉页脚、中文字体、图片附件页、指定模板/固定表实际保存重开及 PDF 对照；关键不可保真项明确而不直接切换。普通编辑/回调/初稿/转换不以额外 Automation API 采购为前置，实际本体许可与版本能力仍须核实。
历史样本专属结果仅保留在私有档案；公开回归采用合成输入，不代表真实语义验收。

### O1-S — 新轮 DOCX 初稿、存储版本与编辑会话

- **目标 / 不做：** 用当前完整冻结输入单向生成包含全部章节和章内待填写表格的整本 DOCX 模板，关联项目/Workspace/新轮/文件版本与编辑会话；复用对象、幂等、CAS，不建设通用版本平台或 DOCX↔块双向同步。具体内容自动填充后置，不作为本期模板生成的验收前置。
- **产出：** 可核验初稿对象/摘要/版本与会话绑定、必要持久化与配置接口实现、对应回归；具体 API/schema 在实施读码后落位，不在台账伪造。
- **可证实验收：** 固定目录优先、表格列/合并及模板可编辑，不强制通用骨架或人员专章；缺材料可生成空章/待确认稿而不造事实。`document.key` 与每次保存版本分离，关闭/重开/forcesave/新轮符合官方生命周期；新轮不复用旧会话或旧响应/页码。并发 CAS、重复请求、对象 staging→owner 与历史身份可核验；已编辑正式稿不从旧树重建。

### O1-C — 受控读取、签名回调与持久化安全

- **目标 / 不做：** 实现保存回调的权限与持久化闭环，不把任意通知/下载成功当保存，不接受任意 URL 抓取。
- **产出：** 受控读取与签名配置、回调处理及真实服务回归，保存关联/幂等/乱序策略的明确实现与证据。
- **可证实验收：** 用户/项目/新轮/文件/用途越权及伪造签名拒绝；secret 不入前端/日志。状态 2/6 按保存语义处理，3/7 明确失败，其他通知不伪保存；重复不重复发布，乱序、旧会话、旧轮不回滚当前稿，不能按到达时间猜最新。主机/协议/重定向/范围、超时/大小预算有效；损坏 DOCX、摘要不符或存储失败不推进 ready、不返回伪成功。

### O1-W — 编制前端真实编辑、保存与重开

- **目标 / 不做：** 接入真实编辑配置与保存反馈，让用户仍按三步操作；不只是嵌入 iframe，也不把 DOCX 回转成 Tiptap 主存。
- **产出：** 可打开/编辑/保存/关闭重开的编制入口、版本/保存中/失败反馈及真实浏览器回归。
- **可证实验收：** 修改段落/表格/图片后重开恢复真实 bytes 中的内容；保存中与我方持久化成功明确区分，刷新/失败/迟到通知不报假 ready。密钥仅后端签发，不能跨项目换稿；业务缺料允许编辑。旧入口在切换前受控保留，不形成同一正式稿双主写入。

### O2-S — 保存关联与冻结出件版本

- **目标 / 不做：** 当前稿导出等待正确保存并冻结具体版本；不把 forcesave 请求接受当作保存完成，不隐式回退历史稿。
- **产出：** 保存→正确回调→冻结出件 identity 的实现及并发/失败回归。
- **可证实验收：** 有未保存编辑时等待或明确失败；正确保存关联后才冻结。保存失败/冲突不输出“最新稿”；持续编辑不改变已冻结输出，界面说明版本边界。历史下载仅可显式选择，同 key 重放保留同一冻结 identity，不能读 live 输入改写历史。

### O2-E — 同稿 DOCX/PDF 与独立报告

- **目标 / 不做：** DOCX 直接取冻结文件、PDF 从同一文件经 ONLYOFFICE 转换，独立报告绑定输出；不从 ContentBlock 独立重排正式 PDF、不自动拆分文件。
- **产出：** 同稿两格式及独立历史报告获取入口、技术失败和版本一致性证据；基础整本能力在本项核验，O3-M 继续补自动入稿/页码细节。
- **可证实验收：** 实际 DOCX/PDF 内容与同一保存摘要/版本关联，转换失败不回退旧自研正式链；历史输出/报告不随当前编辑改变。报价、价格及附件属于同一完整稿，缺料在报告明示仍可出件；报告不混入正文，保留单独提交要求及用户拆分/拆后自行复核提示，不宣称整本已满足分册提交。

### O3-A — 官方插件候选入稿与人工保护

- **目标 / 不做：** 优先验证编辑器内官方插件展示候选、人工确认后定点插入并保存；不后台覆盖全文，不绕过跨域 DOM/私有协议，不默认插件等于 Connector。
- **产出：** 公开插件候选流程、目标/版本/人工冲突保护与入稿后持久化证明，能力不满足则明确缺口供父裁决。
- **可证实验收：** `executeMethod` / `PasteHtml` 等公开能力真实验证，不凭简单插入宣称复杂表保真；过期版本、缺失/重复锚点及未保存人工修改不误插/覆盖。部分接受/拒绝、重复决定不重复插入，保存回调后才称入稿。事实需投标侧证据、拟议响应可待确认，模型不签发权限或路径；只改正文不重跑招标解析。

### O3-M — 报价附件实际入稿与页码安全出口

- **目标 / 不做：** 将报价/证明真实内容写入整本，并对实际排版验证页码、有限轮回填与最终检查；不以快照/对象引用/Conversion 成功冒充已入稿或页码正确。
- **产出：** 确定性报价表、实际附件页、证明定位/回填、最终文件检查与安全降级证据。
- **可证实验收：** Decimal/定点金额逐行舍入再求和、明细/税额/合计与实际 DOCX/PDF 核对；更新报价不后台覆盖人工稿。证据内容实际嵌入，招标附件不冒充我方证明，损坏的已选附件为技术失败。长表换行、多页证明、锚点丢失/重复、不收敛场景经过真实页验证；回填后保存/转换/有限轮重检，最终 DOCX/PDF/页码/报告同版本。不稳定数字须清除或降级且验证输出无伪确定引用，不能安全出件则失败。最终检查区分未满足、待确认与技术失败，业务风险不变 Gate。

### O4 — 真实端到端回归、切换与撤旧

- **目标 / 不做：** 按当前授权直接删除废弃的大纲设计及其专用实现、界面和契约，正式入口统一使用 DOCX；保留新链实际使用的共享原语，端到端验收单独跟踪。
- **产出：** 可复现的新链端到端证据、旧路径替代映射、最小切换/删除 diff、更新后的操作手册。
- **可证实验收：** 同一项目走上传→要求/规范→DOCX 初稿→人工编辑→AI 候选确认→保存→整本 DOCX/PDF+独立报告；文件集变更后重新冻结并全新验收，旧稿仍可追溯。缺料/高风险可编制出件，保存/转换/损坏/不可信页码不假成功。源码与运行调用证明旧 Tiptap 主写入、自研正式 PDF 及隐式块出件路径已撤；有真实初稿/候选消费者的中间原语保留，未替换的解析/权限/身份回归通过。

### R1 — release descriptor、RepoDigest 与启动验证

- **目标 / 不做：** 落实已确认首发 identity 与启动 gate，不另建发布框架，不以 development synthetic digest 或 `latest` 作生产身份。
- **产出：** 复用现有 checked schema 的唯一 release 工具、启动顺序/实际镜像 inspect 验证、拒绝矩阵及发布证据模板。
- **可证实验收：** exact descriptor/JCS hash 与实际 full RepoDigest inspect 一致；缺文件/非法字段/任一 mismatch fail-closed、无 tag fallback；migrator 先成功再启动 API/Worker/Retention，重复启动只读 readiness，DocReader 用 post-start inspect gate。空对象卷真实上传/读取/引用保护/最终回收，记录完整应用外部依赖启动而非仅 shared verifier。所有验收仅专用隔离环境。

### R3 — 首发 named required jobs 闭环

- **目标 / 不做：** 完成既定首发必跑接线并真实执行，不把 development checks 或历史 guard 探针称 release accepted，不在阶段 A/P1 顺手实施。
- **产出：** `schema-contract`、`queue-faults`、`outline-scripted-e2e`、`web-export-e2e`、`release-descriptor` 五个 named required jobs 的完整接线与同候选运行证据。
- **可证实验收：** 五项均真实执行其业务/故障验收，新 DOCX 链不得用旧块 E2E 冒充；已有 rust/三个 suite guard、`--locked` 与 schema job 不丢失。依赖缺失、零用例、ignored/skipped 或 cleanup 失败使 job 失败；保留各命令退出码与资源零残留。无 hosted 结果时只能报告本地验证，正式发布仍不得放行。

### V1 — 条件式 pgvector 分域基准

- **目标 / 不做：** 只有明确性能需求才测量，按实际计划决定是否调优；保留 PostgreSQL+pgvector，不新增向量库/同步/双写，不透明替换 exact 或冻结 policy。
- **产出：** V2 exact 证据检索与普通搜索分别记录 `EXPLAIN ANALYZE`、latency/P95、QPS、recall、CPU/I/O；证据支持的最小 SQL/HNSW 改动或不改结论。
- **可证实验收：** 使用代表性 eligible 集合复现瓶颈并核对优化前后结果与资源，不以任意向量数立门槛；exact/确定性排序、冻结 policy/revision、OCR 映射、eligible scope 与事务发布不变。ANN 候选/召回语义变化须另经证据和语义确认；只有调优后仍不满足 SLO 或独立故障域/扩缩容需求有实证才另评外库，不在本项擅换。
