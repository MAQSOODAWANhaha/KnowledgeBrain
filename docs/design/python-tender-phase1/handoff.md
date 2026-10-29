# Python 招标第一期交接

问题台账统一见[问题与修复记录](bug-fix.md)：每次发现问题追加或更新根因、方案和验证状态；
本文保留运行时间线及证据位置，字段定义仍以[字段字典](field-dictionary.md)为准。

## 修改后完整复跑（2026-10-28，本地时间）

用户要求查看当前修改后的完整效果，已于10月28日18:10:37 America/Los_Angeles
（10月29日01:10:37 UTC）启动同一106页原件的新执行。run
`c189e80a-e1a6-4442-bf09-00c0f58ffb66`，project
`f677c09d-716d-5631-b152-3454b00e77f1`；证据根为
`/opt/workspace/kb-tender-evidence/2026-10-28-full-rerun-v12/`。
使用当前Python冻结源码、normalization12及已核定的API/migrator二进制，3份SQLx
迁移checksum与当前SQL逐一一致；独立数据库/API/gRPC/Redis DB15，不改共享栈或.env。
启动前模型和执行表均空，不继承旧分析、大纲或正文；旧运行及全部回执保留。
模型、上下文131072和并发2沿已授权配置；没有企业知识资料，企业应答及报价留空。
normalization12针对回归最终9/9通过；首次测试目录未建导致的fixture错误单独保留，
不是生产执行错误。完整运行刚启动，尚无新稿，不能用旧输出替代本轮验收。

01:19 UTC：106页解析、2次OCR已完成，冻结仍796范围/4抽取包。89个既有重点锚点
按完整来源位置与文字唯一匹配，另加B限定及诉讼检查；尚不代表语义抽取已验收。
抽取已1/4接受；一包收到供应商api_error中断，原流与未知usage保留，自动重试
在原额度内进行。Dramatiq监控展示子进程端口冲突已记BUG-090，业务Worker与租约
正常，不为修监控打断在途调用。原文检查及错误证据在新根acceptance/和
provider-stream-errors.json。

01:26 UTC：抽取仍1/4接受。一包第二次返回完整JSON，仅第24条关系身份无法绑定
（BUG-077再现），已按同一首次加三次额度反馈纠正。另两条中断回执同时缺完整结束
和usage，分类为transport_response_unknown；尚无底层异常类证据，不能与第一条明确
供应商api_error混为同一已知原因。目前两包第3次尝试在途，不重置耗尽额度、不重跑
成功包。细目及原回执留在新证据根。

01:33:18 UTC：4/4抽取包全部接受，约22分40秒。新台账433事项、796处置、120关系、
83issues；处置478 items/291 non_item/27 unresolved，796原范围逐一覆盖、零重复遗漏。
累计2 OCR、8抽取物理调用（5完整回包含1次合同拒收、3断流）；已知439311 Token，
3条unknown usage不按零计。随后独立review规划11包开始执行。
实际新事项/处置可读预览位于`acceptance/extraction-preview.md`与
`acceptance/extraction-all-items-and-dispositions.md`。30个技术入口均有新事项引用，
备件B限定与诉讼要求已抽取；目录附件3/5的部分改标点引文以问题/未决保留，后续
需核验。上述为抽取及原文覆盖结果，不代表核验或最终稿语义通过。

01:54:39 UTC：独立review11/11全部首轮完成，无新增传输或合同重试；随后定位补读
规划18包并开始发送。该数量来自本轮433事项/核验问题台账，不沿用旧运行13包，
发现/逐问题定位的拆分及后续关联数量仍须按实际计划审计。核验回包完成不等于
待核业务问题已经解决，分析尚未进入人工确认，大纲和正文也尚未生成。

02:24 UTC：locate18/18全部首轮完成，开始42批关联判断。定位实际为6 discovery+
12 reference；发现批数未增，reference89问题由47条needs_source=true、8条false及
34条未声明组成。另47问题走local（43条false、4条未声明）。8条false随5个仍有
真实来源依赖的组进入4请求，不能仅按false删除整批；18份重建payload摘要逐一匹配
真实计划。源证在`acceptance/review-followthrough.{md,json}`。本轮已知问题台账
不同于旧运行，不能把18/13或42/45差异直接归因于优化。关联及后续joint尚在进行。

03:01:24 UTC：关联21/42批完成，77/159逻辑组结果保存，均首轮接受；2批在途。
完整计划159组分别装入34×4、7×3、1×2共42批，每组恰一次。最紧请求估算
28387输入+98304输出预留+4096安全=130787，仅距配置131072余285；这是未完成
端点计量校准的准入估计，不是实际Token窗口保证，BUG-086未据此关闭。分析阶段
仍运行中，已知报告用量3028232 Token，原3条未知usage继续单列；尚无新大纲/正文。

03:27 UTC用户询问运行是否符合预期，完成只读中途审计：35/42批、131/159成员
精确对应计划，无成功同摘要重复发送、成员重复或串组；抽取各包尝试3/1/3/1，均
在首次加三次额度内。成员空shared字段截至该快照没有出现，不能声称真实触发v12
修复。按首发送至末回执，抽取22.21分、review19.89分、locate28.11分；关联仍
进行中，不能以批数下降宣布全流程明显提速。证据`execution-audit-latest.json`。
内容方面，独立核验能识别目录适用性遗漏和替代义务弱化，部分关联提案及关系已
补充正确边界，但尚未joint，不能算最终生效。关联按既有合同不直接改写错误事项；
无amendment不等于没有下游修正作用，也不是新程序缺陷。原台账措辞更正仍走显式
修订，最终大纲/正文须对照原文与已接受解释检查；当前只确认执行和恢复基本符合
设计，内容收敛及预算校准尚未达到完整验收。03:30:06已推进37/42批，无新增失败。

03:37:31 UTC：关联42/42真实发送均首轮完成，159成员精确覆盖一次、零重复遗漏。
实际耗时74.732分钟、1900323已知Token；旧轮78.59分钟/2097250 Token，但逻辑组
和语义台账不同，不能将全部差异归因于v12。42份原始回包均无成员shared_findings
字段，本轮没有触发精确空数组容错分支；该分支隔离回归通过与真实全流程无相关
错误分开陈述。随后joint17批/23成员开始执行，最紧请求估算余44025上下文。
证据`associate-completion-audit.json`，大纲仍未开始；分析完成后自动推进预览大纲，
不代用户提交人工确认。

03:51 UTC重新查询进度发现最新快照停在03:46:48（joint12/17、2在途），不是实时
状态。随后核实原验收PG/Redis于03:47:16退出0，测试容器03:48:59退出255，执行
已经中断；原因待查，记BUG-091。三个原容器及持久挂载仍在，CAS758文件保留。
当前仅准备恢复原三个验收容器、原数据库和同run，不重建或重复成功调用；在途
请求远端结果未知，须按原持久回执/额度恢复。大纲和正文尚未开始，不能报完整通过。

用户确认继续测试恢复后，03:56:26 UTC已重新启动原三个验收容器及同一冻结API/
gRPC/Worker/dispatcher/driver。原数据库、run与command不变；本轮private中的
PG/Redis端口随原容器重新分配调整为32768/32769，Office origin也对准恢复后的
实际容器/网关；配置备份与激活回执保留，模型和协议不变。
03:57:47核对租约重新有效、原command execution_count/delivery_count各2、失联
计1次；旧2条sent自动记executor_lost/uncertain，现90份完整回执+5条未知用量。
原成功包尚无重发，新模型调用尚未产生，正在回放原回执恢复计划；重试额度没有
重置。证据`recovery-activation.json`、`private.before-recovery.json`与恢复前快照
`recovery-run-before.json`；尚待验证剩余核验真正继续及最终输出。

03:59:37 UTC同任务恢复回执审计通过：原90份完整response_ref逐一不变，成功
digest重发0；新增2次joint的目标恰为中断两批4个成员，持久尝试数均2，没有重置。
旧2条发送保留为executor_lost/uncertain。04:00:27已实际接受14/17联合核验，
证明剩余任务恢复推进；5条完成但未知usage单列，另有2条在途不能当未知账单。
证据`recovery-receipt-audit.json`。这仅确认续跑和复用边界，尚无大纲/Word最终验收。

04:02:16 UTC：joint17/17完成，分析进入waiting-confirmation；433事项、356待核
问题，未提交人工确认。分析从01:10:37至04:02:16墙钟约171.65分钟，包含环境
退出/恢复等待，不能把该时间直接当纯模型执行耗时。总100物理调用（含6次追加
尝试）、4463816已知Token、5条完成但未知usage。原成功回执恢复复用检查通过，
大纲于04:02:17自动提交，新execution为
`web-outline-40e0015ab3a0eacecb76e246a4a429a139028baa176028c48627870bc7dcf1d9`。
分析输出与复核台账在新根output/；仍需核查最终语义去向，大纲、正文和Office尚未
通过本轮验收，不能以恢复成功代替完整文件通过。

04:22:56 UTC：本轮大纲source5/5全部首轮接受，实际601438 Token，开始assemble
归并。5包规划覆盖1991个唯一来源、零重复，输入估算86090～92126，保留原2048
纠正余量；这是准入及来源覆盖证据，不是内容已全部正确。当前尚无最终candidate，
已准备真实来源对照审阅，候选完成后及时放行草稿正文，不替用户确认事项。

04:26:56 UTC：assemble首回完整JSON因四个全局字段错放chapters[0]被拒收，
已在原额度内发起第二次纠正；五个source回执保持不变、不重跑。该非空错层不能
套用成员空shared冗余处理，记录BUG-092，原回包及四处真实反馈均保留。
当前最终candidate尚未产生，语义审阅和正文仍待完成。

04:31:48 UTC：第二次assemble已接受，五份source未重跑；大纲共7次物理调用、
867738已知Token，source至归并墙钟29.53分钟。新candidate为57章、121目标
（37 form/48 narrative/36 attachment），商务不含报价、技术、报价三卷；身份及
目标关联机械检查零错。基础内容审阅确认附件3/5不适用有显式排除，诉讼说明及
法律文书位置保留，★整段和强制定制承诺、独立SD-WAN证明在用途与来源中保留。
尚未据此宣布技术参数表逐行完整或最终正文语义通过。原候选及审阅证据在新根
`output/outline-candidate.json`和`acceptance/outline-candidate-*`。

04:35:14 UTC：根代理完成基础审阅后释放验收脚本停点，正式API保存原候选修订
并启动正文执行
`web-document-84b96439e0139286e279619b82fcad8962c9f1e649b7a7cb295cfb9fc4304243`。
未手工修饰候选，未提交业务人工确认，confirmation_ref仍为空；无企业知识资料，
应答及报价继续留空。接下来核查实际生成Word的原技术行、依据措辞、表格与留空，
不以大纲通过替代内容和Office验收。

04:40:17 UTC：13次写作物理调用已全部成功，无合同或传输重试，开始内容复核。
已抽查12份写作回包，使用185份招标来源支持、未使用企业资料，cell_fills和
attachment_ids均为空；没有代填姓名、签章、报价、“无诉讼”或“完全响应”。
这是回包抽查，最终Word中的技术原要求保留、依据措辞、原生表格和Office保存
仍待验收；证据`acceptance/authoring-response-review-latest.json`。

写作规划补核：121目标对应108初始任务，预算拆分后112单元；21个符合写作路径
的候选中仅13个可准入，另8个单目标完整输入超限，未发送模型并走预算占位。
涉及w5/w16/w20/w62/w65/w104/w105/w110，包含卷级说明、保证金、资格资料、
商务评审和有效期/完成期限/付款方式。其余为12原格式保留、39缺允许依据、
4人工填写、36缺证明原件；这些路径不能混为一类。实际13次写作全部成功不等于
112单元实质生成完整。准入重建在`writing-plan-audit.json`，最终units及输入极值
仍需对账；本轮作为BUG-040同类容量缺口记录，不擅改运行预算或截断原文。

用户随后确认本轮基础设施退出是其重启服务所致，BUG-091根因已据此更正，不再将
该中断当成未查明系统故障。继续定位上下文膨胀、关联耗时及已知摘要错误的下游
传递，真实结构复核与文档导出不为根因分析暂停。

04:52:18 UTC：本轮新Word及ZIP已下载，正文执行17.04分钟，实际27次调用全部成功：
13初轮写作、1次w4局部内容修订、11初轮内容复核、1次修订复核和1次真实结构复核，
1872728已知Token；结构调用244.47秒，保留10条待核finding，不将其全部当确认缺陷。
112单元实际为5 generated/107 placeholder，8个ModelBudgetError逐项与规划对账一致；
无企业资料的正常留空、保留原格式、人工填写和预算失败分列。全流程134次发送
（129完整成功、5未知用量），7204282已知Token；分析开始至下载墙钟221.68分钟，
含服务重启恢复和验收审阅等待。不可变result为`6e39f688…`，Word为`8b908684…`，
文件在新根`output/generated.docx`、`output/package.zip`，原分析仍waiting-confirmation。

最终机械验收：57章/121目标书签均保留；23原生表855单元格及合并与原件逐格一致，
562空白格未代填，总稿/三分卷/ZIP哈希核对通过。433原事项完整进入交付底稿，不等于
正文全部安排；正文对应263个不同事项，332次要求展示、718次引文均与当前记录一致。
56依据表的企业应答列全部空白，报价/企业证明未编造。实际语义审阅仍确认BUG-093
跨页原文展示缺口和BUG-095双目录；i15/i36错误摘要保持待确认，合法correct出口
已存在、本轮未触发，不能误报joint已接受更正被丢弃。审阅报告位于acceptance/。

深入根因与当前代码评估：8个超限请求按字段精确去重均删除0条；保留全部原文、
仅停止第二轮关联扩展的因果对照使输入从96312～161875降为31221～80687，均低于
94208准入。该对照不证明新增关联语义都可删除。关联阶段双槽利用率99.07%，
42请求全部首轮成功，completion中89.86%为供应商报告reasoning；不存在明显宿主
调度空等，仍需解决任务及上下文负荷。当前工作区与冻结版本对照后，预算/结构修改
主要统一authoring_structure任务和提示加载，未改变容量公式或写作扩展逻辑；相关
37项隔离回归通过，不据此声称8个容量缺口已修。证据为`stage-efficiency-causal-audit.json`
和`acceptance/writing-context-causality.*`、`current-budget-structure-review.md`。

真实Office验收：首轮保存因本轮隔离API缺本地NO_PROXY超时，原编辑已保全为recovered
版本，首次失败未冒称成功；仅修正该API进程局部环境后，第二轮实际打开/定位/输入/
显式保存/下载校验通过（1项浏览器验收），真实回执saved、178书签完整。标记版和恢复版
独立保留，最终选回原清洁生成版本、标记0，会话/key/pending/save_error均为空；原Word
字节及模型调用数未变化。通用启动配置未改，细目见BUG-094及`office-final-audit.json`。

当前未提交代码评估补充：恢复、客户端、布局与复核113项隔离测试通过，加前述预算/
结构37项共150项。默认沙箱测试两次在asyncio Runner清理阶段挂起，均中止且未算
通过；禁用自动插件、显式启用pytest_asyncio后完成113项，未据此确认业务代码缺陷。
指定持久化、旧入口移除与PAGE字段改动只读审查未发现新的已证实恢复/付费围栏
或跨版本错误；这些清理不等于业务语义和容量问题已解决。
最终10条结构意见逐项对照后，确证双目录角色混入、技术规定次序不符；报价卷混入
评标算法/修正规则为成稿质量问题。E表适用性、可选附件及身份证放置仍待核，A.2
双表、第三偏差表和第二组资信表头丢失未复现，不按10条确认缺陷统计或机械补表。
详见`acceptance/structure-findings-triage.md`，BUG-044/054/093/095同步保留准确边界。

用户随后指出正式Word各章被大量表格遮盖，新增BUG-096并纠正成稿验收口径：
81表中56张为宿主统一插入的要求分析表，23张为原生模板表，2张为模型生成的
报价修正规则表。56张分析表含62095可见字符，占全部表格文字96.8%；技术无损
不表示这些分析内容应进入正式投标文件。正式投标函正文前叠加目录、商务总章、
投标函本章3张分析表，共36条数据、6568字符；本章分析表15行含表头，商务总章
22行含表头。正文在Word body47～82，13条与署名空位均与原函完整对应；原PDF
为物理75～76页（冻结unit使用page:74/page:75零起始索引），不凭XML推定Word
实际页码。发现时生产导出逻辑尚未修复，不能把先前机械检查0错解释为正文内容正确。
证据`acceptance/document-table-placement-review.json`，正确边界是正式正文与
要求/证据侧栏及编制底稿分开，真实技术响应表等业务表单仍须完整保留。

BUG-096导出修复：字段字典先废除旧展示合同，再以反例确认旧实现失败，移除共享
章节渲染中的分析表前插。正式Word保留原格式、写作内容、真实表格和空位；要求
移至既有可读待办，按最终大纲展示章节/封面、提交约束、候选/规则排除和未安排
要求。候选排除不等于整条关联规则不适用，本轮2条规则同时关联被排除候选及正式
章节，其各自用途和理由均保留。没有新增协议、工作流节点、模型调用或生成门禁。
相关139项隔离回归通过（55正文/底稿/确认/布局、46生成链/表单/编制边界、38导出/
书签/编制v2）；含asyncio的默认沙箱执行挂起后已中止，正常授权环境禁用自动插件、
显式加载pytest_asyncio完成，不将挂起计作通过。

同一result、大纲、112写作单元和433条要求已用修复后源码独立重导出：
`reexport-bug096/generated.docx`、`package.zip`、`submission-notes.md`。
导出源码SHA`590feac299b9f947df4a05f84814b75c52b05f12a06944bf5d5acee033ec4af1`；
新Word SHA`e3085eb3d836e24528e9d4ba124e97de0a7a0364b5e04bf2adb4bdbf89bb6c2f`，
62476字节，81表降至25表，56张自动分析表为0；剩余25表XML逐一相同，178书签不变。
独立机械验收确认总稿57章/121目标、三分卷、23原生表855锚点单元格/562空白单元格
及合并保持。原函13条和签章空位仍保留；标题后原有空叙述任务占位仍存在，不宣称
标题后立即就是函文。该占位来自缺知识的独立声明任务，未按标题或表格外观删除。
要求底稿452184字节、SHA`9433504e6c3b48369e8e4414d2f8c22e7e310eef2b56465ac67a405f832f6574`
与原包逐字节一致；新notes589513字节。旧result、大纲、112单元、DOCX/ZIP均未变，
重导出期间禁止网络，模型0调用，没有DB/API写操作或Office当前版本替换，共享
Worker尚未部署新源码。证据`reexport-bug096/reexport-audit.json`、
`quick-comparison.json`及`acceptance/reexport-mechanical-review.json`。
独立可读底稿验收通过：433/433规则的全部非空语义值（正文、主体、范围、条件、
例外、量化、评分、后果及提交细则）及783/783引文，逐条核对65个章节/处理区域；
旧56张分析表332行的语义和引文均有可读出口。i15/i70的候选排除与实际章去向同时
保留，confirmation_ref仍为null，底稿保留待确认提示。原投标函958个规范化字符、
13条及签章空位完整。证据`reexport-bug096/independent-notes-acceptance.md`及同名JSON。
双目录、技术续文覆盖、原摘要缩限、章节排序及写作上下文预算问题仍按原记录保留，
本次去掉分析表不代表这些内容问题已经解决。

BUG-095跟进：用户指出“1 目录/附件1至10清单/本部分内容待补”仍不应进入正文。
原PDF物理74页为第六章投标文件格式的组成索引，source第4批已在purpose中说明
用于确定组成及顺序，却选成form及12个文字部件；assemble以t71/p4～p15继续采用，
最终为chapter-2/target-1。纯文字form无网格的短路产生空w2/placeholder，导出
既复制索引又追加待补；这不是企业资料缺失或模型写作失败。结构复核曾报
toc_volume_order_mismatch，只形成意见，不会自动回修大纲。旧source/assembly提示
没有告知宿主已生成自动TOC，实际用途与实例化的合同边界不足。

先更新问题记录及字段字典，再向来源、组装、结构复核三个提示各补一段职责说明；
说明性清单用constraint_range_ids/submission_refs保留，不无条件实例化为form，
误建候选沿既有excluded_targets处理。明确要求提交指定目录格式时仍保留真实实例，
不新增目录字段/节点、标题规则或门禁。48项相关隔离回归通过；新用例串联source、
assembly、structure、要求快照及导出，验证说明候选排除而真实表格/空值仍保留。
此为既有引用及出口回归，不是新提示已通过真实模型语义验收。

本次依用户已明确的用途裁定，复用apply_outline_edit形成独立修订预览：精确移出
chapter-2/target-1及仅绑定它的空w2，保留全部实际附件和原规则/来源；该规则及两条
来源原已在约束/提交来源中，仍保持75个constraint_rule_refs、93个submission_refs。
其余56章、120目标的身份/内容及111个unit/CAS不变；preserve_requirements重建
target_ids后433条(rule_ref,rule)逐一相同，底稿新摘要变化只来自关联位置更新。
最终文件在`reexport-bug095-user-revision-v2/generated.docx`（62251字节）、
`package.zip`、`submission-notes.md`。相对BUG-096稿仅移出14个索引/占位节点和
2个对应书签，自动TOC保留；25张剩余表XML、投标函及三分卷实质内容不变，55处
标题编号及4处宿主参见编号随实际位置更新。4条精确绑定旧位置的局部待补移入审计
retired_gaps，25条原结构观察保留并注明修订前、尚未重新复核，不宣布其他顺序/
覆盖问题已经解决。原版、BUG-096稿及首版预览均未覆盖，模型0调用，导出禁网络。
这是人工裁定后的独立文件预览，未保存正式API修订或替换Office当前稿；共享服务
未部署新提示，新提示的自动分类尚待真实模型验证。审计为新目录中的
`revision-preview-audit.json`、`revision-content-comparison.json`。
最终独立验收通过：相对BUG-096稿，仅移出14个索引/对应占位节点，剩余525节点在
排除55处标题号、4处宿主参见号及书签序号后XML完全相同；总稿自动TOC保留1处，
178至176书签恰为该章及目标两处移除，三分卷同规则核对通过。原函958个规范化
字符/13条完整，25张表XML完全相同；433规则语义、783引文在notes仍逐条可读，
确认状态仍为空。原章/模板/空w2及用户决定在审计中可追溯，旧文件SHA不变。
详见`reexport-bug095-user-revision-v2/independent-revision-acceptance.md`及同名JSON。

## 当前真实全流程（2026-10-28）

15:06:05 UTC已启动新版整份文件真实流程：run
`900c52eb-302f-42e1-8304-4b2a16ff5031`，项目沿新隔离库中已验证上传的
`9536e332-d533-50dc-8095-456ec262a94a`。使用独立runtime-full（154文件）、协议11、
最新verify条款范围提示，grok原模型/131072上下文/并发2不变。免费预审仍4抽取包，
范围数165/210/271/150、输入估算110478/109701/109320/64061，完整来源全部保留。
只切换该新任务的grpc/worker/dispatcher，旧运行及共享服务未动；实际进度以新证据根
`flow-state.json`、`monitor-latest.json`和logs为准。15:06:19 OCR一成功一在途，无重试。
外部验收脚本会在大纲产出后暂停供根代理检查，不增加产品门禁或让用户再次授权。

