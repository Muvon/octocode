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

use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Default)]
pub struct IndexState {
	pub current_directory: PathBuf,
	pub indexed_files: usize,
	pub total_files: usize,
	pub skipped_files: usize, // Files skipped due to being unchanged
	pub embedding_calls: usize,
	pub indexing_complete: bool,
	pub status_message: String,
	pub force_reindex: bool,
	// GraphRAG state tracking
	pub graphrag_enabled: bool,
	pub graphrag_blocks: usize,
	// File counting state
	pub counting_files: bool,
	// Quiet mode for MCP server (no console output)
	pub quiet_mode: bool,
}

pub type SharedState = Arc<RwLock<IndexState>>;

pub fn create_shared_state() -> SharedState {
	Arc::new(RwLock::new(IndexState::default()))
}
