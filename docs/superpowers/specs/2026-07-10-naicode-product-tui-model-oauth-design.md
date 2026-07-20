# naicode 产品化 TUI、模型目录与 OAuth 修复设计

## 1. 目标

本次改造把 naicode 从“Codex 更换中转 API”提升为酸奶中转站的独立终端产品，同时修复当前真实使用中暴露的价格、模型选择和 OAuth 鉴权问题。

交付目标：

- `/model` 先选分组，再展示该组全部模型及每个模型的完整价格。
- 价格由 new-api 返回最终结构化结果，naicode 不硬编码 `×2`、人民币符号或计价单位。
- 选完模型后继续选择该模型支持的思考等级。
- 换组、模型和思考等级作为一个一致的切换流程；换组失败时不改变本地状态。
- 修复 OAuth 登录后仍提示“尚未登录”、加载弹层出现“无匹配”以及模型请求返回 `Invalid token`。
- 新对话欢迎区、弹层、状态、错误和颜色形成酸奶中转站独立视觉语言。
- 默认主题采用“深空蔚蓝”，并以较小改动支持用户自定义主色。

本设计扩展既有 `2026-07-10-relay-oauth-design.md`，不替换其中的 OAuth 安全与生命周期约束。

## 2. 非目标

- 不移除手动 API key 和用户自定义 provider。
- 不让 catalog 成为真实结算来源；实际扣费继续使用 new-api 既有 relay/settle 链。
- 不在第一版提供完整主题编辑器、主题市场或任意组件级配色。
- 不在主界面加入常驻 topbar 或固定侧栏。
- 不重写整个 Ratatui 组件库；优先建立语义色和可复用状态组件。
- 不在本次工作中提交、发布或部署生产环境。

## 3. 产品与视觉方向

### 3.1 主界面

主界面继续以对话和编码任务为中心，不增加常驻顶栏。品牌、当前模型和目录只在新会话欢迎区出现；用户开始对话后，欢迎区随历史自然滚出视野。

新会话欢迎区包含：

- 由终端块字符组成的 `NAICODE` 标志；
- “酸奶中转站”品牌说明；
- 当前模型、分组和思考等级；
- 当前工作目录；
- `/model` 等关键入口提示。

底部输入区保持紧凑，不重复展示大段账户或连接信息。余额、设备与完整认证状态通过 `/status`、`/account` 或错误提示按需出现。

### 3.2 默认主题

默认主题为“深空蔚蓝”：

```text
accent         #279CFF
accent_bright  #86CAFF
selected_bg    #0B2032
dark_bg        #07121D
```

实现时不应在各组件散落这些具体值，而是新增少量语义颜色：

```text
accent
accent_bright
selection_background
selection_foreground
border_focused
border_muted
status_success
status_warning
status_error
```

产品语义色与现有 `[tui].theme`（代码语法高亮 `.tmTheme`）是两个独立概念，后者保持原义。新增配置键：

```toml
[tui]
product_accent = "#279CFF"
```

配置优先级为命令行/运行时预览（若将来提供）＞ `product_accent` ＞内置深空蔚蓝。用户只配置一个 `#RRGGBB` 主色；客户端将其转换为 HSL，保持 hue，将 bright 的 lightness 提升 18 个百分点、selection background 的 lightness 固定为 14% 且 saturation 不低于 45%，focused border 使用原主色。文本与背景按 WCAG 对比度选择黑/白前景：普通文本至少 4.5:1，大字号/装饰性 Logo 至少 3:1；无法满足或解析失败时回退默认主题并显示一次非阻断警告。

真彩色终端使用 RGB；256 色终端按 xterm-256 最近欧氏 RGB 色映射；ANSI-16 终端按最近基础色映射，深空蔚蓝的 accent/bright/selection 分别回退为 `Blue`、`Cyan`、`DarkGray`。

### 3.3 模型列表

模型弹层保持“左侧分组、右侧模型”结构，但每个模型始终显示自身完整价格；选中状态只改变高亮，不决定价格是否可见。

每个模型按服务端实际返回情况展示：

- 输入；
- 输出；
- 缓存读取；
- 缓存创建；
- 图片输入；
- 音频输入；
- 音频输出；
- 按次；
- 货币与计价单位。

