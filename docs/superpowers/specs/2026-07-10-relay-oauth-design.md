# naicode 一等 OAuth 设计

## 1. 目标

naicode 使用酸奶中转站作为默认身份提供方，实现与官方 Codex 同等级的一等 OAuth 登录体验。浏览器授权后，客户端获得短期 access token 与可轮换 refresh token，不再创建、返回或保存长期 `sk-*`。

本设计同时满足：

- 默认使用浏览器 OAuth 登录；保留手动 API key 作为高级备用入口。
- OAuth 登录在客户端使用独立的 `RelayOAuthTokens` 模式，不伪装成 `ApiKey`、`Chatgpt` 或 `ChatgptAuthTokens`。
- access token 有效期 15 分钟。
- refresh token 连续 90 天未使用后失效，每次刷新都轮换。
- 每台设备对应独立会话，可在网站查看并单独撤销。
- `/v1/responses`、模型列表、价格查询和分组切换均可使用 OAuth 身份。
- 用户自定义 `model_provider` 和 `model_providers` 继续可用，不锁死配置。
- 保持 new-api 现有额度、分组、计费、日志和退款语义。

## 2. 非目标

- 不把酸奶中转站 token 塞入 OpenAI/ChatGPT token 结构。
- 不向 OAuth 客户端暴露 dashboard session cookie、管理 access token 或长期 `sk-*`。
- 不在首版引入第三方 OAuth client 注册；naicode 使用固定公共客户端标识。
- 不允许 OAuth token 调用普通用户后台管理接口。
- 不在本次工作中移除手动 API key 支持。

## 3. 总体架构

### 3.1 客户端认证模式

在 `codex-rs/protocol` 新增：

```rust
AuthMode::RelayOAuthTokens
```

在 login crate 增加独立凭据结构，至少包含：

```text
access_token
refresh_token
expires_at
session_id
account_id
account_name
```

`device_name` 由授权请求提交并保存在服务端；客户端可缓存用于状态展示，但不作为安全依据。

OAuth 凭据通过现有 `AuthCredentialsStoreMode` 和 `AuthKeyringBackendKind` 持久化。文件模式写入 `~/.naicode/auth.json`；系统凭据库模式沿用现有认证存储抽象。写入必须使用临时文件加原子替换，确保 refresh token 轮换期间不会只保存半套凭据。

`relay.json` 只允许保存当前分组、模型等非敏感缓存，不再保存浏览器 cookie、OAuth token 或长期 key。

### 3.2 服务端组件

new-api 新增四个边界明确的组件：

1. OAuth authorization controller：校验浏览器登录态、callback、state 透传参数和 PKCE challenge，签发一次性授权码。
2. OAuth token controller：处理 `authorization_code` 和 `refresh_token` 两种 grant。
3. OAuth session service：创建、刷新、轮换、撤销设备会话。
4. OAuth bearer middleware：验证 access token，并恢复 new-api 现有请求上下文。

OAuth 会话存储在数据库中，不再依赖进程内 `map`。这保证多实例部署、滚动重启和并发兑换均正确。

## 4. 数据模型

### 4.1 `CliOAuthAuthorizationCode`

建议字段：

```text
id
code_hash                 unique
user_id
client_id
redirect_uri
code_challenge
code_challenge_method     固定 S256
device_name
expires_at                创建后 5 分钟
consumed_at               nullable
created_at
```

约束：

- 数据库只存授权码哈希。
- `code_hash` 唯一。
- 兑换时必须在单个数据库事务中将 `consumed_at` 从空更新为当前时间；受影响行数不是 1 即失败。
- 授权码绑定 `client_id`、精确 `redirect_uri` 和 PKCE challenge。
- 过期、已使用或绑定信息不匹配统一返回 `invalid_grant`，避免泄露细节。

### 4.2 `CliOAuthSession`

建议字段：

