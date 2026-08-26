# 招投标证据检索 V2

| 项 | 值 |
| --- | --- |
| 状态 | 已批准方案，待实施与切流 |
| 所有者 | 知识库（检索与证据合同）+ 招投标（生产 policy 切换与 schedule 恢复） |
| 主消费方 | 招投标 `KnowledgeRetrievalPort` |
| 次要消费方 | `POST /search`，仅在招投标切流后复用 |
| 明确排除 | `POST /answer`、LLM rerank、Matching decision/verifier 改造 |

本文是招投标证据检索准确度与稳定性的唯一活动实施定义。WeKnora 只提供「多路召回 → 融合 → 专用 rerank」的分层参考，不是本方案的第二真源。

## 1. 当前合同事实与审查决定

生产路径当前满足：

1. Bidding schedule 固定请求 `knowledge-evidence-v1`，policy 为 `knowledge-evidence-v1:lexical-current-eligible`，每个 requirement 最多 64 hits、单 chunk 最多 256 KiB、总计最多 8 MiB。
2. `PostgresKnowledgeRetrievalAdapter` 扫描完整 current eligible scope，不过滤 `chunk_type`；quote 是整个 chunk，offset 固定 `0..chunk_byte_length`。
3. `LexicalEvidenceVerifier` 的唯一 `supported` 条件是：requirement 与 quote 分别删除全部 Unicode whitespace、转小写后，quote 包含 requirement 全文。其它候选为 `insufficient`；无候选为 `NO_EVIDENCE`。
4. `retrieval_raw_score` 只作为冻结元数据。supported 候选推荐顺序固定为 `(route_product_ordinal, retrieval_rank, candidate_identity_sha256, evidence_v1_sha256)`。
5. 当前 `kb_knowledge_attest_matching_scope_v1` 明确只接受 `retrieval_contract_version=knowledge-evidence-v1`。

据此批准以下产品/架构决定：

- v1 行为和字节输出保持不变。所有新算法从第一行起只存在于未启用的 v2 分支或 shadow 路径。
- v2 可信冻结证据类型仅为 `text | parent_text | image_ocr`。`question | summary | wiki_page | image_caption | graph` 及其它派生内容只能作召回信号。
- v1 中由生成问句、Wiki 等派生内容造成的 supported 不承诺保留；v2 将其定义为 `derived-only false support`，单独统计。
- v2 不修改 verifier、decision、coverage 或 pick 代数。RRF、向量分与 rerank 分都不能产生 `supported`。
- 若 v2 的公平精确证据前缀受单 chunk/总字节配额约束而无法完整冻结，则显式返回不可重试的 `QuotaExceeded`，不发布部分 v2，也不无限重试。

## 2. v1 不可变路径与 v2 隔离

端口必须按 `retrieval_policy.contract_version` 精确分派：

```text
knowledge-evidence-v1 -> 原 lexical current-eligible 实现，字节级不变
knowledge-evidence-v2 -> 本文算法，仅 shadow 或显式切流后可用
其它值                -> InvalidRequest
```

规则：

- 不在 v1 `retrieve()` 内加入 source filter、must-keep、RRF、rerank、fold 或新排序。
- v1 回归 fixture 锁定 eligible_versions、hit identities、rank、score、bytes、digest 与 offsets。
- v2 shadow 不能写 manifest/report，也不能改变 v1 返回或错误。
- 同一次 schedule 的全部 requirement 和 route 必须使用同一 contract/policy digest；禁止混合 v1/v2 hits。

DTO 形状可继续使用 `KnowledgeEvidenceBatchV1`，但 `retrieval_contract_version` 和 policy identity 明确区分算法。若后续改变 DTO shape，必须另升 schema，不借 v2 算法名偷换结构。

## 3. 精确源文 must-keep

### 3.1 成员判定

v2 首先对完整 eligible current scope 的可信源文做 verifier 等价扫描：

```text
trusted_source_types = text | parent_text | image_ocr
normalize(s) = 删除所有 char::is_whitespace() 字符，再逐字符小写
exact_source_hit = normalize(chunk.content).contains(normalize(requirement))
```

该扫描必须穷尽可信源文，不能用 `lexical_score==1`、token、tsv、ANN、关键词 top-k、RRF 或 rerank 代替。扫描不能先用 `max_chunk_bytes` 隐藏超限的 exact hit；超限必须进入配额判定。

