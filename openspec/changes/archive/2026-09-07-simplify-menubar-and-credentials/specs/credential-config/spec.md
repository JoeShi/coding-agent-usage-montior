# credential-config Delta

## REMOVED Requirements

### Requirement: 配置火山 AK/SK

**Reason**: arkcli 自动续期上线后，火山数据源锁定 arkcli 零配置路线；手动 AK/SK 入口成为死重，且不符合"优先 SSO + 临时 STS、不鼓励持久化 AK/SK"的规范。
**Migration**: 未登录 arkcli 的用户运行 `arkcli auth login`；Keychain 中遗留的旧 AK/SK 条目无害，可不清理。

### Requirement: 凭证来源展示与清除

**Reason**: 火山凭证入口删除后，该需求只剩 Kiro API Key 的状态展示与清除，由新增的"Kiro API Key 状态展示与清除"需求承接。
**Migration**: Kiro API Key 的状态展示与清除行为见新增需求"Kiro API Key 状态展示与清除"；轮询偏好见"偏好设置"需求。

## MODIFIED Requirements

### Requirement: 凭证安全存储

Kiro API Key MUST 存储于 macOS Keychain；系统 MUST NOT 将 API Key 以明文写入配置文件、日志或遥测。Kimi Code 凭证遵循其 CLI 的既有存储位置，不额外复制；火山 AgentPlan 凭证完全复用 arkcli 的既有存储，系统不保存任何火山密钥材料。

#### Scenario: Secret Key 不落盘

- **WHEN** 系统持久化或读取任何凭证
- **THEN** 应用数据目录内的任何配置文件中均不包含密钥明文

#### Scenario: Kiro API Key 不落盘

- **WHEN** 用户保存 Kiro API Key 后
- **THEN** 应用数据目录内的任何配置文件中均不包含该 key 明文

## ADDED Requirements

### Requirement: Kiro API Key 状态展示与清除

系统 SHALL 在设置窗口展示 Kiro API Key 的配置状态（已配置 / 未配置），并允许用户清除已保存的 Kiro API Key。

#### Scenario: 查看状态

- **WHEN** 用户打开设置窗口
- **THEN** 界面显示 Kiro API Key 当前是否已配置

#### Scenario: 清除 Kiro API Key

- **WHEN** 用户点击清除已保存的 Kiro API Key
- **THEN** 系统从 Keychain 删除该 key，Kiro 数据源回到未配置状态
