# Python 第一阶段唯一字段字典

本字典先于本轮命名统一实现提交。实现的唯一共享定义为 `tender/schema.py`；原生解析器
`StructuredSourceUnit.key` 等外部协议由新证据适配层一次映射，原 parser、gRPC 与 Rust 不变。
本路径尚未发布，不保留旧 `version`/`ref` 等内部别名，不增加历史 checkpoint 迁移层。

| 字段 | 类型与含义 | 身份与生命周期 |
| --- | --- | --- |
| `run_id` | string，独立工作流身份 | 提交后稳定；全部阶段/命令/产物统一 |
| `document_id` | string，文档集内一份主文件/附件/补遗身份 | 不用文件名或出现次序代替 |
| `document_version` | string，单文件的提交版本标签 | 登记、冻结、范围、引用始终同名；不叫`version` |
| `source_version` | positive int，run的整个文档集版本 | 原文件/清单变化才增加；DB、Lease、队列、checkpoint、确认统一 |
| `domain_revision` | positive int，同来源的要求/作用域审议修订 | 纠错增加、撤销旧确认；共享规范来源回执使用0，不能机械并入source_version |
| `schema_version` | positive int，产物格式版本 | 与业务版本不同，无旧格式兼容框架 |
| `unit_id` | string，规范原生载体身份 | 原parser的key只在适配边界映射一次；后续不改名 |
| `section_id`, `parent_section_id` | string或null，可靠章节身份/父身份 | 同名标题不等于同一章节；无标题允许null，不以heading_path当身份 |
| `heading_path` | string，展示用标题路径 | 不做唯一键；原生单位/共享章节索引持有 |
| `range_id` | string，某个可引用连续来源区间的身份 | 原始范围、合法fragment和support excerpt各有确定身份；证据/处置/验证一致引用 |
| `source_range_id` | string，区间所属未拆分规范范围 | 与range_id不同概念；重分包不改变它 |
| `parent_range_id` | string或缺省，派生区间的直接来源 | 与source_range_id/fragment lineage区别记录，不另造别名 |
| `start`, `end` | int，来源文本Unicode字符半开区间 | 与row_start/row_end的原生行区间不同；不当token位置 |
| `artifact_ref` | ArtifactRef，sha256/relative_path/size/media_type | 所有通用产物索引项统一此名，不再叫ref |
| `original_ref`, `raw_ref`, `source_index_ref` | ArtifactRef，原件、规范原生证据、定位索引 | 三种不同内容角色；全阶段保留同名，不复制正文 |
| `image_key` | string，原生图片载体在图片索引中的标识 | 原parser locator.original_ref字符串只在适配边界映射；不能与原件ArtifactRef同名混用 |
| `registered_ref`, `frozen_ref`, `pack_plan_ref`, `requirements_ref`, `verification_ref`, `response_plan_ref` | ArtifactRef，固定图阶段输出 | checkpoint仅保存这些固定引用，不逐包复制映射 |
| `pack_id` | string，计划内正文选择身份 | 不用它单独代表预算/输入相同；必须结合plan_digest |
| `task` | extract/verify/plan/ocr | 请求/计划所服务的具体领域动作 |
| `input_digest` | sha256，直接上游引用及相关操作配置的摘要 | 相同输入重试幂等；改变输入不得复用旧回执 |
| `budget_digest` | sha256，实际请求构造与容量规范摘要 | 包含model/C/prompt/reserves；不含秘密、不等于usage |
| `context_tokens` | positive int或null，配置的provider总上下文上限（输入+输出+安全余量） | 与输入预算分开；旧131072配置不自动升级为更大服务窗口 |
| `input_budget_tokens`, `correction_input_budget_tokens` | positive int，普通输入131072、显式纠错最多262144 token | 共享预算合同；普通verify/plan和新domain_revision不自动使用纠错额度 |
| `correction` | bool，明确标记一次修复请求可使用纠错输入额度 | 不改变provider总窗口或wire输出参数；未知总窗口不授权双倍 |
| `effective_input_budget_tokens` | int或null，当前输入预算与总窗口扣预留后的较小值 | planner/client使用同一测量；UTF8保守估算不是provider实测token |
| `request_digest` | SHA256，完整无鉴权模型请求的摘要 | 模型审计与发送重放身份；包括图片、消息、模型和协议参数，不是仅payload文本hash；同运行修订/task下sent或uncertain阻止自动重付费，完整成功响应可复用 |
| `plan_digest` | sha256，task+input_digest+budget_digest+selectors/决议的规范摘要 | 新预算或作用域绑定生成新计划，规范来源身份不变 |
| `requirement_id` | string，当前要求清单内义务身份 | 验证决议、计划项引用同一身份；内容修正产生新domain_revision |
| `evidence` | Evidence[]，range_id+原文quote | 不更名为另一种引用；quote必须能在规范范围重放 |
| `requirement_ref` | artifact_ref+requirement_id | 响应项引用已有不可变要求，完整约束只在内存校验 |
| `requirement_refs` | RequirementRef[]，artifact_ref+requirement_id | 后阶段从一个或多个原抽取产物选择完整要求；与响应项requirement_ref同一schema |
| `disposition_refs` | DispositionRef[]，artifact_ref+range_id | 后阶段完整逐范围处置引用；汇总核对完整覆盖，不复制处置正文 |
| `conditions`, `exceptions` | string[]，要求的适用条件/例外语义 | 与来源范围关系scope_binding不同，不机械合并 |
| `scope_binding` | 来源条件到具体range_id集合的关系 | 明确bound/unresolved；祖先正文不自动成为条件 |
| `scope_bindings` | `ScopeBindingRef[]`，每包完整条件的来源范围、字符区间及`scope_binding` | 无text；条件已在body中时仍传递明确目标，正文与supporting区间并集只发送一次 |
| `sections` | 请求内`{section_id,parent_section_id?,heading_path}[]` | 章节语义共享一次；每来源只引用section_id，宿主保留完整物理坐标 |
| `source_aliases` | SourceAlias[]，range_id+segments[{range_id,start,end}] | 跨旧包合并后原证据ID映射至本请求已发送的精确Unicode区间；无text，不更改原要求 |
| `source_context` | SourceView，响应计划显式范围依赖的必要来源视图 | 与extract/verify共享来源schema，不另建同义字段 |
| `blank_table_templates` | 请求内空白填写结构摘要数组 | 每项table_id/column_count、rows区间及原range_ids，可含merges/table_ids；不代表无要求，仍需逐原range处置和独立验证 |
| `column_count` | positive int，表格原矩形宽度 | 保护空白字段及模板列数，不用非空列数替代 |
| `host_reviewed_range_ids` | 仅宿主已证实空白页/显式审议装饰图的range_id[] | 与模型`reviewed_range_ids`分开，模板空行不进入此列表；不伪称模型读过 |
| `scope_decisions` | 人工bind/local_only决议记录列表 | 每项带range_id、证据、理由、明确目标；进入新领域修订后重分包/重验 |
| `applicability_decisions` | requirement_id键控小型适用性决议 | 验证产物保存；计划读取原要求+当前决议有效视图，不复制完整要求 |
| `checklist_digest` | sha256，source_version/domain_revision/来源/计划/要求/验证摘要 | 确认精确绑定，任何相关修订使旧确认失效 |
| `status` | 各共享实体schema内明确枚举 | run、command、OCR、scope、disposition是不同实体状态，不相互冒用 |

