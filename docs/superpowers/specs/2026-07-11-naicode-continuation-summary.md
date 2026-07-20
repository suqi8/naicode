# naicode / new-api 产品化改造交接总结

日期：2026-07-11

本文件用于新对话接手当前工作。核心背景：本轮已经完成并部署了 new-api 生产端一部分 OAuth/catalog/定价改造，也本地编译安装过 naicode，但用户现场验证后仍发现 TUI 视觉、Logo、思考等级选择、每轮余额/消耗展示，以及 `codex-code-mode-host.exe` 缺失等问题，需要继续修。

## 1. 用户当前明确要求

- 继续完成 naicode/new-api 改造。
- 可以启动子代理，但之前出现过子代理/审查代理失控消耗大量 token；后续应谨慎，优先主进程自己处理。
- 用户明确说过“不要 push / 不要推送”，所以不要推送远端，除非用户再次明确授权。
- 用户要求过：部署生产到新服务器 `8.212.180.123`，并让 naicode 新版在当前设备可用。
- 生产 new-api 已部署到 `8.212.180.123` 并 healthy。
- naicode 已重新编译并复制到本机 `~/.cargo/bin/naicode.exe`，但用户反馈 UI 仍不符合预期。
- 用户最后要求把完整总结写到文件，以便开启新对话。

## 2. 关键路径

### naicode 客户端

```text
D:\AndroidStudioProjects\naicode
```

当前分支：

```text
naicode
```

远程：

```text
origin   https://github.com/suqi8/naicode.git
upstream https://github.com/openai/codex.git
```

注意：本地已有多个 commit，但没有成功 push。不要再次 push，除非用户明确要求。

### new-api 服务端

```text
C:\Users\pc\new-api-src
```

当前分支：

```text
integrate-rc20-codex-namespace
```

远程：

```text
origin https://github.com/QuantumNous/new-api.git
```

尝试 push 失败过：

```text
remote: Permission to QuantumNous/new-api.git denied to suqi8.
fatal: unable to access 'https://github.com/QuantumNous/new-api.git/': The requested URL returned error: 403
```

所以 new-api 的服务端 commit 也只在本地，没有推送。

## 3. 生产服务器与部署状态

生产服务器：

```text
8.212.180.123
```

SSH：

```text
root@8.212.180.123
```

生产目录：

```text
/opt/new-api
```

容器：

```text
new-api
```

镜像：

```text
new-api:invite-rebate
```

生产域名：

```text
https://closedai.kylenqaq.com
```

生产部署机制：`/opt/new-api/Dockerfile.custom`

```dockerfile
FROM calciumion/new-api:latest
COPY new-api-custom /new-api
```

本轮已完成：

1. 备份生产二进制与镜像。
2. 本地交叉编译 Linux 二进制：

```bash
cd /c/Users/pc/new-api-src && CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -ldflags "-s -w -X 'github.com/QuantumNous/new-api/common.Version=v1.0.0-rc.19'" -o new-api-custom .
```

输出显示构建成功，产物约 128M。

3. 上传到服务器并替换 `/opt/new-api/new-api-custom`。
4. 重新构建镜像并 `docker compose up -d --force-recreate`。
5. 健康检查通过：

```text
status: OK
new-api container: Up ... (healthy)
```

结论：new-api 生产已部署成功，当前 healthy。

## 4. naicode 本地编译与安装状态

本轮执行过：

```bash
cd /d/AndroidStudioProjects/naicode/codex-rs && cargo build -p codex-cli
```

完成输出包括：

```text
Finished `dev` profile [unoptimized] target(s) in 9m 50s
```

随后复制安装：

```bash
cp -f /d/AndroidStudioProjects/naicode/codex-rs/target/debug/codex.exe "$HOME/.cargo/bin/naicode.exe"
```

输出：

```text
INSTALLED OK
```

