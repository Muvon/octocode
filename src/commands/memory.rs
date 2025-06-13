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

use anyhow::Result;
use clap::{Args, Subcommand};
use serde_json::Value;
use std::io::{self, Write};

use octocode::config::Config;
use octocode::memory::{MemoryManager, MemoryQuery, MemoryType};

#[derive(Args, Debug)]
pub struct MemoryArgs {
	#[command(subcommand)]
	pub command: MemoryCommand,
}

#[derive(Subcommand, Debug)]
pub enum MemoryCommand {
	/// Store important information, insights, or context in memory
	Memorize {
		/// Short, descriptive title for the memory (5-200 characters)
		#[arg(short, long)]
		title: String,

		/// Detailed content to remember
		#[arg(short, long)]
		content: String,

		/// Category of memory for better organization
		#[arg(short = 'm', long, default_value = "code")]
		memory_type: String,

		/// Importance score from 0.0 to 1.0 (higher = more important)
		#[arg(short, long)]
		importance: Option<f32>,

		/// Tags for categorization (comma-separated)
		#[arg(long)]
		tags: Option<String>,

		/// Related file paths (comma-separated)
		#[arg(long)]
		files: Option<String>,
	},

	/// Search and retrieve stored memories using semantic search
	Remember {
		/// What you want to remember or search for
		query: String,

		/// Filter by memory types (comma-separated)
		#[arg(short = 'm', long)]
		memory_types: Option<String>,

		/// Filter by tags (comma-separated)
		#[arg(long)]
		tags: Option<String>,

		/// Filter by related files (comma-separated)
		#[arg(long)]
		files: Option<String>,

		/// Maximum number of memories to return
		#[arg(short, long, default_value = "10")]
		limit: usize,

		/// Minimum relevance score (0.0-1.0)
		#[arg(long)]
		min_relevance: Option<f32>,

		/// Output format: text, json, or compact
		#[arg(short, long, default_value = "text")]
		format: String,
	},

	/// Permanently remove specific memories
	Forget {
		/// Specific memory ID to forget (get from remember results)
		#[arg(short, long)]
		memory_id: Option<String>,

		/// Query to find memories to forget (alternative to memory_id)
		#[arg(short, long)]
		query: Option<String>,

		/// Filter by memory types when using query (comma-separated)
		#[arg(short = 'm', long)]
		memory_types: Option<String>,

		/// Filter by tags when using query (comma-separated)
		#[arg(long)]
		tags: Option<String>,

		/// Confirm deletion without prompting
		#[arg(short = 'y', long)]
		yes: bool,
	},

	/// Update an existing memory
	Update {
		/// Memory ID to update
		memory_id: String,

		/// New title (optional)
		#[arg(short, long)]
		title: Option<String>,

		/// New content (optional)
		#[arg(short, long)]
		content: Option<String>,

		/// New importance score (optional)
		#[arg(short, long)]
		importance: Option<f32>,

		/// Add tags (comma-separated)
		#[arg(long)]
		add_tags: Option<String>,

		/// Remove tags (comma-separated)
		#[arg(long)]
		remove_tags: Option<String>,

		/// Add related files (comma-separated)
		#[arg(long)]
		add_files: Option<String>,

		/// Remove related files (comma-separated)
		#[arg(long)]
		remove_files: Option<String>,
	},

	/// Get memory by ID
	Get {
		/// Memory ID to retrieve
		memory_id: String,

		/// Output format: text, json, or compact
		#[arg(short, long, default_value = "text")]
		format: String,
	},

	/// List recent memories
	Recent {
		/// Maximum number of memories to show
		#[arg(short, long, default_value = "20")]
		limit: usize,

		/// Filter by memory type
		#[arg(short = 'm', long)]
		memory_type: Option<String>,

		/// Output format: text, json, or compact
		#[arg(short, long, default_value = "compact")]
		format: String,
	},

	/// Get memories by type
	ByType {
		/// Memory type to filter by
		memory_type: String,

		/// Maximum number of memories to show
		#[arg(short, long, default_value = "20")]
		limit: usize,

		/// Output format: text, json, or compact
		#[arg(short, long, default_value = "compact")]
		format: String,
	},

	/// Get memories related to files
	ForFiles {
		/// File paths to search for (comma-separated)
		files: String,

		/// Output format: text, json, or compact
		#[arg(short, long, default_value = "text")]
		format: String,
	},

	/// Get memories by tags
	ByTags {
		/// Tags to search for (comma-separated)
		tags: String,

		/// Output format: text, json, or compact
		#[arg(short, long, default_value = "text")]
		format: String,
	},

