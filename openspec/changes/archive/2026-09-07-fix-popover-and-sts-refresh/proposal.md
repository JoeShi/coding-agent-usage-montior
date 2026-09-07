## Why

正式版实测发现两个 bug，均已定位根因：

1. **STS 静默续期在 GUI 环境下完全失效**：arkcli 是 `#!/usr/bin/env node` 脚本，app 从 Finder/launchd 启动时 PATH 极简（无 node），续期子进程退出 127 且错误被静默吞掉，导致用户仍需频繁手动 `arkcli auth login`。
2. **外接显示器下点击托盘图标弹不出面板**：tao 的 `set_outer_position` 用"窗口当前所在屏"的 scale factor 做物理→逻辑坐标转换，混 DPI 多屏下坐标算错，面板被放到屏幕外；之后点击只是 toggle 一个屏外窗口，看起来"点了没反应"。

此外正式版没有任何持久日志（`eprintln!` 在 launchd 下被丢弃），无法回溯诊断，需补齐基础的可观测性。

## What Changes

- 续期子进程 spawn 时把 arkcli 二进制所在目录 prepend 到子进程 PATH（node 与 arkcli 同目录，覆盖 nvm/homebrew/usr-local 所有安装方式），使 GUI 环境下续期真正生效。
- 弹窗定位放弃 tauri-plugin-positioner 的物理坐标路径，改为自行计算逻辑坐标：取光标所在显示器、用该显示器 scale 换算、窗口尺寸按点数计算、水平居中于点击位置并 clamp 到该显示器可见区域、顶部贴菜单栏下沿；托盘菜单"打开面板/设置"入口走同一定位函数。
- 新增本地文件日志：关键事件（续期触发/成功/失败原因、弹窗定位坐标、刷新错误）追加写入 `~/Library/Logs/agent-plan-monitor.log`，单行文本带时间戳，含简单的大小轮转。

## Capabilities

### New Capabilities

- `diagnostics`: 本地文件日志，记录关键诊断事件，支撑正式版问题回溯。

### Modified Capabilities

- `ark-agent-plan-usage`: "arkcli 登录态凭证" 需求补充——续期子进程必须在 GUI 启动的极简 PATH 环境下可运行（为子进程注入 arkcli 同目录到 PATH）。
- `menubar-display`: "下拉明细视图" 需求补充——多显示器、混合 DPI 场景下面板必须出现在用户点击的那块屏幕的状态栏图标下方。

## Impact

- `src-tauri/src/credentials.rs`：续期子进程 PATH 注入。
- `src-tauri/src/lib.rs`：新增逻辑坐标定位函数，替换两处 `move_window(Position::TrayCenter)`；新增日志模块调用。
- 新增 `src-tauri/src/log.rs`（文件追加写入 + 大小轮转，无新依赖）。
- 无新增第三方依赖；不涉及前端界面结构变化。