注意：当前 `[[bin]] name` 仍是 `codex`，所以产物仍叫 `codex.exe`，本地通过复制成 `naicode.exe` 过渡。

## 5. 已有设计/交接/计划文件

产品化规格：

```text
D:\AndroidStudioProjects\naicode\docs\superpowers\specs\2026-07-10-naicode-product-tui-model-oauth-design.md
```

OAuth 设计：

```text
D:\AndroidStudioProjects\naicode\docs\superpowers\specs\2026-07-10-relay-oauth-design.md
```

之前已写的长交接：

```text
D:\AndroidStudioProjects\naicode\docs\superpowers\specs\2026-07-10-naicode-productization-handoff.md
```

实施计划：

```text
C:\Users\pc\.claude\plans\typed-shimmying-patterson.md
```

新对话应先读这些文件，再检查当前 git diff。

## 6. new-api 已完成内容

new-api 本地 commit：

```text
a1f9125f feat(catalog): structured OAuth catalog, strict group ratios, /v1/models OAuth auth
```

主要内容：

- `/v1/models` 和 `/v1/models/:model` 从 `TokenAuth()` 改为 `RelayOAuthOrTokenAuth()`。
- OAuth `CliOAuthAccessAuth` 写入 selected group snapshot 到 context。
- `ResolveGroupRatio(userGroup,targetGroup)`：special ratio 优先；缺失目标组报错；不再静默按 1；显式 0 保留。
- `ResolveCacheCreationRatios`：5m/1h cache-write ratio 共用，1h 使用 `6/3.75` 规则。
- `model.Model.SupportsCacheCreation1h` 明确能力字段。
- 新增/调整 catalog DTO 与 builder，输出结构化价格。
- catalog 返回完整价格通道：
  - input
  - output
  - cache_read
  - cache_create_5m
  - cache_create_1h
  - image_input
  - audio_input
  - audio_output
  - request
  - preview
- catalog 返回 display metadata：
  - USD
  - CNY
  - CUSTOM
  - TOKENS fallback USD
- 补回 `group_ratio` 与 `usable_group`。
- selected_group 权限校验。
- stable SHA-256 `pricing_version`。
- dynamic `tiered_expr` 安全返回 `basis=dynamic_expression` + `preview=null`。
- 公共 `/api/pricing` 契约预期未改变。

已通过的服务端目标测试：

```bash
go test ./controller -run "TestBuildCliOAuthCatalog|TestCliOAuthCatalogDisplay" -count=1 -v
```

```bash
go test ./controller ./relay/helper ./middleware ./router -count=1
```

```bash
go test ./service -run "Test(PostText|CalculateText|TryTiered|BuildTiered|ResolveGroup|ResolveCacheCreation)" -count=1
```

全量 `go test ./service` 曾失败于已有测试隔离问题：

```text
TestObserveChannelAffinityUsageCacheByRelayFormat_MixedMode
expected int(2), actual int64(3)
```

单独运行该测试通过，且相关文件未被本轮修改，判断为已有测试污染/隔离问题。

## 7. naicode 本地 commits

### 7.1 主题/欢迎区

```text
a2949fbd02 feat(tui): product palette, deep-space-blue theme and NAICODE welcome cell
```

内容：

- `[tui].product_accent` 配置。
- `product_palette.rs`。
- 默认 deep-space-blue 主题：
  - `#279CFF`
  - `#86CAFF`
  - `#0B2032`
  - `#07121D`
- `style.rs` 使用 product palette。
- `SessionHeaderHistoryCell` 增加 `▰▰ NAICODE ▰▰ 酸奶中转站`。
- `/clear`、resume、fork、replay 不重复大 Logo。
- history tests/snapshots 更新。

### 7.2 OAuth 请求层

```text
ef85cab2de feat(login): relay OAuth singleflight refresh, authenticated executor, structured catalog DTO
```

内容：

