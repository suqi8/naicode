# naicode / new-api 产品化改造交接记录

> 日期：2026-07-10  
> 用途：在新对话中继续实现、验证、提交和部署。  
> 重要约束：**不要启动任何子代理、Agent、swarm、worker、autopilot 或 workflow；后续全部由主进程完成。**

---

## 1. 项目与仓库

### 1.1 naicode 客户端

```text
路径：D:\AndroidStudioProjects\naicode
分支：naicode
origin：https://github.com/suqi8/naicode.git
upstream：https://github.com/openai/codex.git
```

项目是 Codex CLI fork，产品名为 **naicode**，专用于“酸奶中转站”。默认 relay 地址：

```text
https://closedai.kylenqaq.com
```

### 1.2 new-api 服务端

```text
路径：C:\Users\pc\new-api-src
分支：integrate-rc20-codex-namespace
origin：https://github.com/QuantumNous/new-api.git
```

生产环境：

```text
服务器：8.212.180.123
SSH：root（已配置免密）
生产目录：/opt/new-api
容器名：new-api
镜像：new-api:invite-rebate
域名：https://closedai.kylenqaq.com
```

---

## 2. 用户授权与工作约束

用户已经明确授权：

- 可以修改 naicode 和 new-api；
- 可以创建 Git 提交并推送；
- 可以构建、发布 naicode；
- 可以部署 new-api 到生产；
- 部署前必须备份当前二进制和 Docker 镜像；
- 测试或运行验证失败时不能强行部署；
- 不得覆盖或回退已有未提交改动；
- 后续不得启动任何子代理，全部由主进程完成。

当前所有子代理均已停止。不要恢复旧代理。

---

## 3. 用户要求解决的问题

### 3.1 `/model` 完整价格

当前界面只显示：

```text
gpt-5.6-sol  输入 ¥0.20/1M · 输出 ¥1.60/1M
```

目标是让每个模型始终显示自己的完整价格，而不是只有选中模型展开，也不能只显示输入/输出。

价格通道包括：

- 输入；
- 输出；
- 缓存读取；
- 缓存创建 5 分钟；
- 缓存创建 1 小时；
- 图片输入；
- 音频输入；
- 音频输出；
- 按次价格；
- 货币；
- 计价单位；
- 动态计费标记。

### 3.2 客户端不得硬编码 `×2`

new-api 网页端基础公式为：

```text
base = model_ratio × 2 × group_ratio
```

`×2` 是 new-api 内部倍率基准，不能直接删除，否则价格会变成网页价格的一半。

已经确定的正确职责边界：

- new-api 服务端按真实计费规则计算最终结构化价格；
- `/api/cli/oauth/catalog` 返回数值、货币和单位；
- naicode 不再计算 `×2`，也不硬编码 `¥`；
- naicode 只格式化服务端返回的数据。

### 3.3 模型后选择思考等级

目标流程：

```text
/model
→ 获取 OAuth catalog
→ 选择分组
→ 选择模型并查看完整价格
→ 选择该模型支持的思考等级
→ 服务端换组
→ 保存模型和思考等级
→ 更新当前会话
→ 最后显示成功
```

### 3.4 修复模型与换组状态不一致

当前错误表现：

```text
模型已切换为 gpt-5.6-sol 默认
换组失败（尚未登录……）
```

当前旧实现连续发送：

```rust
RelaySwitchGroup
UpdateModel
PersistModelSelection
```

但换组只是异步启动，模型和配置会提前更新。必须改为有序、可补偿状态机。

### 3.5 修复 OAuth 登录态判断

新登录态是：

```rust
AuthMode::RelayOAuthTokens
```

旧 `RelayState::is_logged_in()` 只判断 cookie + user id，不能用于 OAuth catalog 和换组。

### 3.6 修复 loading 时出现“无匹配”

当前 loading 借用了空 `ListSelectionView`，通用空列表自动渲染“无匹配”。需要独立状态：

```text
Loading
Ready
Error
```

Loading 时不能显示“无匹配”。

### 3.7 修复 `Invalid token`

