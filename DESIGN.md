# Design System: KnowledgeBrain 投标管理平台

企业级投标工作台。设计语言参照 Microsoft Fluent 2：中性灰阶、一个品牌蓝、语义色只表示状态，和投标人员每天使用的 Word / ONLYOFFICE 保持一致。本文只约束 Web 外壳，不改 ONLYOFFICE 编辑器内部和 DOCX 版式。

稿件保存、锚点、版本和编制行为以 [前端工作台](docs/design/frontend-workbench-design.md) 为准。

## 1. 组件来源

界面只用 [Astryx](https://github.com/meta/astryx)（`@astryxdesign/core` + `@astryxdesign/theme-neutral`）的现成组件。品牌外观通过 `defineTheme({ extends: neutralTheme })` 覆盖 token 实现，见 `web/src/theme.ts`。

- 先查组件：`npx astryx component <Name>`；先查模板：`npx astryx template --list`
- 不自己写与 Astryx 组件同类的导航、按钮、表格、标签、对话框、抽屉、提示
- 组件外观只在 `web/src/theme.ts` 的 `components` 里覆盖，不写组件外的 CSS；主题做不到的效果（状态圆点、选中竖条）不做
- 页面代码不写原生 HTML 元素和 `className`，由 `web/eslint.config.js` 的 `no-restricted-syntax` 检查；唯一例外是承载 ONLYOFFICE 的 `DocxEditor.tsx`
- 组件没有完全对应的形态时，改设计去适配组件，不自己拼
- 不用 Tailwind 工具类、不用第二套图标、不用 sonner；图标用 neutral 主题自带的图标

## 2. 颜色

| 用途 | 值 | Astryx token |
| --- | --- | --- |
| 顶栏底色 | `#0E2A47` 深海军蓝 | TopNav 组件覆盖 |
| 品牌蓝：主按钮、选中、链接、焦点 | `#0F6CBD`（暗色 `#479EF5`） | `--color-accent`、`--color-text-accent` |
| 页面底 | `#F3F4F6` | `--color-background-body` |
| 面板、表格、侧栏 | `#FFFFFF` | `--color-background-surface`、`--color-background-card` |
| 正文 / 次级文字 | `#242424` / `#616161` | `--color-text-primary` / `--color-text-secondary` |
| 分隔线 | `#E0E0E0` | `--color-border` |
| 已完成、已落位 | `#107C10` | `--color-success` |
| 待核、待确认 | `#BC4B09` | `--color-warning` |
| 阻断、失败 | `#C50F1F` | `--color-error` |

品牌蓝只出现在主按钮、当前选中、链接和焦点上。状态只用 `Badge` / `StatusDot` 的语义色表达，不用整条彩色横幅表示普通进度。

## 3. 字体与尺寸

- 字体：`"Segoe UI", "Microsoft YaHei", "PingFang SC", "Noto Sans SC", sans-serif`。不加载网络字体
- 字号基准 14，页面标题 20 / 600，区块标题 16 / 600，元信息 12–13。数字用等宽数字
- 圆角 4，控件高 32，间距按 Astryx spacing token（4 的倍数）

## 4. 页面框架

视觉基准是 `/opt/workspace/design-preview/投标台企业版草图.html`（仓库外，本机评审用）。实现与草图并排截图核对，差异要么改实现，要么在这里写明原因。

```text
┌ 顶栏 48：标志 + 产品名                                   账号 · 退出 ┐
├ 图标栏 64 ┬ 上下文面板 232 ┬ 页头（白底，结构固定）                  ┤
│ 投标项目  │ 标题      «    │ 面包屑 12                              │
│ 知识资产  │ 本模块内容      │ 标题 20/600 + 状态   次要按钮 [主按钮]   │
│           │                │ 元信息：标签 值（间隔 20）               │
│           │                │ 页签（只放本页子视图）                   │
│           │                ├──────────────────────────────────────┤
│ » 展开    │                │ 内容区（灰底，16/24 内边距）            │
```

- `AppShell height="fill"`，顶栏 `TopNav`，`sideNav` 放图标栏和上下文面板（`web/src/Shell.tsx`）
- 页头 `PageHeader`（`web/src/shell/PageHeader.tsx`）：`Breadcrumbs`、`Heading level={1}`、状态 `Badge`、元信息、右侧按钮、可选 `TabList`
- 内容区三种模式：
  1. **列表**：白底面板，工具条（搜索、筛选、合计）+ `Table`。项目列表、知识资产
  2. **详情**：列表或正文 + 右侧 `LayoutPanel`（320–380）。招标解读、项目资料。窄于 900px 时右侧面板排到列表下方
  3. **工作区**：目录 | ONLYOFFICE 稿件 | 核对栏。标书编制正文。目录和核对栏可收起成 48px 的图标列；窄于 1200px 时核对栏默认收起。编辑器至少 420px 高，高度不够时稿件区纵向滚动
- 次要工具放在页头「更多操作」（`MoreMenu`）里。与稿件并用的复核工具（定向复核与修订、核对已保存的 Word 内容）在核对栏内打开，不遮挡编辑器；执行详情、直接生成整稿用非模态 `Drawer`。关闭工具不清除未提交的输入
- 草图里的全局搜索、通知、工作台、设置、项目编号、负责人、项目成员没有对应的数据或页面，暂不放入界面

## 5. 导航与联动

两级导航：

- **图标栏**（收起状态的 `SideNav`，名为「导航」）：模块入口 投标项目、知识资产，只显示图标，悬停显示名称。上下文面板收起后，底部出现「展开」
- **上下文面板**（`SideNav`）：显示当前模块内的位置，头部是模块名和「收起」按钮
  - **投标项目列表**：筛选项目的搜索框；「视图」分组（全部、在办、本周截止、已结束，带数量）；「在办项目」分组（状态圆点、名称、截止日期）
  - **进入项目后**（面板名为「项目导航」）：项目卡（`Card` + `Item` + `Avatar`：名称、截止时间、状态和剩余天数 `Badge`，`DropdownMenu` 切换项目）；「投标流程」四步，每步是 `SideNavItem` 链接，右侧 `Badge` 显示状态（资料数、待确认、待补数、最新版本），已完成显示勾选图标；「需要处理」分组列出待确认的解读和待补记录，点击进入对应步骤
  - **知识资产**：资料目录 `TreeList`，每个节点有「+」新建
- 窄于 900px 时上下文面板隐藏，只留图标栏

联动规则：

1. URL 是唯一来源。图标栏选中项、面板选中项、面包屑、页头标题、内容由同一路由算出，列表视图写在查询参数 `?view=`
2. 任何入口进入同一页面，导航状态相同
3. 面包屑可点击，返回上级后导航同步切换
4. 页签只放当前页的子视图（如标书编制的 材料引导 / 大纲 / 正文），不重复导航里的入口
5. 列表的视图（全部 / 在办 / 本周截止 / 已结束）放在上下文面板，页头标题随视图变化

## 6. 组件规范

| 场景 | 组件 |
| --- | --- |
| 外壳 | `AppShell`、`TopNav`、`TopNavHeading`、`NavIcon`；图标栏和上下文面板都是 `SideNav`（图标栏为收起状态） |
| 项目切换、账号菜单、更多操作 | `DropdownMenu`、`MoreMenu` |
| 页头 | `Breadcrumbs`、`Heading`、`Badge`、`Button`、`TabList` |
| 视图切换（同一数据不同呈现） | `SegmentedControl` |
| 列表 | `Table`（`hasHover`，行选中时打开详情面板），`Pagination` |
| 搜索、筛选 | `TextInput`（search）、`Selector` |
| 章节目录、大纲编排、资料目录 | `TreeList`；章节移动用行末 `MoreMenu`（上移、下移、降为子级、提升一级），不做拖放 |
| 详情面板 | `Layout` + `LayoutPanel`；窄屏排到内容下方 |
| 元数据 | `MetadataList` |
| 列表行、卡片内的标题行 | `Item`、`List` |
| 折叠说明、次要细节 | `Collapsible` |
| 页面区块 | `Section`；可点击卡片 `ClickableCard` |
| 状态 | `Badge`（success / warning / error / info / neutral）：浅底、细边框，见 `web/src/theme.ts`；单独的状态点用 `StatusDot` |
| 页内必须看见的提示 | `Banner`，可带重试按钮 |
| 短成功 / 短失败 | Astryx `Toast` |
| 确认、表单弹窗 | `AlertDialog`、`Dialog` |
| 空状态 | `EmptyState` |
| 加载 | `Skeleton`（与最终内容同形）、`Spinner` |
| 进度 | `ProgressBar` |
| 上传 | `FileInput` |
| 表单 | `Field`、`TextInput`、`TextArea`、`Selector`、`CheckboxInput` |

同一件事只报一次：保存失败钉在编辑器工具条上，不再发 Toast。

## 7. 交互与动效

- 动效使用 Astryx 自带时长，不额外加入场动画。尊重 `prefers-reduced-motion`
- 焦点用 Astryx 默认焦点环
- 切换分区、选中章节、展开面板时，稿件不整页跳动
- 未确认的 DOCX 修改：继续 `guardHashNavigation` / `onBeforeLeave` / `beforeunload`
- 保留的 testid：`wizard-files|analysis|authoring|export`、`login-*`、`new-bid`、`upload-drop`、`docx-status`

## 8. 不做

- 暗色侧栏、毛玻璃、渐变、营销式大图
- 页面里自写导航、标签、按钮样式
- 顶栏放流程步骤
- 用整条彩色横幅表示普通进度或新稿就绪
