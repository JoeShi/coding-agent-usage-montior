# diagnostics Specification

## Purpose

为正式版（安装到 /Applications 的 GUI 应用）提供本地文件日志，记录关键诊断事件，使 arkcli 续期失败、弹窗定位异常等问题在事后可回溯，而不依赖用户复现。

## Requirements

### Requirement: 本地诊断日志

系统 SHALL 将关键诊断事件以追加方式写入 `~/Library/Logs/agent-plan-monitor.log`，每行带本地时间戳。记录的事件至少包括：STS 静默续期的触发/成功/失败（含失败原因）、弹窗定位结果（目标坐标与所依据的显示器）、刷新周期的错误。日志 MUST NOT 包含任何密钥材料（AK/SK/session token/API Key）。

#### Scenario: 续期失败可回溯

- **WHEN** STS 静默续期失败（找不到二进制、子进程非零退出、超时）
- **THEN** 日志中出现一条含失败类别的记录，事后可据此判断根因

#### Scenario: 日志不含密钥

- **WHEN** 系统写入任何日志记录
- **THEN** 日志内容不包含 AK/SK、session token 或 Kiro API Key 的明文

### Requirement: 日志大小受控

日志文件 SHALL 做简单大小轮转：超过上限（约 1 MB）时截断重写或滚动，避免无界增长。日志写入失败 MUST NOT 影响任何业务功能。

#### Scenario: 日志轮转

- **WHEN** 日志文件大小超过上限
- **THEN** 系统将其截断或滚动后继续写入，文件大小保持有界
