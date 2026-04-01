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

//! Intelligent vector index optimization for LanceDB
//!
//! This module automatically tunes vector index parameters based on:
//! - Dataset size and characteristics
//! - Vector dimensions
//! - LanceDB best practices
//!
//! ## Index Type Choice
//!
//! We support two index types:
//!
//! ### IVF_HNSW_SQ (Scalar Quantization) - Default when quantization=false
//! - Best recall/latency trade-off according to LanceDB documentation
//! - HNSW provides fast approximate nearest neighbor search
//! - Scalar Quantization (SQ) compresses vectors to 8-bit (4x compression for float32)
//! - Simpler than IVF_PQ (no sub-vector tuning needed)
//!
//! ### IVF_RQ (RaBitQ Quantization) - Default when quantization=true
//! - Better storage efficiency with 32x compression
//! - Uses 1-bit quantization for vectors
//! - Optimal for large-scale semantic search
//! - Maintains good recall while significantly reducing storage footprint
//!
//! ## Parameter Guidelines
//!
//! - `num_partitions`: For IVF indexes, LanceDB recommends `num_rows // 1,048,576`
//!   - For small datasets (< 1M rows), use sqrt(num_rows) as fallback
//!   - Minimum 2 partitions to avoid single-partition degradation
//! - `num_edges` (M): HNSW graph connectivity, default 20 is good for most cases
//! - `ef_construction`: HNSW build quality, default 300 provides good accuracy

use lancedb::Table;
use lancedb::{query::VectorQuery, DistanceType};

/// Vector index type selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexType {
	/// IVF_HNSW_SQ: Inverted File with HNSW and Scalar Quantization (4x compression)
	IvfHnswSq,
	/// IVF_RQ: Inverted File with RaBitQ Quantization (32x compression)
	IvfRq,
}

/// Vector index optimization parameters automatically calculated from dataset characteristics
#[derive(Debug, Clone)]
pub struct VectorIndexParams {
	pub should_create_index: bool,
	pub index_type: IndexType,
	pub num_partitions: u32,
	pub num_edges: u32,       // HNSW M parameter
	pub ef_construction: u32, // HNSW build quality
	pub distance_type: DistanceType,
}

/// Search optimization parameters for vector queries
#[derive(Debug, Clone)]
pub struct SearchParams {
	pub ef: Option<u32>, // HNSW search parameter (optional, LanceDB auto-tunes)
}

/// Intelligent vector index optimizer
pub struct VectorOptimizer;

impl VectorOptimizer {
	/// Apply intelligent search optimization to a vector query
	///
	/// For IVF_HNSW_SQ indexes, LanceDB handles search optimization automatically.
	/// The `ef` parameter can be tuned for recall/latency trade-off, but defaults work well.
	///
	/// # Arguments
	/// * `query` - The vector query to optimize
	/// * `table` - The LanceDB table for index inspection
	/// * `table_name` - Name for logging purposes
	///
	/// # Returns
	/// The optimized query (currently passes through as LanceDB auto-tunes HNSW)
	pub async fn optimize_query(
		query: VectorQuery,
		table: &Table,
		table_name: &str,
	) -> Result<VectorQuery, lancedb::Error> {
		// Get table statistics
		let row_count = table.count_rows(None).await?;
		let indices = table.list_indices().await?;
		let has_index = indices.iter().any(|idx| idx.columns == vec!["embedding"]);

		if has_index {
			tracing::debug!(
				"Vector search on {} with {} rows, index exists (LanceDB auto-tunes HNSW)",
				table_name,
				row_count
			);
		} else {
			tracing::debug!(
				"Vector search on {} with {} rows, no index (brute force)",
				table_name,
				row_count
			);
		}

		Ok(query)
	}

