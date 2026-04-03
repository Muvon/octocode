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

use clap::{Args, Subcommand};
use octocode::indexer;

#[derive(Args, Debug)]
pub struct BranchArgs {
	#[command(subcommand)]
	pub command: BranchCommand,
}

#[derive(Subcommand, Debug)]
pub enum BranchCommand {
	/// List all indexed branch deltas
	List,

	/// Show info about a branch delta index
	Info {
		/// Branch name (defaults to current branch)
		name: Option<String>,
	},

	/// Delete a branch delta index
	Delete {
		/// Branch name to delete
		name: String,
	},

	/// Remove indexes for branches that no longer exist in git or are merged
	Prune {
		/// Show what would be pruned without actually deleting
		#[arg(long)]
		dry_run: bool,
	},
}

pub async fn execute(args: &BranchArgs) -> Result<(), anyhow::Error> {
	let current_dir = std::env::current_dir()?;

	match &args.command {
		BranchCommand::List => {
			let manifests = indexer::branch::list_indexed_branches(&current_dir)?;
			if manifests.is_empty() {
				println!("No branch delta indexes found.");
				println!("Index a branch by checking it out and running 'octocode index'.");
				return Ok(());
			}

			println!("Indexed branch deltas:\n");
			for manifest in &manifests {
				let age = chrono::Utc::now().timestamp() - manifest.indexed_at;
				let age_str = if age < 3600 {
					format!("{}m ago", age / 60)
				} else if age < 86400 {
					format!("{}h ago", age / 3600)
				} else {
					format!("{}d ago", age / 86400)
				};

				println!(
					"  {} ({} changed, {} deleted) [base: {}, indexed {}]",
					manifest.branch_name,
					manifest.changed_paths.len(),
					manifest.deleted_paths.len(),
					manifest.base_branch,
					age_str,
				);
			}
			println!("\n{} branch(es) indexed.", manifests.len());
		}

		BranchCommand::Info { name } => {
			let branch_name = match name {
				Some(n) => n.clone(),
				None => indexer::branch::get_current_branch(&current_dir).ok_or_else(|| {
					anyhow::anyhow!("Not on a branch (detached HEAD?). Specify a branch name.")
				})?,
			};

			let (_, manifest) = indexer::branch::resolve_branch_state(&current_dir, &branch_name)?;

			match manifest {
				Some(m) => {
					println!("Branch: {}", m.branch_name);
					println!("Base branch: {}", m.base_branch);
					println!(
						"Base commit: {}",
						&m.base_commit[..12.min(m.base_commit.len())]
					);
					println!(
						"Branch commit: {}",
						&m.branch_commit[..12.min(m.branch_commit.len())]
					);
					println!("Changed files: {}", m.changed_paths.len());
					for path in &m.changed_paths {
						println!("  M {}", path);
					}
					if !m.deleted_paths.is_empty() {
						println!("Deleted files: {}", m.deleted_paths.len());
						for path in &m.deleted_paths {
							println!("  D {}", path);
						}
					}
					let dt = chrono::DateTime::from_timestamp(m.indexed_at, 0)
						.map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string())
						.unwrap_or_else(|| "unknown".to_string());
					println!("Indexed at: {}", dt);
				}
				None => {
					println!("No delta index found for branch '{}'.", branch_name);
					println!("Check out the branch and run 'octocode index' to create one.");
				}
			}
		}

		BranchCommand::Delete { name } => {
			indexer::branch::delete_branch_index(&current_dir, name)?;
			println!("Deleted branch delta index for '{}'.", name);
		}

		BranchCommand::Prune { dry_run } => {
			if !indexer::git_utils::GitUtils::is_git_repo_root(&current_dir) {
				return Err(anyhow::anyhow!("Not in a git repository root."));
			}

			let pruned = indexer::branch::prune_branches(&current_dir, &current_dir, *dry_run)?;

			if pruned.is_empty() {
				println!("No stale branch indexes to prune.");
			} else if *dry_run {
				println!("Would prune {} branch index(es):", pruned.len());
				for name in &pruned {
					println!("  {}", name);
				}
				println!("\nRun without --dry-run to actually delete.");
			} else {
				println!("Pruned {} branch index(es):", pruned.len());
				for name in &pruned {
					println!("  {}", name);
				}
			}
		}
	}

	Ok(())
}
