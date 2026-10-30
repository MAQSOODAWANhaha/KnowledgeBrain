# Design System: KnowledgeBrain 投标台

外壳是冷灰仪器盘：日光下长时间核对招标与稿件用的工作台。密、静、发丝线，钢蓝只标当前选中、主按钮和焦点。本文只约束 Web 外壳，不强加到 ONLYOFFICE 或 DOCX 正文。

已废止并不再保留：TokHub / Apple 高质感白、暖纸原型、`web/prototype`、`docs/design/frontend-workbench-preview.html`。稿件保存、锚点、下载和编制行为仍以 [frontend-workbench-design.md](docs/design/frontend-workbench-design.md) 为准。页面几何与交互只以本文为准。

## 1. Visual Theme

冷灰画布上的白面板。Variance 2，Motion 2，Density 8。

- 画布与顶栏、侧栏：`#F4F5F7`，实底，不毛玻璃
- 面板、表、对话框：白，1px 发丝线，面板无阴影
- 强调：钢蓝 `#1E3A5F`，不铺亮蓝底
- 控件圆角 6，面板与对话框 8。状态标签圆角 4，不用大胶囊
- 字标：28×28 钢蓝方块，白字 `KB`，无渐变、无投影

登录独立全屏，无应用顶栏。底为画布色，居中白面板，无径向光。

不抄：深色侧栏、暖纸、霓虹、营销 Hero、Cloudflare 或 Linear 的品牌色。冷灰只取仪器盘的密度和中性色，不换成暗色主题。

## 2. Color Palette

| Token | Hex | 用途 |
| --- | --- | --- |
| Canvas | `#F4F5F7` | 窗口、顶栏、侧栏 |
| Surface | `#FFFFFF` | 面板、表、对话框 |
| Fill | `#F4F4F5` | hover、表头、中性提示条 |
| Text | `#18181B` | 正文 |
| Text 2 | `#3F3F46` | 次级，须 ≥ 4.5:1 |
| Text 3 | `#52525B` | 元数据 |
| Border | `#E4E4E7` | 发丝线 |
| Border strong | `#D4D4D8` | 输入描边 |
| Steel | `#1E3A5F` | 主按钮、选中字、焦点 |
| Steel press | `#16304F` | 主按钮 hover |
| Steel wash | `#EEF2F6` | 选中浅底，不用于整条通知 |
| Go | `#166534` | 完成、可继续 |
| Wait | `#92400E` | 待核对 |
| Stop | `#9F1239` | 失败、阻断 |
| Go wash | `#F3F7F4` | 状态底 |
| Wait wash | `#FAF6F0` | 状态底 |
| Stop wash | `#FAF3F5` | 状态底 |
| Elev | `0 8px 24px rgba(24,24,27,.12)` | 仅对话框、菜单、临时抽屉 |

状态靠圆点加两个字，字色用上表深色。整条色带只留给挡住下一步的错误。待补、生成中、新稿就绪用中性条或 Wait，不用亮蓝横幅。

## 3. Typography

PingFang SC / -apple-system / Noto Sans SC。禁止 runtime Google Fonts。

12 元数据 / 13 界面（导航、按钮、树、表） / 15 阅读正文 / 20 页标题。树行 32px。字重 400 / 600。`tabular-nums`。根字号保持 16px，避免 rem 整体缩放。

## 4. Components

Vite + React 19 + Tailwind v4 + shadcn。图标 Tabler。正文 ONLYOFFICE。

- 壳：grid 顶栏 48、左栏 240、主列 1fr
- 按钮高 32、圆角 6。主按钮 Steel 填实。次按钮白底发丝线。危险只用 Stop 字色
- 输入高 32、圆角 6、白底 + Border strong，焦点 2px Steel
- 表在白面板内，无阴影。行 hover 用 Fill，选中用 Steel wash
- Dropzone 虚线、圆角 8。保留 `upload-drop`
- Dialog 宽 440、圆角 8、遮罩 40%、Elev
- Toast：Sonner 顶中，白底发丝线，4s

## 5. Layout

列表和项目内用同一套壳。顶栏只切换产品，左栏负责当前产品里的位置。

```text
64px 顶栏：字标 | 投标项目 知识资产 | 账号 退出
240 左栏：列表是项目或资料树；项目内是返回列表、项目名、四步
48px 页头：当前页标题 | 这一页的主按钮
──────────────────────────────────────────────────────────────
剩余高度给当前分区。编制正文：240 目录 | 文稿 | 300 核对
底部一行工具：复核、执行详情、直接生成整稿。展开时向上盖住内容
```