15:21 UTC：前两包直接通过，第3包完整JSON仅含schema_version/items，缺其余三个
必需数组，已沿原额度反馈重试；第4包仍在途。供应商DONE、stop、completed均正常，
不是流截断。该回包142事项暂不计入已接受台账，完成数仍2/4，实际合同重试1次。
这是BUG-077已知回包完整性问题再现，未热改快照或重复执行成功包。首包已在实际内容
核对中保留目录“不适用”信息；最终核验与成稿去向还需检查，不能提前关闭BUG-079。

15:27 UTC：抽取已3/4。剩余包第一次纠正收到供应商api_error，明确为供应商到Grok
的中途ECONNRESET；没有DONE/usage，保留为uncertain并单列未知计费。目前同包第3次
发送在途，仍在原首次加三次额度内，已接受3包未重跑。去敏传输审计在新证据根
`provider-stream-error-audit.json`，不归因于本机连接、Key额度或上下文超限。

15:31:25 UTC：4/4抽取全部接受，最后包第3次发送成功，推进独立核验。抽取含两次
追加尝试约25分钟；累计7个成功传输和1个uncertain，已知458999 Token，未知usage
仍单列。未复跑已接受包；此时核验计划及后续调用量尚未确定。

15:52 UTC独立review11/11完成，均首轮通过，进入13次locate（6 discovery+7 reference）；
15:57 UTC已完成2次locate。最终抽取421事项/109关系/53issues，796原范围各一次处置
（items520、non_item275、unresolved1）。定位计划对421事项的88410对组合无遗漏，
只说明计划覆盖，不能当语义已通过。累计已知1284288 Token，仍1笔旧unknown usage。
真实抽取及11份review内容核对见`content-acceptance/observations.md`；目录适用性与
30处技术原文重点已保留，条件边界、非源流程文字及局部评分依据不足仍单独追踪。

16:19:29 UTC：13次locate均首轮通过，实际进入45请求的associate合批计划，2个在途。
累计已知1925813 Token，仍1笔抽取阶段unknown usage；逻辑组/候选数及joint数量待
本轮计划与实际回包核对，不把旧台账的192组/55批直接套用到这个新运行。

实际关联为163逻辑组装入45请求（33×4、8×3、3×2、1×1），目标每组一次、无遗漏
或重复。计划输入855247，最紧请求含输出预留/安全量总130487≤131072；全部准入，
预留不等于wire输出硬限。17:37:38 UTC关联45/45批完成、163/163成员结果保存，实际
47次调用含2次仅失败成员纠正。16:19:03起墙钟78.59分钟；prompt1000050、
completion1097200（其中reasoning993906）、total2097250。旧关联阶段117.60分钟，
但新旧语义台账不同，不能把全部差异归因于合批。联合核验16/16批首轮通过，
17:37:47→17:49:41墙钟11.90分钟，prompt458910、completion187129（reasoning185423）、
total646039。17:49:47分析进入waiting-confirmation，整体163.71分钟、95次发送，
累计已知4669102 Token，仍1笔抽取阶段未知usage；421事项、351个人工问题待核对，
未模拟人工确认或宣称analysis-ready。当前已启动预览大纲，结束后按约定暂停审阅。
审计在`associate-plan-audit.json`；阶段收齐后才汇总共同反证和正式成员产物，中途
成员stage暂未全部落盘不表示回包丢失。

17:17首个成员回包把批次顶层专属的`shared_findings: []`放入成员，触发
`extra_forbidden`。成功兄弟先保留；纠正只target坏成员，完整公共依据及原子额度
身份不变。17:22:10该成员纠正已接受，坏成员共2次尝试、成功兄弟仍1次，没有整批
重跑。纠正耗时304.6秒，prompt58125、completion35136（reasoning31555）、total93261；
真实局部恢复证据在`member-retry-audit.json`。该恢复证明成功兄弟未重发，不代表机械
格式错误没有成本，也不能用请求压缩比例代替实际阶段用时。

第二次同类空数组错层在17:28:26完成仅坏成员纠正，另3个成功兄弟均保持1次；两次
恢复合并证据为`member-retry-audits.json`。协议12源码对该精确空字段的规范化已完成
离线回放，未混入本轮冻结协议11。阶段用时/usage以`phase-usage-comparison-latest.json`
为准，未结束阶段仅是截至观察时的部分数据。

47次关联预算已按冻结代码和实际model-wire逐一离线重建，请求摘要与原预算各项一致，
完整messages独立o200k计数+512也一致。实际prompt累计923834估算→1000050，
高8.25%，单次最高高17.498%；2次按实际prompt加原reserve/safety分别超配置窗口
1742/1342，但所有实际prompt+completion均未超窗。当前对非图像请求无条件启用
o200k的校准适用范围存在问题；供应商分词与中转附加内容仍不能仅凭回执区分。
证据在`budget-calibration-samples.json`，此次未加系数、修改运行或新增付费调用。

BUG-085共享写作单元书签修复已完成源码及隔离验证；分析runtime-full保持原样。
大纲暂停且本库两类模型请求在途均0后，根代理明确授权18:24:15切换独立
runtime-authoring：154文件中仅已验证的`authoring_export.py`变化，其余153逐字节
一致，normalization仍11。仅该任务grpc/worker/dispatcher重启，API/旧运行/共享服务
均未动；旧快照、配置备份及两份manifest保留。根代理完成业务/机械大纲审阅后，
18:26:30已单独放行正文；不能将旧稿重导出检查当作本轮成稿验收。

18:12:12 UTC大纲source已5/5接受，第5包首回被宿主拒收后仅该包纠正一次通过；
source共6次物理调用，已知720031 Token。冻结
`_organize.plan_sources`与本执行固定context离线重建为5包，输入估算
92158/91942/91421/92134/85466，421条规则各归属一包，1991个源range无重复；
各source包摘要和实际首轮wire一致。source请求本身不带operation字段，不能误认整份直接
生成。18:12:13 assemble已真实发送，输入88728、含输出/安全预留125592≤131072，
wire354527字节；18:19:07完整回包首次通过，未走单根兜底。18:19:11大纲完成并停在
`OUTLINE_REVIEW_READY`，共29.39分钟、7次物理调用、854227 Token，零在途。产物有
5根/55章节/118位置，商务、技术、报价3卷；仍99条outline_rule_unassigned、24条
outline_template_missing，内容验收待审阅，不能以树结构已生成代替业务通过。
证据为`outline-source-plan-audit.json`及`outline-execution-latest.json`。

第5包首回在16个findings中多交非空range_ids，OutlineFinding合同不接受该字段。
BUG-081叶级反馈只带这16处路径/短片段，反馈2687字节，实际wire332499→335637；
输入85466→86213（+747），在2048纠正余量内，18:07:37真实发送成功进入生成。
前4包各一次且accepted引用保留，未重跑；纠正实际274.79秒，prompt92232、
completion29712（reasoning19805）、total121944，最终已接受。完整审计在
`outline-correction-audit.json`，本轮未热改合同或删除非空来源信息。

归并耗时414.27秒，prompt86331、completion47865（reasoning33185）、total134196。
输出超过32768预留，实际总量比配置131072高3124；供应商完整成功，未证实供应商
自身窗口超限或截断。此前“实际总量未超配置窗口”只适用于关联与定向样本，不能外推
到整轮。完整请求独立重计/摘要核对在`outline-budget-audit.json`，阶段结果在
`outline-checkpoint.json`。外部approval记录说明35条核心技术要求已有响应位置，99条
未分配多为流程规则但仍保留待办；i45/i108等未决项要在成稿核对展示状态，不能当作
语义已解决。该记录只解除本次外部暂停，不构成人工业务确认，分析仍waiting-confirmation。
18:26:33正文执行`web-document-4337afe7256c70b35932687180eeab30393cf342da2aecd13bdeaf7eef65036b`
已提交，18:42:24完成生成及HTTP下载，正文15.83分钟、31次调用、2037812 Token，
无合同重试。18次write含w107“技术条款偏差表”一次核对驱动语义修订；13次review
含修订后复核，不能把这次修订误报为重试或只统计初始17次write。原稿DOCX107837
字节、ZIP537641字节，v1为`c2da4346-1966-465a-a640-bd8037b2ed96`，原实物保留。

原稿机械检查通过，但业务审查发现w103备品备件正文遗漏大纲已知的附件B不适用边界，
来源传递根因另查；待核标注不能替代语义修复，也不宣称内容验收通过。根代理授权后，
18:46:30仅本任务Python角色切换runtime-final-export，仍normalization11，154文件中
仅`authoring_export.py`从BUG-085版变为BUG-087待核展示版（SHA a91edee167912b5d16b6f16d40d87500fde3068ad88d86915fe47cbb8b5e8985）。
旧快照/原稿/API/共享栈/.env不动。

正式API使用同固定outline、空资料及previous_execution_id创建独立执行
`web-document-9c57b8a868642fc565eb3cafc183dba874a0161b6a74982faa367fc59e863603`，
18:47:56成功；实际SQL模型请求0行，31份accepted CAS全量继承（含追加write及再次review），
不是仅据代码推断0调用。18:50:53新稿下载到`final-export/`，DOCX108107字节、
ZIP538264字节，v2为`f11523f4-d620-47e7-9d50-f8f18e53adc1`。
新生成稿按产品设计不会自动替换当前选中稿，下载使用明确v2版本；验收脚本原先把
current当最新的断言已纠正。新稿机械检查及仅标注差异比较通过，分析仍
waiting-confirmation，未伪造人工确认。

18:53:52通过现有选择API将v2设为本次验收稿，随后真实Office浏览器用例通过
（1 passed，22.6秒）：打开、书签定位、实际编辑、显式保存、下载及保存后定位全部
完成，未伪造回调或清pending。v3为`d95fc5fa-f61f-4823-a59f-be5e80a44901`，
下载SHA 995f9003369ff631b4faf3880509885449cfa5e46b1499adc855e63c564a6a78，
XML确认测试标记恰一次、173书签保留；会话已关闭且无pending。带测试标记的保存稿
单独存于`real-office-final-export/`，清洁v2仍在`final-export/generated.docx`。
三阶段共133次物理发送（含2次OCR），已知7561141 Token、1笔未知usage；final-export
新增调用0。总证据在`full-flow-acceptance-status.json`。运行/编辑链路通过，w103等
内容缺陷尚未修复，不因此宣称内容验收通过。

BUG-088最小提示修复与同类前驱恢复经44项来源引用回归、19项隔离PG/Worker回归后，
根代理授权真实复测。19:16:25 UTC确认两类模型请求均零在途、无运行/排队任务后，
仅本任务Python角色切换runtime-scope-fix：从runtime-final-export的154文件冻结，
只覆盖`web.py`、`document_execution.py`和`outline_assemble.md`，其余151文件逐字节
一致，仍normalization11。19:17:19正式API提交新大纲
`web-outline-73cb45498ed977b66a4be973913604f353eed1684cf07ae498bbea511a9d751a`，
沿原模型/run、指定旧大纲前驱。19:25:53新candidate完成并暂停审阅：五份source
accepted（含原纠正后回包）原CAS全部匹配继承，实际新增仅1次assemble、零重试，
两类模型请求零在途。归并468.615秒，执行墙钟514.326秒；prompt86430、completion
55058（reasoning39798）、total141488，比配置131072多10416；供应商完整成功，不能
称预算校准通过。证据在`scope-fix/`及其`inheritance-and-usage-audit.json`，新candidate
已交业务/机械双审，未保存新大纲或启动正文；旧分析仍waiting-confirmation，当前
Word版本、旧执行全字段与回执均未变化，.env及共享栈未动。

随后业务审查确认上述candidate仍未修复BUG-088：备件target继续只选t79，未绑定
实际限定来源；正式写作未启动。来源直绑最小合同经62项回归、完整真实请求离线回放
及独立审查后，19:47:25 UTC根代理授权仅本任务Python角色切换runtime-source-binding。
基于runtime-scope-fix仅改schema、authoring_pipeline及outline_assemble三文件，其余
151文件一致，仍normalization11；切换前零在途、零running/queued。19:47:45正式提交
`web-outline-489183e56d8d1dd0b02ac1a804403e50ee9171e69f1703cfd92fd209871c1d2e`，
以scope-fix为前驱，沿同run、原模型。19:54:38新candidate成功并停止在双审：5份source
原CAS完整继承，实际仅1次新assemble、零重试、零在途。真实wire与完整离线回放逐
字段一致，input估算88998；归并363.254秒、整次412.452秒，prompt86591、completion
50307（reasoning36824）、total136898，仍超配置131072共5826。原始回包、accepted
回执、candidate和继承/usage审计均在`source-binding/`，两名审查代理已启动，尚未
保存新大纲或生成新正文；旧执行和当前Word保持不变，分析仍waiting-confirmation。

source-binding候选随后因遗漏必需诉讼说明等业务退步被拒绝，未采用其章节树。
根代理改为保留清洁v2全树，由明确agent语义审阅仅追加B原文；不是模型自动修复成功。
40项来源编辑、23项结构投影回归及真实完整请求回放通过后，20:13:03 UTC仅本任务
Python角色切换runtime-reviewed-fix，从runtime-source-binding只覆盖outline_edit、
authoring_structure、authoring_view三文件，其余151文件一致、normalization11不变；
切换前两类模型请求均零在途。正式API基于旧revision
`981ba5d9-2b08-4315-9865-4110708ae792`保存新revision
`0ba8084c-31c6-4287-aefa-638ca871d361`，逐字段核对仅
`chapter-3.16:target-1`增加`section:227:page:64`的完整SourceUnitRef，诉讼位置、
其他目标、树、规则和模板均不变。20:13:56以清洁v2执行为前驱提交
`web-document-09d27526185a3e5077113d9ab4fdafd35a89f7928a586cb3cdab4215583cc09a`，
证据在`reviewed-fix/document/`。离线已证29/31正文请求摘要可继承，基础新增为
w103写作、其内容复核批、首次可准入的结构复核；实际新输出/语义修订以真实回执为准。

20:19:47新稿成功并按明确版本下载：清洁新version
`3db2a089-3900-4c53-9e39-6364915e1de4`，DOCX108282字节，SHA
`c3773a84376741842f80ad037478bd7ee53517b1dbbd3f7be961c0195c6a1ac9`。
实际29份accepted原CAS继承，3次新调用均首轮成功、零额外语义修订：w103写作59.135秒/
45113 Token，内容复核44.374秒/79287 Token/零finding，结构复核228.249秒/115513
Token/8条finding。总351.243秒、239913 Token，零在途；结构完整执行不等于零业务
问题。新w103实际3段、9个supports，已包含B原文；独立审阅确认其余113个写作单元
原CAS不变、诉讼留空位置保留，30个技术入口与10个评分因素仍有依据，未编造企业
资料或报价。`execution-result.json`、`actual-units.json`及明确新版本DOCX为本轮依据；
此时`workbench.json`的context仍按当前选中旧Word指向旧稿，保留原响应，不冒充新稿
正文。机械审阅以实际新result重建上下文，55章118目标、3分卷、23原表855格562空格
均保留、无自动填空；主稿173书签及分卷书签顺序与清洁v2一致。结构真实请求与完整
离线请求唯一差异为`units[102].blocks_count: 2→3`，来自新w103实际正文；Prompt和
其他请求字段不变，详见`structure/request-diff-from-baseline.json`。

20:28:11 UTC新稿真实Office验收完成，1 passed（用例21.6秒、总24.3秒）：打开、
书签定位、实际输入、显式保存、下载SHA/XML核对及保存后定位均通过。测试标记版
`ad3bc721-4539-431a-b572-d77eee175964`单独保留，marker恰为1，173个书签；已通过
正式close接口关闭会话，当前选择恢复清洁版`3db2a089-3900-4c53-9e39-6364915e1de4`，
下载SHA仍为`c3773a84376741842f80ad037478bd7ee53517b1dbbd3f7be961c0195c6a1ac9`，
marker为0。editor.key、pending_save_id和save_error均为null；本轮Office无模型调用。
证据在`real-office-reviewed-fix/summary.json`、`browser/`及重新获取的
`workbench-clean-selected.json`。分析仍waiting-confirmation，8条结构发现及B范围
解释待核仍保留；Office通过和本次定点修订不代表自动归并或整稿语义已全部合格。

旧稿Office真实测试已打开、定位并输入；因隔离API此前未绕过本地代理，保存回调下载
失败，原缓存地址后来410。保留原pending/失败证据，没有伪造回调、清pending或声称
保存成功；后续使用新稿和已修正的隔离环境验证正常保存链。当前full使用原冻结API
二进制，最新Rust静态CORS修复仅已做路由回归，真实浏览器通过修复后的preview加载
插件；不能称最新API已发布。旧运行、共享服务与实际.env均保持原样。

### 真实定向对照与验收新增问题

版本11第二轮定向已于约14:58 UTC完成：同样4A+3J，12组完整结果、4个joint结果，
0拒收/重试，0在途；墙钟504.896秒（8分25秒），prompt315144、completion112679、
total427823，对应旧16次的653437减少约34.5%。run为
1c0a53f5-550c-4818-9f99-64a93d63a181。真实A/J卷章引用分别始终为r165/r168，短号
误读未再发生；语义逐组检查报告在`targeted-v2/`，完整新流程仍待开始。

大纲修复已完成源码及隔离验证：551项广泛用例通过，6个失败中5个是新参数/错误路径
的夹具更新，1个为冻结旧代码也存在的矛盾断言；针对性34项复验及新增重复坐标保护
通过。归并叶路径另11项先失败，修复后62项通过，批次有重叠不累加。真实旧归并输入
压缩后93441/94208；22个网格文本/空位/合并关系往返一致，11类叶级反馈均只增加56–79，
最大93520仍准入。极大新输出仍需实际准入，不把单例容量当成任意规模保证。

14:32 UTC完成首轮真实定向A+J：4个A批、3个J批，12逻辑组/13候选全部有正式回执，
没有重试或拒收，已确认在途请求0。总耗时543.6秒；对应旧16次调用变7次，prompt
457282→313992，completion196155→129343，总653437→443335，usage全部已知。
新run为269dec68-0a62-4cf0-acaa-35937a268569，证据在新根`targeted/summary.json`及
`comparison.json`。该run是定向局部验收，不伪装完整analysis-ready。

逐条语义复查发现BUG-082：A说明里的局部r号在J变号，产生一条假的证据不符反证；
同一提案另有真实范围疑点，所以原未接受结果仍保留，不能称该轮语义已全通过。
正在修复共同来源短号稳定性，下一定向使用独立冻结版本，旧回执不改写。

旧全流程基线于14:20:57 UTC完成生成及HTTP下载DOCX/ZIP。DOCX有28表、119书签，
但大纲归并在发送前超预算，最终只有一个“待整理内容”根章节；这是流程可继续、内容
验收未通过。BUG-081确认源纠正粗错误路径复制整树和全量assemble超限两处根因。
源纠正已完成51项针对性检查，反馈13860→241字节；同文件按2048纠正余量规划仍6包，
全部准入。全量归并紧凑视图源码已加入，完整网格/原文不截断，隔离回归及真实复测待完成。

旧Office实际打开后因插件公共静态文件缺CORS而无法定位，尚未保存；BUG-083记录
生产静态服务和preview的同一问题，修复范围仅导航插件路径。不得把DOCX可下载或
编辑器打开当作编辑保存验收通过。新完整运行尚未提交，等待这批修复冻结并完成定向。

### 关联合批源码与隔离验证

normalization_version=10首次接入真实TenderNodes，11补共享来源短号稳定性：associate共享输入、逐组接受，joint保留
全部实际依据，明确共同反证可影响此前成功兄弟组；初次加三次按成员在现有artifact台账
原子预留，未发送失败也累计，重启先恢复原物理回包。没有新增业务表、节点或Agent框架。
预算规划及发送同算最多4组、max(32768,24576*k)；CAS回读和目标缩小均不改变公共短引用。

源码正式离线预审仍为192组/195候选→55物理请求，输入估算1199901；同新Prompt的逐组
基线2653287，旧冻结基线2567798。最大总上下文130000；全部原文、条件、表格、别名、
上下文reason和候选保留，55份请求的CAS往返字节/摘要/预算一致。证据在旧证据根的
`batching-real/preflight/formal-summary.json`及相邻完整私有回放，不将输入降幅当真实提速。

隔离验证：主核验及并发86项通过；最终合批/wire/budget32项通过；恢复14项PG/模拟SSE与
网页进度/合批主流程合计43项通过。完整workflow+acceptance首轮320通过、15失败：
14个下游失败来自旧32k测试窗口与顶层单组协议，专用夹具校准后17项均已复验通过，
保留分包/超限及四次重试断言；1个Worker故障注入因测试初始broker与子进程Redis地址
不一致、Prometheus端口占用，统一测试地址/临时端口后7.93秒通过，未改生产网络。
旧非批/Store/Stream等199项另已通过；上述批次有重叠，不累加声称独立用例总量。

新运行证据根`/opt/workspace/kb-tender-evidence/2026-10-28-analysis-batching-real/`，含
`regression/`、`skeleton-retest/`、`final-progress/`、`root-unit.xml`及定向`targeted/`准备。
真实对照选择4批共12逻辑组/13候选，覆盖有决议、共同反证与长来源；原件没有补遗决议，
该类仅有隔离用例，不能宣称已有真实补遗验收。以下为定向发送前准备记录，当前真实
结果及新增问题以上节为准；新完整流程尚未完成。未修改.env。

新版非付费准备已完成：冻结155文件；专用PG库`kb_real_batching_20261028_008430`，
API36407、gRPC57619、Redis DB14，项目`9536e332-d533-50dc-8095-456ec262a94a`。
登录、原件上传/重放及下载SHA核对通过，模型请求仍为0。沿用基线API/migrator二进制，
本轮仅比较Python核验协议；不以此替代另批数据库重设计的验收。

旧冻结基线于13:34:04 UTC进入waiting-confirmation，分析约227.6分钟、283发送，
已知7941727 Token，1份未知usage；未人工确认。associate实际192次全部成功，约117.6
分钟、4546748 Token；joint实际26次。432事项保留，15决议接受、16问题解决，258新关系
仍未验证，hard_blocking=false。附件3“不适用”遗漏尚未得到正确表达；证明条件关系的
反证与未接受决议留在未决中，不能称业务语义全部通过。大纲和编制的后续真实结果及
容量失败见上节；原始回执及审阅在旧目录`analysis-final-audit.md`及output/。

### 此前基线与方案快照

