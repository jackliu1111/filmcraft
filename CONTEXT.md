# FilmCraft AI 选片原型：项目交接

2026-10-16。后续工作目录为本 FilmCraft 仓库。先读 [AGENTS.md](AGENTS.md)，再读 [AI 选片资料入口](docs/ai-selection/README.md)。

## 当前状态

- 已准备原型阶段开发约定；上游规范作为参考，允许 Python、FFmpeg 和网页演示。
- Q1–Q19 设计、术语、相关决策、网页原型及预览素材、12 张工单快照、复用核对记录已交接到本仓库。
- 原生底座为 `c2f65871e998cca4700a74dbcf9348cab51fb06b`。当前 `codex/ai-selection-integration` 已接入共享分析、双分类子行、独立范围、连续声画编辑、范围拖放、分轨选择与事件映射；进度和证据见 [实现与验收](docs/ai-selection/implementation/README.md)。旧 `71438c627` 接入仅保留为迁移参考补丁。
- Python 共享引擎仍在相邻 Apex 仓库，通过 [AI 接入入口](integrations/apex-ai/README.md)调用；路径可由 APEX_SERVICE_ROOT 调整。
- GitHub 工单仍在 Apex 的 #21–#32；当前 fork 尚未开启 Issues。当前本地文件交接不等于已经转移远端工单或完成原生功能。

## 下一步入口

1. [方案与验收](docs/ai-selection/design/recognition-workspace.md)：按最终 Q1–Q19 实现，不回退到旧多轨或源面板识别行。
2. [工单与依赖](docs/ai-selection/tickets/README.md)：01／#21 与 02／#22 是已发布时的初始前沿；执行前读实时状态。
3. [交接与运行](docs/ai-selection/handoff.md)：新目录下启动网页原型、配置后端及检查接入。
4. [复用范围](docs/ai-selection/reviews/native-capability-audit.md)：已有原生操作优先复用；该记录基于旧版本，按需核对新底座差异。

当前统一集成 PR 为 [FilmCraft #1](https://github.com/jackliu1111/filmcraft/pull/1)。用户暂无真实游戏声／麦克风分轨录像；受控双轨机制通过不等于该真实素材验收。权重、完整录像、模型数据和本地构建产物不随源码上传。
