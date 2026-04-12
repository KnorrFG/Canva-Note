use derive_more::{From, TryInto};
use eframe::egui::{self, Pos2, TextureOptions};

use crate::app::{App, ImageNode, MarkdownNode, Node, NodeId, egui_node_id};
use crate::document::{NodeData, TextNodeId};

#[derive(Clone)]
pub(crate) struct CreateNodeCommand {
    pub(crate) id: NodeId,
    pub(crate) data: NodeData,
}

#[derive(Clone)]
pub(crate) struct DeleteNodeCommand {
    pub(crate) id: NodeId,
    pub(crate) data: NodeData,
}

#[derive(Clone)]
pub(crate) struct MoveNodeCommand {
    pub(crate) id: NodeId,
    pub(crate) from: Pos2,
    pub(crate) to: Pos2,
}

#[derive(Clone)]
pub(crate) struct SetTextContentCommand {
    pub(crate) id: TextNodeId,
    pub(crate) old: String,
    pub(crate) new: String,
}

#[derive(Clone, TryInto, From)]
pub(crate) enum Command {
    CreateNode(CreateNodeCommand),
    DeleteNode(DeleteNodeCommand),
    MoveNode(MoveNodeCommand),
    SetTextContent(SetTextContentCommand),
}

impl CreateNodeCommand {
    pub(crate) fn apply(&self, app: &mut App, ctx: &egui::Context) {
        match &self.data {
            NodeData::Text(text) => {
                app.document
                    .insert_node(self.id, NodeData::Text(text.clone()));
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
            NodeData::Image(image) => {
                let texture = ctx.load_texture(
                    format!("image-{}", self.id),
                    egui::ImageData::Color(image.data.clone()),
                    TextureOptions::default(),
                );
                app.document
                    .insert_node(self.id, NodeData::Image(image.clone()));
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
        DeleteNodeCommand {
            id: self.id,
            data: self.data.clone(),
        }
        .into()
    }
}

impl DeleteNodeCommand {
    pub(crate) fn apply(&self, app: &mut App, _ctx: &egui::Context) {
        match &self.data {
            NodeData::Text(_) | NodeData::Image(_) => {
                app.document.remove_node(self.id);
            }
        }
        app.nodes.remove(&self.id);
        if app.selected == Some(self.id) {
            app.selected = None;
        }
    }

    pub(crate) fn inverse(&self) -> Command {
        CreateNodeCommand {
            id: self.id,
            data: self.data.clone(),
        }
        .into()
    }
}

impl MoveNodeCommand {
    pub(crate) fn apply(&self, app: &mut App, _ctx: &egui::Context) {
        if let Some(node) = app.document.node_mut(self.id) {
            *node.pos_mut() = self.to;
        }
    }

    pub(crate) fn inverse(&self) -> Command {
        Self {
            id: self.id,
            from: self.to,
            to: self.from,
        }
        .into()
    }
}

impl SetTextContentCommand {
    pub(crate) fn apply(&self, app: &mut App, _ctx: &egui::Context) {
        app.document.text_mut(self.id).content = self.new.clone();
    }

    pub(crate) fn inverse(&self) -> Command {
        Self {
            id: self.id,
            old: self.new.clone(),
            new: self.old.clone(),
        }
        .into()
    }
}

impl Command {
    pub(crate) fn apply(&self, app: &mut App, ctx: &egui::Context) {
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