12:47 UTC只读快照：OCR 2/2、Extract 4/4、review 35/35、locate 20/20；associate
已保存133/192组，另2请求在途。已知累计6254266 Token；此前1份uncertain未报告usage，
3次抽取结构纠正和1次review传输重试已恢复，无新增失败。租约正常；joint、最终Verify、
大纲、正文及Word闭环尚未执行完成，不能称全流程通过。原冻结运行继续，不热换协议。

用户明确DeerFlow只参考合理设计，不采用其框架。下一批
[效率方案](analysis-efficiency-plan.md)已统一待实施字段和验收，尚未改运行代码。
离线完整192组/195候选无损重建；最多4组、按组数预留输出的暂定合批为55请求，
输入估算2567798→1317863（-48.68%）。这是associate的假设发送视图，正式字段/Prompt
落地后须重测；joint、实际耗时和新协议语义质量未验，不能外推全流程收益。
证据在本次运行目录`associate-capacity/README.md`、`output-aware-results.json`及
`observed-single-group-output.json`。127份完整单组回执的completion P95=17918、
最大24610、reasoning占92.88%；24k/组仅为试验余量，并非输出保证。本轮只更新设计
文档及离线证据，未部署、未修改.env或在途运行。

遗留复核已更新BUG-074/076：已实现的网页恢复、历史大纲、人工复核、知识版本接线不再
标作待实现；Compose补worker桥接token并让内部API默认端口跟随API_PORT，11项配置
测试通过。此前隔离回归不等于当前联合业务验收。

为同一106页原件新建专用数据库`kb_real_full_flow_20261028_f884b7`，运行当前SQLx
migrator和Python检查点初始化。冻结当前Python代码及Rust二进制，独立API端口59463、
认证gRPC端口59873，dispatcher与Dramatiq worker已启动；不操作现有业务库或旧运行。
原件摘要仍为`4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`。
本地真实HTTP已验证登录、项目同键重放、认证gRPC上传同键重放、原件下载逐字节一致；
项目为`5f0d555e-5b95-5250-aa4a-d854ed914f8d`，证据见`local-http-checks.json`。
实际`.env`尚缺`GRPC_AUTH_TOKEN`，正式Compose启动前仍需配置；本次隔离API/Python
共用独立随机凭据，未写回`.env`。该配置项不阻塞已启动的隔离测试。

免费原生解析预审确认106页、796范围、98图片位置，4个抽取包，范围数165/210/274/147；
输入保守估算110209/109432/110458/62387，均低于110592准入上限，输出预留16384。
该结果不含真实OCR，不能当最终核验计划或真实Token用量。全分析调用量仍为O+P+R+L+A+J，
后续各阶段依本轮实际模型输出规划；不得用预审4包承诺全流程仅4次调用。

当前配置为`https://code.zleiwork.cn/v1`、`grok/grok-4.7-build-fast@500k`，显式上下文
仍为131072。自动审批首次因新目的地未被明确授权而拒绝；用户随后明确答复“同意，按当前
地址和模型执行”，本轮真实HTTP分析已获准提交，后续将继续大纲及无知识资料编制。
保留原要求，企业应答及报价留空；人工语义确认不会由脚本代填。当前尚未得到最终稿，
不能声称已跑完全流程，实时执行身份和阶段见证据根的`flow-state.json`与`logs/flow.log`。

真实分析run为`622f0570-f910-41b3-b9c0-8b20ea078491`，于09:46:30 UTC提交。
截至10:07 UTC，2次OCR完成、4包中2包入账（271事项、437处置）；另外两包在执行/纠正。
三个首次抽取回包被拒收：一例新关系身份无效、两例顶层必需数组缺失，均保留回执并反馈
实际错误；前两例第一次纠正已成功。详见BUG-077。已有回执累计442924 Token，未计在途
请求，不表示最终预算；已完成的回包均正常stop，不能归因为网络截断。

10:12 UTC前4包已全部完成：432事项、796处置；到10:42 UTC，35个首轮review全部
通过，开始20个locate请求。review期间1次供应商流式api_error未给usage，回执保留为
uncertain，第一次重试成功。已知累计1476483 Token，未知回执及在途用量不计入；尚未
到最终Verify或人工确认。BUG-079的“不适用”问题尚未在首轮review中识别，i44/i45
正文获核对，但T:l21的证明条件关系被识别为混淆，继续观察后续处理，不提前判通过。

11:15 UTC，20个locate全部完成且未重试；正式associate计划为192请求，已开始执行。
此时已知累计2591920 Token。BUG-080记录逐逻辑组调用及joint整包接受耦合带来的
预算/耗时遗留；不能以BUG-078的review合批已修复宣称全流程预算问题已解决。本轮继续
已授权的冻结基线，保留全部原文/候选/回执，不靠删候选减少调用；后续还需joint和编制。

真实基线执行期间定位并修复BUG-078关系补读未合批：新源码协议9离线同台账为14请求、
346910输入估算，原冻结8计划为35请求、426725输入。前12包摘要不变，全部844来源ID、
432事项、116关系和原声明保留。92项相关单测、3项独立PG/合成五节点验收及Ruff通过。
证据为`bug078-*.xml`、`bug078-offline-plan-review.json`。本次真实任务仍执行冻结8基线，
不混用在途回执；协议9的合批尚未做真实语义验收。独立原件与Extract审阅另见
`source-acceptance-review.md`、`extract-acceptance-review.md`，BUG-079暂待最终Verify核对。

证据根：`/opt/workspace/kb-tender-evidence/2026-10-28-real-full-flow/`，含
`runtime-snapshot.json`、`setup.json`、`preflight.json`及各进程日志。该目录私有配置
含本次隔离凭据，不提交或输出；实际`deploy/.env`未修改。

## 用户归属字段收敛（2026-10-28，BUG-075）

按用户最新要求删除`kb_actor_identity` DOMAIN及六处操作者字段，不再保存user/api_key
前缀身份。只有尚未绑定业务实体的上传暂存新增`owner_user_id uuid`用户外键；内部后台
产物可为空，业务发布/撤销/主动清理始终严格比较UUID，None不跳过归属校验。招标文件
通过project_id继承项目owner，对象引用通过业务实体继承；CAS对象不设置单一用户owner。
API Key使用所属用户，原件及Word上传直接传UUID，审计/维护/删除事件只去掉无消费的
操作者列。保留既有Word版本创建用户和编辑会话owner，不扩大到工作流协议调整。

两个专用新库验证17项通过：fresh迁移1、对象生命周期8、双用户/Key HTTP1、Word保存1、
retention6；`cargo check --offline --workspace --all-targets`及diff检查通过。fresh确认无
actor DOMAIN/列、uploads UUID/可空/FK、references无冗余用户字段，同时验证迁移重放、
失败恢复及校验和。首次Word测试因测试库名称不含其要求的tender_web前缀，在进入业务
逻辑前拒绝；改用符合命名约定的另一新库后通过，未修改测试保护或操作现有业务库。

证据根`/opt/workspace/kb-tender-evidence/2026-10-28-owner-user-id/`，结果见`results.json`、
`fresh.log`、`uploads.log`、`owner-http.log`、`word-save-v2.log`、`retention.log`及
`workspace-check.log`。未部署、未改真实.env或冻结runtime；正在进行的106页真实流程
不包含此次schema修改，不能以其结果替代新结构的真实联合验收。

## 标准迁移去掉额外空库检查（2026-10-28，BUG-075）

删除首份SQL中的DO递归扫描，SQLx继续管理版本、校验和、并发锁和每份迁移事务。
同名对象按PostgreSQL正常冲突失败；不冲突的无关对象允许共存，不自动接管旧业务库。
`fresh_database_postgres`在两个独立新库先失败后通过：原代码错误地返回55000；删除
门禁后返回预期42P07，原夹具行保留、前置DOMAIN回滚、没有成功迁移回执。夹具表改名
后不删除，后续三份迁移、并发恢复、重复执行与校验和拒绝均通过，最后核对该行仍存在。
证据根`/opt/workspace/kb-tender-evidence/2026-10-28-sqlx-migration/`，见red/green日志及
结果JSON。只修改未发布的新初始化源脚本，未对已运行库重放、清台账或更新校验和；
真实106页任务继续使用冻结基线。文档与diff检查通过，未部署。

## 网页能力闭环（2026-10-28，BUG-074/076，后续修复）

已接入分析失败同run续跑，保留来源、成功付费回执及原模型工作单元额度；未知提交冻结
body/key跨刷新恢复。早期失败沿run/command的修订身份，不把来源回执的revision=0当运行修订。
结构化事项纠正已补业务表单，覆盖类型/主体/领域/范围/条件/例外/数值/评分/后果/提交要求，
证据与完整包由服务端冻结恢复；继续用既有完整纠正及重新核验合同。

大纲可手动设置独立分册和封面角色，保存到不可变修订。章节定向复核选择真实响应及理由，
提交时固定前次成功稿的大纲、来源及目标身份；新结果保持候选稿，人工Word不被覆盖。
展开复核表单导致Word网格挤缩、工具栏被覆盖的浏览器反例已修复，工作区可纵向滚动。

保存Word后的只读语义复核为独立持久任务，输入为明确version_id/SHA对应的实际保存字节。
段落、表格、页眉/页脚及注释文字进入复核，硬换行和制表符保留；未知章节、缺失书签、
不可读图片/签章、跨批一致性与最终排版均保留明确人工核对出口，不声称全稿合格。
原始分析run/source/revision与来源是否更新分别展示；新补遗不改写旧报告的依据。
解析/来源准备及分包在工作线程中进行，避免阻塞租约心跳；已有HTTP同键回放不重新读S3，
完整请求仍由Python比较，新任务继续校验真实Word字节。未新增业务表或改变初始化SQL。

本轮前端67项单测、完整87项浏览器回归、build/lint通过；Python六组最终XML去重153个
唯一用例通过，无失败/错误/跳过。Rust API常规25项通过，外部夹具6项默认忽略；本轮新增
保存稿回放夹具已独立执行1项通过，验证真实JWT HTTP Router→认证gRPC→Python及当前
三份实际迁移，覆盖同键202、变更409、缺原件新请求503、跨用户404与跨项目隔离，模型请求0。
这些门禁使用合成文件、模型/Office替身；本轮未启动新的付费模型任务，未正式部署，
也不替代上方正在进行的106页真实业务流程验收。真实deploy/.env及并行BUG-075保持原样。

证据根：`/opt/workspace/kb-tender-evidence/2026-10-28-complete-web-fixes/`。
最终浏览器记录`frontend-final.log`；后端为`correction-final.xml`、`root-bridge.xml`、
`recovery-final-v2.xml`、`saved-word-bridge.xml`、`saved-source-final.xml`及
`saved-review-execution-final.xml`，含重叠故以测试身份去重；HTTP专项为
`http-review/authenticated-http-{replay.log,result.json}`；去重汇总为`verification-final.json`。
本轮独占PG容器/匿名卷、临时依赖容器、HTTP桥接夹具和新建测试库均已清理，32792/4184
连接拒绝；回执为`cleanup-final.json`与`http-review/cleanup.json`。正在运行的真实全流程
任务及共享服务未操作；代码未提交或推送。

## 网页遗留收尾（2026-10-28，BUG-074/076）

已修复大纲连续未知保存后刷新丢后续编辑、Word默认只读刷新残留pending导致无法切换。
重试保持原body/key，恢复存储单独保留最新工作树；Word只更新已固定阅读版本的真实
会话元数据，不切换稿件或伪造本地保存确认。

大纲新增历史列表、结构预览、另存恢复及响应重排；target_ids只能选择同生成执行、
同authoring_input的原始/祖先定义，完整规则/来源/原表由服务端继承。空历史可另存，
目录失败明确显示未知，历史读取与响应目录独立重试，提示固定当前来源而非最新候选。
未指定固定大纲的任务在提交时固化authoring_input_ref，避免排队期间分析修订使其失败。

项目资料可显式选择知识资产固定版本；worker通过60秒purpose/execution受限内部JWT
回读，任务不存用户凭据，API逐次重验项目/产品/版本/原件关系。检索失败保留待补。
同时修复解压后的响应被再次解码，以及资产停用后已有同键任务被重新准入阻断；已有
任务仍经Python完整请求比较，新任务和真实资料读取继续执行当前权限校验。

招标解读新增逐项人工判定、确认、事项文案纠正与重新核验；数字/条件/证据等结构保持
原值，页面明确提示完整修订出口。未知请求保持原键，409保留输入并可对照最新版本，
输入纳入离开保护且有显式放弃。确认和页面操作新增0模型调用，纠正沿现有R+L+A+J
请求包核验，来源/提取复用；本轮验证均用合成回包，没有调用真实模型。

本轮前端65项单测、65项完整浏览器回归及build/lint通过。Python域/合成DOCX74项、
桥接存储36项、追加catalog2项、人工分析12项、知识/原件46项及key golden2项通过；
各组有重叠，不相加。Rust常规API24项通过（外部夹具5项默认忽略），知识归属和同键
准入各自另跑真实PG专项通过；部署配置9项、本轮Python Ruff及diff检查通过。

证据根为`/opt/workspace/kb-tender-evidence/2026-10-28-remaining-fixes/`，含
`domain-final.xml`、`bridge-final.xml`、`history-final.xml`、`analysis-final.xml`、
`web-knowledge-final.xml`、`key-golden.xml`及`frontend-final-65/`。
真实HTTP/认证gRPC及当前SQLx迁移专项位于`backend-bridge/`；最终运行及清理回执在
该目录单独记录。本轮浏览器使用Office替身，未完成新大纲/知识/真实模型/真实Office
联合业务验收，也未正式部署；保留用户并行BUG-075及真实`.env`，未提交或推送。

跨层收尾：当前Rust HTTP Router→认证gRPC→Python测试1项通过；独立真实HTTP脚本7门
通过，覆盖当前三份实际SQLx迁移、历史/detail正向和跨项目/异用户隔离、分析确认/文案
纠正真实ledger及重放/过期冲突、知识版本准入、实际Python短期JWT经Rust HTTP回读
metadata及wrong job/version/login token/archive/cancel拒绝。18条分析模型回执全部
为synthetic且external_request=false，正文模型请求0；未测真实知识search/blob或真实模型。
证据为`backend-bridge/{verification.json,rust-authenticated-http.log,live-http.log}`。

本轮独占API18089、gRPC40489及独立数据库已清理；任务专用PG容器
`kb_remaining_test_20261028`及独占匿名卷已删除，4184/18089/40489/32794真实连接均拒绝。
原共享Office、Python runner及用户并行BUG-075/业务服务保持不变。根目录
`verification-final.json`、`cleanup-final.json`及`backend-bridge/cleanup.json`为最终回执。

## 数据库职责、统一命名与标准迁移（2026-10-28，BUG-075）

按用户确认只考虑全新初始化，旧 Rust 招标工作流及专用表/队列/脚本退出；Rust 保留
API、知识库与通用 worker。业务归属在应用层按创建人隔离，API Key 继承所属用户，
不保留成员角色与独立 scope。数据库保留事务、主外键、引用、outbox、租约和付费回执，
三领域无自定义业务函数或触发器。命名约定见[数据库命名与关联约定](../database-naming.md)。

初始化已从无版本 baseline 改为三份时间戳 SQL 与标准 SQLx migrator：
`20261028000001_shared_platform.sql`、`20261028000002_knowledge_base.sql`、
`20261028000003_tender.sql`。现有部署入口不变，新增版本自动编译嵌入；每文件和台账
同事务提交，失败版本回滚，已完成版本保留。旧未登记业务库明确拒绝，未来增量通过
新增版本升级，不改写已执行文件。并行大纲编辑表及所有业务改动均保留。

已实际验证：新隔离库56张业务表（10/26/20）及SQLx内部台账，0自定义函数/用户触发器；
迁移顺序/故障回滚/并发恢复/校验和/数据保留综合场景通过，实际migrator连续启动通过，
普通应用账号在预装扩展的空库连续启动也通过。知识库命名后278项通过；对象/retention
11项通过；项目及Word真实PG回归各1项通过。Python命名及迁移路径回归211项通过
（522.02秒，0skip），包含并行大纲逻辑，先前缺方法的4项失败已不再出现。

补查修复两个应用权限旁路：正文提及对象不能成为下载凭据；本地及LDAP配置都禁止
profile改写登录身份。已先复现失败，再在两种配置下执行双用户HTTP测试通过；认证
单元4项通过。未连接真实LDAP。部署离线配置9项、CI来源与diff检查通过。

用户随后要求将状态约束集中在业务代码；已按实体补齐Rust enum/Python StrEnum，
移除未知状态默认pending/active/done的宽松转换，不新增数据库状态限制。Python六类
宿主状态在持久化读写、工作流和调用边界共用定义；Rust平台/项目/编辑/保存五类状态及
知识库九类状态贯穿解码、业务分支与写入，JSON值不变。状态收敛后知识库281项实际通过，
含9类38个合法值及各类未知SQL text解码拒绝；平台2单元及6真实PG、API14单元及2真实
项目/Word回归通过。全workspace Clippy `-D warnings`、Rust workspace测试及fmt/diff
检查已通过；Rust常规组有28项显式忽略，不把它们计作实际PG验收，数据库结果以上述
独立专项为准。全量并发测试曾复现问题生成单测读取了另一个测试临时设置的模型环境，
仅给实际调用索引的3个测试补已有TEST_ENV_LOCK后通过，生产模型与网络配置未改。

Python枚举后完整回归为264通过/1夹具失败（575.83秒）：唯一失败的检查点围栏测试直接
调用graph却省略生产runner一直提供的status，已补RunStatus.RUNNING。随后相关20项
复测通过（2.16秒），含新增未知/缺失状态不得进入confirm的2项，未剩未解决失败；
合计覆盖267个不同用例，不表示267项在一次运行全部通过。上文211项和278项为状态
收敛前结果，不与新结果相加。Python StrEnum保留JSON字符串值；LangGraph内部msgpack
可使用枚举编码，不宣称检查点二进制字节不变，真实检查点/租约恢复由专项覆盖。

证据目录：`/opt/workspace/kb-tender-evidence/2026-10-28-database-redesign/`，
迁移反例为`migration-before-fix.log`，通过结果为`migration-sequence.log`、
`migration-cli-repeat.log`、`migration-application-account.log`及
`python-migration-authenticated.log`。状态验证见`knowledge-state-validation.md`、
`platform-state-enums.log`、`api-state-enums-postgres.log`、`python-status-regression.log`、
`python-status-focused.log`、`state-final-clippy.log`及`state-final-workspace-tests.log`。
专用PG/Redis测试夹具暂保留供并行验收使用，未清理共享Python runner。仅使用专用PG/Redis
与合成资料，未运行真实模型、
未部署、未修改现有业务库或`.env`，未提交或推送。

## 网页大纲编排与固定修订正文（2026-10-28，BUG-076）

本轮补齐“生成大纲→显式载入候选→网页编排→保存修订→按修订生成正文”。编制页提供
大纲编排/正文编辑两视图，章节支持新增、重命名、删除、排序、缩进提升及原生拖拽；
按钮与拖拽等价，循环拒绝。已有成功编制的大纲可直接作为候选，不必重新付费组织。
新章可填编写目标，留空则只生成标题；原章节的表单、附件、规则和来源引用由服务端继承。
删章保留原要求和移出位置的问题，不伪造已满足状态，不自动补回人工删除的章节。

新增`tender_outline_revisions`保存不可变大纲，项目行锁校验父修订；同键异内容经真实
认证gRPC明确返回409/REQUEST_CONFLICT。保存未知保持原payload/key，平台返回规范化
大纲作为已保存基准，保存后的继续编辑仍保持未保存。来源更新给出重生成出口。
正文任务固定revision/hash对应的完整大纲及原authoring_input，跳过组织阶段；模式及
outline_ref固定进执行规格，不能在同ID下更换。默认整稿路径保留，前序正文仍按完整
请求摘要复用，未指定前序不宣称跨执行复用。网页修改不直接重排当前Word，新正文仍
作为候选；两视图切换保留Office实例和修改状态，离开项目同时保护大纲和Word。

本轮验证分别为：新增域/投影/真实DOCX25项通过（domain-final.xml）；既有扩展域集
80项通过（与新增用例有重叠，不相加）；前端61项单测、46项浏览器回归、build及lint通过。
当前fresh schema的真实HTTP/认证gRPC/Word保存合同14项通过（0忽略），Python桥接和
健康探针44项、传输3项通过；实际migrator在专用空库连续两次，fresh专项1项通过，
20张业务表与3张官方checkpoint表的真实只读查询通过。并行BUG-075改名后健康探针
选列按实际baseline修正，没有恢复旧SQL、旧列或加入兼容建表。

真实合成DOCX核对了标题顺序/层级、删除、表单文字与合并及空位、稳定章节/响应书签，
并检查编写目标进入实际任务和正文、原始来源不变。此为固定模型替身与实际文件验证，
没有调用真实模型、没有重新验收真实招标语义或正式部署；本轮浏览器使用Office替身
验证两视图会话保护，未将前轮真实Office结果冒充新大纲全流程验收。

本轮主要证据根：`/opt/workspace/kb-tender-evidence/2026-10-28-outline-workbench/`，
含`domain-final.xml`、`outline-edit-export.xml`及`backend-bridge/`；实际合成文件为
`pytest-outline-edit-export/test_human_outline_projection_0/edited-outline.docx`，同目录有
`word-document.xml`及`export-summary.json`。浏览器最终46项及截图保存在
`/opt/workspace/kb-tender-evidence/2026-10-27-web-integration/outline-final/`。
最终已停止本轮gRPC夹具并确认进程不存在，删除`kb_outline_test_20261028`及独占匿名卷；
共享Office、原Python runner和BUG-075专属PG/Redis均仍运行。未启动本轮常驻API，
4184浏览器测试服务已退出；清理回执为`backend-bridge/cleanup.json`。测试证据和实物保留，
未改真实`.env`、提交或推送。

## 前端工作台及真实Word接入（2026-10-28，BUG-074）

本轮接通v3项目权限、资料上传/原件下载、Python持久任务与工作台、同版要求/依据/待补，
正式正文仅在ONLYOFFICE读取/编辑。新生成稿保留为候选，不覆盖当前人工稿；显式保存
以同会话/父版本/文件哈希相关的签名回调发布不可变版本。页面读取/导航/保存/下载不调用模型。
开发runtime纳入Python三个角色并共享DocReader的数据库、模型和artifact卷，认证桥接token
显式配置。Word映射最初通过增量接入，随后随用户并行BUG-075统一到tender_baseline；
保留用户初始化方案，不恢复旧SQL或旧增量。正式release已接八服务/五镜像及严格阶段，
18项合同回归通过，Python健康探针10项单元测试通过；未实际运行正式发布。