### 3.2 确定性分层

设 `V` 为至少有一个 exact source hit 的 product version 数，`K=max_hits`。

**A. 公平前缀**

1. 每个有 exact hit 的 version 选一个 primary。
2. primary comparator：`chunk_byte_length ASC, document_id ASC, source_chunk_id ASC`。
3. version comparator：`product_id ASC, product_version_id ASC`。
4. 覆盖前 `min(V,K)` 个 distinct versions；若 `V>K`，这是 hit-count 合同下的显式截断，记录 `exact_versions_truncated=V-K`，但 eligible_versions 仍完整。

**B. 其余精确源文**

- A 后仍有 hit/byte 名额时，按 `product_id, product_version_id, chunk_byte_length, document_id, source_chunk_id` 填入剩余 exact source hits。
- 未进入 B 的 exact hits 必须计入 `exact_hits_truncated`，不能静默消失。

**C. 语义尾部**

- 只使用 A/B 后的剩余名额；算法见 §4。

A/B 构成不可变前缀：先赋 dense `retrieval_rank=1..M`，C 只能得到 `M+1..N`。RRF/rerank 不得删除、重排或改写 A/B rank。

v2 `retrieval_raw_score` 语义固定为：A/B 写 `1.000000`（exact trusted-source containment），C 写专用 rerank relevance score 的固定 6 位小数。排序权威是分层与 `retrieval_rank`，不是跨层比较 raw score。

### 3.3 配额

- `max_hits`：A 的合同目标就是 `min(V,K)`；因此 `V>K` 是显式 hit-count 截断而非 QuotaExceeded，按上述确定性规则选择并计量，不把 eligible scope 截断。请求若无法容纳这个已定义的 A 目标，则 fail closed。
- `max_chunk_bytes`：若任一选入 A 的 primary 超限，且该 version 没有另一条合规 exact source hit，返回 `QuotaExceeded`。
- `max_total_bytes`：若完整 A 前缀不能同时放入，返回 `QuotaExceeded`。
- B/C 只能填剩余配额；跳过项必须分别计入 exact/tail truncation metrics。
- `QuotaExceeded` 是新增的 typed、永久错误。调用方不得把它当 `Unavailable` 自动重试。

该规则承诺：在合同 hit-count 公平目标与字节配额可满足时，不丢失 exact trusted-source version coverage；无法满足时 fail closed，而不是发布误导性部分 v2。

## 4. 语义尾部

语义尾部用于提高人工可审的 `insufficient` 候选质量，不改变 supported 判定。

### 4.1 派生信号折叠

派生内容可以参加召回，但只能通过明确映射落到完整 live 可信源文快照：

- question/summary：仅接受有效 `parent_chunk_id` 指向可信源文；
- image_caption：仅接受同文档、稳定关联到 `image_ocr`；
- graph/wiki：仅接受持久化 source/chunk ref 明确指向可信源文；
- 无唯一映射时丢弃信号，禁止按 `start_at` 猜「最近正文」。

折叠后必须重新读取并冻结 source row 及其 live document 的完整 DTO 快照：

```text
product_id, product_version_id, document_id, source_chunk_id, trusted chunk_type,
frozen_document_display_name = live source document.file_name,
content bytes, byte_length, sha256,
quote_start_offset=0, quote_end_offset=byte_length
```

不得把派生文本、派生 display name、派生 offsets 或派生 digest 挂到 source id 上。若折叠后的 source 满足 verifier 等价 containment，它本应已在 §3 A/B 候选集；否则只进入 C。

### 4.2 中文关键词、向量与 RRF

C 候选池可由两路构成：

- keyword：拉丁/数字词 + CJK bigram；单字 CJK 与混合空白必须有明确 fallback fixture；写入和查询 tsv 使用同一 tokenizer，变更后只重建 tsv，不改 vector；
- vector：使用目标 version 声明的 embedding model，查询 embedding 失败必须显式失败，禁止静默改用 stub。

关键词/向量各自通道内 top-k、稳定排序后做加权 RRF：

```text
rrf = vector_weight/(rrf_k+vector_rank)
    + keyword_weight/(rrf_k+keyword_rank)
```

