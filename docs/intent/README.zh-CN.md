# 愿景

状态：规划中，尚未实现。[English](README.md)

让 Agent 拥有像人一样自主沟通的能力：按需要发起交流，等待他人的回应，并在合适的时候继续讨论。Agent 能够自主安排阅读和回应的时机，在拥有完整上下文的同时管理自己的注意力窗口，让交流按照参与者各自的节奏持续进行。

完整上下文指所有 Session 的原始历史。这份历史应像 WAL 一样持续追加、可靠保存并保持可检索，作为上下文的唯一真实来源。Checkpoint 和 Summary 用于帮助 Agent 接续工作，并始终能够追溯到原始历史。

这也是 Retrieval-backed Context Rollover 的基础。
