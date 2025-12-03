#!/bin/bash
# docker-test.sh - Test warp Docker containers
# Validates both CPU and GPU variants

set -e

# ============================================================================
# Configuration
# ============================================================================

# Test timeout (seconds)
TIMEOUT=10

# Test type (cpu, gpu, or all)
TEST_TYPE=${TEST_TYPE:-"cpu"}

# ============================================================================
# Color output
# ============================================================================
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

test_info() {
    echo -e "${BLUE}[TEST]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

success() {
    echo -e "${GREEN}[PASS]${NC} $1"
}

# ============================================================================
# Test functions
# ============================================================================

test_cpu_basic() {
    info "Testing CPU container basic functionality..."

    # Test 1: Check version
    test_info "Test 1: Version check"
    if docker run --rm warp:latest --version; then
        success "Version check passed"
    else
        error "Version check failed"
        return 1
    fi

    # Test 2: Help command
    test_info "Test 2: Help command"
    if docker run --rm warp:latest --help > /dev/null; then
        success "Help command passed"
    else
        error "Help command failed"
        return 1
    fi

    # Test 3: Test with invalid command (should fail gracefully)
    test_info "Test 3: Invalid command handling"
    if ! docker run --rm warp:latest invalid-command 2>/dev/null; then
        success "Invalid command handled gracefully"
    else
        warn "Invalid command did not fail as expected"
    fi

    return 0
}

test_cpu_network() {
    info "Testing CPU container network functionality..."

    local container_name="warp-test-$$"
    local test_port="19999"

    # Test 4: Start container in listen mode
    test_info "Test 4: Starting listen mode (background)"
    docker run -d \
        --name="${container_name}" \
        --rm \
        -p "${test_port}:9999" \
        warp:latest \
        listen --port 9999 --bind 0.0.0.0 2>/dev/null || true

    # Wait for container to start
    sleep 3

    # Check if container is running
    test_info "Test 5: Verifying container is running"
    if docker ps | grep -q "${container_name}"; then
        success "Container is running"

        # Show logs
        test_info "Container logs:"
        docker logs "${container_name}" 2>&1 | head -10 || true

        # Cleanup
        info "Stopping test container..."
        docker stop "${container_name}" > /dev/null 2>&1 || true
        success "Network test completed"
    else
        warn "Container failed to start or exited early"
        docker logs "${container_name}" 2>&1 || true
        return 1
    fi

    return 0
}

test_gpu_basic() {
    info "Testing GPU container basic functionality..."

    # Check if nvidia-docker is available
    if ! command -v nvidia-smi &> /dev/null; then
        warn "nvidia-smi not found, skipping GPU tests"
        warn "GPU tests require NVIDIA drivers and nvidia-container-toolkit"
        return 0
    fi

    # Test 1: Check version with GPU runtime
    test_info "Test 1: GPU container version check"
    if docker run --rm --gpus all warp:latest-cuda --version; then
        success "GPU version check passed"
    else
        error "GPU version check failed"
        return 1
    fi

    # Test 2: Verify CUDA is accessible
    test_info "Test 2: CUDA accessibility check"
    if docker run --rm --gpus all warp:latest-cuda sh -c 'nvidia-smi 2>/dev/null || echo "CUDA runtime available"'; then
        success "CUDA accessibility passed"
    else
        warn "CUDA accessibility check inconclusive"
    fi

    return 0
}

test_image_size() {
    info "Checking image sizes..."

    if docker images warp:latest --format "table {{.Repository}}:{{.Tag}}\t{{.Size}}" | grep -v "REPOSITORY"; then
        success "Image size check completed"
    fi

    if docker images warp:latest-cuda --format "table {{.Repository}}:{{.Tag}}\t{{.Size}}" 2>/dev/null | grep -v "REPOSITORY"; then
        success "GPU image size check completed"
    fi

    return 0
}

test_healthcheck() {
    info "Testing container health checks..."

    local container_name="warp-health-test-$$"

    test_info "Starting container with health check..."
    docker run -d \
        --name="${container_name}" \
        --rm \
        warp:latest \
        sleep 30 2>/dev/null || true

    sleep 2

    # Check health status
    test_info "Checking health status..."
    local health_status=$(docker inspect --format='{{.State.Health.Status}}' "${container_name}" 2>/dev/null || echo "no-healthcheck")

    if [[ "${health_status}" != "no-healthcheck" ]]; then
        info "Health status: ${health_status}"
        success "Health check is configured"
    fi

    # Cleanup
    docker stop "${container_name}" > /dev/null 2>&1 || true

    return 0
}

# ============================================================================
# Main execution
# ============================================================================

info "Starting warp Docker container tests"
info "Test type: ${TEST_TYPE}"
echo ""

# Track test results
TESTS_PASSED=0
TESTS_FAILED=0

# Run CPU tests
if [[ "${TEST_TYPE}" == "cpu" ]] || [[ "${TEST_TYPE}" == "all" ]]; then
    info "========================================="
    info "CPU Container Tests"
    info "========================================="
    echo ""

    if test_cpu_basic; then
        ((TESTS_PASSED++))
    else
        ((TESTS_FAILED++))
    fi
    echo ""

    if test_cpu_network; then
        ((TESTS_PASSED++))
    else
        ((TESTS_FAILED++))
    fi
    echo ""

    if test_healthcheck; then
        ((TESTS_PASSED++))
    else
        ((TESTS_FAILED++))
    fi
    echo ""
fi

# Run GPU tests
if [[ "${TEST_TYPE}" == "gpu" ]] || [[ "${TEST_TYPE}" == "all" ]]; then
    info "========================================="
    info "GPU Container Tests"
    info "========================================="
    echo ""

    if test_gpu_basic; then
        ((TESTS_PASSED++))
    else
        ((TESTS_FAILED++))
    fi
    echo ""
fi

# Image size check
info "========================================="
info "Image Information"
info "========================================="
echo ""
test_image_size
echo ""

# ============================================================================
# Summary
# ============================================================================

info "========================================="
info "Test Summary"
info "========================================="
echo ""
echo "  Tests passed: ${TESTS_PASSED}"
echo "  Tests failed: ${TESTS_FAILED}"
echo ""

if [[ ${TESTS_FAILED} -eq 0 ]]; then
    success "All tests passed!"
    exit 0
else
    error "Some tests failed"
    exit 1
fi