同一 source 接到多个派生信号时，每个通道只取最好 rank，再按 source_chunk_id 去重。RRF 只决定送入 rerank 的 C 候选顺序，不进入 A/B。

### 4.3 专用 rerank

- 使用专用 cross-encoder/rerank 服务，不使用 chat LLM，不解析自由文本。
- query 是 requirement 原文；documents 是折叠后的完整可信源文。
- 非空 C 必须 rerank；空 C 不发模型请求。
- 响应先按输入 index 校验和归位：必须完整覆盖输入，且无重复、缺失、越界、NaN/Inf；响应数组顺序不具有排序语义，任何非法或部分成功视为整批 `Unavailable`。
- rerank 只重排 C；失败时 v2 整体失败，禁止退回 RRF 仍标记 v2。
- 禁止 `model_score + rrf_score` 混分；C 的 final score 就是规范化后的模型 relevance score。
- C 必须使用总序 comparator：`normalized rerank score DESC, stable pre-rerank RRF rank ASC, complete source identity ASC`。模型同分或服务端改变响应行顺序时，`retrieval_rank` 和 report bytes 必须不变。
- focused fixture 必须覆盖「全部模型分相同」和「同一 index→score 映射但响应行逆序/乱序」，验证归位后序列一致。

## 5. v2 policy、模型身份与 attestation

### 5.1 Canonical policy identity

Knowledge 提供不可变的 `RetrievalPolicyV2` artifact/digest；Bidding 只选择并冻结它，不自行拼可变字符串。canonical digest 至少覆盖：

```text
contract_version + normalization_version + trusted_source_types
A/B/C comparators + quota semantics
keyword tokenizer/version + channel top-k/thresholds
embedding policy/version + RRF k/weights
rerank provider protocol version + immutable model revision/config digest
rerank top-k/timeout + score normalization
```

mutable model alias、endpoint 当前内容或未版本化的服务名不能充当模型身份。secret 不进入 digest；非秘密 endpoint/config identity 进入不可变 config revision。

创建新 schedule target 或执行生产 cutover 时，请求中的 `policy_sha256` 必须等于 Knowledge 当前批准用于新目标的不可变 artifact digest，否则 `InvalidRequest`。既有 durable target 的恢复必须继续使用并接受它已经冻结且仍处于 supported 状态的 policy/model/config digest；current policy promotion 不得使既有 intent 失效，也不得让 recovery 偷读当前 artifact。只有 watermark/frozen snapshot identity 不匹配、未知 digest 或该 artifact 被显式 revoke 才是 terminal。模型或参数改变必须产生新 digest，旧 report 和既有 intent 保留旧冻结身份。

### 5.2 v2 attestation

Knowledge 新增独立的 v2 attest/verify 合同（或显式按 contract 分派的等价实现），不能只修改 Bidding 的字符串。v2 attestation 必须证明：

- eligible version scope 完整且类型正确；
- 每个 hit 是该 scope 内 enabled/index_ready 的 live trusted source row，`chunk_type` 属于批准的 trusted source types；
- product/version/document/source id、`frozen_document_display_name=live source document.file_name`、bytes、length、digest、`0..len` offsets 完全相等；
- hit contract 与 durable target 冻结且仍 supported 的 canonical policy digest 为 v2；
- rank dense、无重复、数量和字节配额有效。

Attestation 不重新执行远程 rerank；rerank 输出通过 policy identity、冻结 rank/raw score 和 immutable report 保留。

## 6. Schedule 失败、恢复与所有权

v2 retrieval/rerank 发生在 frozen manifest 持久化之前，因此恢复单位是 **schedule**，不是 route execution job。

Bidding 必须扩展现有 schedule intent/recovery fence：