不存在的价格通道显示 `—`，不使用推测值。布局按弹层内容宽度确定：`>= 96` 列时左侧 18 列分组、右侧模型价格四列；`72..95` 列时保留左侧分组、价格折为两列；`< 72` 列时分组改为顶部单行当前组，价格改为单列。模型名先占一整行，超出可用宽度时中间省略并保留首尾；价格标签不截断。列表使用垂直滚动，焦点移动时保证整条模型（含其价格行）可见；搜索只过滤当前组模型，切组后保留查询，若无结果才显示“无匹配”。

### 3.4 加载与错误状态

“正在获取分组与价格”使用独立 loading view，不再借用空的列表选择器，因此加载期间不显示“无匹配”。

错误状态至少区分：

- 尚未登录；
- access token 已过期且刷新失败；
- 设备会话已撤销；
- 网络失败；
- 无可用分组；
- 所选分组无模型；
- 服务端换组失败；
- 本地配置保存失败。

错误信息提供明确下一步，但不得把所有错误都归结为“请重新登录”。

## 4. 服务端最终价格目录

### 4.1 设计原则

new-api 是可用模型、分组权限、倍率、系统货币和价格规则的权威来源。`/api/cli/oauth/catalog` 返回当前 OAuth 会话可直接展示的结构化价格，naicode 不再复制网页的 `model_ratio × 2 × group_ratio` 规则。

catalog 同时保留现有原始倍率字段，以兼容网页与诊断场景；新增的最终价格只用于展示和预估，不取代结算。

### 4.2 响应元数据

catalog 顶层增加：

```json
{
  "selected_group": "default",
  "display": {
    "kind": "currency",
    "currency_code": "CNY",
    "currency_symbol": "¥",
    "token_unit": "1M tokens",
    "quota_display_type": "CNY",
    "pricing_mode": "billing"
  }
}
```

`selected_group` 和价格必须使用同一次 `CliOAuthAccessAuth` 产生的 access context snapshot，不能重新退回账户默认组。session 组为空时，由服务端在该 snapshot 中解析用户默认可用组。`enable_groups: ["all"]` 在 catalog 中展开为该用户所有具体可用组，并为每组生成 `effective_prices`。

模型目录固定展示**标准计费标价**，与网页价格库的 billing display 语义一致，不默认展示“充值价”开关后的促销成本。转换契约为：

- `USD`：`kind=currency`、`currency_code=USD`、符号 `$`、数值 `usd_price`；
- `CNY`：`kind=currency`、`currency_code=CNY`、符号 `¥`、数值 `usd_price × usd_exchange_rate`；
- `CUSTOM`：`kind=custom`、`currency_code=null`、返回管理员自定义符号，数值 `usd_price × custom_currency_exchange_rate`；
- `TOKENS`：模型定价按 `formatBillingCurrencyFromUSD` 的现有规则回退 USD，即 `$` 和 `usd_price`，不显示额度 token。

token 价格统一以 `1M tokens` 返回；按次价格单位为 `request`。数值 API 保留未格式化的有限非负十进制，TUI 按绝对值 `>=1` 最多 4 位小数、`<1` 最多 6 位小数、去尾零，不使用科学计数法；服务端同时返回 display metadata，但不返回已格式化字符串。

### 4.3 每组每模型价格

同一模型在不同分组价格不同，因此最终价格必须按分组表达，而不能只在模型上返回单个最低价。推荐结构：

```json
{
  "model_name": "gpt-5.6-sol",
  "enable_groups": ["default", "vip"],
  "effective_prices": {
    "default": {
      "group_ratio": 1.0,
      "basis": "per_million_tokens",
      "currency_code": "CNY",
      "currency_symbol": "¥",
      "input": 0.20,
      "output": 1.60,
      "cache_read": 0.02,
      "cache_create_5m": 0.25,
      "cache_create_1h": 0.40,
      "image_input": null,
      "audio_input": null,
      "audio_output": null,
      "request": null
    }
  }
}
```

普通 token 模型的服务端计算继续复用网站规则：

```text
base          = model_ratio × 2 × group_ratio
input         = base
output        = base × completion_ratio
cache_read       = base × cache_ratio
cache_create_5m  = base × create_cache_ratio
cache_create_1h  = base × create_cache_ratio × (6 / 3.75)，但仅对具备 Claude 1 小时 prompt caching 能力的模型返回
image_input      = base × image_ratio
audio_input   = base × audio_ratio
audio_output  = base × audio_ratio × audio_completion_ratio
```

