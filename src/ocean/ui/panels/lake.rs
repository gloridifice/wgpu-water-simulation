use crate::ocean::{
    parameters::lake::{LakeConfig, PropagationMode},
    ui::gui::Gui,
};
use yummy_egui::{PanelButton, ParameterSlider, action_row, group, parameter, segmented_control};

#[derive(Default)]
pub struct LakePanel {
    draft: LakeConfig,
    source: Option<LakeConfig>,
    pub emit: bool,
    pub clear: bool,
    pub reset: bool,
    pub open: bool,
    pub open_ripples: bool,
    pub error: Option<String>,
    pub status: (usize, u64, u32),
}
fn number(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    hint: &str,
) {
    parameter(ui, label, hint, |ui| {
        ui.add(ParameterSlider::new(value, range).clamping(egui::SliderClamping::Edits))
    });
}
impl Gui {
    pub(in crate::ocean::ui) fn lake_panel(&mut self, ui: &mut egui::Ui) {
        self.lake_panel.show_terrain(ui, &mut self.optics.lake);
    }
    pub(in crate::ocean::ui) fn animated_sdf_panel(&mut self, ui: &mut egui::Ui) {
        if !self.optics.lake.enabled {
            ui.label("仅用于湖泊场景；请先在“场景”分类选择“湖泊场景”。");
            return;
        }
        self.lake_panel.show_ripples(ui, &mut self.optics.lake);
    }
}
impl LakePanel {
    fn sync_draft(&mut self, config: &LakeConfig) {
        if let Some(source) = self.source {
            // Applying one panel must not discard the other panel's pending edits.
            if source.terrain != config.terrain {
                self.draft.terrain = config.terrain;
            }
            if source.mode != config.mode {
                self.draft.mode = config.mode;
            }
            if source.wave_seed != config.wave_seed {
                self.draft.wave_seed = config.wave_seed;
            }
        } else {
            self.draft = *config;
        }
        self.source = Some(*config);
    }
    fn apply(&mut self, config: &mut LakeConfig, next: LakeConfig) {
        match next.validate() {
            Ok(()) => {
                *config = next;
                self.source = Some(next);
                self.reset = true;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }
    fn show_terrain(&mut self, ui: &mut egui::Ui, config: &mut LakeConfig) {
        self.sync_draft(config);
        let panel = self;
        let d = &mut panel.draft;
        group(ui, "湖泊地形（应用后重置）", |ui| {
            number(
                ui,
                "湖泊半径（米）",
                &mut d.terrain.radius,
                8.0..=100.0,
                "基础半径 8～100 米；岸线噪波围绕它变化，显式应用后重建并清空波纹。",
            );
            number(
                ui,
                "最大水深（米）",
                &mut d.terrain.depth,
                0.5..=12.0,
                "碗状湖底基础深度 0.5～12 米；应用后重建。",
            );
            number(
                ui,
                "岸线不规则度",
                &mut d.terrain.irregularity,
                0.0..=0.35,
                "径向周期噪波比例 0～0.35，无量纲；应用后重建。",
            );
            parameter(
                ui,
                "场景种子",
                "确定性地形及岩石种子；应用后重建。",
                |ui| ui.add(ParameterSlider::new(&mut d.terrain.seed, 0..=100000)),
            );
            parameter(
                ui,
                "网格分辨率",
                "场景和波场共用网格；应用后重建，512 的开销更高。",
                |ui| {
                    segmented_control(
                        ui,
                        &mut d.terrain.resolution,
                        &[(128, "128"), (256, "256"), (512, "512")],
                    )
                },
            );
            parameter(
                ui,
                "岩石数量",
                "0～32；间距不足时跳过，实际数量显示在状态中。每第三块尝试水下岩石。应用后重建。",
                |ui| ui.add(ParameterSlider::new(&mut d.terrain.rocks, 0..=32)),
            );
            number(
                ui,
                "岩石尺寸（米）",
                &mut d.terrain.rock_size,
                0.5..=8.0,
                "岩石基础水平半径 0.5～8 米；应用后重建。岩石为单层高度几何，无悬挑。",
            );
        });
        if ui.add(PanelButton::new("应用地形并重置")).clicked() {
            let mut next = *config;
            next.terrain = d.terrain;
            panel.apply(config, next);
        }
        if let Some(e) = &panel.error {
            ui.colored_label(egui::Color32::LIGHT_RED, e);
        }
        group(ui, "共享 FFT 波浪", |ui| {
            number(
                ui,
                "FFT 叠加强度",
                &mut config.fft_strength,
                0.0..=1.0,
                "共享波谱高度的无量纲倍率 0～1，实时生效、不重置沿岸波；0 关闭 FFT。波谱在远海分类的“波谱与波形”面板编辑并应用，两场景共用；湖泊只叠加垂直波高，岸边及浅水渐隐，不搬动岸线，不生成泡沫。清空波纹只清沿岸波，不清 FFT。",
            );
        });
        group(ui, "卡通岸边泡沫", |ui| {
            number(
                ui,
                "泡沫强度",
                &mut config.foam_strength,
                0.0..=1.0,
                "覆盖不透明度，无量纲 0～1；0 关闭。实时生效、不重置；仅显示沿向心流向 SDF 增大的岸边／露水岩石背流侧，不输送模拟密度。",
            );
            number(
                ui,
                "泡沫宽度（米）",
                &mut config.foam_width,
                0.1..=10.0,
                "到岸边／露水岩石的网状泡沫带宽，0.1～10 米；实时生效、不重置，随波高轻微变化。",
            );
            number(
                ui,
                "网孔尺寸（米）",
                &mut config.foam_size,
                0.1..=5.0,
                "胞状网纹的基础尺寸，0.1～5 米；实时生效、不重置。越大则孔洞越大，动画随湖泊暂停。",
            );
            number(
                ui,
                "向心流速（米/秒）",
                &mut config.foam_flow_speed,
                0.0..=2.0,
                "泡沫 UV 从湖边向湖心流动，0～2 米/模拟秒；湖心平滑减速。0 关闭 UV 流动；实时生效、不重置，编辑可能改变当前图案，不是物理流速。",
            );
            number(
                ui,
                "流场扭曲",
                &mut config.foam_flow_distortion,
                0.0..=1.0,
                "近岸速度梯度强度，无量纲 0～1，默认 1；越强则贴岸更慢、离岸更快，材质坐标沿简化向心场积分产生拉伸与剪切。0 恢复近似匀速流，仍有向心运动。实时生效、不重置，编辑会改变当前图案；不修改波高、泡沫密度或方向遮罩。",
            );
            number(
                ui,
                "扭曲周期（秒）",
                &mut config.foam_flow_period,
                1.0..=20.0,
                "双 UV 每套的循环周期，1～20 模拟秒，错开半周期并用平滑三角波交叉淡化；周期越长，最大位移越大。实时生效、不重置，编辑会重新映射相位；暂停冻结动画。",
            );
        });
        if ui.add(PanelButton::new("打开动画 SDF 波纹")).clicked() {
            panel.open_ripples = true;
        }
    }
    fn show_ripples(&mut self, ui: &mut egui::Ui, config: &mut LakeConfig) {
        self.sync_draft(config);
        let panel = self;
        let d = &mut panel.draft;
        group(ui, "传播场（应用后重置）", |ui| {
            parameter(
                ui,
                "传播场",
                "深度为归一化水深；SDF 为到外岸及露水岩石边界的归一化欧氏距离。不是绕射或传播时间求解器。应用后清空波纹。",
                |ui| {
                    segmented_control(
                        ui,
                        &mut d.mode,
                        &[
                            (PropagationMode::Depth, "深度"),
                            (PropagationMode::Sdf, "SDF"),
                        ],
                    )
                },
            );
            parameter(
                ui,
                "波纹随机种子",
                "与地形种子独立；应用后重新开始波序列。",
                |ui| ui.add(ParameterSlider::new(&mut d.wave_seed, 0..=100000)),
            );
        });
        if ui
            .add(PanelButton::new("应用传播场 / 种子并重置"))
            .clicked()
        {
            let mut next = *config;
            next.mode = d.mode;
            next.wave_seed = d.wave_seed;
            panel.apply(config, next);
        }
        if let Some(e) = &panel.error {
            ui.colored_label(egui::Color32::LIGHT_RED, e);
        }
        group(ui, "波纹发射（影响新波）", |ui| {
            parameter(
                ui,
                "自动发射",
                "按模拟时间发射，暂停时停止；启停不清空已有波。",
                |ui| ui.checkbox(&mut config.automatic, "启用"),
            );
            number(
                ui,
                "发射间隔（秒）",
                &mut config.interval,
                0.1..=30.0,
                "模拟秒 0.1～30；编辑后从当前时刻重新安排下一次发射，不补旧波。",
            );
            number(
                ui,
                "世界方向角（度）",
                &mut config.direction_deg,
                -180.0..=180.0,
                "0° 朝世界 +X、90° 朝 +Z；标记湖心扇区，反射不翻转扇区；只影响新波。",
            );
            number(
                ui,
                "角度半宽（度）",
                &mut config.spread_deg,
                5.0..=150.0,
                "湖心扇区半宽 5～150°，不是固定世界长度；新波中间粗、两端细。",
            );
            number(
                ui,
                "方向随机偏差（度）",
                &mut config.jitter_deg,
                0.0..=180.0,
                "新波方向的正负随机角度范围，固定种子可重现。",
            );
        });
        group(ui, "波形与反射（影响新波及其后代）", |ui| {
            let p = match config.mode {
                PropagationMode::Depth => &mut config.depth_wave,
                PropagationMode::Sdf => &mut config.sdf_wave,
            };
            number(
                ui,
                "起始传播坐标",
                &mut p.start,
                0.1..=0.95,
                "无量纲 0.1～0.95；0 靠边界、1 为场最大值；必须大于反射坐标。两模式独立保存。",
            );
            number(
                ui,
                "传播进度／秒",
                &mut p.speed,
                0.005..=0.5,
                "无量纲坐标／模拟秒 0.005～0.5，并非恒定水平米／秒；只影响新波。",
            );
            number(
                ui,
                "传播坐标宽度",
                &mut p.width,
                0.005..=0.15,
                "中央波纹宽度，归一化场坐标 0.005～0.15，不是米；只影响新波。",
            );
            number(
                ui,
                "反射坐标",
                &mut p.reflection,
                0.005..=0.3,
                "入射抵达该坐标时反射；必须小于起始坐标；不是逐点岸边碰撞。只影响新波。",
            );
            number(
                ui,
                "波高（米）",
                &mut config.amplitude,
                0.0..=0.5,
                "波峰振幅 0～0.5 米；边界和过浅处限制最终位移，非守恒动画。只影响新波。",
            );
            number(
                ui,
                "两端宽度比例",
                &mut config.tip_ratio,
                0.05..=1.0,
                "端部／中央宽度比 0.05～1；高度在角包络两端平滑为零。只影响新波。",
            );
            parameter(
                ui,
                "岸边波反射",
                "每道入射波最多生成一次反向波；反射不是 SSR。只影响新波，已有后代参数冻结。",
                |ui| ui.checkbox(&mut config.reflect, "启用"),
            );
            if config.reflect {
                number(
                    ui,
                    "反射振幅比例",
                    &mut config.reflectivity,
                    0.0..=0.95,
                    "反射／入射振幅 0～0.95；反射逐渐衰减，不再反射。",
                );
                number(
                    ui,
                    "反射半衰期（秒）",
                    &mut config.reflected_half_life,
                    0.1..=30.0,
                    "振幅减半所需模拟秒，0.1～30；只影响新波的反射后代。",
                );
            }
            number(
                ui,
                "交接时间（秒）",
                &mut config.transition,
                0.05..=2.0,
                "入射淡出／反射淡入的模拟时间 0.05～2 秒，避免突然变弱；新波出生也淡入。",
            );
        });
        let [emit, clear] = action_row(
            ui,
            [PanelButton::new("发射一道"), PanelButton::new("清空波纹")],
        );
        if emit.clicked() {
            if let Err(e) = config.validate() {
                panel.error = Some(e);
            } else {
                panel.emit = true;
            }
        }
        panel.clear |= clear.clicked();
        group(ui, "诊断", |ui| {
            parameter(
                ui,
                "显示",
                "实时切换；深度、带符号 SDF（米）、传播等值线或波高；不重置波纹。",
                |ui| {
                    egui::ComboBox::from_id_salt("lake view")
                        .selected_text(
                            ["正常", "深度", "SDF", "等值线", "波高"][config.view as usize],
                        )
                        .show_ui(ui, |ui| {
                            for (v, label) in
                                ["正常", "深度", "SDF", "等值线", "波高"].iter().enumerate()
                            {
                                ui.selectable_value(&mut config.view, v as u32, *label);
                            }
                        });
                },
            );
            parameter(
                ui,
                "当前状态",
                "活动波上限 128，入射预留反射容量；池满拒绝新增，不覆盖可见波。配置不保存播放进度。",
                |ui| {
                    ui.label(format!(
                        "{} 波 / 丢弃 {} / {} 岩石",
                        panel.status.0, panel.status.1, panel.status.2
                    ))
                },
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mode_selection_is_a_draft_until_apply_and_actions_are_explicit() {
        let context = egui::Context::default();
        yummy_egui::Theme::default().apply(&context);
        crate::ocean::ui::fonts::install(&context);
        let mut panel = LakePanel::default();
        let mut config = LakeConfig {
            enabled: true,
            ..Default::default()
        };
        let mut frame = |events| {
            context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600.0, 2400.0),
                    )),
                    events,
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default()
                        .show_inside(root, |ui| panel.show_ripples(ui, &mut config));
                },
            )
        };
        let mut output = frame(vec![]);
        fn locate(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
            match shape {
                egui::epaint::Shape::Text(t) if t.galley.text() == label => {
                    Some(t.visual_bounding_rect().center())
                }
                egui::epaint::Shape::Vec(v) => v.iter().rev().find_map(|s| locate(s, label)),
                _ => None,
            }
        }
        let mut click = |label: &str| {
            let pos = output
                .shapes
                .iter()
                .rev()
                .find_map(|s| locate(&s.shape, label))
                .unwrap_or_else(|| panic!("missing control {label}"));
            for pressed in [true, false] {
                output = frame(vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
            }
        };
        click("SDF");
        // Observe state between clicks without introducing test-only production APIs.
        assert_eq!(config.mode, PropagationMode::Depth);
        assert!(!panel.reset);
        assert_eq!(panel.draft.mode, PropagationMode::Sdf);
        // A pending terrain edit belongs to the other panel, not this apply action.
        let original_radius = config.terrain.radius;
        panel.draft.terrain.radius = original_radius + 4.0;
        let pos = output
            .shapes
            .iter()
            .rev()
            .find_map(|s| locate(&s.shape, "应用传播场 / 种子并重置"))
            .unwrap();
        for pressed in [true, false] {
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600.0, 2400.0),
                    )),
                    events: vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default()
                        .show_inside(root, |ui| panel.show_ripples(ui, &mut config));
                },
            );
        }
        assert_eq!(config.mode, PropagationMode::Sdf);
        assert_eq!(config.terrain.radius, original_radius);
        assert!(panel.reset);
        assert!(!panel.emit && !panel.clear);
        panel.reset = false;
        let _ = context.run_ui(egui::RawInput::default(), |root| {
            egui::CentralPanel::default()
                .show_inside(root, |ui| panel.show_terrain(ui, &mut config));
        });
        assert_eq!(panel.draft.terrain.radius, original_radius + 4.0);
        assert!(!panel.reset);
    }
}
