# Proposal: menubar-popover-style

## Why

当前点击状态栏图标弹出的是一个普通 NSWindow：白色不透明背景、带标题栏、可拖动，与 macOS 上其他状态栏应用（半透明毛玻璃、锚定在图标下方、不可移动、点击外部自动收起）的原生 popover 体验不一致，显得突兀。

## What Changes

- 主面板窗口改为无边框、透明背景，通过 `window-vibrancy` 应用 macOS popover 毛玻璃材质
- 窗口定位到托盘图标正下方弹出（`tauri-plugin-positioner`），不再出现在屏幕中央/上次位置
- 窗口失焦（点击外部）时自动收起；不再可拖动、不可出现在 Dock 切换器语义之外的普通窗口形态
- 前端样式配合：去掉白底、改为圆角 + 透明，适配毛玻璃背景与系统深浅色
- 设置视图仍在同一窗口内；若输入凭证时误点外部导致失焦收起的问题在实现验证中不可接受，再考虑独立设置窗口（当前不做）

## Capabilities

### New Capabilities

（无）

### Modified Capabilities

- `menubar-display`: 「下拉明细视图」的窗口形态要求变化——从普通窗口改为原生 popover 风格（透明毛玻璃、锚定托盘图标、失焦自动收起、不可拖动）；「入口导航」中设置入口打开的窗口形态随之变化

## Impact

- `src-tauri/tauri.conf.json`：窗口配置（`transparent`、`decorations: false` 等）
- `src-tauri/Cargo.toml`：新增依赖 `window-vibrancy`、`tauri-plugin-positioner`
- `src-tauri/src/lib.rs`：托盘点击时定位窗口、失焦隐藏、应用 vibrancy
- 前端 `src/`：背景透明化、圆角、深浅色适配
- 无 API / 数据模型变化；凭证与轮询逻辑不受影响