	/// Get memories for current Git commit
	CurrentCommit {
		/// Output format: text, json, or compact
		#[arg(short, long, default_value = "text")]
		format: String,
	},

	/// Show memory statistics
	Stats,

	/// Clean up old memories
	Cleanup {
		/// Confirm cleanup without prompting
		#[arg(short = 'y', long)]
		yes: bool,
	},

	/// Clear ALL memory data (DANGEROUS: deletes everything)
	ClearAll {
		/// Confirm deletion without prompting
		#[arg(short = 'y', long)]
		yes: bool,
	},

	/// Create a relationship between two memories
	Relate {
		/// Source memory ID
		source_id: String,

		/// Target memory ID
		target_id: String,

		/// Relationship type
		#[arg(short = 't', long, default_value = "related_to")]
		relationship_type: String,

		/// Relationship strength (0.0-1.0)
		#[arg(short, long, default_value = "0.5")]
		strength: f32,

		/// Description of the relationship
		#[arg(short, long)]
		description: String,
	},

	/// Get relationships for a memory
	Relationships {
		/// Memory ID to get relationships for
		memory_id: String,

		/// Output format: text, json, or compact
		#[arg(short, long, default_value = "text")]
		format: String,
	},

	/// Get related memories through relationships
	Related {
		/// Memory ID to find related memories for
		memory_id: String,

		/// Output format: text, json, or compact
		#[arg(short, long, default_value = "text")]
		format: String,
	},
}