new-api 主 `/v1/*` 路由使用了：

```go
RelayOAuthOrTokenAuth()
```

但 `/v1/models` 原先单独使用 `TokenAuth()`，导致 `nai_at_...` 被当成普通 API key。该路由当前已经修改，详见服务端状态。

### 3.8 TUI 产品化

要求：

- 不能只是 Codex 换 API；
- 主界面不要常驻 topbar；
- 新对话开头使用更美观的终端符号 `NAICODE` Logo；
- 用户选定默认主题：**D · 深空蔚蓝**。

主题色：

```text
accent         #279CFF
accent_bright  #86CAFF
selected_bg    #0B2032
dark_bg        #07121D
```

轻量自定义配置：

```toml
[tui]
product_accent = "#279CFF"
```

现有 `[tui].theme` 仍只表示代码语法高亮 `.tmTheme`，不能与产品语义色混用。

---

## 4. 已批准文档

### 产品化规格

```text
D:\AndroidStudioProjects\naicode\docs\superpowers\specs\2026-07-10-naicode-product-tui-model-oauth-design.md
```

该规格经过多轮评审，最终结果为 `Approved`。

### OAuth 设计

```text
D:\AndroidStudioProjects\naicode\docs\superpowers\specs\2026-07-10-relay-oauth-design.md
```

### 实施计划

```text
C:\Users\pc\.claude\plans\typed-shimmying-patterson.md
```

---

## 5. new-api 服务端当前实现

### 5.1 已修改文件

```text
constant/context_key.go
controller/cli_oauth.go
controller/pricing.go
controller/relay.go
middleware/cli_oauth.go
model/model_meta.go
model/pricing.go
relay/helper/price.go
relay/helper/price_test.go
router/relay-router.go
service/group.go
```

新增：

```text
controller/cli_oauth_catalog.go
controller/cli_oauth_catalog_test.go
dto/cli_oauth_catalog.go
```

原有未跟踪项，不要删除或提交：

```text
.verify-tmpdir
new-api-custom-rc20
```

### 5.2 `/v1/models` OAuth 鉴权

文件：

```text
C:\Users\pc\new-api-src\router\relay-router.go
```

已将：

```go
modelsRouter.Use(middleware.TokenAuth())
```

改为：

```go
modelsRouter.Use(middleware.RelayOAuthOrTokenAuth())
```

覆盖：

```text
GET /v1/models
GET /v1/models/:model
```

普通 API key 仍由 dispatcher 回退到旧 `TokenAuth()`。

### 5.3 OAuth selected group context

新增：

```go
ContextKeyCliOAuthSessionId
ContextKeyCliOAuthSelectedGroup
```

涉及：

```text
constant/context_key.go
middleware/cli_oauth.go
controller/cli_oauth.go
```

`CliOAuthAccessAuth()` 会把同一次 access-token 验证得到的 session id 和 selected group 写入 Gin context。

### 5.4 严格 effective group ratio

涉及：

```text
service/group.go
relay/helper/price.go
controller/pricing.go
controller/relay.go
```

新增逻辑等价于：

```text
若 GroupGroupRatio[user.Group][targetGroup] 存在
→ 使用 special ratio
否则
→ 使用 GroupRatio[targetGroup]

目标组不存在
→ 返回错误，不静默按 1

显式 0
→ 保留为免费倍率
```

真实 relay 计费的 `HandleGroupRatio` 已复用该逻辑。

### 5.5 缓存创建倍率 helper

文件：

```text
relay/helper/price.go
```

新增：

```go
ResolveCacheCreationRatios(modelName)
```

规则：

```text
cache_create_5m_ratio = create_cache_ratio
cache_create_1h_ratio = create_cache_ratio × (6 / 3.75)
```

真实结算与 catalog 共用此 helper。

### 5.6 模型 1 小时缓存能力字段

文件：

```text
model/model_meta.go
model/pricing.go
```

新增：

```go
SupportsCacheCreation1h bool `json:"supports_cache_creation_1h"`
```

