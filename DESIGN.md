# Design System: KnowledgeBrain 投标管理平台

企业级投标工作台。设计语言参照 Microsoft Fluent 2：中性灰阶、一个品牌蓝、语义色只表示状态，和投标人员每天使用的 Word / ONLYOFFICE 保持一致。本文只约束 Web 外壳，不改 ONLYOFFICE 编辑器内部和 DOCX 版式。

稿件保存、锚点、版本和编制行为以 [前端工作台](docs/design/frontend-workbench-design.md) 为准。

## 1. 组件来源

界面只用 [Astryx](https://github.com/meta/astryx)（`@astryxdesign/core` + `@astryxdesign/theme-neutral`）的现成组件。品牌外观通过 `defineTheme({ extends: neutralTheme })` 覆盖 token 实现，见 `web/src/theme.ts`。

- 先查组件：`npx astryx component <Name>`；先查模板：`npx astryx template --list`
- 不自己写与 Astryx 组件同类的导航、按钮、表格、标签、对话框、抽屉、提示
- 自定义 CSS 只允许用于 ONLYOFFICE 编辑框尺寸；颜色、间距、圆角一律用 Astryx token
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

所有登录后的页面用同一个框架：

```text
┌ TopNav 48：产品名                                         账号菜单 ┐
├ SideNav 240（可收起成图标栏）┬ 页头（每页结构相同）                  ┤
│ 投标项目                    │ 面包屑                                │
│ 知识资产                    │ 标题 + 状态 Badge        次要按钮 [主按钮] │
│ ─────────                   │ 元信息一行                             │
│ 进入项目后：                 │ TabList（只放本页子视图）               │
│ 项目切换器                   ├─────────────────────────────────────┤
│ 投标流程                     │ 内容区：列表 / 详情 / 工作区 三选一     │
│  项目资料  3                 │                                       │
│  招标解读  待确认            │                                       │
│  标书编制  阻断 2            │                                       │
│  交付文件  v3                │                                       │
```

- `AppShell height="fill"`，顶栏 `TopNav`，侧栏 `SideNav collapsible`
- 页头组件 `PageHeader`（`web/src/shell/PageHeader.tsx`）：`Breadcrumbs`、`Heading level={1}`、状态 `Badge`、元信息、右侧按钮组、可选 `TabList`
- 内容区三种模式：
  1. **列表**：工具条（搜索、筛选）+ `Table` + 分页。项目列表、项目资料、知识资产
  2. **详情**：列表或正文 + 右侧 `LayoutPanel`（320–380）。招标解读、项目资料
  3. **工作区**：目录 | ONLYOFFICE 稿件 | 核对栏。标书编制正文。窄于 1200px 时核对栏改为 `Drawer`

## 5. 导航与联动

侧栏用一个 `SideNav`，不做两栏导航：

- **模块**（分组标题隐藏）：投标项目、知识资产。始终显示
- **在投标项目列表时**：增加「在办项目」分组，列出在办项目，点击进入该项目
- **进入项目后**：`topContent` 放项目切换器（`DropdownMenu`），下面是「投标流程」分组的四个 `SideNavItem`，右侧 `endContent` 用 `Badge` 显示该步状态（资料数、待确认、阻断数、最新版本）
- **知识资产**：增加「分类」分组
- 收起后只显示图标，带子项的图标悬停弹出子菜单

联动规则：

1. URL 是唯一来源。侧栏选中项、面包屑、页头标题、内容由同一路由算出
2. 任何入口进入同一页面，侧栏状态相同
3. 面包屑可点击，返回上级后侧栏同步切换
4. 页签只放当前页的子视图（如标书编制的 材料引导 / 大纲 / 正文），不重复侧栏的入口
5. 列表的筛选（全部 / 在办 / 已结束）放在页头 `TabList`，不放进侧栏

## 6. 组件规范

| 场景 | 组件 |
| --- | --- |
| 外壳 | `AppShell`、`TopNav`、`TopNavHeading`、`SideNav`、`SideNavSection`、`SideNavItem` |
| 项目切换、账号菜单、更多操作 | `DropdownMenu`、`MoreMenu` |
| 页头 | `Breadcrumbs`、`Heading`、`Badge`、`Button`、`TabList` |
| 视图切换（同一数据不同呈现） | `SegmentedControl` |
| 列表 | `Table`（`hasHover`，行选中时打开详情面板），`Pagination` |
| 搜索、筛选 | `TextInput`（search）、`Selector` |
| 章节目录 | `TreeList` |
| 详情面板 | `Layout` + `LayoutPanel`；窄屏 `Drawer` |
| 元数据 | `MetadataList` |
| 状态 | `Badge`（success / warning / error / neutral）、`StatusDot` |
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