- `AuthManager::execute_relay_request`。
- RelayOAuth snapshot equality。
- 401 reload/refresh/retry。
- `relay_switch_group_remote_only`。
- `relay_commit_group_cache`。
- structured catalog DTO。
- `pricing_version` + legacy `version` fallback。
- `format_price_value`。
- `codex-login` tests 165/165 通过。
- `cargo check -p codex-login` clean。

### 7.3 Relay picker / 原子切换初版

```text
24719b2eed feat(tui): dedicated relay model picker and atomic group/model switching
```

内容：

- 新增：

```text
codex-rs/tui/src/bottom_pane/relay_model_picker.rs
```

- Loading/Ready/Error。
- 左分组 + 右模型。
- 完整价格。
- 搜索。
- 宽度布局。
- `PendingRelayModelSelection`。
- `ApplyRelaySelection` 等事件。
- 简化状态机。
- `cargo check -p codex-tui` 通过。

### 7.4 picker 接线修复

```text
5b9bf7d03e fix(tui): wire /model to RelayModelPicker; add BottomPaneView::as_any_mut
```

这是发现“UI 没变”后补的关键 commit：

- `open_relay_group_popup` 显示 `RelayModelPicker::Loading`。
- `OpenRelayGroups` 不再打开旧 `SelectionView`，而是 `update_relay_picker`。
- `BottomPaneView::as_any_mut()`。
- `RelayModelPicker::set_pricing()`。
- `BottomPane::show_relay_picker()` / `update_relay_picker()`。

之后重新编译并安装到本机。

## 8. 用户现场反馈的问题

### 8.1 `/model` 已变成新 picker，但视觉非常粗糙

用户截图显示：

- 左侧分组。
- 右侧模型。
- 模型完整价格已出现。
- 但样式问题明显：
  - 黑底 + 黄色边框 + 青色价格，像 debug UI。
  - 模型卡片太高。
  - 不像网页端设计稿。
  - 价格行松散。
  - 有些价格显示 `$-`。
  - 没有明显深空蓝/产品质感。

中断前已开始修：

- `CARD_HEIGHT` 从 3 改为 2。
- `model_card_lines()` 从 3 行改为 2 行紧凑显示。
- Wide 模式一行显示：

```text
输入 $x  输出 $x  缓存读 $x  缓存写 $x
```

- Medium 模式用符号：

```text
↑input ↓output ⬡cache_read/cache_write
```

- Narrow 模式只显示输入/输出。

涉及文件：

```text
D:\AndroidStudioProjects\naicode\codex-rs\tui\src\bottom_pane\relay_model_picker.rs
```

这些最新视觉紧凑化改动在中断前可能已写入，但尚未重新编译、提交、安装。新对话必须用 `git status --short` 确认。

### 8.2 新对话没有 Logo，只有公告

用户截图看到新对话顶部只有公告，没有 NAICODE Logo。

排查结果：

- Logo 在 `history_cell/session.rs` 中存在：

```rust
Span::styled("▰▰ NAICODE ▰▰", logo_style)
```

- 但公告由 `startup_tooltip_override` / relay notice 走 tooltip 路径。
- fresh session 的 `is_first_event=true` 分支原本只显示 header/help，公告在 `else` 分支，因此 Logo 与公告路径互斥。
- 用户看到只有公告，说明实际路径可能 `is_first_event=false` 或公告 override 抢走了首屏。

中断前已改思路：

- 在 `is_first_event` 分支内也显示 `tooltip_override`（公告），让 Logo 与公告同时出现。
- 需要继续确认实现、编译、安装。

涉及文件：

```text
D:\AndroidStudioProjects\naicode\codex-rs\tui\src\history_cell\session.rs
```

### 8.3 用户问“怎么切换思考等级”

当前实际状态：还没完整接好。

已实现的 picker/action 大概率仍是：

```rust
PendingRelayModelSelection { group, model, effort: None }
```

也就是选模型后直接应用默认/None，没有继续打开思考等级选择。

需要继续实现：

