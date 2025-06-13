// Copyright 2025 Muvon Un Limited
// Licensed under the Apache License, Version 2.0

use clap::ValueEnum;

/// Output format for command results
#[derive(ValueEnum, Clone, Debug, Default)]
pub enum OutputFormat {
	/// CLI format (default) - human-readable terminal output
	#[default]
	Cli,
	/// JSON format - structured data output
	Json,
	/// Markdown format - documentation-friendly output
	Md,
	/// Text format - token-efficient plain text output
	Text,
}

impl OutputFormat {
	/// Check if this is JSON format
	pub fn is_json(&self) -> bool {
		matches!(self, OutputFormat::Json)
	}

	/// Check if this is Markdown format
	pub fn is_md(&self) -> bool {
		matches!(self, OutputFormat::Md)
	}

	/// Check if this is Text format
	pub fn is_text(&self) -> bool {
		matches!(self, OutputFormat::Text)
	}
}
