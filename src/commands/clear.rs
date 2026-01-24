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

use clap::Args;
use octocode::store::Store;

#[derive(Args, Debug)]
pub struct ClearArgs {
	/// Clear mode: all (default), code, docs, or text
	#[arg(long, default_value = "all")]
	pub mode: String,
}

/// Clear database tables based on mode
pub async fn execute(store: &Store, args: &ClearArgs) -> Result<(), anyhow::Error> {
	match args.mode.as_str() {
		"all" => {
			println!("Clearing all database tables...");
			store.clear_all_tables().await?;
			println!("Successfully dropped all tables and schemas.");
			println!(
				"Note: Tables will be recreated with current schema on next indexing operation."
			);
		}
		"code" => {
			println!("Clearing code blocks table...");
			store.clear_code_table().await?;
			store.clear_git_metadata().await?;
			println!("Successfully cleared code blocks table and git metadata.");
			println!("Note: Code content will be re-indexed on next indexing operation.");
		}
		"docs" => {
			println!("Clearing document blocks table...");
			store.clear_docs_table().await?;
			store.clear_git_metadata().await?;
			println!("Successfully cleared document blocks table and git metadata.");
			println!("Note: Documentation content will be re-indexed on next indexing operation.");
		}
		"text" => {
			println!("Clearing text blocks table...");
			store.clear_text_table().await?;
			store.clear_git_metadata().await?;
			println!("Successfully cleared text blocks table and git metadata.");
			println!("Note: Text content will be re-indexed on next indexing operation.");
		}
		_ => {
			return Err(anyhow::anyhow!(
				"Invalid mode '{}'. Valid modes are: all, code, docs, text",
				args.mode
			));
		}
	}
	Ok(())
}
