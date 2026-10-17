# Python 招标第一期交接

问题台账统一见[问题与修复记录](bug-fix.md)：每次发现问题追加或更新根因、方案和验证状态；
本文保留运行时间线及证据位置，字段定义仍以[字段字典](field-dictionary.md)为准。

## 当前状态（2026-10-17）

真实原件 `BiddingFile.pdf` SHA `4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`。
运行 `bdcf6dac-73b2-4ef5-bb3a-a34019df82f4` 已提交并在跑：106页已解析，分包计划已写出，模型调用已返回。
首次执行在 freeze 因未定类型的空 metadata 更新失败，修复提交 `92a1bc1` 后用命令 `30de0a63-5048-4eb9-98bc-092a0e2e8921` 续跑。
产物在 `/opt/workspace/kb-tender-evidence/2026-10-17-full-run/artifacts`。窗口 131072，并发 2。
完整五节点尚未结束。

## 此前状态（2026-10-17）

用户已清理此前的Python招标运行栈、数据卷及`/tmp`证据；下文各run只剩文档记录，不能续跑。
当前修改方案及四项已确认决策见[全量复审](bug-fix.md#全量根因复审与统一优化方案)。
P0（`e85cd2f`）与P1（`76f09d3`）已在`pr-24`本地提交、未推送、未部署。
P2源码已实现：模型输入去掉版本号、工作单元按内容稳定、discovery只提冲突/修改/条件依赖候选、
同source_version成功回执跨修订复用；D2（无决议不进joint）原已落地。单元927项、临时PG/Redis上
workflow/acceptance 118项通过，Ruff通过。
P3源码已实现：抽取模型只声明 non_item/unresolved，有事项的范围由宿主按证据派生。
默认窗口 131072，显式纠错最多 262144，抽取输出预留仍为 16384。
G3在预算观察里逐请求标记估算低于实际 prompt、输出超过预留、首包等待达到 60 秒。
并发仍为 2。
未提交、未重建镜像或部署，也未重跑隔离 workflow/acceptance。G2 及完整文件运行需另行确认。
证据目录为`/opt/workspace/kb-tender-evidence/`。

## 发现请求去冗余（2026-10-16，源码完成，未部署）

BUG-025最小修复：多个容量块仅发送全部两块并集，不再另发已被覆盖的基础块；
零/单块保持原处理。只改调度，没有删引文、改预算或引入候选合批及新字段。
新增回归先复现3失败，修复后4个相关测试文件共98项通过，Ruff及文档检查通过。

复用下述真实run的383事项离线对照，旧计划重建一致：discovery 21→15，reference保持12，
定位合计33→27；累计输入估算2672459→2335222（约降12.6%，非实际Token节省）。
全部73153对事项覆盖，保留请求除work_unit_id外的内容、问题路由和本地候选不变。
禁网、产物只读，无新付费调用；报告`/tmp/kb-discovery-dedup/comparison.json`。
真实语义召回及完整流程仍待验证。运行镜像未替换，不把改变编号和摘要的新计划套入旧运行。

用户随后授权真实测试，已启动独立discovery对照run `d26f0b15-4a60-4e30-ac62-b149c75bb4a2`，
同一原件及上述383事项，计划15个真实请求，单并发；使用既有Chat配置、独立测试schema、
正式Store/模型客户端/租约/回执及三次重试机制。固定源码快照SHA（analysis.py）
`af3a9b9520f26e9c1120f5772602078ce99436804a5eca730988b539da4535a5`；原生产产物卷只读，
测试产物及状态在`/tmp/kb-discovery-real/`。这是发现阶段真实回归，不是另一次五节点完成。
启动前旧run快照08:34 UTC：locate33/33完成，associate8/259、6个关联计划包超预算，
有1个调用在途；旧运行未取消或切换源码。本测试与旧verify串行调用合计最多2个在途。

真实对照已完成：15/15首次成功，无纠正、拒收或uncertain，供应商报告输入412301、输出55850、
合计468151 Token，调用时间跨度约19.1分钟；旧21次对应502278/71250/573528 Token。
本阶段实际输入下降17.91%、总Token下降18.37%；候选163→124、findings97→59。
13个业务线索抽查仍存在，包括★否决与加分、付款与否决、财务主体/年份、质保、暂列数量，
并核对了服务费“一个月内/5日内”的原引文；这不是完整召回评测或人工确认。
测试run的blocked是组件测试结束标记，容器退出0；没有执行confirm或宣称analysis-ready。

下游离线对照复用新版15份发现回包及旧版12份补读回包（补读未重新付费）：旧关联计划精确
复现259次/9752104输入估算/6包超限；新版为220次/9461679输入估算/9包超限。
调用数下降15.1%，输入估算仅降约3%；联合来源范围仍大，超预算及逐候选调用尚未解决。
新旧模型候选划分存在波动，不能把超限增加全部归因于取消基础块；不能把候选减少当召回证明。
该220次仅为离线预测，没有付费执行或接入旧run。报告见`/tmp/kb-discovery-real/`
下的`report.json`、`semantic-observations.json`、`downstream-report.json`；旧运行回执未改动。
08:57 UTC旧完整任务仍运行，associate54/259，发生过1次HTTP500后自动重试，
仍未进入joint/confirm；累计已报告1751235 Token。发现阶段验证通过不等于五节点完成。

## A/B版真实文件完整运行（2026-10-16 07:25 UTC）

用户要求使用真实文件完整运行。已在无在途请求及有效租约时，将三个Python角色切到已验收
候选镜像`sha256:6646b63df0f48ee4f2331f8e04288aaecf27581536ffc40aa4f62a1369ee12fd`。
52个源码/Prompt指纹一致，normalization_version=3；独立Chat/VLM沿用原配置，模型及分包
并发均为2。共享PG/Redis就绪，原网络overlay保留，无新增端口；未重启其他服务。
本次Compose额外镜像选择文件：`/tmp/kb-ab-full-real/candidate.yml`，与原网络overlay一起使用。

正式Store.submit新建run `addaa817-3a73-4f92-8445-eea99a9384c7`，来源/领域修订均为1，
request_key=`real-v2-ab-full-20261016-0725`。同一106页原件SHA
`4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`；完整原件已重新
登记、解析、OCR、冻结及规划，未挪用旧抽取回执。执行快照与候选配置一致。
旧run状态、来源版本、修订、manifest、产物、请求、命令及历史保留检查通过。

07:44 UTC：登记/冻结及2次OCR完成，4/4抽取接受，383事项/796处置（义务174、
提交101、后果89、评分19），累计已报告194902 Token，命令首次执行、失败0。
原文review新规划12包，累计输入估算834780，全部准入，首包在途；后续定位/关联/联合
核验尚未规划，不能沿用旧387包或组件验证结果。事项及处置可读导出在`extraction-export/`。
本地导出脚本曾依赖旧运行dropped.json，已改为当前产物的issues和diagnostics；与模型
运行无关，没有重发请求或修改已保存台账。
07:57 UTC：全源review12/12完成，累计已报告451420 Token；locate新规划33次，
其中21 discovery+12 reference，累计输入估算2672459，全部准入。只读重建与已存计划
逐项一致：156条问题记录组织成42组reference、52组local、1组revision；续报只共享任务，
原记录保留。本地52候选和后续discovery候选尚需统一关联，33不是剩余总调用数。
路由证据：`routing-plan.json`、`review-routing-observation.json`。

阶段预算、真实用量、问题及最终结果持续记录在`/tmp/kb-ab-full-real/live/`，只读脚本
`/tmp/kb-ab-full-real/poll.py`。部署及提交证据也保存在该目录根部。

按用户随后要求，已清理空闲`kb_python_tender_preflight_unit`和两个已结束的
`kb_python_tender_ab_validation`、`kb_python_tender_ab_prompt_validation`容器。
116份测试log/XML/JSON及容器日志归档于`/tmp/kb-ab-full-real/history-cleanup/`。
没有删除镜像、卷或真实回执；当前worker/dispatcher、PG/Redis及其他正在运行的测试项目保留。

## A/B实施与验证（2026-10-16）

A/B已在源码实现：有效事项核心局部保留，异常可选字段原值送独立核验；以dispositions
界定漏项检查目标；Issue续报共享处理任务，needs_source决定一次补读或本地核对；
Decision只解决明确列出的记录。预算报告按review/discovery/reference/associate/joint
分别观测，并显示尚未规划的下游成本未知。没有新增主节点、运行状态、业务正则或环境变量。
normalization_version=3，短引用与budget builder为v2；未实施C的引文延后及独立候选合批。

**回归与预算。** 定向失败回归先复现丢事项、路由和报告缺项，再实现修复。第一轮986项
通过；首次全套13失败/1237通过/15跳过，定位为提示过长及两项过期的重复拒收断言。
压缩后全套1250通过/15跳过（524.32秒）。真实反例促使补充提示，11418字节又使8项
32K压力用例失败；最终压缩为10658字节，43项定向回归通过，未放宽预算或删减必要原文。
最后补充关联投影缺陷的回归，2项先失败；最终源码全套 **1253 passed / 15 skipped**
（530.82秒），跳过项均缺少样本。使用隔离测试数据库/Redis和固定模型，Ruff及diff通过。
文档118个本地链接/锚点、4段JSON和26项问题索引及根因覆盖检查通过。
新增4份review语义草案后共32份；合同/导出验证通过，人工标注及整套语义验收未完成。

**原件回放。** 同一106页原件4份已付费回包：原接受315条→新代码保留319条核心，
39条关联、796条处置；4份异常条件原值均进入独立核验输入，11个新review请求准入。
原包边界和原回包未改写，回放不是新抽取或完整语义验收。旧样本extract输出最大36865，
4份中3份超16384预留；不凭样本比值更换tokenizer或扩包。报告仍为observations_only。
证据：`/tmp/kb-plan-implementation/{replay-report,budget-observation}.json`。

**真实定向验证。** 使用独立候选镜像、现有Chat配置和测试数据库独立schema，复用正式
OpenAIModel、Store、call_validated及CAS审计，每次最多1个调用；没有直接HTTP绕过围栏。
首轮run `569a3828-d68e-495e-bdc5-e4cf04ce2a21` 的6份调用均成功返回，但合成上下文反例
出现漏项误报；该失败证据保留。第二轮增加明确双向任务说明并压缩提示，完成11份真实调用：
2份原件review、4份合成边界/路由案例、3份未使用提示示例文字的反例、2份真实关联输入。
全部11份传输及宿主合同接受，0拒收/0uncertain，报告prompt72835、completion10046、
total82881 Token，无未知usage。上下文不再误报漏项；真实目标遗漏、例外扩大、附件未提供及
上下文与事项矛盾均按语义被识别，评分条款及额外body反例也符合预期。两份关联分别返回
1条关系/0问题、5条关系/3问题，保留邮寄要求冲突等疑点；未运行对应joint，不能称业务通过。
第二轮run `ec8786f4-9e61-4940-b862-837345d1473a`，证据目录
`/tmp/kb-plan-ab-prompt-validation/`；原始请求、回包、用量和语义观察分别保留。
组件运行已保留为blocked并注明component_only；这不表示真实工作流已完成或业务阻断
已裁决，没有调用confirm，也不计作analysis-ready或人工验收。

**额外定位的真实根因。** 07:06 UTC只读快照，旧run
`bb5fb879-daa0-462f-82ef-79c45286db51` 已failed：315事项/796处置、review12/12、locate49/49，
associate51/387；计划输入估算15284782、6包超预算，joint未规划。累计已报告2194385 Token。
命令执行4次：1次HTTP500与3次finding身份拒收；associate-2、9的反馈纠正曾成功，后续包
再次拒收而耗尽共享重试。3份拒收都涉及同一联合体事项：输入含其disposition却没有item，
模型指出缺对象后返回该ID，宿主又判越界。修复associate/joint仅发送选入范围且随包items
闭合的disposition，额外原文仍保留；未扩大接收身份、不删除finding或全局处置。
两份原件输入对照原文与items逐项相等，处置由5→3及15→14，均去除1个悬空事项引用。
旧运行和全部回执保留，未提交retry、未修改旧协议或人工确认。

候选镜像`knowledgebrain-docreader:plan-ab`：
`sha256:6646b63df0f48ee4f2331f8e04288aaecf27581536ffc40aa4f62a1369ee12fd`。
尚未替换生产worker/dispatcher的local镜像。新版同文件完整五节点、人工语义核对、
预算容量校准及C阶段仍未完成；组件反例符合预期不代表这些工作完成。

## 历史：全量问题复审与修复规划（2026-10-16，设计阶段）

方案进一步收敛：续报复用不可变Issue记录，只共享处理任务，不新增Observation体系；
Decision仅解决明确列出的记录。非阻断提示无需额外调用，本地候选与定位候选统一组织后
再执行。预算增加校准实施顺序及未知下游计划的报告边界；A可独立交付，B整体接通后交付。
字段字典、工作流和验收已同步，仍仅为文档设计，未修改源码、部署或触发真实模型调用。

第二轮静态审查补齐四处实施边界：false跳过定位后的本地处理入口、续报阻断性及决议快照、
异常可选字段原值实际送审、输出余量校准前不扩包。详见问题台账“第二轮方案审查”；
字典与专项验收同步。本次只改方案，尚无新代码、部署或真实模型验证结果。

已逐条复审BUG-001至026，补齐索引、六类共同根因、五种处理边界和三个交付批次；
将历史测试/运行记录与当前状态分开。参考LangGraph官方的workflow/agent及持久执行说明、
Pydantic AI输出校验反馈机制，复用现有框架、回执与付费保护，不增加通用Agent运行时。

首批修复可选字段拖掉事项与局部核验误报；第二批在现有回包内表达问题续报和原文需求，
由宿主组织一次补读，并行校准预算；第三批验证后再决定引文延后和候选合批。
字段字典的下一修订章节新增两个可选字段设计，尚未改schema/Prompt或部署。
完整原文核验、跨组发现基线和显式修订/人工确认保留；不以缩减调用直接宣称语义不退化。

离线容量对照：315事项完整发送估算266919；延后引文、其他字段不变为184468；仅保留
完整正文和引用仍为151043，均超过110592。延后引文并保留跨组覆盖可从21发现包降至10包，
输入估算1787169→810143；只是实验，不是正式新合同或语义验收。前6包的37候选离线生成
37个associate请求，当前joint整包接受条件使直接合批存在相互阻断风险，故列为后续优化。
证据：`/tmp/kb-shortrefs-real/proposal-review.json`；本轮方案审查未新增模型调用。
文档检查：6份文件的114个本地链接/锚点、4段JSON示例、26个问题编号与六类根因覆盖检查
通过，git diff --check通过。本轮仅更新文档，没有运行或声称通过新的代码回归。

## 短引用修复版真实重跑（2026-10-16 05:08 UTC）

用户明确选择取消旧任务并立即测试新版。正式Store.cancel已将
`6f0a29a2-8f45-4725-bebb-70485d8a75ba`取消，已完成产物精确保留；当时在途请求
`aea3c80c589949519c72523f0a3226a3`记为uncertain，不自动重发。

无在途请求/有效租约检查通过后，修复版镜像
`sha256:98f3e76af8a8910481e0d141209f897c947acfe1acd23aa122f0404864b98856`
已部署：worker/dispatcher运行、init退出0，52个相关源码/Prompt指纹一致；
normalization_version=2、短引用budget builder、Chat/VLM原配置、并发2、网络overlay、
PG/Redis和产物卷均核对通过，无新增端口。

同一106页原件通过正式submit创建独立run
`bb5fb879-daa0-462f-82ef-79c45286db51`（source_version=1/domain_revision=1），
请求键`real-v2-shortrefs-recovery-20261016-0508`。原件SHA/manifest不变，重新付费，
未复用旧运行回执冒充新协议结果。05:08快照：登记及2次OCR完成，运行中。
本轮观察实际短引用映射/请求摘要、宿主接受、纠正请求、用量与执行路径；不代替人工确认。
06:02 UTC快照：抽取4/4包接受，315事项、796处置（items431/non_item343/unresolved22）；
事项为义务118、评分22、后果84、提交91。模型原始319条中4条使用合并键
conditions/exceptions，保留为未决问题及原声明，未静默删除（BUG-023）；恢复8处源换行。
独立原文核验已接受12/12包，定位20/49包；review累计输入估算846749、locate3972953，
全部计划输入准入。05:49 UTC discovery-13明确HTTP500后自动恢复，该包已接受，命令第2次
执行、失败计数1，未重做成功兄弟；无model-rejection/合同反馈纠正。供应商内部500原因未知。
已报告983207 Token；已完成抽取最大输出36865，超过16384预留但实际总上下文未超限。
review-2发现一例上下文事项被误报漏项（BUG-024），正在观察后续定位处理，未热改协议。
旧运行部署后11项保留检查通过，未新增旧请求。关联、联合核验、确认尚未开始。
用户要求查看的全量事项/处置/原文证据已导出到`/tmp/kb-shortrefs-real/extraction-export/`。
证据：`/tmp/kb-shortrefs-real/`；运行尚未完成。

## 合同拒收纠正、短引用及宿主核验覆盖（前轮源码验证记录）

本轮按用户认可方案先更新字段字典，再实现BUG-010/022；保持五节点和业务范围不变。
`llm/recovery.py`使用现有产物回执保存具体拒收路径与调用/响应引用，下一次执行发送
真实错误反馈及相关片段。传输succeeded不改写，一次执行最多调用一次，沿用首次加3次
重试；重复已拒收的纠正内容明确停止。原回包、成功兄弟和sent/uncertain围栏保持有效。

`llm/wire.py`统一请求内短引用，原文/引文/理由不替换，父定位及源偏移留在宿主；
保留alias片段区间、单元格、章节、角色和作用域。`model-wire`保存原payload和映射，
单个request_ref防止不同工作包被压缩为相同付费摘要。预算试算和实际发送共用转换器。
review模型只返回schema_version/accepted_decision_ids/findings；宿主从实际请求派生
reviewed_*接受回执。模型附带旧名单时忽略并记诊断，不据此拒收或证明语义完整。
语义样本导出同步使用短引用，将还原映射保存在review目录；不改变草案标注的验收状态。

协议以normalization_version=2及chat_json_object_stream_short_refs_v1冻结。
当前真实run继续用已部署旧协议，不热替换、不把旧产物冒充新协议验收。

验证：新增恢复测试先因缺实现失败；广覆盖493项单元回归通过，最终名单容错及停止策略
177项定向复验通过（与493项有重叠，不相加）；最终85项隔离PG/Redis工作流及分析验收
全部通过，包括完整SSE成功→宿主拒收→反馈纠正→重启缓存复用、共享4次执行上限和
五节点确认。Ruff/diff通过；未重跑全部Python套件，没有真实付费纠正验收。

真实106页原件、330事项的禁网核对：相同选定payload及当前Prompt比较，4包抽取Wire
405792→370828字节（8.62%）；13包新规划review为1046599→920310字节（12.07%）。
全文/引文/单元格/作用域/引用往返逐项一致，全部准入。这是字节测量，不是Token降幅，
13包是新发送协议离线规划，不能替代当前运行的19包回执。
证据：`/tmp/kb-protocol-check/report.json`、`/tmp/kb-protocol-tests-final2.log`、
`/tmp/kb-protocol-last.log`；隔离容器内`/tmp/kb-v2-test-results/contract-release.xml`。

2026-10-16 05:02 UTC只读快照：当前真实run `6f0a29a2-8f45-4725-bebb-70485d8a75ba`
抽取4/4（330事项、796处置）、原文review19/19接受；locate6/58接受，第7包在途。
真实locate计划累计输入估算4933763（非实际Token），全部单次准入；不是旧产物40包测算。
累计已报告749139 Token，命令首次执行、失败0。关联、联合核验、确认及analysis-ready未完成。

## 新版本真实文件重新运行与评测迁移（2026-10-16）

用户明确选择“用新版本重新跑同一文件，保留旧运行及全部回执（会重新付费）”。
已通过正式Store.submit创建独立run `6f0a29a2-8f45-4725-bebb-70485d8a75ba`，
source_version=1/domain_revision=1，请求键`real-v2-locate-compressed-20261016-0400`。
同一106页原件SHA `4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`，
manifest与旧运行完全相同；旧run及状态/版本/来源/修订/产物/请求/命令/历史10项保留检查通过。
没有把旧成功产物标为新协议结果，没有重发或改写旧uncertain。

首次提交被自动审批要求核实具体外部端点授权；只读比较.env、运行容器、旧执行快照与
原件SHA一致后，原样重申请同一提交动作获准。当前Chat/VLM仍使用已配置的
`gpt-5.6-sol`及`https://api.kunkun-gpt.com:8317/v1/chat/completions`，并发2，
网络overlay与03:52部署的定位压缩镜像不变。新run冻结execution_spec与提交时核对值一致。
新运行的事项数量、定位包数将以新模型结果重新规划，不能照搬旧319事项/40包离线结果。
2026-10-16 04:29 UTC快照：登记、冻结、2次OCR及全部4包抽取已完成，
330事项、796处置（135义务、91提交要求、84后果规则、20评分规则），run为running。
当前命令首次执行、失败0，尚未生成核验阶段计划；累计已报告231945 Token。
当前仍未完成五节点及人工语义验收，事项计数不代表语义正确或完整。

同时完成BUG-020：28个合成Verify输入迁移v2（13组正反+2个跨文档），表格明文cells、
独立文档角色、补遗原事项及retained_rules决议均使用当前合同，预期保持agent草案。
禁网导出28份输入，累计输入估算352308、最大13301，全部准入；标注与模型输入隔离。
仍无付费语义评测执行器，未运行合成样本的真实评测或人工裁定，也未据此宣称抽取完整。

BUG-021新增源码status的budget_observation，只读区分计划与实际Token、输出max/p95、
超预留次数、实际总量与上下文容量及未知usage。旧运行4次成功抽取均超16384输出预留，
最大输出36660，但最大实际total69842、未超131072；66份核验usage输出max8417/p957399，
最大total40042，另1份uncertain用量未知。该报告没有调整预留或增加阶段硬门限。
报告CLI改动未热更新正在执行的新运行；离线使用同一源码报告，不影响固定协议。

新增失败基线10 failed/1 passed；最终107项单元/模型预算回归及1项隔离PG status验收通过，
Ruff和diff通过。未重跑全部Python套件，未将结构验证替代语义验收。
私有证据：`/tmp/kb-semantic-v2/`；`evaluation-final/`为最终离线导出，
`old-budget-observation.json`为旧运行只读报告，`live/`保留新运行快照。

## 定位输入压缩修复已部署（2026-10-16 03:53 UTC）

BUG-014代码及Prompt已实现：discovery目录只携带本批事项证据所在的文档/章节、角色、
完整heading_path和range_ids映射；全部TenderItem及evidence.quote不变。reference保留
完整目录、问题原ID/detail和原文，按完整输入预算合批且初始每批最多8题。块间容量
同时预留双方事项和目录；不增加业务正则、工作流节点/状态或环境变量，并发仍为2。
无新事项的删除补遗由全源review产生未决目标，再走reference完整目录定位，已有回归覆盖。

使用同一106页文件的319事项、147问题和17份review产物禁网复算：旧持久计划精确重建；
新计划 **21 discovery + 19 reference = 40包**，原为202包。累计输入估算
**15287999 → 3535319（减少76.88%）**，单包最大109812/110592，全部准入。
全部事项/quote逐项相等、147问题各一次、50721事项对覆盖及原区间/正文/cell内容均核对。
这是新生产Prompt及planner的输入测量；新批量实际输出、语义完整性及费用未验证。
新测试失败基线6 failed / 1 passed；修复后336项定向回归、83项真实隔离PG/Redis工作流
及分析验收通过，0跳过/失败/错误，Ruff及diff通过；未重跑整个Python套件。
证据：`/tmp/kb-locate-fix-576syj9o/`；测试XML保存在测试容器`/tmp/locate-regression.xml`
及`/tmp/kb-v2-test-results/locate-fix.xml`。

旧run `eedcf1b0-40f4-42f2-b6c7-ef47ea723ec8` 已于03:39 UTC停止，03:48只读确认
status=failed。2次OCR、4包抽取（319事项/30关系/796处置）、17/17原文review及49/202
locate均保留；第50包请求`a892d57537f845338e974360da01ac01`等首数据180.175秒后
记为uncertain，原始字节/DONE/usage均未收到。BUG-019记录直接失败点与底层原因待查。
累计已报告prompt1884357 / completion465350 / total2349707 Token，超时调用用量未知。
当前无在途请求；完整定位、关联、联合核验、确认及人工语义验收仍未完成。

本次Prompt及计划摘要发生真实变化，旧修订冻结的execution_spec不被改写；不能给旧run
直接换新40包计划，也不能用correct/revise伪造业务变化。服务升级只供新的执行协议使用；
旧任务未重发、未清缓存、未新增来源/领域修订，旧成功回执保留。原协议恢复需先核实
uncertain的供应商执行结果，并使用匹配的旧镜像/Prompt；尚未实现跨执行协议迁移。
旧运行镜像：`sha256:78ef880f0672c6ad8ddbdc3844a748a688b911791fa57dbf9cff1316ecf0df32`。

03:52 UTC新镜像`sha256:4d3c59667e3ac306915fea7008ffd3e9b895ae41e906c459d78724c55e05df14`
部署核对通过：worker/dispatcher运行、init退出0、三角色同镜像，106源码/Prompt指纹一致；
PG/Redis连通、模型与抽取并发2、Chat/VLM配置保留、无新增宿主端口，原网络overlay保留。
03:53 UTC旧run的状态/版本、来源、修订、产物、请求、命令及历史10项精确保留检查通过，
没有新增模型请求，也未把旧49包回执标成新40包计划完成。真实输出验证与旧断点恢复未完成。

## 前次真实调用测试（2026-10-16 02:10 UTC）

02:10 UTC补充：locate 2/202 已接受，第3包在途；第2包为修复后新增的真实模型调用，
当前命令仍首次执行、失败0。累计已报告 prompt 489743 / completion 192437 /
total 682180 Token。以下02:08记录保留首包缓存恢复的检查事实。

同一 106 页文件、run `eedcf1b0-40f4-42f2-b6c7-ef47ea723ec8` 继续使用 `.env` 已配置模型。
4 个抽取包保留 319 事项、30 关系和 796 处置；**17/17 原文 review 已全部接受**。
这表示核验回执完整，不表示语义通过：review 共返回 89 条 findings，包含重复及待跨包
核对的问题。例如防火墙参数的局部“漏抽”提示在完整台账已有另一条事项对应，不能据此
认定全文缺失；抽查已核对实际抽取请求摘要及原文 cells，尚未代替最终联合核验。

实际 locate 计划为 **202 包**：55 个 discovery、147 个逐问题 reference，输入估算合计
15287999，单包最大 110271，均准入。只读重建与实际计划精确一致：目录重复 10053540
字节，占 payload 合计 13186795 字节约 76.2%；319 事项重复出现 3385 次。已更新 BUG-014，
本轮尚未优化请求组织；估算不是实际计费 Token，后续 associate/joint 容量尚未知。

首个 locate 完整回包因候选 `range_ids=[A,B,A]` 被拒收，随后首次加 3 次执行重试均复用
同一缓存失败；新增 BUG-018。最小修复只在宿主整理完全相同的候选来源引用并记录诊断，
不改模型语义、原回包、Prompt、请求摘要或计划。失败基线 2 failed / 6 passed；修复后相关
协议、缓存和流式 **358 passed / 0 skipped / 0 failed / 0 errors**，Ruff、diff 通过。
禁网、产物卷只读的真实回放保留 13 候选和 2 条 findings，202 包计划及请求摘要不变。

02:05 UTC 部署镜像 `sha256:78ef880f0672c6ad8ddbdc3844a748a688b911791fa57dbf9cff1316ecf0df32`，
三角色同镜像、init 退出0、106 源码/Prompt指纹一致、并发2、PG/Redis连通，原网络overlay保留。
首次正式恢复被自动审批要求核实文件和具体外部服务授权；只读核对原文件 SHA、当前环境与
冻结执行配置一致后，原样重申请同一正式 retry 获准，未绕过拒绝。

恢复命令 `8842cf8f-61eb-4238-919a-f98271e9ee51` 首次执行、失败0。02:08 UTC 首个 locate
已直接复用缓存接受，第2包在途，run 为 running。13 候选、2 条 findings 及去重诊断均
实查通过；原产物、请求、命令、来源及领域修订的10项保留检查通过，两个历史run快照不变。
累计已报告 prompt 465694 / completion 186839 / total 652533 Token；在途用量未返回。
完整定位、关联、联合核验、确认及人工语义验收未完成。BUG-010 通用合同恢复仍未实现。
本轮证据：`/tmp/kb-real-call-wfdivvxl/`；未提交或推送。

## 前次恢复记录（两类审阅身份修复已部署）

2026-10-16 01:46 UTC补充：review 6/17有回执，第7包在途；当前命令仍首次执行、失败0。
累计已报告prompt325844 / completion149740 / total475584 Token。以下01:44记录保留恢复时的检查事实。

2026-10-16 01:44 UTC实查：同一run `eedcf1b0-40f4-42f2-b6c7-ef47ea723ec8` 为running，
review 5/17有宿主回执，第6包在途，恢复命令 `075d3838-f090-4bda-9a26-c4e6892ff36e`
首次执行、失败0。BUG-009修复后第4包正常接受，第5包发现新的ID转写漏字：应收88个、
实回88个，1个缺失、1个未知、无重复；真实摘要及17包计划匹配，事项35/35、关系1/1匹配。
已新增BUG-017，宿主仅在单个未知/缺失身份可由全部已知来源唯一匹配单字符省略时恢复；
不改findings业务引用。未知但已在其他来源登记、歧义、替换和多字符差异仍拒收。

新增失败基线1失败/21通过；修复后相关306项全部通过，零跳过/失败/错误，Ruff及diff通过。
真实第5包禁网只读回放通过；部署后正式retry直接复用缓存，88范围、5条findings及1条
恢复诊断均实查通过，无第5包新增请求。原4包抽取、4包核验、请求审计及修订的10项保留
检查通过，两个历史run完整快照不变。截至该快照已报告prompt293259 / completion144121 /
total437380 Token；第6包用量待返回，完整流程及人工语义验收仍未完成。

01:42 UTC部署镜像 `sha256:ca52c23aeeff834aa0736b46eb9db933b2c8fb2b70f9cb3c2f8087b6f7fffc29`，
三角色同镜像、init退出0、106个源码/Prompt指纹一致、并发2、PG/Redis连通，网络overlay保留。
本轮生产变更仍仅在analysis.py身份处理，未改模型请求、Prompt、分包或生产预算。
证据：`/tmp/kb-review-id-omission-picsmw16/`。BUG-010已恢复两类可确定性修正的身份案例，
一般合同拒收恢复仍未完成；不将其泛化为任意失败可自动恢复。

### 父定位ID首次恢复（2026-10-16 01:29 UTC）

2026-10-16 01:29 UTC实查：同一真实run `eedcf1b0-40f4-42f2-b6c7-ef47ea723ec8` 已恢复running，
review 3/17有宿主回执，第4包在途；恢复命令 `5000f6e7-e047-4c9a-bfc3-c6dbad135770`
首次执行、失败0。第3包直接复用原传输成功缓存，宿主将92个身份规范化为85个实际范围，
保留8条findings并记录7个冗余父ID诊断；未为第3包新增模型调用。10项保留检查通过，
原4个抽取包、2个核验回执、原始回包及用量保持不变，来源及领域修订仍为1。
截至该快照已报告prompt231752 / completion134870 / total366622 Token；在途用量待返回。

本轮仅修改 `tender/checks/analysis.py` 的父定位身份归一化；先经schema检查原列表无重复，
实际范围均已出现时才移除当前body/supporting携带的冗余父ID，未知/缺失/重复仍拒收。
未改Prompt、payload、分包计划或预算。补齐重复父ID和完整SSE成功后宿主拒收/缓存恢复回归；
定向330个独立用例通过，零跳过/失败/错误，Ruff及diff通过；未重跑完整Python套件。
真实缓存禁网只读回放核对request/17包plan摘要不变，模型、网络、DB及产物写入均0。

01:26 UTC已部署镜像 `sha256:1c55ddb645f25c1a4ea3b9ac4929c9bcb72be2be49f15d02b79e73b0ef0a2069`，
三角色同镜像、init退出0、106个源码/Prompt指纹一致、并发2、PG/Redis连通，原网络overlay保留。
恢复首次被自动审批要求核实数据与目的地授权；只读核对原件SHA、10条原请求和.env服务
与冻结执行快照一致后，同一正式retry命令获准执行，未换路绕过拒绝。
证据在 `/tmp/kb-review-id-fix-85w1gdzd/`。BUG-009已完成真实恢复验证；BUG-010的一般合同
拒收恢复尚未完成，不能把当前可确定性恢复案例泛化为任意错误已解决。真实五节点及人工
语义验收仍未完成。后续发现问题继续更新[问题记录](bug-fix.md)。

### 修复前的失败诊断（2026-10-16 00:51 UTC）

2026-10-16 00:51 UTC只读实查：真实run `eedcf1b0-40f4-42f2-b6c7-ef47ea723ec8`
已于2026-10-15 10:35:36 UTC终态failed。4个抽取包保留319事项、30关系、796处置；
17个原文review仅2个有宿主回执。第3个verify收到完整JSON并记录传输succeeded，但其
reviewed_range_ids应为85个，实回92个（额外7个，缺失/重复均0），事项61/61、关系3/3、
决议0/0均匹配；未通过`Independent review did not cover exactly the supplied identities`。7个
额外ID已精确核对：全部来自已发送supporting条目的source_range_id父定位字段，
模型把父定位身份额外列作reviewed_range_ids；不是未知ID，也不是alias segment漏算。
宿主以实际range_id检查85项与回包92项不符而拒收，语义正确性仍须独立核验；
首次失败后的3次自动执行均复用同一已成功传输的缓存响应，重复合同错误而没有新模型调用。
当前命令execution_count=4、infrastructure_failures=4，预算耗尽后停止，无在途模型请求。
累计供应商prompt231752 / completion134870 / total366622 Token。尚未开始后续跨源定位、
联合核验或确认，不能称真实五节点通过。当前断点为核验回执/缓存重试衔接；后续恢复需
保留原回包和已成功抽取、核验回执，不通过清缓存、改摘要或删覆盖检查来伪造通过。
只读归属诊断还核对真实request_digest与完整17包plan完全一致，证据在
`/tmp/kb-review3-origin-9rmepgx_/report.json`。输入同含range_id和source_range_id而回包要求
模型完整枚举ID，将机械身份管理负担留给模型；传输成功缓存与业务接受的恢复策略未衔接，
造成3次执行重试原样重放无效回包。当前未改源码或重发模型。
本段替代下文10:29“正在运行”的历史状态，证据见
`/tmp/kb-lease-repair-_aszzk_8/latest-failure-summary.json`及`live/`快照。


用户最新确认本期只做**完整分析招标文件**：识别原文中的义务、评分规则、不利后果，
以及原文明示的填表、签章、附件与证明材料提交要求。技术、服务、售后、人员等是来源中的
业务内容，不按固定分类凭空补项。不采集或生成企业响应数据，不做响应计划、材料准备待办、
评分机会采纳或实际得分判断；企业是否具备材料、人员和资格也不在本期分析结论内。

当前工作区主线为 `register → freeze → extract → verify → confirm` 五节点，确认后终态为
`analysis-ready`。统一最小 `TenderItem` 合同为 `item_id/kind/text/evidence`，`kind` 仅有
`obligation`、`scoring_rule`、`consequence_rule`、`submission_requirement`；条件、例外、
分档/公式、否决触发范围等必须在条目文本及来源证据中完整保留。提交要求描述原文要求
提供什么，不填写企业实际信息，也不派生准备任务。条目类型和业务含义由 LLM 审阅，
宿主核对结构、来源引用、覆盖、身份版本、预算及执行围栏，不扩充正则承担语义裁决。

跨包引用只允许一轮有边界的目标补读；目录存在不等于正文已审阅，收到目标原文后才可
核验关系。仍然歧义、不可读或需要继续追索的内容转人工证据裁决，未解决前继续阻断。
不因“本包未提供”断言全文缺失，也不以人工确认覆盖来源或语义问题。

**五节点、新条目合同、抽取/核验提示、确认摘要及CLI已完成首次v2部署。** 新主线不再推进
旧Plan，不把旧产物转换成v2分析结果，也不改写旧运行记录。已部署的压缩版真实任务已结束，
抽取协议校验失败，尚未进入独立核验。

2026-10-15 10:24 UTC补充：同一run的失败抽取包已恢复，4包共319事项（117义务、18评分、
81后果、103提交要求）、30关系、796处置；原3包及全部成功付费回执保留。累计供应商用量
259898 Token，8个助手业务锚点均有事项匹配，不能代替独立核验或人工验收。

抽取完成后发现核验同步规划阻塞事件循环：真实离线规划耗时39.36秒，超过30秒租约，
导致旧命令8次执行而失败计数仍0。最小修复将三类规划、描述符预算测量、回包校验与读取
移到线程，预算/请求/回执身份不变；同一文件规划17个review请求覆盖796/796范围，最大
输入估算110490/110592，无超额；702次模拟心跳最大间隔0.149秒，断网且CAS只读。
失租由接管方计入原4次共享失败预算，兼容旧execution_count漏计，旧Worker继续受围栏保护。
旧恢复命令按8次失租终结，未增模型调用；再经显式retry建立同版本命令
`203a1ee6-2d6f-4af6-b454-d15223faf740`继续核验，13项保留检查通过。没有清DB状态、改来源
版本、改请求摘要或重抽成功包。

该修复10:23 UTC部署，镜像
`sha256:7552a5d995f576e6850bf4505b94cb8dd945b4641ad5d407c86e1d1122428858`；三角色同镜像、
init退出0、106个非测试源码/Prompt指纹一致，模型及分包并发2、PG/Redis连通、无新增端口
或环境变量，原网络overlay保留。定向143项通过；新增心跳4例和失租8例均先复现再修复，
完整Python回归1175 passed / 15 skipped / 0 failed / 0 errors，472.191秒；15项因缺少历史
或私有样本跳过。106个非测试源码/Prompt测试后指纹未变，Ruff及diff检查通过。10:29:48 UTC
真实核验1/17已接收，第2个请求在途；当前命令仍首次执行、失败0、心跳有效。累计供应商
用量292910 Token；部署前两个历史run全快照复核不变。证据在`/tmp/kb-lease-repair-_aszzk_8/`，
五节点终态及用户人工业务验收仍未完成。以下09:56/09:58记录为该轮历史状态。

用户随后同意补齐失败包恢复，并要求执行失败默认重试3次。该版于09:56 UTC部署：
镜像`sha256:cfdd20e86d1fad7b17fbc030d11f610c269f4c80667ccbea92835c1539a1addc`，
三角色同镜像、init退出0，106个非测试源码/Prompt与验证基线一致；并发2、PG/Redis连通，
保留网络overlay，无新端口或环境变量。明确执行失败采用首次加最多3次重试，命令发布与
执行共享4次持久失败预算，不叠加SDK重试；取消、版本/租约失效和sent/uncertain仍停止。
并发子操作优先传播不确定付费或失效围栏，防止被格式错误掩盖。

新增CLI retry在同一run/source_version/domain_revision新建幂等命令，保留原kind和业务
payload；重试审计不进入模型或确认产物。当前已以命令
`b1297d2b-c359-4a58-9d77-1bac5679df99`恢复下文run，09:58 UTC为running，仅新增失败包
1个extract在途，原3包221事项/518处置和OCR2次均复用；Verify尚未开始。
本轮定向回归分组最新197个独立用例全部通过，0跳过/失败/错误；包括默认3次、耗尽后显式
恢复、成功包复用、确认resume和uncertain优先停止。首组187通过/1旧预期失败，更新首次失败
即终止的旧断言后，补跑17项全部通过。14个修改Python文件Ruff及diff检查通过；未将该
定向结果称为完整Python回归。证据在`/tmp/kb-bounded-retry-vawj54vv/`，最终五节点与用户
人工业务验收仍未完成。下文09:32 failed及无恢复入口描述为本次修复前的验证事实。

用户于2026-10-15 UTC明确要求使用真实招标文件再次验证。本轮回包容错版已于09:14部署，
三角色镜像为`sha256:1344994e60b6091743e3542ab1c35af52b390a27f583125f3089fa438baa5fce`；
168个源码/Prompt与完整回归版本逐字节一致，init退出0，PG/Redis连通，模型及分包并发2，
保留原网络overlay，无新增宿主端口。使用.env既有Chat/VLM服务与`gpt-5.6-sol`模型，
目的地与上一真实run一致，未新配模型或改旧运行。106页原件SHA256仍为下文同一摘要。
新run为`eedcf1b0-40f4-42f2-b6c7-ef47ea723ec8`，source_version=1、domain_revision=1，
以新请求正常重新登记/冻结及OCR，不将旧回包伪装为新请求。运行于09:17:01开始，
09:32:00 UTC终态failed，耗时899.62秒；OCR2次成功，extract3次成功、1次失败。
3个extract-pack保留221事项（56义务、16评分、74后果、75提交要求）、25关联、518条处置，
未接收的一包对应278条原文范围。独立Verify和确认均未开始，不能称真实五节点通过。

失败请求`f9408dfbe11c459fa2dc9344143e4e31`已收到完整SSE、DONE和`finish_reason=stop`，
有供应商usage；content的两个多余右花括号导致`invalid_json_response`，不是网络超时或
流式截断。仅在内存中移除两个符号的诊断可得到JSON，但其dispositions为空，仍不足以
证明完整覆盖；该诊断不写回、不作为有效抽取、不替代独立语义核验。当前局部字段容错
发生在JSON解码之后，无法覆盖这种整包语法失败；待补齐显式恢复能力，不能直接改DB
重开命令、伪造来源修订或把格式修复当成业务核验。

6次调用全部报告usage：prompt108140、completion83180、total191320 Token；单次extract
最大completion36660。没有在途或uncertain请求，未自动付费重试。实际发送前覆盖审计为
106页、796范围、4包173/214/278/131范围，原文缺漏/重复/区间错误均为0；请求Wire合计
401776字节，4包输入估算均不超110592，最大110482，不能将这些字节当作实际Token。
本轮168个源码/Prompt指纹未变，两个旧run快照深比较一致，旧uncertain围栏保持。
部署、前后旧运行快照、真实终态及用量证据在`/tmp/kb-tender-real-check-ef2lx0v4/`；
发送覆盖审计在`/tmp/kb-live-plan-audit-70htrgxw/report.json`，JSON根因和终态审计在
`/tmp/kb-tender-terminal-audit-ay9itnq6/{failed-json-report,report-terminal}.json`；
14个不同CAS产物的路径、SHA和size核对通过。接收包有34条源LF恢复诊断、15条
unresolved处置，以及34次阻断问题记录，尚未经独立Verify；这些数量不能当作最终业务结论。
目前没有公开的同run失败包恢复入口：底层虽可复用成功回执、允许明确failed的新请求，
终态command不能靠重新投递恢复，且本run缺完整extract根产物，不能走correct伪装恢复。
恢复缺口只读报告为`/tmp/kb-tender-real-check-ef2lx0v4/recovery-gap.md`。
8个业务锚点的助手抽样复核中，评分、否决、财务例外和合同证明6项内容保留；质保服务
1项内容保留，但同一提交事项承载义务和后果，类型与拆分是否恰当待Verify；技术证明1项属于失败包，尚无
有效产物。未执行用户人工验收，不以抽样代替独立核验。报告为
`/tmp/kb-business-anchors-RIG0X2/assisted-sample-review-terminal.json`。
下方尚未部署、调用暂停及六阶段记录均保留其对应轮次的历史含义。

2026-10-15 UTC压缩修订与复审：

此前压缩修订完整Python回归：**954 passed / 15 skipped / 0 failed / 0 errors**，共969项，
404.143秒，退出0；隔离PG/Redis及确定性模型，无真实模型调用。15项因缺少历史或私有
样本跳过。部署配置9项、44个Python文件Ruff、diff检查通过；164个源码/Prompt文件
测试前后指纹一致。完整证据在`/tmp/kb-v2-finaltests-0cs6i341/complete-results/`。

- 压缩版三角色镜像为`sha256:230c2822f8683fdb9ff79112976e06d3fe1b46fb7edb12aaac6cace97df0becb`，
  init退出0、并发2、PG/Redis连通、旧run未变；证据为
  `/tmp/kb-tender-v2-live-iaveude4/compressed-deployment-verification.json`。
- 抽取不发送全目录，保留当前role、原文和必要上下文；locate目录按文档嵌套章节，
  无全量range列表，用section_refs由宿主装载完整章节。目录114909→49753字节。
  同一106页文件离线4包/796正文范围全部准入，最大输入估算110254/110592，覆盖与运行投影一致。
  796条处置仅最小JSON结构就需74328字节，不能当作真实Token或完整输出预测。
- 上一版全套910 passed / 3 failed / 15 skipped（416.81秒）；3项合成压力用例的joint/review
  超过14336输入额度。修复仅校准测试填充，生产上下文、输出预留及超额阻断保持不变；
  增加“associate可准入、完整关系使joint超额且零发送”负例，最终回归已全部通过。
- 格式复审的源码修复已落实：views按登记文档、冻结范围和片段起点排序；cell直接发送
  已切好的原文，能完整表示时省略重复整行text；抽取和关联共用实际可见证据校验。未发送
  整行text时，不能用隐藏行全文或跨cell拼接充当证据；已发送的整行原文仍可引用。
  规范原文及宿主偏移保持不变。
- locate新增受冻结PDF页清单约束的page_refs，按0基物理页补读全页及必要上下文；
  缺页、partial、OCR问题、整页超额均阻断。印刷页码映射不明仍未决，不靠正则或模型猜测。
- 新格式离线预检：4包、796原始范围、796处置、76模板行；完整原文覆盖缺漏、重复及
  错误区间均为0。239个表格行共805cell，172行省略整行text、67行保留兜底，所有cell
  原文与规范区间一致。Wire407944→400055字节（减少1.934%）；这是请求字节，非计费Token。
  最大输入估算110428/110592，规划与运行请求测量全部相同，输入全部准入；输出量与
  独立核验容量仍需真实结果验证。网络、模型及数据库调用均为0，源码指纹前后一致，证据为
  `/tmp/kb-v2-preflight-celltext-9ghbrybb/report.json`。
- 新run `c70e61d5-cd13-4b2a-afe3-665825d7f0fa`已failed：OCR2次、extract4次均succeeded，
  无sent/uncertain或HTTP失败，但只有1个extract-pack通过宿主校验落库。4包disposition合计
  796条完整覆盖；原始回包共315个事项，不能当作已接受事项或独立核验结果。
- 全量供应商usage：prompt111924 / completion130269 / total242193 Token，全部6次调用均报告
  用量；单次extract最大completion43709。16384输出预留不是wire上限，也不是实际输出预测。
  终态证据为`/tmp/kb-tender-v2-live-iaveude4/real-run-terminal-2026-10-15T073257.json`，
  observed_at为2026-10-15T07:32:57.298218+00:00。
- 旧镜像离线回放诊断定位92处纯空白引文差异，均可在各自range内唯一定位；另有2处
  dispositions各多声明1个item_id，以及1个numbers条目缺quote。未修改原回包或弱化校验，
  未重试付费。诊断在`/tmp/kb-tender-v2-live-iaveude4/live-extraction-schema-diagnostic.json`。

首次部署及此前验证事实：

- 2026-10-15 UTC，init、worker、dispatcher已统一更新为镜像
  `sha256:7e676a42033cd8a0c9a48dd8f7d6e5442425e19c987193c1f8278659585485bd`；init退出0，
  双常驻角色运行，容器内核对协议v2五节点、模型/分包并发均为2、PG与Redis实际连通。
  未新增宿主端口；保留原模型TCP中转overlay。旧run全快照未变，仍含2条uncertain请求。
  核验摘要在`/tmp/kb-tender-v2-live-iaveude4/deployment-verification.json`。
- verify已实现locate/associate/review固定子操作、全台账跨源候选发现、候选联合原文审阅、
  具体问题决议及部分补遗投影。宿主核对必要回执、覆盖和预算；独立接受的Decision才可
  解决其绑定问题，不能豁免宿主来源或执行事实。新路径的SupportingIndex词表只选候选
  上下文，不自动绑定或产生语义阻断；人工scope_decisions保留，scope审议不再要求理由
  命中特定词表，CLI按当前核验问题的真实范围受理。parser及旧独立工具的历史启发式仍保留。
- 完整Python回归 **896 passed / 15 skipped**，耗时407.35秒，使用隔离PostgreSQL/Redis
  和确定性模型。覆盖五节点确认、阻断、部分成功恢复、确认提交故障、纠错/来源修订、
  旧版本保护、完整输入预算及跨文档联合审阅；15项均因缺少历史或私有样本跳过。
- 本轮末次复核修复公共来源/事项引发的候选过度合并：仅共享同一issue_id才联合分组，
  单候选仍完整装载；对应回归已执行。同一句原文分片时，支持引文回到规范冻结来源，
  测试同时核对正文片段完整重组。Ruff、部署配置9项离线检查和文档检查通过。
- 新协议真实模型执行、真实文件五节点完成和人工标注质量验收均未完成；原有13项
  作用域未决、图片人工复核及旧uncertain请求仍保留，不能靠清空问题或伪造修订跑通。

本轮在[字段字典](field-dictionary.md#回包容错与继续核验)先明确职责与继续核验规则：模型判断
语义，宿主管理身份、原文和引用。抽取和关联共享受限LF恢复；处置item_ids由宿主派生；
不完整或无法追溯的可选numbers整字段省略并记录诊断，保留text/evidence及原始回包。
局部坏声明保留有效兄弟结果、完整原文和具体问题送独立核验，不新增自动付费修复循环。
宿主补出的待核对处置不冒充模型已判定的来源未决；业务疑点仍须独立核验或显式修订。
工作流执行身份增加normalization_version=1，避免旧派生回执混用；历史产物不重写。
最终源码和Prompt已离线重放原始4包：全部通过接收校验，保留315事项、37关系、796处置；
恢复92处引文中的137个源LF，2个事项省略整个可选numbers，重建2处处置索引。第二个
numbers问题为其引文无法在该事项已引用范围的实际发送文本定位，并非要求两条摘引互相
嵌套。模型原有14个明确unresolved仍保留为来源问题，另2个处置矛盾待独立核对；宿主
没有补造额外的受保护来源未决。原始回包/来源深比较及文件哈希不变，模型、网络、DB调用0。
预算复查使用当前Prompt和发送视图，4包输入估算107588/109873/110530/47760，全部不超
110592；最紧包余量62，后续Prompt或输入变动须由现有规划重新测量，不能复用旧预算描述。
完整请求Wire407944→401401字节；省去处置item_ids使最小输出结构74328→63184字节，
均为字节测量而非计费Token或实际输出预测。报告为
`/tmp/kb-final-protocol-VvBnef/{raw-replay-report,budget-report}.json`。

本轮回包容错最终完整Python回归：**1052 passed / 15 skipped / 0 failed / 0 errors**，共1067项，
426.74秒，退出0；隔离PG/Redis及确定性模型，无真实模型调用。跳过项均缺历史或私有样本。
168个Python/Prompt文件测试前后指纹一致；本轮12个修改Python文件Ruff及diff检查通过。
包括两项异常可选字段/旧式索引下的五节点验收，确认后到analysis-ready且无额外模型修复请求。
首次完整回归1037 passed / 5 failed / 15 skipped；5项均为合成重复填充叠加Verify提示增长
442字节后，完整joint输入超限50–114。仅调整3处测试调用的填充量，生产预算不变，现有
joint超限零发送负例仍通过。完整报告及前后指纹在`/tmp/kb-analysis-recovery-tests/`。

真实315事项的Verify初始计划也已离线测量，使用与真实4包ID一致的原始CAS副本，完整
保留37关系、796处置和54个Issue。review为20包，输入保守估算合计1115191，最大
110584/110592；locate为109包，输入估算合计9236641，全部准入。locate由10个基础块、
45个两两并集和54个逐问题定位包组成，每个事项在discovery中恰出现10次；每包全目录
重复发送，目录贡献约565万Wire字节，是当前累计输入的主要开销。这个规划没有发送模型
请求，也不等于核验完成；associate/joint仍须等待真实候选才能测量。不以新增门禁替代
后续运行，联调时须明确报告该累计开销。报告为`/tmp/kb-real-verify-budget-5tqBXk/`。
上述离线修复轮次未部署或重试付费；随后新建真实运行的状态见本文开头。旧failed和uncertain
不换身份盲目重放。历史v1语义样本仍须
迁移并经人工复核；合同测试、实际模型执行与人工语义验收分别报告，不把原始模型成功
响应等同于五节点成功。

## 现有部署与真实联调（2026-10-15 UTC，六阶段实现）

- 本地独立部署及真实模型调用已由用户明确恢复授权；下方历史记录中的“未部署/真实调用0次/暂停”
  仅代表当时轮次，不能作为当前状态。Rust 接入、正文生成、最终导出仍不在本期。
- 正式镜像已构建，`kb_python_tender_worker`、`kb_python_tender_dispatcher` 使用同一镜像运行；
  migrate 和一次性 init 已成功退出。容器命令使用 `--no-sync`，避免运行时下载构建依赖。
- Chat/VLM 已按用户要求独立配置；Python 不再读取 `KNOWLEDGEBRAIN_CHAT_SUPPORTS_IMAGES`，
  不从 Chat 补齐 VLM 地址/密钥/模型，不使用图片能力模型白名单。该变更后模型单测79项通过。
- 真实 PDF（106页，原件摘要 `4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`）
  已进入 run `96cf4063-56ea-4466-a80b-aa7ed1177b45`，source_version=1、domain_revision=1。
  登记、解析/冻结及2次OCR完成；4个抽取包仅1个完整返回，另有4次HTTP429拒绝、2次uncertain；
  当前终态为 **failed**，独立Verify、人工确认、Plan均未开始，不能宣称真实端到端通过。
- 完整包返回35条要求、165条处置、6项模型问题，耗时418.63秒，实际输出22795 token。
  宿主另发现5处处置与要求证据不一致；有成功回执不等于业务已通过。
- 两份中断流离线核对末尾错误帧分别为 `request_timeout`（904.94秒）和
  `server_is_overloaded`（363.47秒）；均无完整JSON/结束标记/usage，继续保留uncertain防重发。
  HTTP429原因为 `GATEWAY_NORMAL_KEY_CONCURRENCY_EXCEEDED`，不是密钥缺失或额度结论。
- 模型报告的“第五章供货要求缺失”已在第3包找到对应章节，暴露跨包可见范围问题，不能据此
  判原件缺失。全源语义核对尚未完成；原有13项作用域未决和图片人工复核也未被豁免。
- 用户要求默认并发从4降为2：分包调度和模型客户端已同时修改、重建并部署，容器内实例化
  核对两者均为2；客户端/并发回归 **82 passed**。4条消息线程及其他应用仍可能共享模型Key额度。
  并发属于不可变execution_spec，旧修订不会静默换配置；未重跑旧失败任务或删除支付围栏。
- 当前镜像身份 `sha256:3cc842ee83ba48c4b16a613e9427e913825cde715ba7fec825e4c757d1efe9e2`；
  工作区尚未提交。最新定向回归不能替代下方较早的完整Python测试。
- 容器直连新建TCP连接仍不稳定，旧项目新连接也曾复现；底层网络原因未查实。
  本轮模型请求依赖临时宿主TCP中转，仅绑定 `172.19.0.1:8317`，透传TLS，不改模型URL。
  worker 的私有Compose overlay在 `/tmp/kb-workflow-start-g8vpogcy/model-relay.yml`，
  中转脚本同目录 `model-relay.py`；后续重建角色须保留该overlay，不能称直接出口已修复。
- 私有响应报告及完整/中断文本在
  `/tmp/kb-tender-real-ef9nvq8i/output/response-review/`；本次构建和定向回归证据在
  `/tmp/kb-workflow-start-g8vpogcy/build-concurrency2.log`、`concurrency-green.xml`。
  原件、业务全文、完整wire、密钥均不提交Git。

前一轮用户要求先修复差距，并指出评分、否决、技术、服务、售后、人员等只是业务例子，需要
重新核对完整 workflow。已完成[业务流程修订](business-workflow.md)，从来源到要求/规则/关系、
独立核验、人工确认、响应矩阵及独立计划审阅列出产物与放行条件。先统一这条业务主线并完成
合成纵切，再据新合同校准输出分包、跨包补读和显式付费恢复；不再仅按零散分类字段补丁推进。
**新增业务协议、关联决议、响应矩阵及计划审阅仍未实现**。旧自然语言启发式仍可能绑定或阻断，
不能靠删规则、伪造来源/作用域变更或清空问题取得成功。真实任务未重跑。
该轮包含响应规划的方向已被本文顶部的新范围取代，独立计划审阅不再是当前任务。

本轮另修复流式供应商错误被掩盖为 missing_done 的问题：安全记录 type/code，显式错误保持
uncertain，附带 DONE、诊断载荷损坏或响应头错误也不授权自动重付费。相关模型/流式单测
**139 passed，0 skipped**，不计为业务语义验收；私有报告
`/tmp/kb-workflow-start-g8vpogcy/recovery-green.xml`。该代码尚未重建部署，不能把当前容器
当作已包含此修复。

## 较早的隔离真实服务测试（2026-10-14）

- 已用现有 `deploy/.env` 启动本仓库的 `kb_postgres`、`kb_redis`，均健康且未发布宿主端口；
  未修改 `/opt/workspace/zhaobiao` 的旧栈。主 Compose 的 Python PG*/Redis 配置已通过实际连接检查。
- `.env.example` 已与共享配置及新端口一致，无需另配 TENDER 连接；实际 `.env` 的
  `KB_ADMIN_PASSWORD` 已补单引号，确认含 `$` 的密码按原字面值读入，不记录密码内容。
- 首轮完整 Python 测试为 715 passed、1 failed、15 skipped；唯一失败是镜像缺少 Calc，
  无法生成旧版 XLS。`deploy/Dockerfile.docreader` 已补 `libreoffice-calc-nogui`。
- 补齐依赖后的单项回归通过；完整复验为 **716 passed、15 skipped，309.11秒**。
  其中 workflow/acceptance 73项全部通过、零跳过；另有来源检查2项、部署配置检查9项通过。
  15项跳过全部是缺少文件样本，不能当作这些样本的验收成功。
- 测试在同一 PostgreSQL 实例的临时库及 Redis 15号库执行；临时库、账号、测试容器及
  队列已清理，PG/Redis 保留运行。报告在 `/tmp/kb-python-tests-zu7cbv56/summary.json` 和 `pytest.xml`。
- 上述完整测试使用确定性模型，代表该次隔离测试，不计为真实模型六阶段验收；当前进度见上节。

## 较早职责边界复核与六阶段待办（历史）

以下保留当时的审查与接手记录；当前范围和下一次实现顺序以本文顶部为准。

基于 `2ad480b` 完成[LLM 语义职责与宿主规则审查](semantic-review.md)，并同步 README、
领域合同和验收待办。业务语义由 LLM 审阅及必要的人工裁决确定；宿主保留结构、来源、
覆盖、身份版本、预算和执行保障。自然语言正则不能作为最终语义判断依据，不以继续
扩充比较词、动作词或作用域词表作为修复路线。

随后已准备[合成语义样本草案](semantic-cases.md)：13组配对、26个候选抽取，另有2个
跨文档联合审阅草案。agent 复核补齐表格单位案例的报价说明，并已准备
[Verify 离线导出、记录与人工判定流程](semantic-evaluation.md)：26份隔离输入、提示和语料
摘要、未运行记录模板；2个跨文档草案不纳入调用输入。标准库引用/覆盖及导出隔离检查
通过；预期待人工复核，未运行 Pydantic 合同校验、完整 pytest 或真实模型评测，不能据此
报告模型质量。

语义审查阶段只改文档、样本及离线检查，语义运行代码、提示和协议尚未调整；现有语言启发式仍可能直接绑定或阻断。
不将新职责目标写成已实现，不移除现有阻断，不新增真实模型调用，也不将下方历史测试
结果视为本轮语义验收证据。

用户随后明确：下一阶段优先把当前 PR 的真实端到端流程跑通。合成样本评测作为诊断及
回归材料，不要求先完成26例人工标注或新增计划审阅协议才启动真实全流程联调。

下一阶段的主线及可验证结果：

1. 恢复 Python 运行依赖、共享 PostgreSQL/Redis、持久产物目录及模型配置，验证既有
   CLI、worker、dispatcher 能运行；宿主连接使用本机端口。本执行器的 Python 运行依赖
   待装配，`deploy/.env` 已有通用部署及模型配置。按用户要求，Python 角色现已并入主
   Compose 的 `python-tender` profile，复用同一 PostgreSQL、账号和 Redis，无需另配
   TENDER 连接；运行时支持 libpq PG* 变量及 DATABASE_URL。启动顺序固定为共享库
   `migrate`、Python 初始化、worker/dispatcher，避免首次建表竞态。
   初次沙箱端口探测不能代表宿主状态：随后宿主检查确认旧 `knowledgebrain`
   栈健康运行，归属 `/opt/workspace/zhaobiao`。本仓库部署使用独立 `kb` 项目及端口，
   不接管旧栈；下方历史测试结果不能当作本仓库 Python 全流程已就绪的证据。
2. 取得真实原件并校验摘要，执行登记、解析、覆盖及冻结，检查实际分包输入和预算。
   逐项审议真实作用域及图片问题；历史13项未决不按测试需要清零，也不假定新环境已有原件。
3. 满足下方真实调用恢复边界后，用既有模型客户端及工作流执行抽取和独立 Verify。
   按实际回包审查后续完整输入，沿运行结果修复具体阻塞，保留审计与修订后重新验证。
   合成/FixtureModel 的成功不能计为真实模型全流程成功。
4. 提供实际来源、抽取及验证清单供人工确认；确认后执行 Plan，核查实际计划逐条落实
   当前适用要求，最终状态达到 response-plan-ready。未解决阻断不能通过确认豁免。
5. 保存真实 run_id、来源及修订身份、各阶段产物、模型调用/usage、确认及最终计划证据，
   验证中断后复用已成功回执；将本次发现的具体缺陷补成针对性回归。

这里的完整流程终点仍为本期响应计划，不扩大到 Rust 接入、正文生成或最终导出。
真实付费调用继续按下方明确授权边界执行。人工标注和系统性语义质量验收仍待完成，
与“已跑通一次真实全流程”分别记录；当前未实现计划生成后的独立 LLM 审阅。

## 433a464 实现交接与暂停边界（2026-10-14，以下保留原验证事实）

当前两项代码review已修复，并完成本轮确定性workflow、恢复、全量Python和真实原件离线
SDK wire复验。按用户最新要求，提交本交接后暂停，供父任务独立核对；不启动真实模型或新范围。
[Draft PR #24](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/pull/24)保持Draft，未合并、部署或切流。

- 分支：`feat/python-tender-phase1`；本轮起点fetch核对为`f78462ff956d62893f0909bd897071787f21f5f3`，无遗漏后续远端改动。
- 最终运行代码：`433a46475a528f089a4b6942767405c44d375c34`，已推送。最终交接为此文件所在提交，精确HEAD另记在PR描述及任务结束报告。
- 文档先行：`cc6185e`、`1b2ccf7`；解压不确定发送修复：`e64e1a942f948654c27e72e1f00f036377e6d958`；完整下游依赖与回归：`433a464`。
- main基线仍为`e8b681a0997760ec3f567223fe7eb82b622f5ed7`；Rust源码、Cargo、默认链路无diff。未改生产接入或增加第二状态机。
- 剩余额度没有可读接口，已阶段性push；真实模型调用0次并继续暂停。没有待审批或不确定执行动作。

## 433a464 当时方案与合同（历史）

1. 普通输入上限131072 token，显式纠错最多262144 token；两者均受provider总窗口、输出
   和安全预留约束。当前`context_tokens=131072`是总窗口，未扩大；输出16384、安全4096，
   有效普通输入110592。常规verify/plan及新领域修订不自动借纠错额度。不新增wire max_tokens。
   未知tokenizer用标明的UTF8保守估算，不冒充服务真实token或usage。
2. extract/verify使用共享SourceView；后阶段按当前修订同文档**全部extract包**的完整输入
   规划。verify加入全部合法requirements/dispositions，plan加入全部有效要求/约束及显式
   来源引用所需source_context。小章节合并；只有完整请求超限才按原生结构/语义依赖边界
   重分。同处置、完整义务和明确跨包依赖不可分；仍超限则明确预算阻断，零发送，不裁证据。
3. 要求、处置统一为`RequirementRef(artifact_ref, requirement_id)`与
   `DispositionRef(artifact_ref, range_id)`；阶段根`requirements_ref`只指完整索引。
   原文只在冻结来源保存；阶段计划仅持selector和引用，checkpoint仍是六个固定阶段引用。
   旧support证据身份与body重合时，`source_aliases`以无text的精确区间保留原ID，不重复原文。
   显式range依赖按canonical区间相交连接；非重叠片段和仅共享条件不强制绑定。
4. 来源覆盖、要求/处置、条件/例外、模板、修订身份均核对；未知、跨文档、歧义及已失效
   义务依赖明确阻断。verify/plan子包回执绑定完整输入、预算、修订和plan_digest；部分成功
   回执在恢复时复用，修订使旧确认失效。沿用既有LangGraph/PG回执，不增加通用框架。
5. 所有LLM任务stream=true；180秒连续**无原始数据**才idle超时，无总180秒截止。部分
   SSE/gzip解压失败记uncertain；审计写盘/提交故障保留sent围栏，同digest恢复不再付费。
   完整成功响应后的业务提交故障从已校验response_ref继续，缺usage保持unknown/null。

字段和语义以[字段字典](field-dictionary.md)、[合同](contracts.md)及[最终请求审查](request-review.md)为准。

## 最新验证事实（只代表433a464）

| 检查 | 本轮结果 |
| --- | --- |
| 完整Python3.11.16，真实PostgreSQL/Redis | **716 passed / 15 skipped，176.64秒** |
| CI另行Python source-captions检查 | **2 passed，0.05秒** |
| 新增完整下游实际workflow文件 | **12 passed，48.60秒**；已包含在完整套件 |
| 独立范围/别名/失效/歧义依赖回归 | **10 passed，2.02秒**；已包含在完整套件 |
| 流式/请求回放相关套件 | **124 passed，6.64秒**；已包含在完整套件，其中新增8项真实PG恢复用例 |
| Ruff（相关运行模块及unit/workflow/acceptance新增检查）、diff检查 | 通过 |
| wheel | 构建成功，98个Python/类型/提示文件与当前源码逐字节一致 |
| 固定SHA实际SDK离线wire | `runtime_dirty=false`；全部完整输入预算、引用/来源覆盖检查通过 |
| 运行代码远端Python专项CI | **486 passed，172.24秒**，wheel成功；真实PG/Redis，0skip |
| 运行代码远端通用CI的Python job | **642 passed / 89 skipped，48.30秒**；source-captions另2 passed；该job未配置同等真实服务 |

15个skip均为原有缺失fixture：Excel2、Markdown1、PDF3、PPT6、旧私有招标源合同1、
旧testdata/bid2。新增PG/Redis、workflow及恢复测试无skip；本轮真实私有PDF另行明确执行了
下述离线复验。首轮全量为699 passed/5 failed/15 skipped；5个失败均由新增fixture越过
预期预算或触发重复文档校验造成，修正fixture且完成独立发现的实现缺陷后重跑完整套件，
最终结果为上表716/15，不把首轮或旧684/146混作最新通过结果。

实际workflow证明：extract能容纳、合法输出加入后verify超限，跨extract包重新规划后全部
审核；人工确认后plan按其自身完整输入重分。verify及plan的不可分预算分别验证零发送；
两阶段部分成功后的恢复不重做成功兄弟包；同源修订撤销旧确认而冻结引用不变；条件alias
完整重建引用且发送原文不重复。范围别名、失效引用和命名空间歧义由独立审查补测。

wheel SHA256：`e79ca5bcd102770fdbb3a503f627ebcbab3ba54d0f179a3cb87a97fb1f98adea`。
本轮未重建Docker镜像；历史镜像不能称为当前代码镜像。
[当前Python专项CI](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/actions/runs/38545943835)通过；
[通用CI](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/actions/runs/38545943820)的Python/web通过，
Rust及依赖job显示cancelled，未宣称整个CI全绿，也不在此任务扩展运行/修复Rust。
以上均绑定运行433a464；最终纯文档提交触发的后续流程应另核对，不能混为同一SHA结果。

## 真实原件与最终发送审查

当前Library支持方式重新materialize了`libfile_5be1d1c487b88191aed7fe91a0d6ec1f`，
`BiddingFile(1).pdf`，755602字节，SHA256：
`4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`。
本轮真实重解析得到106页、520单元、796范围、42表/318行；来源覆盖通过。98图片位置
绑定两份实际像素，重新作agent直接视觉审阅，`human_review_completed=false`；原件及审议
记录仅本地私有。13项`condition_scope_unresolved`仍在11个页面，未清零。

最终wire证据绑定运行433a464，实际AsyncOpenAI序列化、本地MockTransport捕获，网络/
模型调用0。622条明确synthetic要求及796处置用于完整输入压力测试，**不是实际抽取结果**。
原4个完整verify估算297565/320147/315232/142166，全部超过110592；新结果：

| 阶段 | 请求数 | wire字节 | 最大输入保守估算 |
| --- | ---: | ---: | ---: |
| extract | 4 | 402371 | 110413 |
| verify | 13 | 1186694 | 110517 |
| plan | 7 | 695530 | 110538 |

全部stream=true、无max_tokens；796处置保留，698交模型审核、98宿主单列。622要求各阶段
完整覆盖，条件/例外/跨包source_context保留，extract/verify正文均159186原始UTF8字节。
同请求body/support重复字符0；必要表结构、空模板身份、prompt/schema保留。压力样本
全文quote不可当最小生产引用建议；24请求也不能当真实执行次数预测。

- [新真实结构证据](evidence/real-pdf-downstream-structure.json)
- [最终完整下游wire证据](evidence/request-wire-downstream-synthetic.json)，文件SHA256：`256aa3cb9794a75230a6bbe5ca74d119f6f638b594bf4ac0ffc460a20e254482`
- [字段必要性与局限](request-review.md)；原件正文、完整wire及图片没有提交到GitHub。

## 433a464 当时剩余状态与执行顺序（历史）

当前两项代码缺陷和已安排离线验证完成；PR仍Draft，真实语义验收**未完成且仍暂停**。
后续只有再次收到授权后才执行：

1. fetch核对PR24最新HEAD，读取本交接、AGENTS及可用相关skills，保留任何后来修改。
2. 新执行器通过当前Library支持的materialize方式取得上述file ID，校验大小和SHA；不能
   假设本机私有路径在新执行器存在。若不可读，只做可复现合成回归，明确未作真实文件复验。
3. 对13项scope逐项准备带来源证据的决议并复验，不能按测试需要清零；人工复核图片判定，
   准备人工标注集及验收标准。零基页10、15、21、64、66、67、68、76、77、84、88存在待办。
4. 取得实际provider总窗口/能力证据，维持输入预算和输出/安全预留；不得以付费探测猜容量。
5. 仅在明确恢复真实模型调用的授权后开展真实语义验收。extract前审查最终wire；真实输出
   形成后，再审查实际verify/plan完整wire、必要重分、依赖与成本，逐阶段准入后才付费发送。
   13项未决、人工标注和真实模型结果不能由本轮synthetic通过替代。
6. Rust API/outbox/权限/事务/生产接入及切换另行授权和验收；本任务不继续这些范围。

## 环境、复现与私有产物

仓库AGENTS已读；`/workspace/.agents`为空，仓库无相关`.agents/skills`，不假装已读。
当前本地私有输入：`/workspace/private-tender-acceptance-current/BiddingFile(1).pdf`。
私有结构/新图片审议：同目录`downstream-preflight/`；最终完整wire：`downstream-wire/`。
新环境须通过Library `prepare_materialize`提供的授权方式自行获取，下载地址不写入文档。

本地隔离测试容器`kb-tender-budget-postgres`、`kb-tender-budget-redis`保留，端口15432/16379；
测试临时schema在结束后清理，无生产库改动。Python路径为`services/docreader/.venv311/bin/python`。
最终全量日志`/tmp/kb-downstream-full-final.log`，测试产物`/tmp/kb-downstream-full-final/`；
wheel在`/tmp/kb-downstream-final-wheel/`。复现（从services/docreader执行）：

```bash
export TENDER_TEST_DATABASE_URL=postgresql://tender_test:tender-test-local-only@127.0.0.1:15432/tender_phase1_test
export TENDER_TEST_DSN="$TENDER_TEST_DATABASE_URL"
export TENDER_TEST_REDIS_URL=redis://127.0.0.1:16379/1
export TENDER_REDIS_URL="$TENDER_TEST_REDIS_URL"
.venv311/bin/pytest tests -q -rs --tb=short --basetemp=/tmp/kb-downstream-reproduce
PYTHONPATH=.. .venv311/bin/pytest ../../scripts/tests/test_bidding_source_captions.py -q
UV_CACHE_DIR=/workspace/.cache/uv UV_TOOL_DIR=/workspace/.uv-tools uv tool run ruff check llm tender workflow tests/acceptance tests/workflow tests/unit/test_downstream_dependencies.py
UV_CACHE_DIR=/workspace/.cache/uv uv build --wheel --python .venv311/bin/python --out-dir /tmp/kb-downstream-wheel
.venv311/bin/python tests/acceptance/review_downstream_wire.py \
  --source '/workspace/private-tender-acceptance-current/BiddingFile(1).pdf' \
  --runtime /workspace/KnowledgeBrain \
  --private-root /workspace/private-tender-acceptance-current/downstream-preflight \
  --output-private /workspace/private-tender-acceptance-current/downstream-wire \
  --report /tmp/kb-downstream-wire-reproduce.json
```

wire脚本读取上述新真实解析产物；新执行器还需先按`tests/acceptance/prepare_real_pdf.py --help`
重建私有预检与图片来源绑定审议，不能把缺失私有文件当作可复现。保持原件私有属性与权限。
历史`f78462f`/运行`1480efc`只通过146项且两项review未完；其完整记录在Git历史，不代表当前
结果。以下5aa6b84/运行51d7ec3的684/15skip、454专项CI和镜像证据也仅保留历史原义。

## 以下为5aa6b84历史交接，保留原义（不代表本轮验收结果）

本文件是当前安全保存点；保持 [Draft PR #24](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/pull/24)，不合并、不部署、不切流，付费模型请求继续暂停。
环境未提供可读的剩余额度接口，无法监控10%阈值；采用阶段性提交，避免只保留本地修改。

## 身份与当前结果

- 分支：`feat/python-tender-phase1`。
- 最新只读fetch的main基线：`e8b681a0997760ec3f567223fe7eb82b622f5ed7`。
- 最终运行代码：`51d7ec322e7ac09cb6c05043ccb07e8ecf03d76c`。
- 最终镜像/实际wire证据：`41c772812afeef32f0553e952446d3808b69466d`。
- 当前文档保存点是此文件所在提交；代码未在上述实现之后改动。
- 独立Python阶段1–6及确定性验收完成，终点`response-plan-ready`；真实模型语义验收未完成。
- Rust源码/Cargo/默认执行链路diff为空，旧实现未删除；未来Rust API/outbox/事务合同不表示已集成。

## 已完成

真实Redis/PostgreSQL骨架先验收，再完成来源登记、原生解析与真实图OCR适配、覆盖/冻结、
结构分包、并行抽取、独立全来源验证、人工确认、响应计划和独立CLI。支持租约/围栏、
重复消息、claim token、失联补投、持久interrupt释放worker、心跳取消、源版本及域修订、
完成回执恢复、部分包隔离和恢复配置固定；使用共享严格schema，无未发布旧格式兼容层。

最终发送视图只投影必要语义：正文、来源ID/片段、共享章节、表结构/公式/部分空字段/实际图片、
范围条件及目标；模板逐原行处置，宿主审议和模型审阅分开。实际SDK wire审计由本地transport
截取。未知tokenizer采用decoded-message UTF8保守估算，HTTP转义单独计字节，不冒充usage。

全部模型任务已stream=true；180秒原始接收idle，无总180秒截止。request_digest绑定完整无鉴权
请求，真实PG发送围栏阻止sent/uncertain换ID重付费；成功response_ref校验后复用。收到部分流
保留单份审计、usage unknown，无最终业务成功则不能产生成功领域产物。

## 测试与证据状态

| 检查 | 结果及边界 |
| --- | --- |
| 完整Python3.11真实PG/Redis回归 | 684 passed、15 skipped，142.69秒；新增服务测试无skip，原有可选测试skip |
| 定向phase1/增长/共享schema | 46 passed，48.05秒；之后追加严格SourceView用例已纳入全套 |
| Ruff、git diff --check | 通过 |
| wheel | 构建成功，48运行模块/提示与源码逐字节相同 |
| 镜像 | 构建成功；Python3.11.17；48文件与51d7ec3一致；容器CLI真实PG初始化通过；默认gRPC CMD不变 |
| 真实106页PDF离线预检 | 520原生单元、796范围、4包；原文/结构/图片ref不变；13范围未决仍阻断 |
| 最终SDK wire | 抽取1,021,405→402,371字节，减少60.6%；正文159,186字节不变；不是provider token |
| 2倍/4倍增长 | 文件产物1.980/3.941倍；checkpoint约18.9KB、6阶段引用，非平方增长 |
| 真实LLM、人工标注召回 | 未运行/未完成；真实模型调用0，usage=null |
| Rust测试 | 本阶段未运行/未修；已知main历史500passed/2failed/24ignored不能归因本PR |
| GitHub Actions | 运行代码51d7ec3的Python tender standalone已通过：454 passed / 83.88秒，wheel成功；后续纯证据/交接提交的流程尚在运行，接手核对 |

[运行代码的远端验收](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/actions/runs/38512688636)已确认成功；与本地全套684项口径不同，远端只跑unit/workflow/acceptance。

首轮新增失败断言曾误把“队列执行回执completed且业务failed”写成command failed；已修正测试并全套通过。
镜像验证脚本曾把原parser导入日志与JSON合并解析而报JSONDecodeError；修正只读诊断解析后核验通过，
不是镜像运行失败。此前构建磁盘不足已清理本任务旧镜像/缓存并重建成功；无需清理共享服务或数据。
当前没有未修复的本地测试失败。原有15个skip、真实语义及远端未完成状态保持单列。

公开统计/哈希/逐包比例/边界/字段必要性/镜像和增长证据见
[request-review.md](request-review.md)、[acceptance.md](acceptance.md)、[evidence](evidence/)。
旧sample-*.json、旧镜像与旧21/13包证据保持历史原义，不替换成新模型输出。

## 私有产物与环境

以下仅是工作区位置；文件正文、图片、请求原文和密钥不得加入Git或发给新接收方。

- 原件：`/workspace/private-tender-acceptance/BiddingFile(1).pdf`，755602B/106页，SHA256
  `4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`。
- 当前canonical与4包：`/workspace/private-tender-acceptance/python-phase1-wire-preflight/`；
  `frozen-index-ref.json`指向aggregate PackPlan，`artifacts/`为内容寻址产物。
- 修复前13包：`/workspace/private-tender-acceptance/python-phase1-final-preflight/`。
- 实际SDK发送JSON仅在：`/workspace/private-tender-acceptance/python-phase1-wire-before-requests/`
  和`python-phase1-wire-after-requests/`；有私有权限/原件属性，禁止上传。
- 图片审议输入：`/workspace/private-tender-acceptance/python-phase1-preflight/image-dispositions-reviewed.json`；
  98出现位置/2份内容经视觉审议，结论只对该原件hash有效，不能按重复/大小自动推广。
- 最终pytest文件产物：`/tmp/kb-phase1-wire-final/`；临时schema测试后已删除。
- Python：`services/docreader/.venv311/bin/python`；系统默认3.12不能替代3.11验收。
- 本地PG：127.0.0.1:15432；Redis：127.0.0.1:16379/1。使用独立测试库/临时schema。
- 本地镜像：`knowledgebrain-docreader:python-tender-phase1-review`，
  `sha256:8769713667a41d1810e351b47a29f49b074c1a8acf15e51102895bb7010d3032`。
- 构建/核验辅助脚本：`/tmp/kb-build-latest-image.py`、`/tmp/kb-verify-latest-image.py`；
  日志`/tmp/kb-tender-latest-image-build.log`。仅临时CA secret mount解决代理信任，TLS未关闭。
- 修复前运行代码隔离worktree：`/tmp/kb-before-view-runtime`，基于82678ac，仅用于重放成本审计。

## 复现命令

在仓库根核对当前状态及远端，不将密钥环境完整打印：

```sh
git status --short
git rev-parse HEAD origin/main
git diff e8b681a0997760ec3f567223fe7eb82b622f5ed7 --name-only -- '*.rs' Cargo.toml Cargo.lock
gh pr view 24 --json url,isDraft,headRefOid,statusCheckRollup
gh run list --branch feat/python-tender-phase1 --limit 6
```

在`services/docreader`，设置本地测试配置（这些是隔离测试服务固定凭据，不是模型/生产秘密）：

```sh
export TENDER_TEST_DSN='postgresql://tender_test:tender-test-local-only@127.0.0.1:15432/tender_phase1_test'
export TENDER_TEST_DATABASE_URL="$TENDER_TEST_DSN"
export TENDER_TEST_REDIS_URL='redis://127.0.0.1:16379/1'
export TENDER_REDIS_URL="$TENDER_TEST_REDIS_URL"
.venv311/bin/pytest tests -q --tb=short --basetemp=/tmp/kb-phase1-reproduce
uv build --wheel --python .venv311/bin/python --out-dir /tmp/kb-phase1-reproduce-wheel
```

离线预检（输出私有数据到新目录；不要覆盖旧证据）及wire重放：

```sh
.venv311/bin/python tests/acceptance/prepare_real_pdf.py \
  --source '/workspace/private-tender-acceptance/BiddingFile(1).pdf' \
  --private-root /workspace/private-tender-acceptance/reproduce \
  --report /tmp/kb-reproduce-preflight.json \
  --expected-sha256 4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc \
  --context-tokens 131072 \
  --image-dispositions /workspace/private-tender-acceptance/python-phase1-preflight/image-dispositions-reviewed.json
.venv311/bin/python tests/acceptance/review_wire.py \
  --source '/workspace/private-tender-acceptance/BiddingFile(1).pdf' \
  --runtime /workspace/KnowledgeBrain \
  --private-root /workspace/private-tender-acceptance/reproduce \
  --report /tmp/kb-reproduce-wire.json \
  --output-private /workspace/private-tender-acceptance/reproduce-wire --label after
```

## 剩余项、阻塞与下一步

1. 优先查看最新远端CI；若只有已知Rust基线问题，不越界修Rust。若Python失败，按真实日志修复并重新保存。
2. 私有原件仍有13项范围/引用/优先级绑定审议，不能一概叫文档歧义，也不能confirm清空。
   使用带原文证据的`review-scope`生成新domain_revision，重新分包/抽取/独立验证/确认。
3. 人工标注集及真实LLM语义验收未运行。付费调用是**明确暂停**，不是权限拒绝或已失败；
   先向父任务报告输入、endpoint/model和估计范围取得继续确认。已有配置指向
   `https://cpamc.818996.xyz/v1/chat/completions`，model `gpt-6-luna`，C=131072；禁止打印key。
   当前无重试/重分假设为4抽取+4验证+0..4计划（8..12次），实际用量未知。
4. verify的成本目前只是来源下界，加入真实requirements/dispositions后可能超限；必须重新准入。
   超限会明确阻断，不裁证据、不扩大服务C；必要有界重分需保持依赖/覆盖并记录设计判断。
   当前未实现通用自动后阶段重分框架，也没有把这个未知预算写成验收通过。
5. 达到独立真实语义验收后，未来Rust接入仍需另行授权/验收。knowledge材料检索、正文生成、
   成稿审查及最终导出不在本期，不增加空壳模块冒充完成。

没有待执行的破坏性操作，也没有不确定的Git推送结果；此前41c7728推送及PR正文更新已确认成功。