```text
id                         UUID/随机不可枚举 ID
user_id
client_id
refresh_family_id          unique，随机会话族 ID
current_generation         从 0 单调递增
refresh_idle_expires_at    每次成功刷新后延长至 90 天
family_absolute_expires_at 创建后 365 天，届时必须重新登录
selected_group
device_name
last_used_at
last_ip
last_user_agent
revoked_at                 nullable
revoke_reason              nullable
created_at
updated_at
```

`selected_group` 是该设备会话的请求分组；设备之间可独立选择。用户被禁用、删除或额度策略拒绝时，middleware 即时拒绝，不依赖 token 自身到期。

### 4.3 `CliOAuthAccessToken`

```text
id
session_id
access_token_hash          unique
expires_at                 创建后 15 分钟
created_at
```

access token 独立成表。刷新时签发新 access token，但旧 access token 保留到自身 15 分钟期限结束，避免在途请求、同设备另一进程或刚读取旧凭据的消费者被提前踢下线。请求认证命中 token 后仍必须检查关联 session 是否已撤销、用户是否可用。

### 4.4 `CliOAuthRefreshToken`

```text
id
session_id
family_id
refresh_token_hash         unique
generation
status                     active / consumed / revoked
consumed_at                nullable
replacement_token_id       nullable
idle_expires_at
retry_ciphertext            nullable，仅用于响应丢失恢复
retry_expires_at            nullable，30 秒
created_at
```

所有已签发 refresh token 的哈希都保留到 family 的 365 天绝对期限；届时客户端必须重新进行浏览器登录。这样即使设备持续活跃，历史代际也存在明确上限，且任意旧 generation 的 token 在生命周期内都能定位其 family。family 撤销或绝对过期后再保留 7 天安全审计期，然后由每日清理任务按索引分批删除 refresh/access/code 记录。清理任务只能选择已满足 `revoked_at IS NOT NULL` 或 `family_absolute_expires_at <= now` 的 family，绝不能依据单个 refresh idle expiry 删除仍活跃 family 的历史记录。容量测试按“活跃设备每 10 分钟刷新一次、持续 365 天”的上界评估索引、清理批次和数据库空间，发布前设定监控阈值。

任何历史 generation 的 token 在幂等窗口之外再次出现，都撤销整个 session。`replacement_token_id` 和短期加密响应用于在数据库已提交、响应却丢失时恢复同一轮换结果。

### 4.5 `CliOAuthBillingPrincipal`

OAuth 计费锚点使用独立表，不复用普通 `Token`，避免生成可提交的明文 key：

```text
id
user_id                    unique
created_at
updated_at
```

它只提供稳定的计费 principal id。现有计费链若强依赖 token 记录，则通过一个集中适配器构造完整、系统管理、无限 token quota、无模型白名单的计费上下文；不得在普通 token 查询、认证、列表、详情、更新或删除 API 中出现。异步结算和退款只能引用稳定 principal，用户不能删除。

所有 OAuth 表加入 AutoMigrate，并为 token hash、session、user、family、generation 和过期时间建立必要索引与唯一约束。

### 4.6 token 格式与哈希

token 使用密码学安全随机数，不承载可解析业务信息：

```text
nai_at_<base64url random 32 bytes>
nai_rt_<base64url random 48 bytes>
nai_ac_<base64url random 32 bytes>
```

服务端存储 `SHA-256(token)`；比较使用固定时间比较。前缀只用于诊断和防止凭据误用，不作为认证依据。

## 5. OAuth 协议

### 5.1 固定客户端

```text
client_id = naicode-cli
redirect_uri = http://127.0.0.1:<ephemeral-port>/callback
```

服务端仅接受 loopback HTTP redirect：主机必须是精确的 `127.0.0.1` 或 `[::1]`，端口允许动态分配，禁止用户名、片段、通配域名、非 loopback 地址以及 `localhost` 的 DNS 歧义。

naicode 是 public client，不保存 client secret，必须使用 PKCE S256。

### 5.2 授权请求

客户端：

1. 在 `127.0.0.1:0` 启动 loopback listener。
2. 生成高熵 `state` 和 `code_verifier`。
3. 计算 `code_challenge = BASE64URL(SHA256(code_verifier))`。
4. 打开：

