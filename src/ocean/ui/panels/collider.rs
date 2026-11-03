use crate::ocean::parameters::optics::OpticsConfig;
use crate::ocean::scene::collider::Collider;
use crate::ocean::scene::collider::Shape;
use yummy_egui::{PanelButton, ParameterSlider, action_row, choices, parameter, parameter_table};

pub fn show(ui: &mut egui::Ui, optics: &mut OpticsConfig, domain: [f32; 4]) {
    let resolution = optics.shore.resolution;
    let model = optics.model;
    let interior = if model.enabled {
        model.interior(resolution)
    } else {
        [
            domain[0],
            domain[1],
            domain[0] + 0.78 * domain[2],
            domain[1] + domain[3],
        ]
    };
    yummy_egui::h3(ui, "静态碰撞体");
    parameter_table(ui, "collider actions", |ui| {
        parameter(
            ui,
            "工作状态",
            "在沙滩或模型场景且 SWE 开启时参与模拟、显示。固定在海底，低于顶部时挡水，超过顶部时可漫过；不模拟漂浮或刚体运动。",
            |ui| {
                ui.label(if optics.swe_enabled() {
                    "已启用"
                } else {
                    "请开启海岸场景与 SWE"
                });
            },
        );
        parameter(
            ui,
            "有效 X 范围（米）",
            "整个形状必须处于局部 SWE 区域，并避开外海 FFT 输入带。边缘还预留一格。",
            |ui| ui.label(format!("{:.1} ～ {:.1}", interior[0], interior[2])),
        );
        parameter(
            ui,
            "有效 Z 范围（米）",
            "沿岸范围，边缘预留一格；超出时保留配置，但不参与显示和碰撞。",
            |ui| ui.label(format!("{:.1} ～ {:.1}", interior[1], interior[3])),
        );
        parameter(
            ui,
            "网格间距 X/Z（米）",
            "建议碰撞体在两个方向上均覆盖至少 3 格。形状以网格采样，尺寸过小可能漏采样。",
            |ui| {
                ui.label(format!(
                    "{:.2} / {:.2}",
                    domain[2] / resolution as f32,
                    domain[3] / resolution as f32
                ))
            },
        );
        parameter(
            ui,
            "添加",
            "最多 8 个。默认放在岸线向海 8 米处。编辑即时生效并自动保存，不重置 SWE；仅调整底面变化处的水体，保留其他历史。不是连续移动的刚体。",
            |ui| {
                let can_add = optics.shore.colliders.iter().any(Option::is_none);
                let responses = action_row(
                    ui,
                    [
                        PanelButton::new("圆柱").enabled(can_add),
                        PanelButton::new("长方体").enabled(can_add),
                    ],
                );
                for (response, shape) in responses.into_iter().zip([Shape::Cylinder, Shape::Box]) {
                    if response.clicked()
                        && let Some((index, slot)) = optics
                            .shore
                            .colliders
                            .iter_mut()
                            .enumerate()
                            .find(|(_, c)| c.is_none())
                    {
                        *slot = Some(Collider {
                            shape,
                            position: if model.enabled {
                                [model.center[0], model.center[2] + index as f32 * 8.0]
                            } else {
                                [optics.shore_x + 8.0, index as f32 * 8.0]
                            },
                            ..Default::default()
                        });
                    }
                }
            },
        );
    });
    let mut remove = None;
    for (index, item) in optics.shore.colliders.iter_mut().enumerate() {
        let Some(c) = item else {
            continue;
        };
        ui.separator();
        ui.push_id(index, |ui| {
            yummy_egui::h3(ui, format!("碰撞体 {}", index + 1));
            parameter_table(ui, "collider parameters", |ui| {
                parameter(ui, "启用", "开启或关闭；即时生效并保存，不重置 SWE，暂停时也会应用。", |ui| ui.checkbox(&mut c.enabled, ""));
                parameter(ui, "形状", "圆柱或可旋转长方体，底部连接海底；即时生效并保存，不重置 SWE。", |ui| {
                    choices(ui, &mut c.shape, &[(Shape::Cylinder, "圆柱"), (Shape::Box, "长方体")]);
                });
                for (axis, label) in [(0, "位置 X（米）"), (1, "位置 Z（米）")] {
                    parameter(ui, label, "世界坐标，范围 ±1000 米。X 增大朝向外海，Z 沿岸；即时生效并保存，不重置 SWE。整个形状须在有效范围内。", |ui| ui.add(egui::DragValue::new(&mut c.position[axis]).speed(0.1).range(-1000.0..=1000.0)));
                }
                if c.shape == Shape::Cylinder {
                    parameter(ui, "半径（米）", "范围 0.25～30 米；即时生效并保存，不重置 SWE。小于网格间距的形状可能无法分辨。", |ui| ui.add(egui::DragValue::new(&mut c.radius).speed(0.1).range(0.25..=30.0)));
                } else {
                    for (axis, label) in [(0, "宽度（米）"), (1, "长度（米）")] {
                        parameter(ui, label, "完整尺寸，范围 0.5～60 米；即时生效并保存，不重置 SWE。", |ui| ui.add(egui::DragValue::new(&mut c.size[axis]).speed(0.1).range(0.5..=60.0)));
                    }
                    parameter(ui, "旋转（度）", "绕竖直轴旋转，范围 ±180 度；即时生效并保存，不重置 SWE。", |ui| ui.add(ParameterSlider::new(&mut c.angle, -180.0..=180.0)));
                }
                parameter(ui, "高度（米）", "范围 0.1～30 米，从中心处原海底向上量到水平顶面。水面超过顶面可以漫过；即时生效并保存，不重置 SWE。", |ui| ui.add(egui::DragValue::new(&mut c.height).speed(0.1).range(0.1..=30.0)));
                parameter(ui, "顶面海拔（米）", "中心海底高度加碰撞体高度，相对海平面。模型场景使用重采样床面，不使用沙坡公式。", |ui| ui.label(if model.enabled {"烘焙床面 + 高度".to_string()} else {format!("{:.2}", -optics.beach_slope * (c.position[0] - optics.shore_x) + c.height)}));
                parameter(ui, "模拟状态", "超出有效区域的碰撞体保留配置，但暂不显示、不参与碰撞。FFT 远海没有碰撞反馈。", |ui| {
                    let extent=c.extent();
                    let inside=if model.enabled {(0..2).all(|a|c.position[a]-extent[a]>=interior[a] && c.position[a]+extent[a]<=interior[a+2])} else {c.in_domain(domain,resolution)};
                    ui.label(if !c.enabled { "已关闭" } else if !inside { "超出有效区域" } else { "范围内" });
                });
                parameter(ui, "操作", "删除即时生效并保存，不重置 SWE；空出的区域在继续运行后由周围水流回填。", |ui| { if ui.add(PanelButton::new("删除")).clicked() { remove = Some(index); } });
            });
            let size = if c.shape == Shape::Cylinder { c.radius * 2.0 } else { c.size[0].min(c.size[1]) };
            if size < 3.0 * domain[2].max(domain[3]) / resolution as f32 {
                ui.label("尺寸较小，建议增大以改善网格对形状的分辨。");
            }
        });
    }
    if let Some(index) = remove {
        optics.shore.colliders[index] = None;
    }
}
