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

// Main lib.rs file that exports our modules
pub mod config;
pub mod constants;
pub mod embedding;
pub mod indexer;
pub mod llm;
pub mod lock;
pub mod mcp;
pub mod memory;
pub mod reranker;
pub mod state;
pub mod storage;
pub mod store;
pub mod utils;
pub mod watcher_config;

// Re-export commonly used items for convenience
pub use config::Config;
pub use store::Store;