pub async fn execute(config: &Config, args: &MemoryArgs) -> Result<()> {
	let mut memory_manager = MemoryManager::new(config).await?;

	match &args.command {
		MemoryCommand::Memorize {
			title,
			content,
			memory_type,
			importance,
			tags,
			files,
		} => {
			// Validate input lengths
			if title.len() < 5 || title.len() > 200 {
				return Err(anyhow::anyhow!(
					"Title must be between 5 and 200 characters"
				));
			}
			if content.len() < 10 || content.len() > 10000 {
				return Err(anyhow::anyhow!(
					"Content must be between 10 and 10000 characters"
				));
			}

			let mem_type = MemoryType::from(memory_type.clone());
			let tags_vec = tags
				.as_ref()
				.map(|t| t.split(',').map(|s| s.trim().to_string()).collect());
			let files_vec = files
				.as_ref()
				.map(|f| f.split(',').map(|s| s.trim().to_string()).collect());

			let memory = memory_manager
				.memorize(
					mem_type,
					title.clone(),
					content.clone(),
					*importance,
					tags_vec,
					files_vec,
				)
				.await?;

			println!("✅ Memory stored successfully!");
			println!("Memory ID: {}", memory.id);
			println!("Type: {}", memory.memory_type);
			println!("Title: {}", memory.title);
			if let Some(imp) = importance {
				println!("Importance: {:.2}", imp);
			}
		}

		MemoryCommand::Remember {
			query,
			memory_types,
			tags,
			files,
			limit,
			min_relevance,
			format,
		} => {
			let mem_types = memory_types.as_ref().map(|types| {
				types
					.split(',')
					.map(|s| MemoryType::from(s.trim().to_string()))
					.collect()
			});

			let tags_vec = tags
				.as_ref()
				.map(|t| t.split(',').map(|s| s.trim().to_string()).collect());

			let files_vec = files
				.as_ref()
				.map(|f| f.split(',').map(|s| s.trim().to_string()).collect());

			let memory_query = MemoryQuery {
				memory_types: mem_types,
				tags: tags_vec,
				related_files: files_vec,
				limit: Some(*limit.min(&50)),
				min_relevance: *min_relevance,
				..Default::default()
			};

			let results = memory_manager.remember(query, Some(memory_query)).await?;

			if results.is_empty() {
				println!("❌ No memories found matching your query.");
				println!("Try using different search terms or removing filters.");
				return Ok(());
			}

			match format.as_str() {
				"json" => {
					let json_results: Vec<Value> = results
						.iter()
						.map(|r| {
							serde_json::json!({
								"memory_id": r.memory.id,
								"title": r.memory.title,
								"memory_type": r.memory.memory_type.to_string(),
								"relevance_score": r.relevance_score,
								"content": r.memory.content,
								"created_at": r.memory.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
								"tags": r.memory.metadata.tags,
								"related_files": r.memory.metadata.related_files,
								"importance": r.memory.metadata.importance,
								"selection_reason": r.selection_reason
							})
						})
						.collect();
					println!("{}", serde_json::to_string_pretty(&json_results)?);
				}
				"compact" => {
					println!("🧠 Found {} memories:", results.len());
					for (i, result) in results.iter().enumerate() {
						println!(
							"{}. [{}] {} (Score: {:.2}) - {}",
							i + 1,
							result.memory.memory_type,
							result.memory.title,
							result.relevance_score,
							result.memory.id
						);
					}
				}
				_ => {
					// Default text format
					println!("🧠 Found {} memories:\n", results.len());
					for (i, result) in results.iter().enumerate() {
						println!("{}. Memory ID: {}", i + 1, result.memory.id);
						println!("   Title: {}", result.memory.title);
						println!("   Type: {}", result.memory.memory_type);
						println!("   Relevance: {:.2}", result.relevance_score);
						println!("   Importance: {:.2}", result.memory.metadata.importance);
						println!(
							"   Created: {}",
							result.memory.created_at.format("%Y-%m-%d %H:%M:%S")
						);
						if !result.memory.metadata.tags.is_empty() {
							println!("   Tags: {}", result.memory.metadata.tags.join(", "));
						}
						if !result.memory.metadata.related_files.is_empty() {
							println!(
								"   Files: {}",
								result.memory.metadata.related_files.join(", ")
							);
						}
						println!("   Content: {}", result.memory.content);
						println!("   Why selected: {}", result.selection_reason);
						println!();
					}
				}
			}
		}

		MemoryCommand::Forget {
			memory_id,
			query,
			memory_types,
			tags,
			yes,
		} => {
			if let Some(id) = memory_id {
				if !yes {
					print!("Are you sure you want to delete memory '{}'? (y/N): ", id);
					io::stdout().flush()?;
					let mut input = String::new();
					io::stdin().read_line(&mut input)?;
					if !input.trim().to_lowercase().starts_with('y') {
						println!("Deletion cancelled.");
						return Ok(());
					}
				}

				memory_manager.forget(id).await?;
				println!("✅ Memory '{}' deleted successfully.", id);
			} else if let Some(q) = query {
				if q.len() < 3 || q.len() > 500 {
					return Err(anyhow::anyhow!(
						"Query must be between 3 and 500 characters"
					));
				}

				let mem_types = memory_types.as_ref().map(|types| {
					types
						.split(',')
						.map(|s| MemoryType::from(s.trim().to_string()))
						.collect()
				});

				let tags_vec = tags
					.as_ref()
					.map(|t| t.split(',').map(|s| s.trim().to_string()).collect());

				let memory_query = MemoryQuery {
					query_text: Some(q.clone()),
					memory_types: mem_types,
					tags: tags_vec,
					..Default::default()
				};

				if !yes {
					// First show what would be deleted
					let preview_results = memory_manager
						.remember(q, Some(memory_query.clone()))
						.await?;
					if preview_results.is_empty() {
						println!("❌ No memories found matching your query.");
						return Ok(());
					}

					println!(
						"Found {} memories that would be deleted:",
						preview_results.len()
					);
					for result in &preview_results {
						println!("- [{}] {}", result.memory.id, result.memory.title);
					}

					print!(
						"Are you sure you want to delete these {} memories? (y/N): ",
						preview_results.len()
					);
					io::stdout().flush()?;
					let mut input = String::new();
					io::stdin().read_line(&mut input)?;
					if !input.trim().to_lowercase().starts_with('y') {
						println!("Deletion cancelled.");
						return Ok(());
					}
				}

				let deleted_count = memory_manager.forget_matching(memory_query).await?;
				println!("✅ {} memories deleted successfully.", deleted_count);
			} else {
				return Err(anyhow::anyhow!(
					"Either --memory-id or --query must be provided"
				));
			}
		}

		MemoryCommand::Update {
			memory_id,
			title,
			content,
			importance: _,
			add_tags,
			remove_tags,
			add_files,
			remove_files,
		} => {
			// Update basic fields
			let updated_memory = memory_manager
				.update_memory(memory_id, title.clone(), content.clone(), None)
				.await?;

			if updated_memory.is_none() {
				println!("❌ Memory '{}' not found.", memory_id);
				return Ok(());
			}

			// Handle tag operations
			if let Some(tags_to_add) = add_tags {
				for tag in tags_to_add.split(',') {
					memory_manager
						.add_tag(memory_id, tag.trim().to_string())
						.await?;
				}
			}
			if let Some(tags_to_remove) = remove_tags {
				for tag in tags_to_remove.split(',') {
					memory_manager.remove_tag(memory_id, tag.trim()).await?;
				}
			}

			// Handle file operations
			if let Some(files_to_add) = add_files {
				for file in files_to_add.split(',') {
					memory_manager
						.add_related_file(memory_id, file.trim().to_string())
						.await?;
				}
			}
			if let Some(files_to_remove) = remove_files {
				for file in files_to_remove.split(',') {
					memory_manager
						.remove_related_file(memory_id, file.trim())
						.await?;
				}
			}

			println!("✅ Memory '{}' updated successfully.", memory_id);
		}

		MemoryCommand::Get { memory_id, format } => {
			if let Some(memory) = memory_manager.get_memory(memory_id).await? {
				match format.as_str() {
					"json" => {
						println!("{}", serde_json::to_string_pretty(&memory)?);
					}
					"compact" => {
						println!("[{}] {} - {}", memory.memory_type, memory.title, memory.id);
					}
					_ => {
						println!("Memory ID: {}", memory.id);
						println!("Title: {}", memory.title);
						println!("Type: {}", memory.memory_type);
						println!("Importance: {:.2}", memory.metadata.importance);
						println!("Created: {}", memory.created_at.format("%Y-%m-%d %H:%M:%S"));
						println!("Updated: {}", memory.updated_at.format("%Y-%m-%d %H:%M:%S"));
						if !memory.metadata.tags.is_empty() {
							println!("Tags: {}", memory.metadata.tags.join(", "));
						}
						if !memory.metadata.related_files.is_empty() {
							println!("Files: {}", memory.metadata.related_files.join(", "));
						}
						if let Some(commit) = &memory.metadata.git_commit {
							println!("Git commit: {}", commit);
						}
						println!("Content:\n{}", memory.content);
					}
				}
			} else {
				println!("❌ Memory '{}' not found.", memory_id);
			}
		}

		MemoryCommand::Recent {
			limit,
			memory_type,
			format,
		} => {
			let memories = if let Some(mem_type) = memory_type {
				let parsed_type = MemoryType::from(mem_type.clone());
				memory_manager
					.get_memories_by_type(parsed_type, Some(*limit))
					.await?
			} else {
				memory_manager.get_recent_memories(*limit).await?
			};

			if memories.is_empty() {
				println!("❌ No recent memories found.");
				return Ok(());
			}

			format_memories(&memories, format);
		}

		MemoryCommand::ByType {
			memory_type,
			limit,
			format,
		} => {
			let parsed_type = MemoryType::from(memory_type.clone());
			let memories = memory_manager
				.get_memories_by_type(parsed_type, Some(*limit))
				.await?;

			if memories.is_empty() {
				println!("❌ No memories found for type '{}'.", memory_type);
				return Ok(());
			}

			format_memories(&memories, format);
		}

		MemoryCommand::ForFiles { files, format } => {
			let file_paths: Vec<String> = files.split(',').map(|s| s.trim().to_string()).collect();
			let results = memory_manager.get_memories_for_files(file_paths).await?;

			if results.is_empty() {
				println!("❌ No memories found for the specified files.");
				return Ok(());
			}

			format_search_results(&results, format);
		}

		MemoryCommand::ByTags { tags, format } => {
			let tag_list: Vec<String> = tags.split(',').map(|s| s.trim().to_string()).collect();
			let results = memory_manager.get_memories_by_tags(tag_list).await?;

			if results.is_empty() {
				println!("❌ No memories found for the specified tags.");
				return Ok(());
			}

			format_search_results(&results, format);
		}

		MemoryCommand::CurrentCommit { format } => {
			let memories = memory_manager.get_memories_for_current_commit().await?;

			if memories.is_empty() {
				println!("❌ No memories found for the current Git commit.");
				return Ok(());
			}

			format_memories(&memories, format);
		}

		MemoryCommand::Stats => {
			let stats = memory_manager.get_memory_stats().await?;
			print!("{}", stats.format());
		}

		MemoryCommand::Cleanup { yes } => {
			if !yes {
				print!("Are you sure you want to clean up old memories? (y/N): ");
				io::stdout().flush()?;
				let mut input = String::new();
				io::stdin().read_line(&mut input)?;
				if !input.trim().to_lowercase().starts_with('y') {
					println!("Cleanup cancelled.");
					return Ok(());
				}
			}

			let cleaned_count = memory_manager.cleanup().await?;
			println!("✅ Cleaned up {} old memories.", cleaned_count);
		}

		MemoryCommand::ClearAll { yes } => {
			if !yes {
				println!(
					"⚠️  WARNING: This will delete ALL memories and relationships permanently!"
				);
				print!("Are you absolutely sure you want to clear ALL memory data? (y/N): ");
				io::stdout().flush()?;
				let mut input = String::new();
				io::stdin().read_line(&mut input)?;
				if !input.trim().to_lowercase().starts_with('y') {
					println!("Clear all cancelled.");
					return Ok(());
				}
			}

			let deleted_count = memory_manager.clear_all().await?;
			println!(
				"✅ Cleared all memory data. {} records deleted.",
				deleted_count
			);
		}

		MemoryCommand::Relate {
			source_id,
			target_id,
			relationship_type,
			strength,
			description,
		} => {
			let rel_type = match relationship_type.as_str() {
				"related_to" => octocode::memory::RelationshipType::RelatedTo,
				"depends_on" => octocode::memory::RelationshipType::DependsOn,
				"supersedes" => octocode::memory::RelationshipType::Supersedes,
				"similar" => octocode::memory::RelationshipType::Similar,
				"conflicts" => octocode::memory::RelationshipType::Conflicts,
				"implements" => octocode::memory::RelationshipType::Implements,
				"extends" => octocode::memory::RelationshipType::Extends,
				_ => octocode::memory::RelationshipType::Custom(relationship_type.clone()),
			};

			let relationship = memory_manager
				.create_relationship(
					source_id.clone(),
					target_id.clone(),
					rel_type,
					*strength,
					description.clone(),
				)
				.await?;

			println!("✅ Relationship created successfully!");
			println!("Relationship ID: {}", relationship.id);
			println!("Type: {}", relationship.relationship_type);
			println!("Strength: {:.2}", relationship.strength);
		}

		MemoryCommand::Relationships { memory_id, format } => {
			let relationships = memory_manager.get_relationships(memory_id).await?;

			if relationships.is_empty() {
				println!("❌ No relationships found for memory '{}'.", memory_id);
				return Ok(());
			}

			match format.as_str() {
				"json" => {
					println!("{}", serde_json::to_string_pretty(&relationships)?);
				}
				"compact" => {
					println!("🔗 {} relationships:", relationships.len());
					for rel in relationships {
						let other_id = if rel.source_id == *memory_id {
							&rel.target_id
						} else {
							&rel.source_id
						};
						println!(
							"- {} {} (strength: {:.2})",
							rel.relationship_type, other_id, rel.strength
						);
					}
				}
				_ => {
					println!("🔗 {} relationships:\n", relationships.len());
					for rel in relationships {
						println!("Relationship ID: {}", rel.id);
						println!("Type: {}", rel.relationship_type);
						println!("Source: {}", rel.source_id);
						println!("Target: {}", rel.target_id);
						println!("Strength: {:.2}", rel.strength);
						println!("Description: {}", rel.description);
						println!("Created: {}", rel.created_at.format("%Y-%m-%d %H:%M:%S"));
						println!();
					}
				}
			}
		}

		MemoryCommand::Related { memory_id, format } => {
			let related_memories = memory_manager.get_related_memories(memory_id).await?;

			if related_memories.is_empty() {
				println!("❌ No related memories found for memory '{}'.", memory_id);
				return Ok(());
			}

			format_memories(&related_memories, format);
		}
	}

	Ok(())
}

fn format_memories(memories: &[octocode::memory::Memory], format: &str) {
	match format {
		"json" => {
			println!("{}", serde_json::to_string_pretty(memories).unwrap());
		}
		"compact" => {
			println!("🧠 {} memories:", memories.len());
			for memory in memories {
				println!(
					"- [{}] {} - {}",
					memory.memory_type, memory.title, memory.id
				);
			}
		}
		_ => {
			println!("🧠 {} memories:\n", memories.len());
			for memory in memories {
				println!("Memory ID: {}", memory.id);
				println!("Title: {}", memory.title);
				println!("Type: {}", memory.memory_type);
				println!("Importance: {:.2}", memory.metadata.importance);
				println!("Created: {}", memory.created_at.format("%Y-%m-%d %H:%M:%S"));
				if !memory.metadata.tags.is_empty() {
					println!("Tags: {}", memory.metadata.tags.join(", "));
				}
				println!("Content: {}", memory.content);
				println!();
			}
		}
	}
}

fn format_search_results(results: &[octocode::memory::MemorySearchResult], format: &str) {
	// Use shared formatting function
	octocode::memory::format_memories_for_cli(results, format);
}
