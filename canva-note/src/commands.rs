use eframe::egui::{self, Pos2, TextureOptions};

use crate::{
    ImageNode, MarkdownNode, MyEguiApp, Node, NodeId, PersistentNodeData, egui_node_id,
};

#[derive(Clone)]
pub(crate) struct CreateNodeCommand {
    pub(crate) id: NodeId,
    pub(crate) data: PersistentNodeData,
}

#[derive(Clone)]
pub(crate) struct DeleteNodeCommand {
    pub(crate) id: NodeId,
    pub(crate) data: PersistentNodeData,
}

#[derive(Clone)]
pub(crate) struct MoveNodeCommand {
    pub(crate) id: NodeId,
    pub(crate) from: Pos2,
    pub(crate) to: Pos2,
}

#[derive(Clone)]
pub(crate) struct SetTextContentCommand {
    pub(crate) id: NodeId,
    pub(crate) old: String,
    pub(crate) new: String,
}

#[derive(Clone)]
pub(crate) enum Command {
    CreateNode(CreateNodeCommand),
    DeleteNode(DeleteNodeCommand),
    MoveNode(MoveNodeCommand),
    SetTextContent(SetTextContentCommand),
}

impl CreateNodeCommand {
    pub(crate) fn apply(&self, app: &mut MyEguiApp, ctx: &egui::Context) {
        match &self.data {
            PersistentNodeData::Text(text) => {
                app.document.data_mut().texts.insert(self.id, text.clone());
                app.nodes.insert(
                    self.id,
                    Node {
                        egui_id: egui_node_id(self.id),
                        kind: MarkdownNode {
                            cache: Default::default(),
                        }
                        .into(),
                    },
                );
            }
            PersistentNodeData::Image(image) => {
                let texture = ctx.load_texture(
                    format!("image-{}", self.id),
                    egui::ImageData::Color(image.data.clone()),
                    TextureOptions::default(),
                );
                app.document.data_mut().images.insert(self.id, image.clone());
                app.nodes.insert(
                    self.id,
                    Node {
                        egui_id: egui_node_id(self.id),
                        kind: ImageNode { texture }.into(),
                    },
                );
            }
        }
    }

    pub(crate) fn inverse(&self) -> Command {
        Command::DeleteNode(DeleteNodeCommand {
            id: self.id,
            data: self.data.clone(),
        })
    }
}

impl DeleteNodeCommand {
    pub(crate) fn apply(&self, app: &mut MyEguiApp, _ctx: &egui::Context) {
        match &self.data {
            PersistentNodeData::Text(_) => {
                app.document.data_mut().texts.remove(&self.id);
            }
            PersistentNodeData::Image(_) => {
                app.document.data_mut().images.remove(&self.id);
            }
        }
        app.nodes.remove(&self.id);
        if app.selected == Some(self.id) {
            app.selected = None;
        }
    }

    pub(crate) fn inverse(&self) -> Command {
        Command::CreateNode(CreateNodeCommand {
            id: self.id,
            data: self.data.clone(),
        })
    }
}

impl MoveNodeCommand {
    pub(crate) fn apply(&self, app: &mut MyEguiApp, _ctx: &egui::Context) {
        if let Some(text) = app.document.data_mut().texts.get_mut(&self.id) {
            text.pos = self.to;
        } else if let Some(image) = app.document.data_mut().images.get_mut(&self.id) {
            image.pos = self.to;
        }
    }

    pub(crate) fn inverse(&self) -> Command {
        Command::MoveNode(Self {
            id: self.id,
            from: self.to,
            to: self.from,
        })
    }
}

impl SetTextContentCommand {
    pub(crate) fn apply(&self, app: &mut MyEguiApp, _ctx: &egui::Context) {
        if let Some(text) = app.document.data_mut().texts.get_mut(&self.id) {
            text.content = self.new.clone();
        }
    }

    pub(crate) fn inverse(&self) -> Command {
        Command::SetTextContent(Self {
            id: self.id,
            old: self.new.clone(),
            new: self.old.clone(),
        })
    }
}

impl Command {
    pub(crate) fn apply(&self, app: &mut MyEguiApp, ctx: &egui::Context) {
        match self {
            Self::CreateNode(cmd) => cmd.apply(app, ctx),
            Self::DeleteNode(cmd) => cmd.apply(app, ctx),
            Self::MoveNode(cmd) => cmd.apply(app, ctx),
            Self::SetTextContent(cmd) => cmd.apply(app, ctx),
        }
    }

    pub(crate) fn inverse(&self) -> Self {
        match self {
            Self::CreateNode(cmd) => cmd.inverse(),
            Self::DeleteNode(cmd) => cmd.inverse(),
            Self::MoveNode(cmd) => cmd.inverse(),
            Self::SetTextContent(cmd) => cmd.inverse(),
        }
    }
}