内部 `Pricing` 使用 `json:"-"` 隐藏该字段，避免改变公共 `/api/pricing` JSON 契约。更新路径已把 `supports_cache_creation_1h` 加入显式 `Select(...)`。

仍需确认：

- 管理前端是否能设置该字段；
- 生产已有模型默认值都为 `false`；
- 是否需要为现有 Claude 模型补初始化数据；
- 不得通过模型名猜测能力。

### 5.7 catalog 专用 DTO

文件：

```text
dto/cli_oauth_catalog.go
```

结构包括：

```text
CliOAuthCatalogDisplay
CliOAuthEffectivePrice
CliOAuthCatalogModel
CliOAuthCatalogResponse
```

价格通道：

```text
input
output
cache_read
cache_create_5m
cache_create_1h
image_input
audio_input
audio_output
request
preview
```

basis：

```text
per_million_tokens
per_request
dynamic_expression
```

### 5.8 catalog builder

文件：

```text
controller/cli_oauth_catalog.go
```

已实现：

- 用户可用组过滤；
- 排除 `auto`；
- 展开 `enable_groups=["all"]`；
- 每组使用 strict effective ratio；
- 普通倍率计费；
- 按次计费；
- tiered expression 安全返回 `dynamic_expression` + `preview: null`；
- USD/CNY/CUSTOM/TOKENS billing display；
- 模型和 catalog 稳定 SHA-256 版本。

### 5.9 已通过的服务端测试

主进程已执行：

```text
go -C "C:/Users/pc/new-api-src" test ./controller ./relay/helper ./middleware ./router
```

结果全部通过。

还执行：

```text
go -C "C:/Users/pc/new-api-src" test ./service -run "Test(PostText|CalculateText|TryTiered|BuildTiered)" -count=1
```

结果通过。

`git diff --check` 通过。

---

## 6. 服务端仍需主进程核对/修复

### 6.1 catalog 版本字段不一致

服务端返回：

```json
"pricing_version": "..."
```

当前 Rust parser 读取：

```rust
body.get("version")
```

这是明确契约 bug。应统一读取 `pricing_version`。

涉及：

```text
C:\Users\pc\new-api-src\dto\cli_oauth_catalog.go
D:\AndroidStudioProjects\naicode\codex-rs\login\src\relay\pricing.rs
```

### 6.2 服务端 catalog 未返回 `group_ratio` / `usable_group`

当前服务端专用响应只有：

```text
success
selected_group
display
data
pricing_version
```

Rust 仍依赖：

```text
group_ratio
usable_group
```

目前 Rust 虽从 `effective_prices` 推导组名，但 ratio 会退为 `0`，描述为空，排序可能错误。

推荐服务端补充：

```json
"group_ratio": {
  "default": 1.0
},
"usable_group": {
  "default": "默认分组"
}
```

`group_ratio` 必须是针对当前账户组算出的 effective ratio，而不是全局倍率。

### 6.3 selected group 必须验证仍可用

当前 builder 仅在空值时回退 `user.Group`，还需确认：

- session selected group 仍在用户可用组；
- selected group 有有效倍率；
- 不得返回一个 UI 中不存在的 selected group。

### 6.4 公共 `/api/pricing` 契约快照

实际验证其 JSON 不应出现：

```text
supports_cache_creation_1h
effective_prices
display
selected_group
```

### 6.5 CUSTOM 汇率

再次确认：

```go
operation_setting.GetUsdToCurrencyRate(operation_setting.USDExchangeRate)
```

在 CUSTOM 模式下确实使用 `CustomCurrencyExchangeRate`。最好保留明确测试。

### 6.6 真实 router middleware 测试

需要新增或确认真实 Gin router 测试，使用真实签发的 OAuth access token请求：

```text
GET /v1/models
GET /v1/models/:model
```

不能只直接调用 controller。

### 6.7 model 测试

能力字段涉及 migration/CRUD，上线前运行相关 `model` 目标测试。

---

## 7. naicode OAuth 请求层当前状态

### 7.1 主要修改文件

