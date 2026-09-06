## Purpose

提供图形化设置界面，让用户配置火山引擎 AK/SK、轮询频率与显示偏好，并确保敏感凭证只存储在 macOS Keychain 中。

## ADDED Requirements

### Requirement: 配置火山 AK/SK

系统 SHALL 提供设置窗口，允许用户输入并保存火山引擎 Access Key 和 Secret Key，并在保存时通过一次实际查询验证其有效性，向用户反馈验证结果。

#### Scenario: 保存并验证成功

- **WHEN** 用户输入有效 AK/SK 并保存
- **THEN** 系统将凭证存入 macOS Keychain，执行一次验证查询，并展示成功结果（含套餐档位）

#### Scenario: 验证失败

- **WHEN** 用户输入的 AK/SK 无效或权限不足
- **THEN** 系统展示具体错误原因，并提示用户确认 IAM 权限（如 ArkReadOnlyAccess）

### Requirement: 凭证安全存储

火山 AK/SK MUST 存储于 macOS Keychain；系统 MUST NOT 将 Secret Key 以明文写入配置文件、日志或遥测。Kimi Code 凭证遵循其 CLI 的既有存储位置，不额外复制。

#### Scenario: Secret Key 不落盘

- **WHEN** 用户保存火山凭证后
- **THEN** 应用数据目录内的任何配置文件中均不包含 Secret Key 明文

### Requirement: 凭证来源展示与清除

系统 SHALL 在设置窗口展示当前火山凭证的来源（手动配置 / arkcli 登录态），并允许用户清除已保存的 AK/SK。

#### Scenario: 查看凭证来源

- **WHEN** 用户打开设置窗口
- **THEN** 界面显示当前生效的凭证来源及状态

#### Scenario: 清除凭证

- **WHEN** 用户点击清除已保存的 AK/SK
- **THEN** 系统从 Keychain 删除该凭证，并回退到 arkcli 登录态检测（若可用）

### Requirement: 偏好设置

系统 SHALL 允许用户配置轮询间隔与状态栏显示偏好（显示哪些数据源），并持久化这些非敏感配置。

#### Scenario: 修改轮询间隔

- **WHEN** 用户修改轮询间隔并保存
- **THEN** 配置被持久化，调度器在下一周期按新间隔运行

## MODIFIED Requirements

（无）

## REMOVED Requirements

（无）
