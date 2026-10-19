## Summary

macOS arm64 应用现在携带 AI 后端、Python、模型和媒体工具，从应用自身定位资源；首次启动选择外接 APFS/HFS 数据目录。

```text
FilmCraft.app
  launcher → 外接数据目录校验 → 原生编辑器
  Resources/ai-runtime
    manifest → 冻结共享后端 → inference-worker / guardian
    本地模型权重 + FFmpeg/ffprobe + 许可/来源记录
```

## Evidence

- **Before:** 开发入口依赖外部源码、虚拟环境和媒体工具路径，无法直接搬移为独立应用。
  **After:** 冻结运行时与 macOS 打包命令已实现；源码目录、原环境和 Homebrew 不可读时的真实验收见[交付记录](standalone-runtime.md)。
- **Before:** 签名静态验证未捕获动态库启动失败；直接 ExFAT 数据目录会使共享后端读取 AppleDouble 元文件失败。
  **After:** 仅三项辅助进程使用必要的签名例外，打包后逐一启动；运行数据在写入前要求 APFS/HFS 并核验物理外接存储。
- **Before:** HFS 镜像生成后添加 FinderInfo，使包内应用的严格签名检查失败。
  **After:** AI 镜像使用 APFS，最终只读挂载后验签；公开镜像命令独立核验外接输出和空间。
- 57 项公开边界回归通过，Rust UI 编译与 arm64 release 构建通过；Standards/Spec 两轴审查及聚焦复审无遗留问题。真实推理、原生保存/导出/重开和交付摘要以[实际记录](standalone-runtime.md)为准。

## Merge Danger

**Door:** two-way

源码开发入口保留；独立包数据在应用外部，不修改原片及既有人工编辑。回退代码不会删除外接数据。

**Blast Radius:** packaging

涉及 macOS AI 包装、冻结服务定位与原生 AI worker 启动。当前媒体依赖使实际包要求 macOS 27.0；本机 ad-hoc 签名不代表 Developer ID 公证或另一台 Mac 验收。原 #21–#32 的人工质量与真实分轨缺口仍保留。
