# AI 识别与选片：开发入口

本目录汇集继续开发所需的设计、术语、决策、工单和核对资料。原型阶段约定见 [AGENTS.md](../../AGENTS.md)。

| 要做的事 | 阅读或运行 |
| --- | --- |
| 了解项目现状 | [项目交接](../../CONTEXT.md)、[交接说明](handoff.md) |
| 实现已确认交互 | [Q1–Q19 方案](design/recognition-workspace.md)、[术语表](GLOSSARY.md) |
| 查看长期行为约定 | [原稿与编辑副本](decisions/0003-preserve-ai-draft-and-editing-copy.md)、[事件与范围](decisions/0004-separate-recognition-events-and-clip-selection.md)、[目标剪辑轨](decisions/0005-target-track-controls-contiguous-editing.md) |
| 体验网页交互 | [网页原型](../../prototypes/ai-selection/README.md) |
| 开发与验收 | [12 张工单](tickets/README.md)、[GitHub 编号及依赖](tickets/published.md) |
| 避免重复开发 | [原生能力复用](reviews/native-capability-audit.md)、[原型缺口](reviews/prototype-compatibility.md) |
| 接共享 Python 引擎 | [AI 接入入口](../../integrations/apex-ai/README.md) |
| 理解迁移前实现 | [原生接入基线](integration-baseline.md)、[参考补丁](../../integrations/apex-ai/reference/apex-ai.patch) |

网页原型使用演示识别数据和静音预览。旧原生基线提供历史实现与验证，不代表新底座已经接入。工单远端仍在 Apex；共享模型核心继续从外部复用。

迁移来源、文件校验和及未复制的历史本机证据见 [migration.json](migration.json)。后续原型设计与 FilmCraft 端实现优先在本仓库维护；共享分析算法仍在 Apex 修改，记录对应提交。