隔离验证：Python95个唯一用例通过、实际认证gRPC传输2项、Rust API13项、Word导航/导出46项、
部署离线配置9项；此前platform单元63项和migrator诊断1项通过，另2项明确忽略。
在BUG-075当前fresh-only结构上另建新专用空库，实际migrator连续执行两次，验证Python/Word
19张业务表齐全且保留测试用户，fresh专属integration 1项通过；最新cargo check -p api通过。
随后同一新库复跑Rust API全组13项全部通过，包含真实HTTP→认证gRPC→当前Python只读setup、
上传/资料原件/工作台及Word保存回执，不再仅依赖旧schema的同名测试结果。
前端最终完整30项浏览器回归（其中插件7项）及npm lint通过。
当前fresh schema另有关键Python25项通过（55.32秒，0skip/0error）：桥接/jobs21项、
大纲检查点1项、AuthoringStore恢复/取消/只读setup3项。标准CLI init及真实22表只读检查通过；
实测修复健康探针将checkpoint_writes.blob误写为value的问题，反例回归已保留。
这不代表已启动并验收worker/dispatcher消费者或完整生产健康。JUnit证据为
`current-schema-python-key-tests-final.xml`，仍位于本轮外部证据根。

真实ONLYOFFICE8.3.3合成文件验收已通过（1项，10.5秒）：实际view插件GoTo→edit键入marker
→顶部显式保存→签名status=6回调→不可变v2→下载sha256与版本元数据一致。独立检查Word XML
保留marker及两个chapter书签，保存后重新绑定GoTo成功。此前反例确认直接JWT带iat/exp导致
status=1回调403；已修复并保留签名/过期/业务内容校验。该验收只使用合成文件及任务专用库，
没有调用真实模型，没有读取真实招标资料，没有执行正式部署。最终高度布局再次真实验收
1项通过（8.5秒），保存到不可变v4，新增marker入稿、下载hash一致及保存后GoTo再次成功。
两轮真实Office均使用此前固定API二进制及专用旧schema，不能宣称已覆盖当前fresh schema
的Office完整端到端；当前fresh schema由上述13项真实HTTP/PG/gRPC专项覆盖。

证据根为`/opt/workspace/kb-tender-evidence/2026-10-27-web-integration/`；最终实物位于
`real-word-final-verified/`，首轮位于`real-word-verified/`，包含`saved-synthetic.docx`和
`word-after-save.png`。测试token不打印、不提交；新增当前schema连接配置不含密码，凭据仅经
测试进程环境传递。最终已撤销仅任务fresh库的临时CREATE权限，停止本轮API和三个gRPC夹具，
删除专用PG容器`kb_tender_web_test_20261027`、未使用的专属空Redis容器
`kb_tender_web_redis_test_20261028`及各自独占匿名卷，并只读确认已不存在。
既有`kb_authoring_test_20261026` Python runner和共享`knowledgebrain-onlyoffice`仍在运行，
业务服务与真实`.env`未改；DOCX、截图、JUnit及回放脚本保留在外部证据目录。
本轮不提交、不推送、不修改真实`.env`。

## 参数引用位置的规则混绑优化（2026-10-28，BUG-071）

进一步核对S/A原始回包：S未生成独立主要参数参见位置，A借内容候选t76建该位置，
宿主继承其报价填写及否决规则。旧A合同不允许仅以related_target_ids创建纯参见，
属于协议表达缺口。共享全部已安排规则会令输入从89400增至至少107646，超94208，
未采用重复全量发送、删条件或提高预算。

先补问题记录、字段字典及五项调用/容量方案；沿已有字段允许narrative纯参见，
不继承被参见位置的规则/模板/企业内容。来源验证仅在规则、来源、模板皆空时
记录未绑定，不再给真实来源支持的引用位置误报；未读、未知、多实例及未安排候选
继续保留诊断。已解析且无自身内容的纯参见不派空写作任务、不显示多余正文待补；
旧真实正文及诊断仍保留。未修改S提示、部署或.env，不涉及其他并行Rust/网页任务。

反例分别先复现4项来源误报、5项纯参见合同/关联失败及4项空任务/待补失败。
修复后来源相关35项、合同/保留79项、配套含书签94项通过（模块重叠，不相加计数），
本轮8个修改Python文件Ruff及diff通过。免费预审4S完整摘要不变并成功回放，实际A
89500/94208准入，未增加来源读取或模型阶段。

独立`document-reference-routing-20261027-v1`使用冻结runtime完成真实复测，沿用
专用测试库中前一成功执行以复用完整相同请求；旧运行和证据保留。仍基于旧363规则
快照编制，无知识/企业资料，非新五节点分析。证据目录为
`/opt/workspace/kb-tender-evidence/2026-10-27-remaining-fixes/reference-routing-v1/`；
耗时130.96秒，复用6份完整相同请求回包（4S及2写作），新增1A+4写作+2复核共7次
发送，供应商报告309232 Token，0失败/重试/未知用量；该耗时包含来源缓存重放，
不是重新阅读原文全流程。冻结代码指纹一致。同ID恢复同结果、0新增请求/回执，
0.160秒，旧成功运行及文件未覆盖。

独立实物审计通过：C仅显示“主要技术参数”及“参见2.2.2 专用技术要求”，没有
报价/否决规则、要求底表、独立写作单元或多余待补。19条核心参数完整保留；上轮
91条技术规则在新全稿全部可见，90条仍在技术卷，随机备件计价规则移到报价位置，
不是漏失。技术卷98个章节要求行全部正文及有序引文保留；报价i73在实际报价位置、
否决i98在专用技术风险要求保留，证明/评分/偏差条件均保留。363底稿逐字段不变，
24张原表文本及合并结构无增删；61单元全placeholder，0企业填值/证明，3处人工价格空白。

总稿、ZIP、三分卷及底稿/待办与manifest一致，技术分卷的要求表及C文字与总稿相同。
封面无要求底表，其要求及原引文在提交待办完整。交付目录为
`reference-routing-v1/document-reference-routing-20261027-v1/`，技术卷`volumes/002.docx`，
最终证据`independent-reference-routing-audit.json`。源码未部署；当前验收是已有规则
完整保留与引用位置正确展示，不代表全原文语义召回已完成。F关于目录层级、分项
报价引用、分册封面/可选附件归属的意见仍保留待核，未自动重排或宣称外部提交完成。

## 技术响应要求入表复核（2026-10-27，BUG-063/071）

用户指出技术响应不全。再次对照原件及最新display-revision-v1：旧稿技术卷81条
已绑定唯一规则中，仅71条进入依据表；其余部分由原表保留，部分实际遗漏。原文
物理59—60页七项★功能的证明及原厂盖章要求已抽为i29，只绑attachment，导出
按narrative过滤而漏掉；产品先进性证明及偏差表条件也有同类问题。核心技术段
i1—i48中47条正文已在分散表格，不能称核心参数全未抽取。20张依据表151行仅85条
唯一规则，不能再用总行数证明完整；主要技术参数位置只绑否决后果且无实际参见。

先更新BUG-063/071、字段字典与方案，再修宿主导出：每章汇集所有目标的完整有效
规则引用，精确去重后显示一份要求依据；原表及不同主体实例不合并，企业应答及报价
仍空白。红测5项失败，修复后全部7个导出相关模块98 passed，引用/结构42 passed
（存在模块重叠，不能相加作为唯一测试总数），Ruff/diff通过。来源/组装提示沿已有
constraint_range_ids传递实际跨位置引用依据，由模型选related_target_ids，不按标题
搬规则，不新增合同字段、来源轮或生成门禁。

独立`document-technical-coverage-20261027-v1`完成编制复测，仍用旧363规则冻结
快照，不是重跑五分析节点。无知识/企业资料、无人工补选submission_sources；新来源
提示使4S重新付费，不声明旧S复用。免费预审4包输入94120/93655/94203/85275，均
≤94208，输出预留32768，并发2。证据在`/opt/workspace/kb-tender-evidence/2026-10-27-remaining-fixes/technical-coverage-v1/`。
耗时293.26秒，4S+1A+6写作+2复核共13次真实发送，供应商报告703464 Token，0重试/
失败/未知用量；同ID恢复同结果、0新增请求/回执，0.121秒。64单元全placeholder，
0企业填值/证明，3个人工价格目标保持空白。

独立实物审计确认技术卷91条已绑定唯一规则对应108个章节要求行，逐章全部正文和
完整引文保留；核心参数19条各在专用技术要求出现一次，通用及主要参数位置明确
参见2.2.2，不再复制整套参数。i29七项功能证明及原厂章、产品先进性评分/证明及
9条偏差条件完整入表。363底稿逐字段不变，24张原表无增删，表文和合并结构一致。
这是已知规则成稿保留验证，不是全原文技术召回验收；C本地仍绑定报价填写/否决
说明，虽已有正确参见，其语义归类仍待改进。F关于目录层级、分项报价、分册封面及
财务主体的意见保留待核，不自动当事实或宣称全部解决。

导出修复的封面回归先由1个失败反例确认，再修为原封面不插依据表，相关要求及
引文进入可读提交待办。最终8个导出相关模块112 passed，Ruff/diff通过。独立
export-revision-v1零模型重导出保留成功执行与旧文件，最终审计通过：封面无依据表、
封面要求和引文进入待办，技术卷规则/空位及24原表不变，ZIP/总稿/3分卷与manifest
逐项一致。交付文件在该修订目录的`投标文件.docx`、`投标文件及附件.zip`，技术卷为
`volumes/002.docx`；最终证据final-technical-coverage-audit.json。源码未部署。
并行任务修改integrations/web.py，宽范围code_unchanged为false；编制相关源码运行时
未变，运行后封面导出修订单独记录，不把该宽标记或重导出当作新全流程验证。

## 剩余缺口统一修复与验证（2026-10-27）

用户授权继续全部修复。先补BUG-065—068及BUG-053历史状态说明，统一字段字典和
调用/容量/出口/恢复方案。实施拆为未安排规则及跨目标引用、知识证明原件、正式布局、
持久语义评测；沿既有S/A/写作/复核流程，不新增主节点或门禁。用户明确没有真实
企业资料，本轮继续验证正确留空，不再要求知识库输入。旧产物和回执保留，.env和
部署不改，不把人工GT或真实资料匹配验收自动标为完成。

完整unit及编制相关隔离回归1567 passed，357.49秒；最新衔接先补失败反例，再由
最终56项专项通过，8.31秒，Ruff/diff通过。测试使用既有隔离容器、专用PG库、
每例schema和空Redis DB15，不清理共享业务数据。

### 无资料编制与目录衔接复测（BUG-065/069/070）

独立v1 `document-completion-20261027-v1`完成119.85秒，继承4份来源回包；新增
1A+6写作+2正文核对+1F=10次发送，437601 Token，0重试/失败/未知用量。
同ID恢复同结果，0新请求/回执，0.147秒。363/363规则逐字段保留，24原表、903锚点、
21合并及598空白完整；66单元全placeholder，正文/填值/证明各0，3处人工价格保留。
原48条未安排规则变为1条绑定、3条交付约束、20条按原文条件明确排除、24条仍待核。
5个排除issue对应20条规则；gaps按code/detail去重后的1项不能当未安排规则数量。

完整A初测94601/94208超准入，根因是290部件重复25种完整标题路径；复用现有标题
共享投影后85223准入，省9378输入估算Token/33620 bytes，规则、原表、空位及所有
语义逐字段不变，未提高窗口。实物检查显示表齐全却将独立技术附表组成降为专用正文
子节；3.1.2及附表正文虽在A/F，真实技术目录未交接，解析标题路径还继承错误。

已追加BUG-069/070及字典和五项容量说明：来源S用原constraint_range_ids传递目录/
组成依据，原文高于定位路径；A/F核对父子层级而非只看先后。F实际投影未安排/排除
规则语义、引文及理由，共享支持标题，不新增核对轮/门禁。新S Prompt使来源摘要
改变，必须重新付费，不继续宣称4S可复用。独立v2不传任何人工补充submission_sources，
4S离线输入94144/94200/93947/83502均≤94208；首包488来源含真实技术目录及3.1.2。
v2已完成302.18秒，全部16调用重新付费、933019 Token，0重试/失败/未知用量；同ID
恢复同结果、0新请求/回执，0.151秒。实际S自主选择目录和3.1.2，A/F都收到，技术
通用/专用/附表同级恢复。57单元56空稿、1段来源要求说明，363规则/24原表保真。
另发现参数位置只留空标题、三份规定文件却全is_volume=false；追加BUG-071/072，
没有因F意见多就新增门禁或把每个来源疑点都自动变成章节。

v3只澄清A/F既有字段语义，S完全不变。完整A禁外发预审86788/94208，4份S摘要
一致且重放成功，无人工source注入。`document-completion-20261027-v3`完成138.45秒，
继承5份完整请求一致的回包（含4S），新增13次调用：1outline/7authoring_write/
5authoring_review，693069 Token；0重试/失败/未知用量。此时间包含缓存来源重放，
不能当重新阅读全部原文的耗时。同ID恢复同结果、0新请求/回执，0.181秒。

实际ZIP含总稿及商务/技术/报价三份独立Word，manifest/CAS/实际成员和分组一致；
参数位置已有真实narrative目标和待填空位，没有造原表。该处原文只有“详见技术
规范书”，未建立明确指向正文参数目标的参见，不能宣称引用映射已验收。全部363规则、
24原表/903锚点/21合并/598空白保留；20张底表151行引文精确，企业应答空白。
62单元中61空稿、1段有来源的要求说明，无企业参数/实际承诺/填值/证明或价格代填。
当前未安排27规则，多为其他参与方程序及待核范围，不能按数量自动塞正文或判不适用。

v3正常中文PDF总稿68页，商务23/技术43/报价4，抽查7张代表页未见裁切；总稿页数
增加与要求底表97→151行及目标安排变化有关，不代表增加了有依据的企业正文。
F四项保留待核：分文件封面/可选附件归属、分项报价引用、财务主体及签署、履约保函
适用范围。现有原文不足以自动复制封面、编造附件或删条件实例，仍沿待核草稿出口。
最新代码未部署，真实企业资料匹配和人工GT仍未完成。轻微重复展示标签另记BUG-073，
只作零模型重导出，不覆盖本次成功执行或重新付费阅读。

BUG-073已完成：actual DOCX反例先2 failed/3 passed，修复后四个导出相关模块
66 passed/0 skipped/0 failed，8.90秒，Ruff/diff通过。独立display-revision-v1在禁网
环境从同outline/62单元/review/requirements重导出，新调用/Token均0；旧成功执行及
Word/ZIP哈希不变。审计0问题：底稿和完整待补JSON逐字节相同，原表XML及三卷保留，
151行空响应及人工价格不变，仅总稿/技术卷各7处生成标签和待办17处同名标签显示修正。
最终可交付留空稿位于real-authoring-v3/display-revision-v1/的`投标文件.docx`及
`投标文件及附件.zip`，revision-audit.json/revision.json明确原v3累计用量与本修订0新增
调用的区别，不把显示重导出当新付费或覆盖旧运行。目录实际页码及上述原文待核仍保留。

本批合计214次真实发送（含32次合成评测），供应商报告3985109 Token；无失败/重试/
未知用量。真实分析、三轮编制及评测分别计量，不把缓存重放当新付费或把不同模型的
耗时变化当代码单因果收益。显示修订新增0；原回执和旧产物全部保留。

v1正常字体转换实际查看8个页面，封面、用途、跨页表头、列宽、空应答/价格可读；
Word有TOC域，但原x2t不更新目录页码。docbuilder私有更新副本中文缺字且分页错误，
已拒绝交付，不替换原Word，不将字段存在称为页码实物验收完成。字体工具曾意外
覆盖OnlyOffice FileConverter的三份既有缓存；已保留覆盖版本，并按同版本/同原字体集
私有重建恢复，生成Web字体表与未改原Web表逐字节相等，原Web哈希未变，无服务重启。
还原前后校验及备份见v1/visual-audit/font-cache-recovery/restoration-audit.json。
本节证据均在`/opt/workspace/kb-tender-evidence/2026-10-27-remaining-fixes/`的
real-authoring-v1/v2/v3目录，包含原回执、预算前后对照、独立数据/语义/视觉审计和恢复证明。

### 新完整分析实测（normalization8）

独立运行`6597eb6f-e638-466d-b556-30fb9d161989`从同106页原件重新登记、冻结、
2份唯一图实际OCR、抽取及独立核验。runtime复制冻结，原件及代码SHA核对通过。
总701.54秒（11分41.54），抽取207.90秒，核验478.84秒；实际最大并发2。
143次调用全部成功：2OCR+4抽取+11review+41locate+81associate+4joint，0重试，
供应商报告1839506 Token，未知用量0。OCR逐出现位置callback98次不能当98次付费。
review/locate/associate/joint实测分别57.79/130.17/215.22/16.66秒，核验总计另含规划/
持久化；本次是terra+n8，旧sol+n6核验56分33秒，不能把所有差异都归因代码。

4包187/250/335/24范围，共796原生范围/796处置精确覆盖一次；241事项，551事项
引文+82关系引文+20决定引文均可回溯，443 CAS及138份runtime文件SHA一致。
终态waiting-confirmation，0人工确认；589个人工问题、259条未决处置、25条未复核
关系和0条接受决议保留，未自动confirm或宣称analysis-ready/语义验收。旧363事项
不是人工GT，不能只按数量差异断言本次漏检或召回通过。源码运行与完整语义验收分开。
证据：同父目录full-analysis/的status/output及independent-full-analysis-audit.json/.md。

### 持久语义评测真实执行（2026-10-27，BUG-068）

新增caller-owned Python执行器，复用独立分析Store/Lease及现有模型付费围栏，
不新增CLI、表或主节点。冻结本轮Python/Prompt副本，使用专用PG库及独立schema
`semantic_evaluation_20261027_v1`，没有运行或更改真实招标分析。合同、部分失败、
预算、标注隔离、用量和恢复相关47项隔离测试通过，90.60秒；Ruff及diff检查通过。

当前gpt-6.1-terra、131072窗口、并发2及同已校准endpoint下，32份v2合成独立核验
输入全部准入，累计输入估算95472，单次2826—3181 Token，输出预留32768。真实
执行`synthetic-semantic-v2-20261027-v1`完成46.03秒，32/32回包获宿主合同接受，
32次发送，0重试/耗尽/预算超限；供应商报告79408输入+2506输出=81914 Token，
未知用量0。同一租约内再次执行同身份返回同报告，0新请求行、0新发送。每30秒
保存进度、原始回包及拒收/预算/用量引用；.env及部署未改。

逐案Agent阅读的31个整体判断方向与草案一致，1个`table-unit-consistent`存在标注
分歧：原文要求按表价填报，候选写成投标人的实际单价，模型认为改变义务性质。
该反馈有合理依据，保留原草案等待人工裁定，没有自动改标或计算正式准确率。
`human_review_completed=false`、`semantic_acceptance=false`保持；本轮不验证抽取
召回、五节点全流程或真实投标成稿。隔离Store以blocked释放评测租约，仅因既有
API无评测终态，metadata明确`evaluation_completed=true/workflow_executed=false`，
没有创建业务门禁。报告状态独立保留，不将blocked误称模型执行失败。

证据：`/opt/workspace/kb-tender-evidence/2026-10-27-remaining-fixes/semantic-evaluation/`
的result/report/resume-proof/owner-run/progress及agent-review；隔离回归证据为
`/opt/workspace/kb-tender-evidence/2026-10-27-semantic-evaluation/green.log/xml`。

## 正式业务标题与可读细则修复（2026-10-27，BUG-062/064）

用户指出仍有内部JSON及来源格式编号标题。已先更新问题记录、字段字典及调用/容量
方案。BUG-064仅改要求展示：按已有字段的业务标签显示嵌套细则，不再json.dumps进入
Word；隐藏kind/domains，0/false、成组量值及原文代码字符串保留，完整底稿不改。
actual DOCX反例先5 failed/1 passed，相关79项回归通过，Ruff通过；最终42模块完整
隔离回归494 passed，158.79秒，未以此代替真实语义验收。

BUG-062已实际修改outline_assemble Prompt，区分业务标题和来源格式标签，不默认
一表一章；合章不合并独立填写实例，保留角色/条件/人工价格边界。明确选取必要原格式，
不截表体/续表/备注/签署，不使用附件号或标题正则。来源Prompt和模型回包schema不变。

独立执行`document-business-outline-20261027-v1`已启动，同106页冻结分析、同
gpt-6.1-terra/131072、并发2、无知识/企业资料。离线完整A预审准入，4份来源请求摘要
均未变并已重放，109候选及19完整网格保留；A和实际变化下游需新付费，未把旧A当成
新标题验收。源码指纹固定，旧运行及.env不改，未部署。证据：
`/opt/workspace/kb-tender-evidence/2026-10-27-title-and-readable-output/`。
v1已完成113.96秒，继承4份来源回包，新增10次调用、431950 Token，重试/失败/未知
用量均0；同ID恢复同结果、0新增请求/回执，0.155秒。商务标题已按业务分组，原
“8A 商务部分摘要表”不再独立作为正式标题。但独立审计发现技术卷被改成5个主题章，
压平原规定层级并重新排序；最终F也报告technical_order，不能宣称结构已合格。
已先补BUG-062及字段/方案，在同一次A明确规定结构高于业务归并与标题简化，仅
纯标签可作为冗余省略，实际填报说明保留。独立v2执行
`document-business-outline-20261027-v2`已完成114.42秒，继承4份来源回包；新增
1A+6写作+2正文复核+1F=10次调用、432796 Token，重试/失败/未知用量均0。
四份来源摘要仍一致，完整A估算85551/94208输入Token准入，19完整网格及109候选
不减；没有新增节点、全文件源读取或标题规则引擎。同ID恢复同结果，0新请求/
回执，0.112秒。运行指纹不变，保留v1及全部旧结果、原文和回执，.env不改，未部署。

实际Word共29个正式标题，与最终树一致；原“8A 商务部分摘要表”已组织为
“2.5.1 商务摘要及基本情况”，另有“2.5.2 股权结构”“2.5.3 财务状况”等业务标题。
技术卷恢复“通用部分→专用部分→技术附表”；专用部分按原顺序安排总则/标准、
环境/硬件、技术支持、售后、预装调试、供货等位置，最终F未再报告technical_order。
股权表两段实质填写/截图证明说明在实际Word出现一次，保留原表，不只留在底稿。

独立实物审计确认363/363当前规则逐字段保留，24张原生表XML集合与v6一致，
903锚点/21合并/598空白未变；109行要求依据的规则文字、有序引文及企业空应答格
逐行对应。内部英文键/序列化JSON不再倾倒，中文细则如“最高分：20”可读；原文
真实代码字符串不清洗。主文与三卷、ZIP与CAS一致。65单元仍全为placeholder，
没有真实企业资料、填值、证明或最终报价，不能将结构修复当企业应答已完成。
独立结构审计另确认48条原未安排规则身份集合完全一致，不因改标题消失或被自动
判为已安排；四类财务表各两角色实例、10条用途说明及3个人工报价目标保留。

