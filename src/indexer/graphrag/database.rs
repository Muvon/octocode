// GraphRAG database operations

use crate::indexer::graphrag::types::{CodeGraph, CodeNode, CodeRelationship};
use crate::indexer::graphrag::utils::cosine_similarity;
use crate::store::Store;
use anyhow::Result;
use arrow::array::Array;
use arrow::datatypes::{DataType, Field, Schema};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

pub struct DatabaseOperations<'a> {
	store: &'a Store,
}

impl<'a> DatabaseOperations<'a> {
	pub fn new(store: &'a Store) -> Self {
		Self { store }
	}

	// Load the existing graph from database
	pub async fn load_graph(&self, _project_root: &Path) -> Result<CodeGraph> {
		let mut graph = CodeGraph::default();

		// Check if the tables exist
		if !self.store.tables_exist(&["graphrag_nodes", "graphrag_relationships"]).await? {
			return Ok(graph); // Return empty graph if tables don't exist
		}

		// Get vector dimension for embedding work
		let vector_dim = self.store.get_vector_dim();

		// Get all nodes
		let node_batch = self.store.search_graph_nodes(&vec![0.0; vector_dim], 10000).await?;
		if node_batch.num_rows() == 0 {
			return Ok(graph); // No nodes found
		}

		println!("Loading {} GraphRAG nodes from database...", node_batch.num_rows());

		// Process nodes
		let id_array = node_batch.column_by_name("id").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
		let name_array = node_batch.column_by_name("name").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
		let kind_array = node_batch.column_by_name("kind").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
		let path_array = node_batch.column_by_name("path").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
		let description_array = node_batch.column_by_name("description").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
		let symbols_array = node_batch.column_by_name("symbols").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
		let imports_array = node_batch.column_by_name("imports").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
		let exports_array = node_batch.column_by_name("exports").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
		let hash_array = node_batch.column_by_name("hash").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();

		// Get the embedding fixed size list array
		let embedding_array = node_batch.column_by_name("embedding").unwrap()
			.as_any()
			.downcast_ref::<arrow::array::FixedSizeListArray>()
			.unwrap();

		// Get the values of the embedding array
		let embedding_values = embedding_array.values()
			.as_any()
			.downcast_ref::<arrow::array::Float32Array>()
			.unwrap();

		// Process each row
		for i in 0..node_batch.num_rows() {
			let id = id_array.value(i).to_string();
			let name = name_array.value(i).to_string();
			let kind = kind_array.value(i).to_string();
			let path = path_array.value(i).to_string();
			let description = description_array.value(i).to_string();

			// Parse symbols JSON
			let symbols: Vec<String> = if symbols_array.is_null(i) {
				Vec::new()
			} else {
				serde_json::from_str(symbols_array.value(i)).unwrap_or_default()
			};

			// Parse imports JSON
			let imports: Vec<String> = if imports_array.is_null(i) {
				Vec::new()
			} else {
				serde_json::from_str(imports_array.value(i)).unwrap_or_default()
			};

			// Parse exports JSON
			let exports: Vec<String> = if exports_array.is_null(i) {
				Vec::new()
			} else {
				serde_json::from_str(exports_array.value(i)).unwrap_or_default()
			};

			let hash = hash_array.value(i).to_string();

			// Extract the embedding for this node
			let embedding_offset = i * embedding_array.value_length() as usize;
			let embedding_len = embedding_array.value_length() as usize;
			let mut embedding = Vec::with_capacity(embedding_len);

			for j in 0..embedding_len {
				embedding.push(embedding_values.value(embedding_offset + j));
			}

			// Create the node
			let node = CodeNode {
				id,
				name,
				kind,
				path,
				description,
				symbols,
				imports,
				exports,
				functions: Vec::new(), // Default empty for nodes loaded from old schema
				size_lines: 0, // Default for nodes loaded from old schema
				language: "unknown".to_string(), // Default for nodes loaded from old schema
				hash,
				embedding,
			};

			// Add to graph
			graph.nodes.insert(node.id.clone(), node);
		}

		// Load relationships
		let rel_batch = self.store.get_graph_relationships().await?;
		if rel_batch.num_rows() > 0 {
			println!("Loading {} GraphRAG relationships from database...", rel_batch.num_rows());

			// Process relationships
			let source_array = rel_batch.column_by_name("source").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
			let target_array = rel_batch.column_by_name("target").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
			let type_array = rel_batch.column_by_name("relation_type").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
			let desc_array = rel_batch.column_by_name("description").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
			let conf_array = rel_batch.column_by_name("confidence").unwrap().as_any().downcast_ref::<arrow::array::Float32Array>().unwrap();

			// Process each relationship
			for i in 0..rel_batch.num_rows() {
				let relationship = CodeRelationship {
					source: source_array.value(i).to_string(),
					target: target_array.value(i).to_string(),
					relation_type: type_array.value(i).to_string(),
					description: desc_array.value(i).to_string(),
					confidence: conf_array.value(i),
					weight: 1.0, // Default weight for legacy relationships
				};

				// Add to graph
				graph.relationships.push(relationship);
			}
		}

		if !graph.nodes.is_empty() {
			println!("Loaded GraphRAG knowledge graph with {} nodes and {} relationships",
				graph.nodes.len(), graph.relationships.len());
		}

		Ok(graph)
	}

