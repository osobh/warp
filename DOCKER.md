# Warp Docker Deployment Guide

Comprehensive guide for building, testing, and deploying warp containers with both CPU-only and GPU-accelerated variants.

## Quick Start

```bash
# Build all images (CPU + GPU)
./scripts/docker-build.sh

# Test CPU container
./scripts/docker-test.sh

# Run with docker-compose
docker-compose up warp
```

## Architecture Overview

The warp project provides multi-stage Docker builds optimized for:

- **Minimal image size**: Separates build dependencies from runtime
- **Security**: Runs as non-root user
- **Flexibility**: Supports both CPU-only and GPU-accelerated workloads
- **Production-ready**: Includes health checks, proper logging, and resource limits

## Files Structure

```
warp/
├── Dockerfile              # CPU-only multi-stage build
├── Dockerfile.gpu          # GPU-accelerated with CUDA 12.6
├── docker-compose.yml      # Service orchestration
├── .dockerignore           # Build context optimization
└── scripts/
    ├── docker-build.sh     # Build automation script
    └── docker-test.sh      # Container testing script
```

## Building Images

### CPU-Only Build

```bash
# Using build script (recommended)
./scripts/docker-build.sh

# Or manually
docker build -t warp:latest -f Dockerfile .
```

### GPU-Accelerated Build

```bash
# Using build script
BUILD_TYPE=gpu ./scripts/docker-build.sh

# Or manually
docker build -t warp:latest-cuda -f Dockerfile.gpu .
```

### Custom Registry

```bash
# Build with registry prefix
REGISTRY="ghcr.io/yourusername/" ./scripts/docker-build.sh

# Results in:
#   ghcr.io/yourusername/warp:0.1.0
#   ghcr.io/yourusername/warp:latest
#   ghcr.io/yourusername/warp:0.1.0-cuda
#   ghcr.io/yourusername/warp:latest-cuda
```

## Running Containers

### Basic Usage

```bash
# Show help
docker run --rm warp:latest --help

# Check version
docker run --rm warp:latest --version

# Transfer file
docker run --rm -v $(pwd):/data warp:latest send file.txt receiver:9999
```

### Listen Mode

```bash
# Start listener on port 9999
docker run -d \
  --name warp-listener \
  -p 9999:9999 \
  -v $(pwd)/data:/data \
  warp:latest \
  listen --port 9999 --bind 0.0.0.0
```

### GPU-Accelerated

Requires:
- NVIDIA drivers
- nvidia-container-toolkit

```bash
# Run with GPU support
docker run --rm --gpus all warp:latest-cuda --version

# Check GPU availability
docker run --rm --gpus all warp:latest-cuda sh -c "nvidia-smi"

# Transfer with GPU acceleration
docker run --rm \
  --gpus all \
  -v $(pwd):/data \
  warp:latest-cuda \
  send large-file.bin receiver:9999
```

## Docker Compose

### Default CPU Service

```bash
# Start CPU service
docker-compose up warp

# Run in background
docker-compose up -d warp

# View logs
docker-compose logs -f warp

# Stop service
docker-compose down
```

### GPU Service

```bash
# Start GPU service (requires --profile gpu)
docker-compose --profile gpu up warp-gpu

# Start both CPU and GPU services
docker-compose --profile gpu up
```

### Listener Service

```bash
# Start persistent listener
docker-compose --profile listener up warp-listener

# This runs 'warp listen' continuously
# Incoming transfers saved to ./data/incoming/
```

### Portal Hub Service

```bash
# Start portal hub for multi-node coordination
docker-compose --profile portal up warp-portal

# Access at http://localhost:8080
```

### All Services

```bash
# Start everything (CPU, GPU, listener, portal)
docker-compose --profile gpu --profile listener --profile portal up -d

# View all service logs
docker-compose logs -f
```

## Environment Variables

### Build-time Variables

```bash
# Rust version (default: 1.85)
docker build --build-arg RUST_VERSION=1.85 -t warp:latest .

# CUDA version (default: 12.6.0)
docker build --build-arg CUDA_VERSION=12.6.0 -f Dockerfile.gpu -t warp:latest-cuda .
```

### Runtime Variables

```bash
# Set via docker run
docker run --rm \
  -e RUST_LOG=debug \
  -e RUST_BACKTRACE=1 \
  warp:latest --version

# Set via docker-compose
RUST_LOG=debug docker-compose up warp
```

Available environment variables:
- `RUST_LOG`: Logging level (error, warn, info, debug, trace)
- `RUST_BACKTRACE`: Enable backtraces (0, 1, full)
- `CUDA_VISIBLE_DEVICES`: GPU selection for multi-GPU systems

## Testing Containers

### Automated Tests

```bash
# Run all CPU tests
./scripts/docker-test.sh

# Run GPU tests (requires nvidia-docker)
TEST_TYPE=gpu ./scripts/docker-test.sh

# Run all tests
TEST_TYPE=all ./scripts/docker-test.sh
```

### Manual Testing

