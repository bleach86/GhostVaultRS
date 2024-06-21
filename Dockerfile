# Use an official rust as a parent image
FROM rust:slim-bookworm

# Set environment variables
ENV DEBIAN_FRONTEND=noninteractive

# Install dependencies
RUN apt-get update && \
    apt-get install -y \
    build-essential \
    libssl-dev \
    pkg-config \
    libfontconfig-dev \ 
    tini
    
# Create a non-root user
RUN useradd -ms /bin/bash gvuser && \
    echo "gvuser ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers

# Create persistent directory
RUN mkdir -p /data /home/gvuser/GhostVault && \
    chown -R gvuser:gvuser /data && \
    chown -R gvuser:gvuser /home/gvuser/GhostVault

# link lagacy GhostVault data directory
RUN ln -s /data/GhostVault/daemon.json /home/gvuser/GhostVault/daemon.json

# Switch to the non-root user
USER gvuser

# Create application directory
WORKDIR /home/gvuser/app

# Copy the current directory contents into the container at /app
COPY . .

# Build and install GhostVaultRS
RUN cargo install --path .

# Ensure the persistent directories are accessible
VOLUME /data

ENV DOCKER_RUNNING=true

# Use tini to ensure proper signal handling
ENTRYPOINT ["/usr/bin/tini", "--"]

# Run the compiled binary
CMD ["ghostvaultd", "--gv-data-dir", "/data/.ghostvault", "--daemon-data-dir", "/data/.ghost", "--console"]