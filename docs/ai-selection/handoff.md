# 切换到 FilmCraft 项目的交接说明

2026-10-16。本次准备本地开发资料与路径入口，后续项目工作目录切换到 `/Users/jackliu/game/filmcraft`。

## 文件位置

- `CONTEXT.md`：当前状态和下一步入口。
- `docs/ai-selection/`：设计、术语、决策、已发布工单快照与复用核对。
- `prototypes/ai-selection/`：当前单轨网页原型、预览 MP4、海报及历史截图记录。
- `integrations/apex-ai/`：共享 Python 引擎入口、配置说明和入口测试。
- `integrations/apex-ai/reference/`：迁移前原生补丁及其固定上游信息。

源码和小型预览素材已实际复制，链接已按新位置调整。原录像、权重、分析缓存、旧 target 构建目录和本机 App 没有复制到新仓库。

## 从新项目启动网页原型

在 FilmCraft 根目录执行：

```sh
python3 prototypes/ai-selection/serve.py --port 8791
```

打开 [网页原型](http://127.0.0.1:8791/?variant=A)。如端口被旧原型占用，可加 `--port 8792`。这是独立浏览器演示，不是 FilmCraft 原生界面。

## 共享分析引擎

新入口默认查找相邻的 `../apex-fps-mvp`；迁移或调整目录后设置 `APEX_SERVICE_ROOT` 即可。模型环境仍使用相邻 `../apex-voice-detect` 或已有配置，详见 [AI 接入说明](../../integrations/apex-ai/README.md)。

FilmCraft 端不另存模型核心副本。人声候选等后端修改仍在 Apex 中实施；该仓库须保持可用，并记录共享引擎版本。后续若要独立分发，再处理后端打包。

## 原生适配的起点

当前原生底座是 `c2f65871`，旧接入补丁基于 `71438c627`。参考补丁保留旧 AI 入口、任务和组稿能力，尚未应用。继续原生开发时先核对或适配旧实现，避免重复编写现有模型、缓存和组稿机制；也不要用旧 build.sh 把新底座重置为 v0.7.2。

已执行补丁预检：旧补丁无法直接应用到当前底座，冲突位于 `crates/ui-egui/src/lib.rs` 和 `menus.rs`。本轮保留参考补丁，原生 AI 功能仍待适配。

01／#21 需要把旧接入能力纳入新底座并解耦分析与组稿；02／#22 可先用普通素材验证新剪辑轨。不能因网页原型缺失重排，就假定原始桌面端没有重排能力。

## 工单与远端

12 张任务及 19 条阻塞关系仍位于 Apex 仓库，见[工单发布清单](tickets/published.md)。FilmCraft fork 的 Issues 尚未开启。开启并转移后，更新本目录的 manifest、编号和依赖映射，避免重复发布另一套任务。

本地迁移不包含推送、远端工单转移或项目侧栏切换。切换 Codex 工作目录后先读根 CONTEXT 与 AGENTS，就能从这套资料继续。

## 验证记录

实际复制校验、链接检查、网页服务、路径入口和真实 probe 的结果写入 [migration.json](migration.json)。网页截图、模型推理、原生 UI 和真实导出各自按覆盖范围验收。
