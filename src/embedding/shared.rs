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

//! One loaded model per machine, shared by every octocode process.
//!
//! ONNX weights live in per-process arenas, so N processes embedding with the
//! same model each hold a private copy of the weights. Election is a
//! file-create race on `<storage>/run/embed-<model>.json`: the winner binds a
//! loopback listener, loads the weights and serves inference; everyone else
//! reads the endpoint and connects, never loading the model. Loopback TCP
//! rather than a Unix socket keeps one code path across platforms.
//! (Pattern ported from octomind's shared embedding service, generalized to
//! config-driven model names.)
//!
//! The owner is a normal octocode process — there is no daemon to manage.
//! When it exits its weights go with it and the next join re-elects, paying a
//! one-time model reload. Stale endpoint files self-heal. If the service
//! cannot be joined at all, callers fall back to a private copy: sharing is
//! an optimization, never a hard dependency.

use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use octolib::embedding::types::EmbeddingProviderType;
use octolib::embedding::{EmbeddingProvider, EmbeddingUsage};

use super::create_embedding_provider_from_parts;
use super::InputType;

/// Deferred construction of the real provider — only the election winner
/// runs it, so clients never pay the model load.
type BuildInner = Box<
	dyn FnOnce() -> Pin<Box<dyn Future<Output = Result<Box<dyn EmbeddingProvider>>> + Send>> + Send,
>;

/// Endpoint descriptor written by the owner, read by clients.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
struct Endpoint {
	port: u16,
	token: String,
	/// Guards against a stale file from a different model.
	model: String,
	/// Informational: which process owns the weights.
	pid: u32,
}

/// First line the server sends after a client authenticates: the model facts
/// a client needs but cannot derive without the weights.
#[derive(Serialize, Deserialize, Clone)]
pub(super) struct Hello {
	revision: Option<String>,
	dim: usize,
}

#[derive(Serialize, Deserialize)]
struct Auth {
	token: String,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind")]
enum Request {
	Single {
		text: String,
	},
	Batch {
		texts: Vec<String>,
		input_type: String,
	},
}

#[derive(Serialize, Deserialize)]
struct Response {
	vectors: Option<Vec<Vec<f32>>>,
	input_tokens: u64,
	error: Option<String>,
}

/// Join the machine-wide sharing service for `model_key`.
pub(super) async fn join(
	model_key: &str,
	provider: EmbeddingProviderType,
	model: &str,
) -> Result<SharedProvider> {
	let dir = crate::storage::get_system_storage_dir()?.join("run");
	let model = model.to_string();
	let build: BuildInner = Box::new(move || {
		Box::pin(async move { create_embedding_provider_from_parts(&provider, &model).await })
	});
	join_in(&dir, model_key, build).await
}

/// Election + attach. `dir` is injectable for tests.
async fn join_in(dir: &Path, model_key: &str, build: BuildInner) -> Result<SharedProvider> {
	const ATTEMPTS: usize = 3;
	std::fs::create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;
	let path = endpoint_path(dir, model_key)?;
	let mut last_err: Option<anyhow::Error> = None;

	for attempt in 0..ATTEMPTS {
		if attempt > 0 {
			// Give a racing owner a moment to finish binding before retrying.
			tokio::time::sleep(Duration::from_millis(250)).await;
		}
		// Bind before claiming: the file must never advertise a port that is
		// not listening, or a fast client would get a refusal and evict a
		// live owner. Connections made during the model load queue in the
		// accept backlog until the serve loop starts.
		let listener =
			TcpListener::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))).await?;
		let port = listener.local_addr()?.port();
		let token = format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4()).replace('-', "");
		let endpoint = Endpoint {
			port,
			token: token.clone(),
			model: model_key.to_string(),
			pid: std::process::id(),
		};

		match claim(&path, &endpoint) {
			Ok(true) => {
				// We own it: load the weights, then start answering. On load
				// failure, drop the claim so the next process re-elects
				// instead of discovering a dead port forever.
				let inner: Arc<dyn EmbeddingProvider> = match build().await {
					Ok(p) => Arc::from(p),
					Err(e) => {
						remove_if_unchanged(&path, &endpoint);
						return Err(e);
					}
				};
				let dim = inner.get_dimension();
				let revision = inner.model_revision().await.unwrap_or(None);
				serve(listener, token, Arc::clone(&inner), revision.clone(), dim);
				tracing::debug!(
					"embedding service: elected owner for '{model_key}' on port {port}"
				);
				return Ok(SharedProvider::Owner {
					inner,
					revision,
					dim,
				});
			}
			Ok(false) => {
				drop(listener);
				let existing = read_endpoint(&path, model_key)?;
				let client = Client {
					addr: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, existing.port)),
					token: existing.token.clone(),
				};
				match client.connect().await {
					Ok((_reader, hello)) => {
						tracing::debug!(
							"embedding service: using '{model_key}' owned by pid {} (no local weights)",
							existing.pid
						);
						return Ok(SharedProvider::Client { client, hello });
					}
					Err(e) => {
						// Owner is gone. Drop its claim only if nobody has
						// replaced it since we read it, then re-elect.
						remove_if_unchanged(&path, &existing);
						last_err = Some(e);
					}
				}
			}
			Err(e) => {
				drop(listener);
				last_err = Some(e);
			}
		}
	}

	Err(last_err.unwrap_or_else(|| anyhow::anyhow!("embedding service election failed")))
}

