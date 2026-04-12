use std::{collections::HashMap, sync::Arc};

use derive_more::{From, TryInto};
use eframe::egui::{self, Pos2};
use serde::{Deserialize, Serialize};

use crate::app::NodeId;

#[derive(Clone, Copy)]
pub(crate) struct TextNodeId(pub(crate) NodeId);

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct PersistentData {
    next_node_id: NodeId,
    nodes: HashMap<NodeId, NodeData>,
}

#[derive(Clone, Serialize, Deserialize, TryInto, From)]
pub(crate) enum NodeData {
    Text(TextData),
    Image(ImageData),
}

impl NodeData {
    pub(crate) fn pos(&self) -> Pos2 {
        match self {
            Self::Text(text) => text.pos,
            Self::Image(image) => image.pos,
        }
    }

    pub(crate) fn pos_mut(&mut self) -> &mut Pos2 {
        match self {
            Self::Text(text) => &mut text.pos,
            Self::Image(image) => &mut image.pos,
        }
    }

    pub(crate) fn as_text(&self) -> Option<&TextData> {
        match self {
            Self::Text(text) => Some(text),
            Self::Image(_) => None,
        }
    }
}

impl PersistentData {
    pub(crate) fn nodes(&self) -> &HashMap<NodeId, NodeData> {
        &self.nodes
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct TextData {
    pub(crate) content: String,
    pub(crate) width: usize,
    pub(crate) pos: Pos2,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct ImageData {
    pub(crate) data: Arc<egui::ColorImage>,
    pub(crate) pos: Pos2,
}

pub(crate) struct Document {
    dirty: bool,
    data: PersistentData,
}

impl Document {
    pub(crate) fn new(data: PersistentData) -> Self {
        Self { dirty: false, data }
    }

    pub(crate) fn data(&self) -> &PersistentData {
        &self.data
    }

    pub(crate) fn data_mut(&mut self) -> &mut PersistentData {
        self.dirty = true;
        &mut self.data
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub(crate) fn mark_clean(&mut self) {
        self.dirty = false;
    }

    pub(crate) fn alloc_node_id(&mut self) -> NodeId {
        let data = self.data_mut();
        let node_id = data.next_node_id;
        data.next_node_id += 1;
        node_id
    }

    pub(crate) fn node(&self, node_id: NodeId) -> Option<&NodeData> {
        self.data.nodes.get(&node_id)
    }

    pub(crate) fn node_mut(&mut self, node_id: NodeId) -> Option<&mut NodeData> {
        self.data_mut().nodes.get_mut(&node_id)
    }

    pub(crate) fn insert_node(&mut self, node_id: NodeId, node: NodeData) {
        self.data_mut().nodes.insert(node_id, node);
    }

    pub(crate) fn remove_node(&mut self, node_id: NodeId) -> Option<NodeData> {
        self.data_mut().nodes.remove(&node_id)
    }

    pub(crate) fn as_text_node_id(&self, node_id: NodeId) -> Option<TextNodeId> {
        matches!(self.node(node_id), Some(NodeData::Text(_))).then_some(TextNodeId(node_id))
    }

    pub(crate) fn text(&self, node_id: TextNodeId) -> &TextData {
        match self.node(node_id.0).unwrap() {
            NodeData::Text(text) => text,
            NodeData::Image(_) => unreachable!(),
        }
    }

    pub(crate) fn text_mut(&mut self, node_id: TextNodeId) -> &mut TextData {
        match self.node_mut(node_id.0).unwrap() {
            NodeData::Text(text) => text,
            NodeData::Image(_) => unreachable!(),
        }
    }
}