按次模型：

```text
request = model_price × group_ratio
basis   = per_request
```

普通 ratio 模式的 5 分钟与 1 小时缓存创建必须复用 `relay/helper/price.go` 的同一派生 helper/常量，禁止在 catalog 复制另一个乘数。模型是否具备 1 小时缓存通道由服务端模型能力元数据决定；仅名称猜测不足以开启该通道。动态 `billingexpr` 模式分别以表达式实际使用的 `cc` 与 `cc1h` 为准。

`effective_prices[target_group].group_ratio` 是当前 OAuth 用户的账户组 `user.Group` 到目标使用组 `target_group` 的最终有效倍率：先查询 `GetGroupGroupRatio(user.Group, target_group)`，存在则使用 special ratio，否则使用 `GetGroupRatio(target_group)`；该优先级必须与 `HandleGroupRatio` 一致。OAuth session 的 `selected_group` 只是当前目标组，不得代替账户组参与 special ratio 查询。

最终数值经过上述 billing display 货币规则转换后返回，并明确附带 currency 与 basis。价格构造必须从原始 ratio map 的键存在性读取通道：键不存在才使用该通道明确定义的默认或返回 `null`；显式 `0` 必须保留为免费，不得用 `|| 1` 或 truthy 判断替换。组倍率同样通过 `(value, exists)` 读取，缺失组为服务端数据错误，不得静默按 1 计价。

### 4.4 动态计费

动态/阶梯模型不能伪装为固定价格。catalog 保留 `billing_mode`、`billing_expr` 和 `pricing_version`，并将最终价格标为：

```json
{
  "basis": "dynamic_expression",
  "preview": { }
}
```

只有服务端通过 billingexpr AST 证明表达式是仅由静态 tier 边界和常量单位系数组成、不依赖 `param()`、`header()`、时间或其他请求上下文时，才可返回结构化 tier preview；preview 必须带每档边界、使用字段、组倍率和表达式版本。其余表达式只返回 `basis=dynamic_expression` 与人类可读的“动态计费”，不得伪造首档。不得在 Rust 客户端重新实现 billing expression。

每个模型的 `pricing_version` 是其展示契约版本：由规范化后的计费模式、表达式、所有 ratio/price 通道、可用组及有效组倍率计算稳定哈希；任一输入变化都必须改变版本。顶层 catalog version 则由排序后的模型版本和 display metadata 计算，用于缓存失效，不使用硬编码常量。

## 5. OAuth 鉴权修复

### 5.1 统一登录态

Relay OAuth 的登录状态统一定义为：

```text
AuthMode::RelayOAuthTokens
且 relay_oauth 凭据完整
```

`RelayState::is_logged_in()` 的旧 cookie/session 语义不得用于 `/model`、catalog 或换组。`relay.json` 继续只保存非敏感缓存。

### 5.2 catalog 认证与刷新

`/model` 不再调用匿名 `fetch_pricing()`，改为通过 `AuthManager` 获取有效 Relay OAuth 凭据后调用 `/api/cli/oauth/catalog`。

该请求必须具备与模型请求一致的行为，并统一进入同一个 Relay 授权请求执行器：

1. 以 OAuth session 为键获取进程内 singleflight 锁；
2. 锁内重新从实际 storage backend（文件或 keyring）加载凭据；
3. 若 session/access/refresh/expires_at 相比发起请求时的 snapshot 已变化，直接用新 access token 重试，不再次刷新；
4. 只有 snapshot 未变化且 token 临近过期或请求返回 401 时才调用 refresh；
5. refresh token 轮换结果先原子持久化，再更新内存 snapshot；
6. 原请求最多重试一次，防止认证失败循环；
7. refresh 永久失效时返回可识别的“需重新登录”错误；
8. 不允许静默回退匿名价格目录，因为那会暴露用户不可用分组并产生错误价格。

catalog、换组、`/v1/models` 和模型请求共用这套 reload/refresh/retry 语义；直接 `reqwest` 读取磁盘 token 的旁路应移除。`auths_equal_for_refresh` 增加 Relay OAuth 比较，至少比较 session、access token、refresh token 和 expires_at。

