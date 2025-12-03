# Warp Docker Quick Start

Fast reference for common Docker operations with warp.

## Build

```bash
# Build everything (CPU + GPU)
./scripts/docker-build.sh

# Build CPU only
BUILD_TYPE=cpu ./scripts/docker-build.sh

# Build GPU only
BUILD_TYPE=gpu ./scripts/docker-build.sh

# Build with custom registry
REGISTRY="ghcr.io/yourusername/" ./scripts/docker-build.sh
```

## Test

```bash
# Test CPU containers
./scripts/docker-test.sh

# Test GPU containers (requires nvidia-docker)
TEST_TYPE=gpu ./scripts/docker-test.sh

# Test everything
TEST_TYPE=all ./scripts/docker-test.sh
```

## Run

```bash
# Show help
docker run --rm warp:latest --help

# Check version
docker run --rm warp:latest --version

# Send file
docker run --rm -v $(pwd):/data warp:latest send file.txt receiver:9999

# Listen mode
docker run -d -p 9999:9999 -v $(pwd)/data:/data warp:latest listen

# GPU mode
docker run --rm --gpus all warp:latest-cuda --version
```

## Docker Compose

```bash
# Start CPU service
docker-compose up warp

# Start GPU service
docker-compose --profile gpu up warp-gpu

# Start listener
docker-compose --profile listener up warp-listener

# Start everything
docker-compose --profile gpu --profile listener --profile portal up -d

# Stop all
docker-compose down
```

## Development

```bash
# Use development compose
docker-compose -f docker-compose.dev.yml up

# Run cluster (3 nodes)
docker-compose -f docker-compose.dev.yml --profile cluster up

# Run with monitoring
docker-compose -f docker-compose.dev.yml --profile monitoring up
```

## Common Commands

```bash
# View logs
docker logs warp
docker-compose logs -f warp

# Execute command in running container
docker exec -it warp warp --version

# Check resource usage
docker stats warp

# Inspect container
docker inspect warp

# Remove all warp containers
docker ps -a | grep warp | awk '{print $1}' | xargs docker rm -f

# Remove all warp images
docker images | grep warp | awk '{print $3}' | xargs docker rmi -f

# Clean everything
docker system prune -a
```

## Troubleshooting

```bash
# Check if Docker is running
docker info

# Check if nvidia-docker works
docker run --rm --gpus all nvidia/cuda:12.6.0-base-ubuntu22.04 nvidia-smi

# Rebuild without cache
docker build --no-cache -t warp:latest .

# View BuildKit output
BUILDKIT_PROGRESS=plain docker build -t warp:latest .

# Check port availability
netstat -tulpn | grep 9999

# Test network connectivity
docker network inspect warp-network
```

## Environment Variables

```bash
# Set log level
docker run -e RUST_LOG=debug warp:latest

# Enable backtraces
docker run -e RUST_BACKTRACE=1 warp:latest

# Select GPU
docker run --gpus all -e CUDA_VISIBLE_DEVICES=0 warp:latest-cuda

# Via compose
RUST_LOG=debug docker-compose up
```

## Production

```bash
# Tag for registry
docker tag warp:latest ghcr.io/username/warp:latest

# Push to registry
docker push ghcr.io/username/warp:latest

# Pull from registry
docker pull ghcr.io/username/warp:latest

# Run with resource limits
docker run --memory=8g --cpus=4 warp:latest

# Run with restart policy
docker run -d --restart=unless-stopped warp:latest listen
```

## File Locations

- `/home/osobh/projects/warp/Dockerfile` - CPU-only build
- `/home/osobh/projects/warp/Dockerfile.gpu` - GPU build
- `/home/osobh/projects/warp/docker-compose.yml` - Production compose
- `/home/osobh/projects/warp/docker-compose.dev.yml` - Development compose
- `/home/osobh/projects/warp/.dockerignore` - Build exclusions
- `/home/osobh/projects/warp/scripts/docker-build.sh` - Build script
- `/home/osobh/projects/warp/scripts/docker-test.sh` - Test script
- `/home/osobh/projects/warp/DOCKER.md` - Full documentation

## Image Tags

- `warp:latest` - Latest CPU-only build
- `warp:0.1.0` - Version 0.1.0 CPU build
- `warp:latest-cuda` - Latest GPU build
- `warp:0.1.0-cuda` - Version 0.1.0 GPU build

## Getting Help

- Full documentation: [DOCKER.md](DOCKER.md)
- Project README: [README.md](README.md)
- Issues: GitHub issue tracker
