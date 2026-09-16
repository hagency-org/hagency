# HAgency 全链 E2E 手册(macOS,codex 执行)

面向:在 macOS 机器上由 codex(CLI,可用 shell + 看图)独立搭起 palpo + Hagency + robrix2 + 本地 agent,并跑完三层 E2E,产出可核对的证据。本文基于 2026-09-02~06 在 Linux 上跑通的同一套(OLP 黑板 #8–#17、E2E-1/2/3),命令均为实际用过的形状;macOS 差异单列。配套:`docs/TESTING.md`(测试纪律)、`scripts/verify-agent-e2e.sh`(一键序列)。

### 2026-09-06 macOS 实测校正

#### Web Dashboard 补充验收

- 当前贡献控制台是 `mockup/` 的 Next.js 应用，默认 `127.0.0.1:3100`；8084 是旧 web/queue 服务。运行 `npm ci --prefix mockup`、`npm --prefix mockup run build`，用仅服务端的 `HAGENCY_BACKEND=http://127.0.0.1:8090` 和本套 E2E 的 `HAGENCY_API_TOKEN` 启动。不要把令牌写进 URL 或 `NEXT_PUBLIC_*`。
- 用 Computer Use 逐页检查 resources、workforce、capability、projects、engagements、usage、alerts、config、onboard 和两个真实 agent 详情；本地两名 agent 与样例五名 agent 必须能区分。记录页面来源标签和实际后端字段。
- Config 添加预设/Agent 应打开真实向导。创建独立且不绑定 Agent 的测试预设，核对月额度/日额度、刷新持久化和后端记录，再精确清理该记录。既有预设、原有任务与 Herdr 会话须保留。
- 对 headless agent，`runner.availability=ready` 表示可按需派发，`runner.activity` 来自持久 dispatch 账本；它们不等于常驻进程在线。空闲 runner 没有 tmux pane，不能报 `tmux-missing:auto`。显式 tmux/ACP agent 继续使用各自的存活证据。
- 用量页任务图和总数读取同一份 per-agent 用量；当前承诺图只计 active engagement。未提供项目归属、实时日志、监管评估或保存接口时须明确说明，不能用样例、固定时间或成功 toast 填补。
- 可见页面应每 15 秒同步状态，重新切回窗口应立即刷新；样例模式下预设删除及 Agent 写操作均应禁用。自动刷新和写后刷新不能让旧响应覆盖新状态。
- `cgWindowNotFound` 时先确认桌面是否锁屏；本次锁屏导致新版点击复测中断，桌面恢复后已补测修复相关 GUI、双语/主题、样例操作禁用与无需重载的自动刷新。保留初始失败与后来补测证据，不能以 HTTP/SSR 检查代替截图。
- 自动化补验：根目录 `npm run test:dashboard`；mockup 目录生产构建与 `npm run check`。这些检查单列结果，不能替代真实 GUI 验收。Computer Use 的数字 spinbutton 如 `set_value` 被转换成 0，改用点击、全选、键盘输入，再核对显示值才提交。

下列实测结果优先于本文后续的旧操作示例:

- 本次 Palpo 和 PostgreSQL 均运行于隔离 Docker Compose 项目,Palpo 仅发布 `127.0.0.1:8008`。配置以仓库示例为准:当前监听器是 `[[listeners]]`,容器内配置与 appservice 目录使用容器路径。
- Robrix 的实际数据目录为 `~/Library/Application Support/org.robius.robrix`。必须隔离旧配置,在登录页确认本地 URL。点输入框工具栏的 `@`,从成员列表选择 agent;发送后验证 `m.mentions.user_ids`,不能只用粘贴 MXID 代替此项 GUI 验收。
- `verify-agent-e2e.sh` 实际验证 agent/preset/side/mint/budget/engagement;它不创建 project side、registration 或房间,也不证明聊天收发。`/api/matrix/reach` 返回配置和可达性结构,不能要求一个不存在的字面值 `flowing`。
- 当前普通 tmux MCP 的 `create_task` 会拒绝 `create_task requires a thread-session runner capability`。E2E-3 必须按 `docs/THREAD-SESSIONS.md` 启用本地线程运行器:backend 与 bridge 同时设置 `HAGENCY_THREAD_SESSIONS=1`,backend 设置 `HAGENCY_ROUTER_TASK_CUTOVER=1`;通过 operator API 给 agent 设置稳定 `agentId` 和 worker role,并保留有效的 workspace/MCP 配置。任务存储切换前先备份;不要对已有生产任务库直接套用一次性测试配置。
- worker 在主时间线被提及时,backend 自动创建任务和线程 outbox;Matrix 确认后才启动带 capability 的运行器。观察 `GET /api/router/snapshot`:如果始终为 `pending_thread`,检查 bridge 是否实际轮询 router outbox。该轮询必须在 bot 与 appservice 两种启动路径上均启动,不能依赖普通 bot 登录成功。
- 线程运行器的任务工具现已通过 `/api/router/task-operations` 校验 agent token 和完整 dispatch capability。worker 启动时任务已是 `in_progress`,使用 `get_task`、`comment_task`、`update_task_execution` 和 `transition_task`;不要重复 accept/create。`list_tasks`/`get_task` 只读当前绑定任务和本 coordinator session 创建的任务;写操作只限当前绑定。旧 `post` 仍不可用,最终文本由 reply outbox 回到原线程。必须分别验收内环产物、独立验证、`task=done`、`dispatch=completed` 和 Matrix 送达,不能由完成文字推断任务状态。
- Herdr 外环等待不能仅依赖黑板行数增加:内环可能替换现有 `ACK:` 占位行。应核对实际 ACK 内容、commit、测试和终端状态;`已完成` 与 `空闲` 都需要明确处理。
- 本次 operator 要求仅 E2E agent 禁用 mempal。为 E2E 创建独立 Claude 启动包装器,用 `--strict-mcp-config` 只加载 Hagency MCP,并通过 `--setting-sources project,local --settings <E2E settings copy>` 加载移除 mempal Stop hook 的配置副本;保留其他权限和钩子,不改全局配置。普通 tmux 会话也必须重启到同一包装器。仅写提示词不能阻止全局 Stop hook 覆盖任务最终回复。
- Computer Use 的剪贴板 `-10005` 超时不代表粘贴失败。必须重新截图检查输入框,核对后再发送,避免重复粘贴;本次 `type_text` 也出现中文丢失。GUI 操作使用 Computer Use 技能提供的接口,后文旧的 osascript/cliclick 示例不适用于本次执行环境。
- `herdr session attach hagency-agents-e2e` 可查看本次内环 `w1:p1`。默认 `Ctrl+B` 后按 `Q` 只脱离界面,保留后台任务。F07 退房测试结束后应恢复测试 agent 的成员资格并在房间标注,避免 operator 把预期拒投警告误认为当前故障。
- F07 的增量同步断档不能用 invite..join 补拉:本次初测 45 条仅 22 条到 backend。修复后先持久化断档的 `from`/`to` sync cursor,再按该区间正向分页,经既有 appservice router 投递消息;实际复验恢复 45/45。历史成员事件不重放,以免旧 leave 覆盖新 join。无边界的旧记录、畸形事件、分页超限、读取或投递失败均保留 pending。`/messages` 接受 sync token 作为 from/to 的协议依据见 [Matrix Client-Server API](https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3roomsroomidmessages)。补拉可能晚于已收到的 timeline 消息,本次未证明跨断档的整体顺序。
- 自主监控必须实测:初次外环虽然启动等待,却漏认完成,需要 Codex 介入;后续 `E2EAUTOWATCH20260906A` 复验由房间 agent 自己修复监控,检查新 nonce JSON 内容和新增完成序号,独立测试后回报,没有 Codex 结束等待。两次结果应分别评级,不能用复验通过抹掉首次失败。
- 修复复测 `E2EREPAIR20260906A`:真实 Claude 中层通过 `hagency-inner-loop` 的 prepare/watch 流程委派 octoscode,自己处理实例锁冲突、独立验收 9 个测试及 CLI 行为,自行写评论并完成任务,最终回帖到原线程。该次由本地 Matrix API 发起;Computer Use 对 Robrix/Finder 返回 `cgWindowNotFound`,因此不能标为新的 GUI 通过。
- 同一项目启动第二个 octoscode 可能遇到 `OCTOS_DATA_DIR_LOCKED`。保留旧 session,为新实例通过 `octos serve --instance-data-dir <独立控制目录>` 隔离运行数据,不要删锁或结束其他任务。Herdr 的 runtime 状态和当前 job 的 verified 状态分开记录;不能只凭 idle 或序号接受工作。
- botless 成员变更应通过房间所属 side 的 appservice 身份执行,检查实际 HTTP 结果。项目 2 的初次 remove/add 虽即时读到 leave/join,几秒后却被迟到成员事件的 SSE 回声再次踢出,不能按稳定通过交付。必须区分 Matrix 成员观察和新的成员命令,拒绝旧事件覆盖当前状态,并在同步处理后复核成员仍保持。完整 MXID 必须对应已登记 agent,不能把 Claude/Codex 的 framework type 误判成非 agent。
- 本次修复需要保持本地与 `remote/lib/mcp-server-core.js` 的源码镜像一致,因此该镜像文件随任务工具和 PID 清理修复同步;没有部署或运行远端服务。此项优先于下文旧的“不改 remote/”执行约束。
- provision 创建的项目路径必须进入 `workdir/docs/projects.md`。当前生成器从 manifest 写入受管映射块,明确 `projects/` 相对于 workdir,并说明 copy/symlink 的编辑影响;项目增删会刷新映射并保留块外人工笔记。
- 项目 2 的成员回声修复后,连续 60 秒共 31 次同时检查 Matrix 成员和 Hagency 名单均保持正确,成员事件中没有再次踢人。Matrix 观察必须由 bridge 身份标记来源,后端保留该来源,SSE 消费端不再把观察当作新的邀请/踢人命令。
- Codex 0.153.4 的原生 `mcpServer/elicitation/request` 可能先于 Hagency MCP 调用出现,旧运行器未处理会卡住。当前适配器按活跃结构化 MCP item 关联身份和参数,复用已有的窄范围协调工具例外;其他支持的请求仍进入 owner 审批。未知、重复或失效请求显式拒绝,不能靠解析展示文字或放宽整个 sandbox 绕过。
- 原生审批 E2E 需要代表实际加入 owner 审批房,且当前 agent/project 的 owner binding 和成员事实有效。本次 Codex 测试显式配置了这些前置条件;审批卡片和一次性 verdict 均经本地 Matrix 传递。这证明配置后的审批链,不等于证明 owner 房间和 binding 自动开通。
- Codex 原生执行时限按整轮 wall clock 计算,包含 owner 审批和启动准备。本次 R1 在 20 分钟默认上限处进入 `outcome_unknown`;必须先检查工作区、下层进程和未完成工作,再用 outcome-inspection/resolve-outcome 正式恢复。R2 仅在隔离 E2E `.env` 设置 `HAGENCY_RUNNER_LEASE_MS=3600000`,源代码默认值和原生审批不变。不能修改任务为 done 来掩盖超时。


- `E2EREPAIR20260906CODEX-R2` 复测最终通过:真实 Codex 中层保留监控并追补下层漏写的结果文件,独立纠正测试数量,验收提交 `b1309f6` 的 41 项 Rust/CLI 测试、26 个独立 CLI 场景及 fmt/clippy/build 后,自行完成任务;运行器完成,唯一最终回帖送达原线程。该链路包含已记录的正式恢复和 owner 一次性审批,不是“无需审批且首次即成功”。本次只实测下层 octoscode/kimi,不覆盖所有下层框架组合。
- 修复后的完整 `npm run verify:ci` 为 502 tests / 45 files 通过,专项综合集另有 336 tests 通过。agent-spec 只通过边界检查,Node 场景仍是 skip,实际行为由对应 Vitest 和真实运行验证。最终报告位于 `~/.octos/outer/verify/e2e-repair-20260906/RESULT.md`;Computer Use 最后复查仍为 `cgWindowNotFound`,新 GUI 复测明确未验证。

---
## 0. 目标与三层验收(先读懂再动手)

目标一句话:**用 palpo appservice 把本地 agent 组织进 Matrix 房间——不给每个 agent 注册账号;人在 robrix2 里聊需求,agent 在本地干活,结果回到房间。**

产品流程分三层:Robrix2 ↔ Hagency 负责组织管理;Hagency 直接管理的
Claude/Codex agent 负责需求分解、任务安排、监控和验收;Herdr + octoloop
控制执行代码工作的下层 agent,可选 octoscode/Claude/Codex/Grok 等。
以下 E2E-1/2/3 是测试分组,并不改变这三个产品层的职责。

| 层 | 名称 | 证明什么 | 驱动方式 |
|---|---|---|---|
| E2E-1 | API 层全链 + 回归清单(8 项) | sync 收件、三种收件模式失败边界、多项目隔离(F03/F04)、来源校验(F06)、名单校验(F10)、多 fleet(模型 C) | 真实 Matrix API(@alex token)+ 日志/状态文件取证 |
| E2E-2 | robrix2 图形界面全链 | 人在 GUI 里发 `!request` → 看到 `@ac_<agent>` 的回复显示在时间线 | 截图 + 鼠标/键盘自动化(macOS 见 §6) |
| E2E-3 | agent 起 octoloop(octoscode 内环)完成任务并回报 + 可观测 | 任务在房间下达 → agent 接单 → octoscode 干活 → 状态/结果回房间;tmux/herdr 随时可看 | agent 侧执行策略(CLAUDE.md)+ MCP task 工具 + GUI/API 取证 |

**纪律(硬)**:每项给 verified / partially-verified / failed + 证据路径;失败项记复现步骤不猜根因;**发现 bug 直接修**(operator 裁决:E2E 发现的 bug 由 codex 直修,先红后绿,`npm run verify:ci` 过,只 commit 不 push,ACK 附逐字);不改 remote/;不把测试缝当生产证明;不用 `/tmp` 放构建产物;所有证据放 `~/.octos/outer/verify/e2e-<n>/`。

---
## 1. 组件与代码状态

| 组件 | 仓库 | 说明 |
|---|---|---|
| palpo(Matrix homeserver,Rust) | `palpo-im/palpo` main | 源码构建 `cargo build --release`(首编 6–10 分钟);需 PostgreSQL |
| Hagency(backend + bridge + MCP + CLI,Node) | `hagency-org/hagency` master | **必须含**:#137(F03/F04)、#139/#140(sync majors F05–F10)、#143/#145(F06 gate)、#147(r9 邀请态终局)。**应含**:#149(r10 限流退避)、#150(r11 回复按来源房路由)——若尚未合并,请 `git merge origin/fix/sync-member-read-backoff origin/fix/source-room-reply-routing` 到本地测试分支后再跑 |
| robrix2(Matrix 客户端,Rust/Makepad) | 本地 robrix2 仓 | `cargo build --release`(首编 20–40 分钟,先起后台) |
| 本地 agent | Claude Code(或 codex)在 tmux 内,经 Hagency MCP 连 backend | `hagency up <name> <workspace> claude` |
| octoscode + herdr + octoloop skill | 已安装 | E2E-3 用 |

已知上游 palpo 问题(有 PR/素材,不阻塞):命名空间正则不锚定(PR #421)、退房成员不过滤、filter 无通配、注册表分裂(只用 YAML 目录 + 重启,**别用 admin API 注册**)。

---
## 2. 搭环境(macOS)

### 2.1 依赖
```
brew install postgresql@16 tmux jq
# Rust toolchain(rustup)、Node ≥ 20、cargo 已就绪;Xcode CLT
```
PostgreSQL:用 brew 服务或 Docker(`docker run -d --name palpo-e2e-pg -e POSTGRES_USER=palpo -e POSTGRES_PASSWORD=<pw> -e POSTGRES_DB=palpo_smoke -p 127.0.0.1:5433:5432 postgres:16`)。**只用一次性空库**。

### 2.2 palpo
`~/.hagency/e2e/palpo.toml` 最小形状(与 Linux 同款):
```toml
server_name = "127.0.0.1:8008"
allow_registration = true
enable_admin_room = false
appservice_registration_dir = "/Users/<you>/.hagency/e2e/appservices"
[listener]        # 以仓库 palpo-example.toml 为准
address = "127.0.0.1:8008"
[db]
url = "postgres://palpo:<pw>@127.0.0.1:5433/palpo_smoke"
```
启动:`PALPO_CONFIG=~/.hagency/e2e/palpo.toml nohup ./target/release/palpo >> ~/.hagency/e2e/logs/palpo.log 2>&1 &`;健康:`curl -s http://127.0.0.1:8008/_matrix/client/versions`。**registration 只在启动时装载:每次改 `appservices/` 目录都要重启 palpo。**

### 2.3 Hagency runtime
`~/.hagency/e2e/.env`(键名;值自定,`HAGENCY_OWNER_DM_ROOM` 建完审批房再填):
```
HAGENCY_RUNTIME_DIR=~/.hagency/e2e   HAGENCY_BACKEND_PORT=8090   API_TOKEN=<随机>
MATRIX_BRIDGE_SECRET=<随机>   MATRIX_HOMESERVER=http://127.0.0.1:8008   MATRIX_SERVER_NAME=e2e-home.invalid
MATRIX_BOT_USERNAME=   MATRIX_BOT_PASSWORD=          # 留空 = bot-less 模式(预期告警,不消音)
MATRIX_AGENT_PREFIX=ac_   MATRIX_TRUST_MODE=audit     # 第二轮切 enforce 重走
MATRIX_OPERATOR_MXIDS=@alex:127.0.0.1:8008   MATRIX_ADMIN_MXIDS=@alex:127.0.0.1:8008
HAGENCY_OWNER_MXID=@alex:127.0.0.1:8008   HAGENCY_OWNER_DM_ROOM=<owner 审批房 id>
HAGENCY_APPSERVICE_SYNC_SIDE=127.0.0.1:8008   HAGENCY_APPSERVICE_SYNC_URL=http://127.0.0.1:8008
```
起 backend / bridge(从仓库根目录,同一 .env):
```
set -a; . ~/.hagency/e2e/.env; set +a
nohup node backend-v2.js  >> ~/.hagency/e2e/logs/backend.log 2>&1 &
nohup node bridge-matrix.js >> ~/.hagency/e2e/logs/bridge.log  2>&1 &
```
健康:backend 日志 "listening on http://127.0.0.1:8090";bridge 日志 "[appservice-sync] logged in as @hagency:…";`GET /api/matrix/reach`(Bearer API_TOKEN)→ `flowing`。

### 2.4 一键走完"空 fleet → 人类消息到达组"
`scripts/verify-agent-e2e.sh`(需 `HAGENCY_RUNTIME_DIR`)把 project side 创建、registration 生成(写到 `<runtime>/appservices/*.yaml`,**然后重启 palpo**)、agent 登记、`!offer`/`!request`、verdict `{approve, allocatedTokens}` 等字段名全部编码好了——先跑它,失败再看 §7 的坑。关键 API:`POST /api/project-sides`、`POST /api/agents`、`POST /api/agents/:name/matrix-identity`、`POST /api/engagements/:id/verdict`、`GET /api/agents/:name/pane`。

### 2.5 人类账号与房间(curl 扮人,GUI 前置)
- 注册 `@alex`(开放注册):`POST /_matrix/client/v3/register`(`alex` / 密码自定,记入手册)。
- 建 **明文** 项目房(名 `e2e project room`,**关加密**)、owner 审批房(名 `e2e owner approvals`),把 room id 写入 `~/.hagency/e2e/{project-room.id,owner-room.id}`,alex 的 token 写 `alex.token`。
- 邀请代表 `@hagency:127.0.0.1:8008` 进项目房 → 代表经 sync 收到 invite 自动入房("knock answered")→ group 自动创建。
- owner 审批房还需要两个独立前置条件:owner 与**当前真实代表**都已 `join`,且代表有权写发现 marker 的两个空 state key。先用 owner token 读取该房的 `joined_members`,逐字确认 owner MXID 和当前代表 MXID 都存在;binding、消息来源或已有 marker 只能帮助发现房间,都不是 verdict authority,也不能代替成员事实。
- 随后用 owner token 在**每次准备写入之前重新 GET** 该房空 state key 的 `m.room.power_levels`,保留其完整 JSON,并先确认 owner 当前有权修改它。房间若由有权限的 bot 创建,或当前代表的 level 已达到两个 marker event 的要求,则不写。以下变更只适用于已确认使用常见 `users_default: 0`、`state_default: 50`,且两个 marker event 没有刻意设置更高自定义策略的测试房:保留全部既有 users/admin/defaults/events,把当前代表的 user level 至少设为 `1`,并只把 `com.agentchat.approval.room.v1` 与 `com.agentchat.approval.room.v2` 的 event level 设为 `1`。例如:
  ```bash
  # OWNER_ROOM_ENCODED 是 URL 编码后的 room id;变量值不要写进取证文档。
  curl -fsS -H "Authorization: Bearer $OWNER_TOKEN" \
    "$HS/_matrix/client/v3/rooms/$OWNER_ROOM_ENCODED/state/m.room.power_levels/" > "$PL_FRESH"
  jq --arg rep "$CURRENT_REPRESENTATIVE" '
    .users = (.users // {}) |
    .users[$rep] = ([.users[$rep] // .users_default // 0, 1] | max) |
    .events = (.events // {}) |
    .events["com.agentchat.approval.room.v1"] = 1 |
    .events["com.agentchat.approval.room.v2"] = 1
  ' "$PL_FRESH" > "$PL_PATCH"
  curl -fsS -X PUT -H "Authorization: Bearer $OWNER_TOKEN" -H 'Content-Type: application/json' \
    --data-binary @"$PL_PATCH" \
    "$HS/_matrix/client/v3/rooms/$OWNER_ROOM_ENCODED/state/m.room.power_levels/"
  # PUT 后重新 GET,核对代表 level 与两个精确 event type,并核对其他键没有被覆盖。
  curl -fsS -H "Authorization: Bearer $OWNER_TOKEN" \
    "$HS/_matrix/client/v3/rooms/$OWNER_ROOM_ENCODED/state/m.room.power_levels/" > "$PL_AFTER"
  ```
  不得把 `state_default` 降为 `0`,不得重建一个只含测试键的 power-level 对象,也不得给代表 verdict 权限。最终 readback 不匹配即停止 E2E。

### 2.6 本地 agent
```
mkdir -p ~/.hagency/e2e/agent-ws && hagency up e2e-claude ~/.hagency/e2e/agent-ws claude
tmux ls   # 期望看到 e2e-claude
```
`agent-ws/.mcp.json` 指向仓库 `mcp-server.js`,env 含 `HAGENCY_API`、`HAGENCY_AGENT_STATE_DIR`(令牌 fail-closed:缺 agent-token 会 exit 3,这是设计)。role-capacity 的 strong 档已含 `claude-fable-5-1`;若用别的模型名,preset 里声明可匹配的模型。

---
## 3. E2E-1(API 层)清单
以 @alex token 发消息 / 读 `/messages`,日志在 `~/.hagency/e2e/logs/`,状态在 `~/.hagency/e2e/data/matrix/bridge-state.json`(`appserviceSync` 是 cursor)。
1. **主链**:项目房 `!request coding 500` → 代表回执 "awaiting a decision…" → `POST /api/engagements/<id>/verdict {"approve":true,"allocatedTokens":500}` → 再发一条点名 `@ac_e2e-claude:… <唯一 nonce>` → 房内出现 agent 回复(`m.in_reply_to` 指向 nonce 消息)。**注意:批准不会让 agent 自动发言,必须再点名。**
2. **F03 同名房**:@alex 新建同名 "e2e project room" 邀请代表 → 原映射不变、日志 `Group "…" is ambiguous across sides`/冲突拒绝、新房不接管;(r9 前这里会毒批熔断,r9 后同批 invite→join→state 应一次通过)。**跑完把同名房改名**,否则后续回复按组名路由会歧义(r11 前)。
3. **F04**:同 agent 第二个项目房接 engagement → 两房 owner/DM 绑定各自不变。
4. **F05/F08**:构造代表 join 可重试失败(短停 palpo)→ 不 ack、cursor 不动、恢复后重投成功。
5. **F07 + r10**:agent `/leave` → bridge 清扫日志、该房不再投递;大批 noise 事件制造 `timeline.limited` gap → 补拉触发;palpo 限流 `M_LIMIT_EXCEEDED` 时 collector **指数退避后自动恢复,不永久熔断**(r10)。
6. **F10**:命名空间内但未登记的 `@ac_ghost` 走准入/撤单 → 零 Matrix 请求 + REFUSED(公共 API 会在更前面以 unknown agent 拒绝,如实记边界)。
7. **F06**:sync 结构化 provenance 日志(mode/registration/sideId/room/ref);无 push listener 时"伪 hs_token→403"无可达入口,记 partially;无代表登记的 side → 终局 `side_incomplete_registration`。
8. **多 fleet(模型 C)**:第二份 registration(`id: hagency-e2e-b`,`sender_localpart: hagency_b`,`@bc_.*` exclusive)放入 `appservices/` → 重启 palpo → 用 B 的 as_token `m.login.application_service` 登录为 `@hagency_b` → 断言:互相冒名 403 "not in appservice's namespace";各自 /sync 只见自己前缀事件;B 代表在房不构成 A 的关系(A 查成员 404、A agent 跨发 403)。

---
## 4. E2E-2(robrix2 GUI)
1. **干净会话**:robrix 会自动恢复上次登录(如 matrix.palpo.im)。用隔离目录启动:macOS 下 robrix 数据在 `~/Library/Application Support/robrix`(robius_directories ProjectDirs);把它临时改名,或用 `HOME=<隔离目录>` 启动。
2. 登录页三栏:User ID `alex` / Password / **Homeserver URL 手输 `http://127.0.0.1:8008`**(默认 matrix.org);Makepad 输入框**没有 Tab 切焦点、没有可见焦点指示**,必须**鼠标点击**每个框再输入;输入后截图确认文字落在正确框再提交。
3. 先完成 **engagement 资源分配**:在项目房发送 `!request coding 500`,看到代表的 engagement 回执后,按 §3.1 对 `/api/engagements/:id/verdict` 提交资源批准与 `allocatedTokens`。这一步只建立项目资源/agent 前置条件,不会被记作 native execution approval,也不要求它生成 owner execution 卡片。
4. 分配完成后,在 Robrix 的 `@` picker 选择**实际 agent**,发送一条会触发受保护工具调用的唯一 nonce 任务。此时 owner 审批房必须出现对应的真实 native execution approval 卡片;owner 分别实测卡片上的一次性 **Approve once** 与 **Deny**,核对决定只绑定各自 request/digest,并核对批准后的 agent 回复仍在原项目线程。卡片、任一 owner 按钮或结果关联缺失时本项为 **failed**;直接调用 `/api/approvals` verdict 只能单独诊断后端,不能替代 GUI 验收。
5. 结束后 robrix 保持运行给 operator 看。
安全规则:每次点击/输入前确认前台窗口是 Robrix(`osascript -e 'tell app "System Events" to get name of first process whose frontmost is true'`),只点 Robrix 窗口内坐标;只输入 URL/用户名/测试密码/一条指令;连续两次焦点确认失败即停;**非安全类偏差(如 Enter 变换行)自行处理继续**。

---
## 5. E2E-3(agent 起 octoloop + 回报 + 可观测)

1. 启用本地 thread sessions 和 task cutover,配置稳定 agentId、worker role、workspace 和 E2E-only mempal 隔离。通过 `hagency-sync-skills` 安装 `hagency-inner-loop`,或仅在 E2E workspace 的 `.claude/skills` / `.agents/skills` 链接整个技能目录,确保 `scripts/monitor.mjs` 可读取。
2. 在 Robrix 点击 `@` 选择实际 agent,发送带唯一 nonce 的具体实现任务。backend 自动创建任务/线程,Matrix 确认后启动中层。另行用 API 发起的测试必须标为 API 驱动。
3. 中层读取已经开始的任务,用评论分解验收条件。在明确授权的命名 Herdr session 选择真实下层,先准备 nonce job 和独立 verifier,启动受其管理的 watch,然后发送实现提示。等待期间保留监控句柄,定期更新任务 heartbeat;不要在下层未完成时结束 dispatch。
4. 内环完成后,中层检查结果内容、提交或工作树、进程身份和独立 verifier 报告,必要时在独立 checkout 复验。验证通过才评论证据并显式 `transition_task` 到 done,然后返回最终文本供 Hagency 回帖。不得直接编辑 backend 数据、把旧 ACK 当本次结果或让测试驱动代写实现。
5. 取证分别核对 task 的 comments/heartbeat/done、dispatch completed、nonce report、真实内环提交和同一 Matrix thread 的回帖。普通 tmux pane 可能不是当前 headless runner;查看明确的 Herdr session 和 pane。`herdr session attach <name>` 打开内环 UI,默认 `Ctrl+B` 后 `Q` 只脱离 UI,不结束任务。
6. 按实际组合记录覆盖范围。Claude→octoscode 通过不能代表 Codex/Grok 等全部组合通过;GUI、审批和 continuity gate 也须各自提供证据。

---
## 6. macOS 差异速查
| Linux 用法 | macOS 替代 |
|---|---|
| grim 截图 | `screencapture -x <file>.png`(需"屏幕录制"权限);窗口区域 `screencapture -l <windowid>`(`osascript` 取 id)|
| hyprctl 聚焦/活动窗口 | `osascript -e 'tell app "Robrix" to activate'`;活动窗口见 §4 安全规则 |
| ydotool 点击/输入 | `brew install cliclick` → `cliclick c:<x>,<y>` / `t:<text>`;或 `osascript -e 'tell app "System Events" to keystroke "…"'`(需"辅助功能"权限)|
| wtype | `cliclick t:` 或 System Events keystroke |
| tmux/herdr | 同 Linux(herdr 需在 mac 安装)|
| Retina 坐标 | `screencapture` 出物理像素;cliclick 用逻辑点 = 物理/2。先做一次标定:移到窗口中心截图确认 |
| /tmp | 可用,但构建产物一律放仓库 target/,证据放 `~/.octos/outer/verify/` |

---
## 7. 坑目录(今天真踩过的)
- **robrix 自动恢复旧会话**登到线上服务器 → 隔离数据目录(§4.1)。
- **Makepad 无 Tab 焦点、Enter 换行** → 必须点击;发送按钮在右下角。
- **批准后 agent 不说话** → 再发点名消息。
- **owner 审批房没有审批卡或按钮** → GUI 验收为 failed。API verdict 仅可单列为后端诊断，不能替代 owner 原生按钮；先按 §2.5 核对当前成员、binding、marker 权限与消息投递证据。
- **同名房让组名歧义** → 回复被 `group-route` 拒投(r11 修;修前把同名测试房改名)。
- **代表仅被邀请的房间** → r9 前把 create 事件判可重试 → 毒批 8×500 → collector 熔断、cursor 卡死 → 需重启 bridge;r9 后应一次通过。
- **approval marker HTTP 403** → 核对当前代表已 join 及两个 marker 的 power level；保留原始失败，按 §2.5 做窄范围修正并回读。不要把它当成限流。
- **Matrix HTTP 429** → 按真实请求时间和 retry_after_ms 核对共享限流与退避；两个并发任务槽不能证明 HTTP 请求有间隔。审批适配器的显式 HTTP 使用共享 pacer，SDK 内部请求不能据此宣称全部覆盖。
- **成员读取限流 M_LIMIT_EXCEEDED** → r10 前永久熔断;r10 后指数退避自动恢复。
- **registration 只在 palpo 启动时装载**;**别用 admin API 注册**(进 DB 不进文件表,masquerade 必挂)。
- **bridge 有 owner 锁**:重启要精确找 pid(`ps -eo pid,args | awk '$2=="node" && $3 ~ /bridge-matrix\.js$/'`),别用 pgrep 文本匹配。
- **verify:ci 有未定义标识符门禁**:改测试也要跑 `npm run verify:ci`。
- **令牌 fail-closed**:agent 状态目录缺 `agent-token` → MCP exit 3,属设计。

---
## 8. 交付格式

Owner 审批房的人工审阅清单见 [owner-approval-room-preconditions.md](verification/owner-approval-room-preconditions.md)；CI 测试通过不替代其中的 GUI 验收。
- 每层一个 `RESULT.md`(清单表:项 / 结论 / 证据路径 / 复现步骤),证据文件编号。
- ACK 写到 Hagency 仓 `.octos/OUTER_LOOP_REVIEW.md`:`ACK(E2E-<n> done|blocked)` + 表;bug 修复:分支 `fix/<slug>` 基 master,先红后绿逐字,`npm run verify:ci`,只 commit 不 push,ACK 附 `git show --stat`。
- R2 诚实分级:verified / partially-verified / unverified,不把测试缝、静态证据或"选择器命中"冒充真机行为。
