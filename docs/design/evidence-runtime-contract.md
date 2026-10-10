# 证据与审查运行时契约

本文描述本次改造的生产边界。离线协议测试、领域夹具和真实模型语义验收分别记录，不互相替代；真实验收尚未完成。

## 模型输入与宿主证据

模型输入只使用宿主提供的身份：

- Discover：`evidence_key` 选择当前包主证据，`atom_key` 选择所属原子或章节；`support_key` 单独关联已核验的外部适用条件。
- Organize：`source_key` 用于读取、固定原文复制和生成说明的依据。
- Check：`review_evidence_key` 只能引用当前 Check 已完整交付的范围；比较工具的 quote handle 只在其 review unit/version 内有效。
- 四类读取工具的续页只接受宿主 opaque cursor，不接受数字页码、手写字节范围或模型指定的页面字节预算。

`source_wire` 是模型参数校验与 canonical 解析边界。内部 `EvidenceRef` 仍保存冻结输入摘要、源单元/原生表格坐标和 UTF-8 区间，用于解析原文、复制、覆盖与回执校验。`apply_canonical` 处理已解析的宿主参数，不再次套用模型 key schema，也不构成另一个模型入口。

来源 key 绑定输入、工作修订和读取 epoch；包 key 另绑定包修订和 claim。不同用途不可互换。组织写入或重开使旧 Check 授权失效。模型参数不保留 rawref、atom/index 的兼容路径。

## 容量、续页与真实交付

`prepare_request` / `fit_batch` 对完整下一请求计量，包括系统提示、工具、历史、来源、图像与输出预留。自定义 tokenizer 需有可核实来源或实测校准；经验估算不代表长上下文能力证明。模型上下文与操作员调用/费用/时间限制独立。

`read_evidence`、`read_claim_evidence`、`read_requirements`、`read_outline` 由正式异步 runtime 决定容纳量和续页。完整段、原生行优先；单个载体过大时，宿主生成连续 UTF-8 子范围。超大元数据以稳定 record identity、命名字段和字段范围传递，宿主核对完整字段后恢复记录；模型不拼接 JSON 字节。缺页不能授予完整回执。

成功工具结果先成为 pending frame。最终装配器对实际 compact/project 后的 tool message 封存摘要；只有匹配 journal 已持久化完成响应的请求才确认交付。预取、同响应中读后写、被回收的未发送结果、失败批次、旧修订或旧 Check epoch 不授予授权。

阶段交接共用 `handoff_history`：保留当前尚未交付、身份仍有效的工具组，移除较早历史。保留历史本身不授予读取回执。repair 轮换 claim 后，历史回收可按原提交身份核验已有持久回执和摘要；新模型提交仍按 live claim 拒绝旧 key。

## 显式主张与修复

模型显式声明可独立判断的义务、适用条件、原文覆盖片段、主证据和支持证据，并分别作出判断。宿主检查身份、完整覆盖和一对一判定，不按标点拆分冒充语义原子性，也不以关键词判断业务含义。

完整原段、原生行及同文档相关项目选项供模型对照。跨包条件不能增加当前包覆盖或改变所属章节。缺失公告不得补造；固定原文由 canonical 引用复制。

`contradicts + manual_review` 是合法未解决状态。正式 `execute_turn` 完成工具批次后，可按修复轮次、输入签名和幂等记录执行有界重开：Check → Discover 完整重提 → Organize → fresh Check。重开使旧组织草稿和 Check 授权失效；失败或耗尽保持未解决。

## 验证与清理边界

领域测试使用显式投递 seam，不把递增 turn 当作交付。正式闭环测试必须实际经过 request、SDK session、journal response/tools/commit 和 `execute_turn`；脚本判断只证明协议。

新 schema、提示和配置有新的摘要；不伪改旧检查点摘要来续跑。需要新真实 run 时保留旧失败现场与 usage 账本。通过结构检查不代表召回、主张正确性或响应充分性通过。

