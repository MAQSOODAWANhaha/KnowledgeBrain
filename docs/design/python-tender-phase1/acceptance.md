# 验收标准与证据分层

状态：独立实现与确定性验收已完成；真实 LLM 语义验收未完成。以下区分实际结果和验收标准。

## 已执行：骨架（2026-10-14）

Python 3.11.16，PostgreSQL `127.0.0.1:15432` 隔离数据库 `tender_phase1_test`，
Redis `127.0.0.1:16379/1`；不使用原业务表。

- `.venv311/bin/python -m pytest tests/acceptance/test_skeleton.py -q -x`：**4 passed in 5.89s**。
  使用真实 Dramatiq 子进程和 Redis：SIGKILL 后重新投递恢复同 thread；人工暂停后强杀并重启确认；
  并发重复命令、取消心跳、Redis 投递失败、业务回执已提交而图无 checkpoint 时重放。
- `.venv311/bin/python -m pytest tests/unit/test_artifacts.py tests/workflow/test_store.py -q`：
  **12 passed in 1.11s**（持久模块测试使用同真实 PG 上的独立临时 schema）。
  含 claim token、发布回执与执行回执区分、租约围栏、摘要确认/改版、uncertain 与基础设施失败上限。

这些测试的 SkeletonNodes 仅证明执行可靠性，不含招标语义，不能计为阶段 1–6 领域完成。
依赖锁定：LangGraph 1.2.15、Postgres checkpointer 3.1.3、Dramatiq 1.18.0、
psycopg 3.3.6、OpenAI 2.54.0、httpx 0.28.1。

## A. 真实基础设施骨架（先于领域实现）

- Python 3.11 + 锁定依赖可导入；真实 PostgreSQL 初始化官方 checkpoint 与独立业务表。
- 真实 Redis + Dramatiq AsyncIO 启动/恢复；同 run 并发重复消息只有一个有效执行者。
- worker 强杀后，租约/领取过期、dispatcher 补投，恢复同一个持久 thread。
- 人工 interrupt 已保存后 actor 结束释放线程；重启进程后 confirm 新命令恢复。
- Redis enqueue 失败不丢命令；published 无执行回执可补投；旧 claim token 不可确认发布。
- 旧 epoch/租约不可提交；取消长任务时无需排在队列尾；checkpoint 写入也禁止旧执行者。
- 业务提交成功后注入 checkpoint 失败，重跑复用同产物回执，不重复业务提交。

## B. 确定性模型 + 真实解析器

- 混合 PDF/Word/Excel/图片/附件；重复/缺失/不可读与 unsupported/partial 明确记录。
- Excel 嵌入图和跨工作表计数；章节/段落/表格/合并关系及坐标不丢。
- 扫描件/实际图 OCR；文本 PDF 不逐页 OCR。冻结不可变；重复执行复用。
- 小章节合并/大章节拆包，跨包导语、条件和表头带来源；supporting 不重复计正文覆盖。
- 全空行压缩发送视图；原始网格和部分空字段保留。
- 抽取证据/原文/条件/例外/数字单位/评分否决风险/交叉引用；每范围明确处置。
- 独立全来源查漏、数字反查、非 100 分规则、补遗明确作用域与未决冲突。
- 部分包失败保留独立成功包，失败依赖不放行、全局不假成功。
- 重复/过期确认、运行取消、来源改版、旧确认失效。
- 响应计划逐要求具体落点和材料需求，空标题/空槽位拒绝。
- 各阶段耗时、调用/包数、实际 usage、uncertain 请求、问题与产物可读取。

已执行 B 类验证（2026-10-14，Python 3.11.16）：

- 全套 `pytest tests -q`，显式配置真实 PG/Redis：**352 passed, 15 skipped in 69.38s**。
  原有测试的15个skip因可选数据/依赖条件；新增真实服务测试未跳过。
- 此后新增实际 graph 的 checkpoint 故障注入：**1 passed in 0.56s**，在 register 业务提交后
  让官方 saver 写失败；恢复保留同一 artifact/epoch 回执。
