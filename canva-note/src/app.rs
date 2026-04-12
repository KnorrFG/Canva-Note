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
use derive_more::{From, TryInto};
use eframe::egui::{
    self, ColorImage, FontData, FontDefinitions, FontFamily, Id, Key, Pos2, TextureHandle,
    TextureOptions, Visuals,
};
use egui_commonmark::CommonMarkCache;
use log::error;

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

#[derive(TryInto, From)]
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
                && let Some(data) = self.document.node(selected).cloned()
            {
                self.execute_command_with_ctx(ctx, DeleteNodeCommand { id: selected, data }.into());
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
                let node_id = self.document.alloc_node_id();
                self.execute_command_with_ctx(
                    ctx,
                    Command::CreateNode(CreateNodeCommand {
                        id: node_id,
                        data: ImageData {
                            data: Arc::new(ColorImage::from_rgba_unmultiplied(
                                [image.width, image.height],
                                image.bytes.as_ref(),
                            )),
                            pos,
                        }
                        .into(),
                    }),
                );
            } else if let Ok(text) = clipboard.get_text() {
                let node_id = self.document.alloc_node_id();
                self.execute_command_with_ctx(
                    ctx,
                    Command::CreateNode(CreateNodeCommand {
                        id: node_id,
                        data: TextData {
                            content: text,
                            width: 650,
                            pos: ptr_pos.unwrap_or(self.camera_pos),
                        }
                        .into(),
                    }),
                );
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
