# FilmCraft AI 选片原型：工作入口

先读 [AGENTS.md](AGENTS.md)，这是当前 fork 的开发约定。当前处于原型验证阶段，上游开发规范作为参考。

1. 先读 [CONTEXT.md](CONTEXT.md)，再阅读当前任务与用户最新确认的设计；涉及 AI、迁移或验证时读 [原型开发说明](docs/prototype-development.md)。
2. 检查已有原生能力和相关 crate 源码，完成本次可演示的改动。
3. 按影响范围验证，记录真实结果及未覆盖部分。

本原型允许 Python、FFmpeg 和网页演示，不要求每次改动跑上游全量质量门禁。

原生启动、平台依赖及控制通道示例见 [构建参考](docs/contributing.md)；自动化控制见 [控制协议](docs/control-protocol.md)。这些参考页中的历史工作流程不覆盖 AGENTS 的当前原型约定。
