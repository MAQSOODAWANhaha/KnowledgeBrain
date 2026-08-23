# 风格规范：macOS Glass（参考 gptsolwm）

| 项 | 值 |
|---|---|
| 参考 | https://yynxxxxx.github.io/gptsolwm/ |
| 气质 | macOS 毛玻璃工作台：slate 底 + 三色氛围光 + 半透卡片 |
| 栈 | 继续 **Mantine 7**（不换 Ant）。用 Mantine theme 映射下列 token |
| 模式 | 默认浅色；必须同时做 dark（参考站 `darkMode: class`） |
| 状态 | 待拍板。拍板后再改 `DESIGN.md` / `web/` |

参考站是技术博客，不是投标台。规范抽的是**材料与语法**，不是红绿灯窗、不是 2×2 卖点卡、不是三色渐变大标题。

## 1. 气氛

- 画布不是纯雾灰，是 **slate 浅底 + 三团慢漂氛围光**（青 / 紫 / 翠绿），光斑 `blur(90px)`、`opacity 0.65`、18s 漂浮。
- 主界面是一块大玻璃窗：`rounded-3xl`、半透白、`backdrop-filter: blur(24px) saturate(190%)`。
- 顶栏是浮动玻璃条（不贴顶死线）：`sticky top-4`、`h-14`、`rounded-2xl`。
- 暗色：画布 `#070a11`，玻璃 `rgba(15,23,42,0.75–0.82)`。

投标台用法：氛围光只铺在登录和壳的固定层（`pointer-events: none`）。表、树、检查器内部不再叠光斑。

## 2. 色板

主色是 **翠绿**，不是湖水蓝。青 / 紫只做辅色和语义，不当第二品牌按钮。

| Token | Light | Dark | 用途 |
|---|---|---|---|
| canvas | `#f1f5f9`（slate-100） | `#070a11` | 窗口底 |
| glass | `rgba(255,255,255,0.78–0.82)` | `rgba(15,23,42,0.75–0.82)` | 壳、卡、顶栏 |
| glass-border | `rgba(255,255,255,0.6–0.8)` | `rgba(255,255,255,0.10–0.15)` | 玻璃描边 |
| ink | `#1e293b`（slate-800） | `#f1f5f9` | 正文 |
| muted | `#64748b`（slate-500） | `#94a3b8` | 次级 |
| **brand** | `#10b981` / `#059669` | `#34d399` | 主键、选中、进度、成功 |
| brand-soft | `#10b981` @ 10–15% | 同 | 选中行、pill 底 |
| cyan | `#06b6d4` / `#22d3ee` | 同 | 辅信息、商务 hit |
| purple | `#a855f7` | 同 | 仅徽章 / 渐变点缀 |
| rose | `#f43f5e` | 同 | 失败 / 缺件 / 丢掉 |
| amber | `#f59e0b` | 同 | 警告 / 解析中 / 合规条 |
| terminal | `#0a0e17` @ 95% | 同 | 成稿源码、日志 |
| traffic | `#ff5f56` `#ffbd2e` `#27c93f` | 同 | 仅终端窗标题栏，不进业务导航 |

选区：`rgba(16,185,129,0.3)` 底 + `#34d399` 字。

**禁止：** 纯黑 `#000`、纯白大面当玻璃、Lake Blue `#1D6FD8`（本世界已换主色）、第二主键色。

## 3. 字体

```
sans: -apple-system, BlinkMacSystemFont, "SF Pro Text", "SF Pro Display",
      "PingFang SC", "Noto Sans SC", Inter, "Helvetica Neue", sans-serif
mono: "SF Mono", "JetBrains Mono", Menlo, Monaco, Consolas, monospace
```

| 角色 | 大小 | 字重 | 其它 |
|---|---|---|---|
| 页题 | 22 / 28（工作台） | 700–800 | `tracking-tight`，行高紧 |
| 区题 | 18–20 | 700 | 下沿 hairline |
| 正文 | 14–15 | 400 | `leading-relaxed` |
| 导航 / 表 | 13–14 | 510–600 | |
| 说明 / 标签 | 11–12 | 500 | pill 用 11 |
| 代码 / 路径 / slug | 12 | 500–700 | 翠绿字 |

渐变字（emerald → cyan → purple）只给登录标题或空状态一句，不进条款正文。