### 5.3 `/v1/models` 鉴权

new-api 的 `/v1/models` 当前单独使用旧 `TokenAuth()`，会把 `nai_at_...` 当普通 API key 并返回 `Invalid token`。该路由改用 `RelayOAuthOrTokenAuth()`，与 `/v1/responses` 等 relay 路由保持一致。

OAuth access token 必须始终放在标准 `Authorization` header。服务端 OAuth 路径继续移除下游敏感认证头并重建既有用户、分组和计费上下文。

## 6. 原子模型选择流程

### 6.1 用户流程

```text
打开 /model
→ 获取授权 catalog
→ 选择分组
→ 查看该组全部模型和完整价格
→ 选择模型
→ 选择该模型支持的思考等级
→ 服务端切换 OAuth session 分组
→ 本地保存模型与思考等级
→ 更新当前会话
→ 显示成功
```

模型能力优先从现有 `ModelCatalog`/`ModelPreset` 获取，并复用等级文案与默认等级解析，但把现有思考等级弹层重构为**无副作用选择器**：弹层只通过 callback/action 返回 `ReasoningEffort`，不得直接发送 `UpdateModel`、`UpdateReasoningEffort` 或 `PersistModelSelection`。标准模型流程可在 callback 中继续执行原有立即应用；Relay 流程则只更新 `PendingRelayModelSelection`。

Plan 模式下，Relay `/model` 选择始终修改全局默认模型与全局 reasoning effort，不写 Plan override；Plan 专属等级仍通过原有 Plan 设置入口修改，避免一次 Relay 换组产生两个配置作用域。

如果模型只有一个支持等级，可直接进入应用步骤，但界面仍应明确展示将使用的等级。如果 catalog 找不到模型能力，使用当前模型管理层的安全默认值并明确标注“默认”，不得沿用上一个模型不受支持的等级。

### 6.2 待应用状态

新增一个明确的待应用选择状态，例如：

```rust
PendingRelayModelSelection {
    group,
    model,
    reasoning_effort,
}
```

选择模型和等级时只构造该状态，不发送 `UpdateModel` 或 `PersistModelSelection`。`RelaySwitchGroup` 成功事件携带待应用选择，成功后再进入本地应用。

### 6.3 一致性边界

服务端换组必须先成功，避免当前“模型先保存、换组稍后失败”的错误。

服务端与本地无法组成分布式事务，因此切换前创建完整 snapshot：旧远端 session group、旧 `relay.json` group 缓存、旧持久化 model/effort 和旧当前会话 model/effort。执行顺序与恢复规则为：

1. 服务端切到目标分组，但暂不写 `relay.json`；
2. 原子批量写入本地 model/effort；
3. 通过一个合并的 `ThreadSettingsUpdateParams { model, effort, collaboration_mode }` 请求更新当前会话，并等待匹配 thread id 的 `thread/settings/updated` 通知；协调器使用一个返回 `Result<ThreadSettings>` 的 API，验证通知中的最终 model/effort 与目标一致；
4. 最后原子写入 `relay.json` group 缓存并显示成功。

现有 `send_thread_settings_update` 吞错的行为不适用于该事务：新增可失败的底层方法，由普通设置调用者决定是否转成 UI 消息，事务协调器必须收到错误。恢复旧会话状态也通过同一个合并 API，并再次等待/验证 `thread/settings/updated`，不得拆成两个请求。

任一步失败即按逆序恢复已改变的状态：会话恢复旧 model/effort、本地配置恢复旧值、远端组恢复旧组、`relay.json` 恢复旧缓存。每个恢复动作独立记录结果；全部恢复成功时提示“切换失败，已恢复原状态”。任一恢复失败时进入 `RelaySelectionInconsistent` 状态，禁止显示成功，并展示远端组、本地默认和当前会话各自实际值以及“重新同步”操作。重新同步以服务端实际 group 为权威，让用户重新选择模型/等级后再次执行流程。

`relay_switch_group` 拆分为纯远端切换与显式缓存提交，避免当前函数在远端成功后立即写 `relay.json`。

## 7. 组件边界

### new-api