	/// Calculate optimal index parameters based on dataset characteristics
	///
	/// Based on LanceDB documentation and best practices:
	/// - For datasets < 1000 rows: No index (brute force is faster)
	/// - For datasets >= 1000 rows: Create IVF_HNSW_SQ index
	///
	/// # Arguments
	/// * `row_count` - Number of rows in the dataset
	/// * `vector_dimension` - Dimension of the vectors (unused for HNSW, kept for API compatibility)
	///
	/// # Returns
	/// Optimized index parameters or recommendation to skip indexing
	pub fn calculate_index_params(
		row_count: usize,
		_vector_dimension: usize,
		use_quantization: bool,
	) -> VectorIndexParams {
		// LanceDB performs excellently with brute force search up to ~100K rows
		// For smaller datasets, indexing overhead outweighs benefits
		if row_count < 1000 {
			tracing::debug!(
				"Dataset size {} is small, skipping index creation (brute force will be faster)",
				row_count
			);
			return VectorIndexParams {
				should_create_index: false,
				index_type: if use_quantization {
					IndexType::IvfRq
				} else {
					IndexType::IvfHnswSq
				},
				num_partitions: 0,
				num_edges: 20,
				ef_construction: 300,
				distance_type: DistanceType::Cosine,
			};
		}

		// Calculate optimal number of partitions for IVF indexes
		// LanceDB recommends: num_rows // 1,048,576 for large datasets
		// For smaller datasets, use sqrt(num_rows) as a reasonable default
		let num_partitions = if row_count >= 1_048_576 {
			// Large dataset: use LanceDB recommended formula
			(row_count / 1_048_576) as u32
		} else {
			// Small to medium dataset: use sqrt rule
			// Ensure minimum of 2 partitions to avoid single-partition issues
			std::cmp::max((row_count as f64).sqrt() as u32, 2)
		};

		// Clamp partitions to reasonable bounds
		let num_partitions = num_partitions.clamp(2, 1024);

		let index_type = if use_quantization {
			IndexType::IvfRq
		} else {
			IndexType::IvfHnswSq
		};

		let index_type_name = match index_type {
			IndexType::IvfHnswSq => "IVF_HNSW_SQ",
			IndexType::IvfRq => "IVF_RQ",
		};

		tracing::debug!(
			"Calculated {} index params for {} rows: partitions={}, num_edges=20, ef_construction=300",
			index_type_name,
			row_count,
			num_partitions
		);

		VectorIndexParams {
			should_create_index: true,
			index_type,
			num_partitions,
			num_edges: 20,                       // Default M for HNSW - good balance
			ef_construction: 300,                // Default ef for build quality
			distance_type: DistanceType::Cosine, // Cosine is best for semantic similarity
		}
	}

	/// Calculate optimal search parameters based on index characteristics
	///
	/// For IVF_HNSW_SQ, LanceDB handles search optimization automatically.
	/// The `ef` parameter can be tuned, but defaults work well for most cases.
	///
	/// # Arguments
	/// * `num_partitions` - Number of partitions in the index
	/// * `row_count` - Total number of rows
	///
	/// # Returns
	/// Optimized search parameters (currently returns defaults)
	pub fn calculate_search_params(_num_partitions: u32, _row_count: usize) -> SearchParams {
		// For IVF_HNSW_SQ, LanceDB auto-tunes the search parameters
		// We could optionally tune `ef` for recall/latency trade-off, but defaults work well
		SearchParams {
			ef: None, // Let LanceDB use its defaults
		}
	}

	/// Determine if index should be recreated based on current parameters vs optimal
	///
	/// # Arguments
	/// * `current_partitions` - Current number of partitions
	/// * `optimal` - Optimal parameters for current dataset
	///
	/// # Returns
	/// True if index should be recreated for better performance
	pub fn should_recreate_index(current_partitions: u32, optimal: &VectorIndexParams) -> bool {
		if !optimal.should_create_index {
			return false;
		}

		// Recreate if partition count is significantly different (> 50%)
		let partition_diff = (current_partitions as f32 - optimal.num_partitions as f32).abs()
			/ optimal.num_partitions as f32;

		// Recreate if difference is > 50%
		partition_diff > 0.5
	}

	/// Check if index needs optimization based on dataset growth
	///
	/// # Arguments
	/// * `current_rows` - Current number of rows
	/// * `indexed_rows` - Number of rows when index was created
	///
	/// # Returns
	/// True if index should be recreated due to significant growth
	pub fn needs_reindex(current_rows: usize, indexed_rows: usize) -> bool {
		if indexed_rows == 0 {
			return false;
		}

		// Calculate growth percentage
		let growth = (current_rows as f64 - indexed_rows as f64) / indexed_rows as f64;

		// Recreate index if dataset has grown by more than 50%
		// This ensures optimal partition count as data grows
		growth > 0.5
	}

