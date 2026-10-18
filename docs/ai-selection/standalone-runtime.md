# 独立 AI 运行时与后端打包

2026-10-18，用户确认范围与公开测试接口。集成分支 `codex/standalone-ai-integration`，基线 `be6908cac2a44d709064b6e025e2ba2afcc698a6`。远端创建工单被自动审批拒绝；本文件作为本轮本地工单与依赖记录，不变更旧 #21–#32 的验收状态。

## 交付范围

先支持 macOS arm64。应用携带冻结的 Python/共享 Apex 桥接后端、模型代码与本地权重、FFmpeg/ffprobe 及依赖，不依赖开发源码目录、用户虚拟环境或 Homebrew。保留现有 NDJSON 的 probe/analyze/validate、进度、错误、取消及父进程保护。包内资源只读；模型数据、缓存和临时文件使用显式 `APEX_AI_DATA_DIR` 或 `FILMCRAFT_AI_DATA_DIR` 指定的外接目录，不静默向本机写大数据。保留现有显式开发环境入口。

数据目录所在的挂载卷必须使用 APFS 或 HFS（Mac OS 扩展），在创建缓存、临时目录及启动后端前检查。真实 ExFAT 数据目录生成的 macOS AppleDouble `._*.json` 会被当前共享后端当作任务 JSON 读取，导致恢复失败；本轮不修改共享核心。可使用外接磁盘上的 APFS 磁盘映像；仅数据挂载卷要求 APFS/HFS，映像的物理外接存储可使用 ExFAT，但须继续验证其确为已挂载的外接卷。无兼容卷时明确报错，不回退本机目录。

运行时契约：`Contents/Resources/ai-runtime/manifest.json`，`schema_version: 1`，`platform: macos-arm64`，`service: service/filmcraft-ai`，`model_workspace: model`，`checkpoint: model/checkpoints/BEATs_strong_1.pt`，`ffmpeg: service/_internal/bin/ffmpeg`，`ffprobe: service/_internal/bin/ffprobe`。路径相对运行时根，拒绝绝对路径或逃逸根目录。声明并校验所需文件；错误不得退回开发路径。冻结服务从自身位置读取契约，向共享核心提供显式模型工作进程命令，正确分派 guardian/inference-worker，避免把冻结程序当作通用 Python 解释器。

原生应用优先保留显式 `FILMCRAFT_APEX_PYTHON`/`FILMCRAFT_APEX_BRIDGE` 开发入口；否则存在包内运行时就直接调用冻结服务。声明运行时缺失/损坏必须报错。没有包内运行时的源码开发继续原有行为。

## 本地工单与依赖

- **S1 运行时入口与原生定位**（无阻塞）：公开 probe/analyze/validate 验证包内定位、可写外接数据目录、明确错误及开发覆盖；Rust Worker 调用冻结服务而不追加脚本参数。独立分支/工作树，TDD。
- **S2 可复现冻结运行时构建**（无阻塞，遵循上述契约）：提供打包命令；校验输入与外接输出，冻结共享桥接及依赖，复制模型资源、许可/来源/摘要，不复制虚拟环境。guardian 与 inference-worker 均须可重入。公开命令 TDD，不修改原始共享后端或模型环境。
- **S3 macOS 集成、签名与真实搬移验收**（阻塞 S1、S2）：资源在签名前复制，逐层签名与严格验证；产物及中间文件直接在外接 APFS 容器；中文/空格搬移目录与受限 PATH 下执行真实 probe、新推理、validate、取消及原生启动，记录实际结果和未覆盖部分。

S1/S2 完成后合入集成分支，S3 之后执行 Standards/Spec 两轴审查与修复。审批未允许前不向远端发布本地工单或 PR，不关闭旧票。

## 验证边界

公开测试接口由用户确认：bridge.py probe/analyze/validate、打包命令及实际启动。模拟后端用来验证协议、路径、错误保护；真实模型与原生界面各自取证。另一台干净 Mac、正式 Developer ID 公证和跨平台发行不据本机验证宣称完成。人工区间质量、真实分轨录像与同目标效率比较仍按 #32 保留。