```text
codex-rs/login/src/auth/manager.rs
codex-rs/login/src/relay/api.rs
codex-rs/login/src/relay/mod.rs
codex-rs/login/src/relay/pricing.rs
codex-rs/login/src/lib.rs
```

可能还包括：

```text
codex-rs/login/src/auth/auth_tests.rs
codex-rs/login/src/auth/storage_tests.rs
codex-rs/login/tests/suite/auth_refresh.rs
codex-rs/login/tests/suite/logout.rs
```

### 7.2 已落盘的意图

当前源码可见：

- 新增 `RelayRequestError`；
- 新增 `AuthManager::execute_relay_request(...)`；
- Relay OAuth snapshot equality；
- catalog 使用 `AuthManager`；
- 401 后 reload/refresh/retry；
- OAuth catalog 不匿名降级；
- OAuth 换组使用统一 executor；
- 旧 cookie/API-key 路径保留；
- Rust DTO 增加全部结构化价格通道；
- 新增 `format_price_value()`；
- 新展示路径不再计算 `×2` 或硬编码人民币。

### 7.3 中断时状态

代理在停止前报告：

```text
格式化已完成
正在运行 codex-login 全部测试
准备运行 cargo check -p codex-login
```

最终测试结果未知。必须由主进程重新执行和修复。

### 7.4 已知 Rust catalog bug

当前读取：

```rust
body.get("version")
```

应改为：

```rust
body.get("pricing_version")
```

### 7.5 groups 逻辑不匹配

当前服务端未返回 `group_ratio` 和 `usable_group`；Rust `groups()` 会把 ratio 默认成 `0`。需与服务端契约一起修复。

### 7.6 `models_in_group()` 应收紧

当前：

```rust
model.effective_prices.contains_key(group)
    || model.enable_groups.iter().any(|enabled| enabled == group)
```

OAuth catalog 应只依据 `effective_prices`；旧匿名 `/api/pricing` 才使用 `enable_groups` fallback。

---

## 8. 产品主题与欢迎区当前状态

### 8.1 新增文件

```text
D:\AndroidStudioProjects\naicode\codex-rs\tui\src\product_palette.rs
```

### 8.2 已修改配置链

```text
codex-rs/config/src/types.rs
codex-rs/core/src/config/mod.rs
codex-rs/core/src/config/edit.rs
codex-rs/core/config.schema.json
codex-rs/core/src/config/config_tests.rs
codex-rs/core/src/config/edit_tests.rs
```

目标配置：

```toml
[tui]
product_accent = "#279CFF"
```

### 8.3 palette 已实现内容

`ProductPalette` 包含：

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
dark_background
```

支持：

- 默认深空蔚蓝；
- `#RRGGBB`；
- HSL 派生；
- WCAG 对比度选择；
- truecolor；
- ANSI-256；
- ANSI-16 fallback：Blue/Cyan/DarkGray。

### 8.4 集中样式

```text
codex-rs/tui/src/style.rs
```

`accent_style()` 已开始使用 product palette。

### 8.5 欢迎区

涉及：

```text
codex-rs/tui/src/history_cell/session.rs
codex-rs/tui/src/history_cell/tests.rs
```

目标：

- 静态 `NAICODE` 符号 Logo；
- 酸奶中转站；
- 当前 model/effort/cwd；
- history cell 随对话滚走；
- 不增加常驻 topbar。

### 8.6 已知未完成问题

当前 `/clear` 也会重复显示大 Logo。必须改成：

- fresh session：显示大 Logo；
- resume/fork/replay：不重复；
- `/clear`：只显示紧凑摘要。

主题相关最终测试和 `cargo check` 尚未确认。

---

## 9. TUI picker 与原子切换尚未实现

任务 #48 尚未完成。

主要旧代码：

```text
codex-rs/tui/src/chatwidget/model_popups.rs
codex-rs/tui/src/app_event.rs
codex-rs/tui/src/app/event_dispatch.rs
codex-rs/tui/src/app/thread_settings.rs
codex-rs/tui/src/config_update.rs
```

### 9.1 专用 Relay picker

