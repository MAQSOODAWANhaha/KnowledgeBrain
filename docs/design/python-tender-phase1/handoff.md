# Python 招标第一期交接

## 当前收尾保存点（2026-10-14，未完成修复验收）

用户在本轮实施过程中明确要求收尾提交，故停止扩展。**以下是WIP checkpoint，不是两项review
已解决，也不是全流程或真实模型验收通过。** PR继续Draft；真实模型0次，不合并、不部署、不切流。

- 文档先行合同：`ac77cf0`，已推送。
- 当前运行代码保存点：`1480efc5c5ff173eeb17645889c00a689de4f61a`，已推送同一分支
  `feat/python-tender-phase1`。本交接文件所在的后续提交仅同步状态/字段说明。
- 起点远端：`5aa6b84196837f19404dc3e699a9462aee2071df`；历史运行代码`51d7ec3`。
- 本轮Rust/Cargo/默认链路无修改；没有真实模型网络请求。额度不可读，已采用分阶段保存。

### 已保存的实现及有限证据

1. 独立`ModelBudgetError`与语义错误分开；预算拒绝在发送前发生，不创建付费请求回执。
2. 共享`RequestLimits`区分普通输入131072 token、显式纠错最多262144 token，以及provider
   `context_tokens`总窗口。已核查旧C=131072确实是总窗口；未擅自增加。常规verify、plan或
   新领域修订均不自动启用纠错。双倍仍受总窗口/输出/安全预留约束，不新增wire max_tokens。
   真实provider总窗口能力尚无新证据；当前UTF8算法仍为保守估算，非provider实测token。
3. `tender/evidence/downstream.py`复用现有结构分包的实测前缀选择，新增具体verify/plan
   阶段计划。验证按完整来源、要求及处置探测，保留条件/支持证据；跨范围义务连接的范围
   作为完整单元。计划按完整有效义务/约束探测。不可分超限单独阻断。计划只保存selector
   和要求/处置引用，原文不复制，checkpoint仍只持六个固定阶段引用。
4. `workflow/nodes.py`已接入阶段计划与plan_digest子包回执；验证汇总允许包数改变并核对
   来源/处置/要求覆盖；响应计划汇总核对每项适用要求。旧语义/预算测试已相应更新。
5. **实际workflow回归通过**：一个extract包可容纳，合法抽取输出增加后完整verify超限；
   自动重分、完整逐范围处置/要求审核、人工确认后plan重分，8项计划完成；终态重放无新增调用。
   这是确定性模型+真实PostgreSQL测试，不是付费模型语义结果。

最终有限检查（Python **3.11.16**）：

```text
tests/unit/test_request_budget.py
tests/unit/test_tender_llm.py
tests/unit/test_shared_schema.py
tests/unit/test_structural_packing.py
tests/acceptance/test_phase1.py
=> 146 passed / 27.17秒，无skip
Ruff（本轮相关模块/测试）与git diff --check：通过
```

初次检查有3处旧测试仍期待ModelSemanticError，已改为独立预算错误；新增fixture首版只有一
个原生章节单元，未触发目标超限，改成8个真实章节后达成回归。以上最终146项包含这些修正。
**本轮未跑完整Python、全workflow/恢复套件、wheel、镜像、实际SDK wire复验；远端CI未等候。**
历史684/15skip与454专项CI不能用于宣称当前1480efc已全套通过。

### 两项未关闭review与下一步

1. [评论6161148698](https://github.com/MAQSOODAWANhaha/KnowledgeBrain/pull/24#issuecomment-6161148698)：
   完整verify预算死路已有首版代码和正向workflow回归，但**仍未完成验收，不关闭review**。
   继续审查并补测：不可分阻断不付费；跨范围/跨章节义务、条件/表头、模板、修订失效；
   部分子包成功后失败及重启回执复用。当前后阶段按原extract包分别规划，尤其需审查跨
   extract包的cross_references依赖是否完整，不能把包内闭包当作跨包完整性保证。
   然后跑相关/full Python及真实原件完整输出输入的离线wire复验，再决定是否已可验收。
2. **部分流DecodingError重付费风险，本轮未修**：`llm/client.py`捕获HTTP DecodingError
   后记`failed`；`integrations/store.py::_matching_model_requests`只拦sent/uncertain/succeeded。
   部分流可能已付费，若业务产物提交失败后恢复，同digest可能再次发送。需单独先修合同，
   再补部分流解码失败+业务提交失败+恢复的真实store回归；不能将其视作明确未执行拒绝。

13项真实作用域审议、人工标注和真实模型语义验收仍未完成。所有真实模型调用继续暂停；
最终实际wire须审查后才可考虑付费。provider容量未确认不做付费探测。

### 新执行环境和私有原件

环境可用，无待审批操作或不确定推送；新环境最初为干净main，已显式fetch并切换目标分支。
仓库AGENTS已读；`/workspace/.agents`为空，仓库中未找到相关`.agents/skills`，不假装已读。

- 通过当前Library `prepare_materialize`返回的授权下载方式重新获取原件，保存在
  `/workspace/private-tender-acceptance-current/BiddingFile(1).pdf`（仅本地私有；不是旧路径）。
  已核对755602字节和SHA256 `4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`。
  **本轮仅下载/校验，未解析或复跑真实PDF预算/wire**；旧图片审议文件与私有预检目录未迁移。
- Python：`services/docreader/.venv311/bin/python`；uv缓存/安装目录在`/workspace/.cache/uv/`。
- 本轮启动隔离测试容器`kb-tender-budget-postgres`、`kb-tender-budget-redis`，端口15432/16379，
  复用下文测试DSN；测试使用临时schema，不影响生产库。容器保留供接手复现。

## 以下为5aa6b84历史交接，保留原义（不代表当前WIP验收结果）

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
