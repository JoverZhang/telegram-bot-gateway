# Codex 通知插件

[English](README.md)

这是原生 Codex 插件：主会话每轮停止时，通过 `tbg` 发送本轮最终回答。它不读取 Telegram 回复、不唤起 Agent、不报告子 Agent，也不将本轮结束视为整个任务完成。首版不包含 Claude 适配。

## 安装与验证

需要 Linux/macOS、Python 3、支持 `plugin` 命令和可信插件 Hooks 的 Codex CLI（已用 0.157.1 验证），以及本版本的 `tbg` CLI 和 Gateway（需要支持 Markdown 发送）。使用 `cargo build --release --bin tbg` 构建并安装到 PATH。先配置 `~/.config/tbg/client.yaml`，并在运行中的 Gateway 上准备一个已存在、未关闭的 Topic。

```sh
./integrations/codex/install.sh --topic <topic_id>
# 也可以明确指定可执行文件：
./integrations/codex/install.sh --topic <topic_id> --tbg /absolute/path/to/tbg
./integrations/codex/install.sh --status
./integrations/codex/install.sh --test
./integrations/codex/install.sh --uninstall
```

安装器将附带的 marketplace 复制到稳定的用户数据目录，再调用 `codex plugin marketplace add` 和 `codex plugin add`。重复安装更新目标和插件，不重复添加 Hook；CLI 路径解析后保存为绝对路径，后续安装沿用，除非用 `--tbg` 修改。其他 Codex 插件和 Hooks 保持原样。安装器不会开启全局 Hook 功能，也不会绕过 Codex 的信任确认：如果 Hooks 未开启，需要在 Codex 开启，审阅并信任本插件，然后新建会话。

`--status` 显示原生插件注册状态、配置，并用已有 Session 绑定查询 Topic。尚无绑定时，会明确报告未验证连通性。`--test` 注册一个独立测试 Agent 并发送测试提示；成功表示 Gateway 接受消息，不表示 Telegram 已收到，需要在 Telegram 查看。只有真实 Codex 会话才能验证事件触发和 Hook 信任是否正常。

卸载移除原生插件及其缓存，保留专用 marketplace 源、配置、Session 绑定、日志和 Gateway 记录，便于重新安装；不会清理其他配置。已有会话可能保留已加载的 Hook，卸载后应重启这些会话。插件不会停用旧通知。试运行期间保留旧通道，确认真实投递后再移除旧通知入口。

## 行为与状态

```text
Stop → 持久化的 Session/Agent 绑定 → tbg send → Gateway 持久化投递队列 → Telegram
```

每个 Codex `session_id` 首次使用时自动注册 Agent，后续进程和恢复后的会话复用绑定。Session 使用哈希作为文件名，事件输入不能决定文件路径。同一 Session 并发执行 Hook 时，忙锁会报告失败，避免重复注册或阻塞 Codex。通知不会建立订阅。

通知使用 Markdown，在 Telegram 中渲染显示：

```markdown
**Codex 本轮完成**
项目：telegram-bot-gateway
#agent_2f82684127a1

已**完成**重构。

- 添加了测试
- 更新了文档
```

正文取自 Stop 事件的 `last_assistant_message`。Stop 当前不提供会话标题，因此标题使用 `Codex 本轮完成`。项目名取 Git 原仓库名，linked worktree 也能识别；Git 查询失败时使用工作目录名。Session 的 Agent 以 hashtag 显示，不再重复添加 Agent 标头。最终回答缺失、为 null 或空白时，发送 `Codex turn finished (no final response).`。

插件使用 `send --format markdown --no-header`。Gateway 将 Markdown 转为安全的 Telegram HTML，渲染加粗、斜体、链接、行内代码和代码块，并转义原始 HTML。引用使用文本前缀；样式或链接中的行内代码保留外层样式。历史记录保存完整原始 Markdown；超出 Telegram 限制时，仅截断展示副本并标记 `…（已截断）`，保留前面的标头，确保 HTML 标签完整。长度保守地按 HTML 源码的 UTF-16 单元计算，格式较多时可能提早截断。普通纯文本发送仍拒绝超长消息。

一次 Stop 调用发送一条通知；重放事件可能重复发送。注册结果不明确时可能遗留未使用的 Agent。不承诺恰好一次，不自动重试发送，也没有本地离线队列。

状态保存在 `${XDG_DATA_HOME:-~/.local/share}/tbg/codex/`，独立于插件版本缓存：

```text
config.json           # topic 和 tbg_path 绝对路径
sessions/             # Session 哈希、Agent 绑定、进程锁
notifications.jsonl   # 时间、结果、Session、已接受的 msg_id 或错误类型
marketplace/          # 稳定的插件安装源
```

删除 Session 状态会导致下次使用时注册新身份。不同 Gateway 应使用不同数据目录；绑定属于注册时的 Gateway。这里不保存 Bot Token，日志不记录通知正文。

项目查询、注册与发送共享三秒预算。Git 查询最多半秒，失败不影响通知。适配器通过 `tbg --request-timeout-ms <正整数>` 传递剩余预算，客户端限制 HTTP 请求及响应体读取时间。Codex 的五秒 Hook 超时作为最后保护。失败会记录日志，适配器以成功状态退出并输出 `{}`，不阻止 Stop，也不注入模型指令。Gateway 接受后才有持久投递保证，接受前失败可能丢失通知。请求超时也可能已经提交，因此插件不会盲目重试。

失败记录包含出错步骤及 CLI 退出码、HTTP 状态等诊断信息。日志无法写入时会单独报告到 stderr，已接受的发送不会因此被报告为投递失败。

## 验证改动

```sh
cargo build --locked --features test-support
python3 tests/e2e.py
```

测试覆盖适配器、真实 CLI、Gateway、SQLite 和 Telegram 测试服务。PATH 存在 `codex` 时，还会在隔离的 Codex 配置中实际安装、重复安装和卸载插件；没有 Codex 时会明确报告这部分未经验证。产物为 `target/e2e/codex-plugin.json` 和 `codex-notifications.jsonl`。这些检查不调用模型，也不能证明真实 Codex Stop 事件已触发。