## PR评论6160192506的三个边界

1. **PackPlan**：规范冻结来源在同source_version内复用，分包计划属于domain_revision，含task、
   input_digest、budget_digest及plan_digest。重试复用同计划；作用域/输入/预算变化须新修订/新计划，
   不碰原件、不复用旧extract/verify回执。verify/plan按完整实际输入生成阶段计划，必要时按语义
   和依赖重分；仅不可分单元超限才预算阻断，预算拒绝与模型语义失败分开。
2. **有效要求视图**：`applicability_decisions`按requirement_id保存具体决议；计划只处理原要求与
   当前决议合成的有效视图。实际文本、数字、条件纠正依旧通过领域修订，不能伪装成验证结论。
3. **有限作用域审议**：新增独立`review-scope`入口；检查当前来源和计划摘要、具体问题range_id、
   有效原文证据与目标范围。人工可确认作用域或证明是局部条款/误报，但不能仅删除问题。
   决议持久在新领域修订，来源保持不变；图重建相关支持和计划、重新抽取/独立验证，再允许确认。
   重复提交幂等、过期/无证据/不存在目标拒绝，旧确认和旧执行者失效。它不是通用运行时。

测试包括跨阶段JSON往返、禁止内部旧别名、三类版本区别、稳定unit/range身份、不同预算/决议
新plan_digest、幂等重试、适用性覆盖真正影响计划，以及scope审议后重启重验和过期拒绝。
真实13项原件范围问题在有依据决议并重验之前仍未完成；不把schema统一宣称真实模型验收。

PackPlan为当前领域修订的总计划（引用多个PackDescriptor）；PackDescriptor保存单包正文/
支持selector和planning_details（planner_version、boundary_reason、section_ids、budget）。
`submitted_source_version`/`submitted_domain_revision`仅表示幂等提交回执所指的历史提交，
可能不同于run当前身份，不是当前身份的别名。`review-scope`本期仅能绑定同文档规范目标；
跨文档条件引用保持明确未决，不能凭相邻上传顺序自动合包。补遗适用性是另外的验证决议。

后阶段使用task=verify/plan阶段PackPlan；verify使用PackDescriptor，plan使用
继承同一来源/引用合同的ResponsePackDescriptor，仅保存来源、完整义务选择及预算。对应verify/plan结果中的pack_plan_ref
指向后阶段计划，图原pack_plan_ref仍指extract计划，不扩大checkpoint字段数。
完整输入、跨extract包依赖、修订/部分成功恢复及不可分预算已通过确定性workflow回归；
真实语义验收剩余项见[当前交接](handoff.md)。

已去掉WIP子包的requirements_ref/requirement_ids/disposition_range_ids组合，统一上表
RequirementRef/DispositionRef；requirements_ref固定指阶段根的完整要求索引。内部合同未发布，不增加旧格式迁移。
