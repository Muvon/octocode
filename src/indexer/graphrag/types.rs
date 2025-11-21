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
use std::fmt;
use std::str::FromStr;

/// Direction for relationship queries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelationshipDirection {
	/// Relationships where the node is the source
	Outgoing,
	/// Relationships where the node is the target
	Incoming,
	/// Both outgoing and incoming relationships
	Both,
}

/// Type of relationship between code nodes
/// Ordered by architectural importance (high to low)
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationType {
	// High importance - Architectural relationships (weight: 1.0)
	/// Interface or trait implementation
	Implements,
	/// Class inheritance or module extension
	Extends,
	/// Dependency injection or configuration setup
	Configures,
	/// High-level architectural dependency
	ArchitecturalDependency,

	// Medium importance - Structural relationships (weight: 0.7)
	/// Module or package import
	Imports,
	/// Function or method call
	Calls,
	/// Utility or service usage
	Uses,
	/// Factory pattern instantiation
	FactoryCreates,
	/// Observer pattern (event listening, callbacks)
	ObserverPattern,
	/// Strategy pattern (algorithm selection)
	StrategyPattern,
	/// Adapter pattern (interface adaptation)
	AdapterPattern,

	// Low importance - Organizational relationships (weight: 0.3)
	/// Files in the same directory
	SiblingModule,
	/// Parent directory relationship
	ParentModule,
	/// Child directory relationship
	ChildModule,
}

impl RelationType {
	/// Get importance weight for ranking (0.0-1.0)
	/// Higher values indicate more architecturally significant relationships
	pub fn importance_weight(&self) -> f32 {
		match self {
			// High importance - architectural patterns
			Self::Implements | Self::Extends | Self::Configures => 1.0,
			Self::ArchitecturalDependency => 0.9,

			// Medium importance - structural dependencies
			Self::Imports | Self::Calls | Self::Uses => 0.7,
			Self::FactoryCreates
			| Self::ObserverPattern
			| Self::StrategyPattern
			| Self::AdapterPattern => 0.8,

			// Low importance - organizational structure
			Self::SiblingModule | Self::ParentModule | Self::ChildModule => 0.3,
		}
	}

	/// Convert to string (for database storage and display)
	pub fn as_str(&self) -> &'static str {
		match self {
			Self::Implements => "implements",
			Self::Extends => "extends",
			Self::Configures => "configures",
			Self::ArchitecturalDependency => "architectural_dependency",
			Self::Imports => "imports",
			Self::Calls => "calls",
			Self::Uses => "uses",
			Self::FactoryCreates => "factory_creates",
			Self::ObserverPattern => "observer_pattern",
			Self::StrategyPattern => "strategy_pattern",
			Self::AdapterPattern => "adapter_pattern",
			Self::SiblingModule => "sibling_module",
			Self::ParentModule => "parent_module",
			Self::ChildModule => "child_module",
		}
	}
}

impl FromStr for RelationType {
	type Err = ();

	/// Parse from string (for backward compatibility with old String-based storage)
	fn from_str(s: &str) -> Result<Self, Self::Err> {
		Ok(match s {
			"implements" => Self::Implements,
			"extends" => Self::Extends,
			"configures" => Self::Configures,
			"architectural_dependency" => Self::ArchitecturalDependency,
			"imports" => Self::Imports,
			"calls" => Self::Calls,
			"uses" => Self::Uses,
			"factory_creates" => Self::FactoryCreates,
			"observer_pattern" => Self::ObserverPattern,
			"strategy_pattern" => Self::StrategyPattern,
			"adapter_pattern" => Self::AdapterPattern,
			"sibling_module" => Self::SiblingModule,
			"parent_module" => Self::ParentModule,
			"child_module" => Self::ChildModule,
			// Default fallback for unknown types
			_ => Self::Imports,
		})
	}
}