- 顶栏 64px，左产品、右账号。不放面包屑，不放四步
- 选中为底边 2px Steel，不是胶囊
- 账号与退出只在顶栏右侧。不用渐变圆
- 左栏 240，列表和项目内都在。窄于 700px 收成抽屉
- 页头 48px。`分析招标文件` 与 `编排大纲` 在页头，章节滚动后仍完整留在视口内。编制时大纲 / 正文切换也在页头
- 文稿区只放编辑器。保存稿复核和生成稿修订放在右侧核对栏，不挡在正文上面
- 已完成的任务进度不单独占行；进行中、失败或回执待确认才显示进度
- 目录、文稿、核对各自滚动，栏头 40px 钉住。收起侧栏只过渡列宽
- 宽屏三栏同时可见。窄于 1200px 时核对栏改为盖在文稿上的抽屉，标题和关闭都在抽屉内，不使用写死的 viewport `top`
- 窄于 700px 时，四步换到顶栏第二行，面包屑让位，工作区可以整页滚动。高屏则正文区填满剩余高度，编辑器底边不超过视口
- 上传用「选择项目文件」或「选择 DOCX」按钮。文件控件留在按钮里且不显示浏览器默认外观。报价宽表只在表格区域内横向滚动
- ONLYOFFICE 内部界面不改。保存、下载留在编辑器自己的工具条上

## 6. Motion & Interaction

140ms ease-out，只过渡颜色、边框和分栏列宽。尊重 `prefers-reduced-motion`：关闭位移和脉冲。

`:focus-visible` 2px Steel、offset 2。

切换分区、选中章节、展开执行详情时，文稿不整页跳动。状态条预留一行高度。新稿提示是操作条下的一条，不是叠在正文上的多条色带。

- 状态摘要是链接：文件数进项目资料，分析进招标解读，编制进标书编制，待补进交付文件
- 大纲 / 正文是分段控件。左右方向键切换并移动焦点，不整页滚动
- 章节目录用上下方向键移动并选中。正文视图里 `[` 收放目录，`]` 收放核对栏；输入框内不抢键
- 底部工具同时只展开一个，向上覆盖。Esc 关闭并把焦点还给标题。从核对栏打开复核时不滚动文稿
- 项目列表的名称是链接，键盘可进入；点整行仍然进入

未确认 DOCX：继续 `guardHashNavigation` / `onBeforeLeave` / `beforeunload`。不要用冻结禁用编制/导出。

testid：`wizard-files|authoring|export`、`login-*`、`new-bid`、`upload-drop` 保留。

## 7. 通知

| 通道 | 何时 |
| --- | --- |
| Toast | 短成功/短失败：已入库、创建失败、上传失败、冻结成功 |
| 页内 Alert | 当前页必须看见：读取失败、冻结失败、稿件失败；可带重试 |
| 字段红字 | 表单校验 |

同一件事只报一次。编制保存失败钉在编辑器条上，不用 Toast。成功默认 Toast，不占主列。

## 8. 空 / 载 / 错

空：面板内一句标题，不要说明书。载：与表同形骨架。错：Alert + 重试。

## 9. Framework

- 壳：Tailwind + shadcn
- 图标：Tabler
- 不用：Mantine、Ant、Lucide 换图标、Markdown textarea 当正式正文
- token 只写在 `web/src/app.css` 的 `@theme` 与 `:root`。工作台 CSS 引用这些变量，不写另一套蓝

## 10. Banned

- 深色侧栏或暗色主题当默认壳
- 渐变字标、登录径向光、毛玻璃顶栏、面板轻阴影
- 运行时 Google Fonts
- 顶栏大 Stepper、冻结当第四导航
- 浏览器默认 Choose File
- 外部大纲回写覆盖 DOCX
- 用整条亮蓝底表达普通进度或新稿就绪

## 11. 信息架构

顶栏只切换投标项目 | 知识资产。左栏在列表是项目或资料树，在项目内是四步。页头放当前页标题和主按钮。文件页上传。招标解读核对事项。编制页是大纲或 ONLYOFFICE。交付页下载已保存 DOCX。资料与产品走知识资产树，详情用同一套发丝线面板。

## 12. 风格落地任务

只改外观与动效。不改投标协议、保存语义、路由和 testid。

| 序 | 范围 | 完成时 |
| --- | --- | --- |
| 1 | `web/src/app.css` token、圆角、面板阴影、顶栏高度 | `@theme` / `:root` 与本节色板一致；面板 `box-shadow: none` |
| 2 | `Shell.tsx`、`App.tsx` 登录与加载 | 无渐变、无径向光、无毛玻璃；顶栏 64，页头 48 |
| 3 | `button` `badge` `alert` `dialog` `input` `sheet`、`main.tsx` Toast | 圆角 6 或 8；状态字色走 token |
| 4 | `workbench.css` `analysisReview.css` `docx.css` | 源码中不再出现 `#245ed6` `#2563eb` 和亮蓝通知底；选中用 `--iris` |
| 5 | 动效 | 颜色与分栏 140ms；`prefers-reduced-motion` 关闭位移 |
| 6 | 三屏核对 | 项目列表、分析复核、编写三栏对照本节。流程行为保持原样 |

废止稿已删除，不再作为布局依据。浏览器验收以 1440×900 为准：操作条和项目分区完整留在视口内，Word 框高度至少 420 且底边不超过视口。`tender-workbench`、`tender-execution`、`tender-flow`、`app-navigation`、`analysis-review`、`outline-planner` 已在 Chromium 通过。
