//! AI Creative Studio Canvas & Generative Pipeline Module (`archava_studio_canvas`).
//!
//! Models node-based creative video/image canvases (Higgsfield API integrations):
//! DAG node graph composition, Soul ID character anchor persistence, prompt chaining,
//! and asset synthesis render jobs.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::erp::{bounded_title, valid_uuid};

pub const CANVAS_PERMISSIONS: &[&str] = &[
    "canvas.read",
    "canvas.edit",
    "canvas.render",
    "canvas.publish",
    "audit.read",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AspectRatio {
    Ratio16_9,
    Ratio9_16,
    Ratio1_1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    PromptText,
    HiggsfieldImageGen,
    HiggsfieldMotionVideo,
    BackgroundMatting,
    UpscaleEnhancer,
    ExportMedia,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanvasNode {
    pub node_id: String,
    pub node_type: NodeType,
    pub title: String,
    pub pos_x: f64,
    pub pos_y: f64,
    pub config: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanvasEdge {
    pub edge_id: String,
    pub source_node_id: String,
    pub target_node_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StudioCanvasProject {
    pub project_id: String,
    pub title: String,
    pub owner_id: String,
    pub aspect_ratio: AspectRatio,
    pub soul_id_anchor: Option<String>,
    pub nodes: Vec<CanvasNode>,
    pub edges: Vec<CanvasEdge>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CanvasError {
    #[error("validation error: {0}")]
    Validation(&'static str),
    #[error("node {0} not found in project canvas")]
    NodeNotFound(String),
    #[error("self-referencing edges are forbidden")]
    SelfReferenceEdge,
}

impl StudioCanvasProject {
    pub fn new(
        project_id: String,
        title: String,
        owner_id: String,
        aspect_ratio: AspectRatio,
        soul_id_anchor: Option<String>,
    ) -> Result<Self, CanvasError> {
        if !valid_uuid(&project_id) || !valid_uuid(&owner_id) {
            return Err(CanvasError::Validation("invalid uuid format"));
        }
        if bounded_title(&title, 100).is_none() {
            return Err(CanvasError::Validation(
                "canvas title must be between 1 and 100 characters",
            ));
        }

        Ok(Self {
            project_id,
            title,
            owner_id,
            aspect_ratio,
            soul_id_anchor,
            nodes: Vec::new(),
            edges: Vec::new(),
        })
    }

    pub fn add_node(
        &mut self,
        node_id: String,
        node_type: NodeType,
        title: String,
        pos_x: f64,
        pos_y: f64,
        config: Value,
    ) -> Result<(), CanvasError> {
        if !valid_uuid(&node_id) {
            return Err(CanvasError::Validation("invalid node uuid"));
        }
        if self.nodes.iter().any(|n| n.node_id == node_id) {
            return Err(CanvasError::Validation("node id already exists in canvas"));
        }

        self.nodes.push(CanvasNode {
            node_id,
            node_type,
            title,
            pos_x,
            pos_y,
            config,
        });
        Ok(())
    }

    pub fn connect_edge(
        &mut self,
        edge_id: String,
        source_node_id: String,
        target_node_id: String,
    ) -> Result<(), CanvasError> {
        if !valid_uuid(&edge_id) {
            return Err(CanvasError::Validation("invalid edge uuid"));
        }
        if source_node_id == target_node_id {
            return Err(CanvasError::SelfReferenceEdge);
        }
        if !self.nodes.iter().any(|n| n.node_id == source_node_id) {
            return Err(CanvasError::NodeNotFound(source_node_id));
        }
        if !self.nodes.iter().any(|n| n.node_id == target_node_id) {
            return Err(CanvasError::NodeNotFound(target_node_id));
        }

        self.edges.push(CanvasEdge {
            edge_id,
            source_node_id,
            target_node_id,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canvas_dag_construction() {
        let mut canvas = StudioCanvasProject::new(
            "00000000-0000-0000-0000-000000000001".into(),
            "Indomie Goreng Cyberpunk Commercial".into(),
            "00000000-0000-0000-0000-000000000002".into(),
            AspectRatio::Ratio9_16,
            Some("SOUL-AVA-001".into()),
        )
        .expect("canvas created");

        assert_eq!(canvas.aspect_ratio, AspectRatio::Ratio9_16);
        assert_eq!(canvas.soul_id_anchor, Some("SOUL-AVA-001".into()));

        let node1 = "00000000-0000-0000-0000-000000000011".to_string();
        let node2 = "00000000-0000-0000-0000-000000000022".to_string();

        canvas
            .add_node(
                node1.clone(),
                NodeType::PromptText,
                "Prompt Input".into(),
                0.0,
                0.0,
                json!({ "prompt": "Steaming hot spicy indomie on rainy street" }),
            )
            .expect("node 1 added");

        canvas
            .add_node(
                node2.clone(),
                NodeType::HiggsfieldMotionVideo,
                "Higgsfield Motion Engine".into(),
                200.0,
                0.0,
                json!({ "motion_strength": 0.85, "fps": 24 }),
            )
            .expect("node 2 added");

        canvas
            .connect_edge("00000000-0000-0000-0000-000000000099".into(), node1, node2)
            .expect("connected");

        assert_eq!(canvas.nodes.len(), 2);
        assert_eq!(canvas.edges.len(), 1);
    }
}
