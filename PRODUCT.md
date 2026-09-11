# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

delegated: Vite + TypeScript SPA in `web/`, served by the existing `api` process (`KNOWLEDGEBRAIN_WEB_ROOT`). No extra SPA container; ONLYOFFICE service topology remains to be validated. Chosen after the user asked to ship the full UI immediately; they did not name a framework.

编制目标为 **ONLYOFFICE Docs 直接编辑真实 DOCX**，DOCX 是正式正文唯一来源。主链已部分实现并完成隔离验证，真实 Agent 整稿及同版本 PDF 尚未通过完整验收；不将编辑后的 Word 回转为 Markdown/ContentBlock 再重建整稿。现有 SPA 交付方式不代表文档服务已部署。

## Users

公司内部投标人员（乙方）。LDAP 登录后处理招标文件：上传、按招标要求生成完整模板、在 ONLYOFFICE 中编辑标题、结构和正文、从知识库填充、导出 DOCX/PDF。办公桌、日光灯，白天看屏幕。

## Product Purpose

知识库回答「我们能卖什么、有哪些证」。投标台回答「这一标要求什么、大纲怎么长、正文怎么写、缺什么、怎样导出一份可交的稿」。成功 = 登录用户能走完 建项 → 文件级解析/复用 → 规划结构与 DOCX 初稿 → ONLYOFFICE 编辑/候选确认 → 保存 → 同一完整稿的 DOCX/PDF 与独立检查报告。改完再导出是新文件，不是改旧文件。

## Positioning

用户拥有最终编辑与导出决定权。系统生成完整模板 / 内容候选、提示风险、保存可追踪证据。知识检索用来给「填充」提出证据和图片候选，不做成必须勾选产品才能往下走的向导。正式价格由人确认，保留结构化 `QuoteSnapshot`；快照发布不代表实际 DOCX 已更新，采用后须入稿、保存并核对。已有事实不得虚构，技术方案和商务安排可作为拟议草稿待确认，不要求每句拟议方案都有证件。编制过程没有业务锁；Assessment 只提示。

## Operating Context

内网。同一 Compose：`api` / `worker` / `docreader`。过程中可反复生成 Word / PDF。业务提示不阻止导出。只有权限、并发、文件完整性、保存或转换等技术失败才停止对应操作，不能回退旧文件伪称成功。ONLYOFFICE 服务与部署/嵌入许可待核实，基础编辑、回调保存、后台初稿和 DOCX→PDF 不依赖外部 Automation 附加 API；AI 可优先验证官方插件，只有选择外部 Connector 控制打开中文档才涉及 Developer 加额外 Automation 授权。无采购决定，不承诺插件免费商用或与 Connector 等价。完整稿可打印线下签章；不做 CA 电子签章或电子投标平台自动递交。

## Capabilities and Constraints

- 关注册。LDAP 或本地口令。登录可进入投标和读写知识库；投标项目与文件仍校验既有 owner/用途权限。
- 招标文件不进 `documents`。公司资料走 Document 管线，看 `index_ready`。
- Web 编制面：顶栏切换产品，左栏列项目并把 **文件 / 编制 / 导出** 挂在当前标下；编制步主列是 ONLYOFFICE，检查器可选。解析是文件状态；freeze 是文件页动作，不是第四导航，也不是「确认后才能改」的闸门。
- 结构规划与章节导航保留，招标文件指定目录/表格优先；标题、顺序、层级和表格由 DOCX 承载，外部业务索引不能强制回写覆盖人工修改。人员独立数据不等于强制增加专章。
- 文件未变且有可用结果则复用；只处理新增、变化或无结果文件，汇总当前完整集合。招标集合变化后新轮整稿，不局部修补或自动合并旧稿；仅改正文不重跑源解析。
- AI 只出候选；用户接受后校验版本/目标和未保存编辑，定点入稿并确认保存，永不覆盖并发人工编辑。
- 未覆盖要求、stale、缺件只提示，不禁止编辑或导出。出件等待正确回调持久化后冻结 DOCX，PDF 由同一文件经 ONLYOFFICE 转换，独立报告绑定该版本。
- 金额仅 CNY，价格由人确认；最高限价必须明确含税 / 未税口径。报价精编、台账精编、文档设置面板不是黄金路径必经步。
- 业务时区固定 `Asia/Shanghai`。
- 不做包件、Org、多租户、成本 / 评标引擎、假截图、CA 电子签章或自动递交。
- 禁止把招标文件图片 / 附件自动当作投标方证据。
- 报价表、价格内容与应提供附件全部编入同一完整稿；不另出报价文件或附件包。独立提交要求保留并提示用户自行拆分、核对目录/页码/证明引用/格式。
- 保留 Workspace、来源/文件版本、CAS/权限、知识检索证据、对象存储、队列与历史输出；文档清理不授权清库或部署。新链验收后撤旧正式正文/出件入口，不双主写。
- 证明引用先锚定、真实排版、回填保存再检查；不能稳定则清除/降级不确定数字并验证实际输出，不承诺桌面 Word 默认同页。

