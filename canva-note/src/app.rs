use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
};

use crate::{
    commands::{Command, CreateNodeCommand, DeleteNodeCommand, SetTextContentCommand},
    document::{Document, ImageData, NodeData, PersistentData, TextData, TextNodeId},
    editor,
};
use derive_more::{From, IsVariant, TryInto};
use eframe::egui::{
    self, ColorImage, FontData, FontDefinitions, FontFamily, Id, Key, Pos2, TextureHandle,
    TextureOptions, Visuals,
};
use egui_commonmark::CommonMarkCache;
use log::error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shortcuts {
    paste: bool,
    save: bool,
    delete: bool,
    undo: bool,
    redo: bool,
}

const KEYBOARD_CAMERA_SPEED: f32 = 1200.0;
const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 4.0;
const KEY_ZOOM_FACTOR: f32 = 1.1;

pub fn run(file_path: PathBuf) {
    let persisted_data = load_persistent_data(&file_path);
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "Canva Note",
        native_options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, file_path, persisted_data)))),
    )
    .unwrap();
}

pub(crate) struct App {
    pub(crate) camera_pos: Pos2,
    pub(crate) zoom: f32,
    pub(crate) file_path: PathBuf,
    pub(crate) nodes: HashMap<NodeId, Node>,
    pub(crate) selected: Option<NodeId>,
    pub(crate) undo_stack: Vec<Command>,
    pub(crate) redo_stack: Vec<Command>,
    pub(crate) active_drag: Option<DragState>,
    pub(crate) document: Document,
    pub(crate) editor_rx: std::sync::mpsc::Receiver<editor::InterThreadMessage>,
    pub(crate) editor_tx: Sender<editor::InterThreadMessage>,
    pub(crate) shutdown: Arc<AtomicBool>,
}

pub(crate) type NodeId = u64;

pub(crate) struct Node {
    pub(crate) egui_id: Id,
    pub(crate) kind: NodeKind,
}

#[derive(TryInto, From, IsVariant)]
pub(crate) enum NodeKind {
    Markdown(MarkdownNode),
    Image(ImageNode),
}

pub(crate) struct MarkdownNode {
    pub(crate) cache: CommonMarkCache,
}

pub(crate) struct ImageNode {
    pub(crate) texture: TextureHandle,
}

pub(crate) struct DragState {
    pub(crate) node_id: NodeId,
    pub(crate) start_pos: Pos2,
}

pub(crate) fn egui_node_id(node_id: NodeId) -> Id {
    Id::new(("node", node_id))
}

