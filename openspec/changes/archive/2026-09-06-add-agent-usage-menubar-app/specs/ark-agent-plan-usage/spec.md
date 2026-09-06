## Purpose

通过火山引擎方舟管控面 API 查询 AgentPlan 个人版套餐的额度使用情况，包括 5 小时 / 1 天 / 1 周 / 1 月四个滚动窗口的 AFP 配额与分模型用量明细。

## ADDED Requirements

### Requirement: AK/SK 签名查询套餐配额

系统 SHALL 使用用户配置的火山引擎 Access Key / Secret Key，以 V4 签名（service: `ark`，region: `cn-beijing`）调用 `POST https://ark.cn-beijing.volcengineapi.com/?Action=GetAFPUsage&Version=2024-01-01`，并解析四个滚动窗口（AFPFiveHour、AFPDaily、AFPWeekly、AFPMonthly）的 Quota、Used、ResetTime 及套餐档位（PlanType）。ARK API Key（Bearer 方式）MUST NOT 用于管控面查询——该方式已被服务端拒绝。

#### Scenario: 查询成功

- **WHEN** AK/SK 有效且账号已订阅 AgentPlan 个人版
- **THEN** 系统返回四个滚动窗口的配额、已用量、重置时间和套餐档位

#### Scenario: 鉴权失败

- **WHEN** AK/SK 无效或权限不足
- **THEN** 系统将数据源标记为鉴权错误状态，界面提示用户检查 AK/SK 及其 ArkReadOnlyAccess 权限

### Requirement: 查询分模型用量明细

系统 SHALL 支持调用 `POST https://ark.cn-beijing.volcengineapi.com/?Action=GetUsageDetails&Version=2024-01-01`，按天或小时粒度查询指定时间范围内的分模型用量明细，用于明细视图展示。

#### Scenario: 按天查询明细

- **WHEN** 用户打开明细视图并选择时间范围
- **THEN** 系统以 Day 粒度查询并展示该范围内各模型的用量

### Requirement: arkcli 登录态 fallback

当用户未配置 AK/SK 时，系统 SHALL 检测本机是否存在 arkcli 的 SSO 登录态（`~/.arkcli` 下的 STS 临时凭证），若存在且未过期，则复用该 STS 凭证（含 session token）进行 V4 签名查询。系统 MUST NOT 自行刷新 arkcli 的 STS 凭证；凭证过期时提示用户运行 `arkcli auth login` 重新登录。

#### Scenario: 零配置使用 arkcli 登录态

- **WHEN** 用户未配置 AK/SK，且本机 arkcli 登录态有效
- **THEN** 系统自动复用该登录态完成查询，界面标注凭证来源为 arkcli

#### Scenario: arkcli 凭证过期

- **WHEN** arkcli 的 STS 凭证已过期
- **THEN** 系统将数据源标记为需要重新登录，提示用户运行 `arkcli auth login` 或改配 AK/SK

### Requirement: 凭证优先级

当用户同时配置了 AK/SK 且存在 arkcli 登录态时，系统 SHALL 优先使用用户显式配置的 AK/SK。

#### Scenario: 双凭证存在

- **WHEN** AK/SK 已配置且 arkcli 登录态也存在
- **THEN** 系统使用 AK/SK 进行查询

## MODIFIED Requirements

（无）

## REMOVED Requirements

（无）
