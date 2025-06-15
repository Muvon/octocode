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

//! LSP (Language Server Protocol) integration for MCP server
//!
//! This module provides integration with external LSP servers, allowing users to
//! specify any LSP server command and expose its capabilities through MCP tools.

pub mod client;
pub mod protocol;
pub mod provider;
pub mod tools;

pub use provider::LspProvider;
