# Apex AI 接入：FilmCraft 工作目录入口

2026-10-18。本目录提供当前共享分析引擎入口及迁移前参考资料。原生接入、选片与恢复实现和验收见[当前记录](../../docs/ai-selection/implementation/README.md)。

## 独立应用

macOS arm64 独立包携带冻结的 Python、固定提交的共享后端、模型代码与权重、FFmpeg/ffprobe。正常运行不需要源码目录、虚拟环境或 Homebrew。首次启动选择外接 APFS/HFS 数据目录，缓存和临时文件写到该目录；可使用外接盘上的 APFS 磁盘映像。包内资源保持只读，损坏或不兼容环境明确报错。当前实际交付要求 macOS 27.0，使用本机 ad-hoc 签名。

构建命令及来源记录见[冻结运行时](../../packaging/ai/README.md)、[macOS 打包](../../packaging/macos/README-ai.md)；独立包真实搬迁、推理和原生验收见[交付记录](../../docs/ai-selection/implementation/standalone-runtime.md)。以下部分继续说明源码开发入口，开发环境覆盖保持可用。

## 共享引擎

Python 模型核心源码仍在外部 Apex 仓库维护；独立包构建时复制固定版本，不在 FilmCraft 维护另一套算法。源码开发默认使用相邻的 `../apex-fps-mvp`，也可以设置 `APEX_SERVICE_ROOT`。入口通过环境和相对仓库位置解析，不依赖启动时的当前目录。

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

## 已存分析关联的只读核对

```sh
../apex-voice-detect/.venv-pretrainedsed/bin/python integrations/apex-ai/bridge.py validate --input "/绝对路径/录像.mkv" --audio-stream 1 --result-id COMPLETED_PREDICTION_ID
```

此入口只核对精确关联、完成任务、完整分数和共享核心当前身份，不提交推理；失败输出 error，没有分析 fallback。成功输出 validation，不替换工程中的人工工作。结果缺失或损坏时，原生界面保留已保存事件并提供核对／主动重新分析。仍遵守单 worker 和服务目录锁。详细边界及命令见 [#31](../../docs/ai-selection/implementation/31.md)。

## 原生接入与历史参考

[reference/apex-ai.patch](reference/apex-ai.patch)及[原上游信息](reference/upstream.json)对应 v0.7.2 的 `71438c627`，保留原生菜单、任务进度、取消、事件标记和原稿／副本实现。当前 FilmCraft 底座为 `c2f65871`，先核对适配，不能使用旧脚本重置或重新克隆本仓库。

原生 AI 功能现已在正常源码实现；历史补丁仅供迁移对照，不需要再次应用。

迁移前的有界验证见[原生接入基线](../../docs/ai-selection/integration-baseline.md)。当前工作按[工单](../../docs/ai-selection/tickets/README.md)及实现验收记录推进。

当前原生启动继续使用仓库现有命令：

```sh
cargo run -p filmcraft -- --empty
```

该命令启动当前原生编辑器；分析功能还需正确配置共享模型环境。旧 build.sh、run.sh 和 macOS 打包脚本绑定原 Apex 目录，本次没有作为新仓库启动入口复制。

## 验证

```sh
python3 -m unittest discover -s integrations/apex-ai/tests -p 'test_*.py'
python3 prototypes/ai-selection/serve.py --port 8791
```

路径入口测试覆盖跨目录调用、显式配置、缺失后端和 NDJSON 错误。网页演示和真实分析／原生编辑仍是不同的验证层次。
