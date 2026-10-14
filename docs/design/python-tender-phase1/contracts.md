# 独立入口与持久合同

本合同先于实现提交。实现细节有必要变化时先更新本文件。

## 独立入口

CLI `python -m docreader.integrations.cli`，显式 PostgreSQL DSN、Redis URL、artifact root。
不新增 Web 框架，也不伪造已有 Rust HTTP API。

| 命令 | 输入 | 结果及约束 |
| --- | --- | --- |
| `init` | 运行环境 | 仅初始化 Python 专用业务表与官方 checkpoint 表 |
| `submit` | manifest JSON，幂等 request key | 复制原文件到不可变文件存储；事务创建 run/source_version/start 命令；返回 run_id |
| `status` | run_id | 当前版本/状态、摘要、产物引用、问题与用量；不输出秘密 |
| `confirm` | run_id/source_version/domain_revision/checklist_digest/idempotency_key | 只接受当前等待确认且清单一致的确认，事务创建 resume 命令；重复同确认幂等，过期拒绝 |
| `cancel` | run_id | 直接写意图并作废未执行命令；心跳/节点边界观察 |
| `revise` | run_id、新 manifest | 增加来源版本；旧确认无效；新 checkpoint thread；显式新 start |
| `correct` | run_id/source_version/expected_revision/base_requirements_digest、完整纠错要求清单、reason、request key | 同来源产生新领域修订；重新独立验证与确认；不伪造来源改版 |
| `review-scope` | run_id/source_version/expected_revision/base_plan_digest、带证据的scope_decisions、reason、request key | 同来源新领域修订中绑定明确目标或审定仅局部适用；重分包、重抽取、重验与重新确认 |
| `dispatch` | 可选单轮 | token 领取待投命令、投 Redis、确认发布、对失联命令补投 |

manifest 含 documents（document_id、role=main/attachment/clarification/amendment、document_version、
path、可选 replaces/修改范围）及 expected_document_ids。登记问题持久返回，不能静默跳过。
文件路径仅本地独立验收使用；运行时只认冻结文件引用与内容摘要。

## 命令、租约和回执

队列 payload 仅 `run_id, command_id, source_version`，不携带正文/密钥/模型响应。
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

内部字段统一为 `source_version`：原文件/附件集合改变才增加。单文件标签使用`document_version`；
不再保留通用`version`别名，完整定义见[唯一字段字典](field-dictionary.md)。`domain_revision`
表示同一冻结来源上的要求补漏/纠正，每来源从 1 开始。`correct` 必须带预期当前修订、
基线要求内容摘要和原因；提交完整分包要求清单（每包身份不变，可增删或更正要求/处置），
事务建立新修订与 start 命令，不更改 manifest 或冻结文件。重复同 request key 返回同一修订；
并发旧基线拒绝；cancelled run 禁止纠错而隐式复活。纠错也允许从 blocked、等待确认或已完成计划发起；不能在来源尚未冻结/抽取时使用。

产物身份为 `(run_id, source_version, domain_revision, stage, scope)`；每项另存
`input_digest`（直接上游文件摘要与操作相关配置摘要）。register/freeze/ocr 等来源产物使用
领域修订 0，在同来源内共享；分包计划/抽取/验证/确认/响应计划使用当前修订。分包计划由共享
规范来源加当前scope_decisions生成，绑定task/input_digest/budget_digest/plan_digest。
相同身份与输入重放返回原回执；
同身份换输入或不同内容拒绝；合法纠错建立新修订。文件先原子落盘后登记，checkpoint 失败
可复用已提交回执。不能把旧验证/计划当作新修订有效结果；status 的当前结果只列当前修订
及共享来源，历史回执保留审计。新 checkpoint thread 包含来源版本与领域修订。

确认绑定 `(source_version, domain_revision, checklist_digest)`；摘要包含冻结来源、要求、
独立验证内容。纠错事务清空当前确认摘要、提升 epoch、作废旧未执行命令，保留历史确认但
不再授权新修订；新修订必须重新独立验证并人工确认后生成计划。旧 worker 与旧确认均拒绝。
原来源改版仍用 `revise`，并重置领域修订为 1；完全相同 manifest 不构成来源改版。

来源冻结前另以共享来源回执固定实际解析配置；分包预算在领域修订计划中固定；部分文档恢复不得混用新配置。
已固定配置的恢复在读取 checkpoint/重放终态回执之前核对，不依赖节点恰好重跑。
首次模型调用前按当前修订持久固定非秘密执行配置（规范 endpoint、model、单请求容量、
实际提示内容 SHA256）。恢复必须匹配；不存密钥，不因 key 轮换产生新修订。环境或提示
改变时明确失败，不混用新旧配置续跑。纠错可以固定新配置但必须重新验证。普通 CHAT
现有 `context_tokens=131072` 的代码含义是**输入+输出预留+安全余量的总窗口**，
不是用户规定的128K输入预算；此前“已确认C”只表示配置值，不证明provider真实能力。
普通输入预算 `input_budget_tokens=131072`，显式纠错输入预算上限
`correction_input_budget_tokens=262144`，单位均为token。准入同时满足输入预算及配置的
真实服务总窗口；不因纠错自动翻倍context_tokens，不向wire增加max_tokens。
当前配置131072仍保留；要使用完整128K或256K输入，必须先有包含输出/安全预留的
provider总窗口证据并显式配置。能力未确认不做付费探测。常规verify加要求/处置不是纠错；
domain_revision增加也不能自动开启双倍。未知tokenizer的UTF8上界仅是保守token估算方法，
实际wire字节和provider usage独立记录，不把字节叫实测token。