- 新范围 Ruff 检查：`llm tender workflow jobs integrations tests/unit tests/workflow tests/acceptance` 全通过。
- 领域单测覆盖完整范围处置、数字/单位、非100评分、跨包支持来源、明确/竞争/循环补遗、计划空槽拒绝。
- 独立审阅修复并回归：dispatcher DB 暂时断连后恢复；发布失败与执行共享4次基础设施上限；
  编程/配置型发布错误终止；artifact root 初始化失败进入明确 failed 回执。

可复查的**合成实际产物**：
[运行与阶段耗时/调用数](evidence/sample-run.json)、[响应计划](evidence/sample-plan.json)、
[独立验证/要求](evidence/sample-verification.json)、[冻结来源索引](evidence/sample-sources.json)。
run `5bd26329-1208-4d80-8d27-8d2429ce92ee`：4文件/4包/12范围/6要求，零阻断，
OCR 2、抽取4、验证4、计划4次确定性调用；外部请求0，付费usage=null。
CLI status 实读24个产物回执和6项计划，状态为 `response-plan-ready`。
这些文件是本 PR 合成验收证据，不替代或改写 `docs/validation/` 的历史真实验收。

## C. 分发与包装

- 原有 docreader 测试按改动范围运行，新 tests/unit、tests/workflow、tests/acceptance。
- 构建 wheel 并验证新包/提示文件完整；镜像新模块 import smoke，默认仍是 gRPC。
- Python 专用启动说明与可选部署配置完整；现有 Rust 默认运行链路 diff 为空。

已执行 C 类验证：wheel 构建成功并检查新增模块/四份提示；镜像
`knowledgebrain-docreader:python-tender-phase1` 构建成功，image id
`sha256:7a15c8eea5cdf38a0000ad52d28ae7020cdcb214df89bd27a95481e77e5247ec`。
镜像内 Python 3.11.17 import 原 main、新 llm/workflow/jobs/CLI 与四份提示全部通过；
容器 CLI init 已连接真实 PostgreSQL 成功。默认 CMD 仍是
`["uv","run","--no-dev","python","main.py"]`。

构建环境说明：本执行环境代理的 DNS/CA 不在容器内，直接 build 遇到连接和证书错误。
本地验证使用从提交的 Dockerfile 生成的临时副本，仅将 `RUN uv sync` 加入 BuildKit
CA secret mount，并传入受控 build proxy/add-host；未关闭 TLS，证书不写入镜像。
提交的生产 Dockerfile/default CMD 未改；`.dockerignore` 明确保留新提示资源。
另逐文件比较镜像与代码提交 `aba9d5d0ee3004ebb320c2e2674ddfd694eff384`：
40个新增 Python/提示文件 SHA256 全部相同，见 [镜像核对清单](evidence/image-verification.json)。

## D. 真实 LLM 语义验收（单独记录）

先检查环境输入和模型是否存在，只报告 endpoint/model/文件清单，不输出 key。
付费全流程调用须先报告估计请求范围并由父任务确认。真实输出须人工语义核对遗漏、
数字、补遗和计划；mock/CI 通过不等于语义验收。不具备输入/访问则明确剩余 blocker。

真实标注集质量目标：总体适用要求召回 **≥95%**，否决性关键项召回 **100%**。
这是事先确定并人工标注的测试集目标，不是对未知新文件或所有语义的保证。标注记录包括来源
页/段/单元格、原文、适用条件/例外、数值/单位、否决性质、补遗作用域与预期计划落点。
按独立的原子义务匹配：必须同对象、义务、条件/例外和关键数值；措辞改写可接受，同一要求
重复抽取只计一次；合并项只有完整覆盖各原子义务才能分别计命中。分母为经双人复核或人工
裁决的适用 ground truth；争议未决单列，不从分母静默删除。总体/否决项分别报告 TP/FN 与
召回率，逐条列漏检和错误匹配；同时报告错误/无依据抽取与计划缺口。未达到目标不得宣称通过。

D 类当前未完成：父任务已明确提供 Library 原件 BiddingFile(1).pdf（106 页、755602 字节），
本任务已 materialize 并复核 SHA256 `4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc`。
原件只保存在私有验收目录，不提交 GitHub。此前“缺原件”的判断已被此证据取代。
已确认普通单请求窗口 C=131072；已完成真实解析、4包最终发送视图预检；付费范围仍需确认，未发外部请求。
标注集与真实模型输出尚待完成，不把解析成功或 mock/CI 当作 D 类通过。

