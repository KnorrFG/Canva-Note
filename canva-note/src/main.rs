use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread,
    time::Duration,
};

mod commands;

use clap::Parser;
use commands::{
    Command, CreateNodeCommand, DeleteNodeCommand, MoveNodeCommand, SetTextContentCommand,
};
use derive_more::{From, TryInto};
use eframe::egui::{
    self, Area, Color32, ColorImage, CornerRadius, FontData, FontDefinitions, FontFamily, Id,
    Image, ImageSource, Key, PointerButton, Pos2, Sense, Stroke, StrokeKind, TextureHandle,
    TextureOptions, Visuals, load::SizedTexture,
};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use log::error;
use serde::{Deserialize, Serialize};

fn main() {
    let args = Cli::parse();
    pretty_env_logger::init();
    let persisted_data = load_persistent_data(&args.file);
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "Canva Note",
        native_options,
        Box::new(move |cc| Ok(Box::new(MyEguiApp::new(cc, args.file, persisted_data)))),
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
    nodes: HashMap<NodeId, Node>,
    selected: Option<NodeId>,
    undo_stack: Vec<Command>,
    redo_stack: Vec<Command>,
    active_drag: Option<DragState>,
    document: Document,
    editor_rx: std::sync::mpsc::Receiver<EditorThreadMessage>,
    editor_tx: Sender<EditorThreadMessage>,
    shutdown: Arc<AtomicBool>,
}

type NodeId = u64;

struct Node {
    egui_id: Id,
    kind: NodeKind,
}

#[derive(TryInto, From)]
enum NodeKind {
    Markdown(MarkdownNode),
    Image(ImageNode),
}

struct MarkdownNode {
    cache: CommonMarkCache,
}

struct ImageNode {
    texture: TextureHandle,
}

#[derive(Default, Serialize, Deserialize)]
struct PersistentData {
    next_node_id: NodeId,
    texts: HashMap<NodeId, TextData>,
    images: HashMap<NodeId, ImageData>,
}

#[derive(Clone, Serialize, Deserialize)]
struct TextData {
    content: String,
    width: usize,
    pos: Pos2,
}

#[derive(Clone, Serialize, Deserialize)]
struct ImageData {
    data: Arc<egui::ColorImage>,
    pos: Pos2,
}

#[derive(Clone)]
enum PersistentNodeData {
    Text(TextData),
    Image(ImageData),
}

struct DragState {
    node_id: NodeId,
    start_pos: Pos2,
}

struct Document {
    dirty: bool,
    data: PersistentData,
}

impl Document {
    fn new(data: PersistentData) -> Self {
        Self { dirty: false, data }
    }

    fn data(&self) -> &PersistentData {
        &self.data
    }

