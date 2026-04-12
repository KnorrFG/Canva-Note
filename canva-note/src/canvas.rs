use std::sync::Arc;

use eframe::egui::{
    self, Area, Color32, CornerRadius, Frame, Image, ImageSource, Margin, PointerButton, Sense,
    Stroke, StrokeKind, load::SizedTexture,
};
use egui_commonmark::CommonMarkViewer;

use crate::{
    app::{App, DragState, NodeKind},
    commands::MoveNodeCommand,
    editor,
};

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

            let node_ids = self.nodes.keys().copied().collect::<Vec<_>>();
            for node_id in node_ids {
                let node_pos = self.document.node(node_id).unwrap().pos();
                let egui_id = self.nodes[&node_id].egui_id;
                let is_markdown = self.document.as_text_node_id(node_id).is_some();

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

                if let Some(ptr_pos) = ptr_pos {
                    if secondary_pressed
                        && !global_drag_active
                        && resp.response.rect.contains(ptr_pos)
                    {
                        self.active_drag = Some(DragState {
                            node_id,
                            start_pos: self.document.node(node_id).unwrap().pos(),
                        });
                    }

                    if self.active_drag.as_ref().map(|drag| drag.node_id) == Some(node_id) {
                        *self.document.node_mut(node_id).unwrap().pos_mut() += ptr_delta;
                    }

                    if !global_drag_active
                        && primary_double_click_occured
                        && resp.response.rect.contains(ptr_pos)
                        && is_markdown
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
            }

            if !secondary_down && let Some(drag) = self.active_drag.take() {
                let end_pos = self.document.node(drag.node_id).unwrap().pos();
                if end_pos != drag.start_pos {
                    self.record_applied_command(
                        MoveNodeCommand {
                            id: drag.node_id,
                            from: drag.start_pos,
                            to: end_pos,
                        }
                        .into(),
                    );
                }
            }
        });
    }
}
