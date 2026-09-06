# Design: menubar-popover-style

## Context

当前主面板是 `tauri.conf.json` 里声明的普通窗口（有标题栏、白色不透明背景、可拖动），托盘左键点击时 `show() + set_focus()`（`src-tauri/src/lib.rs`）。目标是把它改造成 macOS 原生 popover 观感。方案 A：纯 Tauri 配置 + 两个成熟 crate，不引入 `tauri-nspanel` 的原生 NSPanel hack。

## Goals / Non-Goals

**Goals:**
- 毛玻璃半透明背景（`window-vibrancy` 的 popover material），随系统深浅色自适应
- 无边框、不可拖动、圆角
- 弹出时定位到托盘图标正下方
- 失焦自动隐藏；再次点击托盘图标切换显隐（现有 toggle 逻辑保留）

**Non-Goals:**
- 不引入 `tauri-nspanel` / 原生 NSPanel（作为体验不达标时的备选 B 方案）
- 不改设置视图的承载方式（仍在同一窗口内；输入凭证时失焦收起的问题实现时验证，不可接受再单开 change）
- 不改轮询、凭证、托盘标题逻辑

## Decisions

1. **窗口配置**：`tauri.conf.json` 中 main 窗口设 `transparent: true`、`decorations: false`、`alwaysOnTop: true`、`skipTaskbar: true`。无边框后系统不再提供拖动区域，满足"不可移动"；`alwaysOnTop` 保证点击其他全屏/置顶窗口前面板可见。
2. **毛玻璃**：用 `window-vibrancy` crate 的 `apply_vibrancy(window, NSVisualEffectMaterial::Menu, Some(Active), Some(12.0))`。选 `Menu` material 而非 `Popover`——`Popover` 的 BehindWindow 混合会透过桌面壁纸取色，暗色壁纸下浅色模式会渲染成深灰导致文字不可读（实测踩坑）；`Menu` 材质与系统菜单栏下拉一致，深浅色自适应由系统托管且不受壁纸影响。第 4 个参数直接给原生圆角 12。前端需把 `html/body/#root` 背景改为 `transparent`，否则 WebView 白底会盖住毛玻璃。
3. **定位**：用 `tauri-plugin-positioner` 的 `WindowExt::move_window(Position::TrayCenter)` 在每次 `show()` 前定位到托盘图标下方居中。替代方案（手写 tray icon rect 计算）没有必要的精度收益，crate 已处理多屏和 menu bar 高度。
4. **失焦收起**：在 `on_window_event` 里监听 `Focused(false)` → `hide()`。这与现有 `CloseRequested → hide` 共存。注意 dev 模式下前端热更新/调试可能触发焦点事件，实现时用真实 app 验证。
5. **圆角**：透明无边框窗口的圆角由前端 CSS 提供（根容器 `border-radius: 12px; overflow: hidden`），这是 Tauri 社区标准做法，无需原生代码。

## Risks / Trade-offs

- [透明窗口在 macOS 上偶发渲染残留/阴影异常] → 用真实构建验证；若出问题退化为 `decorations: false` + 不透明的深色/浅色纯色背景
- [`Focused(false)` 在 `set_focus()` 竞态下误触发立即收起] → show 前先定位再 `set_focus`，验证 toggle 行为；必要时加短延迟防抖
- [`tauri-plugin-positioner` 的 TrayCenter 依赖 tray icon event 里的位置信息，多屏下可能偏移] → 实现时在主屏/副屏各验证一次；偏差可接受则不加自定义逻辑
- [设置页输入凭证时误点外部导致输入丢失] → 验收时实测；若不可接受，后续单开 change 做独立设置窗口
