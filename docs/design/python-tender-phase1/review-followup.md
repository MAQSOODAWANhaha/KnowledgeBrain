# 追加评审逐项处理与真实文件准备

评审基于 `a009307`；本轮仍在 [Draft PR #24](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/pull/24)。
所有修复仅涉及独立 Python 路径与相关文档，Rust/Cargo/默认运行链路不变。真实模型请求为 0。

## 文档先行记录

- `9f402cb`：来源版本与领域修订、上游摘要、确认失效、执行配置与标注集目标。
- `0d790b7`：宿主问题等级、不可变计划约束、累计条件合同。
- `edea00b`：纠错入口和来源配置恢复保护。
- `6b4eea8`：literal OCR 按图片内容复用，保留全部出现位置。
- `d80bfd5`：无法可靠分句的条件保留待审引用，避免重复发送长文本。
- `966812f`：评估最小完整章节、父章正文、完整请求准入与Rust规则借鉴边界。
- `f9ef8cd`：规范冻结、分包引用、请求去重、各层字节及2倍/4倍增长验收。
- `0c34eb5` / `7022d48`：唯一字段字典、共享schema、分包计划修订身份、有限作用域审议合同。

## 评论逐项对应

| 评论 / 项目 | 处理结果 | 代码与回归 |
| --- | --- | --- |
| [6159352822](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/pull/24#issuecomment-6159352822) 产物修订；[6159516770](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/pull/24#issuecomment-6159516770) 同来源纠错；[6159745297](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/pull/24#issuecomment-6159745297) 第4项 | 已修复 | `integrations/store.py` 的 source version / domain_revision、input_digest、correct；`integrations/cli.py` 完整分包纠错；`workflow/nodes.py` 新修订重验。`test_domain_revisions.py` 和 `test_phase1.py` 验证 r1/r2 同原件、r1确认不可授权r2、旧结果只作历史、同修订幂等。 |
| 6159352822 补投与终态回执 | a009307已有主要保证，本轮补齐文档与取消回执 | 有效租约下不补投；pending/published/running与处置分开；claim token围栏；取消/来源改版/领域纠错均记录cancelled原因及完成时间。真实PG与Redis骨架回归。 |
| 6159352822 checkpoint原子围栏；6159516770旧epoch写入测试 | 沿用原机制，补实际竞态验证 | `FencedPostgresSaver` 同连接短事务持run行锁。真实PG保留旧连接，失效租约、新执行者写入后，旧`aput`及`aput_writes`均拒绝；最新checkpoint未污染。 |
| 6159352822 恢复配置固定 | 已修复并扩大到实际来源配置 | `llm/client.py` 固定非秘密endpoint/model/context/prompt摘要；密钥轮换允许。`workflow/source_spec.py` 固定实际生效原生解析参数，修订内pack-plan另固定预算身份；runner在读取checkpoint前prepare，完整checkpoint和局部成功恢复均不能绕过。 |
| 6159352822 真实质量目标 | 已补标准，尚未宣称达到 | `acceptance.md` D 定义人工原子义务ground truth、总体召回≥95%、否决项100%、匹配和逐项漏检口径。该测试集目标不是未知新文件保证；人工标注/真实LLM验收仍未完成。 |
| 6159352822 进度事实；6159516770领域入口 | 已同步 | 文档与独立CLI/正式TenderNodes对应；mock/基础设施、真实LLM、未来Rust三层分开；历史证据保留。 |
| 6159745297 第1项，模型降级问题 | 已修复 | `tender/schema.py` 的宿主等级策略用于extract/verify/aggregate；漏项、适用性等与未知问题强制阻断，仅明确格式信息可非阻断。恶意/错误blocking:false有回归。 |
| 6159745297 第2项，计划仅比数字 | 已修复 | `planning/constraints.py` 在内存读取完整约束，`plan.py` 核验对象、条件、例外、完整单位、工作/自然日、上下限；持久计划只保留原要求引用/摘要和具体动作材料，正确摘要不能掩盖错误action。自由文本完整语义仍属真实验收边界。 |
| 6159745297 第3项，累计条件 | 已修复丢失路径；真实未决保留 | `evidence/context.py` 与`packing.py` 按section父链累计完整条件、支持片段来源/范围绑定；“仅第一标段”和“联合体还需”同时保留。无法证明作用域/完整句时保持pending与阻断，不能模型或人工confirm强制放行。 |
| [6159969354](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/pull/24#issuecomment-6159969354) 固定字符分包 | 已按先方案后实施改写，统一验收见下文 | `evidence/planner.py` 完整相邻原生章节优先，只有实际请求失败才按结构/行/单格/文字拆；删除6000/12000字符阈值。规划和client共用`RequestBudget`；所有必需支持参加同一次probe，普通与纠错均C=131072。 |
| [6159955029](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/pull/24#issuecomment-6159955029) 真实文件差异 | 旧计数保留为诊断证据 | 新增98实际图片载体解释422→520；表格行展开解释520→796范围，不等于多解析出义务。两份图经视觉/位置审议为品牌装饰，显式审议绑定原件/像素/全部出现位置；禁止仅按重复次数忽略图片。44项scope告警不一律视为文档歧义。 |
| 用户追加存储/传输原则 | 已实现，增长证据单列 | canonical native raw、source index与pack selector；repo每来源缓存一次；验证引用要求索引，plan聚合仅引用子产物；模型完整响应单次产物和response_ref；固定阶段checkpoint，无逐包历史映射。 |
| [6160192506](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/pull/24#issuecomment-6160192506) 三个结构边界 | 已实现，真实原件的具体决议尚未提交 | 共享来源与域修订`pack-plan`拆分，task/input_digest/budget_digest/plan_digest绑定计划；`applicability_decisions`生成有效要求视图；`review-scope`保存有证据的具体bind/local_only决议、新修订重分包重验。8项真实PG确定性流程包含scope恢复至ready及过期拒绝。 |
| 用户追加唯一字段命名 | 已统一 | `tender/schema.py`唯一共享定义；document_version/source_version/domain_revision分开，unit_id从外部parser边界一次映射，artifact_ref及六阶段引用名称一致；删旧schema与内部version/ref别名，不做未发布格式兼容。跨阶段序列化与旧别名拒绝有回归。 |

## 追加发现

- OCR回调/缓存读取的存储损坏曾被宽泛`ValueError`吞为文档问题；已仅捕获明确语义响应错误，
  真实文件/回执完整性错误传播为技术失败，两个路径有回归。
- 真实 PDF 有98个实际图片位置，但仅2份不同内容。literal OCR现在以内容复用；每个
  来源位置、锚点仍保留。重复图片与重启恢复测试不把位置去重成丢失来源。
- 首轮真实预检发现长篇不完整条件反复作为supporting，制造1个字符预算阻断；改为
  `pending_scope_context`定位引用，完整原文保留于body/raw。未解决范围问题依旧阻断，
  这不是把文档问题标成功，也不是增加局部整页渲染。

## 实际验证

本轮统一验证及最新包装结果由末尾事实更新；早期完整Python回归为
**465 passed / 15 skipped（83.78s）**，15个skip是原有可选数据/依赖测试。
随后同图片OCR复用的端到端测试 **7 passed（19.55s）**。
原样保留旧 `sample-*.json` 与 `image-verification.json`：它们明确对应以前代码/合成样本，
不是本轮真实 PDF 的模型输出，也不冒充最新镜像。

## 真实 PDF 的当前界线

Library 原件重新取得并核验：755602字节、106页，SHA256
`4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`。
原件、解析正文、图片和冻结包均留在私有验收目录，未提交GitHub；
[最新脱敏结构预检](evidence/real-pdf-structure-preflight.json)仅含统计、摘要和问题类别；
[旧固定字符预检](evidence/real-pdf-preflight.json)保留为历史诊断。

当前已具备输入、模型凭证与普通C=131072。CHAT/VLM目标为
`https://cpamc.818996.xyz/v1/chat/completions`，model=`gpt-6-luna`。
本轮用户明确不开展付费模型验收；尚无OCR/真实抽取/独立验证的模型输出或真实usage。
用量估计只作付费前范围说明，单请求窗口与累计预算分开；不设置输出token硬上限。

真实原件当前13项未决是结构/引用/优先级绑定待审工作，不能一概称为真实文档歧义。
`review-scope`入口已可提交有来源证据的bind/local_only决议，来源保持不变、产生新domain_revision，
重新分包、抽取、独立验证和人工确认；不能把confirm当解除未决的开关。
本轮未替用户对这13项作出人工决议；人工标注集和真实模型语义验收仍待开展。

## 请求成本追加审查（修复前基线）

结构/字段/持久修订这一批代码统一回归为 **619 passed / 15 skipped（124.20s）**，
显式使用真实Redis/PostgreSQL，新增服务测试未跳过。Ruff通过；wheel内46个运行模块/提示
与源码逐字节相同，旧requirements/schema模块已移除。此结果不代表发送成本已优化。

用户随后要求逐项审查最终wire请求成本，真实模型继续暂停。首轮定位：42表318行，
80行本行anchor无文字，其中4行由非空纵向合并覆盖；76个真正全空行在extract均已省略。
verify却重新发送这些行以及98个显式审议装饰图载体，阶段视图不一致，此问题已在后续统一SourceView中修复。
空白填写模板也需一次保留结构/坐标摘要，不能把空行自动等同无业务语义。
13包只证明当前保守预算准入，不能据此宣称分包合理；先记录逐包wire成分、元数据、估算保守程度和合法合包利用率，再完成修复。

最新4包的实际SDK成本、原文/结构不变证明、空白模板处置、流式与防重复付费结果见
[请求成本与流式修复证据](request-review.md)。最终统一测试和镜像以acceptance.md追加结果为准。
