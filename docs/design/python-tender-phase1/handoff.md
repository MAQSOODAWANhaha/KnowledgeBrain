# Python 招标第一期交接

## 当前保存点与暂停边界（2026-10-14）

当前两项代码review已修复，并完成本轮确定性workflow、恢复、全量Python和真实原件离线
SDK wire复验。按用户最新要求，提交本交接后暂停，供父任务独立核对；不启动真实模型或新范围。
[Draft PR #24](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/pull/24)保持Draft，未合并、部署或切流。

- 分支：`feat/python-tender-phase1`；本轮起点fetch核对为`f78462ff956d62893f0909bd897071787f21f5f3`，无遗漏后续远端改动。
- 最终运行代码：`433a46475a528f089a4b6942767405c44d375c34`，已推送。最终交接为此文件所在提交，精确HEAD另记在PR描述及任务结束报告。
- 文档先行：`cc6185e`、`1b2ccf7`；解压不确定发送修复：`e64e1a942f948654c27e72e1f00f036377e6d958`；完整下游依赖与回归：`433a464`。
- main基线仍为`e8b681a0997760ec3f567223fe7eb82b622f5ed7`；Rust源码、Cargo、默认链路无diff。未改生产接入或增加第二状态机。
- 剩余额度没有可读接口，已阶段性push；真实模型调用0次并继续暂停。没有待审批或不确定执行动作。

## 当前方案与合同

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

## 剩余状态与下一次执行顺序

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
