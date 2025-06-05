use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tracing::{debug, error, info, trace, warn};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt::Layer, prelude::*, registry::Registry, EnvFilter};

static MCP_LOG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Initialize logging for MCP server with file rotation
pub fn init_mcp_logging(base_dir: PathBuf, debug_mode: bool) -> Result<(), anyhow::Error> {
	// Use the system-wide storage directory for logs
	let project_storage = crate::storage::get_project_storage_path(&base_dir)?;
	let log_dir = project_storage.join("logs");
	std::fs::create_dir_all(&log_dir)?;

	// Store log directory for potential future use
	MCP_LOG_DIR
		.set(log_dir.clone())
		.map_err(|_| anyhow::anyhow!("Failed to set log directory"))?;

	// Cross-platform way to create a "latest" indicator
	let latest_file = project_storage.join("latest_log.txt");
	std::fs::write(&latest_file, log_dir.to_string_lossy().as_bytes()).unwrap_or_else(|e| {
		eprintln!("Warning: Could not create latest log indicator: {}", e);
	});

	// Create rotating file appender
	let file_appender = RollingFileAppender::new(Rotation::DAILY, &log_dir, "mcp_server.log");

	// Set up environment filter with sensible defaults
	let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
		if debug_mode {
			// In debug mode, show debug level for our crate, but info for others
			EnvFilter::new("info,octocode=debug")
		} else {
			// In production mode, only show info and above, with warnings for file processing
			EnvFilter::new("info,octocode::mcp::logging=info")
		}
	});

	// File layer with JSON formatting for structured logs
	let file_layer = Layer::new()
		.with_writer(file_appender)
		.with_ansi(false)
		.with_target(true)
		.with_file(true)
		.with_line_number(true)
		.with_thread_ids(true)
		.with_thread_names(true)
		.json();

	// Console layer (only in debug mode)
	let console_layer = if debug_mode {
		Some(
			Layer::new()
				.with_writer(std::io::stderr)
				.with_ansi(true)
				.with_target(false)
				.with_thread_ids(false)
				.with_thread_names(false),
		)
	} else {
		None
	};

	// Create registry with layers
	let registry = Registry::default().with(file_layer).with(env_filter);

	if let Some(console) = console_layer {
		registry.with(console).init();
	} else {
		registry.init();
	}

	info!(
		project_path = %base_dir.display(),
		log_directory = %log_dir.display(),
		debug_mode = debug_mode,
		"MCP Server logging initialized"
	);

	Ok(())
}

/// Log MCP server request details with reduced verbosity
pub fn log_mcp_request(
	method: &str,
	params: Option<&serde_json::Value>,
	request_id: Option<&serde_json::Value>,
) {
	// Extract key parameters for common methods without logging full params
	let key_info = match method {
		"tools/call" => params
			.and_then(|p| p.get("name"))
			.and_then(|v| v.as_str())
			.map(|tool| format!("tool={}", tool)),
		"initialize" => Some("client_init".to_string()),
		"tools/list" => Some("list_tools".to_string()),
		_ => None,
	};

	info!(
		method = method,
		request_id = ?request_id,
		params_size = params.map(|p| p.to_string().len()).unwrap_or(0),
		key_info = key_info,
		"MCP Request received"
	);

	// Only log full params in trace level for deep debugging
	if let Some(params) = params {
		trace!(
			method = method,
			params = %params,
			"MCP Request full parameters"
		);
	}
}

/// Log MCP server response
pub fn log_mcp_response(
	method: &str,
	success: bool,
	request_id: Option<&serde_json::Value>,
	duration_ms: Option<u64>,
) {
	if success {
		info!(
			method = method,
			request_id = ?request_id,
			duration_ms = duration_ms,
			"MCP Request processed successfully"
		);
	} else {
		warn!(
			method = method,
			request_id = ?request_id,
			duration_ms = duration_ms,
			"MCP Request processing failed"
		);
	}
}

/// Log critical errors with context
pub fn log_critical_error(context: &str, error: &dyn std::error::Error) {
	error!(
		context = context,
		error = %error,
		error_chain = ?error.source(),
		"Critical error in MCP server"
	);
}

/// Log critical errors from anyhow::Error
pub fn log_critical_anyhow_error(context: &str, error: &anyhow::Error) {
	error!(
		context = context,
		error = %error,
		error_chain = ?error.source(),
		"Critical error in MCP server"
	);
}

/// Log file watcher events with better context
pub fn log_watcher_event(event_type: &str, path: Option<&std::path::Path>, count: usize) {
	let path_info = path.map(|p| p.display().to_string());

	match event_type {
		"file_change_batch" if count > 5 => {
			info!(
				event_type = event_type,
				event_count = count,
				"File watcher detected batch changes"
			);
		}
		"file_change" if count % 10 == 0 => {
			debug!(
				event_type = event_type,
				accumulated_events = count,
				"File watcher accumulating changes"
			);
		}
		"debounce_trigger" => {
			info!(
				event_type = event_type,
				trigger_count = count,
				"File watcher triggering reindex"
			);
		}
		_ => {
			trace!(
				event_type = event_type,
				path = path_info,
				event_count = count,
				"File watcher event"
			);
		}
	}
}

