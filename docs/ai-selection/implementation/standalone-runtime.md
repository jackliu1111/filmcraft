# 独立 AI 运行时与后端打包验收

2026-10-18 至 2026-10-19。规格与本地依赖图见[standalone-runtime.md](../standalone-runtime.md)。集成分支 `codex/standalone-ai-integration`，基线 `be6908cac2a44d709064b6e025e2ba2afcc698a6`，功能代码验收提交 `c8f735babbd8882442be7d0682adc04af02f275e`。

## 交付行为

应用包含冻结的 Python 3.12.14、共享 Apex 桥接后端、PyTorch 2.8.0/torchaudio 2.8.0、BEATs 模型代码与本地权重、FFmpeg/ffprobe 9.0.2 及共享库。资源从应用自身位置定位，正常运行不读取开发源码、虚拟环境或 Homebrew。保留显式开发入口。缺失、损坏、非外接数据目录或不兼容文件系统在写缓存和导入后端前明确报错。

原生程序复用现有 AI worker、识别关联、时间线、保存及导出能力。首次启动的文件夹选择器记住外接 APFS/HFS 数据目录；模型缓存、临时数据和原生运行数据均落在该目录。包内资源保持只读。外接 ExFAT 上的 APFS 磁盘映像受支持，物理外接存储单独核验；直接 ExFAT 数据目录会被拒绝。

## 产物与运行

- 安装包：`/Volumes/EXTERNALUSB/codex-artifacts/FilmCraft/standalone-ai-20261018/FilmCraft-AI-0.8.0-macos-arm64.dmg`，**633,888,960 字节（约 605 MiB）**。
- SHA-256：`1076b0387fd95f37f9a17003b86d08b2d4c17411262f366b026294f2255c0cd0`，复制前后已比对；同目录 `SHA256.txt` 和 `artifacts.json` 保存清单。
- 独立数据映像：`/Volumes/EXTERNALUSB/codex-artifacts/FilmCraft/standalone-ai-20261018/FilmCraftAIData.sparsebundle`，验收后文件合计 **31,962,084 字节**、分配 **34,340,864 字节**，容量上限 16 GiB，随分析数据增长。挂载点 `/Volumes/FilmCraft AI Data`。
- 镜像内应用从只读中文/空格路径 `/Volumes/FilmCraft AI 独立验证/FilmCraft.app` 实测运行；交付目录的 `运行说明.txt` 保存操作说明，`validation/commands/` 保存本机复现脚本。

当前为 arm64、macOS **27.0 或更新版本**：实际扫描 240 个内嵌 Mach-O，媒体工具要求 27.0，安装包自动采用最高最低系统版本。使用本机 ad-hoc 签名；未完成 Developer ID 公证及另一台干净 Mac 验收。

先挂载 `FilmCraftAIData.sparsebundle`，得到 `/Volumes/FilmCraft AI Data`；再打开 DMG 内的 `FilmCraft.app`，首次选择 `/Volumes/FilmCraft AI Data/Data`。重开应用会读取该路径。映像必须继续存放于已挂载的外接盘；外接卷断开时不会回退到本机。完整应用可以复制到兼容的外接卷；本次保留在外接 DMG 内运行。原有小型 `FilmCraft AI.app` 开发启动器未被覆盖。

构建输入及中间文件：`/Volumes/FilmCraftBuild20261018/standalone-ai-20261018`，验收后的 `du -sk` 为 9,292,120 KiB（约 8.86 GiB，含多轮运行时和构建缓存）。其 APFS 映像位于 `/Volumes/EXTERNALUSB/codex-artifacts/FilmCraft/build-20261018-arm64/FilmCraftBuild.sparsebundle`。最终交付仅依赖上面的独立数据映像，不需要该构建卷。所有大文件均直接在真实外接磁盘生成；未移动或修改原模型环境、后端仓库及原录像。

## 版本、来源与构建

- 共享后端固定干净提交 `978fed1580fdf85d34e2b5c17f973693c5510693`。
- PretrainedSED 提交 `1aa47e482f7e89904cba2338999345025d8b4e36`。
- `BEATs_strong_1.pt`：364,091,999 字节，SHA-256 `db13a79ae90a0cfd0f9911a6a1d8cdb89324322bee642dcfe32de022123b8b54`。
- PyInstaller 6.16.0；NumPy 1.26.4、SciPy 1.16.2、soundfile 0.13.1。构建工具安装到本次任务的隔离目录，未修改原虚拟环境。
- `manifest.json` 记录后端提交、Python/依赖版本、源文件和权重摘要、资源清单；许可和媒体工具来源保存在运行时 `licenses/`。资源摘要针对签名前的 staging，代码签名后的完整包使用交付 DMG 摘要核验。

复现命令见[冻结构建](../../../packaging/ai/README.md)与[macOS 打包](../../../packaging/macos/README-ai.md)。本次使用外接卷中的原生 release 二进制和 `--skip-build`，运行时输出 `dist/ai-runtime-v3`，打包执行 `--arch aarch64 --ai-runtime .../ai-runtime-v3`。Rust UI 编译及 filmcraft/filmcraft-cli release 构建通过；后续仅修改 Python、Shell、签名配置和文档。

## TDD、审查与实际问题

用户确认公开 probe/analyze/validate、打包命令和真实启动为测试接口。按本地 S1/S2/S3 依赖在独立工作树实施，并由合并代理接入同一集成分支。

