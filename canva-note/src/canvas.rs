use std::sync::Arc;

use eframe::egui::{
    self, Area, Frame, Image, ImageSource, Margin, PointerButton, Rect, Sense, load::SizedTexture,
};
use egui::Window;
use egui_commonmark::CommonMarkViewer;

use crate::{
    app::{App, DragState, NodeKind},
    commands::MoveNodeCommand,
    editor,
};

#[derive(Clone, Copy)]
enum SelectedNodeSizing {
    Width(f32),
    Size(egui::Vec2),
}

impl SelectedNodeSizing {
    fn resize_state_salt(self, egui_id: egui::Id) -> impl std::hash::Hash {
        match self {
            Self::Width(width) => ("selected_width", egui_id.value(), width.round() as i32, 0),
            Self::Size(size) => (
                "selected_size",
                egui_id.value(),
                size.x.round() as i32,
                size.y.round() as i32,
            ),
        }
    }
}

fn fit_size_with_aspect(source_size: egui::Vec2, available_size: egui::Vec2) -> egui::Vec2 {
    let scale = (available_size.x / source_size.x)
        .min(available_size.y / source_size.y)
        .max(0.0);
    source_size * scale
}

fn show_zoomed_markdown(
    ui: &mut egui::Ui,
    zoom: f32,
    cache: &mut egui_commonmark::CommonMarkCache,
    content: &str,
    width: Option<usize>,
) {
    for font_id in ui.style_mut().text_styles.values_mut() {
        font_id.size *= zoom;
    }
    let mut viewer = CommonMarkViewer::new();
    if let Some(width) = width {
        viewer = viewer.default_width(Some(width));
    }
    viewer.show(ui, cache, content);
}

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

fn show_node_container(
    ctx: &egui::Context,
    egui_id: egui::Id,
    screen_pos: egui::Pos2,
    sizing: SelectedNodeSizing,
    add_contents: impl FnOnce(&mut egui::Ui) -> Rect,
) -> egui::Response {
    let mut window = Window::new("")
        .id(egui_id)
        .title_bar(false)
        .collapsible(false)
        .resize(|r| {
            let (resizable, with_stroke) = match sizing {
                SelectedNodeSizing::Width(_) => ([true, false], false),
                SelectedNodeSizing::Size(_) => ([true, true], true),
            };
            r.with_stroke(with_stroke)
                .resizable(resizable)
                .min_size(egui::Vec2::ZERO)
                .id_salt(sizing.resize_state_salt(egui_id))
        })
        .movable(false)
        .current_pos(screen_pos)
        .constrain(false);
    window = match sizing {
        SelectedNodeSizing::Width(width) => window
            .default_width(width)
            .default_height(0.0)
            .min_height(0.0),
        SelectedNodeSizing::Size(size) => window.default_size(size),
    };
    window.show(ctx, add_contents).unwrap().response
}

fn show_unselected_node_container(
    ctx: &egui::Context,
    egui_id: egui::Id,
    screen_pos: egui::Pos2,
    add_contents: impl FnOnce(&mut egui::Ui) -> Rect,
) -> egui::Response {
    Area::new(egui_id)
        .fixed_pos(screen_pos)
        .sense(Sense::click_and_drag())
        .constrain(false)
        .show(ctx, add_contents)
        .response
}

fn show_selected_markdown(
    ctx: &egui::Context,
    egui_id: egui::Id,
    screen_pos: egui::Pos2,
    zoom: f32,
    text: &crate::document::TextData,
    cache: &mut egui_commonmark::CommonMarkCache,
) -> egui::Response {
    show_node_container(
        ctx,
        egui_id,
        screen_pos,
        SelectedNodeSizing::Width(text.width as f32 * zoom),
        |ui| {
            Frame::NONE
                .inner_margin(Margin::same(4))
                .show(ui, |ui| {
                    show_zoomed_markdown(ui, zoom, cache, &text.content, None);
                })
                .response
                .rect
        },
    )
}

fn show_unselected_markdown(
    ctx: &egui::Context,
    egui_id: egui::Id,
    screen_pos: egui::Pos2,
    zoom: f32,
    text: &crate::document::TextData,
    cache: &mut egui_commonmark::CommonMarkCache,
) -> egui::Response {
    show_unselected_node_container(ctx, egui_id, screen_pos, |ui| {
        Frame::NONE
            .inner_margin(Margin::same(4))
            .show(ui, |ui| {
                ui.set_width(text.width as f32 * zoom);
                show_zoomed_markdown(
                    ui,
                    zoom,
                    cache,
                    &text.content,
                    Some((text.width as f32 * zoom).round() as usize),
                );
            })
            .response
            .rect
    })
}

