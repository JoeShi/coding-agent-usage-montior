# kiro-usage Specification

## Purpose

通过用户配置的 Kiro API Key 查询 Kiro CLI 订阅套餐的月度 Credits 用量、重置时间与超额计费状态，作为应用的第三个数据源。

## Requirements

### Requirement: 通过 API Key 查询套餐用量

系统 SHALL 使用用户配置的 Kiro API Key（`ksk_` 前缀），以 `Authorization: Bearer <key>`、`tokentype: API_KEY` 头调用 `POST https://codewhisperer.us-east-1.amazonaws.com/?isEmailRequired=true`（X-Amz-Target: `AmazonCodeWhispererService.GetUsageLimits`，body `{"isEmailRequired":true}`），并伪装 User-Agent 为 Kiro CLI 的 `aws-sdk-rust` 标识。系统 SHALL 解析订阅档位（subscriptionInfo.subscriptionTitle）、Credits 窗口（usageBreakdownList 中 resourceType 为 CREDIT 的 currentUsage/usageLimit/nextDateReset）、超额状态（overageConfiguration、currentOverages、overageCharges）与账号邮箱。该查询 MUST NOT 依赖本机 kiro-cli 进程或凭证。

#### Scenario: 查询成功

- **WHEN** API Key 有效且已订阅套餐
- **THEN** 系统返回月度 Credits 已用/配额/重置时间、订阅档位、超额状态与预估费用

#### Scenario: API Key 无效

- **WHEN** 服务端返回鉴权错误（如 token is invalid）
- **THEN** 系统将数据源标记为鉴权错误状态，界面提示用户检查 API Key

### Requirement: User-Agent 伪装

系统 SHALL 在请求中携带 Kiro CLI 兼容的 User-Agent 与 x-amz-user-agent（`aws-sdk-rust/1.3.10 ua/2.1 api/codewhispererruntime os/cli lang/rust app/AmazonQ-For-CLI`）。服务端对不认识的 UA 返回 403。

#### Scenario: 默认 UA 被拒

- **WHEN** 请求不带 CLI 兼容 UA
- **THEN** 服务端返回 403，系统将数据源标记为鉴权错误而非崩溃

### Requirement: 错误与网络异常处理

网络错误或 5xx SHALL 使数据源进入数据过期状态并保留上一次成功数据；未配置 API Key 时数据源为未配置状态。

#### Scenario: 网络错误

- **WHEN** 请求超时或连接失败
- **THEN** 界面保留上一次成功数据并标记为过期
