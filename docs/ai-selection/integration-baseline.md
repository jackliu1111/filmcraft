# FilmCraft AI 本机原型：迁移前基线

本文件保留原 Apex 仓库的 v0.7.2 接入及历史验收记录。下面的 `outputs/`、构建和启动命令属于原 Apex 目录，不是当前新底座的可用入口。当前接入位置与适配状态见 [AI 接入说明](../../integrations/apex-ai/README.md)。

本机证据说明：本文中的 `outputs/` 路径均相对于仓库根目录，是当时本机验证记录或生成产物；该目录已被 Git 忽略，不随源码提交，也不是 GitHub 下载入口。

另有[新版选片交互原型](../../prototypes/ai-selection/README.md)，对应已确认的识别轨道与拖拽选片设计。它是可在浏览器体验的独立演示，使用预设标记；下文描述的原生模型接入仍是另一份原型。

已在 FilmCraft 0.7.2 原生界面接入本仓库的枪声模型服务。保留原生剪辑和导出形态，不把 HEVC 实验中的 50 Mbps 或其他参数写成默认值。

## 原生原型的本机使用

完成下方构建与启动器生成后，本机入口为 `outputs/filmcraft-ai-prototype/FilmCraft AI.app`。这是引用当前工作区、现有模型环境及权重的开发启动器，不是可移动到另一台机器的安装包。

1. 打开应用，点击右上角 **AI 剪辑**。
2. 选择录像，或使用项目中当前选中的素材；确认用于分析的游戏音轨。
3. 点击 **生成 AI 草稿**。首次分析保存原始预测；再次生成会校验并复用匹配结果。面板可收起，任务继续，也可取消。
4. 分析完成后，原片上出现 AI 事件标记，候选组装成连续 V1/A1 草稿。原稿单独保存，当前打开编辑副本，可用原生工具删段、调整边界和补段。
5. 复核后使用 FilmCraft 原生导出。默认参数和预设由 FilmCraft 决定；本次全新原型的 Quick Export 实际默认是 H.264 的 Match Source – Adaptive High Bitrate，并非自定义 H.265 档位。

当时本机已准备演示编辑副本（`outputs/filmcraft-ai-prototype/demo/ai-edit-copy.fcproj`）、AI 原稿（`outputs/filmcraft-ai-prototype/demo/ai-original.fcproj`）和识别证据与原片映射（`outputs/filmcraft-ai-prototype/demo/ai-evidence.json`）。原录像保持在原位置。

## 已验证的完整路径

真实素材为本机样片 `2026-08-09 15-31-44.mkv`（原始录像未入库），496.650 秒、3840×2160、60 fps、单路 AAC 双声道。

| 项目 | 结果 |
| --- | --- |
| 原生入口 | 通过 FilmCraft 界面的“生成 AI 草稿”按钮触发实际服务，窗口与截图已检查 |
| 模型分析 | 首次真实分析完成；40 个枪声候选事件、10 个留白候选，非高光准确率结论 |
| 草稿组装 | 按帧吸附和重叠并集合并为 6 段，连续 8316 帧／138.6 秒；保持 4K60，声画范围和关联一致 |
| 事件证据 | 原片上的 40 条标记逐条关联原始事件与预测／结果标识，完整结果另存证据文件 |
| 保护已有编辑 | 分析期间的已有编辑保留；原工程磁盘文件未被覆盖 |
| 原稿／副本 | 删除副本首段后保存重开为 7690 帧，AI 原稿仍为 8316 帧且内容哈希不变 |
| 复用结果 | 二次生成 `cached=true`，没有新增分析任务，模型工作日志不变；旧人工修改不会变成新 AI 原稿 |
| 取消／后台 | 实际原生取消未改变序列；收起 AI 面板后继续分析，编辑器可继续操作 |
| 原生默认导出 | 从生成草稿导出跨剪辑点的 3 秒／180 帧样片，完整音画解码和剪辑点对应抽查通过；未导出整个 138.6 秒草稿 |
| 导出快照 | 导出开始后立即删除首段，成片仍对应提交时的序列；不是随当前时间线改变 |