## 4. 形与影

| 件 | 圆角 | 影 |
|---|---|---|
| 顶栏 / 按钮 / 输入 | 12px（`rounded-xl`） | 主键 `shadow-md` |
| 卡片 / 表壳 / 终端 | 16px（`rounded-2xl`） | `0 8px 32px rgba(0,0,0,.08)` |
| 外壳窗口 | 24px（`rounded-3xl`） | `0 20px 50px rgba(0,0,0,.15)` |
| pill / 滚动条 | 9999 | 无 |
| 图标井 | 12px | 无 |

玻璃卡 hover：`translateY(-2px)` + `0 12px 30px rgba(0,0,0,.12)`，曲线 `cubic-bezier(0.16, 1, 0.3, 1)`，300ms。  
主键按下 `scale(0.95)`，悬停 `scale(1.02)` 仅登录/空状态；工作台按钮只做颜色与 0.98。

滚动条：6px、透明轨、灰拇指、全圆。

## 5. 组件语法（对照参考站）

**顶栏：** 浮动玻璃；左：可选三点灯 + 竖线 + 方标（翠绿→青渐变）+ 名；右：主题切换、主行动。高 56。

**侧栏：** 仍是树，但贴在玻璃窗里，不要实心 `#F7F8FA` 死板。选中 = brand-soft 底 + 翠绿字。组标题 11px muted。

**表：** 外壳 `rounded-2xl` + 半透描边。表头 `bg-slate-200/50`。行 hover `slate-200/30`。当前行 brand-soft。字段名可用 mono 翠绿。缺件 rose。

**检查器：** 玻璃卡叠放，不是实心白柱。人评用 segmented；去补证是黑底浅色字（参考站 CTA：`bg-slate-900 text-white`，暗色反相）或翠绿填实，二选一钉死：**主键 = 翠绿填实**。

**徽章：** `rounded-full`，15% 色底 + 25% 色边 + 同色字。种类：brand / cyan / purple / rose / amber。不要实心灰块。

**告警条：** 玻璃卡 + 左 4px amber。

**终端 / 成稿源码：** 深色窗、三点灯、mono 翠绿代码、顶栏「复制」。预览列仍是浅玻璃表。

**步骤点：** 翠绿圆 + `shadow-glow`（`0 0 30px rgba(16,185,129,.25)`）。只用于空状态/引导，不给条款编号。

**图标：** 单色面标，放在 36px 圆角井里（`bg-emerald-500/15 text-emerald-500`）。继续 `@tabler/icons-react`，不引 Font Awesome。

## 6. 动效

- 氛围光 18s `ease-in-out` 交替漂。`prefers-reduced-motion` 时关掉。
- 顶阅读条（可选）：1px 渐变 `emerald-400 → cyan-400 → purple-500`。工作台可做成匹配进度，不要假阅读进度。
- 主题切换 300ms `transition-colors`。
- 不做列表 stagger、不做入场编排。

## 7. 投标台怎么铺（不是博客）

```
浮动玻璃顶栏
└ 玻璃大窗
   232 树 | 1fr 表/文稿 | 300 检查器
```

| 屏 | 怎么用这套语法 |
|---|---|
| 登录 | 全屏氛围光 + 居中玻璃卡 + 翠绿进入 |
| 列表 | 顶栏 + 左在办树 + 玻璃表 |
| 商务/技术 | 三栏玻璃；表是主角；检查器小卡 |
| 成稿 | 左源码终端窗，右玻璃预览 |
| 资料/产品 | 同一壳，少检查器 |

## 8. 故意不搬

- 每张卡一套彩虹图标井（工作台会花）
- 标题整句三色渐变
- 红绿灯当导航
- 2×2 卖点宫格当条款
- Inter 当唯一字体（系统栈在前，Inter 只垫底）
- 换 Ant / 卸 Mantine

## 9. 和旧规范的替换

| 旧（iCloud） | 新（这套） |
|---|---|
| Mist `#F5F5F7` | slate-100 + 氛围光 |
| Lake `#1D6FD8` | Emerald `#10b981` |
| 圆角 10–12 | 12 / 16 / 24 |
| 实心白面 | 半透玻璃 |
| 只浅色 | 浅 + 深 |
| 顶栏贴边 AppShell | 浮动玻璃顶栏 |