1. 在外部 retrieval/rerank 前持久化或确认 `(project, watermark, snapshots, v2 policy digest)` 的 durable schedule target；首次创建只接受当时批准用于新目标的 current policy，恢复只读取该 target 冻结且仍 supported 的 digest。
2. 任一 requirement 返回瞬时 `Unavailable`：丢弃本次全部内存 batches，不写 manifest/hits/jobs，不混入 v1；intent 保持 retryable，进程重启后仍按同一冻结 target 和 backoff 重跑 schedule。
3. `QuotaExceeded`、未知 contract、policy/model 配置不匹配或 artifact 被显式 revoke：标记 terminal/manual-action；在显式新 intent 或 config/policy revision 前，自动调度不得再次调用 retrieval/rerank。
4. 全部 requirement 成功后，v2 attestation、manifest、frozen hits、jobs 与 intent completion 按现有 fence **恰好提交一次**；并发恢复、响应丢失或进程重启不得产生第二份 manifest。
5. watermark/frozen snapshot identity 改变后旧 recovery intent 失效，不能发布陈旧证据；普通 current policy promotion 不属于 identity mismatch，不能使旧 intent 失效。

所有权：

| 领域 | 必须实现 |
| --- | --- |
| Knowledge | v1 byte-lock；v2 retrieve 分支；exact prefix、tail、rerank；typed errors；canonical policy artifact；v2 attest/verify |
| Bidding | 选择/冻结 v2 policy；schedule intent retry/terminal 状态；v2 payload 与 attestation 调用；shadow 和生产 cutover |
| Matching workflow | verifier、decision、candidate/pick 代数保持不变 |

## 7. 端口优先实施顺序

### P0 — 锁定合同，不切流

- v1 golden/regression tests：完整 batch、派生 chunk 行为、rank/score/bytes/offsets。
- 增加 v2 contract dispatch、未知合同拒绝、`QuotaExceeded` 类型、canonical policy schema 与 v2 attestation contract。
- 所有 v2 路径只可在测试/shadow 调用。

### P1 — 精确源文前缀

- 实现 §3 穷尽扫描、可信类型、A/B prefix、确定性 rank、配额与 QuotaExceeded。
- 只产生 shadow 输出；验证 v1 不变。

### P2 — 语义尾部召回与折叠

- 实现显式 source mapping、CJK tokenizer/tsv rebuild、vector recall 和 RRF。
- C 仍不进入生产 report。

### P3 — 尾部专用 rerank 与 schedule recovery

- 接入 immutable rerank model/config；非空 C 必须成功，equal-score/乱序响应 fixture 证明 C total comparator 与响应行序无关。
- Bidding 完成 durable schedule retry/terminal 语义；故障注入必须证明：瞬时 `Unavailable` 经 backoff 与进程重启后只产生一次 fenced manifest commit，且无混合/部分 hits；永久 `QuotaExceeded`/配置错误进入 terminal 后，直到显式新 intent/config revision 前自动 retrieval 调用次数为 0。

### P4 — Shadow 门禁与招投标切流

- 同一 requirement 并行计算 v1/v2，v2 只记差异。
- §8 门禁通过并人工批准 policy digest 后，Bidding 原子切到 v2。
- 切流与 Knowledge v2 attest/verify 同一发布窗口验收。

### P5 — 可选复用

- 招投标稳定后，`/search` 可复用 tokenizer、fusion、source folding、rerank adapter。
- `/search` 不阻塞 P0–P4，也不得反向改变 v2 port 合同。
- `/answer` 不在本方案。

## 8. 招投标评测、shadow 与发布门禁

评测集按 requirement × product_version 标注，至少覆盖：中文精确条款、空白/大小写变体、跨 child 的 `parent_text`、图片 OCR、近义但不精确、单字 CJK、派生问句复述条款、65+ eligible versions、超大 chunk、总字节溢出、rerank timeout/残缺响应、equal-score/乱序 rerank 响应、policy promotion 后冻结 intent 恢复、显式 artifact revocation。

必须记录：

- `trusted_source_containment_recall`：合同配额可满足时必须 100%；
- requirement+product_version 的 source-supported membership；
- v1-supported raw `source_chunk_id` recall（按 trusted/derived 分组）；
- `derived_only_v1_false_supported` 数量；它允许在 v2 消失，不算 source recall 回归；
- A prefix 的 distinct-version coverage、`exact_versions_truncated`、`exact_hits_truncated`；
- v1/v2 recommended tuple 差异及原因；
- source purity：v2 frozen hits 必须 100% 属于可信类型；
- rerank/schedule `Unavailable` 率、QuotaExceeded 数、延迟与重试次数。

发布门禁：

