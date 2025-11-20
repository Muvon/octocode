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

// GraphRAG data structures and types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// A node in the code graph - represents a file/module with efficient storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeNode {
	pub id: String,           // Relative path from project root (efficient storage)
	pub name: String,         // File name or module name
	pub kind: String,         // Type of the node (file, module, package, function)
	pub path: String,         // Relative file path from project root
	pub description: String,  // Description/summary of what the file/module does
	pub symbols: Vec<String>, // All symbols from this file (functions, classes, etc.)
	pub hash: String,         // Content hash to detect changes
	pub embedding: Vec<f32>,  // Vector embedding of the file content
	pub imports: Vec<String>, // List of imported modules (relative paths or external)
	pub exports: Vec<String>, // List of exported symbols
	pub functions: Vec<FunctionInfo>, // Function-level information for better granularity
	pub size_lines: u32,      // Number of lines in the file
	pub language: String,     // Programming language
}

// Function-level information for better granularity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInfo {
	pub name: String,                // Function name
	pub signature: String,           // Function signature
	pub start_line: u32,             // Starting line number
	pub end_line: u32,               // Ending line number
	pub calls: Vec<String>,          // Functions this function calls
	pub called_by: Vec<String>,      // Functions that call this function
	pub parameters: Vec<String>,     // Function parameters
	pub return_type: Option<String>, // Return type if available
}

// A relationship between code nodes - simplified and more efficient
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeRelationship {
	pub source: String,        // Source node ID (relative path)
	pub target: String,        // Target node ID (relative path)
	pub relation_type: String, // Type: imports, calls, extends, implements, etc.
	pub description: String,   // Brief description
	pub confidence: f32,       // Confidence score (0.0-1.0)
	pub weight: f32,           // Relationship strength/frequency
}

// The full code graph
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodeGraph {
	pub nodes: HashMap<String, CodeNode>,
	pub relationships: Vec<CodeRelationship>,
}

// Helper struct for batch relationship analysis request
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BatchRelationshipResult {
	pub source_id: String,
	pub target_id: String,
	pub relation_type: String,
	pub description: String,
	pub confidence: f32,
	pub exists: bool,
}
