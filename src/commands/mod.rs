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

pub mod clear;
pub mod commit;
pub mod config;
pub mod debug;
pub mod format;
pub mod graphrag;
pub mod index;
pub mod mcp;
pub mod memory;
pub mod review;
pub mod search;
pub mod view;
pub mod watch;

// Re-export all the command structs and enums
pub use commit::CommitArgs;
pub use config::ConfigArgs;
pub use debug::DebugArgs;
pub use format::FormatArgs;
pub use graphrag::GraphRAGArgs;
pub use index::IndexArgs;
pub use mcp::McpArgs;
pub use memory::MemoryArgs;
pub use review::ReviewArgs;
pub use search::SearchArgs;
pub use view::ViewArgs;
pub use watch::WatchArgs;