```text
GET /cli-auth?
  response_type=code&
  client_id=naicode-cli&
  redirect_uri=<encoded>&
  state=<random>&
  code_challenge=<challenge>&
  code_challenge_method=S256&
  device_name=<encoded>
```

授权页只显示当前账号、设备名称、请求权限、同意和拒绝，不选择分组或模型。

同意后，浏览器使用 `303` 跳转精确 redirect URI，并附加 `code` 与原始 `state`。拒绝时返回 `error=access_denied&state=...`。

客户端必须校验 state，且 loopback 首次收到终态回调后立即停止监听。超时、错误回调或 state 不匹配均不得兑换 code。

### 5.3 授权码兑换

```http
POST /api/cli/oauth/token
Content-Type: application/x-www-form-urlencoded

grant_type=authorization_code
&client_id=naicode-cli
&code=...
&redirect_uri=...
&code_verifier=...
```

授权码条件消费、`CliOAuthSession` 创建、初始 access token、初始 refresh token 和 billing principal 建立/关联必须在同一数据库事务中提交。任一步失败都回滚，授权码仍可在有效期内重试；只有事务提交后才能构造并发送 token 响应，避免已烧毁 code 对应不上完整会话。

成功响应：

```json
{
  "access_token": "nai_at_...",
  "token_type": "Bearer",
  "expires_in": 900,
  "refresh_token": "nai_rt_...",
  "session_id": "...",
  "account": {
    "id": 123,
    "name": "..."
  }
}
```

响应使用 `Cache-Control: no-store` 和 `Pragma: no-cache`。错误遵循 OAuth 风格的 `error` 字段，不返回内部栈或 token 状态。

### 5.4 刷新与轮换

```http
POST /api/cli/oauth/token
Content-Type: application/x-www-form-urlencoded

grant_type=refresh_token
&client_id=naicode-cli
&refresh_token=nai_rt_...
```

成功时原子完成：

1. 锁定对应 session。
2. 在带行锁/条件更新的同一事务中检查：session 未撤销、用户可用、refresh 未超过 idle expiry，并且严格满足 `now < family_absolute_expires_at`。绝对期限到达或超过时返回 `invalid_grant`，不得签发后继 token，也不得通过 retry cache 恢复旧轮换响应。
3. 生成新的 access token 和 refresh token。
4. 保存新哈希、generation、15 分钟 access expiry、90 天 refresh idle expiry。
5. 返回完整新 token pair。

客户端收到成功响应后必须原子替换本地 token pair；旧 refresh token 不再作为正常凭据使用。

refresh rotation 必须在单个数据库事务中完成，并由 `(family_id, generation)` 唯一约束防止多实例生成多个后继 token。事务同时把旧 token 标记为 consumed、创建下一代 token、更新 session generation，并保存一份仅在 30 秒内有效的加密轮换响应。若数据库已经提交但 HTTP 响应丢失，同一旧 refresh token 在窗口内重试只能返回第一次的同一 token pair，不能生成第二个后继 token。

重试响应必须使用具备认证完整性的 AEAD 加密。关联数据至少绑定 `session_id + family_id + generation + consumed_refresh_token_id + consumed_refresh_token_hash`；记录 `key_version`，密钥来自服务端 secret/key ring，不入数据库。旧 key 至少保留到所有对应 retry window 结束。解密后必须重新哈希响应中的 access/refresh token，并分别核对 `replacement_token_id` 和已写入 access token 记录；任一不一致即拒绝、撤销 session 并记录安全事件。禁止把某行 ciphertext 移到另一 session 或 generation 后仍可解密。

30 秒窗口之外再次使用任何已消费 generation（包括 N-2、N-10）均视为重放，立即撤销整个 session/family，并返回 `invalid_grant`。首版不得省略响应丢失恢复；客户端单飞锁只能降低并发，不能代替服务端的事务和幂等恢复。不同进程共享同一 auth 文件造成的历史 token 重放也按此安全策略处理。

### 5.5 主动刷新与 401 恢复

客户端在 access token 距离过期不足 60 秒时主动刷新。所有并发模型请求共享单飞刷新锁：