建议新增：

```text
codex-rs/tui/src/bottom_pane/relay_model_picker.rs
```

要求：

- `Loading / Ready / Error`；
- loading 不显示“无匹配”；
- 同一 view 选择分组和模型；
- 每个模型始终显示全部价格；
- 切组保留搜索；
- 搜索只过滤当前组；
- 整张模型卡滚动可见；
- 宽度布局：`>=96` 四列、`72..95` 两列、`<72` 单列。

### 9.2 无副作用 reasoning selector

现有 reasoning popup 会直接 Update/Persist。需拆成：

```text
选择 UI → callback 返回 ReasoningEffort
```

标准模型 callback 继续即时应用；Relay callback 只创建：

```rust
PendingRelayModelSelection {
    group,
    model,
    reasoning_effort,
}
```

### 9.3 一致切换状态机

正确顺序：

```text
远端换组
→ 原子保存本地 model/effort
→ 合并更新当前 thread model/effort
→ 等待 thread/settings/updated
→ 写 relay.json group
→ 显示成功
```

失败时逆序补偿。

### 9.4 合并 thread settings API

使用一个：

```rust
ThreadSettingsUpdateParams {
    model,
    effort,
    collaboration_mode,
}
```

请求，并返回 `Result<ThreadSettings>`，等待和验证：

```text
thread/settings/updated
```

不能继续使用会吞错的旧方法。

### 9.5 拆分换组与缓存写入

当前 `relay_switch_group()` 在远端成功后立即写 `relay.json`。需拆分：

```text
relay_switch_group_remote()
commit_relay_group_cache()
```

---

## 10. Git 工作区与提交注意事项

### 10.1 naicode 工作区很脏

现有大量未提交修改，包括此前 OAuth、中文化、产品文案、app-server、远程控制以及本轮改造。

禁止直接使用：

```text
git add .
git add -A
```

应按明确文件分批暂存和提交。

建议提交分组：

1. 既有 OAuth/中文化基线；
2. OAuth catalog 请求层；
3. TUI picker 与原子切换；
4. 产品主题与欢迎区。

每个提交末尾：

```text
Co-Authored-By: Claude <noreply@anthropic.com>
```

### 10.2 new-api 不要提交

```text
.verify-tmpdir
new-api-custom-rc20
```

发布二进制通常不应进入源码提交。

---

## 11. 生产部署机制

生产 Dockerfile：

```dockerfile
FROM calciumion/new-api:latest
COPY new-api-custom /new-api
```

路径：

```text
/opt/new-api/Dockerfile.custom
```

本地交叉编译必须使用 Bash：

```bash
CGO_ENABLED=0 GOOS=linux GOARCH=amd64 \
go build \
  -ldflags "-s -w -X 'github.com/QuantumNous/new-api/common.Version=v1.0.0-rc.19'" \
  -o new-api-custom .
```

上传：

```bash
gzip -c new-api-custom |
ssh root@8.212.180.123 \
  "gunzip > /opt/new-api/new-api-custom.new && chmod +x /opt/new-api/new-api-custom.new"
```

核对本地/远端字节数后原子替换。

部署前备份：

```bash
cp /opt/new-api/new-api-custom \
   /opt/new-api/new-api-custom.bak.$(date +%Y%m%d-%H%M%S)

docker tag new-api:invite-rebate \
  new-api:invite-rebate-bak-$(date +%Y%m%d-%H%M%S)
```

构建和重启：

```bash
cd /opt/new-api
docker build -f Dockerfile.custom -t new-api:invite-rebate .
docker compose up -d --force-recreate
```

生产验证：

```text
/api/status = 200
/api/cli/oauth/catalog 非 404
/v1/models 使用 nai_at token 不返回 Invalid token
容器 healthy
真实模型请求成功
```

生产有真实余额和付费用户，重启会断流几十秒。失败必须回滚。

---

## 12. naicode 版本和发布

正式版格式：

```text
v<年两位>.<月>.<日>.<当日序号>
```

2026-07-10 当天首个正式版示例：

```text
v26.7.10.0
```

