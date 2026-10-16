# Apex AI 接入：FilmCraft 工作目录入口

2026-10-16。本目录提供共享分析引擎的路径入口和迁移前原生接入资料。当前 FilmCraft Rust 主源码仍是新上游底座，旧 AI 补丁尚未应用；不能仅因本目录存在就宣称原生 AI 界面已经接通。

## 共享引擎

Python 模型核心仍在外部 Apex 仓库，不复制第二份到 FilmCraft。本地默认使用相邻的 `../apex-fps-mvp`，也可以设置 `APEX_SERVICE_ROOT`。入口通过环境和相对仓库位置解析，不依赖启动时的当前目录。

[backend.json](backend.json)记录参考仓库和提交。当前是开发工作区接入，调用实际检出的代码；修改后端后应记录新提交，不能把该记录误当作已经安装的固定版本包。

`bridge.py` 转发到外部已有 `integrations/filmcraft/bridge.py`，并在后端清理结束后通过同一 `ModelService` 的校验和分页完整分数接口补充人声类别。模型推理与缓存仍由共享核心管理；FilmCraft 适配层拥有明确版本化的声音后处理策略，见 [#23 实现说明](../../docs/ai-selection/implementation/23.md)。stdout 保持 NDJSON，导入期日志转到 stderr。外部路径不存在时返回明确错误，不偷偷改用另一份引擎。

使用现有模型 Python 环境，在 FilmCraft 根目录执行：

```sh
../apex-voice-detect/.venv-pretrainedsed/bin/python integrations/apex-ai/bridge.py probe --input "/绝对路径/录像.mkv"
../apex-voice-detect/.venv-pretrainedsed/bin/python integrations/apex-ai/bridge.py analyze --input "/绝对路径/录像.mkv" --audio-stream 1
```

分析音轨编号来自 probe 结果，不应把示例中的 1 当作所有录像的音轨。模型运行环境沿用已有配置。

| 环境变量 | 默认行为 |
| --- | --- |
| APEX_SERVICE_ROOT | 当前 FilmCraft 仓库相邻的 apex-fps-mvp |
| APEX_MODEL_WORKSPACE | 相邻的 apex-voice-detect |
| APEX_AI_DATA_DIR | 当前 FilmCraft 下 outputs/apex-ai/model-data |
| APEX_ENGINE_PYTHON、APEX_CHECKPOINT | 由现有引擎配置解析 |
| APEX_FFMPEG、APEX_FFPROBE | 由现有引擎配置解析 |

## 原生基线与下一步

[reference/apex-ai.patch](reference/apex-ai.patch)及[原上游信息](reference/upstream.json)对应 v0.7.2 的 `71438c627`，保留原生菜单、任务进度、取消、事件标记和原稿／副本实现。当前 FilmCraft 底座为 `c2f65871`，先核对适配，不能使用旧脚本重置或重新克隆本仓库。

已执行补丁预检：旧补丁无法直接应用到当前底座，冲突位于 `crates/ui-egui/src/lib.rs` 和 `menus.rs`。本轮保留参考补丁，原生 AI 功能仍待适配。

迁移前的有界验证见[原生接入基线](../../docs/ai-selection/integration-baseline.md)。后续先把旧接入适配为正常源码，再按[工单](../../docs/ai-selection/tickets/README.md)实现分析展示和连续剪辑轨。普通素材路径可以并行推进。

当前原生启动继续使用仓库现有命令：

```sh
cargo run -p filmcraft -- --empty
```

该命令启动当前编辑器，不承诺已经出现旧原型中的“AI 剪辑”按钮。旧 build.sh、run.sh 和 macOS 打包脚本绑定原 Apex 目录，本次没有作为新仓库启动入口复制。

## 验证

```sh
python3 -m unittest discover -s integrations/apex-ai/tests -p 'test_*.py'
python3 prototypes/ai-selection/serve.py --port 8791
```

路径入口测试覆盖跨目录调用、显式配置、缺失后端和 NDJSON 错误。网页演示和真实分析／原生编辑仍是不同的验证层次。