impl fmt::Display for RelationType {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.as_str())
	}
}

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
	pub source: String,              // Source node ID (relative path)
	pub target: String,              // Target node ID (relative path)
	pub relation_type: RelationType, // Type: imports, calls, extends, implements, etc.
	pub description: String,         // Brief description
	pub confidence: f32,             // Confidence score (0.0-1.0)
	pub weight: f32,                 // Relationship strength/frequency
}

impl CodeRelationship {
	/// Create a new relationship with default values
	pub fn new(
		source: String,
		target: String,
		relation_type: RelationType,
		description: String,
	) -> Self {
		Self {
			source,
			target,
			relation_type,
			description,
			confidence: 0.9, // Default high confidence for AST-based relationships
			weight: 1.0,     // Default weight, will be calculated later
		}
	}

	/// Create from legacy string-based relation_type (for backward compatibility)
	pub fn from_legacy(
		source: String,
		target: String,
		relation_type_str: &str,
		description: String,
		confidence: f32,
		weight: f32,
	) -> Self {
		Self {
			source,
			target,
			relation_type: relation_type_str.parse().unwrap_or(RelationType::Imports),
			description,
			confidence,
			weight,
		}
	}
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_relationship_type_importance_weights() {
		// High importance relationships
		assert_eq!(RelationType::Implements.importance_weight(), 1.0);
		assert_eq!(RelationType::Extends.importance_weight(), 1.0);
		assert_eq!(RelationType::Configures.importance_weight(), 1.0);
		assert_eq!(
			RelationType::ArchitecturalDependency.importance_weight(),
			0.9
		);

		// Medium importance relationships
		assert_eq!(RelationType::Imports.importance_weight(), 0.7);
		assert_eq!(RelationType::Calls.importance_weight(), 0.7);
		assert_eq!(RelationType::Uses.importance_weight(), 0.7);
		assert_eq!(RelationType::FactoryCreates.importance_weight(), 0.8);

		// Low importance relationships
		assert_eq!(RelationType::SiblingModule.importance_weight(), 0.3);
		assert_eq!(RelationType::ParentModule.importance_weight(), 0.3);
		assert_eq!(RelationType::ChildModule.importance_weight(), 0.3);
	}

	#[test]
	fn test_relationship_type_ordering() {
		// Verify architectural relationships have higher weight than structural
		assert!(
			RelationType::Implements.importance_weight()
				> RelationType::Imports.importance_weight()
		);
		assert!(
			RelationType::Extends.importance_weight() > RelationType::Calls.importance_weight()
		);

		// Verify structural relationships have higher weight than organizational
		assert!(
			RelationType::Imports.importance_weight()
				> RelationType::SiblingModule.importance_weight()
		);
		assert!(
			RelationType::Calls.importance_weight()
				> RelationType::ParentModule.importance_weight()
		);
	}

	#[test]
	fn test_relationship_type_from_str() {
		// Test all enum variants
		assert_eq!(
			"implements".parse::<RelationType>().unwrap(),
			RelationType::Implements
		);
		assert_eq!(
			"extends".parse::<RelationType>().unwrap(),
			RelationType::Extends
		);
		assert_eq!(
			"imports".parse::<RelationType>().unwrap(),
			RelationType::Imports
		);
		assert_eq!(
			"calls".parse::<RelationType>().unwrap(),
			RelationType::Calls
		);
		assert_eq!(
			"sibling_module".parse::<RelationType>().unwrap(),
			RelationType::SiblingModule
		);

		// Test unknown type defaults to Imports
		assert_eq!(
			"unknown_type".parse::<RelationType>().unwrap(),
			RelationType::Imports
		);
		assert_eq!("".parse::<RelationType>().unwrap(), RelationType::Imports);
	}

	#[test]
	fn test_relationship_type_as_str() {
		// Test string conversion
		assert_eq!(RelationType::Implements.as_str(), "implements");
		assert_eq!(RelationType::Extends.as_str(), "extends");
		assert_eq!(RelationType::Imports.as_str(), "imports");
		assert_eq!(RelationType::Calls.as_str(), "calls");
		assert_eq!(RelationType::SiblingModule.as_str(), "sibling_module");
	}

