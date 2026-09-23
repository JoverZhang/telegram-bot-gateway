# 操作能力总览

状态：规划中，接口草案，尚未实现。[English](README.md)

Agent 使用 `tbg` CLI，User 使用 Telegram Bot。本文汇总两个入口的能力；通信场景和错误边界见[通信规范](communication.zh-CN.md)，管理状态与交互见[管理规范草案](operations.zh-CN.md)。

## CLI：tbg --help

`tbg` 专供 Agent 使用。Agent 自助注册，获得自动生成的唯一名称；后续携带该名称操作。Agent 的内部 ID 不作为使用者需要管理的标识。同一名称在不同 CLI 中共享订阅、静音设置和 ack 进度，不同 Agent 的消费进度各自独立。

```text
tbg — Telegram Bot Gateway

USAGE
  tbg agent register
  tbg --agent <name> <command>

IDENTITY
  agent register                         注册并返回自动生成的唯一名称
  whoami                                 查看当前 Agent 身份
  agent list                             查看 Agent 及参与状态

TOPIC
  group list                             查看已接入的 Group
  topic list [--group <group>]            查看 Topic 及状态
  topic show <topic>                      查看 Topic 和订阅者
  topic create --group <group> --name <name>
                                         创建 Topic
  topic close <topic>                    关闭自己创建的 Topic
  topic reopen <topic>                   重新打开自己创建的 Topic

COMMUNICATION
  send <topic> "<content>" [--quote <msg_id>]
                                         发送消息；--quote 引用回复，正文支持 @
  unread <topic> [--cursor <msg_id>] [--limit <n>]
                                         从消费边界（最后一次 ack 的位置）之后顺序读取，可用 cursor 继续
  ack <topic> --through <msg_id>          累计确认到该消息，包含该消息

  history <topic> [--cursor <msg_id>] [--limit <n>]
                                         从最新消息倒序回看，可用 cursor 继续
  wait [--topic <topic>] [--timeout <seconds>]
                                         默认持续等待所有当前订阅，每个 Agent 只允许一个 wait

SUBSCRIPTION
  subscribe <topic> [--muted]             订阅新消息，或恢复已有订阅及原进度
  subscriptions                         查看订阅及已确认消息 ID
  mute <topic> [--off]                   开启静音；--off 取消静音
  unsubscribe <topic>                    停止订阅，保留进度
```

除帮助文本外，CLI 成功时向 stdout 输出一个完整的紧凑 JSON 对象，末尾追加换行。日志写入 stderr；需要格式化时通过管道交给 `jq .`，CLI 不提供 pretty 选项。消息正文完整保留，正文中的换行按 JSON 规则转义。接入层可将 stdout 原文作为 LLM 的工具结果文本，外层包装由 Agent runtime 决定。文档中的 JSON 结果按 `jq .` 格式化展示。

注册示例：

```text
$ tbg agent register | jq .
{
  "name": "hopeful_morse"
}
```

新 Agent 没有默认订阅。subscribe 管理订阅，首次从订阅生效后的新消息开始消费，新订阅默认 unmute。send 和 history 都不要求订阅，也不会自动建立订阅；history 可以读取订阅前的对话。unread 和 ack 要求当前已订阅该 Topic。default topic 暂时预留。

Topic 是共享对话空间。Reply 保留回应关系，@ 表达希望谁关注，两者都不改变消息对订阅者的可见性。静音只影响 wait 的提醒：静音时，仅明确 @ 当前 Agent 的消息触发 wait，其他消息仍可主动读取。

unread 和 history 默认最多返回 20 条，`--cursor` 排除指定消息，沿命令的读取方向继续。每条消息包含 `msg_id`、发送时间、发送者及完整正文。Agent 应先读完后续对话再回应，并显式 ack 已处理的消息；读取和发送都不自动确认。消息字段、分页返回、订阅恢复及 wait 场景见[通信规范](communication.zh-CN.md)。

## Telegram Bot：/help

User 在 Topic 中发言、Reply 或 @ Agent。查询与管理统一从 `/manage` 进入，通过按钮操作；菜单根据当前位置和 User 权限展示可用能力。

```text
/help       查看使用帮助
/whoami     查看自己的 Telegram user_id 和身份
/manage     打开当前位置的查询与管理菜单
```

菜单始终提示当前位置与操作范围。例如在 Topic 内打开：

```text
当前位置：Group「项目协作」→ Topic「设计讨论」
操作范围：当前 Topic
```

管理员在与 Bot 的私聊中可以进入全局管理。以下是完整能力目录；Topic 内的菜单按当前范围提供相关操作。

```text
/manage
├─ 状态总览
├─ Agent
│  └─ [全部] [活跃] [等待中]
├─ Group
├─ Topic
│  └─ 查看、关闭、重新打开
├─ User 信任（管理员）
│  └─ 查看、授予、撤销
├─ Doctor
│  └─ 检查通信条件并给出处理建议
└─ 个人设置
   └─ 语言
      └─ [跟随 Telegram] [简体中文] [English]
```

子页面提供返回按钮，筛选通过按钮更新列表。User 分为管理员和普通可信 User：管理员管理信任名单、Group 接入与全部 Topic；普通可信 User 参与对话、查看相关状态。

首次使用时，User 通过 `/whoami` 获取自己的 user_id，并将它写入配置文件的管理员名单。管理员随后可以在菜单中直接授予或撤销普通 User 的信任。管理员将 Bot 拉进 Group 后，网关自动登记，Doctor 展示可用能力及缺少的权限。

语言设置按 User 保存，默认跟随其 Telegram 语言，也可手动选择。语言字段缺失时沿用上次记录，无法选择支持的语言时回退到 English。菜单使用打开者的语言；本地化覆盖菜单、按钮、提示和诊断文字，名称与对话内容保留原文。语言判定参考 [Telegram 的语言支持约定](https://core.telegram.org/bots/features#language-support)。

## 详细规范

| 文件 | 职责 |
|---|---|
| [communication.zh-CN.md](communication.zh-CN.md) | Agent CLI 与 Telegram User 的通信场景：订阅、发送、读取、ack、wait、Reply/@、暂离与恢复。 |
| [operations.zh-CN.md](operations.zh-CN.md)（讨论草案） | 注册与身份、Bot 配置、User 信任、Group/Topic 管理、菜单交互、Doctor、状态及语言设置。 |

本轮先收敛 CLI 操作与 Telegram 交互。CLI → Gateway → Telegram 的职责、传输方式和状态存储留给后续架构文档。历史检索与 Checkpoint 暂不设计。
