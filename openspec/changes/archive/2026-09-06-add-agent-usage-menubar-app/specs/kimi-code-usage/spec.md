## Purpose

通过读取本机 Kimi Code CLI 的共享 OAuth 凭证，查询用户的 Kimi Code 订阅用量，包括周配额、5 小时滚动窗口用量和 Extra Usage 余额。

## ADDED Requirements

### Requirement: 读取共享 OAuth 凭证

系统 SHALL 从 `~/.kimi-code/credentials/kimi-code.json` 读取 Kimi Code CLI 的 OAuth 凭证（access_token、refresh_token、expires_at），与 CLI 共享同一份凭证文件。系统 MUST NOT 要求用户单独为 Kimi Code 配置凭证。

#### Scenario: CLI 已登录时读取凭证

- **WHEN** 本机存在有效的 `~/.kimi-code/credentials/kimi-code.json`
- **THEN** 系统读取其中的 access_token 并可用于用量查询

#### Scenario: CLI 未登录

- **WHEN** 凭证文件不存在或无法解析
- **THEN** 系统将 Kimi Code 数据源标记为"未配置"状态，并在界面提示用户先在 Kimi Code CLI 中登录

### Requirement: 查询订阅用量

系统 SHALL 使用 access_token 以 Bearer 方式调用 `GET https://api.kimi.com/coding/v1/usages`，并解析返回中的周配额（limit/used/remaining/resetTime）、滚动速率窗口（limits 数组中各窗口的 limit/used/remaining/resetTime）、会员等级与 Extra Usage 钱包余额（boosterWallet）。

#### Scenario: 查询成功

- **WHEN** access_token 有效且网络正常
- **THEN** 系统返回结构化的用量数据，包含周配额、滚动窗口、会员等级和 Extra Usage 余额

#### Scenario: 查询失败

- **WHEN** 请求返回非 401 的错误或网络超时
- **THEN** 系统将数据源标记为错误状态并保留上一次成功的数据，界面可区分"数据过期"与"数据有效"

### Requirement: 自动刷新过期 token

当 access_token 过期时，系统 SHALL 使用 refresh_token 向 `https://auth.kimi.com/api/oauth/token` 发起 refresh_token 授权请求获取新 token，并将新凭证原子写回 `~/.kimi-code/credentials/kimi-code.json`（写回前重读文件比对，避免覆盖 CLI 并发刷新的结果）。当刷新返回 `invalid_grant` 时，系统 SHALL 重新读取凭证文件（视为 CLI 已并发刷新）并重试一次。

#### Scenario: 过期后自动刷新

- **WHEN** access_token 已过期且 refresh_token 有效
- **THEN** 系统自动获取新 token、原子写回凭证文件，并用新 token 完成用量查询

#### Scenario: refresh_token 轮换竞争

- **WHEN** 刷新请求返回 `invalid_grant`
- **THEN** 系统重读凭证文件并使用文件中最新的 token 重试；若文件中的 refresh_token 同样失效，则将数据源标记为"需要重新登录"

### Requirement: 凭证安全

系统 MUST NOT 将 Kimi Code 的 access_token 或 refresh_token 写入日志、遥测或除原凭证文件外的任何位置。

#### Scenario: 日志不包含凭证

- **WHEN** 系统进行任何日志输出或错误上报
- **THEN** 输出中不包含 access_token、refresh_token 的完整值

## MODIFIED Requirements

（无）

## REMOVED Requirements

（无）
