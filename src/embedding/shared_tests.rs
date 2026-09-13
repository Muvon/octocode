// Copyright 2026 Muvon Un Limited
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//	   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use anyhow::Result;
use octolib::embedding::{EmbeddingProvider, EmbeddingUsage};

use super::super::InputType;
use super::{endpoint_path, join_in, read_endpoint, BuildInner, Endpoint, SharedProvider};

struct FakeProvider;

#[async_trait::async_trait]
impl EmbeddingProvider for FakeProvider {
	async fn generate_embedding(&self, text: &str) -> Result<(Vec<f32>, EmbeddingUsage)> {
		Ok((
			vec![text.len() as f32, 1.0],
			EmbeddingUsage {
				input_tokens: 3,
				cost: None,
			},
		))
	}

	async fn generate_embeddings_batch(
		&self,
		texts: Vec<String>,
		_input_type: InputType,
	) -> Result<(Vec<Vec<f32>>, EmbeddingUsage)> {
		Ok((
			texts.iter().map(|t| vec![t.len() as f32, 2.0]).collect(),
			EmbeddingUsage {
				input_tokens: 7,
				cost: None,
			},
		))
	}

	fn get_dimension(&self) -> usize {
		2
	}

	async fn model_revision(&self) -> Result<Option<String>> {
		Ok(Some("fakerev".to_string()))
	}
}

fn fake_build() -> BuildInner {
	Box::new(move || Box::pin(async { Ok(Box::new(FakeProvider) as Box<dyn EmbeddingProvider>) }))
}

#[tokio::test]
async fn second_participant_becomes_a_client_of_the_owner() {
	let dir = tempfile::tempdir().unwrap();
	let key = format!("test-shared-{}", std::process::id());
	let first = join_in(dir.path(), &key, fake_build())
		.await
		.expect("owner elected");
	let second = join_in(dir.path(), &key, fake_build())
		.await
		.expect("client attaches");
	assert!(matches!(second, SharedProvider::Client { .. }));

	// Client round-trips through the owner's FakeProvider.
	let (vector, usage) = second
		.generate_embedding("abcd")
		.await
		.expect("remote embed");
	assert_eq!(vector, vec![4.0, 1.0]);
	assert_eq!(usage.input_tokens, 3);

	let (vectors, _) = second
		.generate_embeddings_batch(vec!["ab".to_string(), "abc".to_string()], InputType::Query)
		.await
		.expect("remote batch");
	assert_eq!(vectors, vec![vec![2.0, 2.0], vec![3.0, 2.0]]);

	// Dimension and revision come from the handshake, not local weights.
	assert_eq!(second.get_dimension(), 2);
	assert_eq!(
		second.model_revision().await.unwrap(),
		Some("fakerev".to_string())
	);

	// Owner answers locally, identically.
	let (local, _) = first.generate_embedding("abcd").await.expect("owner embed");
	assert_eq!(local, vector);
}

#[tokio::test]
async fn stale_endpoint_is_replaced() {
	let dir = tempfile::tempdir().unwrap();
	let key = format!("test-stale-{}", std::process::id());
	let path = endpoint_path(dir.path(), &key).unwrap();
	let dead = Endpoint {
		port: 1,
		token: "stale".to_string(),
		model: key.clone(),
		pid: 0,
	};
	std::fs::write(&path, serde_json::to_string(&dead).unwrap()).unwrap();

	let elected = join_in(dir.path(), &key, fake_build())
		.await
		.expect("recover from stale endpoint");
	assert!(matches!(elected, SharedProvider::Owner { .. }));
	let current = read_endpoint(&path, &key).expect("endpoint rewritten");
	assert_ne!(current.token, "stale");
	assert_eq!(current.pid, std::process::id());
}
