# Design: fix-popover-and-sts-refresh

## Context

见 proposal.md 的 Why。两个 bug 均已在本机实锤根因：

- Bug 2（STS 续期失效）：`env -i PATH=/usr/bin:/bin:... arkcli ...` 退出 127（`env: node: No such file or directory`）；注入 arkcli 同目录后 exit 0。正式版运行期间 sts.json 从未被 app 刷新（现场观测 7 分钟无写入）。
- Bug 1（弹窗定位）：tao 0.35 `set_outer_position` 用 `self.scale_factor()`（窗口**当前**所在屏的 backingScaleFactor）做物理→逻辑转换；窗口上次显示在 Retina 内屏（scale 2），用户在 1x 外接屏点击时坐标被错误地除以 2，面板落到屏幕外。之后 `is_visible()` 恒 true，点击只是 toggle 屏外窗口。tauri-apps/tauri#7890 同族问题。

## Goals / Non-Goals

Goals:
- GUI 极简 PATH 环境下 STS 续期真正生效。
- 混 DPI 多屏下弹窗 100% 出现在被点击屏幕的可见区域。
- 关键事件有本地日志可回溯。

Non-Goals:
- 不改 positioner 依赖本身（仍保留用于非托盘场景的话也可以直接移除插件——见 Decisions D2）。
- 不引入日志框架依赖（log/tracing 等）。
- 不做 arkcli 不存在的场景外的任何凭证行为变更。

## Decisions

### D1: 续期子进程注入 PATH，而不是解析 shebang 或包装 shell

`Command::new(&bin)` 不变，仅 `cmd.env("PATH", format!("{}:{}", bin_dir, existing_path))`，其中 `bin_dir = bin.parent()`。

- 为什么对：nvm、homebrew、/usr/local 安装方式下 `node` 都与 `arkcli` 同目录；一条规则全覆盖。
- 备选 1：`sh -c "source nvm.sh && arkcli ..."` —— 引入 shell 依赖、启动慢、nvm.sh 初始化很慢（秒级），排除。
- 备选 2：直接调 `node <arkcli.js>` —— 依赖 arkcli 内部布局（wrapper 脚本会找 pkg/bin 下真实二进制），比 PATH 注入脆弱，排除。

### D2: 弹窗定位改为自算逻辑坐标，弃用 positioner 的 TrayCenter

新增 `fn position_popover(win: &WebviewWindow)`：

1. `win.cursor_position()`（物理）→ `app.monitor_from_point(x, y)` 得目标显示器（找不到则退回 `current_monitor` / primary）。
2. 光标逻辑坐标 = 物理 ÷ 目标显示器 scale；窗口点尺寸 = `win.outer_size()`（物理）÷ 窗口自身 scale（点尺寸跨屏不变）。
3. x = 光标逻辑 x − 窗口点宽/2，clamp 到 `[monitor.x, monitor.x + monitor.w − window.w]`（逻辑域）；y = 显示器逻辑 top（macOS 菜单栏在顶部，窗口管理器会自动避开菜单栏；为稳妥可用 `monitor.position().y` 直接贴顶）。
4. `win.set_position(tauri::LogicalPosition::new(x, y))` —— `Position::Logical` 在 tao 的 `to_logical` 中是恒等转换，完全绕开有 bug 的 scale 换算。

- 为什么不用 `move_window_constrained`：它内部最终仍走 `set_position(PhysicalPosition)`，同一个 bug，clamp 也救不了。
- positioner 插件去留：插件仍保留（`on_tray_event` 记录无害，移除属于额外 churn），仅不再调用 `move_window`。
- `show_main_window`（托盘菜单"打开面板/设置"）与点击处理共用 `position_popover`。
- 另一个坐标坑（实装后用户实测发现）：`cursor_position()` 返回"物理坐标"= 全局点 × **主屏** scale，而 `monitor_from_point` 内部用 `CGDisplayBounds`（全局**点**）做包含测试——直接传物理坐标会全部 miss 并静默落到 `current_monitor` 兜底，内屏点击被误判到窗口上次所在的外接屏。修复：光标先除以主屏 scale 还原为点，再做显示器命中测试。

### D3: 手写极简文件日志，不引依赖

新增 `src-tauri/src/log.rs`：

- `pub fn log(msg: &str)`：单行 `{本地时间} {msg}\n` 追加到 `~/Library/Logs/agent-plan-monitor.log`；写入前检查文件大小，超过 1 MB 则截断重写（保留尾部一半亦可，取最简：直接 truncate 重写并记一行 "log rotated"）。
- 任何 IO 错误静默忽略（日志绝不能搞挂业务）。
- 埋点位置：`try_refresh_arkcli_sts`（触发/冷却跳过/找不到二进制/spawn 失败/超时/非零退出含 exit code/成功）、`position_popover`（目标坐标+显示器）、`refresh_all` 各源错误状态。
- 不记密钥：只记状态类别与坐标。

## Risks / Trade-offs

- [PATH 注入污染子进程其他行为] → 仅 prepend 一个目录，不动其余环境变量；arkcli 只读 PATH 找 node。
- [逻辑坐标的 y 贴顶被菜单栏遮挡] → macOS 对 setFrame 不会自动避开菜单栏；若实测遮挡，改为 y = monitor.y + 菜单栏高度（约 24 点），实现时实测确认。
- [显示器拓扑在点击瞬间变化（拔插中）] → 每次点击都现取 cursor_position 与 monitor，无缓存状态，天然安全。
- [日志并发写交错] → 写入路径都在低频事件上，用一次性 `OpenOptions append` 写单行即可，不加锁。

## Migration Plan

无数据/配置迁移。构建 release、重装到 /Applications 后验证：
1. 把 sts.json expires_at 改到过去 → 等一个轮询周期 → sts.json mtime 更新且界面不报 NeedRelogin。
2. 接外接屏，在两块屏分别点击托盘图标 → 面板都出现在对应屏幕菜单栏下方。
3. 检查 `~/Library/Logs/agent-plan-monitor.log` 有续期与定位记录、无密钥。