### 后阶段完整输入重分（评论6161148698，先于代码）

- extract计划不代表verify/plan可容纳。verify先以当前不可变要求、逐范围处置和完整来源视图
  构造真实请求，生成task=verify的阶段计划；plan以有效适用性视图、完整要求及约束绑定生成
  task=plan的阶段计划。提示/schema/协议、输出预留和安全余量与最终client准入一致。
- 优先合并相邻完整章节；只有当前完整请求失败才按章节内原生结构拆分。验证不能切坏已有
  evidence/disposition的身份：同一范围的处置、关联要求及跨范围义务是完整依赖单元；引用
  的条件、表头、支持证据和明确跨引用随包。要求/处置不靠截断、摘要或省略来适配预算。
  单个依赖单元仍放不下时记录明确的阶段预算阻断，不记作模型语义失败、不付费发送。
- 新阶段计划与分包回执绑定task、完整上游引用、source_version/domain_revision、预算和
  plan_digest；输入变化不复用旧回执。同输入恢复复用阶段计划及成功子包，保留发送不确定
  围栏。计划仅持来源selector和要求/处置引用，冻结原文不复制；checkpoint仍为六个固定
  阶段引用，后阶段计划由相应阶段产物引用。沿用LangGraph和现有回执，不加第二状态机。
- 汇总分别核对抽取来源覆盖、验证计划覆盖、要求/处置完整审核和响应计划逐适用要求唯一
  落点；验证包数可以与抽取包数不同。原件13项作用域审议仍独立阻断。
- 必须用实际workflow回归证明：extract可容纳、合法输出加入后verify超限、重分后全范围
  审核与确认/计划完成；另测不可分阻断、条件/跨范围依赖、修订和部分成功后的恢复。

### 继续修复的依赖与解码失败边界（2026-10-14，先于代码）

后阶段规划的输入是当前修订**同一文档全部抽取包**，不以旧extract边界截断义务。
验证选择使用共享`RequirementRef(artifact_ref, requirement_id)`与新增
`DispositionRef(artifact_ref, range_id)`；根`requirements_ref`只指完整要求索引，
子包不再把它复用成单个抽取产物，也不复制要求/处置正文。引用闭包以明确的来源范围或
要求身份关联；同文档跨抽取包关联义务完整进入同一不可分验证/计划单元。
条件、表头和支持引用随目标发送；仅共享条件不把所有受条件影响的义务强制绑成一个大包。
不为非相邻依赖吞入无关中间范围。无法解析的文本引用、歧义身份或跨文档引用若没有完整
多文档发送合同，应显式依赖未决阻断，不能漏掉引用后发送或宣称完整。
计划读取当前适用性决议；若适用义务依赖已失效义务而未明确解开关系，同样保持未决。
恢复计划、成功子包与当前修订绑定；旧修订/过期确认不授权新输入。

跨旧包合并后，旧support ID可能与现body区间重合。保留不可变evidence的原ID，以共享
`source_aliases`把旧ID映射到本请求已发送的精确字符区间，不重写要求或再发送相同正文。
验证逐个核查别名及其实际片段，别名无text。响应计划的显式来源范围引用附带必要的
`source_context`（共享SourceView及完整条件）；纯要求身份依赖直接携带完整关联义务。

HTTP压缩/解码失败发生在发送以后，不能因为不是合法模型JSON就证明服务未执行。
`httpx.DecodingError`必须保留已收到的部分审计及未知usage，标为`uncertain`并禁止同digest
自动重新发送。即使部分响应写盘、审计提交或业务产物提交失败，先前`sent`/`uncertain`
围栏仍阻止恢复重付费。明确发送前连接失败与明确服务拒绝保留既有有界重试规则。
用真实PostgreSQL和本地流transport覆盖部分SSE/解压失败、提交故障和重启，证明发送次数为1；
沿用180秒原始数据idle、不增加总超时或第二套重试状态机。

真实原件离线检查可用明确标记的synthetic合法抽取输出测试后阶段完整请求，不能称为模型
真实结果或语义验收。最后通过实际SDK MockTransport捕获wire，公开仅脱敏统计/摘要；
原件/正文/图片/请求私有，13项scope未决不得清零，未知provider能力不进行付费探测。

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
这保留真实 PDF 的98个图片位置并允许相同像素复用。后续视觉审议判定两份图片为装饰，
显式来源绑定审议保留全部位置，本原件实际无需OCR；未知图片不适用此结论。

最终分包规则以 [结构优先分包评估](packing-review.md) 为准；废止固定6000/12000字符驱动请求边界。