v2最终F的4条当前意见为偏差表待真实应答、平台上传可编辑压缩包、跨卷报价表
引用及报价承诺格式待核；意见不新增门禁，也不要求据此创建额外章节。当前确认
的是同一冻结106页分析上的编制语义及展示修复，不是从PDF重跑五节点，不代表
全部类型招标文件或完整布局已验收。知识检索与BUG-060版式边界仍待后续。
v2证据：`/opt/workspace/kb-tender-evidence/2026-10-27-title-and-readable-output-v2/`，
包括result/status/resume、独立readable-output审计及artifact-audit；最终Word与
ZIP在执行ID同名子目录。独立结构报告为independent-structure-audit-v2.md/json，
确认原规定层级、顺序及说明已进入实际Word。两个新版真实编制合计新增20次调用、
864746 Token。最终Ruff、diff及修改文档本地链接检查通过。

## 已知要求独立保留修复（2026-10-27，BUG-063）

用户明确要求无企业响应也不能丢失已知招标数据。已先更新字段字典及五项容量约束，
新增宿主只读AuthoringRequirements快照，写作前保存全部当前规则及target关联；
新结果以requirements_ref索引，ZIP附完整`招标要求底稿.json`。正文narrative位置
独立显示要求依据、范围/条件/例外、量化/评分/后果/提交细则及原引文，企业应答栏
保持空白，实际生成内容仍按旧单元单独渲染。form/attachment不盲目复制原文造表。
快照复制保留可读旧字段，不以二次合同校验删除内容；补遗仅用当前effective_rule。

专项失败反例先红后绿：无资料、组织/写作耗尽、应答整块被剔除、改稿耗尽、人工
报价、部分补遗、未安排和source-only均覆盖；来源背景不被冒充已确认要求。最终
42模块完整回归485 passed，154.46秒。随后补两个已处置身份反例，确保提交约束
及已明确排除候选不误报未安排，36项本轮专项全部通过，7.94秒；最终42模块完整
回归**487 passed / 0 skipped / 0 failed**，152.79秒，证据在v2目录。
七个实现及新测试文件Ruff、语法、diff和文档检查通过。所有测试使用既有隔离容器，
完整回归采用专用PG库/每例schema及空Redis DB15，不清理共享业务数据。

同106页冻结分析的独立v1`document-requirement-preservation-20261027-v1`已完成，
40.09秒，14份真实回包继承、新请求/Token均0，生产指纹未变；私有验收transport拒绝
任何未命中缓存的模型外发。同ID恢复0新请求/回执、同结果，0.108秒。此为本地编制
重放，不是新模型语义验收，也未从PDF重跑分析五节点。

独立实物审计确认363/363当前规则及字段完整，引用去重及目标关系一致；技术位置
原31条规则和71条有序引文全部进入Word。共13张要求依据表、122行空响应，旧24张
原模板表XML不变，总稿/分卷一致。底稿144条没有正文target，其中95条已有提交约束
安排、1条显式排除、48条确实未安排，不能将144都称漏响应。全部仍在底稿，不自动
塞入技术章或把未安排当适用结论。原64个单元均为placeholder，未增加企业正文/填值。

已处置诊断修复后的独立v2`document-requirement-preservation-20261027-v2`已完成，
40.68秒、14份回包继承、新请求/Token均0、代码指纹未变；同ID恢复0新请求/回执、
同结果，0.125秒。底稿requirements_ref与v1相同，DOCX解包后所有成员逐字节相同，
继承v1实物及PDF定向核对结果；保留v1及全部旧回执，不覆写旧成功结果。
v1实物审计见`independent-requirements-audit.md/json`，技术卷转为32页PDF，已实际
查看第18/22/23页，参数、联动条件和跨页原文清晰可见，应答栏为空；列宽和跨页表头
仍属BUG-060。此为数据保留/显示定向验收，不证明企业响应内容已齐备或全篇版式合格。
最终v2独立报告见`independent-v2-delta-audit.json`：三卷正文XML同样与v1一致，技术卷
共102行要求依据，其中技术规格位置31行；全部企业应答格为空，没有自动承诺。
证据：`/opt/workspace/kb-tender-evidence/2026-10-27-requirement-preservation/`及
`/opt/workspace/kb-tender-evidence/2026-10-27-requirement-preservation-v2/`。本轮没有
修改Prompt、模型/预算合同、.env或部署；BUG-062正式标题和BUG-060完整布局仍未修复。

## 新增内容完整性核查（2026-10-27，BUG-062/063）

用户指出正式标题夹带来源附件/表号及技术参数响应表缺失。已对照v6大纲、写作单元、
原PDF及冻结来源，确认chapter-2.5.1使用“8A 商务部分摘要表”，导出另加2.5.1章节号，
随后采用的模板又包含原表标识；编号来自模型接受的title，不是导出器额外生成。
原PDF物理91页本身就有外层格式标签和表内标题两行，当前成稿再加Heading3形成三处。
已核规定要求按第六章格式编写并编目录，但未要求正式章名必须前置附件号或表号；
原模板标签是否都应插入须由模型按部件边界选择，不能由代码按编号删除。
字段字典已明确任意层级章节均按业务组织，来源表号仅在原格式中保留，不按编号正则
删改，也不机械一表一章。本轮先核实根因及修订文档，尚未改Prompt或运行新模型调用。

技术卷实物4页，chapter-3.2.2主叙述目标已绑31条规则、16份来源引用，包含五个原生
参数grid；w49却为knowledge-only而全空，最终只显示待补。原PDF物理55页要求逐段
响应，物理57—61页的参数原表仍完整可读；参数附表C只引用技术规范书，G.1/G.2是
偏差表，不能替代全部参数应答。物理82页报价表也有要求/投标参数列，实际留空；
不能把报价卷存在的空表算成技术卷已逐项响应。根因是招标要求骨架和企业真实应答
混为一个可跳过任务，而不是解析器漏表或全部归因于未接知识库。

后续修复须保留来源要求/条件及可填写位置，企业参数、符合/偏差结论和证明按资料
匹配，没有资料留空。现有table块可表达底表，但混合来源和整块移除的限制尚须设计
核对，未宣称仅改Prompt即可保证逐行完整。旧v6及全部回包、原文件保留，未部署。
原文/目标/空置单元的只读证据见v6目录`technical-response-audit/review.md`及
`audit.json`。五个参数grid包含跨页续段，不能按单元格片段数冒充独立要求条数。

## 通用提交结构修复实施（2026-10-27，BUG-054/057/058/059/061）

按已批准后续方案实施。已对照原PDF、冻结原文、四份来源回包及A实际输入：两项
被误报缺失的格式实际有本次不适用限定，第二卷技术规范也已提供；限定在来源读取
时可见，却未被选作提交依据，来源适用性finding又未进入A。不能用固定附件名/页码
清单作生产判断，也不能把本批未读等同于原文件缺失。

源码已恢复A完整原生grid及locator标题路径，传入来源观察；增加模型明确排除候选
及理由的可选表达，未知/选排冲突沿原合同纠正，真正遗漏继续待整理。调用方可用
现有SourceUnitRef显式补充当前冻结提交原文，输入冻结spec后供A/F实际读取；不新增
主节点、主体体系、结构改稿循环或业务词表。明确排除候选的专属旧问题按原批次/目标
身份退役，原回包保留。BUG-059另在展示层解释解析标题装饰，不修改冻结原文或定位。
BUG-061在同章多个form复用相同模板时显示各自已有purpose，避免空白表单无法区分
填写用途；不推断主体、不改原表，不新增角色字段或模型调用。

定向回归包含真实PG入口、来源去重/冻结/恢复、完整网格、排除与实际DOCX合同。
首轮39模块完整相关回归447 passed/0 skipped/0 failed，149.64秒；随后补精确退役
反例及来源阶段标注后448项通过；再补实际多实例用途展示，最终39模块完整回归
**451 passed / 0 skipped / 0 failed**，149.00秒。使用专用PG测试库及空Redis DB15，
模型为确定性替身；七个实现/测试文件Ruff、语法、diff及文档链接检查通过。未以这些
结果代替真实语义验收。最终回归证据在v6目录。

独立v5已完成，保留v1—v4全部回执及文件。离线重放确认原四份来源摘要完全未变；
补充的82份原生引用只作本次复测输入，实际文件特定选择保存在仓库外，生产代码中
没有附件名、页码或范围ID。A完整请求估算85,041/94,208输入Token，含19完整网格及
37条来源观察及82份补充原文引用，输出预留32,768，实际发送并成功。同gpt-6.1-terra、
131072窗口、并发2、无知识/企业资料，运行代码指纹未变。

v5执行`document-submission-followup-20261027-v5`耗时117.30秒，继承四份来源回包，
新增1A+6写作+2正文复核+1F=10次调用，434,009 Token，0重试/失败/未知用量；各请求
输入估算比供应商报告保守多502。形成34章、65位置、64个placeholder单元，新增正文、
单元格填值及真实附件均为0，不能算已完成实质企业应答。备件候选明确排除，原件中
本次不适用的格式不再误报缺失；多角色财务表形成独立实例。F的5条当前待核意见包括
条件性签署/授权、参数附表对应、跨卷价格引用、平台上传和报价承诺格式，仍须核实；
52条来源阶段观察和797条累计缺口另计，不能当成797个独立当前缺陷。v5同ID恢复
0新请求、0新回执、同结果，0.162秒。

BUG-061修复后独立v6执行`document-submission-followup-20261027-v6`耗时44.57秒，
14份模型回包全部继承，**新增模型请求0、计入新调用Token为0**，运行代码指纹未变；
同ID恢复0新请求、0新回执、同结果，0.146秒。上述两个耗时均为已有冻结分析上的
编制/重新导出，不是从PDF开始重跑五节点。保留全部旧文件及回执，未修改.env或部署。
独立实物审计确认增加10条既有用途说明，各在对应实例前出现一次；除此之外段落
序列及全部原表内容与v5一致，24表、903锚点、21合并、598空白保持，总稿/三卷/ZIP
一致，字面Markdown标题残留为0。大纲、依据、64个单元及复核引用和797条累计缺口
均与v5严格相同；14份继承回执的输入摘要及产物逐条对应，未补造付费请求。
证据分别位于`/opt/workspace/kb-tender-evidence/2026-10-27-submission-followup-v5/`及
`/opt/workspace/kb-tender-evidence/2026-10-27-submission-followup-v6/`。
v6独立报告为`independent-replay-audit.md/json`、`artifact-audit.json`和
`native-grid-and-instance-audit.json`；最终文件保存在该目录下的执行ID同名子目录。
v6总稿及三卷经已有OnlyOffice转换为26/19/4/4页PDF，代表页截图实际核对各角色
用途说明位于对应表前，条件、空白及页脚正常；见`visual-audit-v6/review.md`。
此为BUG-061定向显示验收，不代替BUG-060全篇版式验收，转换新增模型调用0。

无模型重导出v4并经已有OnlyOffice转换为22页PDF：20处Markdown标题装饰被正确
解释，原19张表格文字、段落数及旧文件不变；视觉定向检查通过，未宣称所有版式
验收完成。真实知识库当前无可用真实资料：默认资料库为空，其他集合为模拟验收资产；
不能用它们冒充知识匹配验收。部署API认证探针401，搜索实际合同尚未核实，不因健康
探针正常宣称可检索。未改.env、网络配置或部署。

通用性约束已在修复方案明确：不预设固定三卷、角色或附件清单，案例特定选择只在
仓库外测试输入，生产代码按当前原文及显式引用工作。单文件结果不代表多类型招标已
全面验收。BUG-060的目录页码、封面顺序和跨页表头仍待通用布局合同，未宣称已修复。

## 提交结构与表单归属修复实施（2026-10-27，BUG-054）

用户要求完整方案，已新增[修复方案](submission-structure-plan.md)，同步字段字典的
唯一后续合同、实施入口和15项未勾选验收。方案覆盖正式目录、依据与模板分离、精确
部件/填写实例、整表人工报价、实际采用内容复核、紧凑结构核对及同树组卷；不新增
分析节点、Agent、审批门禁或多层路由。旧17回包缺少新模板语义，升级不能免费补认。
用户随后批准直接实施。已先复现模板/目录错误，再接通精确模板装载、一次全局归并、
最终结构核对和同树组卷。修复新增冻结根不可读出口及图片模板OCR语义投影，原图不重复
打印OCR；关联条款原文进入写作可选支持，最终引用仍须模型选择。相关32模块完整回归
**378 passed / 0 skipped / 0 failed**，141.81秒，使用专用PG测试库与独立Redis DB15；
模型为确定性替身。源码Ruff、语法及diff检查通过。

真实同文件新执行已启动：`document-submission-structure-20261027-v1`，专用schema
`submission_structure_20261027`，gpt-6.1-terra/131072、并发2、不提供knowledge/materials。
复用106页原件的冻结分析快照，不重跑五节点；不继承旧17份来源模型回包。新离线规划
21来源包全部准入，累计输入保守字节估算1,783,298（非付费Token），最大94,175/94,208，
输出预留32,768（非API硬限）。A/F各最多一次，真实完整请求与结构效果仍待观察。
自动审批首次要求明确具体文件及模型目的地，用户随后明确授权当前目的地付费复测；
被拒绝的操作没有发送请求。deploy/.env及全部旧产物保留，未部署、提交或推送。
本次证据：`/opt/workspace/kb-tender-evidence/2026-10-27-submission-structure/`。

### 当前验证结果：v4已导出草稿，正式语义验收仍有待办

最终相关36模块完整回归**426 passed / 0 skipped / 0 failed**，145秒。v4已成功导出
总稿及商务/技术/报价三卷：30章、58响应位置，ZIP与总稿/卷子树机械一致；实际模板
检查未发现上下文整段串入或精确部件错位，价格和未知企业资料留空。

实际编制逻辑为4来源+1A+6写作+2正文复核+1F=14调用。来源/A的5个付费调用在v3已
成功，v4继承这5份回执，仅新增9次、0重试；新增335,212 Token，无未知用量。v4实耗
72.43秒是**恢复后编制阶段**，不等于从PDF开始或14次调用全新的端到端耗时。
同v4执行再次读取结果实测0新请求、0新回执、同结果引用，0.148秒，未覆盖原耗时证据。
运行代码指纹未变；本次没有改.env、部署、提交或推送。

无知识/企业资料场景下58位置最终均为placeholder，0新增正文/填值/真实附件；原模板
仍按要求保留，这不等于完成有资料的投标响应或知识库匹配验收。F确已实际发送，指出
目录遗漏/附件3与报价承诺函、8D多主体实例、技术顺序及上传安排待核；G.2未填偏差
也不能认作符合。问题保留且不阻断下载，**正式目录和内容语义尚未全部通过**，不能将
三卷存在或succeeded当作可直接投标。实物及独立审阅报告在v4证据目录。

以下保留本轮失败、定位、恢复时间线；动态事实以上述当前结果为准。

上述v1真实执行已结束，521.22秒（8分41秒），39次发送全部成功、0重试；输入761,555、
输出39,743，合计801,298 Token，未知用量0。21来源、13次写作、5次正文审阅/改后审阅；
A实际输入字节估算229,025超过94,208未发送，F也因预算未完成。仅导出“待整理内容”
一章、173位置、0正式分卷，故**正式结构未通过**，不能将succeeded当作本次修复完成。
精确模板/价格留空等机械检查通过，正文仅复述条件，缺企业资料时不能算有效企业应答。

用户随后追问4包与21包的差异，已记录BUG-055：4为原分析抽取阶段，21为编制重新读源，
同时存在标题路径重复及保守字节计量扩大分包的问题。39份供应商用量与o200k计量逐条
核对，保留512固定开销后每份都高估502，无低估；A真实保守token计量62,985，可在
现有窗口和32,768输出预留下准入。已共享heading_path并显式启用已校准编制计量，
默认/分析合同不变。源视图7模块102项与预算2模块50项定向回归通过，完整相关回归在跑。
新版离线计量来源回到4包，全部准入：94,176/94,006/93,711/83,368 Token，累计365,261；
请求wire由1,900,628降至1,409,164字节，原文字符串、完整父路径及模板锚点恢复测试通过。
这些是离线计量，不等于新4包已经实际完成或输出已验收。

独立新执行`document-submission-structure-20261027-v2`已启动，schema
`submission_structure_20261027_v2`；同模型、同目的地、无知识场景，启动前核对目的地/
模型与39回执校准对象完全相同，配置变化即停止，不按别名猜tokenizer。
新视图改变来源请求及提示，需重新发送4包，不将旧21回包补认成新语义；v1证据完整保留。
本次新证据：`/opt/workspace/kb-tender-evidence/2026-10-27-submission-structure-v2/`。

v2现场未按离线4包执行：execute_document重建预算漏传新开关，实际仍按字节计量。
已精确取消该独立执行、防止继续错误付费；全部成功回执和在途状态保留，不复用该ID。
413项相关回归全部通过仍未覆盖这处入口接线，正在补执行器级反例后精准修复。
不能将v2说成实际4包已通过；下一次使用新ID并在首个实际请求核对method与冻结spec。

v2精确取消后共10发送：9成功、1在途取消记uncertain，已报告220,610 Token，1次用量
未知，不计为免费。接线反例2项先失败，补传tokenized_authoring后入口/恢复/预算四模块
88项通过；持久spec与调用方预算全等，真实测试请求method已核对。Ruff与diff检查通过。
另BUG-055完整相关回归34模块413项通过（入口新反例另计，不重复相加）。
独立v3离线再次确认4来源包全部准入并已启动；首个实际请求须核对计量方法和冻结spec，
不将预审替代真实执行。证据位于`/opt/workspace/kb-tender-evidence/2026-10-27-submission-structure-v3/`。

v3实际验证4来源均成功，累计输入363,253、输出16,349 Token；估算每份仍保守多502。
第五请求确为A，估算50,604准入并成功，实际形成商务/技术/报价三卷，价格表安排正确。
随后宿主ConnectionTimeout，尚无DOCX。首次288.23秒、同ID恢复92.12秒均保留5份成功
回执，没有新增源/A发送。恢复记录循环阻塞35.457/13.476/40.841秒，确认同步准备及
反复全源重建问题，记BUG-056；不将其当作模型或外部网络故障。

BUG-056已复用执行内EvidenceRepository并保持原生对象深拷贝，源/A/写作/改稿准备移到
线程；来源及请求语义不变。缓存专项160项不同测试、线程及改稿相关141项、F/导出25项
均通过（分组有重叠，不相加当总数）；最终全组回归在跑。改稿准入同步补authoring_write。
独立v4已启动，继承v3的5份成功模型回执，源码指纹重新固定；不重做付费来源/A。
证据：`/opt/workspace/kb-tender-evidence/2026-10-27-submission-structure-v4/`。

## 知识匹配修复（2026-10-27，BUG-047至BUG-052）

