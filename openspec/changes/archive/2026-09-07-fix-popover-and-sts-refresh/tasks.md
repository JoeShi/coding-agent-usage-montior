# Tasks

## 1. 修复 STS 续期子进程 PATH（Bug 2）

- [x] 1.1 `src-tauri/src/credentials.rs`：`try_refresh_arkcli_sts` spawn 时将 arkcli 二进制所在目录（`bin.parent()`）prepend 到子进程 PATH（保留原有 PATH 在后）
- [x] 1.2 单测：验证 PATH 注入逻辑（可抽出 `refresh_command(bin) -> Command` 纯函数，断言其 PATH env 以 arkcli 目录开头）

## 2. 新增本地诊断日志

- [x] 2.1 新增 `src-tauri/src/log.rs`：`pub fn log(msg: &str)`，追加写 `~/Library/Logs/agent-plan-monitor.log`，单行带本地时间戳；超过 1 MB 截断重写并记 "log rotated"；一切 IO 错误静默忽略
- [x] 2.2 `lib.rs` 注册 `mod log`（或 `pub mod log`），在续期触发/冷却跳过/找不到二进制/spawn 失败/超时/非零退出/成功处埋点（credentials.rs 调用），日志不含任何密钥

## 3. 修复多显示器弹窗定位（Bug 1）

- [x] 3.1 `src-tauri/src/lib.rs`：新增 `position_popover(win)` —— 光标物理坐标 → `monitor_from_point` 目标显示器 → 全部换算为该显示器逻辑（点）坐标 → x 居中于光标并 clamp 进显示器逻辑边界，y 贴显示器顶部（实测确认菜单栏遮挡则 +24 点）→ `set_position(LogicalPosition)`
- [x] 3.2 托盘点击处理与 `show_main_window` 都改为调用 `position_popover`，移除两处 `move_window(Position::TrayCenter)`
- [x] 3.3 在 `position_popover` 埋点日志（目标坐标 + 显示器名/位置）
- [x] 3.4 修复显示器命中测试：`cursor_position()` 的物理坐标先除以主屏 scale 还原为全局点，再传给 `monitor_from_point`（其内部用 CGDisplayBounds 点坐标做包含测试；直接传物理坐标会全部 miss，静默落到 current_monitor 兜底导致内屏点击定位到外接屏）

## 4. 验证

- [x] 4.1 `cd src-tauri && cargo test` 全绿，`cargo check` 无新增警告
- [x] 4.2 手动：改 `~/.arkcli/identities/*/sts.json` 的 expires_at 到过去 → release 版等一个轮询周期 → sts.json 被刷新、界面不报 NeedRelogin、日志有续期成功记录
- [x] 4.3 手动：接外接屏，两块屏分别点击托盘图标 → 面板均出现在对应屏幕菜单栏下方
- [x] 4.4 检查日志文件无密钥明文