	// Save just the newly added nodes and relationships in batches
	pub async fn save_graph_incremental(&self, new_nodes: &[CodeNode], new_relationships: &[CodeRelationship]) -> Result<()> {
		if new_nodes.is_empty() && new_relationships.is_empty() {
			// Nothing to save
			return Ok(());
		}

		// First, save any new nodes
		if !new_nodes.is_empty() {
			// Create a HashMap for the batch conversion function
			let nodes_map: HashMap<String, CodeNode> = new_nodes.iter()
				.map(|node| (node.id.clone(), node.clone()))
				.collect();

			// Convert just these nodes to a RecordBatch
			let nodes_batch = self.nodes_to_batch(&nodes_map).await?;

			// Store the nodes in the database (appending to existing data)
			self.store.store_graph_nodes(nodes_batch).await?;
		}

		// Now save any new relationships
		if !new_relationships.is_empty() {
			// Convert just these relationships to a RecordBatch
			let rel_batch = self.relationships_to_batch(new_relationships).await?;

			// Store the relationships in the database (appending to existing data)
			self.store.store_graph_relationships(rel_batch).await?;
		}

		Ok(())
	}

	// Search for nodes in database
	pub async fn search_nodes_in_database(&self, query_embedding: &[f32], query: &str) -> Result<Vec<CodeNode>> {
		// Check if the tables exist
		if !self.store.tables_exist(&["graphrag_nodes"]).await? {
			return Ok(Vec::new()); // No nodes in database
		}

		// Search for nodes similar to the query (increased limit to 50)
		let node_batch = self.store.search_graph_nodes(query_embedding, 50).await?;
		if node_batch.num_rows() == 0 {
			return Ok(Vec::new());
		}

		// Process the results into CodeNode objects
		let mut nodes = Vec::new();
		let query_lower = query.to_lowercase();

		// Extract columns from the batch
		let id_array = node_batch.column_by_name("id").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
		let name_array = node_batch.column_by_name("name").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
		let kind_array = node_batch.column_by_name("kind").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
		let path_array = node_batch.column_by_name("path").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
		let description_array = node_batch.column_by_name("description").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
		let symbols_array = node_batch.column_by_name("symbols").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();
		let hash_array = node_batch.column_by_name("hash").unwrap().as_any().downcast_ref::<arrow::array::StringArray>().unwrap();

		// Get the embedding fixed size list array
		let embedding_array = node_batch.column_by_name("embedding").unwrap()
			.as_any()
			.downcast_ref::<arrow::array::FixedSizeListArray>()
			.unwrap();

		// Get the values of the embedding array
		let embedding_values = embedding_array.values()
			.as_any()
			.downcast_ref::<arrow::array::Float32Array>()
			.unwrap();

		// Process each row
		for i in 0..node_batch.num_rows() {
			let id = id_array.value(i).to_string();
			let name = name_array.value(i).to_string();
			let kind = kind_array.value(i).to_string();
			let path = path_array.value(i).to_string();
			let description = description_array.value(i).to_string();

			// Parse symbols JSON
			let symbols: Vec<String> = if symbols_array.is_null(i) {
				Vec::new()
			} else {
				serde_json::from_str(symbols_array.value(i)).unwrap_or_default()
			};

			let hash = hash_array.value(i).to_string();

			// Extract the embedding for this node
			let embedding_offset = i * embedding_array.value_length() as usize;
			let embedding_len = embedding_array.value_length() as usize;
			let mut embedding = Vec::with_capacity(embedding_len);

			for j in 0..embedding_len {
				embedding.push(embedding_values.value(embedding_offset + j));
			}

			// Calculate semantic similarity
			let similarity = cosine_similarity(query_embedding, &embedding);

			// Check if the query is a substring of various node fields
			let name_contains = name.to_lowercase().contains(&query_lower);
			let kind_contains = kind.to_lowercase().contains(&query_lower);
			let desc_contains = description.to_lowercase().contains(&query_lower);
			let symbols_contain = symbols.iter().any(|s| s.to_lowercase().contains(&query_lower));

			// Use a lower threshold for semantic similarity (0.5 instead of 0.6)
			// OR include if the query is a substring of any important field
			if similarity > 0.5 || name_contains || kind_contains || desc_contains || symbols_contain {
				// Create the node
				let node = CodeNode {
					id,
					name,
					kind,
					path,
					description,
					symbols,
					imports: Vec::new(), // Default empty for nodes loaded from old schema
					exports: Vec::new(), // Default empty for nodes loaded from old schema
					functions: Vec::new(), // Default empty for nodes loaded from old schema
					size_lines: 0, // Default for nodes loaded from old schema
					language: "unknown".to_string(), // Default for nodes loaded from old schema
					hash,
					embedding,
				};

				// Add to results
				nodes.push(node);
			}
		}

		// Sort nodes by relevance (exact matches first, then by similarity)
		nodes.sort_by(|a, b| {
			let a_contains = a.name.to_lowercase().contains(&query_lower) ||
			a.kind.to_lowercase().contains(&query_lower) ||
			a.symbols.iter().any(|s| s.to_lowercase().contains(&query_lower));

			let b_contains = b.name.to_lowercase().contains(&query_lower) ||
			b.kind.to_lowercase().contains(&query_lower) ||
			b.symbols.iter().any(|s| s.to_lowercase().contains(&query_lower));

			if a_contains && !b_contains {
				std::cmp::Ordering::Less
			} else if !a_contains && b_contains {
				return std::cmp::Ordering::Greater;
			} else {
				// Both contain or both don't contain, sort by similarity
				let a_sim = cosine_similarity(query_embedding, &a.embedding);
				let b_sim = cosine_similarity(query_embedding, &b.embedding);
				return b_sim.partial_cmp(&a_sim).unwrap_or(std::cmp::Ordering::Equal);
			}
		});

		Ok(nodes)
	}

