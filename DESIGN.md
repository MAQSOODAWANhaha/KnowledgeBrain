# Design System: KnowledgeBrain 投标台

外壳走 TokHub / Apple 高质感白：纯白、毛玻璃顶栏、浅侧栏、iOS 蓝、大圆角轻投影。本文只约束 Web 外壳，不强加到 ONLYOFFICE 或 DOCX 正文。

## 1. Visual Theme

纯白工作台，卡片浮起。Variance 5，Motion 3，Density 7。

- 顶栏：白 82% + `blur(16px)` + `saturate(180%)`
- 侧栏：`#FAFAFB`，不是深色
- 主列：白
- 品牌：`#2563EB`，字标蓝渐变
- 卡片圆角 14、按钮 9、徽章胶囊

登录独立全屏，无应用顶栏。底 `#FAFAFB` + 顶部淡蓝径向光，居中白卡。

不抄 TokHub 首页 KPI / 营销 Hero。不学：深色侧栏、Cloudflare 锌台、稿纸、霓虹。

## 2. Color Palette

| Token | Hex | 用途 |
| --- | --- | --- |
| Background | `#FFFFFF` | 窗口、主列 |
| Soft | `#FAFAFB` | 侧栏、表头、工具条 |
| Soft 2 | `#F6F6F8` | hover |
| Surface | `#FFFFFF` | 卡、表、登录、Dialog |
| Text | `#14141A` | 正文 |
| Text 2 | `#5B5B67` | 次级 |
| Text 3 | `#9A9AA6` | 元数据 |
| Border | `#EDEDF1` | 细线 |
| Border strong | `#E2E2E8` | 输入描边 |
| Brand | `#2563EB` | 主按钮、选中、焦点 |
| Brand 2 | `#3B82F6` | 字标亮端 |
| Brand ink | `#1D4ED8` | 主按钮 hover |
| Brand soft | `#EAF1FF` | 选中浅底 |
| Go | `#15A34A` | 可检索 |
| Wait | `#D97706` | 解析中 |
| Stop | `#E11D48` | 失败 |
| Shadow | `0 1px 2px rgba(17,17,26,.04), 0 6px 18px rgba(17,17,26,.05)` | 卡 |

字标 32×32、圆角 10、渐变 `140deg #60A5FA → #2563EB → #1D4ED8`。

## 3. Typography

PingFang SC / -apple-system / Noto Sans SC。禁止 runtime Google Fonts。

12 元数据 / 14.5 导航与按钮 / **16 正文** / 16 小节 / 24 页标题。树行 40px / 15px。文档 Tab：浅底条 52px、16px 字、选中为字下 28×3 圆角蓝条。字重 400 / 550–600 / 700。`tabular-nums`。

## 4. Components

Vite + React 19 + Tailwind v4 + shadcn。图标 Tabler。正文 ONLYOFFICE。

- 壳：grid 顶栏 62、左栏 246、主列 1fr
- 按钮高 36、圆角 9。主 Brand 填实。次白底细边。危险只用 Stop 字色
- 输入高 36、圆角 9、白底 + Border strong，焦点 2px Brand
- 表在 14px 白卡 + 轻阴影内。状态胶囊
- Dropzone 虚线、圆角 12。保留 `upload-drop`
- Dialog 440、圆角 14、遮罩 40%
- Toast：Sonner 顶中，白卡细边轻阴影，4s

## 5. Layout

```text
62px 毛玻璃顶栏
246px 浅侧栏 | 1fr 主列
```

- 顶栏左字标；正中 投标项目 | 知识资产是**导航文字**（选中底边 Brand 2.5px），不要做成胶囊按钮
- **账号与退出在左栏底部**
- 左栏永远显示。当前标下挂 文件 / 编制 / 导出
- 编制主列给 ONLYOFFICE

## 6. Motion & Interaction

150–180ms ease-out。尊重 `prefers-reduced-motion`。

`:focus-visible` 2px Brand、offset 2。

未确认 DOCX：继续 `guardHashNavigation` / `onBeforeLeave` / `beforeunload`。不要用冻结禁用编制/导出。

testid：`wizard-files|authoring|export`、`login-*`、`new-bid`、`upload-drop` 保留。

## 7. 通知

| 通道 | 何时 |
| --- | --- |
| Toast | 短成功/短失败：已入库、创建失败、上传失败、冻结成功 |
| 页内 Alert | 当前页必须看见：读取失败、冻结失败、稿件失败；可带重试 |
| 字段红字 | 表单校验 |

同一件事只报一次。编制保存失败钉在编辑器 banner，不用 Toast。成功默认 Toast，不占主列。

## 8. 空 / 载 / 错

空：卡内一句标题，不要说明书。载：与表同形骨架。错：Alert + 重试。

## 9. Framework

- 壳：Tailwind + shadcn
- 图标：Tabler
- 不用：Mantine、Ant、Lucide 换图标、Markdown textarea 当正式正文
- token：`web/src/app.css` `@theme` 与 `:root`

## 10. Banned

- 深色侧栏当默认壳
- 运行时 Google Fonts / 必选 SF Pro 文件
- 顶栏大 Stepper、冻结当第四导航
- 浏览器默认 Choose File
- 外部大纲回写覆盖 DOCX

## 11. 信息架构

顶栏切换 投标项目 | 知识资产。投标左栏：当前标三步 + 其他项目。文件页：上传表 + 冻结。编制：ONLYOFFICE。导出：下载已保存 DOCX。资料/产品走知识资产树。