/// The sharing provider: owner of the loaded weights, or a client of the
/// process that owns them. Implements `EmbeddingProvider` so callers cannot
/// tell the difference.
pub(super) enum SharedProvider {
	Owner {
		inner: Arc<dyn EmbeddingProvider>,
		revision: Option<String>,
		dim: usize,
	},
	Client {
		client: Client,
		hello: Hello,
	},
}

#[async_trait::async_trait]
impl EmbeddingProvider for SharedProvider {
	async fn generate_embedding(&self, text: &str) -> Result<(Vec<f32>, EmbeddingUsage)> {
		match self {
			SharedProvider::Owner { inner, .. } => inner.generate_embedding(text).await,
			SharedProvider::Client { client, .. } => {
				let resp = client
					.request(&Request::Single {
						text: text.to_string(),
					})
					.await?;
				let vector = resp
					.vectors
					.filter(|v| v.len() == 1)
					.map(|mut v| v.pop().expect("length checked above"))
					.ok_or_else(|| {
						anyhow::anyhow!(
							"embedding service returned no vector: {}",
							resp.error.unwrap_or_default()
						)
					})?;
				Ok((
					vector,
					EmbeddingUsage {
						input_tokens: resp.input_tokens,
						cost: None,
					},
				))
			}
		}
	}

	async fn generate_embeddings_batch(
		&self,
		texts: Vec<String>,
		input_type: InputType,
	) -> Result<(Vec<Vec<f32>>, EmbeddingUsage)> {
		match self {
			SharedProvider::Owner { inner, .. } => {
				inner.generate_embeddings_batch(texts, input_type).await
			}
			SharedProvider::Client { client, .. } => {
				let resp = client
					.request(&Request::Batch {
						texts,
						input_type: input_type.as_api_str().unwrap_or("none").to_string(),
					})
					.await?;
				let vectors = resp.vectors.ok_or_else(|| {
					anyhow::anyhow!(
						"embedding service batch failed: {}",
						resp.error.unwrap_or_default()
					)
				})?;
				Ok((
					vectors,
					EmbeddingUsage {
						input_tokens: resp.input_tokens,
						cost: None,
					},
				))
			}
		}
	}

	fn get_dimension(&self) -> usize {
		match self {
			SharedProvider::Owner { dim, .. } => *dim,
			SharedProvider::Client { hello, .. } => hello.dim,
		}
	}

	fn is_model_supported(&self) -> bool {
		true
	}

	async fn model_revision(&self) -> Result<Option<String>> {
		match self {
			SharedProvider::Owner { revision, .. } => Ok(revision.clone()),
			SharedProvider::Client { hello, .. } => Ok(hello.revision.clone()),
		}
	}
}

/// Handle to the owner's service. Cheap to clone; one short-lived connection
/// per request, so a dead owner surfaces as a connect error instead of a
/// silently broken pooled socket.
#[derive(Clone)]
pub(super) struct Client {
	addr: SocketAddr,
	token: String,
}

impl Client {
	async fn connect(&self) -> Result<(BufReader<TcpStream>, Hello)> {
		let stream = TcpStream::connect(self.addr).await?;
		stream.set_nodelay(true)?;
		let mut reader = BufReader::new(stream);
		let auth = serde_json::to_string(&Auth {
			token: self.token.clone(),
		})?;
		reader.get_mut().write_all(auth.as_bytes()).await?;
		reader.get_mut().write_all(b"\n").await?;
		let mut line = String::new();
		if reader.read_line(&mut line).await? == 0 {
			bail!("embedding service closed the connection during handshake");
		}
		let hello: Hello = serde_json::from_str(line.trim())?;
		Ok((reader, hello))
	}

	async fn request(&self, req: &Request) -> Result<Response> {
		let (mut reader, _) = self.connect().await?;
		let body = serde_json::to_string(req)?;
		reader.get_mut().write_all(body.as_bytes()).await?;
		reader.get_mut().write_all(b"\n").await?;
		let mut line = String::new();
		if reader.read_line(&mut line).await? == 0 {
			bail!("embedding service closed the connection before responding");
		}
		Ok(serde_json::from_str(line.trim())?)
	}
}

/// One endpoint file per model: processes using different models never
/// contend for the same owner.
fn endpoint_path(dir: &Path, model_key: &str) -> Result<PathBuf> {
	let tag: String = model_key
		.chars()
		.map(|c| {
			if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
				c
			} else {
				'_'
			}
		})
		.collect();
	Ok(dir.join(format!("embed-{tag}.json")))
}

