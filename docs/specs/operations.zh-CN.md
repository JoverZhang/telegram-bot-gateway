# 管理规范

状态：规划中，行为规范，尚未实现。[English](operations.md)

本文描述如何建立通信环境、管理参与者，以及发现通信异常。命令入口见[操作能力总览](README.zh-CN.md)，消息消费规则见[通信规范](communication.zh-CN.md)。以下场景均为预期行为；CLI 实际输出紧凑 JSON，文档示例通过 `jq .` 格式化展示。

## 首次启动与管理员身份

Client 与 Server 通过 HTTP 通信，各使用一份用户级全局 YAML 配置。`~` 指运行相应程序的系统用户主目录；同一系统用户下的所有项目和 Agent 共用 Client 配置，Agent 身份仍通过 `--agent <name>` 指定。两端的端口都必须显式配置，以下 `18473` 仅为示例值。

```yaml
# ~/.config/tbg/client.yaml
host: "127.0.0.1"
port: 18473
```

Client 必须显式填写 Gateway 的 IP（`host`）和 `port`；缺少任一项时报配置错误，不发起连接。CLI 每次调用读取该文件，已经运行的 wait 保持原连接。本例连接 `http://127.0.0.1:18473`。

```yaml
# ~/.config/tbg/server.yaml
listen: "0.0.0.0:18473"
telegram:
  bot_token: "<bot_token>"
admins:
  - 12345678
```

Server 必须通过 `listen` 显式指定监听 IP 和端口，缺少时启动失败；修改配置后手动重启 Gateway 生效。端口被占用时启动失败并报告监听地址，不自动换端口。Client 的 `port` 填写 Gateway 实际对外提供的端口。

User 的管理员身份来自 Server 配置；Bot 的群管理员权限由 Telegram 群设置授予。User 先配置 Bot token，通过私聊 Bot 的 `/whoami` 获取自己的 user_id，再写入管理员名单并手动重启 Gateway。

尚未配置管理员时，Bot 允许 `/help` 和 `/whoami`，提示完成初始化；管理和 Group 接入暂不可用。

```text
# Bot 已连接 Telegram，管理员名单为空。
User → Bot：/whoami
Bot：
  user_id: 12345678
  身份：未信任
  尚未配置管理员，请将此 user_id 写入 Gateway 的管理员名单。

# User 将 user_id 写入 server.yaml，并手动重启 Gateway 后。
User → Bot：/whoami
Bot：
  user_id: 12345678
  身份：管理员

User → Bot：/manage
Bot：
  当前位置：与 Bot 的私聊
  操作范围：全局
  [状态总览] [Agent] [Group] [Topic]
  [User 信任] [Doctor] [个人设置]
```

## Agent 身份与 CLI 状态

Agent 通过 `agent register [--name <name>]` 注册。仅省略 `--name` 时自动生成名称。所有 Agent 名称必须完整匹配以下正则：

```regex
^[A-Za-z][A-Za-z0-9_]*$
```

名称以英文字母开头，后续只允许英文字母、数字和下划线。显式传入的值按原样校验；不匹配或名称冲突均报错，不自动修剪、替换或改名。注册失败不改变已有 Agent 的状态。

注册成功后使用返回的唯一名称；同一名称对应同一份订阅、静音设置和 ack 进度，由 Gateway 保存，CLI 不另存一份消费边界。

```text
# 指定名称。
$ tbg agent register --name hopeful_morse | jq .
{
  "name": "hopeful_morse"
}

# 仅省略 --name 时自动生成。
$ tbg agent register | jq .
{
  "name": "calm_turing"
}

$ tbg agent register --name hopeful_morse
# 注册失败：名称已被使用，已有 Agent 的状态不变。

$ tbg --agent hopeful_morse whoami | jq .
{
  "name": "hopeful_morse"
}

# 两个 CLI 共用 Client 配置，使用同一身份时也共享消费状态。
CLI A: tbg --agent hopeful_morse subscribe <topic>
CLI B: tbg --agent hopeful_morse subscriptions | jq .
       {
         "subscriptions": [
           {
             "topic": "<topic>",
             "last_acked_msg_id": "m41"
           }
         ]
       }
```

本例假设订阅时最新消息为 m41。CLI 进程退出后，身份、订阅和消费进度仍保存在 Gateway；新进程携带相同名称即可继续使用，无需重新注册。需要独立消费进度的 Agent 分别注册；读取、确认及恢复规则沿用通信规范。

## User 信任

