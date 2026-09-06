# Tasks: menubar-popover-style

## 1. 窗口配置与依赖

- [x] 1.1 在 `src-tauri/Cargo.toml` 添加 `window-vibrancy` 和 `tauri-plugin-positioner` 依赖，在 `src-tauri/tauri.conf.json` 将 main 窗口设为 `transparent: true`、`decorations: false`、`alwaysOnTop: true`、`skipTaskbar: true`；验证 `cargo build` 通过
- [x] 1.2 在 `src-tauri/src/lib.rs` 注册 `tauri_plugin_positioner` 插件，setup 时对 main 窗口应用 `window_vibrancy::apply_vibrancy(..., NSVisualEffectMaterial::Popover, ...)`；验证 `cargo build` 与 `cargo test` 通过

## 2. 行为实现

- [x] 2.1 托盘左键 toggle 逻辑中，`show()` 前调用 `WindowExt::move_window(Position::TrayCenter)` 定位到托盘图标下方；验证点击图标后面板出现在图标正下方
- [x] 2.2 在 `on_window_event` 增加 `Focused(false) → hide()` 失焦自动收起；验证打开面板后点击桌面其他区域，面板自动隐藏，且再次点击托盘图标能正常切换显隐
- [x] 2.3 前端根容器（`html`/`body`/`#root` 及主面板容器）背景改为 `transparent`，加 `border-radius: 12px; overflow: hidden`；验证面板呈现毛玻璃圆角、无白色底块，深浅色模式下均正常

## 3. 验证

- [x] 3.1 运行 `cargo test` 与 `openspec validate menubar-popover-style --strict`，全部通过
- [x] 3.2 真实 app 端到端验证：毛玻璃半透明、锚定图标下方、不可拖动、点击外部收起、再次点击图标切换、设置页输入凭证时失焦收起是否可接受（如不可接受，记录为后续独立设置窗口的 follow-up）
