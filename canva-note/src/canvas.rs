use std::sync::Arc;

use eframe::egui::{
    self, Area, Color32, CornerRadius, Frame, Image, ImageSource, Margin, PointerButton, Rect,
    Sense, Stroke, StrokeKind, load::SizedTexture,
};
use egui_commonmark::CommonMarkViewer;

use crate::{
    app::{App, DragState, NodeKind},
    commands::MoveNodeCommand,
    editor,
};

fn hit_test_node(rects: &[(u64, Rect)], point: egui::Pos2) -> Option<u64> {
    rects
        .iter()
        .rev()
        .find(|(_, rect)| rect.contains(point))
        .map(|(node_id, _)| *node_id)
}

fn maybe_start_drag(
    active_drag: &Option<DragState>,
    global_drag_active: bool,
    hit_node: Option<u64>,
    start_pos: impl FnOnce(u64) -> egui::Pos2,
) -> Option<DragState> {
    debug_assert!(
        active_drag.is_none(),
        "started a new drag while one was already active"
    );
    debug_assert!(
        !global_drag_active,
        "started a node drag while a global drag was active"
    );

    let node_id = hit_node?;
    Some(DragState {
        node_id,
        start_pos: start_pos(node_id),
    })
}

fn finish_drag(
    active_drag: &mut Option<DragState>,
    end_pos: egui::Pos2,
) -> Option<MoveNodeCommand> {
    let drag = active_drag.take()?;
    (drag.start_pos != end_pos).then_some(MoveNodeCommand {
        id: drag.node_id,
        from: drag.start_pos,
        to: end_pos,
    })
}

fn dragged_node(active_drag: &Option<DragState>, global_drag_active: bool) -> Option<u64> {
    if global_drag_active {
        None
    } else {
        active_drag.as_ref().map(|drag| drag.node_id)
    }
}

