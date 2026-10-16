# FilmCraft AI 识别与选片：GitHub 发布清单

2026-10-14。12 张工单已发布，全部为 open，带 ready-for-agent 标签；19 条 GitHub 原生阻塞关系、87 条验收及调整后的正文已逐票读回核对。

| 本地票号 | GitHub 工单 | Blocked by |
| --- | --- | --- |
| 01 | [#21 导入录像后自动识别枪声并展示原片子行](https://github.com/jackliu1111/apex-fps-mvp/issues/21) | 无，可立即开始 |
| 02 | [#22 普通素材进入声画关联剪辑轨并连续拼接](https://github.com/jackliu1111/apex-fps-mvp/issues/22) | 无，可立即开始 |
| 03 | [#23 从真实分析结果展示人声候选子行](https://github.com/jackliu1111/apex-fps-mvp/issues/23) | [#21](https://github.com/jackliu1111/apex-fps-mvp/issues/21) |
| 04 | [#24 预览并调整待剪范围，手动划选补片](https://github.com/jackliu1111/apex-fps-mvp/issues/24) | [#21](https://github.com/jackliu1111/apex-fps-mvp/issues/21) |
| 05 | [#25 剪辑片段删改、重排与自动合拢](https://github.com/jackliu1111/apex-fps-mvp/issues/25) | [#22](https://github.com/jackliu1111/apex-fps-mvp/issues/22) |
| 06 | [#26 拖拽待剪范围加入剪辑轨并提示重复使用](https://github.com/jackliu1111/apex-fps-mvp/issues/26) | [#22](https://github.com/jackliu1111/apex-fps-mvp/issues/22)、[#24](https://github.com/jackliu1111/apex-fps-mvp/issues/24) |
| 07 | [#27 选择分析音轨，同时保留剪辑的原有声音](https://github.com/jackliu1111/apex-fps-mvp/issues/27) | [#21](https://github.com/jackliu1111/apex-fps-mvp/issues/21)、[#22](https://github.com/jackliu1111/apex-fps-mvp/issues/22) |
| 08 | [#28 多素材分析排队、取消及失败处理](https://github.com/jackliu1111/apex-fps-mvp/issues/28) | [#27](https://github.com/jackliu1111/apex-fps-mvp/issues/27) |
| 09 | [#29 映射全部声音事件，区分原片与成片时间](https://github.com/jackliu1111/apex-fps-mvp/issues/29) | [#23](https://github.com/jackliu1111/apex-fps-mvp/issues/23)、[#25](https://github.com/jackliu1111/apex-fps-mvp/issues/25)、[#26](https://github.com/jackliu1111/apex-fps-mvp/issues/26) |
| 10 | [#30 按当前范围组稿，保留原稿与编辑副本](https://github.com/jackliu1111/apex-fps-mvp/issues/30) | [#23](https://github.com/jackliu1111/apex-fps-mvp/issues/23)、[#24](https://github.com/jackliu1111/apex-fps-mvp/issues/24)、[#25](https://github.com/jackliu1111/apex-fps-mvp/issues/25) |
| 11 | [#31 保存重开工程，隔离人工编辑与可复用分析](https://github.com/jackliu1111/apex-fps-mvp/issues/31) | [#26](https://github.com/jackliu1111/apex-fps-mvp/issues/26)、[#28](https://github.com/jackliu1111/apex-fps-mvp/issues/28)、[#30](https://github.com/jackliu1111/apex-fps-mvp/issues/30) |
| 12 | [#32 真实录像完成选片、重开与原生后台导出验收](https://github.com/jackliu1111/apex-fps-mvp/issues/32) | [#29](https://github.com/jackliu1111/apex-fps-mvp/issues/29)、[#31](https://github.com/jackliu1111/apex-fps-mvp/issues/31) |

当前执行前沿：#21 与 #22。只有当前置工单完成后，才推进被阻塞的工单。

[工单、验收数与依赖映射](manifest.json)

本目录各票正文保留用户批准的内容及本地票号关系。GitHub 的 Blocked by 使用原生关系；本次仅迁入开发快照，未转移远端工单。