- 第一个请求执行刷新。
- 其他请求等待并复用刷新结果。
- 刷新结果持久化成功后才向等待者发布。

若 `/v1` 返回明确的 token expired 401，客户端强制刷新一次并重试原请求一次。以下情况不重试：

- refresh 返回 `invalid_grant`。
- session 已撤销。
- 账户禁用。
- 非认证类 401。
- 已经刷新并重试过一次。

refresh 失效后清除 OAuth 凭据，并提示用户重新登录；不得自动退回旧 API key。

## 6. `/v1` 鉴权与计费兼容

### 6.1 凭据归一化与 dispatcher 顺序

现有 new-api 支持从标准 Bearer、WebSocket subprotocol、Anthropic `x-api-key`、Gemini query key、`x-goog-api-key`、Midjourney secret 等位置归一化 API key。必须先保留这套现有输入归一化，再按规范化后的凭据类型分派：

1. 标准 `Authorization: Bearer nai_at_...` 才允许进入 OAuth bearer middleware；OAuth token 不接受 query、WebSocket subprotocol 或其他兼容 key 载体。
2. `sk-` 及现有 token 格式继续进入原 `TokenAuth()`，包括既有渠道后缀规则。
3. 未识别格式直接拒绝，不在多个认证数据库间盲查。

OAuth 验证完成后，在任何请求头复制、参数覆盖、表达式、诊断或日志逻辑运行前，统一清除或遮蔽原始 `Authorization`。统一敏感头 denylist 同时覆盖 `x-api-key`、`x-goog-api-key` 及现有其他凭据载体。

### 6.2 middleware 上下文契约

OAuth middleware 不得零散手写 context；必须复用或抽取现有 `SetupContextForToken()` 的完整语义，通过 OAuth principal adapter 一次性建立计费、日志、退款、限流和路由所需上下文，包括但不限于：

```text
ContextKeyUserId
ContextKeyUserGroup
ContextKeyUsingGroup
ContextKeyTokenGroup
ContextKeyTokenId / BillingPrincipalId
额度、无限额度、模型限制、cross-group retry 及异步结算所需上下文
```

`ContextKeyUsingGroup` 与 OAuth 语义下的 `ContextKeyTokenGroup` 均来自 session 的 `selected_group`。每次请求都重新校验用户当前是否仍可访问该分组、分组是否启用、请求模型是否可用，以及 `auto` 和 cross-group retry 的现有规则；不得只信任 session 的历史值。OAuth billing principal 本身不携带用户可配置模型白名单或固定 group。

### 6.3 独立计费 principal

OAuth 使用 `CliOAuthBillingPrincipal`，不创建普通 token，也不生成任何可认证明文 key。每个用户最多一个稳定 principal，由系统管理，不能通过普通 token 的认证查询、列表、详情、更新或删除 API 访问。

计费适配器必须覆盖现有 token 上下文的完整契约，而不只是设置 token id：无限 token quota、用户额度、预扣、结算、退款、模型限制、group、日志关联与 cross-group retry。异步结算与退款引用稳定 principal id；设备 session 撤销不删除 principal。若某条旧代码只能接受 `model.Token`，适配器可以构造不可持久认证的内部值，但不得写入普通 token 表。

## 7. 分组与模型

### 7.1 获取可用项

OAuth access token 可访问专用只读接口：

```text
GET /api/cli/oauth/catalog
```

响应包含当前用户可选分组、每组倍率，以及该组实际支持的模型和价格。服务端从现有动态配置和权限计算，客户端不得硬编码分组。

匿名 `/api/pricing` 可以继续用于登录前展示，但登录后的 `/model` 应优先使用授权 catalog，避免显示用户无权使用的分组。

### 7.2 切换分组

```http
PUT /api/cli/oauth/group
Authorization: Bearer nai_at_...
Content-Type: application/json

{"group":"..."}
```

服务端验证用户权限后更新当前 OAuth session 的 `selected_group`。它不修改用户账户默认分组，也不依赖 dashboard cookie。

