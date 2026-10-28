# 执行任务

以下任务已实现并完成所列验证；变更仍保持 active，未执行 complete 或 archive。需求编号对应 [proposal.md 的验收](../proposal.md#验收)，设计见 [implement.md](implement.md)。

- [x] 1.1 建立浪花配置与 GPU 数据契约
  - Design: [API 与数据模型](implement.md#3-apis-and-data-model)
  - Acceptance: 增加独立配置、默认值、有限数/范围校验、Rust/WGSL 一致的数据布局和设备容量约束；旧配置缺字段仍保留原参数，默认关闭浪花。配置兼容测试只覆盖持久化与非法输入风险，不复述结构字段。满足验收 6。

- [x] 1.2 从完成的 SWE 子步生成 GPU 发射信号
  - Dependencies: 1.1
  - Design: [发射判据](implement.md#发射判据)
  - Acceptance: 独立 pass 读取一致的前后状态；每完成子步累积一次，不在二阶预测阶段重复计入。固定网格 buffer 按面积和实际模拟秒积分；静水、干地、平坦回流为零，指定上升陡坡为正；不读泡沫密度触发。满足验收 1、2、3。

- [x] 1.3 实现 GPU 粒子池、发射余量与间接参数
  - Dependencies: 1.1
  - Design: [容量与时间](implement.md#容量与时间)
  - Acceptance: 固定容量双缓冲先压紧存活粒子，再消费信号追加新粒子；GPU 生成 indirect 参数。常量信号验证低发射率累积、不重复乘时间、池满丢弃且无整数积欠、寿命到期释放容量；运行时无计数回读和同步等待。满足验收 1、3、4。

- [x] 2.1 接入帧调度、碰撞和资源生命周期
  - Dependencies: 1.2, 1.3
  - Design: [整体方案](implement.md#2-overall-approach)、[容量与时间](implement.md#容量与时间)
  - Acceptance: viewer 常规帧与快照推进路径都消费实际模拟时长；出生校正、重力/阻力、落水/触地和出域消亡有效。暂停不重消费，禁用、重置、域/谱重建清除相应资源；开启浪花不改写 SWE/泡沫。满足验收 1、4、6。

- [x] 2.2 绘制纹理粒子并处理深度软交界
  - Dependencies: 2.1
  - Design: [纹理渲染](implement.md#纹理渲染)
  - Acceptance: 引入许可明确且仓库内可访问的纹理，记录来源；GPU 间接绘制预乘透明度四边形，水面/沙滩正确遮挡并软化交界。主深度可采样、正确保存，不出现附件读写冲突；resize 后仍有效。固定场景实际 GPU 预览满足验收 5。

- [x] 2.3 接入参数面板、发射调试视图与保存恢复
  - Dependencies: 2.1, 2.2
  - Design: [配置与资源失败](implement.md#配置与资源失败)
  - Acceptance: 统一两列网格，名称 hover 说明单位、范围和生效方式；可单独开关浪花和查看 GPU 发射强度。参数通过现有会话/命名配置链路保存恢复，不保存粒子状态，不覆盖个人配置。满足验收 2、6。

- [x] 3.1 完成真实 GPU 行为验证与回归覆盖
  - Dependencies: 2.1, 2.2, 2.3
  - Design: [验证与文档影响](implement.md#6-verification-and-documentation-impact)
  - Acceptance: 在现有 GPU 验证入口集中检查源→粒子的积分、余量、饱和恢复、两种求解阶数和状态重置。明确新增断言守护的行为、可信回归和既有覆盖缺口；不靠源码搜索、假实现或测试专用生产接口证明正确。GPU 编译/绑定/运行错误为零，既有水体与泡沫验证通过。覆盖验收 1–6；仅验证程序允许末尾读回。

- [x] 3.2 执行整体检查并同步已实现文档
  - Dependencies: 3.1
  - Acceptance: 执行下列构建、测试和有界预览，记录实际结果及未验证限制；更新 [doco/guides/ocean.md](../../../../guides/ocean.md)、[doco/architecture.md](../../../../architecture.md) 和已交付的 [doco/specs/](../../../../specs/) 合约。检查无运行时代码依赖 CPU 回读发射信号/计数；`doco check add-gpu-spray-particles` 和差异检查通过后才可申请完成，不自动归档。

- [x] 4.1 接入联网调研的浪花图集
  - Dependencies: 3.2
  - Acceptance: 按用户追加要求核查外部纹理及许可，将原图和署名保存到仓库。四个粒子变体使用独立 Alpha 图层、透明边和 mip，不改变发射或运动算法；验证四层真实 GPU 输出不同且仍正确遮挡/淡出。
  - Verification: 采用 Effekseer 官方 Pierre 1.30 包内 Splash.png，官方页面标注 CC0；原图 SHA-256 与官方包一致。已有 GPU 光栅验证扩展为两种尺寸 × 四个图层，检查不同输出及软交界，完整 `--verify` 通过。Clippy、release 构建通过，完成固定条件快照检查；烘焙文件 349,524 字节，来源与完整许可在 assets/spray 内。

## Verification

### 执行结果

- `cargo fmt --check`、`cargo clippy --features ocean --all-targets -- -D warnings`：通过。
- `cargo test --features ocean --lib`：60 项通过。新增配置兼容/校验用例；扩展既有 CLI 覆盖测试，先复现启用浪花将已保存密度 321 重置为 150 的失败，再修复为仅覆盖显式开关，最终通过。
- `cargo check --no-default-features --bin water-simulation`、`cargo test --no-default-features --lib`：通过，CPU 库测试 1 项通过。
- `cargo build --release --features ocean --bin ocean`、`target\release\ocean.exe --verify`：通过，实际设备为 NVIDIA GeForce RTX 3070 / Vulkan。
- GPU 新增验证：静水/干地/平坦回流为零、上升陡坡非零；32 字节信号的加权世界坐标/速度；时间分割和面积归一；小数发射；满池不会用新粒子替换存活粒子，过期复用且无整数欠账；暂停/重置和落水。真实一阶/二阶单子步输出与依据子步前后状态计算的 f64 发射量一致，排除预测阶段重复计量；开关浪花不改变水体和泡沫字节。图形验证在 32×32、64×64 两种目标尺寸下检查已知深度遮挡、交界淡出和非零纹理覆盖。
- 一阶已运行 `--beach --spray --no-settings --frames 1800`；另启动有限帧预览，通过 cutty 检查浪花页签及窗口缩放至 1280×900 的布局/画面，然后只关闭本次启动的窗口。二阶 `--beach --spray --no-settings --swe-order 2 --frames 600` 正常退出。
- 固定默认相机、种子与波谱，使用 `--snapshot`、`--time 5` 分别检查一阶粒子、一阶发射强度和二阶粒子画面。可以看到岸边上方的离散白色飞沫及绿色发射区域；截图不用于替代面板布局验证。
- `doco check add-gpu-spray-particles` 与 `git diff --check`：通过。README、当前架构和 [GPU 浪花合约](../../../../specs/gpu-spray.md) 已同步；实现细化没有扩大远海或三维碰撞范围。

### 外部纹理接入补充

此次只修改浪花纹理、材质及其 GPU 光栅验证。执行期间工作区出现并行的水面泡沫材质修改，未还原或混入该范围；全仓库 `cargo fmt --check` 曾报告这些并行改动的格式差异，因此外部纹理接入以相关文件的格式检查与已执行的 GPU 检查为准，不覆盖另一项工作的验证结论。

### 验证范围与限制

测试主要守护 GPU 信号→粒子消费、资源生命周期和深度绘制边界；没有添加源码字符串或纯布局复述测试。读回只存在于独立验证模块，运行时数据路径经代码检查不依赖读回。

仅验证上述 Vulkan 设备；没有强制制造真实显存耗尽，也没有在其他 GPU 后端上运行。透明重叠未排序、碰撞仅为高度场、按帧平均子步出生位置属于首版近似，不声称完整物理破浪或逐粒子跨硬件确定性。预览均使用 `--no-settings`，未改写个人配置；本地截图和日志保留在被忽略的 target 或系统临时目录，未加入版本管理。