| 身份 | 能力 |
|---|---|
| 管理员 | 管理 User 信任、接入 Group、管理全部 Topic，并在私聊中查看全局状态。 |
| 普通可信 User | 参与对话，查看相关状态。 |
| 未信任 User | 使用 `/help` 和 `/whoami`；普通发言直接忽略，不存入 DB，不进入 unread/history，也不触发 wait。 |

管理员从菜单直接授予或撤销普通 User 的信任，以 user_id 指定对象。

```text
# 管理员与 Bot 私聊。
/manage → [User 信任] → [授予]
Bot：请输入 Telegram user_id。
管理员：87654321
Bot：已授予 87654321 信任。

/manage → [User 信任] → [87654321] → [撤销]
Bot：已撤销 87654321 的信任。
```

撤销信任仅影响生效后接收的新消息和新请求：普通发言直接忽略，受限操作拒绝执行。此前已接收的消息继续保留、读取和处理，已受理的操作继续执行，消费进度不变。

```text
撤销前：可信 User 的消息 m42 已被接收，Agent 尚未 ack。
管理员撤销该 User 的信任。
撤销后：该 User 的新发言直接忽略；m42 仍可读取和 ack。
```

## 接入 Group 与管理 Topic

Gateway 管理员将 Bot 加入 Group 后，Gateway 自动登记。其他 User 邀请 Bot 时暂不接入对话；Gateway 管理员在该 Group 输入 `/manage` 后完成接入，并打开当前位置的菜单。接入前的普通对话直接忽略，不补录。

Doctor 在该 Group 的菜单中展示通信条件及缺少的权限。这里的接入权限来自 Gateway 管理员身份，与 Telegram 的群管理员身份分别判断。

```text
# 管理员将 Bot 加入已启用 Topic 的 Group「项目协作」。
管理员在 Group 中输入：/manage
Bot：
  当前位置：Group「项目协作」
  操作范围：当前 Group
  [状态总览] [Agent] [Topic] [Doctor] [个人设置]

管理员点击：[Doctor]
# 查看权限与诊断建议，示例见下文。

# 另一个 Group 由非 Gateway 管理员邀请 Bot。
User：请开始讨论。
# Group 尚未接入，普通发言被忽略。

Gateway 管理员在该 Group 中输入：/manage
# 完成接入并打开菜单；此后按 User 信任规则接收对话。
```

Agent 可以在已接入的 Group 中自动创建 Topic，并关闭或重新打开自己创建的 Topic；管理员可通过菜单管理全部 Topic。

```text
$ tbg --agent hopeful_morse topic create --group <group> --name "设计讨论" | jq .
{
  "topic": "<topic>",
  "name": "设计讨论",
  "status": "active"
}

$ tbg --agent hopeful_morse topic close <topic> | jq .
{
  "topic": "<topic>",
  "status": "closed"
}

$ tbg --agent hopeful_morse topic reopen <topic>
# Topic 重新处于 active 状态。
```

Topic 使用 `active` 和 `closed` 表示开放与关闭。关闭只限制发送，其他操作沿用通信规范。

| 操作或状态 | 关闭后的行为 |
|---|---|
| Agent 发送消息 | 拒绝发送，提示 Topic 已关闭。 |
| history、unread、ack | 继续允许，便于查看和处理已有对话。 |
| subscribe | 继续允许；首次订阅从当时的最新位置开始，恢复订阅沿用原进度。 |
| 订阅、静音设置和消费边界 | 持续保留。 |
| wait | 符合条件的待处理消息照常使其返回，否则继续等待；关闭和重开本身不触发、也不终止 wait。 |