TUI 选择流程必须保证 UI 一致性：

1. 用户选择分组。
2. 显示该组支持的模型。
3. 用户选择模型。
4. 先成功更新服务端 session group。
5. 再持久化本地模型与分组并更新当前会话。

若步骤 4 失败，不得先显示“模型已切换”；本地状态保持原值并显示可操作错误。

## 8. 设备管理、撤销和登出

网站新增“naicode 登录设备”列表，展示：

```text
设备名称
创建时间
最后使用时间
最近 IP（可脱敏）
当前分组
会话状态
```

用户可撤销单个 session；撤销立即设置 `revoked_at`，之后 access token 与 refresh token 都失效。也可“撤销全部设备”，但保留当前网页登录 session，除非用户明确退出网站。

CLI 登出使用 refresh token 定位会话，因此 access token 已过期时仍可撤销：

```http
POST /api/cli/oauth/revoke
Content-Type: application/x-www-form-urlencoded

client_id=naicode-cli&token=<refresh_token>&token_type_hint=refresh_token
```

撤销接口幂等；未知、已过期或已撤销 token 均返回成功，避免泄露状态。服务端按 refresh hash 撤销整个当前 session/family。客户端无论远端撤销成功、token 已失效还是服务端不可达，最终都清除本地凭据；若远端网络失败，需要明确告知该设备凭据可能仍有效，建议在网站撤销。

设备管理的状态变更接口必须使用现有网站会话的 CSRF 防护、SameSite cookie 与 Origin 校验；“撤销全部设备”要求近期重新认证。批量撤销在用户维度事务执行，并记录 `oauth_revoked_before` 水位，确保并发创建或刷新不能逃过撤销。

`device_name` 是不可信输入：限制为 1–80 个 UTF-8 字节、拒绝控制字符，并在 HTML、JSON 和审计日志中按上下文编码。

手动 API key 模式沿用原登出行为，不调用 OAuth revoke。

## 9. 客户端集成点

### 9.1 auth storage

`AuthDotJson` 增加可选的 relay OAuth token 数据，`auth_mode` 写为 `relay_oauth_tokens`。文件后端必须升级为真正的原子安全写入：同目录创建不跟随符号链接的临时文件，写入、flush、`fsync` 后原子 rename，并在支持的平台同步目录；Unix 权限为 `0600`，Windows ACL 仅授予当前用户。系统凭据库失败不得静默降级为明文文件。启动时应能识别并恢复“旧文件完整、新临时文件完整、任一文件损坏”的崩溃场景。

读取旧文件时：

- `ApiKey` 保持 API key 模式，不自动伪装为 OAuth。
- 现有由旧浏览器流程生成的 `sk-*` 无法安全自动换成 OAuth session；首次升级后提示重新进行浏览器登录。
- 成功 OAuth 登录覆盖当前 active auth，但保留行为遵循现有认证存储语义。

### 9.2 token provider 与 origin 绑定

新增 relay bearer provider，只注入：

```http
Authorization: Bearer <access_token>
```

Relay OAuth 凭据与内置 `newapi` provider 及规范化 origin `https://closedai.kylenqaq.com` 强绑定。只有请求目标经过 URL 规范化后满足 HTTPS、host 精确匹配、允许端口且路径位于 `/v1` 或明确的 CLI OAuth API 范围时，才可附加 bearer。HTTP 降级、跨 origin redirect、userinfo、同形域名和自定义 `base_url` 一律不得携带或转发 OAuth Authorization header。

用户自定义 `model_provider` / `model_providers` 仍可使用，但必须配置该 provider 自身的认证；选择自定义 provider 时不得复用 Relay OAuth token。不得注入 `ChatGPT-Account-ID`，不得使用 ChatGPT backend URL，也不得触发 OpenAI plan/rate-limit/connectors 逻辑。

### 9.3 AuthMode 传播语义

新增枚举后必须覆盖 protocol、app-server wire 类型、`AuthDotJson`、`CodexAuth`、Bearer provider、遥测字段和所有 exhaustive match。语义固定为：

