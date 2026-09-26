# telegram-bot-gateway

[English](README.md) · [愿景](docs/intent/README.zh-CN.md) · [规范](docs/specs/README.zh-CN.md)

为 Agent 提供持久化的 Telegram Topic 通信。`tbg` 通过 HTTP 发送和读取消息；Gateway 用 SQLite 保存对话、独立订阅和确认进度，User 通过 Telegram 参与。

首条端到端链路已实现：管理员初始化、Group 接入、Agent 注册、Topic 创建/关闭/重开、发送、订阅、unread/history、wait、ack 和 Bot ❤️ 回执。目前 `/manage` 用于接入 Group 并显示操作范围。完整按钮菜单、普通 User 信任管理、Doctor 页面和持久化语言偏好仍待实现。当前请将参与通信的 User 配置为管理员，不要通过修改数据库授予信任。

## 在 Linux PC 上运行

需要 Docker 和 Compose。使用 Rust 1.95 或更新版本构建并安装 CLI：

```sh
cargo install --path . --locked --bin tbg
mkdir -p ~/.config/tbg
cp config/client.example.yaml ~/.config/tbg/client.yaml
cp config/server.example.yaml ~/.config/tbg/server.yaml
```

编辑两个配置文件。在 `server.yaml` 设置 Bot token，并明确填写监听 IP 和端口；在 `client.yaml` 填写可访问的相同 IP 和端口。`18473` 仅为示例，没有默认端口。Compose 使用 Linux host 网络，因此 `listen` 就是宿主机地址；Server 配置只读挂载，完整的 `/var/lib/tbg` 数据目录保存在命名卷中。示例监听本机回环地址。HTTP 不鉴权，能够连接的调用方都可以选择已注册的 Agent 身份。

```sh
docker compose up -d --build
docker compose logs -f gateway
```

私聊 Bot 发送 `/whoami`，将返回的数字 `user_id` 加入 `server.yaml` 的 `admins`，再重启：

```sh
docker compose restart gateway
```

创建启用 Topic 的 Telegram Group，将 Bot 设为管理员并授予 Manage Topics 权限，再由已配置的 Gateway 管理员在 Group 内输入 `/manage`。Gateway 使用轮询接收消息；发现已有 webhook 时会记录冲突，不会自动删除 webhook。同一数据目录只允许一个 Gateway 进程使用，并绑定首次核实的 Bot 身份。

## 使用 CLI

成功时输出一行紧凑 JSON，可用 `jq .` 格式化。错误写入 stderr，并以非零状态退出。注册一次后，后续进程复用同一个 Agent 名称。

```sh
tbg agent register --name hopeful_morse | jq .
tbg --agent hopeful_morse group list | jq .
tbg --agent hopeful_morse topic create --group <group> --name "Discussion" | jq .
tbg --agent hopeful_morse subscribe <topic>
tbg --agent hopeful_morse send <topic> "Ready to collaborate"
tbg --agent hopeful_morse wait | jq .
tbg --agent hopeful_morse unread <topic> --limit 20 | jq .
# 如有剩余消息，用 --cursor <next_cursor> 继续读完再回应。
tbg --agent hopeful_morse send <topic> "Completed" --quote <msg_id>
tbg --agent hopeful_morse ack <topic> --through <msg_id>
tbg --agent hopeful_morse history <topic> --limit 20 | jq .
```

`unread` 从保存的 ack 之后向前读，`history` 从最新消息向历史回看。两者返回 `next_cursor` 和 `remaining_count`，游标本身不包含在下一页中。读取不保存进度，只有 ack 推进确认位置。同一 Agent 只能有一个 wait，不指定 timeout 就持续等待。静音订阅只被明确的 `@AgentName` 唤起，但其中的全部对话仍可读取。

send 成功表示消息和投递任务已在本地提交。后台投递支持重启恢复和 Telegram 限流重试。Telegram 已接收但响应丢失时，重试可能造成外部重复消息，本地 `msg_id` 不变。再次执行 CLI send 会创建新消息。ack 和待发送的心形回执一起提交；回执失败不会撤销确认进度。容器日志记录投递失败和重试。备份前先停止容器，复制完整的数据目录，包括 SQLite 附属文件。

出站文本连同 Agent 标头超过 4096 个 UTF-16 单元时，在接收前报错。Telegram 非文本消息保留 `[类型]` 标记、caption 和已接收的 update 内容（嵌套回复仅保留平台引用），暂不下载附件。general/default Topic 预留。未信任 User 的普通消息被忽略；允许的管理交互独立保存，不进入对话历史。

## 任务结束通知

Hook 是独立的 CLI 调用方。传入已注册的 Agent、已有 Topic 和通知 JSON，并确保 PATH 中有 `tbg`。它需要 Python 3，将 `agent-turn-complete` 事件的 `last-assistant-message` 转发出去，不自动重试结果不确定的发送。通知失败写入 stderr，不影响原任务的退出状态。

```sh
./integrations/codex-notify.sh hopeful_morse <topic> \
  '{"type":"agent-turn-complete","last-assistant-message":"Task completed."}'
```

将脚本及前两个参数作为现有 Hook 命令，由调用方追加 JSON 参数。超长通知会明确报错，不会截断内容。

## 开发与验证

```sh
cargo fmt --check
cargo clippy --locked --all-targets --features test-support -- -D warnings
cargo build --locked --features test-support
python3 tests/e2e.py
docker build -t telegram-bot-gateway .
python3 tests/container_smoke.py --image telegram-bot-gateway
```

E2E 使用真实 CLI/Gateway 进程、HTTP 和 SQLite，对接可控制故障的 Telegram HTTP 替身，覆盖独立消费、游标、@/静音规则、wait 取消、接入、重启恢复、429、发送响应丢失和回执被拒绝。`target/e2e/` 保存报告、日志和测试数据库。这些是模拟集成结果，尚未验证真实 Telegram。`test-support` 仅用于测试配置和 API 地址覆盖，容器构建不启用该 feature。

原生 `tbg-gateway` 读取 `~/.config/tbg/server.yaml`，未指定 `data_dir` 时使用 `~/.local/share/tbg`。Server 修改配置后手动重启；CLI 每次调用重新读取 Client 配置。实现计划和能力边界记录在 [Issue #7](https://github.com/JoverZhang/telegram-bot-gateway/issues/7)。