impl App {
    pub(crate) fn new(
        cc: &eframe::CreationContext<'_>,
        file_path: PathBuf,
        data: PersistentData,
    ) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);

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

        cc.egui_ctx.set_visuals(Visuals::light());

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

        let (editor_tx, editor_rx) = std::sync::mpsc::channel();
        let nodes = create_runtime_nodes(&cc.egui_ctx, &data);
        let app = Self {
            camera_pos: Pos2 { x: 0., y: 0. },
            zoom: 1.0,
            nodes,
            selected: None,
            undo_stack: vec![],
            redo_stack: vec![],
            active_drag: None,
            document: Document::new(data),
            file_path,
            editor_tx,
            editor_rx,
            shutdown: Arc::new(AtomicBool::new(false)),
        };
        app.update_window_title(&cc.egui_ctx);
        app
    }

    pub(crate) fn create_new_node_and_open_editor(&mut self, pos: Pos2) {
        let node_id = self.document.alloc_node_id();
        self.execute_command_with_ctx(
            &egui::Context::default(),
            CreateNodeCommand {
                id: node_id,
                data: NodeData::Text(TextData {
                    content: String::new(),
                    width: 650,
                    pos,
                }),
            }
            .into(),
        );
        editor::spawn(
            TextNodeId(node_id),
            "",
            self.editor_tx.clone(),
            Arc::clone(&self.shutdown),
        );
    }

    pub(crate) fn execute_command_with_ctx(&mut self, ctx: &egui::Context, command: Command) {
        command.apply(self, ctx);
        self.undo_stack.push(command);
        self.redo_stack.clear();
    }

    pub(crate) fn record_applied_command(&mut self, command: Command) {
        self.undo_stack.push(command);
        self.redo_stack.clear();
    }

    pub(crate) fn undo(&mut self, ctx: &egui::Context) {
        let Some(command) = self.undo_stack.pop() else {
            return;
        };
        command.inverse().apply(self, ctx);
        self.redo_stack.push(command);
    }

    pub(crate) fn redo(&mut self, ctx: &egui::Context) {
        let Some(command) = self.redo_stack.pop() else {
            return;
        };
        command.apply(self, ctx);
        self.undo_stack.push(command);
    }

    pub(crate) fn update_window_title(&self, ctx: &egui::Context) {
        let file_name = self
            .file_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Canva Note");
        let dirty = if self.document.is_dirty() { " *" } else { "" };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "Canva Note - {file_name}{dirty}"
        )));
    }

    pub(crate) fn world_to_screen_pos(&self, world: Pos2) -> Pos2 {
        ((world - self.camera_pos) * self.zoom).to_pos2()
    }

    pub(crate) fn screen_to_world_pos(&self, screen: Pos2) -> Pos2 {
        self.camera_pos + screen.to_vec2() / self.zoom
    }

    pub(crate) fn screen_to_world_delta(&self, screen_delta: egui::Vec2) -> egui::Vec2 {
        screen_delta / self.zoom
    }

    pub(crate) fn viewport_world_size(&self, viewport_screen_size: egui::Vec2) -> egui::Vec2 {
        viewport_screen_size / self.zoom
    }

    fn zoom_around(&mut self, factor: f32, ptr_pos_screen: Pos2) {
        let old_zoom = self.zoom;
        let new_zoom = (old_zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        if (new_zoom - old_zoom).abs() <= f32::EPSILON {
            return;
        }

        let ptr_pos_world = self.screen_to_world_pos(ptr_pos_screen);
        self.zoom = new_zoom;
        self.camera_pos = ptr_pos_world - ptr_pos_screen.to_vec2() / self.zoom;
    }

    fn delete_selected_command(&self) -> Option<Command> {
        let selected = self.selected?;
        let data = self.document.node(selected)?.clone();
        Some(DeleteNodeCommand { id: selected, data }.into())
    }

    fn image_paste_command(
        &mut self,
        image: ColorImage,
        ptr_pos: Option<Pos2>,
        viewport: egui::Vec2,
    ) -> Command {
        let node_id = self.document.alloc_node_id();
        CreateNodeCommand {
            id: node_id,
            data: ImageData {
                data: Arc::new(image.clone()),
                pos: image_spawn_pos(self.camera_pos, ptr_pos, viewport, image.size),
            }
            .into(),
        }
        .into()
    }

    fn text_paste_command(&mut self, text: String, ptr_pos: Option<Pos2>) -> Command {
        let node_id = self.document.alloc_node_id();
        CreateNodeCommand {
            id: node_id,
            data: TextData {
                content: text,
                width: 650,
                pos: ptr_pos.unwrap_or(self.camera_pos),
            }
            .into(),
        }
        .into()
    }
}

fn load_persistent_data(file_path: &PathBuf) -> PersistentData {
    if !file_path.exists() {
        return PersistentData::default();
    }

    let content = fs::read(file_path).unwrap();
    postcard::from_bytes(&content).unwrap()
}

fn save_persistent_data(file_path: &PathBuf, data: &PersistentData) {
    let serialized = postcard::to_allocvec(data).unwrap();
    fs::write(file_path, serialized).unwrap();
}