fn show_selected_image(
    ctx: &egui::Context,
    egui_id: egui::Id,
    screen_pos: egui::Pos2,
    zoom: f32,
    image_size: egui::Vec2,
    source_size: egui::Vec2,
    texture: &eframe::egui::TextureHandle,
) -> egui::Response {
    show_node_container(
        ctx,
        egui_id,
        screen_pos,
        SelectedNodeSizing::Size(image_size * zoom),
        |ui| {
            Frame::NONE
                .inner_margin(Margin::same(4))
                .show(ui, |ui| {
                    let display_size =
                        fit_size_with_aspect(source_size * zoom, ui.available_size());
                    ui.add(
                        Image::new(ImageSource::Texture(SizedTexture::from_handle(texture)))
                            .fit_to_exact_size(display_size),
                    )
                    .rect
                })
                .inner
        },
    )
}

fn show_unselected_image(
    ctx: &egui::Context,
    egui_id: egui::Id,
    screen_pos: egui::Pos2,
    zoom: f32,
    image_size: egui::Vec2,
    texture: &eframe::egui::TextureHandle,
) -> egui::Response {
    show_unselected_node_container(ctx, egui_id, screen_pos, |ui| {
        Frame::NONE
            .inner_margin(Margin::same(4))
            .show(ui, |ui| {
                ui.add(
                    Image::new(ImageSource::Texture(SizedTexture::from_handle(texture)))
                        .fit_to_exact_size(image_size * zoom),
                )
                .rect
            })
            .inner
    })
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
                self.camera_pos -= self.screen_to_world_delta(ptr_delta);
                global_drag_active = true;
            }

            let resp = ui.allocate_response(ui.available_size(), Sense::click_and_drag());
            if resp.double_clicked()
                && let Some(pos) = ptr_pos
            {
                self.create_new_node_and_open_editor(self.screen_to_world_pos(pos));
            }
            if resp.clicked_by(PointerButton::Primary) {
                self.selected = None;
            }

            if !global_drag_active
                && self.active_drag.is_none()
                && resp.dragged_by(PointerButton::Secondary)
            {
                ui.ctx().input(|i| {
                    self.camera_pos -= self.screen_to_world_delta(i.pointer.delta());
                });
            }

            let mut node_ids = self.nodes.keys().copied().collect::<Vec<_>>();
            node_ids.sort_unstable();
            let mut node_rects = Vec::with_capacity(node_ids.len());
            let ptr_delta_world = self.screen_to_world_delta(ptr_delta);
            for node_id in node_ids {
                let node_pos = self.document.node(node_id).unwrap().pos();
                let screen_pos = self.world_to_screen_pos(node_pos);
                let egui_id = self.nodes[&node_id].egui_id;
                let zoom = self.zoom;
                let selected = self.selected == Some(node_id);

                let resp = if selected {
                    match &mut self.nodes.get_mut(&node_id).unwrap().kind {
                        NodeKind::Markdown(md_node) => {
                            let text = self.document.node(node_id).unwrap().as_text().unwrap();
                            if secondary_down {
                                ui.style_mut().interaction.selectable_labels = false;
                            }
                            let response = show_selected_markdown(
                                ui.ctx(),
                                egui_id,
                                screen_pos,
                                zoom,
                                text,
                                &mut md_node.cache,
                            );

                            response
                        }
                        NodeKind::Image(image_node) => {
                            let (image_size, source_size) =
                                match self.document.node(node_id).unwrap() {
                                    crate::document::NodeData::Image(image) => (
                                        image.size,
                                        egui::vec2(
                                            image.data.size[0] as f32,
                                            image.data.size[1] as f32,
                                        ),
                                    ),
                                    _ => unreachable!(),
                                };
                            let response = show_selected_image(
                                ui.ctx(),
                                egui_id,
                                screen_pos,
                                zoom,
                                image_size,
                                source_size,
                                &image_node.texture,
                            );
                            response
                        }
                    }
                } else {
                    match &mut self.nodes.get_mut(&node_id).unwrap().kind {
                        NodeKind::Markdown(md_node) => {
                            let text = self.document.node(node_id).unwrap().as_text().unwrap();
                            if secondary_down {
                                ui.style_mut().interaction.selectable_labels = false;
                            }
                            show_unselected_markdown(
                                ui.ctx(),
                                egui_id,
                                screen_pos,
                                zoom,
                                text,
                                &mut md_node.cache,
                            )
                        }
                        NodeKind::Image(image_node) => {
                            let image_size = match self.document.node(node_id).unwrap() {
                                crate::document::NodeData::Image(image) => image.size,
                                _ => unreachable!(),
                            };
                            show_unselected_image(
                                ui.ctx(),
                                egui_id,
                                screen_pos,
                                zoom,
                                image_size,
                                &image_node.texture,
                            )
                        }
                    }
                };

                if resp.clicked_by(PointerButton::Primary) {
                    self.selected = Some(node_id);
                }

                node_rects.push((node_id, resp.rect));

                if dragged_node(&self.active_drag, global_drag_active) == Some(node_id) {
                    *self.document.node_mut(node_id).unwrap().pos_mut() += ptr_delta_world;
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