功能验证通过后再同步：

```text
codex-rs/Cargo.toml
codex-cli/package.json
其他发布版本字段
Git tag
```

当前二进制产物仍叫 `codex.exe`，因为 `[[bin]] name = "codex"`。本地过渡安装：

```bash
cp -f target/debug/codex.exe "$HOME/.cargo/bin/naicode.exe"
```

`[[bin]] codex → naicode` 改名任务仍延后，不要顺带实现。

---

## 13. 建议继续顺序

### 阶段 1：确认状态

1. 不启动任何代理；
2. 查看两个仓库 `git status` 和实际 diff；
3. 清理异常代理创建的过细任务，但不要删除代码；
4. 只由主进程工作。

### 阶段 2：修复服务端/Rust catalog 契约

优先完成：

1. `pricing_version` 字段一致；
2. 服务端 catalog 返回 effective `group_ratio` 和 `usable_group`；
3. selected group 权限与存在性验证；
4. Rust groups 倍率和描述解析；
5. OAuth catalog 模型只依据 `effective_prices`；
6. 真实 `/v1/models` router + OAuth 测试；
7. model capability migration/CRUD 测试。

### 阶段 3：完成 OAuth 请求层验证

运行：

```text
cargo test -p codex-login
cargo check -p codex-login
```

若全量太慢，先运行 Relay OAuth、pricing 和 group switching 目标测试。

### 阶段 4：完成主题欢迎区

1. 修复 `/clear` 重复 Logo；
2. 确认 palette 初始化入口；
3. 非法颜色只警告一次；
4. 运行 config/palette/history tests；
5. `cargo check -p codex-tui`。

### 阶段 5：实现 picker 与一致切换

全部由主进程完成。

### 阶段 6：确定性测试

服务端：

```text
go -C "C:/Users/pc/new-api-src" test ./controller ./relay/helper ./middleware ./router
go -C "C:/Users/pc/new-api-src" test ./service -run "Test(PostText|CalculateText|TryTiered|BuildTiered)" -count=1
```

客户端：

```text
codex-login 目标测试
codex-tui picker tests
thread settings transaction tests
config/palette/history tests
cargo check 受影响 crates
```

### 阶段 7：一次真实冒烟

```text
本地测试 new-api
→ 浏览器 OAuth
→ auth.json RelayOAuthTokens
→ /model 完整价格
→ reasoning 选择
→ 换组/模型/thread 一致
→ /v1/models
→ 实际模型请求
→ token refresh
→ device revoke
→ 深空蔚蓝
→ 新会话 Logo
→ 无常驻 topbar
→ 70 列窄屏
```

### 阶段 8：主进程手工审查

不要调用多代理 code-review 流程。

### 阶段 9：分批提交和推送

禁止 `git add .`，只暂存明确文件。

### 阶段 10：生产部署

只有测试、编译、冒烟和审查均通过后：

```text
备份二进制
→ 备份镜像
→ 交叉编译
→ 上传并核对大小
→ 构建镜像
→ 重启容器
→ 健康检查
→ catalog 验证
→ /v1/models 验证
→ 真实请求验证
```

---

## 14. 新对话建议开场提示

```text
继续完成 naicode/new-api 产品化改造。不要启动任何子代理、Agent、swarm、worker、autopilot 或 workflow，全部由主进程自己处理。

先阅读：
1. D:\AndroidStudioProjects\naicode\docs\superpowers\specs\2026-07-10-naicode-productization-handoff.md
2. D:\AndroidStudioProjects\naicode\docs\superpowers\specs\2026-07-10-naicode-product-tui-model-oauth-design.md
3. C:\Users\pc\.claude\plans\typed-shimmying-patterson.md
4. 两个仓库的 git status 和实际 diff

仓库：
- naicode: D:\AndroidStudioProjects\naicode
- new-api: C:\Users\pc\new-api-src

先从修复 catalog 服务端/Rust 契约和完成 codex-login 编译测试开始。不要恢复旧代理，不要删除已有未提交改动，不要使用 git add .。
```
