# --- Build Stage ---
FROM rust:1.92-slim-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    cmake \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy only the dependency manifests to cache them
COPY Cargo.toml Cargo.lock ./

# Create a dummy source file to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

# Now copy the actual source code
COPY . .

# Build the application
# We touch main.rs to ensure cargo rebuilds it after the dummy build
RUN touch src/main.rs && cargo build --release

# --- Runtime Stage ---
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary from the builder stage
COPY --from=builder /app/target/release/omnissiatac /app/omnissiatac

# Copy the wrapper script
COPY run.sh /app/run.sh
RUN chmod +x /app/run.sh

# Copy static assets required by the web server
COPY static /app/static

# Create directory for playlists
RUN mkdir /app/playlists

# Copy the example config as a default
COPY config.toml.example /app/config.toml

# Expose the web server port
EXPOSE 3000

# Run the bot wrapper
CMD ["./run.sh"]