证据：独立结构核验（47 项）（`outputs/filmcraft-ai-prototype/evidence/independent-draft-validation.json`）、副本保护（`outputs/filmcraft-ai-prototype/evidence/edit-copy-protection.json`）、缓存复用（`outputs/filmcraft-ai-prototype/evidence/cache-reuse-verification.json`）、默认导出调用（`outputs/filmcraft-ai-prototype/evidence/native-default-export-submit.json`）、成片验证（`outputs/filmcraft-ai-prototype/evidence/native-default-export-validation.json`）、取消验证（`outputs/filmcraft-ai-prototype/evidence/native-cancellation.json`）。

Python 桥 10 个边界测试通过，包含源内容／音轨／模型身份、损坏预测、人工改选与阈值重算后的缓存复用、SIGTERM 和父进程消失后的子进程清理。原生 3 个 AI 单测通过，覆盖吸附并集、无效范围和切换项目后的结果保护。

原生构建与格式检查通过。严格 Clippy 在上游未改动的 `panels/file_dialogs.rs` 和 `panels/timeline.rs` 有 4 个既有风格提示（`nonminimal_bool`、`collapsible_match`）；新入口自身提示已修复。仅放行这两类提示后的检查通过，未声称整个上游严格检查全绿。

## 当前边界

- 当前完成的是单素材的原生 AI 组稿闭环。自动放置仅接受视频时间起点为 0、单音轨、单／双声道、默认解读且在线的本地素材；不支持的情况保留分析结果并说明原因，避免放错声画。当前样片满足这些条件。
- 修改过素材解读、代理映射或多音轨的工程未验收；现有批量分析、完整分数曲线和后处理调参界面尚未迁入本原型。
- 现有枪声候选可能包含他人或远处枪声，仍由用户决定哪些值得保留。
- 原稿保护通过单独工程文件实现；未增加智能合并、多版本管理或训练标注功能。
- 本机 macOS 原型验证不代替 Windows、安装分发、跨重启恢复或长时间导出的验收。保持应用运行时使用现有后台导出能力。

## 文件与复现

- `bridge.py`：现有 `ModelService` 的一次请求适配器，stdout 为 NDJSON，诊断发 stderr。核心模型代码未复制或改写。
- `apex-ai.patch`：对固定上游提交的原生 AI 入口、进度、草稿和保护逻辑补丁；源码 checkout 与构建产物位于忽略的 `outputs` 目录。
- `upstream.json`：上游提交和补丁摘要。
- `build.sh`：获取固定版本，检查补丁可应用或已应用，再构建；不重置冲突工作区。
- `run.sh`：使用隔离应用数据与模型结果目录启动，不替换已安装的 FilmCraft。
- `package_local.py`：生成本机 `.app` 启动器，继续引用工作区和已有 AI 环境。

```sh
sh integrations/filmcraft/build.sh
python3 integrations/filmcraft/package_local.py
sh integrations/filmcraft/run.sh
```

需要本机 Rust 1.95 与平台构建依赖，以及既有 `apex-voice-detect/.venv-pretrainedsed`、模型权重和媒体工具。可通过 `APEX_MODEL_WORKSPACE`、`APEX_ENGINE_PYTHON`、`APEX_CHECKPOINT`、`APEX_FFMPEG`、`APEX_FFPROBE`、`APEX_AI_DATA_DIR` 覆盖路径。构建脚本不安装这些环境。

```sh
../apex-voice-detect/.venv-pretrainedsed/bin/python -m unittest discover -s tests -p test_filmcraft_bridge.py -v
cd outputs/filmcraft-ai-prototype/filmcraft
cargo test --locked -p filmcraft-ui-egui --lib apex_ai
```