	#[test]
	fn test_relationship_type_roundtrip() {
		// Test that from_str and as_str are inverses
		let types = vec![
			RelationType::Implements,
			RelationType::Extends,
			RelationType::Imports,
			RelationType::Calls,
			RelationType::Uses,
			RelationType::SiblingModule,
		];

		for rel_type in types {
			let as_string = rel_type.as_str();
			let parsed = as_string.parse::<RelationType>().unwrap();
			assert_eq!(parsed, rel_type, "Roundtrip failed for {:?}", rel_type);
		}
	}

	#[test]
	fn test_relationship_type_display() {
		// Test Display trait
		assert_eq!(format!("{}", RelationType::Implements), "implements");
		assert_eq!(format!("{}", RelationType::Extends), "extends");
		assert_eq!(format!("{}", RelationType::Imports), "imports");
	}

	#[test]
	fn test_relationship_type_serialization() {
		// Test serde serialization
		let rel_type = RelationType::Implements;
		let serialized = serde_json::to_string(&rel_type).unwrap();
		assert_eq!(serialized, "\"implements\"");

		let deserialized: RelationType = serde_json::from_str(&serialized).unwrap();
		assert_eq!(deserialized, RelationType::Implements);
	}

	#[test]
	fn test_code_relationship_new() {
		let rel = CodeRelationship::new(
			"src/main.rs".to_string(),
			"src/lib.rs".to_string(),
			RelationType::Imports,
			"Imports lib module".to_string(),
		);

		assert_eq!(rel.source, "src/main.rs");
		assert_eq!(rel.target, "src/lib.rs");
		assert_eq!(rel.relation_type, RelationType::Imports);
		assert_eq!(rel.description, "Imports lib module");
		assert_eq!(rel.confidence, 0.9); // Default
		assert_eq!(rel.weight, 1.0); // Default
	}

	#[test]
	fn test_code_relationship_from_legacy() {
		// Test backward compatibility with string-based types
		let rel = CodeRelationship::from_legacy(
			"src/main.rs".to_string(),
			"src/lib.rs".to_string(),
			"implements",
			"Implements trait".to_string(),
			0.95,
			2.5,
		);

		assert_eq!(rel.source, "src/main.rs");
		assert_eq!(rel.target, "src/lib.rs");
		assert_eq!(rel.relation_type, RelationType::Implements);
		assert_eq!(rel.description, "Implements trait");
		assert_eq!(rel.confidence, 0.95);
		assert_eq!(rel.weight, 2.5);
	}

	#[test]
	fn test_code_relationship_legacy_unknown_type() {
		// Test that unknown legacy types default to Imports
		let rel = CodeRelationship::from_legacy(
			"src/a.rs".to_string(),
			"src/b.rs".to_string(),
			"some_old_type",
			"Description".to_string(),
			0.8,
			1.0,
		);

		assert_eq!(rel.relation_type, RelationType::Imports);
	}

	#[test]
	fn test_relationship_direction_enum() {
		// Test that enum variants exist and are distinct
		let outgoing = RelationshipDirection::Outgoing;
		let incoming = RelationshipDirection::Incoming;
		let both = RelationshipDirection::Both;

		assert_ne!(outgoing, incoming);
		assert_ne!(outgoing, both);
		assert_ne!(incoming, both);
	}

	#[test]
	fn test_code_relationship_serialization() {
		let rel = CodeRelationship::new(
			"src/main.rs".to_string(),
			"src/lib.rs".to_string(),
			RelationType::Imports,
			"Test relationship".to_string(),
		);

		// Test JSON serialization
		let json = serde_json::to_string(&rel).unwrap();
		let deserialized: CodeRelationship = serde_json::from_str(&json).unwrap();

		assert_eq!(deserialized.source, rel.source);
		assert_eq!(deserialized.target, rel.target);
		assert_eq!(deserialized.relation_type, rel.relation_type);
		assert_eq!(deserialized.description, rel.description);
	}
}