    fn data_mut(&mut self) -> &mut PersistentData {
        self.dirty = true;
        &mut self.data
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn mark_clean(&mut self) {
        self.dirty = false;
    }
}

pub(crate) fn egui_node_id(node_id: NodeId) -> Id {
    Id::new(("node", node_id))
}

impl MyEguiApp {
    fn new(cc: &eframe::CreationContext<'_>, file_path: PathBuf, data: PersistentData) -> Self {
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

        let (editor_tx, editor_rx) = std::sync::mpsc::channel();
        let nodes = create_runtime_nodes(&cc.egui_ctx, &data);
        let app = Self {
            camera_pos: Pos2 { x: 0., y: 0. },
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

    fn alloc_node_id(&mut self) -> NodeId {
        let data = self.document.data_mut();
        let node_id = data.next_node_id;
        data.next_node_id += 1;
        node_id
    }

    fn create_new_node_and_open_editor(&mut self, pos: Pos2) {
        let node_id = self.alloc_node_id();
        self.execute_command_with_ctx(
            &egui::Context::default(),
            Command::CreateNode(CreateNodeCommand {
                id: node_id,
                data: PersistentNodeData::Text(TextData {
                    content: String::new(),
                    width: 650,
                    pos,
                }),
            }),
        );
        spawn_editor(
            node_id,
            "",
            self.editor_tx.clone(),
            Arc::clone(&self.shutdown),
        );
    }

    fn execute_command_with_ctx(&mut self, ctx: &egui::Context, command: Command) {
        command.apply(self, ctx);
        self.undo_stack.push(command);
        self.redo_stack.clear();
    }

    fn record_applied_command(&mut self, command: Command) {
        self.undo_stack.push(command);
        self.redo_stack.clear();
    }

    fn undo(&mut self, ctx: &egui::Context) {
        let Some(command) = self.undo_stack.pop() else {
            return;
        };
        command.inverse().apply(self, ctx);
        self.redo_stack.push(command);
    }

    fn redo(&mut self, ctx: &egui::Context) {
        let Some(command) = self.redo_stack.pop() else {
            return;
        };
        command.apply(self, ctx);
        self.undo_stack.push(command);
    }

    fn node_snapshot(&self, node_id: NodeId) -> Option<PersistentNodeData> {
        self.document
            .data()
            .texts
            .get(&node_id)
            .cloned()
            .map(PersistentNodeData::Text)
            .or_else(|| {
                self.document
                    .data()
                    .images
                    .get(&node_id)
                    .cloned()
                    .map(PersistentNodeData::Image)
            })
    }

    fn update_window_title(&self, ctx: &egui::Context) {
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
    let text_nodes = data.texts.keys().copied().map(|node_id| {
        (
            node_id,
            Node {
                egui_id: egui_node_id(node_id),
                kind: MarkdownNode {
                    cache: CommonMarkCache::default(),
                }
                .into(),
            },
        )
    });

    let image_nodes = data.images.iter().map(|(&node_id, image)| {
        (
            node_id,
            Node {
                egui_id: egui_node_id(node_id),
                kind: ImageNode {
                    texture: ctx.load_texture(
                        format!("loaded-image-{node_id}"),
                        egui::ImageData::Color(image.data.clone()),
                        TextureOptions::default(),
                    ),
                }
                .into(),
            },
        )
    });

    text_nodes.chain(image_nodes).collect()
}

fn spawn_editor(
    id: NodeId,
    content: &str,
    tx: Sender<EditorThreadMessage>,
    shutdown: Arc<AtomicBool>,
) {
    let content = content.to_string();
    _ = thread::spawn(move || editor_thread_fn(id, content, tx, shutdown));
}

fn editor_thread_fn(
    id: NodeId,
    mut content: String,
    tx: Sender<EditorThreadMessage>,
    shutdown: Arc<AtomicBool>,
) {
    let fname = std::env::temp_dir().join(format!("canva-note-{id}.md"));
    fs::write(&fname, &content).unwrap();
    let mut modified = fs::metadata(&fname).unwrap().modified().unwrap();

    let mut proc = std::process::Command::new("wezterm")
        .args(["start", "--always-new-process", "hx"])
        .arg(&fname)
        .spawn()
        .unwrap();

    let mut results = vec![];
    let res = (|| {
        loop {
            let current_modified = fs::metadata(&fname)?.modified()?;
            if current_modified != modified {
                modified = current_modified;
                let current_content = fs::read_to_string(&fname)?;
                if current_content != content {
                    content = current_content.clone();
                    tx.send(EditorThreadMessage {
                        node_id: id,
                        content: current_content,
                    })?;
                }
            }

            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            if proc.try_wait()?.is_some() {
                break;
            }

            thread::sleep(Duration::from_millis(250));
        }
        anyhow::Ok(())
    })();
    results.push(res);

    results.push(fs::remove_file(&fname).map_err(anyhow::Error::from));
    results.push(proc.kill().map_err(anyhow::Error::from));

    for res in results {
        if let Err(e) = res {
            error!("{e:?}");
        }
    }
}

struct EditorThreadMessage {
    node_id: NodeId,
    content: String,
}

impl eframe::App for MyEguiApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(msg) = self.editor_rx.try_recv() {
            if let Some(text) = self.document.data().texts.get(&msg.node_id) {
                let old = text.content.clone();
                if old != msg.content {
                    self.execute_command_with_ctx(
                        ctx,
                        Command::SetTextContent(SetTextContentCommand {
                            id: msg.node_id,
                            old,
                            new: msg.content,
                        }),
                    );
                }
            }
        }

        let (
            _copy_pressed,
            paste_pressed,
            save_pressed,
            delete_pressed,
            undo_pressed,
            redo_pressed,
        ) = ctx.input(|i| {
            let copy = i.key_pressed(Key::C) && i.modifiers.command;
            let paste = i.key_pressed(Key::I) && i.modifiers.ctrl;
            let save = i.key_pressed(Key::S) && i.modifiers.command;
            let delete =
                i.key_pressed(Key::D) || i.key_pressed(Key::X) || i.key_pressed(Key::Delete);
            let undo = (i.key_pressed(Key::U) && !i.modifiers.command && !i.modifiers.shift)
                || (i.key_pressed(Key::Z) && i.modifiers.command);
            let redo = (i.key_pressed(Key::U) && i.modifiers.shift)
                || (i.key_pressed(Key::Y) && i.modifiers.command)
                || (i.key_pressed(Key::R) && i.modifiers.command);
            (copy, paste, save, delete, undo, redo)
        });

        if save_pressed {
            save_persistent_data(&self.file_path, self.document.data());
            self.document.mark_clean();
        }

        if delete_pressed {
            if let Some(selected) = self.selected
                && let Some(data) = self.node_snapshot(selected)
            {
                self.execute_command_with_ctx(
                    ctx,
                    Command::DeleteNode(DeleteNodeCommand { id: selected, data }),
                );
            }
        }

        if undo_pressed {
            self.undo(ctx);
        }

        if redo_pressed {
            self.redo(ctx);
        }

        if paste_pressed {
            let ptr_pos = ctx
                .input(|i| i.pointer.interact_pos())
                .map(|pos| pos + self.camera_pos.to_vec2());
            let mut clipboard = match arboard::Clipboard::new() {
                Ok(clipboard) => clipboard,
                Err(_) => {
                    error!("Couldn't access clipboard");
                    return;
                }
            };

            if let Ok(image) = clipboard.get_image() {
                let pos = ptr_pos.unwrap_or_else(|| {
                    let viewport = ctx.content_rect().size();
                    let image_size = egui::vec2(image.width as f32, image.height as f32);
                    self.camera_pos + (viewport - image_size) * 0.5
                });
                let node_id = self.alloc_node_id();
                self.execute_command_with_ctx(
                    ctx,
                    Command::CreateNode(CreateNodeCommand {
                        id: node_id,
                        data: PersistentNodeData::Image(ImageData {
                            data: Arc::new(ColorImage::from_rgba_unmultiplied(
                                [image.width, image.height],
                                image.bytes.as_ref(),
                            )),
                            pos,
                        }),
                    }),
                );
            } else if let Ok(text) = clipboard.get_text() {
                let node_id = self.alloc_node_id();
                self.execute_command_with_ctx(
                    ctx,
                    Command::CreateNode(CreateNodeCommand {
                        id: node_id,
                        data: PersistentNodeData::Text(TextData {
                            content: text,
                            width: 650,
                            pos: ptr_pos.unwrap_or(self.camera_pos),
                        }),
                    }),
                );
            }
        }

        self.update_window_title(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let mut global_drag_active = false;
            let (secondary_down, ctrl_down, ptr_delta, ptr_pos, primary_double_click_occured) = ui
                .input(|i| {
                    (
                        i.pointer.button_down(PointerButton::Secondary),
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

            if !global_drag_active && resp.dragged_by(PointerButton::Secondary) {
                ui.ctx().input(|i| {
                    self.camera_pos -= i.pointer.delta();
                });
            }

            for (&node_id, node) in &mut self.nodes {
                let resp = match &mut node.kind {
                    NodeKind::Markdown(md_node) => {
                        let text = &self.document.data().texts[&node_id];
                        let area = Area::new(node.egui_id)
                            .fixed_pos(text.pos - self.camera_pos.to_vec2())
                            .sense(Sense::click_and_drag())
                            .constrain(false);

                        if secondary_down {
                            ui.style_mut().interaction.selectable_labels = false;
                        }
                        area.show(ui.ctx(), |ui| {
                            CommonMarkViewer::new()
                                .default_width(Some(text.width))
                                .show(ui, &mut md_node.cache, &text.content);
                        })
                    }
                    NodeKind::Image(image_node) => {
                        let image = &self.document.data().images[&node_id];
                        let area = Area::new(node.egui_id)
                            .fixed_pos(image.pos - self.camera_pos.to_vec2())
                            .sense(Sense::click_and_drag())
                            .constrain(false);

                        if secondary_down {
                            ui.style_mut().interaction.selectable_labels = false;
                        }
                        area.show(ui.ctx(), |ui| {
                            ui.add(
                                Image::new(ImageSource::Texture(SizedTexture::from_handle(
                                    &image_node.texture,
                                )))
                                .fit_to_original_size(1.0),
                            );
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

                if let Some(ptr_pos) = ptr_pos
                    && !global_drag_active
                    && secondary_down
                    && resp.response.rect.contains(ptr_pos)
                {
                    if self.active_drag.is_none()
                        && let Some(start_pos) = self
                            .document
                            .data()
                            .texts
                            .get(&node_id)
                            .map(|text| text.pos)
                            .or_else(|| {
                                self.document
                                    .data()
                                    .images
                                    .get(&node_id)
                                    .map(|image| image.pos)
                            })
                    {
                        self.active_drag = Some(DragState { node_id, start_pos });
                    }
                    match &node.kind {
                        NodeKind::Markdown(_) => {
                            self.document
                                .data_mut()
                                .texts
                                .get_mut(&node_id)
                                .unwrap()
                                .pos += ptr_delta;
                        }
                        NodeKind::Image(_) => {
                            self.document
                                .data_mut()
                                .images
                                .get_mut(&node_id)
                                .unwrap()
                                .pos += ptr_delta;
                        }
                    }
                }

                if let Some(ptr_pos) = ptr_pos
                    && !global_drag_active
                    && primary_double_click_occured
                    && resp.response.rect.contains(ptr_pos)
                    && let NodeKind::Markdown(_) = &node.kind
                {
                    spawn_editor(
                        node_id,
                        &self.document.data().texts[&node_id].content,
                        self.editor_tx.clone(),
                        Arc::clone(&self.shutdown),
                    );
                }
            }

            if !secondary_down
                && let Some(drag) = self.active_drag.take()
                && let Some(end_pos) = self
                    .document
                    .data()
                    .texts
                    .get(&drag.node_id)
                    .map(|text| text.pos)
                    .or_else(|| {
                        self.document
                            .data()
                            .images
                            .get(&drag.node_id)
                            .map(|image| image.pos)
                    })
                && end_pos != drag.start_pos
            {
                self.record_applied_command(Command::MoveNode(MoveNodeCommand {
                    id: drag.node_id,
                    from: drag.start_pos,
                    to: end_pos,
                }));
            }
        });
    }

    fn on_exit(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}
