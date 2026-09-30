# CLI 到 HTTP 的映射

状态：部分实现，当前范围见[项目 README](../../README.zh-CN.md)。[English](http.md)

HTTP 接口由[操作能力总览](README.zh-CN.md)中的 CLI 命令推导。命令含义、校验和返回字段沿用[通信规范](communication.zh-CN.md)与[管理规范](operations.zh-CN.md)，本文只定义通用映射。

## 请求

统一使用 `POST http://<host>:<port>/v1/<命令路径>`，请求与响应均为 `application/json`。host 和 port 按管理规范显式配置；CLI 帮助与连接配置由本地处理。

| CLI 元素 | HTTP 映射 |
|---|---|
| 命令词 | 用 `/` 连接成路径，如 `topic create` → `/v1/topic/create`；参数不进入路径。 |
| `--agent <name>` | JSON 的 `agent` 字段；必填规则与 CLI 一致，`agent register` 不要求该字段。 |
| 位置参数 | 按 CLI 占位名作为 JSON 字段，如 `<topic>`、`<content>`。 |
| 带值选项 | 去掉 `--`，名称中的 `-` 转为 `_`；如 `--quote <msg_id>` → `quote`。 |
| 无值开关 | 出现时传 `true`；`false` 等同于未传该开关。 |

请求体为一个 JSON 对象。字符串、整数、布尔值和对象按 CLI 参数类型传递；省略的可选参数沿用 CLI 行为。空字符串或 null 不会被当作省略，也不自动修正类型或内容。HTTP 只接受总览定义的业务命令及其参数。

下面两个例子展示命令层级和参数映射，JSON 为阅读方便而格式化：

```text
$ tbg agent register --name hopeful_morse
→ POST /v1/agent/register
{
  "name": "hopeful_morse"
}

$ tbg --agent hopeful_morse send <topic> "@calm_turing 请复核结果" --quote m42
→ POST /v1/send
{
  "agent": "hopeful_morse",
  "topic": "<topic>",
  "content": "@calm_turing 请复核结果",
  "quote": "m42"
}
```

## 按钮与编辑请求

HTTP 中的 `reply_markup` 是 JSON 对象，不能传 JSON 编码后的字符串。CLI 会先解析 JSON 参数，再发送请求。例如：

```json
{
  "agent": "hopeful_morse",
  "topic": "<topic>",
  "content": "Page 1 of 2: A, B",
  "reply_markup": {
    "inline_keyboard": [[{"text": "Next", "callback_data": "pg_7f2a"}]]
  }
}
```

将该请求体发送到 `POST /v1/send`。以下路径沿用同一映射，均要求 `agent`：

| 路径 | 其他必填字段 | 可选字段 | 成功结果 |
|---|---|---|---|
| `/v1/send` | `topic`, `content` | `quote`, `format`, `no_header`, `reply_markup` | `{"msg_id":"m42"}` |
| `/v1/callback/list` | 无 | `topic`, `cursor`, `limit` | `{"callbacks":[...],"next_cursor":"cq-73","remaining_count":0}` |
| `/v1/callback/answer` | `callback_query_id` | `text`, `show_alert` | `{"callback_query_id":"cq-73"}` |
| `/v1/edit/text` | `topic`, `msg_id`, `content` | `format`, `no_header`, `reply_markup` | `{"msg_id":"m42"}` |
| `/v1/edit/markup` | `topic`, `msg_id`, `reply_markup` | 无 | `{"msg_id":"m42"}` |

回调请求中的 `cursor` 是 `callback_query_id`，不能使用对话的 `msg_id`。正文编辑省略 `reply_markup` 时保留已保存的键盘，传 `{"inline_keyboard":[]}` 则清除键盘；两种情况都不能用 null 表示。回调回答和编辑同步调用 Telegram，不做持久化重试。校验规则、回调字段、消息归属、就绪条件及结果不确定时的处理见[按钮接口契约](buttons.zh-CN.md)。

## 响应

成功返回 `200`，响应体就是 CLI 规范中的 JSON 对象，不增加包装层。CLI 将其作为紧凑 JSON 输出到 stdout，末尾追加换行。

参数错误或业务拒绝使用 `4xx`，服务端或依赖故障使用 `5xx`，错误体统一为一个包含 `error` 字符串的 JSON 对象。状态码遵循 [HTTP 语义](https://www.rfc-editor.org/rfc/rfc9110.html#section-15)。例如同一 Agent 已有 wait 时返回 `409`：

```json
{
  "error": "This Agent already has an active wait."
}
```

CLI 将错误说明写入 stderr，并以非零状态退出；未收到完整 HTTP 响应时报告连接或读取失败。CLI 不自动重试，调用方不能将响应缺失视为命令未执行。Gateway 对已持久接收消息的投递重试，遵循[通信规范](communication.zh-CN.md)。

## wait

wait 保持一次 HTTP 请求，直到通信规范中的结束条件满足；省略 `timeout` 时持续等待。正常等待超时仍返回 `200` 和既有的 `topics` 空数组结果。

客户端取消请求或连接中断后，Gateway 在检测到请求断开时结束该 wait，释放该 Agent 的等待位置，不自动 ack。其他命令仍按各自规则执行。

HTTP 接口不做鉴权，不要求访问 token。能够连接 Gateway 的调用方可以注册 Agent，并通过 `agent` 选择已注册身份；Telegram User 的信任规则仍按管理规范执行。Agent 归属校验仅保护路由，不构成身份认证；接口应仅在本机或受控私有网络中开放。