	// Convert nodes to a RecordBatch for database storage with updated schema
	async fn nodes_to_batch(&self, nodes: &HashMap<String, CodeNode>) -> Result<arrow::record_batch::RecordBatch> {
		// Get the vector dimension from the store
		let vector_dim = self.store.get_vector_dim();

		// Create updated schema with new fields
		let schema = Arc::new(Schema::new(vec![
			Field::new("id", DataType::Utf8, false),
			Field::new("name", DataType::Utf8, false),
			Field::new("kind", DataType::Utf8, false),
			Field::new("path", DataType::Utf8, false),
			Field::new("description", DataType::Utf8, false),
			Field::new("symbols", DataType::Utf8, true),  // JSON serialized
			Field::new("imports", DataType::Utf8, true),  // JSON serialized
			Field::new("exports", DataType::Utf8, true),  // JSON serialized
			Field::new("functions", DataType::Utf8, true), // JSON serialized
			Field::new("size_lines", DataType::UInt32, false),
			Field::new("language", DataType::Utf8, false),
			Field::new("hash", DataType::Utf8, false),
			Field::new(
				"embedding",
				DataType::FixedSizeList(
					Arc::new(Field::new("item", DataType::Float32, true)),
					vector_dim as i32,
				),
				true,
			),
		]));

		// Prepare arrays
		let nodes_vec: Vec<&CodeNode> = nodes.values().collect();
		if nodes_vec.is_empty() {
			return Err(anyhow::anyhow!("No nodes to convert to batch"));
		}

		let ids: Vec<&str> = nodes_vec.iter().map(|n| n.id.as_str()).collect();
		let names: Vec<&str> = nodes_vec.iter().map(|n| n.name.as_str()).collect();
		let kinds: Vec<&str> = nodes_vec.iter().map(|n| n.kind.as_str()).collect();
		let paths: Vec<&str> = nodes_vec.iter().map(|n| n.path.as_str()).collect();
		let descriptions: Vec<&str> = nodes_vec.iter().map(|n| n.description.as_str()).collect();
		let symbols: Vec<String> = nodes_vec.iter().map(|n| serde_json::to_string(&n.symbols).unwrap_or_default()).collect();
		let imports: Vec<String> = nodes_vec.iter().map(|n| serde_json::to_string(&n.imports).unwrap_or_default()).collect();
		let exports: Vec<String> = nodes_vec.iter().map(|n| serde_json::to_string(&n.exports).unwrap_or_default()).collect();
		let functions: Vec<String> = nodes_vec.iter().map(|n| serde_json::to_string(&n.functions).unwrap_or_default()).collect();
		let size_lines: Vec<u32> = nodes_vec.iter().map(|n| n.size_lines).collect();
		let languages: Vec<&str> = nodes_vec.iter().map(|n| n.language.as_str()).collect();
		let hashes: Vec<&str> = nodes_vec.iter().map(|n| n.hash.as_str()).collect();

		// Create the embedding fixed size list array
		let mut flattened_embeddings = Vec::with_capacity(nodes_vec.len() * vector_dim);
		for node in &nodes_vec {
			// Ensure embeddings have the correct dimension
			if node.embedding.len() != vector_dim {
				return Err(anyhow::anyhow!(
					"Node embedding has dimension {} but expected {}",
					node.embedding.len(), vector_dim
				));
			}
			flattened_embeddings.extend_from_slice(&node.embedding);
		}
		let values = arrow::array::Float32Array::from(flattened_embeddings);

		// Create the fixed size list array
		let embedding_array = arrow::array::FixedSizeListArray::new(
			Arc::new(Field::new("item", DataType::Float32, true)),
			vector_dim as i32,
			Arc::new(values),
			None, // No validity buffer - all values are valid
		);

		// Create record batch
		let batch = arrow::record_batch::RecordBatch::try_new(
			schema,
			vec![
				Arc::new(arrow::array::StringArray::from(ids)),
				Arc::new(arrow::array::StringArray::from(names)),
				Arc::new(arrow::array::StringArray::from(kinds)),
				Arc::new(arrow::array::StringArray::from(paths)),
				Arc::new(arrow::array::StringArray::from(descriptions)),
				Arc::new(arrow::array::StringArray::from(symbols)),
				Arc::new(arrow::array::StringArray::from(imports)),
				Arc::new(arrow::array::StringArray::from(exports)),
				Arc::new(arrow::array::StringArray::from(functions)),
				Arc::new(arrow::array::UInt32Array::from(size_lines)),
				Arc::new(arrow::array::StringArray::from(languages)),
				Arc::new(arrow::array::StringArray::from(hashes)),
				Arc::new(embedding_array),
			],
		)?;

		Ok(batch)
	}

