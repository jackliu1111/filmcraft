# AI 选片两轴审查与修复确认

2026-10-18 汇总。全量审查固定范围 `c2f65871e998cca4700a74dbcf9348cab51fb06b...3b05b81ed2fcaf179cbf8dfc703705d27192f96c`；修复提交 `1dc26c0143987aefb7ddc5b002f5745ad5fdf214`，集成合入 `a16b200e`。两轴分别保留原定级；同一缺口没有合并或跨轴重排。

## Standards

范围：`c2f65871e998cca4700a74dbcf9348cab51fb06b...3b05b81ed2fcaf179cbf8dfc703705d27192f96c`。按当前原型约定审查；不以纯 Rust、全仓 CI 或跨平台门禁作缺陷。

### 原始硬违反

- **[P1] 拖入已有 AI 范围前未核对识别来源。** `crates/engine/src/selection_placement.rs:61–79` 从保存的范围直接捕获，`source_signature` 只记录拖动开始时的当前文件。若识别后同一路径文件被替换，或原片被重新链接，旧范围仍可拖入／Shift+Enter 追加，捕获和提交时的“当前”签名相等，因而通过 `validate`。`recognition.recovery` 的 source_changed 警告没有进入这条提交链路。违反 `AGENTS.md:26`：“外部分析输出先校验来源、音轨、时间范围与状态，再通过编辑器修改工程。”应在捕获及提交处核对范围 provenance 对应的来源、音轨和映射；可复用 `assembly.rs:118–121` 已使用的 `recognition.mapAudio` 检查。缓存文件缺失应与实际来源变化区分，保留现有人工工作。静态调用链已确认；本轴未运行 UI 或构建。

### 原始判断项

- **Possible Primitive Obsession / Repeated Switches：队列状态重复散布。** `crates/project/src/analysis.rs:11,42` 的 `status: String`／`matches!(…, "completed" | "cancelled" | …)`，`crates/engine/src/analysis_queue.rs:202,259` 与 `crates/ui-egui/src/analysis_queue_ui.rs:19–31,189–190` 分别维护活动、终止、重试和恢复集合。取消及迟到接收属于核心工作保护，新增状态需要多处同步，漏改可能重启终态任务或错误接受迟到消息。建议使用小型 `AnalysisStatus` 与共享状态谓词，保持现有序列化字符串；这是维护性判断，未据此宣称已有状态迁移错误。

已读 AGENTS、CLAUDE、CONTRIBUTING、prototype-development、构建参考、架构、ui-egui README 及 apex-ai README。`crates/project/README.md`、`crates/engine/README.md` 当前不存在；未将文档缺失列为缺陷。

合计：1 项硬违反，1 项判断项；最严重为旧分析范围可作用于已变化的来源。

### 修复后聚焦复核

复核提交 `1dc26c0143987aefb7ddc5b002f5745ad5fdf214`，以 AGENTS 与原型开发说明为准。

- 原硬违反 **P1：已闭合**。AI 范围捕获核对原 result_id、完成状态和音轨，复用 check_saved 校验保存来源；预览及提交重验来源和识别摘要。替换、重链、代理、解读及音轨变化均受保护。缓存缺失与来源变化分开，手绘范围保持独立，失败链路没有编辑写入。
- 原判断项 **Primitive Obsession / Repeated Switches：已采纳并处理**。AnalysisStatus 集中工作、待处理、终态、取消与重试判定；serde 保留旧字符串，未知值恢复为 interrupted 并保留 previous_status，清除运行令牌，不自动派发。

未发现新的具体 Standards 缺陷。仅静态复核调用链、差异与公共接口回归用例；未独立构建或操作 UI，原生正向验收由主任务补齐。

## Spec

审查范围：`c2f65871e998cca4700a74dbcf9348cab51fb06b...3b05b81ed2fcaf179cbf8dfc703705d27192f96c`。只读追踪实际调用路径，未重复执行构建或操纵原生界面。

- **[P2] 拖入时只核对手势期间的变化，未核对范围所依据的识别来源。** `crates/engine/src/selection_placement.rs:61–79` 先读取已保存范围，再用当前媒体设置和磁盘文件生成 `source_signature`；`:95–100` 仅比较这份新签名在拖动期间是否变化。因此，在分析完成后、拖动开始前替换同路径原片（时长保持足以容纳旧范围），随后拖主体或 Shift+Enter，旧识别范围仍能经 `selection.place → parameters → commands::place_one` 写入现在的媒体。原片已变化时 `recognition.recovery` 可以报告 `source_changed`，但该检查没有进入放置路径。自定义解读／代理在捕获前已改变时，也会被作为新的合法基线接受。与之对应，组稿路径 `crates/engine/src/assembly.rs:118–121` 已用保存的 `result.media` 核对来源。应在捕获／提交识别来源范围时复用这项来源和媒体映射检查，并保留旧范围供查看；不能只防拖动期间变化。依据：`docs/ai-selection/tickets/07-multitrack-analysis-and-retention.md:16` 要求“暂不支持的素材解读或代理情形在放置前给出原因……不静默放错声画”；仓库 AGENTS 的“外部分析输出先校验来源、音轨、时间范围与状态，再通过编辑器修改工程”。

缺失／部分：除实现文档已明确保留的真实分轨素材、区间效果与效率验收，以及父任务正在补测的 #31 原生恢复场景，未另确认可独立报告的缺失。未发现有证据支持的未要求范围扩张。以上为 1 项“看似实现但校验边界错误”。

### 修复后聚焦复核

静态复核修复 `1dc26c0`。原问题维持 **P2，已修复关闭**：`selection_placement.rs:80–89` 将 AI 范围绑定保存的 result ID；`:32–42` 核对完成状态、分析音轨并调用 `check_saved`，拒绝来源、代理及解读变化；`:119–122` 让预览和提交复核同一分析证据，阻止中途替换结果。

校验不依赖外部预测文件，也不把缓存不可用等同原片变化；独立手绘范围跳过 AI 来源约束，仍保留媒体快照保护。

`AnalysisStatus` 的终态、排队、工作进程、取消／重试谓词与原判断一致；保存仍为原字符串，未知旧状态重开转中断，不自动续跑。未确认新的规格回退或范围扩张。

本次未运行测试或操作 UI；真实分轨素材和人工作业验收仍属已声明缺口，不新增重复 finding。

Standards：原 1 项 P1 硬违反已修复、1 项维护性判断已处理，无新增具体缺陷；Spec：原 1 项 P2 已修复，无新增具体规格回退。修复回归及边界详见[修复记录](review-fixes.md)。真实分轨、识别真值和同目标效率仍按[验收矩阵](32.md)保留，不因审查完成变为通过。
