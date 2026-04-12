use std::path::PathBuf;

use clap::Parser;
use eframe::egui::{
    self, Area, FontData, FontDefinitions, FontFamily, Id, Image, ImageSource, Pos2, Sense, Visuals,
    load::SizedTexture,
};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

fn main() {
    let args = Cli::parse();
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "Canva Note",
        native_options,
        Box::new(|cc| Ok(Box::new(MyEguiApp::new(cc, args.file)))),
    )
    .unwrap();
}

#[derive(clap::Parser)]
struct Cli {
    file: PathBuf,
}

struct MyEguiApp {
    camera_pos: Pos2,
    file_path: PathBuf,
    nodes: Vec<Node>,
}

struct Node {
    id: Id,
    pos: Pos2,
    kind: NodeKind,
}

enum NodeKind {
    Markdown(MarkdownNode),
    Image(ImageNode),
}

struct MarkdownNode {
    cache: CommonMarkCache,
    content: String,
}

struct ImageNode {
    texture: SizedTexture,
}

impl MyEguiApp {
    fn new(cc: &eframe::CreationContext<'_>, file_path: PathBuf) -> Self {
        // Always configure the `eframe`-provided context.
        // A fresh `egui::Context::default()` would be a different context and would not affect the app.
        egui_extras::install_image_loaders(&cc.egui_ctx);

        // Global font registration. `egui_commonmark` uses normal `egui` text styles underneath,
        // so markdown typography is mostly controlled through the same fonts/styles as the rest of the UI.
        let mut fonts = FontDefinitions::default();
        fonts.font_data.insert(
            "source_sans_3".into(),
            FontData::from_static(include_bytes!("../../SourceSans3-Regular.ttf")).into(),
        );
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "source_sans_3".into());
        cc.egui_ctx.set_fonts(fonts);

        // light mode.
        cc.egui_ctx.set_visuals(Visuals::light());

        // Global text sizing. `egui_commonmark` derives heading/body sizes from these text styles.
        cc.egui_ctx.global_style_mut(|style| {
            style.text_styles.insert(
                egui::TextStyle::Body,
                egui::FontId::new(18.0, egui::FontFamily::Proportional),
            );
            style.text_styles.insert(
                egui::TextStyle::Heading,
                egui::FontId::new(28.0, egui::FontFamily::Proportional),
            );
            style.text_styles.insert(
                egui::TextStyle::Monospace,
                egui::FontId::new(17.0, egui::FontFamily::Monospace),
            );
        });

        Self {
            camera_pos: Pos2 { x: 0., y: 0. },
            nodes: vec![],
            file_path,
        }
    }
}

impl eframe::App for MyEguiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let mut global_drag_active = false;
            let (secondary_down, ctrl_down, ptr_delta, ptr_pos) = ui.input(|i| {
                (
                    i.pointer.button_down(egui::PointerButton::Secondary),
                    i.modifiers.ctrl,
                    i.pointer.delta(),
                    i.pointer.interact_pos(),
                )
            });

            if secondary_down && ctrl_down {
                self.camera_pos -= ptr_delta;
                global_drag_active = true;
            }

            let resp = ui.allocate_response(ui.available_size(), Sense::click_and_drag());
            // if resp.double_clicked() {
            //     self.create_new_node_and_open_editor();
            // }

            if !global_drag_active && resp.dragged_by(egui::PointerButton::Secondary) {
                ui.ctx().input(|i| {
                    self.camera_pos -= i.pointer.delta();
                });
            }

            for node in &mut self.nodes {
                let img_area = Area::new(node.id)
                    .fixed_pos(node.pos - self.camera_pos.to_vec2())
                    .sense(Sense::click_and_drag())
                    .constrain(false);

                let resp = img_area.show(ui.ctx(), |ui| {
                    match &mut node.kind {
                        NodeKind::Markdown(md_node) => {
                            if secondary_down {
                                ui.style_mut().interaction.selectable_labels = false;
                            }
                            CommonMarkViewer::new().show(ui, &mut md_node.cache, &md_node.content);
                        }
                        NodeKind::Image(image_node) => {
                            ui.add(
                                Image::new(ImageSource::Texture(image_node.texture))
                                    .fit_to_original_size(1.0),
                            );
                        }
                    };
                });

                if let Some(ptr_pos) = ptr_pos
                    && !global_drag_active
                    && secondary_down
                    && resp.response.rect.contains(ptr_pos)
                {
                    node.pos += ptr_delta;
                }
            }
        });
    }
}