1. `RelayModelPicker` 选中模型后不要直接应用。
2. 触发一个“选择 reasoning effort”的 popup/selector，携带 group/model。
3. 从 catalog/model metadata 找到该模型支持的 thinking/reasoning levels。
4. 选完后再发送：

```rust
PendingRelayModelSelection { group, model, effort: Some(effort) }
```

5. 如果模型只有一个 supported effort，也要明确显示或自动应用并提示。
6. 最终 `PersistModelSelection` 也要带 selected effort，不要丢失。

### 8.4 用户要求“每句对话结束显示这几次调用用的余额”

用户新需求：每轮对话结束后显示本次调用消耗/余额。

当前未实现。

需要调研：

- app-server/core 是否在 `TurnCompletedNotification` 中有 token usage/cost。
- new-api 是否在 response metadata 或日志接口返回本次 quota/amount。
- TUI 目前是否已有 token usage cell。
- 如果服务端不返回本次扣费，需要新增 relay response metadata 或单独请求 usage/status。

已看到的相关入口：

```text
codex-rs/tui/src/app/side.rs: ServerNotification::TurnCompleted
codex-rs/app-server-protocol: TurnCompletedNotification
```

### 8.5 `codex-code-mode-host.exe` 缺失，导致 ping 失败

用户在新对话里执行：

```text
执行一个ping
```

系统返回：

```text
尝试执行 ping -n 4 8.8.8.8，但当前工具运行环境无法启动命令执行主机：

codex-code-mode-host.exe 文件不存在。

因此这次 ping 没有实际发出。
```

这不是服务端问题，而是本地 naicode 安装/打包问题。

推测：只复制了 `codex.exe` 到 `naicode.exe`，但缺少配套 `codex-code-mode-host.exe` 或类似辅助二进制。

需要排查：

- `D:\AndroidStudioProjects\naicode\codex-rs\target\debug` 下是否有 `codex-code-mode-host.exe`。
- `~/.cargo/bin` 下是否缺失该 host。
- CLI 如何定位 host 可执行文件。
- 是否应 `cargo build` 对应 package/bin 并复制多个 exe，而不只是 `codex.exe`。

## 9. 下一步建议顺序

1. 在 `D:\AndroidStudioProjects\naicode` 执行 `git status --short`，确认未提交改动。
2. 检查并完成：
   - `history_cell/session.rs`：Logo + 公告同时显示。
   - `relay_model_picker.rs`：紧凑价格 UI、产品色、不要黄色 debug 边框。
3. 实现“选模型后选择思考等级”。
4. 排查 `codex-code-mode-host.exe` 缺失，修安装/构建步骤。
5. 重新运行：

```bash
cd /d/AndroidStudioProjects/naicode/codex-rs
cargo check -p codex-tui
cargo build -p codex-cli
cp -f target/debug/codex.exe "$HOME/.cargo/bin/naicode.exe"
```

如果 host 需要额外复制，也一并复制到 `~/.cargo/bin`。

6. 启动/运行 naicode，实际验证：
   - 新对话首屏是否显示 Logo + 公告。
   - `/model` 视觉是否符合预期。
   - 选择模型后是否进入思考等级选择。
   - 选择成功后是否不再先显示成功再报换组失败。
   - `hello` 是否不再 `Invalid token`。
   - `ping` 是否能启动命令执行主机。
7. 再考虑每轮对话结束后的余额/消耗展示。

## 10. 重要注意事项

- 不要 push。
- UI 修复通常只需要重编译/安装 naicode，不需要重新部署 new-api。
- new-api 生产已部署并 healthy，除非改服务端，否则不要动服务器。
- 不要再声称“全部完成”，用户已经现场指出明显缺口。
- 对 Logo 问题，要从 `is_first_event`、`startup_tooltip_override`、history/session construction 实际路径修，不要只解释。
- 对思考等级问题，要承认当前没有接完整并实现。
- 对每轮余额/消耗展示，先查协议字段，不要假设服务端已有精确余额。
- 对 `codex-code-mode-host.exe`，这是本地安装缺失问题，优先解决，否则工具调用无法工作。

