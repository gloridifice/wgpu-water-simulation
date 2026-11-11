use super::*;
use crate::volumes::FogVolume;
#[derive(Clone, Default)]
struct Draft {
    applied: Vec<FogVolume>,
    boxes: Vec<FogVolume>,
    selected: usize,
    show: bool,
}
fn id() -> egui::Id {
    egui::Id::new("fog volume draft")
}
fn draft(ui: &egui::Ui, session: &Session) -> Draft {
    let mut d = ui
        .ctx()
        .data_mut(|data| data.get_temp::<Draft>(id()))
        .unwrap_or_else(|| Draft {
            show: true,
            ..Default::default()
        });
    if d.applied != session.volumes {
        d.applied = session.volumes.clone();
        d.boxes = session.volumes.clone();
        d.selected = d.selected.min(d.boxes.len().saturating_sub(1));
    }
    d
}
pub(super) fn editor(ui: &mut egui::Ui, s: &mut Session) {
    yummy_egui::h3(ui, "局部雾场盒体");
    let mut d = draft(ui, s);
    ui.checkbox(&mut d.show, "显示盒体线框（穿透）");
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(d.boxes.len() < 8, egui::Button::new("新增"))
            .clicked()
        {
            d.boxes.push(FogVolume {
                bottom: s.player.feet,
                name: format!("雾场 {}", d.boxes.len() + 1),
                ..Default::default()
            });
            d.selected = d.boxes.len() - 1;
        }
        if ui
            .add_enabled(
                !d.boxes.is_empty() && d.boxes.len() < 8,
                egui::Button::new("复制"),
            )
            .clicked()
        {
            let mut b = d.boxes[d.selected].clone();
            b.name.push_str(" 副本");
            d.boxes.push(b);
            d.selected = d.boxes.len() - 1;
        }
        if ui
            .add_enabled(!d.boxes.is_empty(), egui::Button::new("删除"))
            .clicked()
        {
            d.boxes.remove(d.selected);
            d.selected = d.selected.min(d.boxes.len().saturating_sub(1));
        }
    });
    for (i, b) in d.boxes.iter().enumerate() {
        ui.selectable_value(&mut d.selected, i, &b.name);
    }
    if let Some(b) = d.boxes.get_mut(d.selected) {
        parameter_table(ui, "fog volume transform", |ui| {
            parameter(
                ui,
                "名称",
                "用于区分盒体；随会话手动保存。",
                |ui| ui.text_edit_singleline(&mut b.name),
            );
            for (axis, label) in ["底心 X (m)", "底面 Y (m)", "底心 Z (m)"]
                .into_iter()
                .enumerate()
            {
                number(
                    ui,
                    label,
                    "世界位置，±1000 m。编辑为草稿，点击应用后重新烘焙并重置全部雾场。",
                    &mut b.bottom[axis],
                    -1000.0,
                    1000.0,
                    false,
                );
            }
            for (axis, label) in ["宽度 (m)", "高度 (m)", "深度 (m)"].into_iter().enumerate()
            {
                number(
                    ui,
                    label,
                    "盒体局部完整尺寸，0.1–256 m。顶面以上、底面以下的几何不参与烘焙；跨界三角形裁切。点击应用生效。",
                    &mut b.size[axis],
                    0.1,
                    256.0,
                    false,
                );
            }
            number(
                ui,
                "Y 旋转 (°)",
                "绕底心、世界 Y 轴旋转，±360°；点击应用生效。",
                &mut b.yaw_degrees,
                -360.0,
                360.0,
                false,
            );
        });
    }
    ui.horizontal_wrapped(|ui| {
        if ui.button("应用盒体并烘焙").on_hover_text("每个盒体独立模拟；重叠区域光学厚度相加，不交换质量。空列表恢复原单雾场。烘焙失败保留旧资源；配置仍需手动保存。").clicked() {
            s.volumes = d.boxes.clone();
            d.applied = s.volumes.clone();
        }
        if ui.button("撤销草稿").clicked() { d.boxes = s.volumes.clone(); d.selected = 0; }
    });
    ui.ctx().data_mut(|data| data.insert_temp(id(), d));
}
pub(super) fn overlay(ui: &egui::Ui, rect: egui::Rect, s: &Session, matrix: glam::Mat4) {
    let d = draft(ui, s);
    if !d.show {
        return;
    }
    let painter = ui.painter().with_clip_rect(rect);
    for (index, b) in d.boxes.iter().enumerate() {
        let points = b.corners().map(|p| matrix * p.extend(1.0));
        let color = if index == d.selected {
            egui::Color32::YELLOW
        } else {
            egui::Color32::LIGHT_BLUE
        };
        for i in 0..8 {
            for bit in [1, 2, 4] {
                let j = i ^ bit;
                if j <= i {
                    continue;
                }
                let mut a = points[i];
                let mut b = points[j];
                // Clip against the near plane before perspective division.
                if a.z < 0.0 && b.z < 0.0 {
                    continue;
                }
                if a.z < 0.0 {
                    a = a.lerp(b, -a.z / (b.z - a.z));
                }
                if b.z < 0.0 {
                    b = b.lerp(a, -b.z / (a.z - b.z));
                }
                if a.w <= 0.0 || b.w <= 0.0 {
                    continue;
                }
                let project = |p: glam::Vec4| {
                    egui::pos2(
                        rect.left() + (p.x / p.w + 1.0) * 0.5 * rect.width(),
                        rect.top() + (1.0 - p.y / p.w) * 0.5 * rect.height(),
                    )
                };
                painter.line_segment([project(a), project(b)], egui::Stroke::new(1.5_f32, color));
            }
        }
    }
}