/// Atomically claim ownership. `Ok(false)` means someone else holds it.
fn claim(path: &Path, endpoint: &Endpoint) -> Result<bool> {
	use std::io::Write;

	let mut opts = std::fs::OpenOptions::new();
	opts.write(true).create_new(true);
	#[cfg(unix)]
	{
		use std::os::unix::fs::OpenOptionsExt;
		// The token authenticates a loopback port: owner-readable only.
		opts.mode(0o600);
	}
	match opts.open(path) {
		Ok(mut f) => {
			f.write_all(serde_json::to_string(endpoint)?.as_bytes())?;
			f.flush()?;
			Ok(true)
		}
		Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
		Err(e) => Err(e).with_context(|| format!("failed to claim {}", path.display())),
	}
}

fn read_endpoint(path: &Path, model_key: &str) -> Result<Endpoint> {
	let raw = std::fs::read_to_string(path)
		.with_context(|| format!("failed to read {}", path.display()))?;
	let endpoint: Endpoint = serde_json::from_str(&raw)
		.with_context(|| format!("malformed endpoint file {}", path.display()))?;
	if endpoint.model != model_key {
		bail!("endpoint file advertises model {}", endpoint.model);
	}
	Ok(endpoint)
}

/// Delete the endpoint file only if it still holds `expected`, so we never
/// evict an owner that was elected between our read and this call.
fn remove_if_unchanged(path: &Path, expected: &Endpoint) {
	if matches!(read_endpoint(path, &expected.model), Ok(current) if current == *expected) {
		let _ = std::fs::remove_file(path);
	}
}

/// Answer embed requests until the process exits.
fn serve(
	listener: TcpListener,
	token: String,
	inner: Arc<dyn EmbeddingProvider>,
	revision: Option<String>,
	dim: usize,
) {
	let hello = match serde_json::to_string(&Hello { revision, dim }) {
		Ok(s) => s,
		Err(e) => {
			tracing::debug!("embedding service: cannot serialize hello: {e}");
			return;
		}
	};
	let token = Arc::new(token);
	tokio::spawn(async move {
		loop {
			let Ok((stream, _)) = listener.accept().await else {
				continue;
			};
			let token = Arc::clone(&token);
			let hello = hello.clone();
			let inner = Arc::clone(&inner);
			tokio::spawn(async move {
				if let Err(e) = handle(stream, &token, &hello, inner.as_ref()).await {
					tracing::debug!("embedding service: client dropped: {e}");
				}
			});
		}
	});
}

async fn handle(
	stream: TcpStream,
	token: &str,
	hello: &str,
	inner: &dyn EmbeddingProvider,
) -> Result<()> {
	stream.set_nodelay(true)?;
	let mut reader = BufReader::new(stream);
	let mut line = String::new();
	if reader.read_line(&mut line).await? == 0 {
		return Ok(());
	}
	let auth: Auth = serde_json::from_str(line.trim())?;
	if auth.token != token {
		bail!("rejected client with a bad token");
	}
	reader.get_mut().write_all(hello.as_bytes()).await?;
	reader.get_mut().write_all(b"\n").await?;

	loop {
		line.clear();
		if reader.read_line(&mut line).await? == 0 {
			return Ok(());
		}
		let req: Request = serde_json::from_str(line.trim())?;
		let resp = match req {
			Request::Single { text } => match inner.generate_embedding(&text).await {
				Ok((v, usage)) => Response {
					vectors: Some(vec![v]),
					input_tokens: usage.input_tokens,
					error: None,
				},
				Err(e) => Response {
					vectors: None,
					input_tokens: 0,
					error: Some(e.to_string()),
				},
			},
			Request::Batch { texts, input_type } => {
				let parsed = match input_type.as_str() {
					"none" => Some(InputType::None),
					"query" => Some(InputType::Query),
					"document" => Some(InputType::Document),
					other => {
						tracing::warn!("embedding service: unknown input_type {other}");
						None
					}
				};
				match parsed {
					Some(it) => match inner.generate_embeddings_batch(texts, it).await {
						Ok((v, usage)) => Response {
							vectors: Some(v),
							input_tokens: usage.input_tokens,
							error: None,
						},
						Err(e) => Response {
							vectors: None,
							input_tokens: 0,
							error: Some(e.to_string()),
						},
					},
					None => Response {
						vectors: None,
						input_tokens: 0,
						error: Some("unrecognized input_type".to_string()),
					},
				}
			}
		};
		let body = serde_json::to_string(&resp)?;
		reader.get_mut().write_all(body.as_bytes()).await?;
		reader.get_mut().write_all(b"\n").await?;
	}
}

#[cfg(test)]
#[path = "shared_tests.rs"]
mod tests;
