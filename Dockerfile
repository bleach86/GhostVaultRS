# Use an official Ubuntu as a parent image
FROM rust:slim-bookworm

# Set environment variables
#ENV DEBIAN_FRONTEND=noninteractive

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

# Create persistent directories
RUN mkdir -p /data/ghostd_data /data/gv_data /home/gvuser/GhostVault /legacy_data/ && \
    chown -R gvuser:gvuser /data && \
    chown -R gvuser:gvuser /home/gvuser/GhostVault && \
    chown -R gvuser:gvuser /legacy_data


# Link link legacy data to the new dir in home
RUN ln -s /legacy_data/daemon.json /home/gvuser/GhostVault/daemon.json


# Switch to the non-root user
USER gvuser

WORKDIR /home/gvuser


# Create application directory
WORKDIR /home/gvuser/app

# Copy the current directory contents into the container at /app
COPY . .

# Build and install GhostVaultRS
RUN cargo install --path .

# Ensure the persistent directories are accessible
VOLUME /data/ghostd_data /data/gv_data /home/gvuser/GhostVault /legacy_data

# Make the persistent directories available as environment variables
ENV ghostd_data=/data/ghostd_data
ENV gv_data=/data/gv_data
ENV DOCKER_RUNNING=true

# Run the compiled binary
CMD ["ghostvaultd", "--gv-data-dir", "/data/gv_data/.ghostvault", "--daemon-data-dir", "/data/ghostd_data/.ghost", "--console"]