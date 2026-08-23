# 技术/商务 Agent 稳定性与 WeKnora 可借鉴性报告

## 结论摘要

1. **现有证据不足以确定我们当前 Agent 的具体根因。**材料中没有当前系统的提示词、模型配置、检索结果、工具成功率、完整运行轨迹或回归评测，因此不能严谨地把问题归因于模型、RAG、工具或编排中的某一项。下面只能给出优先排查假设。
2. **首要假设是“任务与编排方式不匹配”，而非单纯模型不行。**对于边界清楚、步骤固定的任务，预定义 workflow 通常比让模型动态决策的 agent 更可预测；agent 更适合确实需要灵活决策的任务，并通常以更高延迟和成本换取能力。许多场景用“单次模型调用 + 检索 + 上下文示例”已经足够。[Anthropic：Building effective agents](https://www.anthropic.com/research/building-effective-agents)
3. **精度必须按“模型 + Agent harness + 检索/工具”整体诊断。**Agent 会跨多轮调用工具并依据中间结果继续行动，早期错误可能传播和累积；评测对象也应是模型与负责输入处理、工具编排和结果返回的 harness 整体。[Anthropic：Demystifying evals for AI agents](https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents)
4. **WeKnora 值得参考，但应借鉴架构模式，不应整套照搬。**它具备双模式路由、显式 RAG 流水线、工具编排、运行时上下文、全链路事件与追踪等可借鉴设计，同时也存在已由源码确认的 rerank 降级和“图谱工具实际未走图谱召回”等问题。[WeKnora](https://github.com/Tencent/WeKnora) [Issue #2703](https://github.com/Tencent/WeKnora/issues/2703) [Issue #2042](https://github.com/Tencent/WeKnora/issues/2042)

## 一、当前不稳定、不准确的优先排查假设

> 以下是待验证假设，不是对当前系统的既定事实。

### 1. 所有任务都走开放式 ReAct，导致不必要的随机性

如果技术或商务任务本身有固定输入、规则和输出格式，却仍让模型自主决定每一步和工具调用，那么系统可能承担了不必要的路径波动。Anthropic 将 workflow 定义为代码预设路径，将 agent 定义为模型动态决定过程与工具使用，并建议从最简单的可行方案开始。[来源](https://www.anthropic.com/research/building-effective-agents)

### 2. 检索、重排或工具的早期误差被后续步骤放大

多轮 Agent 会依据中间结果继续行动，因此错误可能逐轮传播；这意味着最终答案不准确，未必是最终生成环节的问题，也可能源于召回、rerank、参数生成或工具结果。[来源](https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents)

### 3. 只检查最终文案，没有验证真实结果

Agent “声称完成”与环境中的实际结果并不等价。Anthropic 以预订任务为例，是否成功应检查环境中的 SQL 状态，而不是相信最终文本。因此，商务 Agent 的写入、计算、审批或外部操作应尽量以结构化状态或单元测试验证。[来源](https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents)

### 4. 缺少可重复评测，无法区分随机波动与真实回归

模型输出会跨运行变化，因此同一评测任务需要多次试验。没有 eval 时，调试容易变成被动救火，也难以区分回归与噪声、自动发现回归或在发布前覆盖大量场景。[来源](https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents)

### 5. 框架隐藏了提示词、工具输入和中间结果

Agent 框架可能遮蔽实际 prompt 与 response、增加不必要复杂度并妨碍调试；如果团队不了解底层行为，框架本身也可能成为错误来源。[来源](https://www.anthropic.com/research/building-effective-agents)

## 二、WeKnora 最值得参考的设计

### 1. 双模式路由，而不是“一切 Agent 化”

WeKnora 区分一次检索的 Quick Q&A 与多轮 ReAct/智能推理：后者用于多步骤、跨文档、联网或外部工具任务。[仓库](https://github.com/Tencent/WeKnora) [Agent 文档](https://raw.githubusercontent.com/Tencent/WeKnora/main/website-docs/03-features/07-agent.md)

**建议：**默认走确定性较强的检索或 workflow；仅在任务确实需要动态规划、跨源探索或外部工具时升级到 ReAct。

### 2. 将 Agent 建立在清晰的检索、工具与记忆接口之上

WeKnora 的 ReAct Agent 由 `AgentEngine` 驱动，可编排知识检索、网页搜索/抓取、skills、数据工具及动态注册的 MCP 工具。[仓库](https://github.com/Tencent/WeKnora) [Agent 文档](https://raw.githubusercontent.com/Tencent/WeKnora/main/website-docs/03-features/07-agent.md) Anthropic 同样把具备检索、工具和记忆的增强 LLM 视为基础构件，并强调接口应贴合用例、易用且文档清楚。[来源](https://www.anthropic.com/research/building-effective-agents)

### 3. 显式、可插拔的 RAG 流水线

WeKnora 文档给出的链路覆盖意图/查询处理、并行检索、重排、合并、过滤、数据分析、上下文组装、流式生成、引用展开及断线续传；其中阶段实现 `Plugin`，由 `EventManager` 注册并通过动态 `EventType` 列表执行。流水线明确串联 `CHUNK_SEARCH_PARALLEL → CHUNK_RERANK → WEB_FETCH → CHUNK_MERGE → FILTER_TOP_K → DATA_ANALYSIS → INTO_CHAT_MESSAGE → CHAT_COMPLETION_STREAM`，并行检索组合普通搜索与实体搜索。[RAG Pipeline 文档](https://raw.githubusercontent.com/Tencent/WeKnora/main/website-docs/02-architecture/04-rag-pipeline.md)

**建议：**把技术/商务 Agent 的召回、重排、证据门禁、分析和生成拆成可独立记录与评测的阶段，避免把所有逻辑塞进一个大 prompt。

### 4. 会话历史与本轮运行上下文分离

WeKnora 的 Agent 引擎跨轮次无状态：调用方每轮从数据库重建历史，再把 `llmContext` 传给 `Execute`。知识库详情、固定文档、当前时间和 session ID 则作为本轮 `runtime_context` 注入，不写入历史，从而避免后续轮次沿用过期范围。[Agent 文档](https://raw.githubusercontent.com/Tencent/WeKnora/main/website-docs/03-features/07-agent.md) [Agent Service 源码](https://raw.githubusercontent.com/Tencent/WeKnora/main/internal/application/service/agent_service.go)

**建议：**显式区分持久会话事实与每轮临时范围，尤其是当前项目、客户、知识库、权限和时间条件。

### 5. 全轨迹观测，而不只记录最终答案

WeKnora 通过 EventBus 发出思考、工具调用、工具结果、最终答案和完成事件，再由 Handler 转成 SSE 并持久化。每次执行建立 `agent.execute → agent.round.N → agent.tool.<name>` 的 Langfuse span，记录轮次、token 使用和截断后的工具输出预览，并对 `database_query` 的 SQL 参数做遮罩。[Agent 文档](https://raw.githubusercontent.com/Tencent/WeKnora/main/website-docs/03-features/07-agent.md) Anthropic 将完整 trial 中的输出、工具调用、推理、中间结果及其他交互定义为 transcript/trace/trajectory。[来源](https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents)

**建议：**将一次请求的路由、检索候选、rerank、每次工具参数/结果、模型轮次、最终答案及真实环境结果关联到同一个 trace。

### 6. 引用标识与知识治理

WeKnora 在模型调用前用短别名替换持久 chunk/document/web UUID，并在流式输出时解码，避免把真实 UUID 暴露给模型。[Agent 文档](https://raw.githubusercontent.com/Tencent/WeKnora/main/website-docs/03-features/07-agent.md) 其 Wiki Mode 还可把原始文档组织为可维护、互相链接的 Markdown 知识库和交互式知识图谱。[仓库](https://github.com/Tencent/WeKnora)

这适合作为后续知识治理参考，但不能替代对召回质量和 Agent 执行路径的评测。

## 三、不能照搬的两处风险

### 1. LLM rerank 存在静默降级路径

`knowledge_search` 的 LLM rerank fallback 每批处理 15 条，`maxTokens := len(batch)*20 + 100`，满批为 400 tokens，并按批次串行执行。如果模型返回空内容或全部无法解析，`parseScoresFromResponse` 会报 `no valid scores found in response`，随后整批退回原始分数。[Issue #2703](https://github.com/Tencent/WeKnora/issues/2703) [源码](https://raw.githubusercontent.com/Tencent/WeKnora/main/internal/agent/tools/knowledge_search.go)

**启示：**不能只看流程是否“完成”，还应记录 rerank 解析成功率、降级次数及降级后质量。

### 2. Agent 的“知识图谱工具”实际未由图结构驱动召回

Agent 工具 `query_knowledge_graph` 调用的是 `HybridSearch`，没有调用 `SearchNode`；图配置主要进入响应元数据，召回 chunk 仍来自混合搜索。与之不同，普通聊天链路会先让 LLM 抽取实体，再把实体名数组传给 `SearchNode`，最后把图节点的 chunk ID 映射回文本。[Issue #2042](https://github.com/Tencent/WeKnora/issues/2042) [Agent 工具源码](https://raw.githubusercontent.com/Tencent/WeKnora/main/internal/agent/tools/query_knowledge_graph.go) [实体抽取](https://raw.githubusercontent.com/Tencent/WeKnora/main/internal/application/service/chat_pipeline/extract_entity.go) [实体搜索](https://raw.githubusercontent.com/Tencent/WeKnora/main/internal/application/service/chat_pipeline/search_entity.go)

**启示：**工具名称和描述不能代替实现验证；应测试工具是否真的使用了宣称的数据源与算法。

## 四、建议的落地顺序

1. **先建基线评测集：**分别覆盖技术和商务 Agent 的典型任务、边界条件、工具失败与知识不足场景；同一任务执行多次。
2. **保存完整 trace：**同时记录最终答案与真实环境结果，对检索、rerank、工具调用和每轮模型输出分段评分。
3. **做三路对照：**比较“单次 LLM + 检索/示例”“固定 workflow”“ReAct Agent”，以任务成功率、跨次波动、延迟和成本选择路由。
4. **优先 workflow 化固定任务：**可采用“输入校验 → 检索 → 重排 → 证据门禁 → 规则/工具执行 → 结果验证 → 生成”的受控链路。Prompt chaining 可通过拆分步骤并在步骤间加入程序化 gate，以额外延迟换取更高准确性。[Anthropic：Building effective agents](https://www.anthropic.com/research/building-effective-agents)
5. **最后才扩展开放式 Agent：**仅用于多步骤、跨文档、联网或外部工具等确需动态决策的任务，并设置轮次、工具、权限及人工审批边界。

## 最终判断

**可以参考 WeKnora，推荐参考其“双模式路由 + 显式 RAG pipeline + 运行时上下文隔离 + 全轨迹观测”四项设计；不建议直接迁移整套 Agent。**当前最重要的不是先更换框架，而是用可重复 eval 和完整 trace 找出问题究竟发生在路由、检索、rerank、工具、状态还是生成阶段。由于缺少当前系统实证数据，本报告不能进一步断言唯一根因。