```bash
# Test version
docker run --rm warp:latest --version

# Test help
docker run --rm warp:latest --help

# Test listener startup
docker run --rm -p 9999:9999 warp:latest listen --port 9999 &
sleep 2
docker ps
docker stop $(docker ps -q --filter ancestor=warp:latest)

# Test GPU availability
docker run --rm --gpus all warp:latest-cuda sh -c "nvidia-smi || echo 'No GPU'"
```

## Image Sizes

Expected sizes (approximate):

| Image | Size | Description |
|-------|------|-------------|
| `warp:latest` | ~100 MB | CPU-only, Debian slim runtime |
| `warp:latest-cuda` | ~2.5 GB | GPU-accelerated, CUDA 12.6 runtime |

Check actual sizes:
```bash
docker images warp --format "table {{.Repository}}:{{.Tag}}\t{{.Size}}"
```

## Production Deployment

### Push to Registry

```bash
# GitHub Container Registry
docker tag warp:latest ghcr.io/yourusername/warp:latest
docker push ghcr.io/yourusername/warp:latest

# Docker Hub
docker tag warp:latest yourusername/warp:latest
docker push yourusername/warp:latest
```

### Resource Limits

```yaml
# docker-compose.yml with resource limits
services:
  warp:
    image: warp:latest
    deploy:
      resources:
        limits:
          cpus: '4.0'
          memory: 8G
        reservations:
          cpus: '2.0'
          memory: 4G
```

### Health Checks

Built-in health check runs every 30 seconds:
```bash
# Check health status
docker inspect --format='{{.State.Health.Status}}' warp

# View health check logs
docker inspect --format='{{range .State.Health.Log}}{{.Output}}{{end}}' warp
```

### Kubernetes Deployment

Example Kubernetes manifest:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: warp
spec:
  replicas: 3
  selector:
    matchLabels:
      app: warp
  template:
    metadata:
      labels:
        app: warp
    spec:
      containers:
      - name: warp
        image: ghcr.io/yourusername/warp:latest
        ports:
        - containerPort: 9999
        resources:
          limits:
            memory: "8Gi"
            cpu: "4"
          requests:
            memory: "4Gi"
            cpu: "2"
        livenessProbe:
          exec:
            command:
            - warp
            - --version
          initialDelaySeconds: 5
          periodSeconds: 30
```

## Troubleshooting

### Build Issues

```bash
# Clean Docker cache
docker builder prune -a

# Build with no cache
docker build --no-cache -t warp:latest .

# Enable BuildKit debug
BUILDKIT_PROGRESS=plain docker build -t warp:latest .
```

### Runtime Issues

```bash
# Check container logs
docker logs warp

# Run interactive shell
docker run --rm -it warp:latest bash

# Inspect container
docker inspect warp

# Check resource usage
docker stats warp
```

### GPU Issues

```bash
# Verify nvidia-docker installation
docker run --rm --gpus all nvidia/cuda:12.6.0-base-ubuntu22.04 nvidia-smi

# Check NVIDIA driver
nvidia-smi

# Check docker GPU runtime
docker info | grep -i nvidia
```

### Network Issues

```bash
# Check port binding
docker port warp

# Test connectivity
docker exec warp ping -c 3 google.com

# Inspect network
docker network inspect warp-network
```

## Security Considerations

1. **Non-root user**: Containers run as user `warp` (UID 1000)
2. **Minimal base images**: Uses Debian slim to reduce attack surface
3. **No unnecessary packages**: Only runtime dependencies included
4. **Health checks**: Automatic container health monitoring
5. **Read-only root filesystem**: Consider adding `--read-only` flag

### Security Hardening

```bash
# Run with read-only root filesystem
docker run --rm --read-only -v /tmp warp:latest --version

# Drop all capabilities
docker run --rm --cap-drop=ALL warp:latest --version

# Set security options
docker run --rm \
  --security-opt=no-new-privileges:true \
  --cap-drop=ALL \
  warp:latest --version
```

## Performance Optimization

### Multi-stage Build Caching

The Dockerfiles use dependency caching to speed up rebuilds:
1. First builds dependencies only (cached layer)
2. Then copies source and builds actual binaries
3. Typical rebuild time: 2-5 minutes (vs 15-20 minutes full build)

### BuildKit Features

```bash
# Enable inline cache for faster CI builds
docker build --build-arg BUILDKIT_INLINE_CACHE=1 -t warp:latest .

# Use cache from registry
docker build --cache-from=warp:latest -t warp:latest .
```

### Runtime Performance

```bash
# Increase shared memory for large transfers
docker run --rm --shm-size=2g warp:latest send large-file.bin receiver:9999

# Use host network for maximum throughput
docker run --rm --network=host warp:latest send file.bin receiver:9999
```

## Contributing

When modifying Docker files:

1. Test both CPU and GPU variants
2. Verify image size hasn't increased significantly
3. Run automated tests: `./scripts/docker-test.sh`
4. Update this documentation

## License

Same as warp project: MIT OR Apache-2.0
