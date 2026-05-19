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

//! Application-wide constants

/// Maximum number of queries allowed in multi-query operations
pub const MAX_QUERIES: usize = 10;

/// Embedding input type prefixes for manual injection (non-API providers)
pub const QUERY_PREFIX: &str = "Represent the query for retrieving supporting documents: ";
pub const DOCUMENT_PREFIX: &str = "Represent the document for retrieval: ";
