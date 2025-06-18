# Multi-stage Dockerfile for octocode
# Stage 1: Build
FROM rust:1.87-slim as builder

# Install system dependencies
RUN apt-get update && apt-get install -y \
		pkg-config \
		libssl-dev \
		&& rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code and config templates
COPY src ./src
COPY config-templates ./config-templates

# Build the application
RUN cargo build --release --no-default-features

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
COPY --from=builder /app/target/release/octocode /usr/local/bin/octocode

# Change ownership to non-root user
RUN chown -R octocode:octocode /app

# Switch to non-root user
USER octocode

# Expose port (if applicable)
# EXPOSE 8080

# Health check (customize based on your application)
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
		CMD octocode --help || exit 1

# Set the entrypoint
ENTRYPOINT ["octocode"]
CMD ["--help"]