- `middleware/cli_oauth.go`：统一 `/v1` OAuth 或 API key 鉴权，补齐 models 路由使用。
- `controller/pricing.go`：保留公共 pricing 原料，并构造结构化最终价格。
- `controller/cli_oauth.go`：catalog 注入 OAuth session 的 selected group；换组保持权限校验。
- 现有 operation/ratio settings：继续作为货币和倍率真值。
- `pkg/billingexpr`：动态计费预览如需扩展，必须遵循 `pkg/billingexpr/expr.md`。

### naicode

- `login/src/relay/pricing.rs`：扩展 DTO，删除客户端 `×2` 和固定人民币格式化；使用授权 catalog。
- `login/src/auth/manager.rs`：提供可复用的有效 Relay bearer/授权请求路径，补齐刷新快照比较。
- `tui/src/chatwidget/model_popups.rs`：分组/模型完整价格、思考等级衔接和窄屏布局。
- `tui/src/app_event.rs` 与 `tui/src/app/event_dispatch.rs`：待应用状态、换组 continuation、补偿和最终成功事件。
- `tui` 主题相关模块：语义色、深空蔚蓝默认值和单主色自定义。
- 新会话欢迎组件：符号 Logo 与会话摘要，不引入常驻 topbar。

## 8. 验证策略

验证分为确定性自动测试与一次真实运行验收。实现期间只运行受影响包的目标测试/编译，避免频繁全量构建；最终只进行一次浏览器与 TUI 真实流程。

确定性自动测试必须覆盖：

- 经过真实 Gin router + `RelayOAuthOrTokenAuth` 的 `GET /v1/models` 与 `GET /v1/models/:model`；
- USD/CNY/TOKENS/CUSTOM、special group ratio、显式零、缺失通道、5m/1h cache create、按次与动态表达式价格 DTO；
- 凭据 snapshot 未变化时 refresh、已变化时仅 reload、并发 catalog/模型请求 singleflight 只轮换一次；
- 远端换组、本地配置写、合并 thread settings 更新、`relay.json` 提交各阶段失败，以及逆序补偿成功/失败；
- 96/72 列布局边界、搜索过滤、整条模型滚动可见性与模型名中间省略。

最终运行验收只覆盖一条代表性冒烟流程：

1. 启动测试 new-api，使用一个具有普通价格、缓存价格和两个可用组的测试账号；
2. 本地构建并安装 naicode，浏览器完成 OAuth 授权；
3. 检查 `auth.json` 使用 `RelayOAuthTokens` 且不保存 `sk-*`；
4. `/model` 加载授权 catalog，无“无匹配”闪现，每个模型显示自身完整价格；
5. 选择一个模型及其思考等级，确认换组、默认配置和当前会话一致；
6. 在 70 列代表性窄终端中确认折行、搜索、滚动和键盘焦点可用；
7. 实际执行 `GET /v1/models` 和一次模型生成请求，不再返回旧式 `Invalid token`；
8. 让 access token 进入临近过期状态，观察一次自动刷新后请求成功；
9. 撤销测试设备，确认下一次请求被拒绝并明确提示重新登录；
10. 确认默认深空蔚蓝、新会话 Logo、无常驻 topbar，并设置一个合法 `product_accent` 后重启确认生效。

并发 singleflight、四种 display mode、故障注入补偿、全部宽度边界、低对比度和 ANSI 降级只由上方确定性自动测试覆盖，不在人工冒烟中重复制造。

不提交、不推送、不部署生产，除非用户后续明确授权。

## 9. 验收标准

- naicode TUI 不再显示或解释客户端内部 `×2` 定价公式。
- `/model` 中所有模型均显示服务端提供的完整可用价格通道。
- 价格的货币、单位、分组和数值与 new-api catalog 一致。
- 模型选择后必然进入思考等级选择或明确的单等级确认。
- 换组失败不会改变当前模型、思考等级或默认配置。
- OAuth 登录态不再依赖旧 cookie；catalog 和模型请求都支持自动刷新。
- `/v1/models` 接受 Relay OAuth bearer，不再出现旧式 `Invalid token`。
- loading、空列表与错误状态彼此独立。
- 新对话拥有 NAICODE 符号 Logo，主界面没有常驻 topbar。
- 默认主题为深空蔚蓝，用户可以用一个主色完成安全的轻量自定义。