/// Log indexing operations with enhanced metrics
pub fn log_indexing_operation(
	operation: &str,
	file_count: Option<usize>,
	duration_ms: Option<u64>,
	success: bool,
) {
	let performance_tier = duration_ms.map(|d| match d {
		0..=1000 => "fast",
		1001..=5000 => "normal",
		5001..=15000 => "slow",
		_ => "very_slow",
	});

	if success {
		info!(
			operation = operation,
			file_count = file_count,
			duration_ms = duration_ms,
			performance_tier = performance_tier,
			"Indexing operation completed"
		);
	} else {
		error!(
			operation = operation,
			file_count = file_count,
			duration_ms = duration_ms,
			performance_tier = performance_tier,
			"Indexing operation failed"
		);
	}
}

/// Log detailed indexing progress with file-level metrics
pub fn log_indexing_progress(
	phase: &str,
	files_processed: usize,
	total_files: usize,
	current_file: Option<&str>,
	embedding_calls: usize,
) {
	let progress_percent = if total_files > 0 {
		(files_processed as f64 / total_files as f64 * 100.0) as u32
	} else {
		0
	};

	match phase {
		"file_processing" if files_processed % 50 == 0 || files_processed == total_files => {
			info!(
				phase = phase,
				files_processed = files_processed,
				total_files = total_files,
				progress_percent = progress_percent,
				embedding_calls = embedding_calls,
				"Indexing progress update"
			);
		}
		"cleanup" | "git_optimization" | "graphrag_build" => {
			info!(
				phase = phase,
				files_processed = files_processed,
				total_files = total_files,
				current_file = current_file,
				"Indexing phase started"
			);
		}
		_ => {
			debug!(
				phase = phase,
				files_processed = files_processed,
				total_files = total_files,
				current_file = current_file,
				"Indexing phase update"
			);
		}
	}
}

/// Log file processing errors with context
pub fn log_file_processing_error(file_path: &str, operation: &str, error: &dyn std::error::Error) {
	warn!(
		file_path = file_path,
		operation = operation,
		error = %error,
		error_source = ?error.source(),
		"File processing error (non-critical)"
	);
}

/// Log performance metrics for operations
pub fn log_performance_metrics(
	operation: &str,
	duration_ms: u64,
	items_processed: usize,
	memory_usage_mb: Option<f64>,
) {
	let throughput = if duration_ms > 0 {
		(items_processed as f64 / (duration_ms as f64 / 1000.0)) as u32
	} else {
		0
	};

	info!(
		operation = operation,
		duration_ms = duration_ms,
		items_processed = items_processed,
		throughput_per_sec = throughput,
		memory_usage_mb = memory_usage_mb,
		"Performance metrics"
	);
}

/// Log git operations for debugging
pub fn log_git_operation(
	operation: &str,
	repo_path: &str,
	files_affected: Option<usize>,
	success: bool,
) {
	if success {
		debug!(
			operation = operation,
			repo_path = repo_path,
			files_affected = files_affected,
			"Git operation completed"
		);
	} else {
		warn!(
			operation = operation,
			repo_path = repo_path,
			files_affected = files_affected,
			"Git operation failed"
		);
	}
}

/// Get the current log directory
/// Get the current log directory
pub fn get_log_directory() -> Option<PathBuf> {
	MCP_LOG_DIR.get().cloned()
}

/// Get all log directories for MCP server
pub fn get_all_log_directories(base_dir: &std::path::Path) -> Result<Vec<PathBuf>, std::io::Error> {
	let project_storage =
		crate::storage::get_project_storage_path(base_dir).map_err(std::io::Error::other)?;
	let logs_dir = project_storage.join("logs");

	if !logs_dir.exists() {
		return Ok(Vec::new());
	}

	// For the new structure, we just return the single logs directory
	// since each project has its own storage directory
	Ok(vec![logs_dir])
}

/// Print log directory information
pub fn print_log_directories(base_dir: &Path) -> Result<(), std::io::Error> {
	let project_storage =
		crate::storage::get_project_storage_path(base_dir).map_err(std::io::Error::other)?;
	let logs_dir = project_storage.join("logs");

	println!("MCP Server Log Directory:");
	if logs_dir.exists() {
		println!("  {}", logs_dir.display());

		// Show log files in the directory
		let mut log_files: Vec<_> = std::fs::read_dir(&logs_dir)?
			.filter_map(|entry| {
				let path = entry.ok()?.path();
				if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("log") {
					Some(path)
				} else {
					None
				}
			})
			.collect();

		if !log_files.is_empty() {
			log_files.sort_by(|a, b| {
				let a_time = a
					.metadata()
					.and_then(|m| m.modified())
					.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
				let b_time = b
					.metadata()
					.and_then(|m| m.modified())
					.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
				b_time.cmp(&a_time)
			});

			println!("  Log files:");
			for log_file in log_files {
				println!(
					"    - {}",
					log_file.file_name().unwrap_or_default().to_string_lossy()
				);
			}
		} else {
			println!("    (no log files)");
		}
	} else {
		println!("  (directory does not exist)");
	}
	Ok(())
}