当前收尾仍包括：静态 schema 单一来源一致性、残余内部字节页辅助路径与样例清理、全 features/targets 测试和 clippy、最新补丁逐文件复现、新 wire 正式闭环及有界真实运行。未完成项不能列为验收通过；发布保持暂停。

## 集合写入模式

`put_chapters`、`bind_forms`、`put_slots` 使用必填 `mode`：`replace` 全量替换，`upsert` 按 ID 局部更新并保留其余条目。不提供默认模式或旧 append 别名。两种模式均保留各自的目标、续表链、重复 ID、证据和事务校验。运行时共 13 个工具：Discover 3、Organize 8、Check 8。

## 运行参数默认值与边界

应用层统一提供请求输出预算 8192 token、单请求传输超时 180000 ms；环境变量仅作显式覆盖，非法正数仍拒绝。此默认值是保守请求预算，不冒称未知供应商的原生最大输出能力。完整请求继续计入输出预留并受上下文预算约束。隔离启动器无需复制这两个非秘密参数才能启动。

旧 source-review、finding repair、Reviewer/Fill 协议及其操作限额已经删除；实际生产只使用 Discover/Organize/Check，内容生成经 published outline → match_queries → retrieval → response::respond。旧检查点不兼容。分包根据包含提示、工具、历史、图片和输出预留的完整请求 token 判断。

Rust/API/worker 与 Python 验收入口共享 `config/request-defaults.json` 的非秘密请求默认值。Python 比较 effective 值与启动快照，不再要求原始环境必须包含默认参数。私有验收 adapter 独立实施累计费控，生产 journal 仅累计计数；同一请求最多三次的边界重试保护仍保留。旧 Main source package 包装已删除。

生产配置已删除累计操作预算及其暂停/授予入口，不以 Option 或超大数保留。累计调用和 token 仅作诊断计数，算术溢出仍拒绝。私有真实验收 adapter 在发送前实施唯一累计费控（71 调用、7166928 输入 token、581632 输出 token、5438 秒），不属于生产 Run 配置。每请求上下文、取消、超时、传输恢复和重复无进展保护保留。删除后的运行时、响应生成和配置回归必须全部通过。

## 环境默认配置

生产从所选 provider 解析模型身份。已核实的 tokenizer 映射不需要额外 Limits JSON；未知模型必须提供 `KB_AUTHORING_TOKENIZER_PROFILE` 的实测 encoding/calibration，不接受重复的 model_id。有效配置自动绑定 provider.model_id。`KB_TENDER_AGENT_LIMITS` 可省略，也可仅声明 `vision_enabled` 等单项覆盖；已删除的操作配额和工具字节上限字段会被拒绝。

默认 `max_context_tokens=131072` 是应用层单请求预算，包含提示、历史、工具、图片估算和输出预留，不代表自动探测到供应商上下文能力。图片能力必须由同一模型的实测或资料确认。宿主预览明确给出省略数和正式分页入口，不构成读取回执；所有 canonical 要求保持不变，正式工具使用完整请求 token 适配和 opaque cursor 续读。

当前profile的131072为硬上界，局部覆盖只能降低预算；更大窗口必须引入经过验证的新profile契约。分页续页仅传`{cursor}`，首次筛选保存在宿主cursor中，不再重复mode或筛选字段。

## 未发布阶段的变更原则

KnowledgeBrain 尚未发布，架构和协议改造直接采用新设计，不提供旧数据、旧检查点或旧协议迁移，也不保留兼容层。旧验收原始文件和记录仅作为审计证据保留，不作为新合同的运行状态或验收进度。不同合同从新状态执行；同一合同内的持久化恢复、取消、已收到响应的重放去重和未发送暂停幂等继续保留。

模型侧 claim 与 operation 使用宿主签发的短 opaque handle，持久绑定 run、pack、revision、generation 和 canonical 标识。未知、跨域、过期和碰撞严格拒绝，不猜补损坏标识。负提交缺失说明或真实完整检查记录时不得通过；业务拒绝以顶层失败反馈给模型与运行日志。领取工作不算完成进展。
