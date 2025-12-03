#!/bin/bash
# docker-build.sh - Build warp Docker images with proper tagging
# Supports both CPU-only and GPU-accelerated variants

set -e

# ============================================================================
# Configuration
# ============================================================================

# Detect version from Cargo.toml workspace
VERSION=${VERSION:-$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')}

# Optional registry prefix (e.g., "ghcr.io/username/" or "docker.io/username/")
REGISTRY=${REGISTRY:-""}

# Build platform (default: current platform)
PLATFORM=${PLATFORM:-"linux/amd64"}

# Build type (cpu, gpu, or all)
BUILD_TYPE=${BUILD_TYPE:-"all"}

# Enable BuildKit for better build performance
export DOCKER_BUILDKIT=1

# ============================================================================
# Color output
# ============================================================================
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# ============================================================================
# Build functions
# ============================================================================

build_cpu() {
    info "Building CPU-only warp image..."
    info "  Version: ${VERSION}"
    info "  Platform: ${PLATFORM}"

    docker build \
        --platform="${PLATFORM}" \
        --file=Dockerfile \
        --tag="${REGISTRY}warp:${VERSION}" \
        --tag="${REGISTRY}warp:latest" \
        --build-arg="BUILDKIT_INLINE_CACHE=1" \
        .

    info "CPU build complete: ${REGISTRY}warp:${VERSION}"
}

build_gpu() {
    info "Building GPU-accelerated warp image..."
    info "  Version: ${VERSION}"
    info "  Platform: ${PLATFORM}"

    docker build \
        --platform="${PLATFORM}" \
        --file=Dockerfile.gpu \
        --tag="${REGISTRY}warp:${VERSION}-cuda" \
        --tag="${REGISTRY}warp:latest-cuda" \
        --build-arg="BUILDKIT_INLINE_CACHE=1" \
        .

    info "GPU build complete: ${REGISTRY}warp:${VERSION}-cuda"
}

# ============================================================================
# Main execution
# ============================================================================

info "Starting warp Docker build process"
info "Build type: ${BUILD_TYPE}"

# Verify we're in the project root
if [[ ! -f "Cargo.toml" ]]; then
    error "Must be run from the warp project root directory"
    exit 1
fi

# Build based on type
case "${BUILD_TYPE}" in
    cpu)
        build_cpu
        ;;
    gpu)
        build_gpu
        ;;
    all)
        build_cpu
        build_gpu
        ;;
    *)
        error "Invalid BUILD_TYPE: ${BUILD_TYPE}. Must be 'cpu', 'gpu', or 'all'"
        exit 1
        ;;
esac

# ============================================================================
# Summary
# ============================================================================

info "Build complete! Images created:"
echo ""

if [[ "${BUILD_TYPE}" == "cpu" ]] || [[ "${BUILD_TYPE}" == "all" ]]; then
    echo "  CPU:"
    echo "    - ${REGISTRY}warp:${VERSION}"
    echo "    - ${REGISTRY}warp:latest"
    echo ""
fi

if [[ "${BUILD_TYPE}" == "gpu" ]] || [[ "${BUILD_TYPE}" == "all" ]]; then
    echo "  GPU:"
    echo "    - ${REGISTRY}warp:${VERSION}-cuda"
    echo "    - ${REGISTRY}warp:latest-cuda"
    echo ""
fi

info "To test the images, run: ./scripts/docker-test.sh"
info "To push to registry, run: docker push ${REGISTRY}warp:<tag>"
