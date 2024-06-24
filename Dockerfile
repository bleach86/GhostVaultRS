# Stage 1: Build the Rust application
FROM rust:slim-bookworm AS builder

# Set environment variables
ENV DEBIAN_FRONTEND=noninteractive

# Install dependencies
RUN apt-get update && \
    apt-get install -y \
    build-essential \
    libssl-dev \
    pkg-config \
    libfontconfig-dev

# Create a non-root user
RUN useradd -ms /bin/bash gvuser && \
    echo "gvuser ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers

# Create application directory
WORKDIR /home/gvuser/app

# Copy the current directory contents into the container
COPY . .

# Build the application
RUN cargo build --release

# Stage 2: Create the runtime container
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y \
    libssl-dev \
    libfontconfig-dev \
    ca-certificates \
    tini && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

# Create a non-root user
RUN useradd -ms /bin/bash gvuser && \
    echo "gvuser ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers

# Create persistent directory
RUN mkdir -p /data /home/gvuser/GhostVault && \
    chown -R gvuser:gvuser /data && \
    chown -R gvuser:gvuser /home/gvuser/GhostVault

# Link legacy GhostVault data directory
RUN ln -s /data/GhostVault/daemon.json /home/gvuser/GhostVault/daemon.json

# Switch to the non-root user
USER gvuser

# Copy the built binaries from the builder stage
COPY --from=builder /home/gvuser/app/target/release/ghostvaultd /home/gvuser/ghostvaultd
COPY --from=builder /home/gvuser/app/target/release/gv-cli /home/gvuser/gv-cli

# Ensure the persistent directories are accessible
VOLUME /data

ENV DOCKER_RUNNING=true

# Use tini to ensure proper signal handling
ENTRYPOINT ["/usr/bin/tini", "--"]

# Run the compiled binary
CMD ["/home/gvuser/ghostvaultd", "--gv-data-dir", "/data/.ghostvault", "--daemon-data-dir", "/data/.ghost", "--console"]
