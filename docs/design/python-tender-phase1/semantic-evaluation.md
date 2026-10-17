# 独立语义核验评测准备与裁定

当前[语料](semantic-cases.json)使用v2合同，共32个独立review输入：13组正反配对，
另有2个跨文档样本及4个漏项目标、异常例外及补读路由样本。原文是合成数据，expected仍为agent草案，人工复核、真实模型评测
及semantic_acceptance均为false。合同以[字段字典](field-dictionary.md)为准。

## 输入与导出

从仓库根目录执行（使用项目Python依赖，不读取模型凭证、不联网）：

```bash
PYTHONPATH=services uv run --project services/docreader python \
  services/docreader/tests/acceptance/check_semantic_cases.py \
  --prepare /tmp/tender-verify-offline-v2
```

工具用当前SourceView、AnalysisExtraction、Decision校验输入，检查来源身份、可见引文、
处置引用和预期引用。表格发送cells.text，原跨单元格整行引文拆为实际可见的单元格引文；
不将v1的offset视图、requirements或applicable混入当前请求。跨文档保留独立source_views
及角色；补遗用相同原事项和不同amendment决议，观察retained_rules是否保留维护义务。

只接受v2核验Prompt。目录必须不存在，避免覆盖旧输入或人工记录。输出为：

| 路径 | 内容 |
| --- | --- |
| `model-inputs/v001.json`至`v032.json` | 仅当前v2 review payload；不含case_id、category、expected、provenance |
| `verify.md` | 实际核验Prompt快照 |
| `review/corpus.json` | 本地原始语料及草案标注 |
| `review/manifest.json` | 本地case映射、语料/Prompt/输入SHA、各请求及累计输入预算估算 |
| `review/records.json` | not_run状态，annotation_review/model_observation/host_observation/adjudication均为null |

命令行默认预算为131072总上下文、16384输出预留及4096安全余量。该离线默认不是模型
自动发现；使用现有配置评测时须向prepare_evaluation传入该模型的RequestBudget，并核对
导出Prompt完全一致。估算不是计费Token，也不是已证明输出足够。

模型只接收指定payload和Prompt；预期、文件名、manifest、配对另一半及人工记录不发送。
每例独立请求，不共享对话。不能把SourceView伪装成冻结包来声称完整工作流已经运行。

## 真实执行与记录边界

仓库中的工具仍只做离线检查/导出，没有完整的付费评测执行器。A/B修复使用隔离脚本
调用正式OpenAIModel、Store及call_validated，对部分合成反例和原件请求做了定向真实验证，
保留请求、原响应、用量和拒收记录；它不代表32份语料全部评测或人工标注。
真实106页工作流与组件验证分别记录在[交接](handoff.md)。
付费执行器仍需复用现有请求审计、原响应保存、预算和sent/uncertain围栏，不能直接用无
持久记录的HTTP脚本代替。已有完整JSON的宿主拒收也要单列，不能原样重复读取后算成功。

人工先核对原文及草案预期，冻结标注，再观察模型回答；agent复核不能填写人工完成。
每例记录输入与Prompt摘要、模型/端点、请求和原回包引用、实际usage及未知项、传输状态、
宿主合同结果和人工裁定。失败、uncertain、合同错误及未运行样本均保留在统计分母中。
无完整回包或未知usage不能记为0费用、语义正确或可自动重发。

## 判定口径

- 错误样本：findings必须指出具体目标错误并有来源支持，泛称“需核实”或无关问题不算命中。
- 正确样本：允许等价改写及表头单位继承；无依据质疑记误报。来源或预期存在争议时记录disputed。
- 补遗：分别检查决议接受/拒绝、修改范围及保留规则，不因事项文字相同就认为结果相同。
- 跨文档：同一对象确实冲突与不同标段分别适用须区分，不能按数值不同自动判冲突。
- 路由样本：unresolved表示现有输入不足以判定；人工核对needs_source及continued_issue_ids
  是否有依据。字段合法不证明路由正确，跳过补读不表示问题已解决。
- 语义判断不用正则、关键词、固定issue code或reason字符串相等自动评分。
- 结构合法、引用可回查、reviewed列表完整均不证明语义完整；模型原始表现、宿主处理和
  最终工作流状态分别记录，不能用最终findings猜测原始模型表现。

分别报告错误样本命中/漏检、正确样本误报、宿主误阻断、未裁定与失败数量，并明确分母。
这批小规模Verify诊断集不代表Extract覆盖率、真实标书准确率或五节点已经验收。
评分层级/公式/封顶、否决与不得分区分等更完整业务语料，以及真实文件逐项标注，仍待补充。
