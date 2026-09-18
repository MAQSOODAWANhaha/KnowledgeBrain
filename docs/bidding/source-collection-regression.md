# F1/F2 文件集合与来源回归

> 历史验证记录：仅说明所记录版本的结果，不是现行设计或新流程验收。当前方案与任务见 [统一方案](../../plans/bidding/product-two-phase.md)。

当前文件集合回归已使用 V4 提取 Agent 的完整冻结输入和持久化发布协议。下文早期需求编译器及显式 apply 的记录仅为历史证据；这些旧生成设计不再指导当前 DOCX 链路。没有新增业务关键词、固定章节、样稿匹配规则或生产环境默认值。

## 需求拆分修复

`requirement_compile.rs` 原先在编号前缀、编号片段、普通行及长行句子四处过滤少于 8 个字的内容。同一来源中存在较长片段时，“不得分包”“须盖章”等短要求被静默丢弃。现改为保留每个非空片段，继续执行原有字节和片段数量上限；不据长度推断业务含义。

需求编译模块回归共 7 个通过，包括新增的混合长短行、编号短片段、长行短句及容量上限四例。修前同组测试为 3 通过 / 4 失败，修后为 7 通过 / 0 失败；这是聚焦模块结果，非整个 bidding package 或 CI 通过。

## 数据库覆盖

可重跑文件：[document_collection_acceptance.sql](../../crates/bidding/tests/sql/document_collection_acceptance.sql)。测试动态生成 UUID、内容及摘要，从上传请求读取 converter 身份。连接串由调用者提供，测试不内置服务器地址、端口、凭据或真实样稿路径。

| 场景 | 断言 |
| --- | --- |
| A → A+B → A+B+补遗 | 完整成员及顺序保持；旧 source revision ID 与摘要复用 |
| 再加入 pending 文件，随后变 failed | 成员仍可见，分别有对应 warning，三个 ready 来源保留 |
| 每次冻结及同请求重放 | 每个新集合只增加一个 compile request，不增加 tender-process request；重放身份不变 |
| 来源集合 | disposition 恰好覆盖全部 ready SourceUnit；编译输入覆盖全部要求来源而非本轮差量 |
| 后续轮次 | 每轮重新读取此前所有冻结编译输入及集合 payload，均保持不变 |
| 非法输入 | 重复成员、错误 CAS、遗漏/重复/外来来源被指定错误拒绝；current 与请求/集合数量保持 |

前提是独立 `knowledgebrain_test_*` 数据库已安装三份 baseline，连接角色可 `SET ROLE kb_app_owner`。执行入口：

```sh
psql -X --dbname="${KNOWLEDGEBRAIN_TEST_DATABASE_URL:?dedicated test database required}" \
  --file=crates/bidding/tests/sql/document_collection_acceptance.sql
```

fixture 使用事务并最终 `ROLLBACK`，失败由 `ON_ERROR_STOP` 返回非零。该脚本不创建或重置调用者数据库。运行仅应指向专用测试库。

## 实证范围与剩余项

本地最终 fixture 在独立 PostgreSQL 16/pgvector 容器执行，五轮及五类拒绝断言通过，退出 0。容器无网络及端口映射，基线使用冻结副本；fixture 回滚后项目数为 0，专属容器及临时卷已删除，清理退出 0。来源元数据由测试模拟成功解析结果，因此只证明数据库复用和编译输入边界，不证明真实转换/OCR/模型调用次数、真实文件变更与失败重试。

还需真实 A/B/补遗解析链的调用计数、业务类别与页码覆盖、新轮 Workspace 不继承旧响应/检查/证明页码等验证。当前 fixture 使用结构化表行，不验证现有初始 disposition 的关键词判断准确性；本次没有新增或扩大该规则，也不能据此声明语义提取完成。

本机证据：F2 `/tmp/knowledgebrain-f2-source-coverage.5aaV1aoI/`（before、red/green）；F1 最终 `/tmp/knowledgebrain-f1-collection.mocmwi4x/`（冻结 SQL、摘要清单、runner、日志、退出码与清理记录），前一版五轮结果保留于 `/tmp/knowledgebrain-f1-collection.skr4bnze/`。这些路径不是永久发布资产。没有修改生产 baseline、P2 runner、CI 或依赖；没有提交或部署。

## 后续切片：空来源、乱序发布及真实 DOCX 结构

需求编译器还存在混合输入漏洞：一条正常来源和一条空来源一起输入时，返回成功并把两条都列为已覆盖，空来源却没有要求或提示。新增回归先真实失败（1 例，exit 101）；修复后在处理空字符串或纯空白来源时返回含来源 ID 的错误，走已有编译失败路径，不伪造要求、不静默声明完成。普通来源及带表单定义的来源均覆盖，完整模块 **8/8，exit 0**。这属于已选 requirement 来源的无效技术输入，不是依据业务风险锁定编制，也不改 disposition 或新增词表。