fn create_runtime_nodes(ctx: &egui::Context, data: &PersistentData) -> HashMap<NodeId, Node> {
    data.nodes()
        .iter()
        .map(|(&node_id, node)| {
            let kind = match node {
                NodeData::Text(_) => MarkdownNode {
                    cache: CommonMarkCache::default(),
                }
                .into(),
                NodeData::Image(image) => ImageNode {
                    texture: ctx.load_texture(
                        format!("loaded-image-{node_id}"),
                        egui::ImageData::Color(image.data.clone()),
                        TextureOptions::default(),
                    ),
                }
                .into(),
            };
            (
                node_id,
                Node {
                    egui_id: egui_node_id(node_id),
                    kind,
                },
            )
        })
        .collect()
}

fn image_spawn_pos(
    camera_pos: Pos2,
    ptr_pos: Option<Pos2>,
    viewport_world: egui::Vec2,
    image_size: [usize; 2],
) -> Pos2 {
    ptr_pos.unwrap_or_else(|| {
        camera_pos + (viewport_world - egui::vec2(image_size[0] as f32, image_size[1] as f32)) * 0.5
    })
}

fn keyboard_camera_delta(input: &egui::InputState) -> egui::Vec2 {
    let mut delta = egui::Vec2::ZERO;
    let step = KEYBOARD_CAMERA_SPEED * input.stable_dt;

    if input.key_down(Key::H) {
        delta.x += step;
    }
    if input.key_down(Key::L) {
        delta.x -= step;
    }
    if input.key_down(Key::K) {
        delta.y += step;
    }
    if input.key_down(Key::J) {
        delta.y -= step;
    }

    delta
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(msg) = self.editor_rx.try_recv() {
            let old = self.document.text(msg.node_id).content.clone();
            if old != msg.content {
                self.execute_command_with_ctx(
                    ctx,
                    SetTextContentCommand {
                        id: msg.node_id,
                        old,
                        new: msg.content,
                    }
                    .into(),
                );
            }
        }

        let (camera_delta, zoom_factor_from_scroll, zoom_focus, zoom_key_factor) = ctx.input(|i| {
            let mut delta = keyboard_camera_delta(i);
            let zoom_factor_from_scroll = if i.modifiers.ctrl {
                i.zoom_delta()
            } else {
                1.0
            };
            let zoom_key_factor = match (i.key_pressed(Key::Plus), i.key_pressed(Key::Minus)) {
                (true, false) => KEY_ZOOM_FACTOR,
                (false, true) => 1.0 / KEY_ZOOM_FACTOR,
                _ => 1.0,
            };
            if !i.modifiers.ctrl {
                delta.y -= i.smooth_scroll_delta.y;
            }

            (
                delta,
                zoom_factor_from_scroll,
                i.pointer.interact_pos(),
                zoom_key_factor,
            )
        });
        self.camera_pos += self.screen_to_world_delta(camera_delta);
        if zoom_factor_from_scroll != 1.0 {
            let focus = zoom_focus.unwrap_or_else(|| ctx.content_rect().center());
            self.zoom_around(zoom_factor_from_scroll, focus);
        }
        if zoom_key_factor != 1.0 {
            self.zoom_around(zoom_key_factor, ctx.content_rect().center());
        }

        let shortcuts = ctx.input(|i| {
            let modifiers = i.modifiers;
            let u_pressed = i.key_pressed(Key::U);
            Shortcuts {
                paste: i.key_pressed(Key::I) && modifiers.ctrl,
                save: i.key_pressed(Key::S) && modifiers.command,
                delete: i.key_pressed(Key::D)
                    || i.key_pressed(Key::X)
                    || i.key_pressed(Key::Delete),
                undo: (u_pressed && !modifiers.command && !modifiers.shift)
                    || (i.key_pressed(Key::Z) && modifiers.command),
                redo: (u_pressed && modifiers.shift)
                    || (i.key_pressed(Key::Y) && modifiers.command)
                    || (i.key_pressed(Key::R) && modifiers.command),
            }
        });

        if shortcuts.save {
            save_persistent_data(&self.file_path, self.document.data());
            self.document.mark_clean();
        }

        if shortcuts.delete
            && let Some(command) = self.delete_selected_command()
        {
            self.execute_command_with_ctx(ctx, command);
        }

        if shortcuts.undo {
            self.undo(ctx);
        }

        if shortcuts.redo {
            self.redo(ctx);
        }

        if shortcuts.paste {
            let ptr_pos = ctx
                .input(|i| i.pointer.interact_pos())
                .map(|pos| pos + self.camera_pos.to_vec2());
            let mut clipboard = match arboard::Clipboard::new() {
                Ok(clipboard) => clipboard,
                Err(e) => {
                    error!("Couldn't access clipboard: {e:?}");
                    return;
                }
            };

            if let Ok(image) = clipboard.get_image() {
                let command = self.image_paste_command(
                    ColorImage::from_rgba_unmultiplied(
                        [image.width, image.height],
                        image.bytes.as_ref(),
                    ),
                    ptr_pos,
                    self.viewport_world_size(ctx.content_rect().size()),
                );
                self.execute_command_with_ctx(ctx, command);
            } else if let Ok(text) = clipboard.get_text() {
                let command = self.text_paste_command(text, ptr_pos);
                self.execute_command_with_ctx(ctx, command);
            }
        }

        self.update_window_title(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.canvas_ui(ui);
    }

    fn on_exit(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        commands::{CreateNodeCommand, MoveNodeCommand, SetTextContentCommand},
        document::TextNodeId,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_app() -> (App, egui::Context) {
        let ctx = egui::Context::default();
        let (editor_tx, editor_rx) = std::sync::mpsc::channel();
        let app = App {
            camera_pos: Pos2::ZERO,
            zoom: 1.0,
            file_path: PathBuf::from("test.canva"),
            nodes: HashMap::new(),
            selected: None,
            undo_stack: vec![],
            redo_stack: vec![],
            active_drag: None,
            document: Document::new(PersistentData::default()),
            editor_rx,
            editor_tx,
            shutdown: Arc::new(AtomicBool::new(false)),
        };
        (app, ctx)
    }

    fn unique_test_path(suffix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "canva-note-test-{}-{nanos}.{suffix}",
            std::process::id()
        ))
    }

    #[test]
    fn commands_cover_create_edit_move_delete_for_text_and_image_nodes() {
        let (mut app, ctx) = test_app();
        let text_id = app.document.alloc_node_id();
        let image_id = app.document.alloc_node_id();

        CreateNodeCommand {
            id: text_id,
            data: TextData {
                content: "hello".into(),
                width: 650,
                pos: Pos2::new(10.0, 20.0),
            }
            .into(),
        }
        .apply(&mut app, &ctx);
        assert!(app.nodes.contains_key(&text_id));
        assert_eq!(app.document.text(TextNodeId(text_id)).content, "hello");

        SetTextContentCommand {
            id: TextNodeId(text_id),
            old: "hello".into(),
            new: "updated".into(),
        }
        .apply(&mut app, &ctx);
        assert_eq!(app.document.text(TextNodeId(text_id)).content, "updated");

        MoveNodeCommand {
            id: text_id,
            from: Pos2::new(10.0, 20.0),
            to: Pos2::new(30.0, 40.0),
        }
        .apply(&mut app, &ctx);
        assert_eq!(
            app.document.node(text_id).unwrap().pos(),
            Pos2::new(30.0, 40.0)
        );

        CreateNodeCommand {
            id: image_id,
            data: ImageData {
                data: Arc::new(ColorImage::from_rgba_unmultiplied(
                    [1, 1],
                    &[255, 0, 0, 255],
                )),
                pos: Pos2::new(50.0, 60.0),
            }
            .into(),
        }
        .apply(&mut app, &ctx);
        assert!(app.nodes.contains_key(&image_id));
        assert!(matches!(
            app.document.node(image_id),
            Some(NodeData::Image(_))
        ));

        DeleteNodeCommand {
            id: text_id,
            data: app.document.node(text_id).unwrap().clone(),
        }
        .apply(&mut app, &ctx);
        assert!(!app.nodes.contains_key(&text_id));
        assert!(app.document.node(text_id).is_none());
    }

    #[test]
    fn undo_and_redo_restore_node_lifecycle_and_text_edits() {
        let (mut app, ctx) = test_app();
        let node_id = app.document.alloc_node_id();

        app.execute_command_with_ctx(
            &ctx,
            CreateNodeCommand {
                id: node_id,
                data: TextData {
                    content: "first".into(),
                    width: 650,
                    pos: Pos2::new(10.0, 20.0),
                }
                .into(),
            }
            .into(),
        );
        app.execute_command_with_ctx(
            &ctx,
            SetTextContentCommand {
                id: TextNodeId(node_id),
                old: "first".into(),
                new: "second".into(),
            }
            .into(),
        );

        assert_eq!(app.document.text(TextNodeId(node_id)).content, "second");

        app.undo(&ctx);
        assert_eq!(app.document.text(TextNodeId(node_id)).content, "first");

        app.undo(&ctx);
        assert!(app.document.node(node_id).is_none());

        app.redo(&ctx);
        assert_eq!(app.document.text(TextNodeId(node_id)).content, "first");

        app.redo(&ctx);
        assert_eq!(app.document.text(TextNodeId(node_id)).content, "second");
    }

    #[test]
    fn save_and_load_roundtrip_persistent_data() {
        let path = unique_test_path("bin");
        let mut document = Document::new(PersistentData::default());
        let text_id = document.alloc_node_id();
        let image_id = document.alloc_node_id();
        document.insert_node(
            text_id,
            TextData {
                content: "saved".into(),
                width: 123,
                pos: Pos2::new(1.0, 2.0),
            }
            .into(),
        );
        document.insert_node(
            image_id,
            ImageData {
                data: Arc::new(ColorImage::from_rgba_unmultiplied(
                    [1, 1],
                    &[0, 255, 0, 255],
                )),
                pos: Pos2::new(3.0, 4.0),
            }
            .into(),
        );

        save_persistent_data(&path, document.data());
        let loaded = load_persistent_data(&path);

        assert_eq!(loaded.nodes().len(), 2);
        assert!(
            matches!(loaded.nodes().get(&text_id), Some(NodeData::Text(text)) if text.content == "saved" && text.width == 123 && text.pos == Pos2::new(1.0, 2.0))
        );
        assert!(
            matches!(loaded.nodes().get(&image_id), Some(NodeData::Image(image)) if image.pos == Pos2::new(3.0, 4.0))
        );

        _ = fs::remove_file(path);
    }

    #[test]
    fn loading_missing_file_returns_default_data() {
        let path = unique_test_path("missing");
        let loaded = load_persistent_data(&path);
        assert!(loaded.nodes().is_empty());
    }

    #[test]
    fn loading_invalid_file_panics() {
        let path = unique_test_path("bad");
        fs::write(&path, b"definitely not postcard").unwrap();

        let result = std::panic::catch_unwind(|| {
            let _ = load_persistent_data(&path);
        });

        assert!(result.is_err());
        _ = fs::remove_file(path);
    }

    #[test]
    fn hjkl_camera_keys_map_to_expected_movement() {
        let mut input = egui::RawInput {
            time: Some(1.0),
            predicted_dt: 0.5,
            ..Default::default()
        };
        input.events = vec![
            egui::Event::Key {
                key: Key::H,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            },
            egui::Event::Key {
                key: Key::J,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            },
        ];
        let ctx = egui::Context::default();
        ctx.begin_pass(input);
        let delta = ctx.input(keyboard_camera_delta);
        let _ = ctx.end_pass();

        assert_eq!(
            delta,
            egui::vec2(KEYBOARD_CAMERA_SPEED * 0.5, -KEYBOARD_CAMERA_SPEED * 0.5)
        );
    }

    #[test]
    fn world_screen_transforms_roundtrip() {
        let (mut app, _) = test_app();
        app.camera_pos = Pos2::new(100.0, 50.0);
        app.zoom = 2.0;

        let world = Pos2::new(130.0, 90.0);
        let screen = app.world_to_screen_pos(world);
        assert_eq!(screen, Pos2::new(60.0, 80.0));
        assert_eq!(app.screen_to_world_pos(screen), world);
        assert_eq!(
            app.screen_to_world_delta(egui::vec2(20.0, 10.0)),
            egui::vec2(10.0, 5.0)
        );
    }

    #[test]
    fn zoom_around_pointer_keeps_focus_world_position_stable() {
        let (mut app, _) = test_app();
        app.camera_pos = Pos2::new(100.0, 50.0);
        app.zoom = 1.0;
        let focus = Pos2::new(200.0, 150.0);
        let before = app.screen_to_world_pos(focus);

        app.zoom_around(2.0, focus);

        assert_eq!(app.zoom, 2.0);
        assert_eq!(app.screen_to_world_pos(focus), before);
    }

    #[test]
    fn delete_selected_command_returns_delete_only_for_existing_selection() {
        let (mut app, ctx) = test_app();
        let node_id = app.document.alloc_node_id();
        CreateNodeCommand {
            id: node_id,
            data: TextData {
                content: "hello".into(),
                width: 650,
                pos: Pos2::new(10.0, 20.0),
            }
            .into(),
        }
        .apply(&mut app, &ctx);

        assert!(app.delete_selected_command().is_none());

        app.selected = Some(node_id);
        let command = app.delete_selected_command();
        assert!(matches!(command, Some(Command::DeleteNode(_))));
    }

    #[test]
    fn image_paste_uses_pointer_or_centers_in_camera_rect() {
        let (mut app, _) = test_app();
        app.camera_pos = Pos2::new(100.0, 200.0);

        let direct = image_spawn_pos(
            app.camera_pos,
            Some(Pos2::new(320.0, 240.0)),
            egui::vec2(800.0, 600.0),
            [100, 50],
        );
        assert_eq!(direct, Pos2::new(320.0, 240.0));

        let centered = image_spawn_pos(app.camera_pos, None, egui::vec2(800.0, 600.0), [100, 50]);
        assert_eq!(centered, Pos2::new(450.0, 475.0));

        let command = app.image_paste_command(
            ColorImage::from_rgba_unmultiplied([100, 50], &vec![255; 100 * 50 * 4]),
            None,
            egui::vec2(800.0, 600.0),
        );
        assert!(matches!(
            command,
            Command::CreateNode(CreateNodeCommand {
                data: NodeData::Image(ImageData { pos, .. }),
                ..
            }) if pos == Pos2::new(450.0, 475.0)
        ));
    }

    #[test]
    fn text_paste_uses_pointer_or_camera_position() {
        let (mut app, _) = test_app();
        app.camera_pos = Pos2::new(100.0, 200.0);

        let direct = app.text_paste_command("hello".into(), Some(Pos2::new(10.0, 20.0)));
        assert!(matches!(
            direct,
            Command::CreateNode(CreateNodeCommand {
                data: NodeData::Text(TextData { pos, content, .. }),
                ..
            }) if pos == Pos2::new(10.0, 20.0) && content == "hello"
        ));

        let fallback = app.text_paste_command("world".into(), None);
        assert!(matches!(
            fallback,
            Command::CreateNode(CreateNodeCommand {
                data: NodeData::Text(TextData { pos, content, .. }),
                ..
            }) if pos == Pos2::new(100.0, 200.0) && content == "world"
        ));
    }
}