源码已切换[knowledge_required_v2](field-dictionary.md#编制最小修订knowledge_required_v2)，
分析五节点未改；实施边界与真实待验收项见[实施计划](implementation.md)和
[验收清单](acceptance.md)。未部署、未提交或推送，deploy/.env未修改；下述无知识真实验证已启动。
既有v1及历史真实文件仍保留，不能作为本次策略的真实验收证据。

在现有大纲调用内产生业务检索意图，宿主去重、检索并绑定来源；写作/复核通过一份
supports表选择依据。背景可由招标原文支持，企业应答须有适用知识/明确企业资料，
未取得资料及最终报价留空，继续生成文件。人工/无支持空位免正文复核，但有候选
却被模型完整判空的结果仍须复核，允许一次有依据补写。首次复核不可用保留未被否定
草稿；初审明确否定后，改稿失败或再核不可用回退到已剔除问题内容的安全稿，不能
恢复已否定内容。附件须有可判断证据，复核可精确移除单份附件。

共同依据每批仅处理当前支持，宿主合并已绑定安排，不逐批重读旧材料；可读OCR正文
只投影一次，空OCR状态不算支持。跨任务旧问题只有在某次成功复核完整看到全部关联
任务时才替换，部分上下文/失败包继续保留问题。显式重审使用新的execution继承前驱，
review_request的真实范围和理由冻结进spec及相关复核/必要修订请求，不改变初次写作，
不清缓存、不加盐或覆盖旧稿。知识回执按单query及提供器范围冻结并重绑当前目标；
无法证明知识同版时不跨执行复用检索。

必要失败用例先复现后修复，最终相关回归321项全部通过、无跳过，131.90秒。使用
已有隔离测试容器，专用PostgreSQL库kb_authoring_v2_test_20261027及每例独立schema，
Redis独立空DB15，不清理共享业务数据。模型均为确定性测试替身；覆盖短引用、大纲
两入口、来源背景/source-only、语义空稿补写、混合原表及报价空白、a1局部删除、
初审/改稿/再核故障回退、跨任务问题、实际DOCX/ZIP、同ID恢复和新执行重审。
首次组合回归242通过/25跳过是测试连接变量未注入；补齐TENDER_TEST_DSN后已全部
实际执行，不将跳过计为通过。最终证据为仓库外
`/opt/workspace/kb-tender-evidence/2026-10-27-authoring-v2/final.xml`；各模块红绿记录
同目录outline-*、knowledge/、revision/及root/保留。
相关源码与测试Ruff通过，7个实现模块语法检查及git diff --check通过。

新检索意图会改变原文大纲请求，旧17份来源回包不能直接复用为新合同；真实调用前
须重新计量实际调用数、完整输入及输出容量。输出预留不是API硬限，
中文关键词召回限制仍待真实知识库纵切验证，不将短查询修正等同于召回已合格。
当前入口仍由调用方传入knowledge提供器及检索集合。用户随后明确先测无可用知识资料
场景：同一106页原件，不传knowledge/materials，使用当前.env模型gpt-6.1-terra与
131072窗口，新执行document-no-knowledge-20261027-v2，不继承旧17份模型回包。
复用既有冻结分析快照，未重新运行五节点分析。离线规划17个原文大纲包全部准入，
累计输入估算1,418,773，wire为1,497,399字节；单包输入估算最大94,115/94,208，
输出预留32,768，非供应商硬上限；未沿用旧模型tokenizer。真实执行已完成，并发2，
知识检索和共同知识依据调用为0。
不把无知识留空场景当作真实知识库召回验收。证据目录为仓库外
`/opt/workspace/kb-tender-evidence/2026-10-27-authoring-no-knowledge/`。

本次17个来源包已全部返回，796个范围各发送一次，没有来源读取重复。归并请求实际
输入估算145,661，大于94,208准入（总上下文182,525/131,072），未付费发送；
触发outline_assembly_unavailable后保留原批次91个章节节点、63个顶级章节、196个
响应位置。这是已知BUG-040的容量出口，存在跨批重复风险，不能称已完成大纲归并。

执行于2026-10-27 03:58:02至04:09:18 PDT完成，状态succeeded，实耗676.29秒
（11分16秒）。80次新增调用全部成功：17次原文大纲、37次首写、13次首核、11次
定向修订、2次再核；分阶段已按实际payload字段核对。重试、失败、不确定及未知用量
均为0；输入1,216,306、
输出50,786，合计1,267,092 Token。按字节累计输入估算4,012,901不等于付费Token。
同ID恢复实测0新增请求、0新增回执、同一结果引用，原真实耗时记录未改写。

实物检查：181个写作单元中164个占位、17个generated；实际36个非空块、0格填值、
0真实附件，无知识materials/shared_arrangements。80份实际请求全部可读，无知识来源
supports、纯知识任务写作/复核、纯人工任务写作/复核或无原件附件任务写作/复核。
DOCX及ZIP可打开且CRC、成员CAS一致；35个原生表实例的638个固定锚点、117个合并
锚点及489个空白锚点均与原网格一致，未填写原空白格。
实际Word投标价格表的单价、总价、产品参数和品牌型号格均空，原数量保留；投标函
价格引用未填金额，开标价格表的大写/金额/数量填写线及签章主体仍空。未以原预算、
限价或历史参考价代填投标价格，具体Word坐标见semantic-review.json。

内容质量仍未完成验收。36块初审未发现新增产品能力、型号、人员、业绩或价格。
BUG-053的初审定性已纠正：w25的指定有效期及起算点确有第11页前附表原文，四阶段
均通过related_rules/relations收到，不能称为凭空生成或必然错误删除例外。确认问题
是最终块只绑定第22页通用条款，未绑定支持指定值的前附表；承诺授权是否需额外资料
另审。最初只核block.source_refs遗漏完整关联上下文，原审计结论已保留并修正。
来源可读、指定数值有原文与最终引用链完整须分别核对。固定模板中的第一人称声明仍是名称、签章
未填的规定格式，应保留，不能将这些原条款本身当作模型伪造企业事实；但原模板与
另一份重写/留空版同章重复，仍属BUG-040的模板/成文角色问题。

3个局部“模板缺失”位置在后批均已有真实原表：2张偏差表和1张获奖表。归并未执行
使早期待定位目标、后批原表目标及部分局部警告同时保留；不能误报为PDF缺表。
363条规则中325条安排了目标或约束，38条保留未安排诊断，数量不等于独立业务漏项。
实际form引用146次、不同来源115个，16组来源重复、额外31次引用；不能将这些全称为
重复表格，语义实例仍待整理。最终共1377条缺口，是累积诊断数量，非1377个独立问题。
ZIP待补清单与document.gaps经同一AuthoringGap默认值规范化后完全相等；最初检查器
将可选null字段省略误判为清单不一致，已修正检查器，生产数据没有丢失。

结果位于上述证据目录的document-no-knowledge-20261027-v2/：投标文件.docx、
投标文件及附件.zip、document.json和gaps.json。原运行与全部回执保留，未静默修改
不合格内容。结构检查为acceptance.json，来源/归并审计为source-audit.json，恢复检查
为resume-check.json；完整内容审阅为semantic-review.json，问题四阶段追溯为
bug053-trace.json。结构及零知识库合同通过，内容审阅为reviewed_with_findings，
不能据此宣称真实知识库召回、五节点分析或正式投标文件内容质量已经验收。

用户随后指出目录/附件表不合常理，新增BUG-054。真实追溯：“附件1 投标函及相关声明”
首次出现在第9来源包，最终为chapter-16一级标题；原文确有附件1格式清单，但该概括
标题不是强制目录名称。其唯一form目标引用了形式评审依据，实际导出为评审表及占位，
没有绑定真实投标函等原格式。只删“附件”字样无法修复。

最终提交结构亦未验收：原文3.1.1/3.1.2/3.1.3分别规定商务、技术、报价组成；最终
技术分卷只含早批chapter-4标题（5段、0表、无响应目标），商务分卷只含chapter-3及
3.1早批内容（11段、1表），报价分卷仅早批chapter-5内容，真正表2A在chapter-50、
开标价格表在chapter-62，没有进入报价分卷。需统一原文规定结构、表单实例与章节
归属。此前“结构检查通过”仅指文件/引用/网格完整性，不代表目录和交付结构通过。
标题追溯见attachment1-title-trace.json；分卷及目录复核见私有内容审阅记录。

## 知识匹配应答修正（2026-10-27，BUG-046，已实现基线）

以下保留BUG-046的v1实现及验证历史；随后发现的BUG-047至BUG-052已按上节修复。
下述224项属于v1隔离回归，不代表当时已修复后续问题，也不代表真实知识库匹配验收。

本次用户口径替代此前“无知识仍可生成拟议方法”的设计：新增投标应答必须匹配可用知识
资料，未匹配留空，技术/实施/服务方法同样适用；招标背景、要求、固定模板照常保留，
报价最终值继续人工填写，缺资料不阻断文件生成和导出。

文档已先统一knowledge_required_v1：一次write完成语义匹配和改写，各block/fill绑定
target_ids/material_ids；无候选目标跳过basis/write；共同依据不再自由生成全源方案；
复核content_ids定位无依据内容并移除，定位不精确则清空明确关联任务应答，复核不可用
也暂不采用该批生成应答。保留原稿/回执及模板/空位，策略进入真实请求和执行spec。

代码已按合同实现。额外边界包括：纯原件无可读文字不能支撑新增正文，仍可作为附件；
填值必须属于其目标原表；共同依据携候选关联实际要求；无依据发现按任务规范化，
分批再核可以替换旧意见，不再按旧块编号误删已修正稿。缺资料不触发额外匹配轮。

隔离容器完整编制回归224项通过（117.71秒），包括25项知识应答回归、实际PG持久化、
租约、缓存继承、改稿及DOCX/附件导出；模型为确定性测试替身。Ruff与diff检查通过。
证据位于仓库外`2026-10-26-authoring-implementation/bug046-final-regression.xml`；
首次失败及边界红绿记录保留在同目录`bug046-grounding/`及`bug046-*-red.xml`。

部署和真实知识库文件验证待完成。当前Python编制入口仍由调用方传入knowledge提供器
及检索集合范围；没有提供器或可用资料时输出空位，不静默使用模型常识补写。
本次没有付费调用、没有改写deploy/.env及旧v2文件；下方v2记录保留历史实际行为，不能用其
执行成功证明新知识匹配策略已验收。旧v2未连接真实知识服务、没有真实企业材料，既有
拟议方法不能直接作为本策略合格应答。

## 当前工作区状态（2026-10-26）

### 完整编制链与本轮真实验证（T3–T5）

**本次恢复执行已结束，状态succeeded，但产物仍是有明确缺口的待补草稿。**
`document-real-20261026-v2`使用原gpt-6.1-sol配置，于2026-10-26 22:18:47至23:32:10
PDT完成，DB记录总耗时4,402.76秒（73分23秒）。消费既有未确认分析快照，本次没有
重新执行五节点分析，也未验证真实知识库或企业材料匹配。

| 阶段 | 新增调用 | 实际阶段耗时 |
| --- | ---: | ---: |
| 大纲归并（17份来源回包复用） | 1 | 3分02秒 |
| 共同编制依据 | 30 | 24分07秒 |
| 首轮写作 | 61 | 21分58秒 |
| 首次正文复核 | 58 | 20分41秒 |
| 定向改稿 | 5 | 1分35秒 |
| 改稿后再核 | 4 | 1分24秒 |

共159次新增调用全部成功，失败、不确定、重试、合同拒收及未知用量均0；上述耗时是
该阶段首发至最后完成的跨度，含阶段间组织及最终导出的总耗时以DB记录为准。
末次模型完成至执行结束13.17秒。本次输入3,000,118、输出230,803，合计3,230,921
Token；历史17次441,769 Token另计，合计3,672,690，不将旧回包复用再次记为付费。

最终产物位于仓库外`2026-10-26-authoring-implementation/document-real-20261026-v2/`：
`投标文件.docx`（163,714字节）、`投标文件及附件.zip`（549,487字节）、`document.json`
及`gaps.json`。ZIP含主稿、商务/技术/报价三分卷、清单及待补事项；CRC及各文件哈希
通过，四份DOCX均可打开。主稿70标题、675段落、101表格。没有企业证明原件，包内
无真实企业附件。汇总为`real-v2-acceptance-summary.json`，版式及语义完整性另见下文。

独立入口为`tender.document_execution.execute_document`，内部连接大纲分批组织及目标级
语义归并、可选知识资料快照、共同编制依据、完整响应单元写作、实际正文复核、一次可修
问题定向改稿及受影响内容再核、主DOCX/明确分册/原件ZIP导出。未新增分析节点、CLI、
Rust、API或页面入口。宿主管引用与执行，模型负责语义；知识片段与实际原件分离，
给定事实与拟议安排分离，旧稿和初次复核均留CAS。原报价、单价、总价、折扣等最终值
由manual_fill位置保留空白，人工最后填写，不调用模型代填或用0补空；技术内容继续。

`previous_execution_id`只复用同一分析运行中完整请求摘要相同的已接受模型JSON，
经本次validator重新绑定宿主引用；知识搜索同执行固定回执。未实现独立通用依赖图或
Word人工修改自动合并。每请求初次加三次恢复，重复同摘要并发在进程内等待并复用；
不同摘要可并发。数据库/CAS故障、取消及失租保留真实错误，局部模型耗尽保留位置继续导出。

真实验证已获用户“运行发送”明确授权并通过自动审批，独立schema
`authoring_real_20261026`内的`document-real-20261026-v1`消费历史捕获的106页未确认快照
（363规则、796范围、42表格），不把旧快照说成当前业务库已确认。原CAS不回写，新产物
位于私有`2026-10-26-authoring-implementation/real-artifacts`。实际使用gpt-6.1-sol、窗口
131072、输出预留32768、并发上限2；知识服务未配置，不能宣称实际知识库召回验收通过。

v1的17个来源请求全部成功，真实报告输入388,546、输出53,223、合计441,769 Token；
没有模型重试或未知用量。后续宿主组装失败，不是整份真实编制完成：归并被UTF8字节
计量误拒，随后多片段大纲的model_dump注入EffectiveItemRef可选null，严格读取拒绝。
全部17份回包保留，v1自然failed且没有在途调用，未清缓存或篡改发送状态。

修复仅两处持久引用输出省略未提供字段；回归覆盖原项及retained index 0/1多次CAS往返。
归并规则改为完整语义共享表及短引用，并使用独立outline_assemble.md，保持原source
prompt和分包不变。15个已完成请求离线比较均为provider prompt=o200k正文+10；仅对
归并显式开启已校准o200k，保留512 framing、32768输出预留及4096安全余量。默认分析
及其他编制请求不变，不新增多层归并。共享输入的准确准入与真实输出仍分别验证。

恢复过程中发现`.env`改为gpt-6.1-terra。用户最新明确选择“可以用原来的配置继续验证，
先跑完一遍流程”；该文件未改写，继续冻结sol配置。新执行`document-real-20261026-v2`
以previous_execution_id指向v1，已复用17/17旧来源回包，没有重新支付原文读取。
归并首次成功：实测49,119输入及12,981输出Token，164章节/233候选位置归并为
70章节/118响应位置，原候选全部安排，保留7个人工填写位置。结构归并成功不代表
363条规则全部语义覆盖；仍有38条未分配规则、17个无规则绑定目标及2条来源读取问题待核对。
共同依据30/30批、全部准入写作及复核请求均已结束，5份改稿的旧版本保留在CAS。

实际DOCX核对确认：source_refs混合上下文与模板，44次原表渲染中包含评审表9次、
投标须知表4次，同一报价表在一个章节输出3次；两类合同及渲染缺陷均已实际发生，
详见BUG-040补充。44次渲染的919个非空固定锚点及139个合并锚点保持，源文字段段落
未丢失，但这不能抵消非交付表被带入或重复输出的问题。报价表三份副本的单价/总价
6格均空白，人工目标涉及的8个原空白锚点均未填入。只有1条export_cell_fill_unplaced：
叙述任务重复提交同章表单单元格填值，因不属于该任务form位置而未落位；相应form
任务已有同格填值，封面及报价未因此丢失。详细证据为`real-v2-final-export-review.json`
及早期`real-v2-template-export-review.json`。未进行分页视觉或逐项语义完整性验收，
未热改在途代码；真实流程结束后仍需修复模板角色/实例合同及预算误拒。

最终共同依据下只读重放规划得到113个写作单元：61个可准入模型请求、13个单目标
预算占位、32个缺本任务证明原件占位、7个人工填写占位。13个完整请求的UTF8估算为
96,970–218,100，高于94,208准入；o200k正文加512 framing仅31,197–71,789，全部可保留
32,768输出及4,096余量。明确为写作仍走byte计量的宿主误拒，不是模型调用失败；
其中关联规则/问题/关系合计占载荷字段体积约74%，压缩问题另计。已保存
`real-v2-writing-admission-plan.json`及`real-v2-writing-budget-diagnosis.json`，
本轮保持冻结执行配置完成，不以占位导出替代完整正文验收。

最终61个准入首轮写作请求全部成功，无重试；实际单元
为56 generated、57 placeholder，后者包括52个宿主预定占位及5个已接受的留空回包：
4项财务表缺真实财务数据，1项可选配置清单缺实际配置/分项报价事实，均error_type=null，
不能记为模型执行失败。初审实际79批：58批准入覆盖92单元，21个单单元
请求预算拒发。58次初审及4次再核均已完成，最终保留151条finding；不能说113单元
均完成独立内容复核。汇总gaps为1,665条，含继承的问题及多位置提示，不代表1,665个
独立故障；证据为`real-v2-acceptance-summary.json`及
`real-v2-review-admission-plan.json`。
这21批另行精确对照：UTF8估算95,493–217,067，o200k正文加512仅30,067–71,525；
保留输出及安全余量后66,931–108,389，全部低于131,072，亦属计量误拒。相关规则/
问题/关系累计1,957,531字节，原文376,847字节，正文content仅24,437字节；不能把
输入膨胀误归因于生成正文太长。本轮配置保持冻结，未热改预算或清除已接受回执。

隔离PG+合成SSE完整纵切已经验证：未确认分析、原表填值、真实PNG装配、某章4次失败
仍输出真实DOCX/ZIP、同ID零发送，以及新执行精确复用后仅重写失败章及复核。
`test_authoring_revision_integration`验证只读A/B不能消除仍涉及B/C的旧问题；实际Word
保留A新稿、C原稿、B占位及旧稿CAS。真实v1修复前的全部unit及本轮受影响持久验收同轮
**1241 passed（279.73秒）**，覆盖当时的输入、组织、报价留空、改稿、导出、独立执行
与共享恢复。此后引用序列化及归并计量修改使用定向回归，不能将1241项视为修改后的
完整复跑，也不累加此前分轮结果。
后续定向报告分别为引用往返152项、归并计量34项、执行及任务穿透37项、最新编制
边界54项通过；各轮有重叠，不累计为一次全量测试结果。实时任务中未再修改源码。
静态检查通过，122个本地文档链接路径均存在。

全部证据放在`/opt/workspace/kb-tender-evidence/2026-10-26-authoring-implementation/`；
v1日志为`real-document-live.log`，当前v2日志为`real-v2-document-live.log`，
只读监控为`real-v2-monitor-latest.json`，全量回归报告为`final-regression.xml`。
未部署共享服务，未提交或推送。

### 真实投标成品完整内容审阅（2026-10-27）

本轮按用户要求阅读主稿及商务/技术/报价三分册全部70标题对应的正文/表格和113个
最终写作单元，逐项核对大纲、共同依据、实际初审/改稿及相关原招标条款。技术部分
27标题、37单元单独逐章列出，商务及报价全部生成单元及实际章节亦已阅读；封面和
附件位置另核对。没有付费模型调用、生产代码修复或原产物改写，不以此声明分页视觉
验收、扫描原件逐像素核查或真实评标通过。

内容结论：当前稿件存在实质质量问题，仍是需要修正的编制中间稿，不能根据执行完成
和56个generated宣称完整实质响应。实际存在以下已确认问题：

- 专用附表明示本次不适用的商业运行备件，仍被写作和报价位置重新设为本项目义务；
  不能归因于缺企业材料。大纲、共同依据及任务原文遗漏了项目适用边界。
- 原件要求按原结构逐段应答，当前只有主题方案及待定稿说明，逐段主任务预算缺失；
  质量安全风险变更专章也未生成。售后母任务虽空，两个子章已有具体内容，不能把
  整个售后评分项误说成完全未写。
- 商务及技术多处把编制、取证、核验及评分教学当正文，内外职责边界不清；有的
  generated财务单元只有待补填值，非空结构指标不代表财务内容完成。
- 承诺模板和模型重写全文同时输出，出现双份签章/承诺；原生section跨附件，使
  相邻保函进入另一章。已发现的表格重复只是这一合同缺陷的一部分。
- 完整原件已有的资格条款未送到当前任务，正文却称资料缺失；部分真实业绩门槛
  又被局部复核误判为无来源。全局原文确实缺失与本任务未发送必须分清。
- 复核只见模型内容，没见宿主实际组装结果；既漏报成品重复，也以模型未画原表
  误报缺表，引发对宿主本会输出表单的改稿。初审148条含127条模型finding及21条
  未复核诊断，不能混作148条已完成的语义发现。
- 成品有19处局部来源短号（6个内容块），72段以解析标题标记开头；内部核对任务
  也成为公开章节。功能说明、预装流程重复，同一开发方案两套时间安排未明确衔接。

可保留的内容包括现场安装/培训、三年质保及硬件12小时到场、每批30天预装、到货
与预装节点区分、人员培训/资格审查/考试及监督整改等具体方法。未知价格、财务、
型号、资质及业绩事实继续留空合理，不新增资料确认门禁，也不要求模型虚构满足结论。

问题根因已追加BUG-041至BUG-045，并补充BUG-040模板缺陷。当前仅审阅和记录，
尚未实施所列修复。完整私有报告为`real-v2-content-review.md`（含70标题清单、评分
对照及修复边界）；详细技术报告为`real-v2-technical-content-review.md`，商务证据为
`real-v2-business-content-review.json`，均在本轮仓库外证据目录。原件对照文字为
`real-v2-review-original-layout.txt`；报告附成品SHA256，原文件保持不变。

### 可用编制输入与独立执行适配（BUG-040，T1/T2）

输入合同已改为固定实际可读根及成功抽取子包；缺确认、未完成分析、业务问题、不可读
资料均带具体诊断继续提供可用快照，不伪造analysis-ready或确认。核验投影可用时沿用
current_items的既有接受决议；否则保留未核验抽取，只有原文时装载来源。
发送视图rule_origin明确区分verified_projection/extraction/source_only；原始提取不被
标记成有效补遗投影，后续资料和来源问题进入大纲问题，不静默丢失。

execute_document与AuthoringStore优先恢复固定输入；后续分析修订不废弃旧版本成果，
结果仍绑定原快照。心跳与发布保留显式取消、独立epoch/租约、存储围栏，移除重复确认
门禁。Store支持outline、authoring_basis、authoring_write、authoring_review，sent与缓存
按task+logical_request_digest隔离，每个完整逻辑单元共用首次加三次发送；不同章节不再
争抢同一逻辑请求身份或相互消耗额度。稳定ID不热换快照，不增加CLI或Rust入口。

该阶段同轮编制相关回归 **174 passed（103.74秒）**：包含全部unit/test_authoring*.py、
unit/test_outline*.py、独立AuthoringStore及既有输入/大纲/执行三组acceptance，使用真实
隔离PG/Redis和合成模型/SSE。覆盖缺确认/部分抽取/零资料、损坏引用、保留补遗、分析
修订期间完成、同ID缓存恢复、取消/租约、多逻辑单元独立重试，以及8项正文链边界回归。
正文边界包括空输出占位、跨位置资料选择、非事项项目背景和复核载荷不含持久CAS身份。
新增文档持久执行入口由其独立回归报告覆盖，不包含在此174项结果内。

报告位于`/opt/workspace/kb-tender-evidence/2026-10-26-authoring-implementation/`的
`snapshot-final-regression.xml`；此前136项、84项和8项分轮报告仍保留，不与最终结果
相加。原分析workflow无改动，STAGES与默认Task及模型提示快照保持既有范围。
上述合成回归没有使用付费模型，不等于真实投标内容的语义验收。共享服务未部署；
本轮真实编制运行进度以上方完整编制记录为准。

上述174项之后，核验问题/关系投影复核新增2个边界回归，先复现**1 failed、9 passed
（4.07秒）**：只有source_refs的背景任务遗漏原文问题及人工意见。补齐来源范围路由及
共同依据中全局问题的人工意见后，整个边界文件**10 passed（4.17秒）**，Ruff通过。
同时验证无关长问题不进入其他局部包；报告为`analysis-context-boundaries.xml`与
`analysis-context-boundaries-fixed.xml`。这是后续定向回归，不与174项相加，也不表示
在新增投影后再次执行了全部174项。

按新增“投标报价留空、最终人工填写”要求，补manual_fill纵切回归：边界文件
**11 passed（3.47秒）**，Ruff通过。测试实际读取生成DOCX，原报价表头和200000元
招标限价保留，总价/单价格仍为空字符串，没有采用历史123456元/888元或填0；其他
技术正文存在，write只调用技术任务一次。复核回包即使要求自动改报价，也不会让人工
占位进入改稿。报告为`manual-quote-boundaries.xml`，使用合成来源及mock语义，不是
真实报价或真实模型验收；不与此前各轮结果累计。

### 多批大纲引用序列化修复（BUG-040）

针对真实v1暴露的EffectiveItemRef显式null问题，先新增实际预算分包、归并后同时包含
原项与两条保留规则的往返回归，**1 failed（2.85秒）**准确复现。仅在多批大纲返回与
WritingTask序列化两处使用exclude_unset=True；未放宽分析schema，也未修改Prompt或
Token估算设置。修复后同例**1 passed（2.89秒）**，保留规则索引0/1、明确允许null的
AuthoringInput字段，以及正文装载按引用查找规则均验证正确；outline和task分别经过
两轮CAS保存/重读/校验。随后全部authoring/outline单元同轮**152 passed（17.57秒）**，
Ruff与diff检查通过。报告为`effective-ref-roundtrip-before.xml`、
`effective-ref-roundtrip-after.xml`和`effective-ref-unit-regression.xml`。
该轮未重跑完整持久验收或真实编制；实际v1失败及后续恢复状态以顶部实时记录为准，
不与前面的1241项或其他分轮测试累计。

## 历史工作区状态（2026-10-23）

### Excel表外跨行合并修复（BUG-038）

本轮按BUG-038继续最小修复：先补字段字典与问题记录，再补原生解析及冻结/装载失败
用例。旧实现5 failed、21 passed（6.15秒）；仅修改excel_parser.py行定位的一处筛选，
按物理矩形相交保留完整原始合并坐标，定义表格仍按完整包含生成grid。修复后聚焦
26 passed（6.29秒），包含纵向、带空尾列的横向、二维合并、同行列不相交反例及
实际Excel解析→冻结→大纲CAS→来源装载往返，行region与表内稀疏grid均保持原义。

解析器源码摘要使parser_version变化，同步生成合成protobuf回放夹具；逐字段核对仅
metadata.source_contract.parser_version变化，不改Rust执行逻辑、通信字段或业务节点。
最终同轮相关回归**255 passed、2 skipped（69.40秒）**，无失败/错误，覆盖原生Excel、
结构化来源、完整请求/短引用、原生证据、分包/预算、来源配置、大纲组织及执行时序。
跳过项为缺少en_mergecell.xlsx及en_calcchain.xlsx；未计作通过，未重跑完整Python套件。
Ruff、git diff --check及文档本地链接/锚点检查通过，独立只读复核无新增发现。

证据位于`/opt/workspace/kb-tender-evidence/2026-10-23-xlsx-merge-fix/`，包含修复前后及
最终XML、跳过原因、源码指纹和合成夹具差异。使用断网容器及合成文件，真实模型调用0，
没有使用DB/Redis；未部署或修改环境，未进行真实招标文件语义验收。
本轮唯一临时测试容器及匿名卷已清理，未创建数据库、缓存或专用网络，未触碰共享开发栈。
纯空白表外布局及模型合并语义不在本批范围，旧冻结产物不会自动补齐；需要补齐旧文件
时沿原来源修订重新冻结和分析确认，不原位改写旧CAS或热换旧快照。

### 大纲来源完整装载修复（BUG-036/037）

只读复审已确认两处来源装载缺口：图像unit之外的冻结OCR/图片/锚点未返回；Excel行
绑定grid时遗漏表外单元格，装载器也未展开grid/row所依赖的共享单元格。先补字段字典
及问题记录，再以9 failed、5 passed（5.46秒）复现，最小修改tender/authoring.py：
保持document/unit原样，补齐冻结OCR/图片/锚点及所需单元格；按原生行坐标补表外行引用，
完整grid仍去重。字段值只在读取副本中恢复，不重新OCR、计算公式或改写CAS。

修复后聚焦14 passed（5.07秒）；补充实际Excel解析/冻结到大纲来源的合成文件往返。
新增测试夹具的ordinal和freeze返回引用使用错误已纠正；最终同轮相关回归
**163 passed（52.67秒）**，无失败或跳过，包含15项来源用例及大纲组织、条件锚点、
共享合同、原生证据、分包、预算、短引用、完整请求及执行时序。未重跑完整Python套件，
不沿用或累加上一批结果。本次无DB/Redis依赖，测试容器断开外网，真实模型调用0。
Ruff及git diff --check通过，独立review-agent只读复核无新增发现。

另以定义表B1:C2及表外合并A1:A2的合成文件确认BUG-038：原件保留，但冻结unit的
merged_ranges全部为空，根因在原生解析的单行包含判定；该批仅记录，当前修复见上节。本批只保证
已有冻结结构不被装载器丢弃，不能恢复冻结前已遗漏的合并跨度。

报告、源码指纹及合成复现位于
`/opt/workspace/kb-tender-evidence/2026-10-23-outline-source-fix/`。
本轮唯一临时测试容器及其匿名卷已清理，未创建专用数据库、缓存或网络。
本批不增加模型调用、业务节点、Prompt或持久字段，未部署、未修改环境或共享开发栈，
未使用真实招标文件验收；该批之后的BUG-038修复见上节，实际模型的大纲语义/容量仍待验收。

### Python大纲独立持久执行（T2）

先补[独立执行字段](field-dictionary.md#t2独立持久执行)与BUG-035，再实现
`tender/document_execution.py::execute_document`。调用方固定execution_id及父分析确认身份，
同身份重新调用即恢复；`integrations/authoring_store.py`用两张独立PG表管理epoch租约、
请求及少量CAS回执。分析run、五节点、默认Task和原确认均不变；没有新增CLI或队列。
本批未接dispatcher，进程退出后的继续执行依赖调用方再次启动，不表示无人值守接管。

共享OpenAIModel完整请求发送入口，保留SSE、超时、响应脱敏、用量及原始审计，不复制
HTTP栈。既有分析调用仍按原构造、短引用及预算委托；拒收恢复只增加编制所有者元数据。
同一编制执行仅一个outline逻辑请求，所有真实发送跨epoch共用四次；反馈变更不重置，
已sent或其他副本已成功时不能再发。传输成功后合同拒收保留成功缓存，再带真实错误
纠正；成功回包或拒收回执保存后中断，同ID重入复用既有响应和反馈。

执行快照固定实际outline提示、Chat地址/模型/窗口及预算，不含密钥或分析提示集合；
密钥轮换可复用，其他已固定配置变化报冲突。纠正完整请求再次准入，预算不符不会
新增HTTP发送，统一重试后仍返回明确预算错误。取消/失租停止工作，迟到响应只补审计；
发布事务同时检查父分析仍为同一已确认修订及独立租约，避免返回前快照检查的竞争窗口。
首次并发初始化的PG目录冲突也已先复现，再用schema级事务锁修复。后续只读review确认
原同时结束优先工作仍漏掉客户端关闭窗口：成功发布后，客户端尚未关闭即可能被心跳误报
失租。本次按BUG-035调整为关闭客户端、停止并等待心跳、原事务围栏发布、返回。

上一批验证使用隔离PG/Redis及合成HTTP/SSE，未调用真实模型。首轮相关回归387 passed、1 failed
（122.47秒），唯一失败为新加入的首次并发setup用例在修复前执行；最小锁修复后完整
Store回归25 passed（5.45秒）。最终执行入口及心跳竞争回归11 passed（85.83秒），包含
成功/拒收中断恢复、统一耗尽、取消、父修订、配置变更/密钥轮换及纠正预算出口。
逐项合并XML去重，该批399个相关用例分轮通过，无剩余失败或跳过，不写成一次完整
399 passed。Ruff、git diff --check及221个本地文档链接/锚点通过；报告和源码指纹位于
`/opt/workspace/kb-tender-evidence/2026-10-23-authoring-execution/`。
该批三个隔离测试容器及专用网络已清理，证据保留，未触碰共享开发栈。

本次发布时序修复用5项事件驱动测试先复现旧实现：4 failed、1 passed（4.03秒）；
最小修复后5 passed（3.51秒）。最终同轮回归40 passed（91.52秒），无失败或跳过，
包含上述5项、25项独立Store及10项执行验收，验证关闭期间续租、关闭失败、组织及关闭
阶段真实心跳错误、等待心跳停止时的调用方取消和原有恢复/发布围栏；未重跑完整Python套件。
测试使用隔离真实PG/Redis及合成HTTP/SSE，真实模型调用0；本次修改的Python文件Ruff
及git diff --check通过，独立只读review未发现新的确定缺陷。报告及源码指纹位于
`/opt/workspace/kb-tender-evidence/2026-10-23-outline-publication-fix/`。
本次三个隔离测试容器、匿名测试卷及专用网络已清理，证据保留，未触碰共享开发栈。

未部署或修改环境、Rust/API/页面，未确认上次真实文件；实际模型及真实招标文件的大纲
语义和容量验收仍待完成。下一步先验证技术条款/评分/表格样例的真实模型执行，再接T3
资料匹配、T4正文/填表、T5核对/导出和T6完整文件验收。

### Python大纲与响应位置（T2，内部组织）

先在[唯一字段字典](field-dictionary.md#python大纲与响应位置t2)定义EffectiveItemRef、
SourceUnitRef及大纲/响应位置合同，再于`tender/schema.py`实现；
`tender/authoring.py`提供load_outline_source和大纲校验；正式组织由`authoring_pipeline._organize`完成。
模型按有效要求、评分及规定格式组织章节；宿主产生章节/位置身份、还原短引用并保存CAS。
同一规则可进入正文、规定表格和证明附件，局部补遗多条保留规则分别引用，不复制旧评分。

输入复用T1当前已确认分析及current_items；相关原文按完整原生unit供给一次，其他来源
只给可用性目录。表格保留明文及空行压缩，片段保留无正文alias；XLSX行映射完整grid，
横向多表不丢引用，虚拟空白页不可当模板。未分配/未绑定/模板缺失/正文未读均保留具体
问题及原有人工出口，不增加业务节点、状态或语义规则。条件元数据锚点同样可回溯，
只有pending元数据不视为已阅读。原人工意见及未核验关系继续保持原义。

该批是接收执行端的Python领域函数：交付完整请求和同步校验器，调用前完整准入，返回前
重查确认版本。分析模型客户端和持久回执依赖活跃分析租约，不能借用来编制；本批没有
接通独立真实付费执行、成功缓存、首次加三次持久恢复或队列。完整请求超限发送0次，
不截断输入，也未实现超限全文件分批合成；显式输出预留需真实样例校准。

新增类型先在入口缺失时失败，关系短引用、虚拟空白页/XLSX来源、条件元数据锚点均先以
失败用例复现再修复。验证使用测试模型执行端和隔离真实PG/Redis，未调用真实模型、
未确认真实招标文件，未部署；不能将本批记为投标编制全流程完成。
最终相关回归333 passed（54.41秒），无失败或跳过，包含30项T2用例和T1/预算/短引用/
原分析兼容回归；此前聚焦27项通过，补充片段和条件锚点后合并复跑，不重复累加。
Ruff、git diff --check及本地文档链接/锚点检查通过。
报告及代码指纹位于`/opt/workspace/kb-tender-evidence/2026-10-23-authoring-outline/`。
本轮三个隔离测试容器及专用网络已清理，证据保留，未触碰共享开发栈。

该批之后的独立持久执行现见上节；技术样例真实调用及T3至T6仍须继续，按任务规划推进，
先定义字段，不新增CLI或Rust/API/页面接入。

### Python编制输入首批（T0/T1）

先在[唯一字段字典](field-dictionary.md#python编制输入t0t1)定义AuthoringInput，再于
`tender/schema.py`实现严格共享类型、`integrations/authoring.py`提供read_authoring_input。
输入只含现有分析身份、确认摘要和五份根产物引用；校验当前analysis-ready、CAS、引用及
持久确认一致性，兼容v2旧normalization。复用current_items和EvidenceRepository按需装载，
保留局部补遗、人工意见及未核验关系状态，不创建新事实台账、CLI、节点或状态。
本轮新增模型调用0，未修改Rust、公共API、页面、环境变量或部署。

新增用例先因入口缺失失败，修复后首轮49 passed；补充片段原文/定位后，相关回归
284 passed（100.17秒），共享合同补充61 passed（10.62秒），两轮去重321项全部通过，
无失败/跳过。其中新增入口用例47项单元、3项真实PG验收，覆盖确认前后、保留疑点及
领域修订后旧版本失效；PG/Redis均为独立容器，模型是测试替身。Ruff、git diff --check
及文档本地链接检查通过。报告位于
`/opt/workspace/kb-tender-evidence/2026-10-22-authoring-input/`。
本轮三个临时测试容器及专用网络已清理，未触碰共享开发栈。

尚未部署或用真实已确认招标文件验收本入口；上次真实运行仍待人工确认，未自动确认。
T0该批只固定输入合同，后续产物仍在各任务编码前细化；T2内部组织现见上节，独立真实
执行及T3至T6尚未完成，不能将输入读取完成记为投标文件生成已跑通。

### 既有分析优化

本轮继续完成[discovery已有关系供给](bug-fix.md#已有关系供给修复已实现隔离回归通过)，
normalization_version=8：本批两个端点均在场的完整Relation及其evidence进入discovery，
目录同时映射关系证据；候选和finding允许引用已提供的关系证据，其他来源仍拒收。
初始分块之后按实际完整并集准入，必要时确定性二分事项块并重建全部块对，保留全部
事项对及关系；不可分超限仍走原预算修复出口。共享verify Prompt同步引用边界，
全部verify摘要受影响，仅新执行使用；没有部署、付费调用、人工确认或提交推送。

核验职责与调度的[最小方案](bug-fix.md#核验职责与共读修复方案最小修改已实现)已实现，
此前normalization_version=7：joint仅覆盖dispositions=[]，其余原文及对象保留，该轮共享Prompt未改；
review/locate/associate/joint阶段内复用既有并发2，依赖阶段仍依次推进。相同宿主请求
身份先加局部锁再获取额度，保存后复用回执；按计划顺序归并。单元最终失败后不启动
排队工作单元，已启动者继续按原重试额度结算；恢复时复用成功者，执行失败和未启动者。
保留原重试、付费围栏、取消/失租及人工出口，没有增加节点、模型字段或配置。

上一轮定向测试先在旧实现复现，再修复：完整单元1020 passed（164.92秒），包含14项并发/恢复
及4项joint职责/反证用例；Ruff、本地链接/锚点和diff检查通过。首轮单元因临时容器
缺少仓库合成语料文件有9项环境失败，补齐原仓库fixture后完整重跑通过，没有修改语料。
临时PG/Redis上的workflow/acceptance共138 passed（349.36秒），不使用共享业务库、
模型密钥或外部模型。三个临时测试容器及专用内部网络已清理，未触碰业务栈；
该轮没有部署、付费调用、人工确认或提交推送。

首轮流程回归136 passed、2 failed：旧重试fixture按整个阶段共用四份坏回复，旧joint预算
预期仍继承处置。已按实际工作单元隔离回复、逐单元检查首次加三次及反馈/审计，并更新
joint预算预期；两项定向验收及最终完整回归均通过。失败费用复审另用失败测试复现
排队调用继续发送，加入阶段停止标记；完整单元和两项定向验收均在此最终源码上通过。

既有relations供给已按本节首段实现，整章补读精度仍仅为后续评估项；新增关系可能增加
discovery分包，没有可靠范围引用时整章读取可能必要。当前修改不证明104次关联
或全程耗时已减少，实际提速及语义行为待新版本真实对照。旧版本运行不热切换，
后续验证须新建运行并保留旧回执；下节为上次部署与真实运行记录，不代表本轮部署状态。

后续项的只读预算核查：同一原运行的1份discovery和4份extract-pack均校验CAS摘要；
原87关系的113处证据引用（73个唯一范围）都在当前460个事项引用/目录范围内，缺失0。
用现有容器、相同完整预算spec及verify Prompt哈希复现基线：输入估算86431、短引用
请求328930字节；仅补现有87关系后为93057、354625字节。输出预留32768、安全余量4096、
窗口131072保持不变，两者均fits；补充后输入距94208准入上限仅1151。此文件仍可一次
discovery，不据此承诺其他文件的块数或真实输出完整性；未发新模型请求、未改源码，
没有重复候选、后续associate/joint或耗时收益的验证。

本轮验证：新增用例在旧实现先复现5 failed / 2 passed，修复后定向125 passed，完整
单元1027 passed（184.80秒）。首轮定向的1项新fixture把未知语义提示误当作非阻断，
改为断言保留该人工疑点及无硬阻断，没有修改宿主问题分级策略。全部相关Python的
Ruff和108个本地文档链接/锚点通过。

临时PG/Redis上的workflow/acceptance首轮134 passed / 4 failed（514.12秒）：3项成功
fixture的刻意scope填充使旧事项对已达14332/14336字节估算，Prompt新增后14441，
生产准入正确拒发。成功fixture重复数12→11，重分包/覆盖/恢复断言保留；真实不可分
超限用例不变。另1项worker重投测试在导入broker前未设置TENDER_REDIS_URL，仅传
TENDER_TEST_REDIS_URL，导致连接默认端口；修正临时容器的测试启动环境，没有修改
broker、业务配置或生产预算。4项复验全部通过（36.91秒），逐项对照XML确认覆盖完整
138项，无遗漏或跳过；这是分轮通过，不是重新一次完整138 passed。报告及代码指纹
在本节证据目录的validation-summary.json、acceptance-initial.xml和acceptance-fixed.xml。
三个临时测试容器及专用内部网络已清理，未触碰业务栈；未部署或调用真实模型。

真实文件只读重规划先精确复现旧locate计划，CAS读取逐份校验SHA-256，再装载相同
source review回执；363事项、87关系、65703个事项对保持完整。采用本轮实际Prompt，
discovery仍1次，输入估算86431→93077，短引用请求328930→354735字节；输入准入
94208，剩余1131。reference仍36次，累计输入估算709692→710304（共享Prompt增加
612估算Token），两类请求全部fits；输出预留32768、安全余量4096不变。没有新模型
回包，不能据此宣称候选减少、下游G/K减少或实际提速。对照报告与修改前源码/Prompt
保存在`/opt/workspace/kb-tender-evidence/2026-10-22-discovery-relations/`，未修改旧回执。

## 上次部署与真实运行（2026-10-21 UTC）

normalization_version=6 已于2026-10-21 02:59 UTC部署。三个Python角色统一镜像
`knowledgebrain-docreader:20261021-retry-v6`，镜像ID
`sha256:578de027146e21279cf90a29e735574460617a6c8ff5295b3517e60a33878bbb`；
53份运行源码/Prompt指纹与工作区一致，init退出0，worker/dispatcher运行。
复用现有PG/Redis及`knowledgebrain_default`，健康预检通过，无新增宿主端口；
并发仍为客户端默认2，窗口131072，Chat/VLM沿用各自既有配置。

用户调整环境变量后，2026-10-21 03:01 UTC重新force-recreate三个Python角色。
Chat/VLM现均为各自显式配置的`gpt-6.1-sol`，窗口仍为131072；
标准`/models`探测均200且模型在列表中，PG/Redis正常，53份运行指纹仍一致。
没有关闭TLS校验或修改用户配置；此结果不证明旧端点已经续期。
新独立运行`e28605cb-468f-46ed-b2cf-e22530ce7025`已正式提交，
source_version/domain_revision均1，request_key=`real-v6-full-20261021-0302`。
自动分析已完成，停在`waiting-confirmation`；尚未人工确认，未到`analysis-ready`。

04:19:34 UTC自动执行结束：2 OCR、4 extract、16 review、37 locate、104 associate、
22 joint全部完成，共185次成功、实际188次发送；3次早期抽取中断通过工作单元重试恢复，
未知用量仍保留，无在途请求。command执行一次、基础设施失败0，租约已释放。
最终363事项、188关系、796原生范围全部覆盖，执行/来源覆盖缺口0，hard_blocking=false。
关系中117条进入独立核验，71条仍标记unverified；进入核验不等于语义已通过。
22条决议最终仅接受2条、解决2问题；累计639问题中637条仍active并进入人工出口，
其中70条原始明确未决仍保留。问题数不是去重后的独立业务缺口数，不能据此宣称语义验收。
尚无人工确认或事项修订，不自动代替用户裁决。已报告3193067输入、235165输出、
3428232总Token，另3次中断用量未知，不能据此计算完整费用。

最终阅读版及完整JSON/Markdown已从实际产物只读导出到私有证据目录
`/opt/workspace/kb-tender-evidence/2026-10-21-full-run/output/`：
`final-analysis.html`可按类别和关键词查看事项、原文引文及页码、关系、处置、问题及决议；
`final-analysis.json`保留实际字段，`final-analysis.md`为完整文本，`source.pdf`为同SHA原件。
导出逐项核对363事项及637待确认问题与终态一致，796处置无重漏，2008条展示记录完整；
报告见`export-validation.json`。导出未触发模型调用、语义合并或人工确认。

### 2026-10-22耗时与任务展开复审（原回执只读计算）

同一运行从命令开始到结束为4624.38秒（77分04秒），verify为3392.83秒（56分33秒）。
核验请求sent_at到finished_at区间峰值在途均为1，配置并发2未用于阶段内调度，见BUG-032。
review16次跨度311.63秒，discovery1次57.69秒，reference36次741.99秒，
associate104次1732.37秒，joint22次476.90秒；阶段规划、收据及间隙另计。
没有核验失败重试，不能把该耗时归因于重试；并发只能缓解等待，任务展开仍见BUG-026。

关联104包中103包仅1候选、1包5候选。原计划出现5500次range身份，来自697个唯一range
（含上下文），平均7.89次，同一range最多进入91包；2367次item身份来自305个唯一事项。
这些是请求来源重复计数，不等于所有原文重复都可以删除。108候选中88个选择完整章节，
24个关联请求携带受保护的明确未决问题；模型继续分析不等于它有权消除保护。
关联产生101关系、22提案、133新问题；没有提案的82包仍可能提供有效关系或缺口。
joint22次中模型认可21提案，18条因所在包另有blocking findings未接受，剩余3条中
1条又因保护问题未接受，最终接受2条、解决2问题。哪些疑点与决议无关尚需语义审查，
没有据此自动放宽接受或删除问题。核验实报输入3099934 Token，为抽取92598的33.48倍；
其中含必要独立复核、不同目标的联合阅读及提示开销，比例不能证明可裁减的原文或任务数量。
优先核实结果归属与重读任务是否重叠；本次只读复审及文档记录，新增模型调用0，
未改协议、源码、部署或确认状态。

### 重读合理性补充复审

全部188发送对应185个唯一(task, request_digest)，成功后同摘要再发送0次；额外3次是
两个抽取包的uncertain恢复。四个核验阶段计划各1份，无递归或外层重跑。去掉work_unit_id
后，183份非OCR模型payload仍无完全相同组。179份核验视图内，同一来源字符区间重叠0；
不能把累计range身份次数当成单包原文重复。关联最高频的context-80c8…实际是10字符卷标题，
进入91包，作为章节定位上下文合理；此前频次统计不能证明91次大段重读。

六组代表对照中，joint-21d40a82366a与joint-44357c88a243共享6个来源范围，但分别核对
续接条件和一般规则的局部例外，两提案均被接受，不能仅凭同源删掉一个任务。
associate-38515729f5bf与associate-95258a6534ae的source_views/items/relations/dispositions
完全相同，来源、事项及所指7个章节相同，均核对同一资格材料引用；问题绑定和输出不同，
属于可评估共读的重叠任务，尚不能直接删除任一输出。joint-0df88a0626fd继承48事项、
101范围及80处置，确认在核对一条提案时同时再次报告其他已知缺口；同一保证金金额来源
共15条问题记录（13来自joint），联系人来源9条。这些是记录数，不等于新业务缺口数，
也不证明相关原文都可裁剪。未完成104关联逐包的语义裁剪验证，未给出可省调用数或时间承诺。

报告与CAS引用位于`/opt/workspace/kb-tender-evidence/2026-10-22-reread-review/`的
`reread-review.md`与`reread-review.json`。本轮新增付费调用0；只更新问题记录及交接，
原始产物、协议、源码、部署和人工确认状态均未改变。

### 核验职责与共读设计（2026-10-22，评审记录）

只读统计104份associate原始发送投影，逐份核对CAS SHA-256；除work_unit_id、candidates、
issues外，其余公共字段逐字段比较，得到102种完整上下文、2个双任务组。因此仅合并完整
相同上下文的结构上限是少2次associate，尚未验证新输出容量或语义，不能作为主提速。
统计使用既有2026-10-21-full-run/live/latest.json及artifacts，无新增模型调用。

三轮复审后收缩[方案](bug-fix.md#核验职责与共读修复方案最小修改已实现)：
一，原共读草案按显式引文重建joint依赖可能漏掉模型未引述的上下文，已撤回该裁剪规则。
二，现有usage可直接作为输出校准起点；共享verify Prompt变更会影响全部核验请求摘要，
不能只算joint。三，通用groups/group_results收益不足以支持立即开发，降为离线证明
完整链路收益后再评审的备选。

已重新按成功回执核对：associate104次实际输入1488915、输出69260、单次输出中位525.5、
最大3318；joint22次输入497394、输出19593、中位984、最大1629。这是旧合同单文件
观察值，不是新包上限。缩减joint续报不会回头减少已完成的104次关联或36次引用补读。

当前建议先仅覆盖joint发送的dispositions=[]，保留原文、对象和旧问题；现有Prompt已有
相应职责边界，先不改Prompt，后续用真实模型新调用对照。032并发调度单独修复，不增加额度，
不把等待下降算作减少调用。源码、Prompt、环境变量、部署及确认状态均未变；本轮新增
付费调用0，未宣称提速或真实验收完成，原运行仍等待人工确认。
本轮仅更新设计、字段约束、验收和交接；105个本地链接/锚点及git diff --check通过。
未运行代码测试，未新增运行证据文件；上述统计只读复用既有真实回执。

### 前述运行过程快照

03:09 UTC快照：登记、冻结及2次去重OCR完成，796个原生范围进入4个抽取包，
全部请求通过现有输入准入。尚无抽取包接受，2个请求在途；已有3份uncertain回执，
两份在约108秒持续输出后传输中断，另一份在约44秒收到供应商
`server_error/request_timeout`。三份均无完整JSON/DONE/usage，不能统计实际用量。
两包分别已实际发送2次、3次，logical_request_digest与command保持不变，
execution_count=1、infrastructure_failures=0，工作单元重试未被外层失败预算抵消。
worker租约与心跳正常；不得将`/models`200或OCR成功宣称为完整文件验收。
已报告用量仅OCR的535输入、127输出、662总Token；不含在途或中断调用未知用量。
问题机制、根因边界与处理见BUG-031，原始流完整保存在新目录CAS，未提交人工确认。

03:23 UTC：抽取4/4包及总索引全部提交，363事项、796处置、202待核验问题，
正在准备独立核验。前两包分别在第2次、第3次实际发送后成功，后两包首次成功；
command始终execution_count=1、infrastructure_failures=0，无失租或外层重投。
累计已报告93133输入、102884输出、196017总Token（含2次OCR），3份中断用量仍未知。
首包只读审查确认187个正文范围对应187个唯一处置，95事项与15关系的197条引文均可定位；
模型声明47 non_item、24 unresolved，宿主按事项证据派生116个items处置，无恢复丢弃或诊断。
该包17个模型问题加24个明确未决范围问题=41；这是来源明确的宿主派生，不是模型重复输出。
首包实际26110输入/27548输出，输入估算110058约为实际4.22倍，输出超过16384预留。
完整回包正常接受；该结构与引用检查不证明全文件语义召回或五节点验收。

03:26 UTC：完整抽取只读审查确认4包正文796个唯一范围，796处置exactly1，
无缺失/额外/重复；363事项、87关系均与原模型数量一致，832条事项/关系引文可在发送内容定位。
事项类型为152 obligation、114 submission_requirement、80 consequence_rule、17 scoring_rule。
处置460 items、221 non_item、115 unresolved；未决中70由模型明确声明、45由宿主恢复缺失处置。
另1处non_item与已有事项证据矛盾，保留事项并记问题；46个extraction_disposition_unresolved
不代表丢弃46个事项。202问题=86模型+116宿主，3条诊断均为evidence_quote_restored。
抽取输出102757，比4包预留多56.8%，前三包超预留；容量参数尚未调整。
独立原文review计划16包，累计输入估算344108，最大32656，0超限；5/16接受、1个在途，
无新增失败，累计已报告214885输入/106833输出/321718总Token，3次中断用量仍未知。
locate/associate/joint尚未规划，不能把review16包当成全部核验调用量或最终语义验收。

03:43 UTC：原文review16/16、locate37/37均首次接受，无新增失败。
review真实输入336076、输出11674；review产生67个问题，与抽取合并为269个唯一问题。
locate计划1 discovery+36 reference；discovery发送全部363事项，输入估算86431≤94208，
本文件没有启用两两分块组合，65703事项对由同一发现请求覆盖。
补读累计输入估算709692，单包17562–24361；259问题进入补读，10问题进入本地关联，
依赖组保持完整，36批中12/9问题的两批未为了8条偏好拆开。
已生成associate104次，累计输入估算1541123、单包最大51384、0超限，第1次在途。
joint尚未规划；正常总调用量目前为2 OCR+4 extract+16 review+37 locate+104 associate+J，
另已有3次中断重发，J未知，不能记作0。累计已报告1206758输入、146312输出、1353070总Token。
运行仍同一command/epoch，未自动提交人工确认或宣称analysis-ready。

04:12 UTC：associate104/104全部首次接受；review16/16、locate37/37保持完整，
核验阶段尚无失败或纠正发送。joint已生成22次，首个在途；正常整文件计划调用量现为
2+4+16+37+104+22=185，另有3次早期中断重发，若余下请求均首次成功则实际188次。
累计已报告2695673输入、215572输出、2911245总Token；3次中断用量仍未知。
关联计划来源为locate101候选+7本地候选=108；5个共享issue依赖候选合为1组，故104请求。
未按共享range/item串为巨包，候选无遗漏；最大192范围/102事项/51384输入估算，0超限。
独立来源在多个候选间重复读取仍是主要累计开销，未因本轮监控热改分组或冻结配置。
locate新增104问题，连同前269合为373，保留未决、无递归重启定位；最终解决数等待joint。

本次最初预检的BUG-030已确认：`api.kunkun-gpt.com:8317`服务端叶证书于
2026-10-21 02:08:32 UTC过期，02:57 UTC新旧容器均在TLS握手报verify_code=10；
公开证书链同样在depth=0报过期，非新版代码回归。后续配置调整及恢复情况如上。
同一106页原件SHA已核对；旧运行、CAS和全部付费回执保留在原目录。
新部署覆盖、预检、指纹及`tls-diagnosis.log`位于
`/opt/workspace/kb-tender-evidence/2026-10-21-full-run/`，没有密钥落盘。
本次通过该目录compose.py及既有
`/opt/workspace/kb-tender-evidence/2026-10-17-full-run/input/manifest.json`
提交独立v6运行；不要把新协议热套旧v4运行，不自动提交人工确认。

工作单元的普通重发及合同纠正统一
首次加三次，以现有请求审计的command_id/logical_request_digest/sent_at持久限额；
按用户最新要求，已补齐所有失败统一重试，包括uncertain、重复拒收及缓存不可验证。
未知付费重发可能重复计费，原回执/状态/未知用量保留，活动sent仍禁止并发重发。
外层基础设施预算独立，完整JSON语法失败提供有界脱敏反馈。反馈首次发送即采用CAS
规范字段顺序，重启继续命中成功缓存；OCR结构校验也在接受前进入同一恢复入口，
有效partial/unresolved仍保留业务问题。相同合同错误继续按真实rejection_count纠正。
最新完整单元1002项通过（171.29秒），统一策略110项、OCR结构4项及异常分类3项包含其中，
不重复累加。此前999项为分类修复前基线（161.03秒）。
完整workflow86项通过（111.34秒），报告为
`test-results/recovery-contract/workflow-unified-final.xml`，包含隔离PG定向49项及增强工作单元16项。
完整acceptance新合同52项通过（419.54秒），报告为
`test-results/acceptance-final/all-failures-final.xml`；最终单元报告为`test-results/unit-complete-final.xml`。
acceptance进程在耗尽异常基类最后调整前启动；该调整的旧领域传播由完整unit及随后完整
workflow验证，未因这处分类修复重跑无关的acceptance用例。
实际OpenAIModel+模拟SSE+隔离PG已验证中断后第二次成功、落盘失败重启复用成功缓存并
在夹具确认后analysis-ready，以及连续四次中断耗尽，5项定向通过。均非真实模型调用。
耗尽异常已改为执行类RuntimeError，旧extract/verify/plan不再吞为领域问题；三项失败
回归及领域/恢复/冻结/队列306项关联回归通过，包含在上述最终整套unit中。
本轮测试环境曾缺/docs挂载，补齐后原985项基线通过；acceptance基线49通过/1项因
导入时Redis地址缺失失败，补齐容器级TENDER_REDIS_URL后该项单独通过，原失败报告保留。
上述离线测试不含模型付费调用；新真实运行已到人工确认入口，终态及待裁决边界见本文顶部。
真实失败SSE按整块、7/17/8191字节分块禁网回放一致，均报offset1939/column1940，
新版含rejection_count的反馈243字符（此前无计数为223）；未接受修补前缀或改写原回执。证据：
`/opt/workspace/kb-tender-evidence/2026-10-21-retry-fix/test-results/`。
新版回放报告为`real-json-replay-unified.json`。所有修改Python的Ruff及diff检查通过。
测试执行容器已退出，本轮专用`kb_tender_fix_pg`、`kb_tender_fix_redis`及`kb-tender-fix-test`
网络已清理；私有报告、真实回执及镜像保留。v6镜像及运行服务已更新，新真实任务已提交。

最近查询2026-10-21 01:45 UTC：run `979ab808-1db3-411e-932c-73ca4a10567c` 已于
2026-10-20 14:27 UTC结束为failed，无在途或uncertain请求。抽取4/4、原文review9/9、
locate25/25接受，associate67/103接受，joint尚未规划；人工确认和analysis-ready未发生。
301事项、45关系、796处置及所有成功回执保留。累计已报告prompt1586409、completion229065、
total1815474 Token，另有一份HTTP500的用量未知。

停止原因：抽取两次invalid_json_response之后，关联`associate-25829aa83789`收到HTTP500，
重试后该单元成功；随后`associate-e6fc49149a18`收到完整DONE/stop但JSON无效，
16914输入/2679输出Token。命令execution_count=4、infrastructure_failures=4，
首次加3次的跨阶段共享失败预算耗尽，回执原因为infrastructure_retries_exhausted。
原始SSE诊断进一步确认：最后两个content事件逐字相同，均为`]}`；完整JSON对象后
多出第二组闭合符，在第1940列报Extra data。独立重建与四种分块回放一致，宿主拼接
未制造错误；上游模型生成还是中转重复发送仍待查。该请求输入估算17120/预算94208，
输出2679/预留32768，无截断或预算不足证据。最后工作单元只失败一次便耗尽command
剩余额度；JSON解析错误也未进入contract_feedback。这两处是上述v6修复针对的恢复缺口。
诊断报告：`/opt/workspace/kb-tender-evidence/2026-10-20-recovery/full-run/live/associate-848d-diagnosis-20261021T015235Z.json`。
本轮完成原回包回放、源码修复及隔离回归，零模型调用，未接受或提交修补后的前缀。
尚未提交新的retry、校正或部署；旧运行冻结于v4，v5镜像仅用于挂载源码测试。
v6不能热套旧run，需以独立新版运行验证；若正式恢复原v4，沿用原合同并复用成功回执，
175条处置的旧版错误出口仍需按下文既定方式处理。

### 前次执行中快照

13:15 UTC：正式运行`979ab808-1db3-411e-932c-73ca4a10567c`仍在执行，原件SHA与下述106页文件
一致；来源/领域修订均为1，request_key=`real-v2-full-20261020-001`。登记、冻结和2次OCR已完成，
抽取4/4包接受：301事项（义务132/提交100/后果53/评分16）、45关系、796处置、214问题。
处置items337/non_item284/unresolved175；完整JSON接受不代表语义通过。失败包前两次返回
无效JSON，第三次正式执行成功，另外3包未重发；execution_count=3、infrastructure_failures=2。
已报告prompt207152、completion116910、total324062 Token（含两次失败），在途用量未知。
独立review计划9包、累计输入218731 o200k Token、最大32726、0超限；2/9接受，第3包在途。
discovery固定部分可完整容纳301事项及引文：1包、67709输入Token、0超限，无需两两组合；
reference、associate、joint依赖真实review回包，调用量仍未知。人工处理及analysis-ready未发生。

本次真实回包另暴露BUG-028处置恢复出口错误：175条宿主恢复问题被错误归为
correct硬阻断。最小修复已完成；保留unresolved及有效事项，后续独立决议/人工
处理，真实来源和执行缺口仍硬阻断。当前v4运行继续使用冻结镜像，不热改回执或切换协议。
若结束后因此blocked，可在原v4通过正式correct把缺省处置明确保留unresolved后重新核验；
不重新OCR/抽取，同摘要成功请求可复用，但变更的核验请求仍付费。此路径不能冒充新code实测。

源码normalization_version=5：96项定向及949项完整单元通过，Ruff/diff通过；完整单元首轮
940通过/9项缺语料文件，补齐一次性容器的只读docs挂载后949项通过，前后报告均保留。
真实PG上`test_analysis.py`整文件12项通过（合成模型），新增用例覆盖独立无事项正文的
处置漏报、完整review、human出口、缺处理拒绝确认及夹具处理后analysis-ready且零新增调用。
首轮11通过/1项失败是DOCX同章合并导致夹具未形成独立范围，补充分章后整文件通过。
本轮未重新跑v5全部workflow/acceptance，不与下述v4的118项合并宣称全套通过。
四份真实已付费回包禁网回放通过：301事项/45关系/796处置逐字段不变，214问题全部保留，
仅175条恢复问题code及宿主问题身份变化，完整台账中均不再protected；真实来源/预算仍hard。
回放0模型调用、0生产及旧产物写入，不当作v5真实调用或语义召回验收。
新候选`knowledgebrain-docreader:20261020-disposition`，镜像
`sha256:a844d96a75abad7f3aa8014f0eeb1ca21c0a19dd755bfa1ae09e0f7eef8d5399`；
53份源码/Prompt指纹与工作区一致、断网tokenizer通过，尚未替换在途v4容器。

BUG-028的可信问题来源修复已部署，normalization_version=4。单元941项通过，隔离PG/Redis的
workflow/acceptance共118个独立用例通过（117项首轮通过、1项修正测试Redis启动环境后通过），
Ruff及diff检查通过。三个Python角色统一镜像
`sha256:85d7d16ed2da9e1841ebdef4d76f7b72077200e268e1abc12cd85d138eb1b3a5`，
部署时53份运行源码/Prompt指纹一致，tokenizer断网检查通过；本轮v5源码修复尚未热替换。
`kb_python_tender_worker`、`kb_python_tender_dispatcher`运行，init正常退出0；未新增宿主端口。
复用`knowledgebrain_default`上的现有PG/Redis；仓库PG账号与共享栈不一致，本次执行在内存中
读取现有API的有效DATABASE_URL，未改共享账号或用户`.env`，私有覆盖文件不保存凭据。
窗口131072、并发2、Chat/VLM沿用已配置模型；worker容器`/models`探测200。
证据、部署覆盖、指纹和只读`poll.py`：
`/opt/workspace/kb-tender-evidence/2026-10-20-recovery/full-run/`；
测试证据在其上级`test-results/`。自动执行到确认前，保留全部付费回执，人工确认不自动提交。

### 恢复现场核对

恢复前现场已无Python招标容器，共享PostgreSQL也没有`tender_*`运行回执表，不能续跑旧run。
`2026-10-17-full-run/artifacts`仍保留：2次OCR成功、1个抽取包接受（67事项/187处置/14问题），
另有3份HTTP401回执；没有完整抽取台账和verify产物。旧运行不能视为仍在执行。
预检确认仓库`.env`与现有API容器Chat Key一致，供应商`/models`只读探测返回200，当时尚未发送生成请求。
BUG-028来源分级遗漏的失败基线为4失败/4通过，修复后状态见上文；隔离证据保存于
`/opt/workspace/kb-tender-evidence/2026-10-20-recovery/`。

## 上次提交状态（2026-10-17，已停止）

真实原件 `BiddingFile.pdf` SHA `4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`。
运行 `bdcf6dac-73b2-4ef5-bb3a-a34019df82f4` 当时已提交：106页已解析，分包计划已写出，模型调用已返回。
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

## 2026-10-28 UI交互收尾（独立验证，不修改真实全流程任务）

- 排队状态统一为pending/published/running，保留取消及同键未知请求确认，禁止另建重复任务。
- 首稿明确“正文已生成，查看Word”；大纲未保存调整保留，首次查看前不隐藏初始化Office；
  后台新稿继续由用户显式选择，不替换当前Word。
- Word附近提供保存稿内容复核和生成稿修订入口，业务版本名优先；保存新版本仍保留旧版
  报告与理由，入口明确要求进入后确认复核版本，不自动切换固定依据。
- 下载明确只包含已确认保存的版本；dirty/syncing/saving/unknown/pending均显示边界。
- 分析/大纲/正文/保存稿复核显示真实阶段及回执，当前来源/领域修订隔离；不估算百分比，
  不把收到模型返回当已接受单元，同逻辑请求重试按最新状态统计。新增模型调用0，新增DB
  查询及artifact读取0，不改模型协议/Prompt/预算。
- 最终前端build/lint、67项单测、107项浏览器回归通过，覆盖1440/390px、300条事项、
  80个长章节及合法书签实际定位；两类复核表单不覆盖Word工具栏。真实Office截图补查后
  编辑区保底420px，工作区允许纵向滚动；核对栏抽屉包含自身关闭入口，桌面缩窄后
  可实际关闭并点击下载。旧视口检查改为先确认项目操作可见，再滚动显示完整编辑区，
  保留1px浏览器舍入容差；未以隐藏编辑器或缩小正文换取测试通过。
- 后端单轮209个唯一用例全部通过，无失败、错误或跳过，相关25文件Ruff通过；独立后端
  回归数据库已删除。当前未部署、未提交或推送，也未运行付费模型或读取真实招标文件。
- UI证据：`/opt/workspace/kb-tender-evidence/2026-10-28-ui-interaction-fixes/`，
  `frontend-accepted.log`及`frontend-accepted/`含最终浏览器结果和桌面/窄屏截图；失败回归
  分别为`red-download.log`、`red-guidance.log`、`red-review-target.log`、`red-office-layout.log`。
- 后端证据沿本轮代理原目录保存：
  `/opt/workspace/kb-tender-evidence/2026-10-28-complete-web-fixes/backend-final-all.xml`、
  `backend-final-summary.json`、`backend-ruff-final.log`及`backend-final-cleanup.json`。
- 真实Office8.3.3独立验收通过（1项，18.4s）：实际载入/目录定位→输入唯一文字→未保存
  下载仍为旧版→显式保存/真实回调发布新版→实际DOCX含新文字且摘要变化→原版本不变→
  再次定位；390px正常关闭核对栏及目录后实际点击下载，所得字节与新版完全一致。
  宿主另行核对4份DOCX的摘要、实际XML文字及新旧版本不变性；数据库关联回执为saved，
  pending_save=null，两类模型请求均0。Office首次“知道了”介绍按真实路径关闭，无force。
- Office证据在UI目录`office-live/`：`playwright-layout-final.log`、`browser-layout-final/`、
  `acceptance.json`、`save-receipt-evidence.json`及`cleanup-evidence.json`。仅使用新建合成
  项目和新凭据；4个容器、8个匿名卷、独占网络/API已精确清理，32850～32854和4175释放。
  不影响另一个真实全流程任务的59463/59873、数据库或证据。本轮真实Office通过不代表
  真实招标/企业资料/付费模型的联合语义验收通过；该任务仍由原任务独立维护。
- 本轮最终汇总为UI证据目录`verification-final.json`，包含测试数量、Office/清理回执及
  已验证源码摘要；4184预览端口也已确认释放。没有待清理的本轮服务或待确认保存操作。


## 2026-10-28 UI异常边界修复验收（独立复核）

- 多入口读取按最新请求序号接收，在提交开始时失效旧读取，成功与错误都受围栏保护；
  同一分析/恢复身份的最新状态控制新提交，保留本地意见、理由和已冻结的未知请求。
- 已结束项目拒绝新编辑、切换当前稿及新可执行任务。Rust结束项目与Word编辑/保存按
  项目行串行，活跃编辑或待保存时提示先保存关闭；Python最终提交先锁项目，再以同
  连接写入run/项目映射、job或分析command。既有同key同body回执、来源只读确认、
  读取、下载、关闭和取消保留；分析二次准入发现结束时在旧版本业务校验前精确回放。
  不可变artifact/source_only准备资料仍可保留，不宣称项目结束后所有准备资料零写入。
- Word打开未知结果按稳定用户身份和本标签页保存原body、语言、模式与key，刷新后
  显式确认原请求；不关闭其他标签页会话。关闭/切模式先锁输入、失焦并排空已排队
  状态事件，回执未知时保持锁定，重试同editor_key。迟到修改保留iframe，通过原生
  “文件→下载为→DOCX”导出恢复稿后显式关闭，签名配置允许download，不虚报平台已保存。
  卸载不复活旧实例，预检期间卸载释放本组件清洁会话，已发送close不重复发送。
- 恢复任务pending/published覆盖旧execution.failed显示，保留同执行身份和已接受回执；
  恢复排队期间不放开重复生成按钮，既有5秒轮询继续刷新。
- 最终前端80项单测、130项完整浏览器回归、build及完整lint通过。新增19项状态回归中
  18项在旧dist失败，保存稿读取排序一项旧dist也通过；4项Word浏览器边界另行覆盖。
  预检卸载RED是明确标注的修复前条件隔离回放，完整GREEN运行当前真实代码。
- 最终Rust API单轮34/34通过，显式执行全部真实PostgreSQL/认证gRPC夹具用例，0忽略；
  含结束后上传回执、保存稿复核回执、Word生命周期和保存原子性。Python最终单轮
  213/213通过，317.457s，0失败/错误/跳过；XML身份核对原147全部覆盖、新20项、
  原生Store46项。Ruff/AST 7文件通过；旧209项3个测试替身失败的轮次不是最终结果。
- 真实Office8.3.3最终冻结源码复验1/1通过（34.0s，0跳过）：合法书签实际定位→输入→
  未保存下载仍为旧版→显式保存/真实回调发布→下载实际DOCX含新文字→历史版本不变→
  390px实际下载→返回阅读释放后台key→再次编辑→正常关闭、iframe销毁且session清空。
  独立核对4份DOCX摘要/XML标记和3个历史/当前版本HTTP下载；两次save均saved，
  pending/session均null，未关闭session0，模型回执0。首轮证据保留，最终只认冻结后复验。
- 本轮未部署、未提交或推送，新增模型调用0；全部使用新建隔离数据及合成Word。
  真实招标/企业资料/付费模型联合语义验收由原独立任务维护，本轮不替代其业务验收。
- 本轮证据根：`/opt/workspace/kb-tender-evidence/2026-10-28-ui-edge-fixes/`。
  总回执`verification-final.json`含数量、边界及源码摘要；前端`frontend-final.log`、
  `frontend-unit-final.log`，Rust`rust-api-final.log`，Python`python-project-end-complete.xml`
  及`python-project-end-final-summary.json`，Office`office-live-new/verification-final.json`
  与`office-live-new/logs/office-live-final.log`。失败证据与首轮结果保留，不以旧进程或局部补跑冒称最终。
- 清理完成：本轮6容器、10匿名卷、1独立网络及API/预览已精确释放；32850～32854、
  32793、历次bridge端口及4175/4184均无监听。回执`backend-cleanup-final.json`、
  `office-live-new/cleanup-final.json`；未触碰并行真实任务的进程、数据库或证据。
