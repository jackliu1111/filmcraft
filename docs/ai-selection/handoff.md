# 切换到 FilmCraft 项目的交接说明

2026-10-16。本文件保留本地迁移与运行入口；当前项目工作目录为 `/Users/jackliu/game/filmcraft`，原生实现与实际验证见[实现与验收](implementation/README.md)，完整流程覆盖和缺口见[#32矩阵](implementation/32.md)。

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

原生底座是 `c2f65871`，旧接入补丁基于 `71438c627`。参考补丁保留旧 AI 入口、任务和组稿能力，仅作迁移对照；当前集成分支已在正常源码实现共享分析、分类子行、独立范围、连续声画编辑、队列、当前范围组稿与恢复。不要再次应用旧补丁或用旧 build.sh 把新底座重置为 v0.7.2。

迁移时补丁预检曾发现 `crates/ui-egui/src/lib.rs` 和 `menus.rs` 冲突；该预检是历史记录，不是当前功能缺失结论。新增行为的自动回归、原生操作及媒体证据分别在实现目录记录。

分析完成只关联结果；组稿由用户主动触发，采用当前范围。普通素材与AI范围复用同一原生目标剪辑轨、撤销、保存和导出；网页演示不作为原生行为或实际编码验收。

## 工单与远端

12 张任务及 19 条阻塞关系的发布快照仍指向 Apex 仓库，见[工单发布清单](tickets/published.md)。当前代码集成入口为 [FilmCraft #1](https://github.com/jackliu1111/filmcraft/pull/1)；这不表示工单已转移或所有验收已完成。若以后迁移工单，再更新 manifest、编号和依赖映射，避免重复发布。

迁移记录不等于远端工单转移或项目侧栏切换。切换工作目录后先读根 CONTEXT 与 AGENTS，再读当前实现记录；未完成的真实素材、识别质量和效率验收保留为待验证。

## 验证记录

实际复制校验、链接检查、网页服务、路径入口和真实 probe 的结果写入 [migration.json](migration.json)。网页截图、模型推理、原生 UI 和真实导出各自按覆盖范围验收。