## 11. 2026-07-12 本轮继续完成内容

本轮在 `D:\AndroidStudioProjects\naicode` 继续修复客户端。没有 commit、没有 push、没有部署 new-api；仓库原本就存在大量其他未提交改动，后续不要误删或整体回退。

### 11.1 `/model` 界面缩小与产品化

主要文件：

```text
codex-rs/tui/src/bottom_pane/relay_model_picker.rs
```

已完成：

- Ready 状态高度从固定 20 行改为宽屏/中屏 14 行、窄屏 13 行。
- 每个模型固定两行：模型名 + 高频价格摘要。
- 选中模型底部增加详情区，展示图片、音频、按次、1h cache 等低频价格通道。
- 缺失价格显示 `—`，不再出现 `$-` 或 `¥-`。
- 焦点边框、选中背景、价格和搜索标题改用 `product_palette` 深空蓝语义色，移除黄色 debug 风格。
- 滚动可见卡片数按实际渲染高度计算。
- 新增紧凑高度和价格占位测试。

运行验证：真实启动 naicode 并打开 `/model`，确认紧凑界面、分组列表、模型价格和底部详情正常显示。

### 11.2 Relay 思考等级选择

主要文件：

```text
codex-rs/tui/src/app_event.rs
codex-rs/tui/src/app/event_dispatch.rs
codex-rs/tui/src/chatwidget/model_popups.rs
codex-rs/tui/src/bottom_pane/relay_model_picker.rs
```

已完成：

- 新增 `OpenRelayReasoningPopup { group, model }`。
- Relay picker 选模型后不再直接应用，而是打开思考等级弹层。
- 复用 `ModelCatalog` 的 supported/default reasoning effort。
- 目录找不到模型能力时提供“默认（由模型服务决定）”。
- 最终 `PendingRelayModelSelection` 会携带 `effort`。
- 远端换组成功后同时发送 `UpdateModel`、`UpdateReasoningEffort` 和带 effort 的 `PersistModelSelection`。
- 成功提示包含分组、模型和思考等级。

运行验证：真实选模型后已看到“思考等级”弹层。

注意：当前 Apply Relay 流程仍是简化版，使用 `relay_switch_group()`；完整远端切组→本地配置→thread settings→缓存提交及逆序补偿状态机仍未完成。

### 11.3 Logo + 公告

主要文件：

```text
codex-rs/tui/src/history_cell/session.rs
```

fresh session 顺序已改为：

```text
NAICODE Logo/header
→ relay 公告/tooltip
→ 帮助命令
```

`/clear`、resume、fork、replay 仍不重复大 Logo。

### 11.4 每轮 token 消耗和余额摘要

主要文件：

```text
codex-rs/tui/src/chatwidget.rs
codex-rs/tui/src/chatwidget/constructor.rs
codex-rs/tui/src/chatwidget/protocol.rs
```

已实现：

- 缓存 `ThreadTokenUsageUpdated.turn_id`。
- `TurnStarted` 清除旧 turn 对应关系，避免跨轮误用。
- 成功 `TurnCompleted` 时，仅在 turn id 匹配且非 replay 的情况下追加紧凑摘要。
- 摘要包括：总 token、输入、缓存输入、输出、推理 token。
- 若 `AccountRateLimitsUpdated` 中有 credits balance，则追加余额；服务端没返回时不伪造余额。

代码已通过 `cargo check`，但真实对话结束展示尚未验收：当前 new-api 请求连续出现：

```text
stream disconnected before completion: stream closed before response.completed
```

因此任务 #5 应视为“实现完成，运行验收被服务端阻塞”。

### 11.5 `codex-code-mode-host.exe`

已构建并安装：

