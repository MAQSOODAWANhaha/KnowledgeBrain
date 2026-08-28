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
- **Lake Wash** (#E8F1FC) — 当前导航、当前章节
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

全部走 Mantine 7，禁止裸 HTML 控件当主交互。编制画布的正文编辑器用 Tiptap，外壳仍是 Mantine。

- **Buttons：** 高 36px，圆角 10px。主按钮 Lake Blue 填实，按下 scale(0.98)。次按钮浅填 `#E8E8ED`，无描边。危险操作用字色 Stop Clay
- **Inputs：** 标签在上 13px Quiet Label，框高 40px，圆角 10px，浅填 `#F2F2F7`，焦点 2px Lake Wash + 1px Lake Blue。不要浮动标签
- **Grouped lists：** 白底圆角 12px，内部分割 Hairline，行高 48px。主键可点。不要卡片宫格
- **Sidebar：** 宽 240px，Mist Canvas。选中行 Lake Wash + Lake Blue 字，圆角 8px
- **Inspector：** 宽 320px，白底，分组控件。当前章证据和生成状态，不是审批锁
- **Dropzone：** 虚线浅框，圆角 12px。禁止浏览器默认 Choose File
- **Modal：** 居中 440px，圆角 16px，软阴影。只用于「新建标」这种需要护栏的创建
- **Loaders：** 与列表同形的骨架，禁止居中大转圈
- **Empty：** 一句下一步，不要插画
- **SegmentedControl：** 仅用于资料/产品等非编制主路径，替代自制描边按钮组

## 5. Layout Principles

Apple HIG Sidebar + Split View：

```
240px 侧栏 | 1fr 主列 | 320px 检查器（投标编制）
```

打开标的编制步：左槽是本标大纲树，主列是连续画布，右槽是当前章知识 / 提示。列表页（投标 / 资料 / 产品）收起检查器。小于 768px：侧栏抽屉，检查器叠在文稿下。编制画布主列可以比列表页更宽，不必卡 920px。全高用 `min-height: 100dvh`。

## 6. Motion & Interaction

180–220ms，`transform` + `opacity`，ease-out。按钮按下 0.98。侧栏抽屉滑动。不要列表 stagger。骨架只在载入。

业务路径（同一壳，打开标是一张工作台）：

1. 登录 → 进入
2. 投标列表 → 新建标（Modal）或点开一行
3. 文件：拖入招标文件，解析状态写在文件行和检查器
4. 编制：生成大纲（overlay）、改树、在连续画布打字 / 插表插图、填充空章
5. 导出：当前 `WorkspaceRevision` 的 Word / PDF；改完再导一份新文件

生成中、有候选、有检查提示时都不锁编辑器。不做「事实/条款 → 匹配勾选 → ①～⑥」向导，也不做业务 Gate。

## 7. Framework & Component Map

- **壳：** Vite + React 19
- **库：** Mantine 7（企业级组件，已在仓库）。图标 `@tabler/icons-react`
- **编制画布：** Tiptap WYSIWYG。章标题来自大纲树；正文关闭 heading。只给聚焦章挂活编辑器
- **不用：** 原生 button/input/file、Ant Design 密表、shadcn 换栈、Markdown textarea + GFM 当编辑真源
- **映射：** AppShell · NavLink · Modal · Dropzone · Button · Badge · Progress · Skeleton · Notifications · FileButton

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
- 用 Markdown `#` / 巨型 heading 文档冒充大纲
- 生成中 disable 画布，或把 Assessment 做成红色拦截门

## 9. 业务交互（信息架构）

三个平级根，侧栏永远先选根，再进子树。禁止把招标文件丢进资料或产品。产品交互契约见 [`docs/bidding/authoring.md`](docs/bidding/authoring.md) §2.4。

```
投标
  在办的标…
  打开后本标
    上导航：文件 → 编制 → 导出
    进标先落本标文件；编制步侧栏跟一棵本标大纲树
      文件：本标招标文件（点文件名滚到该行）
      编制：左大纲（点击跳转、拖拽调序/层级、增删改名拆合）
            中连续画布（树前序叠章；聚焦章 Tiptap，其余只读可点）
            右当前章证据 / 生成状态 / 风险提示
      导出：当前稿 DOCX / PDF；业务提示不禁用按钮
资料                         ← company，不是产品
  资质证照 / 体系认证 / 业绩案例 / 服务能力
产品                         ← 产品线，参与检索
  产品线…
    型号手册
```

**资料怎么设计。** 四个分类夹就是子分类，不要无限文件夹。夹里只有证/案例/服务扫描件。状态看 `index_ready`（可检索），不要等 wiki 完成。知识填充按当前章要求找文档，夹与夹不排名。缺证：编制检查器可点「去补证」落到资料夹，拖入，等可检索后再填。

**产品怎么设计。** 产品线是分类，型号是被检索的对象。手册进型号，不进资料。招标文件绝对不进这里。

**投标怎么走（三步，不锁死）。** 顶栏只有文件 / 编制 / 导出。下面是每步里的动作，不是六步向导。

1. **文件** — 新建标（Modal：名称、负责人、结束日；业务时间 Asia/Shanghai）。Dropzone 上传一份或多份，补遗=再传。失败只红这一份，可重试/删。文件行显示排队 / 解析中 / 已解析 / 失败；可边解析边等，不必等齐才能进编制。
2. **编制** — 「生成大纲」出 Candidate overlay（默认全选、可取消节点）。接受后立刻可改名、拖拽、打字。「填充全部空章」或「生成本章」从知识库出内容候选；文本/表/图可部分接受。人改的留下，过期候选不得覆盖。
3. **导出** — 导出当前稿。再改一字再导出是新文件。Assessment 只提示；技术失败才真正失败。

系统减少重复操作，但大纲、正文、是否忽略提示、是否导出都由人决定。缺了就给直接修复路径，不锁死编制。
