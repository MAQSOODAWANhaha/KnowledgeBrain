# Design System: KnowledgeBrain 投标台

## 1. Visual Theme & Atmosphere

白天办公桌上的 macOS 工作台：iCloud 设置那种分组列表，Mail 那种三栏审阅。Density 5 Daily App Balanced — 行距留白够点、够读，不挤成机房值班屏。Variance 3 Predictable Symmetric — 投标 / 资料 / 产品 同一壳，打开标是一张工作台。Motion 4 Fluid CSS — 180ms 的面板开合与按钮回弹，没有入场编排。

气氛是苹果系统设置 + 邮件：浅雾灰底、白分组面、一颗湖水蓝只给当前项和主行动。不是 Cloudflare 锌灰密表，不是稿纸，不是赛博深色。

登录是唯一第一眼：雾灰全屏，中央一张白卡片，一个「进入」。不要分栏海报，不要插图标题。

## 2. Color Palette & Roles

- **Mist Canvas** (#F5F5F7) — 窗口与侧栏底，苹果系统灰
- **Pure Surface** (#FFFFFF) — 分组列表、文稿、检查器
- **Graphite Ink** (#1D1D1F) — 主文字，禁止 #000000
- **Quiet Label** (#6E6E73) — 次级说明、未选导航
- **Hairline** (rgba(60, 60, 67, 0.12)) — 分组内部分割
- **Lake Blue** (#1D6FD8) — 唯一强调：进入、开标、确认、定稿、选中行。饱和度 < 80%
- **Lake Wash** (#E8F1FC) — 当前导航、当前条款
- **Go Pine** (#248A3D) — 仅语义：completed / 可检索 / 已确认
- **Wait Amber** (#C93400) — 仅语义：pending / 解析中（苹果系统橙，不作第二品牌色）
- **Stop Clay** (#D70015) — 仅语义：failed / 丢掉

禁止第二品牌色。链接与焦点环只用湖水蓝。

## 3. Typography Rules

- **UI：** `-apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", "Noto Sans SC"`。苹果系统栈是本世界的字，不是退路
- **字号阶：** 13 / 15 / 17 / 22 / 28。正文 15px，行高 1.47。登录标题 28px，字距 -0.025em
- **字重：** 400 正文，510 导航，600 标题与主键
- **数字：** `tabular-nums`。日期、覆盖率、状态码用等宽数字，不另开展示体
- **条款正文** ≤ 65ch
- **Banned：** Inter、Outfit、宋体/衬线、流体 clamp 大标题、全大写栏目标签

## 4. Component Stylings

全部走 Mantine 7，禁止裸 HTML 控件当主交互。

* **Buttons：** 高 36px，圆角 10px。主按钮 Lake Blue 填实，按下 scale(0.98)。次按钮浅填 `#E8E8ED`，无描边。危险操作用字色 Stop Clay
* **Inputs：** 标签在上 13px Quiet Label，框高 40px，圆角 10px，浅填 `#F2F2F7`，焦点 2px Lake Wash + 1px Lake Blue。不要浮动标签
* **Grouped lists：** 白底圆角 12px，内部分割 Hairline，行高 48px。主键可点。不要卡片宫格
* **Sidebar：** 宽 240px，Mist Canvas。选中行 Lake Wash + Lake Blue 字，圆角 8px
* **Inspector：** 宽 320px，白底，分组控件。确认是唯一主按钮
* **Dropzone：** 虚线浅框，圆角 12px。禁止浏览器默认 Choose File
* **Modal：** 居中 440px，圆角 16px，软阴影。只用于「新建标」这种需要护栏的创建
* **Loaders：** 与列表同形的骨架，禁止居中大转圈
* **Empty：** 一句下一步，不要插画
* **SegmentedControl：** 表 / 文稿，替代自制描边按钮组

## 5. Layout Principles

Apple HIG Sidebar + Split View：

```
240px 侧栏 | 1fr 主列 | 320px 检查器（投标工作台）
```

列表页（投标 / 资料 / 产品）收起检查器。小于 768px：侧栏抽屉，检查器叠在文稿下。主列最大 920px 居中。全高用 `min-height: 100dvh`。

## 6. Motion & Interaction

180–220ms，`transform` + `opacity`，ease-out。按钮按下 0.98。侧栏抽屉滑动。不要列表 stagger。骨架只在载入。

业务路径（同一壳，打开标是一张工作台）：

1. 登录 → 进入
2. 投标列表 → 新建标（Modal）或点开一行
3. 拖入招标文件 → 解析 / 抽取状态写在检查器
4. 先确认项目事实与条款 kind：接受/修订事实，确认或丢弃条款
5. 再看两路匹配：商务证据、技术 supported 候选，按需求勾选 1..N 个产品
6. 人工完成报价定稿、公司/投标资料和程序材料处理
7. 主列 `[表 | 文稿]` 编①～⑥；过程 Word 可带 warning，正式 PDF 通过门禁且不覆盖人稿

## 7. Framework & Component Map

- **壳：** Vite + React 19
- **库：** Mantine 7（企业级组件，已在仓库）。图标 `@tabler/icons-react`
- **不用：** 原生 button/input/file、Ant Design 密表、shadcn 换栈
- **映射：** AppShell · NavLink · Modal · Dropzone · SegmentedControl · Table · TextInput · PasswordInput · Textarea · Button · Badge · Progress · Skeleton · Notifications · FileButton

## 9. 业务交互（信息架构）

三个平级根，侧栏永远先选根，再进子树。禁止把招标文件丢进资料或产品。

```
投标
  在办的标…
  打开后本标
    上导航：文件 → 事实/条款 → 匹配/选择 → 报价/材料 → 成稿
    进标先落本标文件；侧栏跟步走一棵本标树
      文件：本标招标文件（点文件名滚到该行）
      事实/条款：项目事实 / 待确认 / 已确认
      匹配/选择：商务 / 技术单元… / 未归段
      报价/材料：人工报价 / 公司与投标资料 / 程序附件
      成稿：① / 各② / ③④⑤ / ⑥函件·授权·报价·实施·程序检查
资料                         ← company，不是产品
  资质证照 / 体系认证 / 业绩案例 / 服务能力
产品                         ← 产品线，参与排序
  产品线…
    型号手册
```

**资料怎么设计。** 四个分类夹就是子分类，不要无限文件夹。夹里只有证/案例/服务扫描件。状态看 `index_ready`（可检索），不要等 wiki 完成。商务按条款找文档，夹与夹不排名。缺证：商务缺件时检查器点「去补证」落到资料夹，拖入，等可检索后自动再检。

**产品怎么设计。** 产品线是分类，型号是被排序的对象。手册进型号，不进资料。招标文件绝对不进这里。

**投标怎么走（不是向导，不锁死）。**

1. 新建标（Modal：名称、负责人、结束日；业务时间按 Asia/Shanghai）
2. 左侧「文件」Dropzone 上传一份或多份。补遗=再传。失败只红这一份，可重试/删
3. 文件行显示 排队 / 解析中 / 已解析 / 失败。全部 completed 后自动抽 draft
4. 一张工作台。先确认抽取事实与条款；人工编辑后的条款显示来源变化。未确认不进匹配
5. 确认后异步发布两路匹配。检查器展示全部 supported 并标 recommended，人按需求勾 1..N；商务按条款看证据/缺件
6. 报价由人录入、确认并定稿；最高限价含税/未税口径不明时给出明确修复路径
7. 公司/投标 profile 和程序材料逐项处理；附件状态和不适用原因可见、可审计
8. 文稿是①～⑥ current parts。过程 Word 可带 warning/placeholder；正式 PDF 对报价、有效期、待重确认条款、程序材料和 stale 做硬门禁。⑤ 缺件链到资料夹

系统减少重复操作，但事实、条款、产品选择、正式价格和程序性决定都保留人工确认。缺了就给直接修复路径；过程稿不锁死，正式 PDF 必须通过门禁。

## 8. Anti-Patterns (Banned)

- 炉橙 Cloudflare 密控台、1px 锌线铺满、Outfit
- 米黄稿纸、居中纸影、宋体条款
- Inter、纯黑、霓虹、紫蓝光晕
- Emoji、三人卡片、hero 大数字
- 「Elevate / Seamless / Unleash」
- 整页只写「载入…」
- 浏览器默认 Choose File
- 第二品牌色、深色赛博壳
- 全大写栏目标签