## Brand Commitments

- 名称：KnowledgeBrain / 投标。
- 视觉钉死：TokHub 高质感白（毛玻璃顶栏、浅侧栏 `#FAFAFB`、品牌蓝 `#2563EB`、卡片 14px + 轻阴影）。中文 UI 字体 PingFang SC / Noto Sans SC，禁止运行时 Google Fonts。规范见 `DESIGN.md`。
- 组件钉死：Tailwind CSS + shadcn/ui 做壳，图标 Tabler。正文编辑由 ONLYOFFICE 承担；外壳字体/颜色规范不强加到 DOCX 招标版式或编辑器内部界面。
- 禁止把招标文件或人补图写回产品库。

## Evidence on Hand

- [业务 PRD](docs/bidding/prd.md)、[ONLYOFFICE 技术契约](docs/bidding/onlyoffice.md)、[领域边界](docs/bidding/authoring.md)、[编辑与出件顺序](plans/bidding/onlyoffice-integration.md)、[Agent 实施方案](plans/bidding/agent-runtime-rig.md)
- `docs/knowledge-base/domain.md`（知识库领域与证据检索端口）
- 当前 `web/src/bid/authoring/DocumentCanvas.tsx` 仍使用 Tiptap；`crates/bidding/src/render_v2.rs` 仍自研双格式渲染，不是 ONLYOFFICE 已接入证据
- `1.png`：Plannotator 三栏编辑器仅作结构参照（左树 / 中文稿 / 右检查器）；外壳外观以 `DESIGN.md` 为准，不再仿 iCloud，也不走 Cloudflare 锌灰密表
- 无客户照片、无真实标书样张可当品牌图

## Product Principles

1. 引导式工作台，但不是锁死向导。进标先看本标文件。用户主路径是文件 → 编制 → 导出，步骤在左栏当前标下，不在顶栏大 Stepper。解析、冻结、检查都不是另一步。
2. 一份真实 DOCX。保留章导航和证据提示；文档内编辑结果优先于外部结构索引，失效定位提示核对。
3. 缺了就补，不锁死。生成中、有候选、有检查提示时，用户仍可改树、改字、导出当前稿。
4. 系统建议，人改写。大纲候选和内容候选都是 overlay；人改的留下，过期候选不得覆盖。
5. DOCX/PDF 是同一完整稿的两种格式，不按格式分业务等级；正式稿不写水印、风险声明或内部知识来源，检查报告独立。技术损坏 fail-closed。
6. 知识库不做 Workspace 成员门闩；投标保持既有项目 owner 与文件用途校验，不能将登录等同任意投标访问权。

## Accessibility & Inclusion

中文界面。我方 Web UI 文字对比 ≥ 4.5:1，DOCX 正文格式服从招标要求。键盘可走完上传、大纲跳转 / 调序（拖拽的按钮备份）、正文编辑、导出。
