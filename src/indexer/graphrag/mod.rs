// Copyright 2025 Muvon Un Limited
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// GraphRAG module entry point

pub mod ai;
pub mod builder;
pub mod database;
pub mod relationships;
pub mod types;
pub mod utils;

// Re-export the main types and interfaces for backward compatibility
pub use builder::GraphBuilder;
pub use types::{CodeGraph, CodeNode, CodeRelationship, FunctionInfo};
pub use utils::{
	cosine_similarity, detect_project_root, graphrag_nodes_to_markdown, graphrag_nodes_to_text,
	render_graphrag_nodes_json, to_relative_path,
};

// GraphRAG implementation for searching (backward compatibility)
use crate::config::Config;
use anyhow::Result;

#[derive(Clone)]
pub struct GraphRAG {
	config: Config,
}

impl GraphRAG {
	pub fn new(config: Config) -> Self {
		Self { config }
	}

	pub async fn search(&self, query: &str) -> Result<String> {
		let builder = GraphBuilder::new(self.config.clone()).await?;
		let nodes = builder.search_nodes(query).await?;
		Ok(graphrag_nodes_to_text(&nodes))
	}
}
