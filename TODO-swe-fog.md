# SWE 贴地雾迁移 — 未完成事项（临时）

> 临时交接记录：标注 `crates/swe-fog/`（独立 Rust/wgpu 贴地雾）这一轮迁移中尚未完成、
> 或未经真实验收的部分。全部条目完成后请直接删除本文件，不要归档进 `doco/`。
>
> 记录时间：2026-11-09 21:45。来源：pi 会话（`w21:p1`）被模型侧 300 秒超时中断后，
> 由 Hermes 接管提交；下面的"已完成"部分是该会话自述的结论，未在本文件里二次复验。

## 已完成（据会话记录）

- `cargo run -p swe-fog` 可启动独立实验场；根目录默认入口仍是 Ocean。
- GPU 数值检查通过；空间采样对照固定场/相机的高分辨率平均不透明度，误差约 0.07%–0.20%。
- 文档已产出：`doco/specs/swe-fog.md`、`doco/guides/fog.md`，并更新 `doco/architecture.md`。
- `cargo check -p swe-fog` 在本轮提交前通过（2.11s，离线）。
- `cargo test -p swe-fog` 通过：9 项测试全绿，含两项 egui 输入测试
  （`app::ui::tests::held_drag_and_precise_input_deliver_reset_before_release…`、
  `app::ui::tests::shift_drag_is_finer_and_initialization_switch_does_not_request_a_reset`）、
  2 项 CLI 契约测试、5 项 contracts 测试。

## 未完成 / 待人工验收

1. **桌面键鼠人工验收（未通过，不得记为通过）**
   覆盖 WASD 移动、跳跃与落地冲击、鼠标捕获/释放、Dock 手势。
   自动化键鼠被 Windows 焦点/输入权限阻断；egui 输入测试不等价于真实桌面交互。
2. **视觉验收**
   1 cm 薄层积分、前景遮挡、雾内视线、暂停连续帧、环绕返回尚未逐项验收；
   垂直曲线在参数沿视线变化时是近似，全视角画质不作保证。
3. **Ocean 回归**
   本轮未跑完 Ocean 侧的回归（公共框架 `crates/common` 暴露了新的 `device_event`，
   需确认 Ocean 的视口拖拽等行为未受影响）。
4. **集成与性能**
   尚未接入 Ocean 的同屏水雾合成；跨设备固定毫秒预算未验证。
5. **记录缺口**
   最后一次会话被上游 SSE 超时中断，没有留下完整的执行日志结论；
   上面的"已完成"条目建议接手者用 `--verify` 重新跑一遍。

## 验证命令

```bash
cargo run -p swe-fog
cargo run -p swe-fog -- --verify
cargo test -p swe-fog
cargo test --workspace
```