直接在 Telegram 中关闭或重开 Topic 时，Gateway 收到状态通知后同步状态，并采用相同规则。状态通知作为管理记录保存，不进入对话。Telegram 提供了相应的 [Topic 状态通知](https://core.telegram.org/bots/api#message)。

Bot 被移出 Group 后，全局菜单和 `group list` 将该 Group 标为不可用，保留已有历史、订阅和消费进度。需要访问该 Group 的 Telegram 操作报错；本地读取、ack 和 wait 仍沿用原规则。重新加入时沿用上述接入流程，接入后继续使用保存的状态。

## 菜单范围与运行状态

`/manage` 跟随当前位置，页面始终显示操作范围。在 Topic 内查看 Agent 时展示当前 Topic 的订阅者；管理员在与 Bot 的私聊中查看全局。完整菜单目录见操作能力总览。

```text
# 在 Topic「设计讨论」中打开菜单。
/manage → [Agent]
当前位置：Group「项目协作」→ Topic「设计讨论」
操作范围：当前 Topic
[全部] [活跃] [等待中]

hopeful_morse   活跃，正在等待当前 Topic
calm_turing     活跃，未等待
[返回]
```

`active` 表示该 Agent 最近 5 分钟内成功调用过 Gateway，或当前仍在 wait；它描述通信活动，不代表 Agent 正在执行任务。`waiting` 表示该 Agent 当前有一个尚未结束的 wait；同一 Agent 只能有一个 wait，等待期间持续计入活跃。

Topic 内的 Agent 列表限于当前订阅者，Group 内为其各 Topic 订阅者的并集；全局展示所有已注册 Agent。Topic/Group 页面中的“等待中”只统计 wait 当前覆盖该范围至少一个 Topic 的 Agent，全局统计所有正在 wait 的 Agent；每个 Agent 计数一次。默认 wait 随当前订阅变化，指定 Topic 的 wait 在 Topic/Group 页面只计入对应范围。

```text
# 管理员在私聊中查看全局；数量仅为示例。
/manage → [状态总览]
Bot：已连接 Telegram
Agent：共 4 个，活跃 2 个，其中等待中 1 个
Group：已接入 2 个
Topic：active 3 个，closed 1 个
[Agent] [Group] [Topic] [Doctor] [返回]

/manage → [Group]
项目协作
日常任务
[返回]

/manage → [Topic]
[active] [closed]
```

菜单只允许打开者操作，每次点击都重新检查其当前权限。页面可以展示此前状态，操作结果以点击时的权限和状态为准。其他 User 点击时提示其通过 `/manage` 打开自己的菜单，原菜单的范围和语言保持不变。

```text
User A 打开 Topic 内的 /manage 菜单。
User B 点击该菜单按钮。
Bot 提示 User B：请先通过 /manage 打开自己的菜单。
# User A 的菜单保持不变。
```

## Doctor 与语言设置

Doctor 从 `/manage` 进入，说明检查范围、发现的问题及下一步操作。Group 中创建 Topic 需要 Bot 拥有管理员身份和 `can_manage_topics` 权限；普通群消息的可见性还与管理员身份及 Privacy Mode 有关。依据：[Topic 创建要求](https://core.telegram.org/bots/api#createforumtopic)、[群消息接收规则](https://core.telegram.org/bots/faq#what-messages-will-my-bot-get)。

```text
/manage → [Doctor]
检查范围：Group「项目协作」

Topic 功能：已启用
Bot 管理员：否
创建 Topic：不可用

建议：将 Bot 设为管理员，并授予“管理话题”权限。
[重新检查] [返回]

# 权限补齐后点击“重新检查”。
Bot 管理员：是
管理话题权限：已授予
创建 Topic：权限条件满足
```

诊断区分权限条件满足与实际通信成功。Doctor 展示连接状态和权限检查结果；实际收发结果来自正常通信，不自动发送探测消息。Telegram 不可用时，本地诊断通过 Gateway 的启动与运行日志提供，Docker 部署时查看容器日志。

Doctor 展示待投递消息、待发送 ❤️ 回执及阻塞原因，能够定位到 Topic 和 `msg_id`。Group 未允许 ❤️ 时提示调整 reaction 设置；回执失败不影响已保存的 ack。恢复通信或权限后继续处理，诊断结果同时写入 Gateway 日志。

| 场景 | 反馈 |
|---|---|
| Bot token 缺失或无效 | Gateway 日志说明配置问题，供本地排查。 |
| CLI 无法连接 Gateway | CLI 向 stderr 报告实际使用的 HTTP 地址和连接失败原因，并以非零状态退出。 |
| Bot 在 Group 内缺少权限 | `/manage → Doctor` 指出缺少的权限与补齐步骤。 |

诊断输出不包含 Bot token。

语言设置按 User 保存，默认跟随 Telegram，也可手动选择；菜单使用打开者的语言。语言字段缺失时沿用上次记录，无法选择支持的语言时回退到 English。名称与对话正文保留原文。

```text
/manage → [个人设置] → [语言]
当前：跟随 Telegram（简体中文）
[跟随 Telegram] [简体中文] [English]

User 点击：[English]
Language: English
[Follow Telegram] [简体中文] [English]
```

管理命令、菜单操作、Bot 的管理回复及结果保存在 DB，不进入 Agent 的 unread/history，不触发 wait。

本文定义可观察行为。请求与响应的转换见 [HTTP 映射规范](http.zh-CN.md)；实现与部署计划记录于 GitHub issue。