| 验证层 | 结果与边界 |
| --- | --- |
| Python bridge/runtime | 40 项通过，含 16 项冻结入口回归；模拟后端证明路径、协议和错误保护，不证明模型质量 |
| 构建 CLI | 4 项通过；真实外接元数据及完整输入的 check-only 只读检查均执行，无跳过 |
| macOS 包及启动器 | 13 项通过，无跳过；含签名后服务/媒体工具失败传播、开发覆盖、路径字面值、不修改包内资源、镜像独立外接输出保护和真实镜像验签 |
| Rust | 受影响 UI crate 编译及两个 arm64 release 二进制构建通过 |
| Standards 审查 | 0 项硬性违规、0 项可操作维护问题；运行时、签名、文件系统及镜像修复后的聚焦复审通过 |
| Spec 审查 | 最初 2 项 P2，均用失败回归复现并修复；后续镜像和独立输出保护复审后 0 项遗留 |

两项 P2 分别为数据目录可指向运行时祖先并破坏应用签名、启动器在显式开发模式下仍强制检查冻结服务。现已在任何写入前拒绝整个 `.app` 内的数据目录，并优先保留开发入口。

实际签名/推理还揭示环境和交付问题，均先复现再修复：

1. 静态签名验证成功后，硬化运行时仍拒绝冻结 Python 和媒体工具加载无共同 Team ID 的内嵌库。仅对冻结服务、FFmpeg、ffprobe 三个精确可执行路径设置 `disable-library-validation`；编辑器、外层应用和各库不增加例外。打包现在先严格验证签名，再实际启动这三项，任何失败均停止生成 DMG。
2. 直接 ExFAT 模型数据产生 AppleDouble `._*.json`，共享后端恢复任务时误作 JSON。没有修改共享核心；运行时新增 APFS/HFS 数据卷前置校验，并用独立的外接 APFS 数据映像完成真实链路。
3. HFS `makehybrid` 给签名文件添加 FinderInfo，源应用与 staging 验签通过、最终只读镜像内验签却失败。用真实微型签名应用复现后，AI 镜像改用原生 APFS 创建；生成后只读挂载并严格验签，成功或失败均卸载。公开 `image` 命令也独立核验外接 APFS、源输出不重叠、可写性和空间。真实测试还验证已签名资源被污染时镜像 gate 拒绝交付；非 AI 原有镜像流程保持可用。

## 搬迁后真实验收

实际素材是原真实录像的 20 秒、960×540 H.264/AAC 单音轨样本，9,468,130 字节，保存在交付目录 `validation/游戏声音样本.mp4`；原录像未移动。该短样本只验证模型和媒体链路，不作为效果真值或性能基准。

| 最终 APFS 镜像验收 | 实际结果 |
| --- | --- |
| 搬迁与隔离 | 清除开发环境覆盖，只保留系统 PATH；只读中文/空格挂载目录运行。通过 macOS sandbox 明确禁止读取 FilmCraft 源码、原模型目录、Homebrew 和整个原构建卷 |
| 离线公开协议 | 额外禁止所有网络；check/probe/首次 analyze（cached=false）/validate（valid）/再次 analyze（cached=true）全部通过 |
| 取消 | 独立新数据目录在模型加载/推理阶段发 SIGTERM，返回结构化取消错误、非零退出且没有结果 |
| 原生真实分析 | 公开显式外接数据参数启动应用，导入后全新推理成功，得到 2 个枪声事件和 3 个人声事件；原生阶段允许 localhost 控制连接，联网隔离证据取自上述 CLI 阶段 |
| 原生编辑与持久化 | 从原片 7–9 秒放置一组关联声画片段；保存真实工程、退出并新进程打开，分析关联只读核对为 valid，视频/音频片段、目标轨、60 帧/2 秒时长逐字段一致 |
| 真实媒体导出 | 默认快速导出完成；包内 ffprobe 确认 640×360、30fps、60 帧、视频 2.000 秒、双声道 AAC；容器及音频 2.005 秒，5ms 尾部补齐在一个 AAC 编码帧内；包内 ffmpeg `-xerror` 完整解码成功 |
| 签名与资源只读 | 构建时和搬迁使用后均通过 `codesign --verify --strict --deep`；复制后的 DMG 自身严格验签通过。应用始终位于只读镜像中 |
| 真实文件系统保护 | 直接 ExFAT 数据目录的 check 返回 runtime_unavailable/APFS 提示，目录树没有新增缓存或临时文件 |

最终原始证据根目录：`/Volumes/EXTERNALUSB/codex-artifacts/FilmCraft/standalone-ai-20261018/validation`。离线链路在 `relocated-cli-apfs-final/`，原生状态、工程、截图、导出、完整解码与使用后签名记录在 `native-apfs-final/`；审查及镜像红绿记录在 `reviews/`。旧 HFS 失败记录保留为诊断材料，不作为交付通过证据。

首次文件夹选择器的人工点选没有计为通过：本机界面工具无法绑定由 osascript 托管的选择器；目录选择失败时的保护和记忆路径由进程回归覆盖。真实原生验收通过公开 `FILMCRAFT_AI_DATA_DIR=/Volumes/FilmCraft AI Data/NativeFinal` 启动，未伪称完成 Finder 首次点选或记忆位置的人工验证。该点选仍可单独补验；正常应用入口及显式目录入口均已实现。

这些结果验证本机当前 arm64/macOS 环境中的独立运行、缓存、取消、持久化和媒体链路；未宣称人工识别质量、真实游戏声/麦克风分轨效果、同目标效率比较或另一台 Mac 兼容性通过。原 #21–#32 状态不随本轮本地交付关闭。

## 远端交接

自动审批拒绝向 Apex GitHub 创建本轮工单，原因是未明确授权向该外部仓库发布内部项目内容。本轮代码、本地 S1/S2/S3 和审查证据已准备；分支未推送，未创建新 PR，也未关闭远端旧工单。待用户明确授权后，可发布这次源码变更和对应工单/PR；不上传模型、录像、运行数据或构建包。
