# Implementation design

## 1. Baseline and goals

基线为 3d23873，开始时工作区干净；上一轮 CPU 包、WGSL 和文档迁移已提交。现有入口 src/bin/ocean.rs、foam_lab.rs、foam_weight_lab.rs、foam_bake.rs、terrain_bake.rs 均保持。当前 64 个 ocean 库测试通过，release ocean 已构建。当前架构及 doco/specs/gpu-spray.md 是行为约束。

其他活动包 refactor-ocean-workspaces 与 add-gpu-spray-particles 的实现已存在于基线；本变更只移动其代码，不修改或完成其追踪状态。

## 2. Overall approach

- ocean 公共门面提供 Error 及原路径重导出；内部代码使用职责路径，不通过旧门面回引。
- gpu/context 只封装设备及回读，gpu/timer 只负责异步时间戳。Ocean 移至 simulation/offshore/fft；Renderer 移至 render/renderer。
- parameters 保留 OceanConfig、FoamConfig、ShoreConfig、SprayConfig、SkyConfig、ModelConfig、OpticsConfig 的现有定义、默认值和校验；Camera、CameraUniform 一起移至 ocean/camera，参数层不引用 app/ui。
- simulation/offshore 包含波谱、FFT 与远海泡沫；nearshore 保留 Shore 的资源与调度，将 ShoreUniform 和验证分离；spray 保留当前信号与粒子实现，暂不拆资源所有权。
- scene 放模型几何、地形和碰撞体；render 放场景绘制、光学、天空、海底及纹理。天空 depth_graph 移至 ui/panels/optics，数值参考留在 render/skylight。
- app/viewer 组织窗口与帧，app/snapshot 保留离屏流程；ui 收纳 GUI、控件、各面板与 workspace/store；settings 分 session、library、store；labs 收纳泡沫权重实验。
- 面板方法仍属于 Gui，移动到 ui/panels 的正常子模块；方法只在 ui 内可见，共享编辑器字段沿用原有祖先可见性，不暴露其他私有字段。参数模块可在原归属模块重导出，避免更改现有签名。
- WGSL 按 offshore、nearshore、spray、render、textures 和实验所有权搬迁；include 路径重定位到唯一文件，不改变拼接顺序或内容。

## 3. APIs and data model

现有公开签名全部保留，包括 ocean::gpu::{Gpu,Ocean,Renderer,CameraUniform,Error}、ocean::viewer::{Camera,run,snapshot}、各 config/solver/verify 入口。用 Rust 重导出保持类型身份，不建立运行时包装或双份实现。

内部辅助函数只在必要的祖先模块内共享；保持私有字段，领域验证作为子模块可访问所属内部状态。settings 的原子写入、500ms 防抖、重试及错误行为原样搬迁；工作区 sidecar 不并入会话。uniform/storage 的字段、顺序、repr、大小不变。不改资源创建、释放、重置、异步回读、设备降级语义。

## 4. Algorithms and rules

机械移动、导入和 include 路径调整为主。FFT → SWE 物理耦合不变；保留 Euler/MUSCL+RK2、CFL、泡沫双模式与浪花源逐完整子步发射。保留 Renderer 持有 Shore、Shore 持有 SpraySource、Renderer 持有 SprayParticles；不重排窗口与快照调用。验证仍在非 cfg(test) 构建中可运行。测试只搬迁/调整路径，不增加复述目录结构的断言，不删除或削弱现有行为验证。

## 5. Fixed decisions and discretion

固定：用户确认的职责树、行为/兼容性不变、不重写算法、不引入过度防御或额外通用框架。局部文件名、私有共享函数归属、验证子文件粒度可按现有访问关系调整；不为迁移批量改为 pub(crate)。Blocked: none。

## 6. Verification and documentation impact

先保存基线源码、WGSL 指纹、release 程序及固定输入快照到系统 Temp。迁移后运行 fmt、clippy --features ocean --all-targets、ocean 库/工具测试、CPU 包测试与 clippy、no-default-features 检查、release 构建及实际 GPU --verify。对远海、海滩两阶、浪花、双浓度等固定配置比较快照，检查 WGSL 字节未变，并启动 --no-settings 有限帧窗口用截图核对布局。

实测限制：浪花池使用 GPU 并行追加并且透明叠加不排序，基线程序自身重复运行即存在少量像素差异；保留这一行为，不为截图引入排序。可重复路径要求 PNG 字节相同；浪花源的调试快照另做精确对照，实际粒子图报告同版/跨版重复差异，并结合完整 GPU 容量、积分、生命周期和水体隔离验证。

当前文档更新 doco/architecture.md、doco/guides/ocean.md、foam-lab.md、doco/specs/gpu-spray.md 及 AGENTS.md 的路径。README 如无受影响事实则不改。记录证据与限制，不将机械检查说成视觉验证；不自动完成或归档其他包。
