# Execution tasks

用户已确认五个图标并授权执行；增加常驻工具栏性能摘要。Blocked: none。任务需实际验证后才勾选，不把预览当成 egui 交付。

- [ ] 1.1 确认五个分类 SVG 的视觉风格并接入资源
  - Design: [SVG 与模块边界](implement.md)
  - Acceptance: 五个可编辑 SVG 满足 [A5、A8](specs/workspace-ui.md#acceptance-scenarios)，用户确认预览；在 egui 中正确显示主题颜色、缩放、焦点与选中态，无每帧重复解码。
  - Preparation: 本次先交付 [assets/ui/workspaces/](../../../../../assets/ui/workspaces/) 源文件和系统 Temp HTML；尚未接入 egui。

- [ ] 1.2 建立固定导航和五套独立 Dock 工作区
  - Design: [所有权与工作区切换](implement.md)
  - Acceptance: 满足 [A1、A2、A3](specs/workspace-ui.md#acceptance-scenarios) 的运行期部分；支持跨工作区添加、关闭和重置当前布局；每套一个 Viewport，不复制模拟／草稿，不增加浮动窗口。

- [ ] 2.1 按场景、浅水、远海、着色、配置重组参数面板
  - Dependencies: 1.2
  - Acceptance: 参数归属满足 [R3](specs/workspace-ui.md#r3-参数归属)，FFT → SWE 增益／过滤／混合成组；模型及波谱草稿显式应用语义保留；共用颜色与显示开关不受错误的 SWE 门控。

- [ ] 2.2 统一条件隐藏与少量单选按钮
  - Dependencies: 2.1
  - Acceptance: [A4、A5](specs/workspace-ui.md#acceptance-scenarios) 通过；隐藏值和位置保留，错误、恢复与纠错入口可达；两列比例和水平互斥不被窄栏破坏。

- [ ] 3.1 实现独立布局文件与兼容／失败处理
  - Dependencies: 1.2, 2.2
  - Acceptance: [A2、A6](specs/workspace-ui.md#acceptance-scenarios) 通过；持久化五套布局、激活分类与临时隐藏锚点；不修改 `SavedSettings` 或命名配置；只读／无保存模式无隐式 IO，损坏布局不损坏模拟配置。

- [ ] 3.2 接入工具栏 FPS、CPU、GPU 主帧和条件 SWE 时间
  - Dependencies: 1.2
  - Acceptance: [A9](specs/workspace-ui.md#acceptance-scenarios) 通过，GPU 使用非阻塞时间戳；明确统计范围及不支持／等待／失败／空闲状态。

- [ ] 4.1 验证布局状态、配置隔离和实际界面
  - Dependencies: 1.1, 2.1, 2.2, 3.1, 3.2
  - Acceptance: 对 [A1～A8](specs/workspace-ui.md#acceptance-scenarios) 记录实际验证；有意义的状态／兼容测试通过，截图覆盖窄栏与切换；新增测试遵守 test-audit，不用仅复述实现的测试替代实际 GUI 检查。

- [ ] 4.2 同步当前文档并完成最终回归核对
  - Dependencies: 4.1
  - Acceptance: fmt、clippy、ocean 库测试及受影响的无默认 feature 检查通过；更新 [doco/guides/ocean.md](../../../../guides/ocean.md)、当前架构、当前界面规范和 [AGENTS.md](../../../../../AGENTS.md)，不把未交付事项写成当前事实；保留个人配置及无关文件，报告未验证限制。

## Verification

创建阶段已完成以下检查：

- `doco check refactor-ocean-workspaces`：机械检查通过，仅提示上述执行任务未完成；包内本地链接检查通过。
- 对五个 SVG 运行 `inspect_svg.py --strict-svg`：全部通过，无外链、脚本、无效数值或可访问性警告；另检查了各路径与五种图形语义的对应关系。
- HTML 内联脚本通过 `node --check`；Edge headless 渲染 1440×1000 截图并实际查看，五个图标、16／20／24／32 px 对照、深浅底和浅水选中侧栏均显示。24 px 下浅水坡底与远海卷浪可区分，未见裁切；16 px 仅作缩小参考。
- Temp 中已生成 `ocean-workspace-icons.html`，并调用系统默认浏览器打开。截图和生成参考图均留在 Temp，不加入仓库。未验证 egui 实际加载、Dock 交互或跨启动布局，因为这些尚未实现；浏览器按钮尚未逐一进行交互测试。
- `git diff --check` 未报告空白问题。尚未运行 Rust 构建、单元测试或 GPU 验证，因为没有修改应用代码。

用户已确认图标；下方补充执行验证，尚未验证的任务保持未完成。
