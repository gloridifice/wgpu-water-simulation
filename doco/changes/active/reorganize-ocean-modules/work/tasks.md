# Execution tasks

Blocked: none。

- [x] 1.1 记录基线与固定输入验证材料
  - Design: [基线与验证](implement.md)
  - Acceptance: 记录基线测试、WGSL 指纹、实际 GPU 验证和可重复快照；不写个人配置。

- [x] 2.1 建立职责目录并拆分设备、模拟、渲染与参数
  - Dependencies: 1.1
  - Acceptance: 设备不引用求解器/Renderer；参数不引用 app/ui；原公开路径保持类型与签名兼容；WGSL 原字节保留。

- [x] 2.2 拆分应用、界面、设置与所属验证
  - Dependencies: 2.1
  - Acceptance: 相机无窗口依赖；天空图表归面板；面板/验证私有访问不过度扩张；窗口/快照、保存语义和运行时验证入口保留。

- [x] 3.1 更新文档并执行完整回归
  - Dependencies: 2.2
  - Acceptance: 当前路径说明准确；fmt、Clippy、库/工具/CPU 测试与 release GPU 验证通过，固定输入快照一致，有限帧 GUI 检查有实际证据；差异审查确认无算法和配置格式改动。

- [ ] 3.2 提交并推送本次重构
  - Dependencies: 3.1
  - Acceptance: 仅包含本变更，提交成功并推送 origin 当前分支；无测试产物或个人配置混入。

## Verification

- 初始工作区干净，基线 3d23873；cargo test --features ocean --lib：64 通过；cargo build --release --features ocean --bin ocean：通过。
- 基线源码、WGSL SHA-256、release 程序保存在系统 Temp/ocean-module-refactor；RTX 3070 / Vulkan 的完整 --verify 通过。远海、沙滩一阶/二阶、浪花、双浓度、脊线六组快照生成成功；cutty 实际查看暂停海滩 GUI，工具栏、固定导航、左侧上下 Dock 和右侧视口正常。
- 2.1/2.2：cargo check --features ocean --all-targets、cargo clippy --features ocean --all-targets -- -D warnings、cargo test --features ocean --lib 均通过，仍为 64 个库测试；21 份 WGSL 的 SHA-256 全部与基线相同。源码函数对照排除路径和 rustfmt 差异后，仅有验证总入口拆分及原损坏选择修复片段封装；未删除测试、未改变模拟函数体或公开字段。
- 重构后 RTX 3070 / Vulkan 完整 --verify 通过；foam-weight-lab --verify 通过；cargo test --release --features ocean --bin foam-lab -- --ignored 的 GPU 保形测试通过。
- 远海、海滩一阶/二阶、双浓度、脊线五组 PNG 与基线字节一致；浪花发射调试图也字节一致。粒子透明图跨版有 35/1,024,000 像素不同，最大通道差 2/255；基线同版重复有 22 像素差、最大 2/255，重构同版重复有 23 像素差、最大 2/255。这是既有并行粒子追加/透明叠加的非确定性，不修改算法以消除它。
- cargo test --workspace --all-targets：CPU 33、海洋库 64、foam-bake 6 个测试通过（另有 1 个 GPU 保形测试先忽略，已用上面的 release 命令显式运行）；CPU Clippy、根包 --no-default-features --all-targets 检查通过。
- 相同只读暂停配置的迁移前后 cutty 窗口截图已实际查看，默认面板分区、导航、工具栏和海滩视口一致；不声称逐一手工验证所有页签交互。另用 --beach --no-settings --swe-resolution 64 --frames 90 正常退出，个人配置没有改写。
- 最终格式化后再次运行 fmt --check、全 target 海洋 Clippy、全 workspace/all-targets 测试、CPU Clippy、no-default-features 检查、全部 release 工具构建及两个 --verify，全部通过。最终 release 的五组可重复 PNG 再次与基线字节一致；根 ocean 同层 Rust 文件由 44 减至 3。当前架构、使用说明、浪花规范与 AGENTS 路径已同步；git diff --check 和 doco check 通过（提交任务执行前保持未勾选）。
- Test audit：保留原有 64 个海洋库测试及工具/CPU 测试；仅调整归属和导入，没有删除断言、测试专用生产出口或新增目录清单测试。测试数量不作为行为等价的唯一证明，另有真实 GPU 和基线对照。