	/// Check if index should be optimized based on dataset growth
	///
	/// This is a convenience method that combines growth detection with
	/// index parameter validation. Used for incremental optimization.
	///
	/// # Arguments
	/// * `current_rows` - Current number of rows in the table
	/// * `_vector_dim` - Vector dimension (unused, kept for API compatibility)
	/// * `check_growth` - Whether to check for growth (if false, returns false)
	///
	/// # Returns
	/// True if the index should be recreated for better performance
	pub fn should_optimize_for_growth(
		_current_rows: usize,
		_vector_dim: usize,
		check_growth: bool,
	) -> bool {
		if !check_growth {
			return false;
		}

		// For IVF_HNSW_SQ, we should recreate the index when:
		// 1. Dataset has grown significantly (more than 2x since last optimization)
		// 2. Or when crossing size thresholds (e.g., from 1K to 10K, 10K to 100K)
		//
		// Since we don't track when the index was created, we use heuristics:
		// - Small datasets (< 10K): recreate at 2x growth
		// - Medium datasets (10K-100K): recreate at 1.5x growth
		// - Large datasets (> 100K): recreate at 1.25x growth

		// This is a simplified check - in production, you'd want to track
		// the row count when the index was created and compare
		// For now, we return false and rely on manual optimization calls
		false
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_small_dataset_no_index() {
		let params = VectorOptimizer::calculate_index_params(500, 768, true);
		assert!(!params.should_create_index);
	}

	#[test]
	fn test_medium_dataset_creates_index() {
		let params = VectorOptimizer::calculate_index_params(5000, 768, true);
		assert!(params.should_create_index);
		assert!(params.num_partitions >= 2);
	}

	#[test]
	fn test_large_dataset_more_partitions() {
		let params_small = VectorOptimizer::calculate_index_params(5000, 768, true);
		let params_large = VectorOptimizer::calculate_index_params(50000, 768, true);

		assert!(params_large.num_partitions > params_small.num_partitions);
	}

	#[test]
	fn test_very_large_dataset_formula() {
		// For datasets >= 1M rows, use LanceDB recommended formula
		let params = VectorOptimizer::calculate_index_params(2_000_000, 768, true);
		assert!(params.should_create_index);
		// 2_000_000 / 1_048_576 ≈ 1.9, so should be ~2 partitions
		assert_eq!(params.num_partitions, 2);
	}

	#[test]
	fn test_minimum_partitions() {
		// Even small indexed datasets should have at least 2 partitions
		let params = VectorOptimizer::calculate_index_params(1000, 768, true);
		assert!(params.num_partitions >= 2);
	}

	#[test]
	fn test_should_recreate_index() {
		let optimal = VectorIndexParams {
			should_create_index: true,
			index_type: IndexType::IvfHnswSq,
			num_partitions: 100,
			num_edges: 20,
			ef_construction: 300,
			distance_type: DistanceType::Cosine,
		};

		// Small difference - don't recreate
		assert!(!VectorOptimizer::should_recreate_index(80, &optimal));

		// Large difference - recreate
		assert!(VectorOptimizer::should_recreate_index(10, &optimal));
	}

	#[test]
	fn test_quantization_false_uses_ivf_hnsw_sq() {
		let params = VectorOptimizer::calculate_index_params(5000, 768, false);
		assert!(params.should_create_index);
		assert_eq!(params.index_type, IndexType::IvfHnswSq);
	}

	#[test]
	fn test_quantization_true_uses_ivf_rq() {
		let params = VectorOptimizer::calculate_index_params(5000, 768, true);
		assert!(params.should_create_index);
		assert_eq!(params.index_type, IndexType::IvfRq);
	}

	#[test]
	fn test_needs_reindex() {
		// Small growth - don't reindex
		assert!(!VectorOptimizer::needs_reindex(1500, 1000));

		// Large growth - reindex
		assert!(VectorOptimizer::needs_reindex(2000, 1000));

		// No growth
		assert!(!VectorOptimizer::needs_reindex(1000, 1000));
	}
}