1. v1 golden 全绿且生产 v1 输出未改变。
2. v2 在配额可满足样本上不丢 exact trusted-source support；不可满足时按合同显式 truncation 或 `QuotaExceeded`，不发布静默部分前缀。
3. rerank 前后 A/B identities 与 ranks 完全相等；C 在 equal-score 和同一 index→score 映射的乱序响应下得到相同 dense ranks/report bytes。
4. shadow 无 v1/v2 混合 manifest、无旧 watermark 发布、无生成内容冻结成 v2 证据。
5. schedule fault gate：瞬时 `Unavailable` 经进程重启/backoff 恢复为恰好一次 fenced commit；`QuotaExceeded`/永久配置错误进入 terminal 后，在显式新 intent/config revision 前后续自动 retrieval 次数为 0。
6. v2 attest/verify gate：合法冻结 payload 可重复正向 attest+verify；分别篡改 policy digest、trusted source type、product/version/document/source identity、document display name、bytes/digest/length/offsets 或 dense ranks 时必须拒绝。
7. 新目标创建只接受 current approved policy；既有 intent 在 current policy promotion 后仍用 pinned supported digest 恢复，显式 revocation 才 terminal。
8. 真实技术与商务条款人工抽查通过后才允许切 policy。

## 9. 非目标

- 不改变 `LexicalEvidenceVerifier`、support 优先级、coverage、candidate recommendation comparator 或人工 pick。
- 不把 cosine、RRF、rerank 当 semantic support。
- 不承诺保留 question/wiki/summary 等 derived-only v1 supported。
- 不做 query rewrite、MMR、FAQ/Wiki boost、LLM rerank或问答生成。
- 不先改 `/search` 再回头接端口。
- 不提供生产 `v1|v2` 自动降级；contract 只能由 Bidding 明确选择。

## 10. 改动文件地图

| 所有者 | 文件/位置 | 计划改动 |
| --- | --- | --- |
| Knowledge domain | `crates/domain/src/knowledge_retrieval.rs` | v2 contract/policy identity、typed QuotaExceeded、验证规则 |
| Knowledge adapter | `crates/storage/src/knowledge_retrieval.rs` | 保留 v1；新增 v2 exact prefix、tail 和 dispatch |
| Knowledge index/storage | `crates/index/src/lib.rs`, `crates/storage/src/persist.rs` | tail tokenizer、tsv rebuild/query、原始通道召回 |
| Knowledge rerank | 新建窄模块（最终路径按现有 crate 边界确定） | 专用 HTTP adapter、响应验证、model revision |
| Knowledge baseline | `migrations/knowledge_base_baseline.sql` | v2 policy/attest/verify 与 role 权限；v1 合同保留 |
| Bidding adapter | `crates/storage/src/bid_matching.rs` | v2 policy cutover、schedule intent recovery、全批失败语义 |
| Bidding baseline/runtime | `migrations/bidding_v1_baseline.sql`, 必要 runtime seam | v2 schedule identity、retry/terminal 状态与 fence |
| Matching workflow | `crates/bid/src/matching/workflow.rs` | 仅增加不变量回归测试；生产 verifier/代数不改 |
| Evaluation | `testdata/retrieval_eval.json` 及 focused tests | shadow、quota、rerank equal-score/乱序、schedule restart/terminal、attest/verify tamper、真实条款 fixture |
| Docs | `docs/knowledge-base/domain.md`, `plans/bidding/matching.md` | 实施并批准切流后回写稳定合同与消费方 policy |
| Deferred | `crates/search/*` | P5 才复用，不进入招投标切流 PR |

## 11. PR 切片

1. **PR-A（Knowledge）**：v1 golden + v2 dispatch/policy/error/attestation skeleton；不切流。
2. **PR-B（Knowledge）**：exact trusted-source A/B prefix、fairness、quota、shadow tests。
3. **PR-C（Knowledge）**：派生映射、中文 keyword/vector/RRF tail、tsv rebuild。
4. **PR-D（Knowledge + Bidding 协调发布）**：专用 rerank、durable schedule recovery、故障注入；仍不切流。
5. **PR-E（Bidding）**：shadow 报告与受控 policy cutover；真实运行验收。
6. **PR-F（可选）**：`/search` 复用；不得改变已发布 v2 port。

各 PR 分别报告已实现、本地验证、已提交、已部署和真实运行验收。任何阶段未满足上一阶段门禁，不得提前启用 v2。