## 本轮追加评审验收

- [x] 同来源 v1 下产生 r1/r2 要求；同修订重放复用回执；冻结原件保持不变。
- [x] r1 确认失效；r2 重新独立验证和确认；旧确认不能授权 r2 计划。
- [x] 恢复固定模型/提示摘要；漂移拒绝且不发模型请求。
- [x] 实际 FencedPostgresSaver：旧 epoch 暂停，租约到期，新执行者接手，旧 aput/aput_writes 均拒绝，最新 checkpoint 不受污染。
- [x] 原 PDF 原生解析、来源覆盖、分包大小与上下文/表头检查；报告阻断项和预计模型请求范围。

## E. 最终生产集成（本 PR 不完成）

Rust run/outbox/API/鉴权/事务围栏、流量切换、旧实现移除、部署及历史数据迁移均未实施。
不把独立 Python 验收勾选为最终生产集成。

## 保留的领域限制

- 文档解析的 unsupported/partial、嵌入附件未展开、Excel缓存/显示不完整、OCR不可读均保留
  为可追溯阻断项，需要来源修订/补充，不能靠人工确认强制清空。
- 文本意义的任意冲突/遗漏仍依赖独立模型和真实语义验收；数字规则不是通用语义证明。
- 上下文预检用明确标记的保守估算；独立 VLM 容量未知时不挪用 CHAT 容量。

## 最终请求视图/流式统一验收（2026-10-14）

- 完整 `.venv311/bin/pytest tests -q --tb=short`，真实PG/Redis环境：**684 passed, 15 skipped in 142.69s**。
  skip仍为原有可选测试，新服务测试均执行。Ruff同范围通过，`git diff --check`通过。
- 定向phase1/存储增长/共享schema先跑 **46 passed in 48.05s**；全套包含此后追加的严格SourceView用例。
- 新增原始SSE首数据/相邻数据180秒idle、持续1360秒、gzip/心跳/thinking/tool活动、截断、断流、
  usage缺失/正常结束、完成响应回执复用、PG并发send围栏以及uncertain+429兄弟包防自动重付费。
- wheel成功；48个运行模块/提示逐字节核对，旧requirements/schema不存在。新版镜像验收追加在下节，
  上文aba9d5d对应的旧镜像事实仅作历史。
- 真实106页原件重新解析：796范围、4包；正文/网格/图片ref及范围原文前后相同。
  抽取wire字节减少60.6%，独立验证source-only 0超限；完整验证还需真实要求/处置后重新准入。
  详见[逐包请求成本](request-review.md)和[当前结构预检](evidence/real-pdf-structure-preflight.json)。
- 2倍/4倍增长的新事实：[storage-growth.json](evidence/storage-growth.json)；前版保留为
  [storage-growth-before-wire.json](evidence/storage-growth-before-wire.json)。只测确定性实际PG/文件层，
  不将模拟模型响应大小/耗时解释为付费模型性能。
- 全程真实模型调用0次、真实usage未知；13范围审议未决、人工ground truth与真实LLM验收未完成。
  目前无重分/重试假设下为4抽取+4验证+0..4计划，8..12次仅作待确认预估。
- Rust/Cargo diff为空；独立Python已验收，Rust生产集成仍未实施，未切流/合并/部署。

## 当前运行代码的镜像核验

实现提交 `51d7ec322e7ac09cb6c05043ccb07e8ecf03d76c`；镜像
`sha256:8769713667a41d1810e351b47a29f49b074c1a8acf15e51102895bb7010d3032`，镜像内Python 3.11.17。
48个运行模块/提示SHA256逐项与提交源码一致，原main/新schema/graph/jobs/CLI/四提示导入通过；
容器CLI init连接真实PostgreSQL独立临时schema成功并清理。默认CMD仍为
`["uv","run","--no-dev","python","main.py"]`。见[当前镜像证据](evidence/image-verification-review.json)。

本机构建依然只用临时Dockerfile的BuildKit CA secret mount解决受控代理信任，不关闭TLS，
证书不持久保存；提交Dockerfile的默认生产入口不变。本核验不代表已部署或已切流。