impl App {
    pub(crate) fn canvas_ui(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let mut global_drag_active = false;
            let (
                secondary_down,
                secondary_pressed,
                ctrl_down,
                ptr_delta,
                ptr_pos,
                primary_double_click_occured,
            ) = ui.input(|i| {
                (
                    i.pointer.button_down(PointerButton::Secondary),
                    i.pointer.button_pressed(PointerButton::Secondary),
                    i.modifiers.ctrl,
                    i.pointer.delta(),
                    i.pointer.interact_pos(),
                    i.pointer.button_double_clicked(PointerButton::Primary),
                )
            });

            if secondary_down && ctrl_down {
                self.camera_pos -= ptr_delta;
                global_drag_active = true;
            }

            let resp = ui.allocate_response(ui.available_size(), Sense::click_and_drag());
            if resp.double_clicked()
                && let Some(pos) = ptr_pos
            {
                self.create_new_node_and_open_editor(pos + self.camera_pos.to_vec2());
            }
            if resp.clicked_by(PointerButton::Primary) {
                self.selected = None;
            }

            if !global_drag_active
                && self.active_drag.is_none()
                && resp.dragged_by(PointerButton::Secondary)
            {
                ui.ctx().input(|i| {
                    self.camera_pos -= i.pointer.delta();
                });
            }

            let mut node_ids = self.nodes.keys().copied().collect::<Vec<_>>();
            node_ids.sort_unstable();
            let mut node_rects = Vec::with_capacity(node_ids.len());
            for node_id in node_ids {
                let node_pos = self.document.node(node_id).unwrap().pos();
                let egui_id = self.nodes[&node_id].egui_id;

                let resp = match &mut self.nodes.get_mut(&node_id).unwrap().kind {
                    NodeKind::Markdown(md_node) => {
                        let text = self.document.node(node_id).unwrap().as_text().unwrap();
                        let area = Area::new(egui_id)
                            .fixed_pos(node_pos - self.camera_pos.to_vec2())
                            .sense(Sense::click_and_drag())
                            .constrain(false);

                        if secondary_down {
                            ui.style_mut().interaction.selectable_labels = false;
                        }
                        area.show(ui.ctx(), |ui| {
                            Frame::NONE.inner_margin(Margin::same(4)).show(ui, |ui| {
                                CommonMarkViewer::new()
                                    .default_width(Some(text.width))
                                    .show(ui, &mut md_node.cache, &text.content);
                            });
                        })
                    }
                    NodeKind::Image(image_node) => {
                        let area = Area::new(egui_id)
                            .fixed_pos(node_pos - self.camera_pos.to_vec2())
                            .sense(Sense::click_and_drag())
                            .constrain(false);

                        area.show(ui.ctx(), |ui| {
                            Frame::NONE.inner_margin(Margin::same(4)).show(ui, |ui| {
                                ui.add(
                                    Image::new(ImageSource::Texture(SizedTexture::from_handle(
                                        &image_node.texture,
                                    )))
                                    .fit_to_original_size(1.0),
                                );
                            });
                        })
                    }
                };

                if resp.response.clicked_by(PointerButton::Primary) {
                    self.selected = Some(node_id);
                }

                if self.selected == Some(node_id) {
                    ui.painter().rect_stroke(
                        resp.response.rect.expand(10.0),
                        CornerRadius::same(4),
                        Stroke::new(1.5, Color32::BLACK),
                        StrokeKind::Outside,
                    );
                }

                node_rects.push((node_id, resp.response.rect));

                if dragged_node(&self.active_drag, global_drag_active) == Some(node_id) {
                    *self.document.node_mut(node_id).unwrap().pos_mut() += ptr_delta;
                }
            }

            if let Some(ptr_pos) = ptr_pos {
                let hit_node = hit_test_node(&node_rects, ptr_pos);
                if secondary_pressed && !global_drag_active {
                    self.active_drag = maybe_start_drag(
                        &self.active_drag,
                        global_drag_active,
                        hit_node,
                        |node_id| self.document.node(node_id).unwrap().pos(),
                    );
                }

                if !global_drag_active
                    && primary_double_click_occured
                    && let Some(node_id) = hit_node
                    && self.nodes[&node_id].kind.is_markdown()
                {
                    let text_node_id = self.document.as_text_node_id(node_id).unwrap();
                    editor::spawn(
                        text_node_id,
                        &self.document.text(text_node_id).content,
                        self.editor_tx.clone(),
                        Arc::clone(&self.shutdown),
                    );
                }
            }

            if !secondary_down
                && let Some(end_pos) = self
                    .active_drag
                    .as_ref()
                    .map(|drag| self.document.node(drag.node_id).unwrap().pos())
                && let Some(command) = finish_drag(&mut self.active_drag, end_pos)
            {
                self.record_applied_command(command.into());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2};

    #[test]
    fn hit_test_returns_last_matching_node() {
        let rects = vec![
            (1, Rect::from_min_size(pos2(0.0, 0.0), vec2(50.0, 50.0))),
            (2, Rect::from_min_size(pos2(25.0, 25.0), vec2(50.0, 50.0))),
        ];

        assert_eq!(hit_test_node(&rects, pos2(10.0, 10.0)), Some(1));
        assert_eq!(hit_test_node(&rects, pos2(30.0, 30.0)), Some(2));
        assert_eq!(hit_test_node(&rects, pos2(100.0, 100.0)), None);
    }

    #[test]
    fn drag_starts_only_on_secondary_press_without_global_drag() {
        let started = maybe_start_drag(&None, false, Some(7), |_| pos2(10.0, 20.0));
        assert!(matches!(
            started,
            Some(DragState {
                node_id: 7,
                start_pos
            }) if start_pos == pos2(10.0, 20.0)
        ));

        assert!(maybe_start_drag(&None, false, None, |_| pos2(0.0, 0.0)).is_none());
    }

    #[test]
    fn drag_finish_emits_one_move_command_only_when_position_changed() {
        let mut active_drag = Some(DragState {
            node_id: 5,
            start_pos: pos2(10.0, 20.0),
        });
        let command = finish_drag(&mut active_drag, pos2(30.0, 40.0));
        assert!(matches!(
            command,
            Some(MoveNodeCommand {
                id: 5,
                from,
                to
            }) if from == pos2(10.0, 20.0) && to == pos2(30.0, 40.0)
        ));
        assert!(active_drag.is_none());

        let mut stationary_drag = Some(DragState {
            node_id: 5,
            start_pos: pos2(10.0, 20.0),
        });
        assert!(finish_drag(&mut stationary_drag, pos2(10.0, 20.0)).is_none());
        assert!(stationary_drag.is_none());
    }

    #[test]
    fn global_drag_suppresses_node_drag_updates() {
        let active_drag = Some(DragState {
            node_id: 5,
            start_pos: pos2(10.0, 20.0),
        });

        assert_eq!(dragged_node(&active_drag, false), Some(5));
        assert_eq!(dragged_node(&active_drag, true), None);
        assert_eq!(dragged_node(&None, false), None);
    }
}
