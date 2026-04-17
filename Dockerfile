# Copyright 2025 Muvon Un Limited
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

# Multi-stage Dockerfile for octocode
# Stage 1: Build
FROM rust:1.94.0-slim AS builder

# Install system dependencies
RUN apt-get update && apt-get install -y \
		pkg-config \
		protobuf-compiler \
		libssl-dev \
		&& rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /app

# Copy manifests first for dependency caching layer
COPY Cargo.toml Cargo.lock ./

# Create a dummy main.rs to build and cache dependencies
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs

# Build dependencies only (cached via BuildKit mount)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
	--mount=type=cache,target=/app/target \
	cargo build --release --no-default-features 2>/dev/null || true

# Remove dummy source and copy real source code + config templates
RUN rm -rf src
COPY src ./src
COPY config-templates ./config-templates

# Build the application (dependencies hit cache, only source recompiled)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
	--mount=type=cache,target=/app/target \
	touch src/main.rs && cargo build --release --no-default-features

# Copy binary out of cache mount to a known location
RUN --mount=type=cache,target=/app/target \
	cp target/release/octocode /app/octocode-bin

# Stage 2: Runtime
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
		ca-certificates \
		&& rm -rf /var/lib/apt/lists/* \
		&& update-ca-certificates

# Create a non-root user
RUN groupadd -r octocode && useradd -r -g octocode octocode

# Create app directory
WORKDIR /app

# Copy the binary from builder stage
COPY --from=builder /app/octocode-bin /usr/local/bin/octocode

# Change ownership to non-root user
RUN chown -R octocode:octocode /app

# Switch to non-root user
USER octocode

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
		CMD octocode --help || exit 1

# Set the entrypoint
ENTRYPOINT ["octocode"]
CMD ["--help"]