- 属于已登录的 Relay 人类账号，可用于中转站状态展示与刷新。
- `has_chatgpt_account()`、`uses_codex_backend()` 及同类 OpenAI 判断均返回 false。
- 不启用 ChatGPT connectors、套餐限额、cloud task、OpenAI account headers 或 OpenAI refresh URL。
- 只有 Relay 专用分支可以读取和刷新 Relay OAuth token。

编译期必须跑 workspace 级 `cargo check`，行为测试覆盖所有上述边界。

### 9.4 状态与登录 UI

状态展示按真实 auth mode 分支：

- `RelayOAuthTokens`：`已登录酸奶中转站`，显示账号和当前设备/分组。
- `ApiKey`：`API 密钥模式`。
- OpenAI auth mode（若自定义场景仍可达）：保持其原有语义。

删除将 `ApiKey` 临时映射为已登录账号的兼容代码。

默认 onboarding 第一项为浏览器登录酸奶中转站；“手动填写 API key”保留为高级备用选项。

## 10. 安全要求

- 所有生产 OAuth 端点只通过 HTTPS 提供，唯一例外是客户端本机 loopback redirect。
- PKCE 仅允许 `S256`，禁止 `plain`。
- `state` 至少 256 bit 熵并严格比较。
- 授权页 callback 必须由服务端解析并按白名单规则验证，禁止字符串前缀判断。
- OAuth 响应和日志不得记录 code、access token、refresh token 或 Authorization header。
- OAuth 端点添加按 IP、用户和 session 的速率限制。
- 授权码兑换、refresh、group update 和 revoke 记录安全审计事件，但只保存 token 指纹。
- 数据库 token 哈希列不可通过普通管理 API 返回。
- access token 只允许模型请求及明确列出的 CLI 端点，不能访问用户管理、充值、令牌管理或管理员接口。
- 公告富文本继续执行消毒，OAuth UI 不渲染未消毒的远端 HTML。

## 11. 错误处理

服务端使用稳定、机器可判定的错误协议。OAuth token 相关 `/v1` 失败至少规定：

```text
401 + WWW-Authenticate: Bearer error="invalid_token", error_description="expired"
JSON error.code = access_token_expired

401 + JSON error.code = session_revoked
403 + JSON error.code = account_disabled
403 + JSON error.code = group_not_allowed
429 + JSON error.code = rate_limited
```

OAuth grant 端点使用标准风格错误：

```text
invalid_request
invalid_client
invalid_grant
access_denied
unsupported_grant_type
```

客户端只依据 HTTP 状态、`WWW-Authenticate` 和稳定 `error.code` 判断是否刷新，不解析中文 message 或任意响应文本。网络错误与凭据失效必须区分：网络暂时失败不清除 refresh token；确定的 `invalid_grant` 或撤销才清除并要求重新登录。

## 12. 测试策略

### 12.1 new-api 单元与集成测试

必须覆盖：

- loopback redirect 白名单，包括 IPv4、IPv6、恶意 host、userinfo、片段和编码绕过。
- PKCE 正确、错误 verifier、缺失参数、非 S256。
- 授权码单次消费、过期、并发双花和多实例数据库共享。
- access token 正常、过期、撤销、用户禁用，以及旧 access token 在刷新后持续有效至自身到期。
- refresh 成功轮换、90 天 idle expiry、365 天绝对期限、多实例并发、HTTP 响应丢失后的幂等恢复，以及 N-2/N-10 历史 token 重放后 family 撤销。绝对期限测试覆盖到期前最后一次刷新、恰好到期、并发跨过期点，以及到期后 retry cache 不得恢复 token pair。
- 设备单独撤销、过期 access 下使用 refresh token 登出、批量撤销与并发登录/refresh 竞态。
- 分组权限变化后的即时拒绝，以及 `UsingGroup`/`TokenGroup`/cross-group retry 上下文一致。
- OAuth `/v1/responses` 与 API key `/v1/responses` 的预扣、结算、退款、日志和限流上下文等价。
- OAuth token 无法访问 dashboard 管理接口；billing principal 不可见、不可认证、不可更新或删除。
- 敏感认证头在后续 header clone、表达式、诊断和日志前被清除。
- 授权码消费与 session 创建事务回滚，不产生孤立会话或已消费 code。
- `device_name` 的存储型 XSS、超长输入、控制字符、Unicode 和日志注入。
- 网站设备操作的 CSRF、Origin、近期重认证和用户级撤销水位。