	// Convert relationships to a RecordBatch for database storage
	async fn relationships_to_batch(&self, relationships: &[CodeRelationship]) -> Result<arrow::record_batch::RecordBatch> {
		// Create updated schema with weight field
		let schema = Arc::new(Schema::new(vec![
			Field::new("id", DataType::Utf8, false),
			Field::new("source", DataType::Utf8, false),
			Field::new("target", DataType::Utf8, false),
			Field::new("relation_type", DataType::Utf8, false),
			Field::new("description", DataType::Utf8, false),
			Field::new("confidence", DataType::Float32, false),
			Field::new("weight", DataType::Float32, false),
		]));

		// Generate unique IDs
		let ids: Vec<String> = relationships.iter().map(|_| uuid::Uuid::new_v4().to_string()).collect();
		let sources: Vec<&str> = relationships.iter().map(|r| r.source.as_str()).collect();
		let targets: Vec<&str> = relationships.iter().map(|r| r.target.as_str()).collect();
		let types: Vec<&str> = relationships.iter().map(|r| r.relation_type.as_str()).collect();
		let descriptions: Vec<&str> = relationships.iter().map(|r| r.description.as_str()).collect();
		let confidences: Vec<f32> = relationships.iter().map(|r| r.confidence).collect();
		let weights: Vec<f32> = relationships.iter().map(|r| r.weight).collect();

		// Create record batch
		let batch = arrow::record_batch::RecordBatch::try_new(
			schema,
			vec![
				Arc::new(arrow::array::StringArray::from(ids)),
				Arc::new(arrow::array::StringArray::from(sources)),
				Arc::new(arrow::array::StringArray::from(targets)),
				Arc::new(arrow::array::StringArray::from(types)),
				Arc::new(arrow::array::StringArray::from(descriptions)),
				Arc::new(arrow::array::Float32Array::from(confidences)),
				Arc::new(arrow::array::Float32Array::from(weights)),
			],
		)?;

		Ok(batch)
	}
}