```text
~/.cargo/bin/naicode.exe
~/.cargo/bin/codex-code-mode-host.exe
```

构建命令：

```bash
cargo build --manifest-path D:/AndroidStudioProjects/naicode/codex-rs/Cargo.toml \
  -p codex-cli -p codex-code-mode-host
```

`codex-code-mode-host.exe` 可直接启动并正常退出，原先“文件不存在”问题已经排除。

完整 ping 探针没有运行到工具调用阶段，因为 new-api 流提前断开；报错已经不再是 host 缺失。

### 11.6 验证结果

```text
cargo check -p codex-tui -p codex-login
```

通过，仅有既有 dead_code warnings。

Relay picker 目标测试：

```text
16 passed; 0 failed
```

`git diff --check` 通过，只有 Windows LF/CRLF 提示。

## 12. 当前首要新问题：经常出现 reauthentication_required

用户反馈经常看到：

```text
[reauthentication_required] 酸奶中转站登录已失效，请重新登录
```

初步根因已经定位，最可能是 rotating refresh token 的并发刷新冲突，而不一定是真正退出登录。

关键证据：

1. `AuthManager::shared()` 名字虽然叫 shared，但每次都只是 `Arc::new(Self::new(...))`，不是按 `codex_home` 复用的全局实例：

```text
codex-rs/login/src/auth/manager.rs:2502
```

2. `refresh_lock` 是每个 `AuthManager` 实例自己的一把 `Semaphore`：

```text
codex-rs/login/src/auth/manager.rs:1982
```

3. `/model` catalog、换组和实际模型请求可能各自创建新的 manager。多个实例会同时拿同一个 rotating refresh token：第一个刷新后旧 token 作废，第二个再刷新收到 401/invalid_grant，然后被永久映射成 `ReauthenticationRequired`。

4. 刷新接口当前只要：

```rust
status == 401 || status == 400 && body.contains("invalid_grant")
```

就永久判定登录失效：

```text
codex-rs/login/src/auth/manager.rs:1502
```

5. 任意 Relay 请求收到 401 都会 reload/refresh/retry，但 reload 和锁仍限于单个 manager：

```text
codex-rs/login/src/auth/manager.rs:2591
```

建议下一轮优先实现：

1. 同一 `codex_home` 使用真正共享的进程级 Relay refresh coordinator/lock，或让 TUI/catalog/换组全部复用 app-server 已有的同一个 `Arc<AuthManager>`。
2. refresh 返回 401/invalid_grant 后，再次从实际 storage reload：若 refresh token 已变化，说明另一个请求已成功轮换，直接用新 access token 重试，不提示登录失效。
3. 只有服务端明确返回 `device_revoked`、`session_revoked`、`refresh_token_revoked`，且 reload 后仍未变化，才映射 `ReauthenticationRequired`。
4. 将 `refresh_token_reused` 单独分类为可恢复竞争，不要等同 revoked。
5. 增加并发测试：两个不同 `AuthManager` 实例同时 catalog/model refresh，只允许一次实际轮换，另一请求 reload 新 token 后成功。
6. 检查服务端 `/api/cli/oauth/token` 的具体错误码和 refresh rotation 事务，必要时让 reused token 在短窗口内返回可识别错误，而不是通用 401。

临时规避：关闭所有旧 naicode 窗口，只保留一个进程，重新 `naicode login` 后重启客户端；这不能代替根治。

## 13. 下一轮建议顺序

1. 先修 `reauthentication_required` 的跨 manager/跨请求 refresh 竞争。
2. 运行并发 refresh 测试和一次 access token 临近过期的真实 `/model` + 模型请求。
3. new-api stream 恢复后验证：普通消息完成时显示本轮 token/余额摘要。
4. 再验证 ping 确实由已安装的 `codex-code-mode-host.exe` 执行。
5. 完成 Relay 原子选择的完整补偿状态机。
6. 不要 push；除非改了服务端且用户明确要求，否则不要部署生产。