同一 SQL fixture 新增了五轮编译结果的倒序发布。预先通过真实 Workspace mutation 保存一个非空人工节点，再先发布最新轮、后发布四个旧轮：最新结果要求显式 apply；迟到结果持久化为历史，均不回退当前 requirement set/projection。每轮结果保留其冻结集合身份及要求数量；同请求重放不新增 requirement/projection，整个人工 Workspace 快照保持不变。使用的是合成编译结果，验证 SQL 发布协议，不冒充 Rust 编译器与数据库的端到端运行。

最终 SQL **exit 0**，fixture 回滚后项目数为 0，专属容器/临时卷清理 **exit 0**。第一次因 PostgreSQL 初始化临时服务退出窗口而连接失败，尚未执行 fixture；失败及清理记录保留。第二次改用容器内 TCP 就绪检查后通过，不修改 P2 runner。

用户真实 DOCX 另运行现有 `_docx_structured_units`：产生 **30 单元（5 section、4 table_region、21 table_row）**，无空白单元，key 唯一、ordinal 连续。独立读取 `word/document.xml`，486 个非空直属正文段落的文本与顺序和解析 section 输出一致，4 张表及 21 行数量一致，原件摘要保持。本次只验证正文结构提取，不验证全部单元格合并/字体、页眉页脚、OCR、gRPC、调用次数或语义提取。

**新轮实现缺口仍存在：** 发布仅推进可用 requirement projection，保留旧 Workspace 并提示显式应用。现有 `kb_bid_v2_advance_workspace_projection` 会复制旧节点、块、binding 和 quote snapshot，因此不能作为“不继承旧响应/检查/页码”的新轮整稿实现。O1-S 应建立明确的新轮 DOCX 输入和版本边界；旧应用操作在新链接替前保留，不以此次乱序发布成功宣布完整 F2 已完成。

本切片证据位于 `/tmp/knowledgebrain-f2-publication.63sf05sk/`：`before/`、`empty-source-red.log`、`empty-source-green.log`、`real-source-summary.json`、`real-source-coverage.json`、`parser-source-sha256.json`；最终数据库执行在 `database-attempt2/`，含精确 SQL/基线副本、摘要、完整日志与清理收据。上述先前 7/7 与 SQL 结果保留为历史批次，本段是后续增量；没有改生产 SQL、P2 文件或依赖。

后续 O1-S 已增加独立 DOCX 新轮/初始版本持久化，不使用旧 projection apply 复制正文，见[新轮接缝](docx-rounds.md)。这次后续切片修改了 Bidding 所属 baseline，并扩展同一 SQL fixture；上述“没有改生产 SQL”只描述当时的 F2 切片，不代表后续始终无 schema 增量。初稿生成、HTTP/对象读写整链、编辑会话及完整新轮业务隔离仍待验收。


## V4 集合与迟到结果回归

`document_collection_acceptance.sql` 已改为调用现有 V4 loader、claim、reserve、checkpoint 和 publisher；仅在专属测试事务中定义辅助函数，模型未运行，合成结果明确保留 `needs_review` 和 `SQL_BOUNDARY_FIXTURE` finding，不冒充语义提取通过。冻结输入传入测试专属 runtime，读取全部 ready 来源，不再按预先判定的 requirement 过滤。无调用方的 V3 loader/publisher 及授权已删除；旧 Workspace apply 不再是新分析发布步骤。

真实回归发现：V4 原先按项目全部历史判定取 `max(revision)+1`，四个迟到结果也占用版本，下一轮当前发布因此违反既有连续 CAS。修复后只有当前冻结输入可创建新的判定版本；迟到分析连同原冻结身份保留在 requirement 历史，既不创建当前判定版本，也不推进当前 requirement/projection。没有放宽 CAS，没有增加表、字段或 migration 文件。

隔离 PostgreSQL 已验证五轮完整集合、重复/非法输入、四轮迟到发布、重放、非空人工草稿保护，以及随后两轮 DOCX 的 current/CAS/权限/历史。新增同集合连续两次判定的回归：旧分析先完成时仍不能发布为 current，最新分析恰好推进一个判定版本，旧稿保持不变。脚本模型的 Agent 发布/重放/物理预算、诊断 UTF-8 与错误 owner 两项数据库回归通过，证据在 `artifacts/bid-full-sample/retirement/v4-agent-regression-final/`；78 项模块测试和 17 项 baseline 合同通过，见 `retirement/v4-final-tests.log`。这些证据不替代真实模型的完整招标义务覆盖验收。
