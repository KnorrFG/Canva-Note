use eframe::egui::{
    self, Area, FontData, FontDefinitions, FontFamily, Id, Image, Label, Pos2, Sense, TextWrapMode,
    Vec2, Visuals,
};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

fn main() {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "Canva Note",
        native_options,
        Box::new(|cc| Ok(Box::new(MyEguiApp::new(cc)))),
    )
    .unwrap();
}

const MD_TEXT: &str = "
# Some MD Title

## A smaller title
I like mardown *it's cool*

**and bold**
";

// App state is the authoritative model in immediate-mode UI.
// Positions live here so they can later be serialized instead of being stored in widgets.
struct MyEguiApp {
    imageid: Id,
    textid: Id,
    image_pos: Pos2,
    // Camera is currently treated like a world-space position, but conceptually it is an offset.
    // Rendered position = object_pos - camera_pos.to_vec2().
    camera_pos: Pos2,
    md_cache: CommonMarkCache,
    md_pos: Pos2,
    md_id: Id,
}

impl MyEguiApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
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
            imageid: Id::new("image"),
            textid: Id::new("text"),
            md_pos: Pos2::new(400., 0.),
            md_id: Id::new("md"),
            image_pos: Pos2 { x: 100., y: 100. },
            camera_pos: Pos2 { x: 0., y: 0. },
            md_cache: CommonMarkCache::default(),
        }
    }
}

impl eframe::App for MyEguiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            // Background camera pan:
            // - secondary drag on empty canvas pans the camera
            // - Ctrl+secondary drag pans the camera even over objects
            // This is driven from pointer input, not only widget drag responses.
            let mut global_drag_active = false;
            // querying input state requires the usage of ui.input() closures
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

            // A full-panel response so the background participates in drag hit-testing.
            // this basically registeres a drag handler for the background. It uses all available space,
            // which currently is all of it, because nothing was rendered yet. (And it would work
            // further down too, because Areas don't block space, they float)
            let resp = ui.allocate_response(ui.available_size(), Sense::click_and_drag());
            if !global_drag_active && resp.dragged_by(egui::PointerButton::Secondary) {
                ui.ctx().input(|i| {
                    self.camera_pos -= i.pointer.delta();
                });
            }

            // `Area` gives absolute placement. Widgets inside it still use normal `egui` layout rules.
            // `Id` gives the area stable identity across frames for interaction/state tracking.
            // Widgets don't know abou their placement. That's a containers job, and Area is the
            // FreeFloating container. Otherwise there are layouts
            let img_area = Area::new(self.imageid)
                .fixed_pos(self.image_pos - self.camera_pos.to_vec2())
                .sense(Sense::click_and_drag())
                .constrain(false);
            let resp = img_area.show(ui.ctx(), |ui| {
                ui.add(
                    Image::new("file:///home/felix/Pictures/dobby.png").fit_to_original_size(1.0),
                )
            });
            // the response holds information about the rendered object
            if !global_drag_active && resp.response.dragged_by(egui::PointerButton::Secondary) {
                self.image_pos += resp.response.drag_delta();
            }

            let md_area = Area::new(self.md_id)
                .fixed_pos(self.md_pos - self.camera_pos.to_vec2())
                .sense(Sense::click_and_drag())
                .constrain(false);
            let resp = md_area.show(ui.ctx(), |ui| {
                if secondary_down {
                    ui.style_mut().interaction.selectable_labels = false;
                }
                CommonMarkViewer::new().show(ui, &mut self.md_cache, MD_TEXT);
            });

            // Markdown renders as ordinary `egui` labels/widgets.
            // While secondary is held we disable selectable labels so right-drag does not begin text selection.
            // Using `rect.contains(pointer_pos)` is more reliable than `hovered()` here because child widgets
            // can own hover even when the pointer is geometrically inside the markdown block.
            if let Some(ptr_pos) = ptr_pos
                && !global_drag_active
                && secondary_down
                && resp.response.rect.contains(ptr_pos)
            {
                self.md_pos += ptr_delta;
            }

            Area::new(self.textid)
                .constrain(false)
                .fixed_pos(Pos2 { x: 0., y: 0. } - self.camera_pos.to_vec2())
                .show(ui.ctx(), |ui| {
                    ui.add(
                        Label::new(format!("Image pos: {}", self.image_pos))
                            .wrap_mode(TextWrapMode::Extend),
                    );
                })
        });
    }
}

// Concurrency note:
// `egui::Context` is a cheap cloneable handle and can be sent to worker threads so they can call
// `ctx.request_repaint()` when work finishes.
// The actual results should live in app-owned state, usually via a channel or `Arc<Mutex<Option<T>>>`,
// and be read from `ui()` / `logic()` on the UI thread.