### 12.2 Rust 单元与集成测试

必须覆盖：

- state 与 PKCE 校验。
- token 响应解析与原子持久化。
- 到期前主动刷新。
- 多并发请求只触发一次 refresh。
- 401 只刷新并重试一次。
- `invalid_grant` 清理登录态。
- relay bearer 仅发送到规范化的中转站 HTTPS origin；恶意自定义 `base_url`、HTTP 降级和跨 origin redirect 不携带 token。
- `RelayOAuthTokens` 在 app-server、遥测和所有 auth helper 中不触发任何 ChatGPT/OpenAI 专属逻辑。
- auth 文件原子写入、崩溃恢复、权限/ACL、符号链接防护，以及 keyring 失败不降级。
- `/model` 先更新远端分组，成功后才更新本地模型。
- 自定义 `model_provider` 仍按用户配置工作，并使用自身认证而非 Relay token。

### 12.3 端到端验收

在 staging 完成：

1. 新设备浏览器授权，CLI 不接触 `sk-*`。
2. 连续运行超过 15 分钟，模型请求自动刷新不中断。
3. 网站撤销当前设备，下一请求失效并要求重新登录。
4. 两台设备独立选择分组，互不覆盖。
5. `/model` 只显示所选分组支持的模型及正确价格。
6. 手动 API key 登录仍能请求模型，状态明确为 API key 模式。
7. OAuth 和 API key 请求的余额扣减、日志、退款行为一致。

## 13. 迁移与发布

分阶段发布，避免生产中断：

1. 服务端先部署数据库迁移、OAuth 端点和 bearer middleware；保留旧 `TokenAuth()`。
2. 在 staging 完成 OAuth、刷新、撤销、计费与回滚测试。
3. 发布支持 `RelayOAuthTokens` 的 naicode，但 OAuth 默认入口先由服务端 feature gate 控制；此时 N-1 服务端镜像也必须包含兼容 OAuth schema、bearer 验证和 refresh 端点，或保留经过验证的兼容回滚镜像。
4. 在双栈兼容窗口验证部署矩阵：旧客户端 + 新服务端、新客户端 + 新服务端、新客户端 + 兼容回滚服务端；任何组合都不得导致余额或凭据损坏。
5. 逐步开启 OAuth 默认入口。旧浏览器登录生成的 API key 仍按 API key 模式可用，但 UI 提示用户重新登录以启用设备管理与自动刷新。
6. 新客户端和兼容服务端稳定后移除旧 `/api/cli/authorize`、`/api/cli/token` 的 `sk-*` 兑换能力。
7. 最后清理客户端 `finalize_relay_session`、dashboard cookie state 和旧换组代码。

服务端部署必须先备份数据库与当前镜像；新 migration 只新增表和索引，不修改现有 token 表。普通旧镜像不理解 `nai_at_`，不能作为 OAuth 开启后的直接回滚目标。回滚必须使用兼容镜像或先关闭新登录、等待/迁移现有 OAuth 会话并明确通知，禁止静默中断 OAuth-only 客户端。

## 14. 实施边界

预计涉及：

- naicode：protocol auth enum、login storage/manager、relay OAuth client、bearer provider、core 401 刷新、TUI onboarding/status/model flow、相关测试。
- new-api：OAuth models/migration、controllers/services/middleware/routes、授权页、设备管理页、相关测试。

实施时按可独立回滚的功能提交：服务端 schema、授权码流程、refresh/session、bearer middleware、group/catalog、设备管理、Rust auth mode/storage、Rust OAuth/refresh、TUI 集成、迁移清理。每个提交保持可编译或明确标注仅服务端阶段，并在部署前执行完整联调。
