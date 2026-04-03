// Copyright 2026 Muvon Un Limited
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

use clap::Args;

use octocode::config::Config;
use octocode::indexer::git_utils::GitUtils;
use octocode::store::Store;

use crate::commands::OutputFormat;

#[derive(Debug, Args)]
pub struct StatsArgs {
	/// Output format: 'cli', 'json', or 'text'
	#[arg(short, long, default_value = "cli")]
	pub format: OutputFormat,
}

pub async fn execute(
	store: &Store,
	args: &StatsArgs,
	config: &Config,
) -> Result<(), anyhow::Error> {
	let current_dir = std::env::current_dir()?;

	// Gather all stats in parallel
	let (
		file_count,
		code_count,
		text_count,
		doc_count,
		commit_count,
		graphrag_nodes,
		graphrag_rels,
		last_commit,
	) = tokio::try_join!(
		async { store.get_all_indexed_file_paths().await.map(|f| f.len()) },
		store.get_table_row_count("code_blocks"),
		store.get_table_row_count("text_blocks"),
		store.get_table_row_count("document_blocks"),
		store.get_table_row_count("commit_blocks"),
		store.get_table_row_count("graphrag_nodes"),
		store.get_table_row_count("graphrag_relationships"),
		store.get_last_commit_hash(),
	)?;

	// Get current HEAD for staleness check
	let current_head = GitUtils::get_current_commit_hash(&current_dir).ok();
	let is_stale = match (&last_commit, &current_head) {
		(Some(last), Some(head)) => last != head,
		(None, Some(_)) => true,
		_ => false,
	};

	// Count commits behind if stale
	let commits_behind = if is_stale {
		if let Some(ref last) = last_commit {
			GitUtils::get_changed_files_since_commit(&current_dir, last)
				.ok()
				.map(|_| {
					// Count commits between last indexed and HEAD
					std::process::Command::new("git")
						.args(["rev-list", "--count", &format!("{}..HEAD", last)])
						.current_dir(&current_dir)
						.output()
						.ok()
						.and_then(|o| String::from_utf8(o.stdout).ok())
						.and_then(|s| s.trim().parse::<usize>().ok())
						.unwrap_or(0)
				})
				.unwrap_or(0)
		} else {
			0
		}
	} else {
		0
	};

	if args.format.is_json() {
		let json = serde_json::json!({
			"files_indexed": file_count,
			"code_blocks": code_count,
			"text_blocks": text_count,
			"document_blocks": doc_count,
			"commit_blocks": commit_count,
			"graphrag_nodes": graphrag_nodes,
			"graphrag_relationships": graphrag_rels,
			"last_indexed_commit": last_commit,
			"current_head": current_head,
			"stale": is_stale,
			"commits_behind": commits_behind,
			"config": {
				"code_model": config.embedding.code_model,
				"text_model": config.embedding.text_model,
				"similarity_threshold": config.search.similarity_threshold,
				"reranker_enabled": config.search.reranker.enabled,
				"reranker_model": config.search.reranker.model,
				"contextual_enabled": config.index.contextual_descriptions,
				"contextual_model": config.index.contextual_model,
				"graphrag_llm": config.graphrag.use_llm,
			}
		});
		println!("{}", serde_json::to_string_pretty(&json)?);
		return Ok(());
	}

	// CLI / text format
	let short_hash = |h: &str| h.get(..7).unwrap_or(h).to_string();

	println!("Index Status");
	println!("  Files indexed:        {}", file_count);
	println!("  Code blocks:          {}", code_count);
	println!("  Text blocks:          {}", text_count);
	println!("  Document blocks:      {}", doc_count);
	println!("  Commit blocks:        {}", commit_count);

	if graphrag_nodes > 0 || graphrag_rels > 0 {
		println!("  GraphRAG nodes:       {}", graphrag_nodes);
		println!("  GraphRAG relations:   {}", graphrag_rels);
	}

	println!();

	match (&last_commit, &current_head) {
		(Some(last), Some(head)) => {
			println!("  Last indexed commit:  {}", short_hash(last));
			println!("  Current HEAD:         {}", short_hash(head));
			if is_stale {
				println!(
					"  Status:               Stale -- {} commit{} behind",
					commits_behind,
					if commits_behind == 1 { "" } else { "s" }
				);
			} else {
				println!("  Status:               Up to date");
			}
		}
		(None, Some(head)) => {
			println!("  Last indexed commit:  (none)");
			println!("  Current HEAD:         {}", short_hash(head));
			println!("  Status:               Not indexed");
		}
		_ => {
			println!("  Status:               No git repository");
		}
	}

	println!();
	println!("Configuration");
	println!("  Code model:           {}", config.embedding.code_model);
	println!("  Text model:           {}", config.embedding.text_model);
	println!(
		"  Threshold:            {}",
		config.search.similarity_threshold
	);

	if config.search.reranker.enabled {
		println!(
			"  Reranker:             {} (enabled)",
			config.search.reranker.model
		);
	} else {
		println!("  Reranker:             disabled");
	}

	if config.index.contextual_descriptions {
		println!(
			"  Contextual:           {} (enabled)",
			config.index.contextual_model
		);
	} else {
		println!("  Contextual:           disabled");
	}

	if config.graphrag.use_llm {
		println!("  GraphRAG LLM:         enabled");
	} else {
		println!("  GraphRAG LLM:         disabled");
	}

	Ok(())
}
