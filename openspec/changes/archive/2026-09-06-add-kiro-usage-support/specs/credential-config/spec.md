## ADDED Requirements

### Requirement: 配置 Kiro API Key

系统 SHALL 提供设置窗口，允许用户输入并保存 Kiro API Key（`ksk_...`），保存时通过一次真实的 GetUsageLimits 调用验证其有效性，验证成功才写入 macOS Keychain，并向用户反馈验证结果（含订阅档位）。

#### Scenario: 保存并验证成功

- **WHEN** 用户输入有效的 Kiro API Key 并保存
- **THEN** 系统将 key 存入 Keychain，展示验证成功与订阅档位，并立即刷新 Kiro 数据源

#### Scenario: 验证失败

- **WHEN** 用户输入的 key 无效
- **THEN** 系统展示服务端返回的具体错误，且不写入 Keychain

## MODIFIED Requirements

### Requirement: 凭证安全存储

火山 AK/SK 与 Kiro API Key MUST 存储于 macOS Keychain；系统 MUST NOT 将 Secret Key 或 API Key 以明文写入配置文件、日志或遥测。Kimi Code 凭证遵循其 CLI 的既有存储位置，不额外复制。

#### Scenario: Secret Key 不落盘

- **WHEN** 用户保存火山凭证后
- **THEN** 应用数据目录内的任何配置文件中均不包含 Secret Key 明文

#### Scenario: Kiro API Key 不落盘

- **WHEN** 用户保存 Kiro API Key 后
- **THEN** 应用数据目录内的任何配置文件中均不包含该 key 明文

### Requirement: 凭证来源展示与清除

系统 SHALL 在设置窗口展示当前火山凭证的来源（手动配置 / arkcli 登录态）与 Kiro API Key 的配置状态，并允许用户分别清除已保存的火山 AK/SK 和 Kiro API Key。

#### Scenario: 查看凭证来源

- **WHEN** 用户打开设置窗口
- **THEN** 界面显示当前生效的凭证来源及状态

#### Scenario: 清除凭证

- **WHEN** 用户点击清除已保存的 AK/SK
- **THEN** 系统从 Keychain 删除该凭证，并回退到 arkcli 登录态检测（若可用）

#### Scenario: 清除 Kiro API Key

- **WHEN** 用户点击清除已保存的 Kiro API Key
- **THEN** 系统从 Keychain 删除该 key，Kiro 数据源回到未配置状态

## REMOVED Requirements

（无）
