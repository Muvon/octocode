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

// Memory module for AI context and conversation state management
// Uses LanceDB for vector storage and semantic search capabilities

pub mod formatting;
pub mod git_utils;
pub mod manager;
pub mod store;
pub mod types;

// Re-export the main types and interfaces
pub use formatting::{format_memories_as_text, format_memories_for_cli};
pub use git_utils::{CommitInfo, GitUtils};
pub use manager::{MemoryManager, MemoryStats};
pub use store::MemoryStore;
pub use types::{
	Memory, MemoryConfig, MemoryMetadata, MemoryQuery, MemoryRelationship, MemorySearchResult,
	MemorySortBy, MemoryType, RelationshipType, SortOrder,
};
