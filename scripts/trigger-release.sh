#!/bin/bash

# Script to trigger release from parent PinePods repository
# Usage: ./trigger-release.sh <version> [--dry-run]

set -e

VERSION="$1"
DRY_RUN=false

if [ "$2" = "--dry-run" ]; then
    DRY_RUN=true
fi

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

validate_version() {
    if [ -z "$VERSION" ]; then
        log_error "Version is required"
        echo "Usage: $0 <version> [--dry-run]"
        echo "Example: $0 1.2.3"
        exit 1
    fi
    
    # Remove 'v' prefix if present
    VERSION=${VERSION#v}
    
    # Validate semantic version format
    if ! echo "$VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.-]+)?$'; then
        log_error "Invalid version format: $VERSION"
        log_error "Expected format: X.Y.Z or X.Y.Z-suffix"
        exit 1
    fi
    
    log_info "Validated version: $VERSION"
}

check_github_token() {
    if [ -z "$GITHUB_TOKEN" ]; then
        log_error "GITHUB_TOKEN environment variable is required"
        log_error "Please set it to a GitHub personal access token with repo permissions"
        exit 1
    fi
}

trigger_github_dispatch() {
    local repo="$1"
    local payload="$2"
    
    if [ "$DRY_RUN" = true ]; then
        log_warn "DRY RUN: Would trigger release for $repo with payload:"
        echo "$payload" | jq .
        return 0
    fi
    
    log_info "Triggering repository_dispatch event for $repo..."
    
    local response
    response=$(curl -s -w "\n%{http_code}" \
        -X POST \
        -H "Accept: application/vnd.github+json" \
        -H "Authorization: Bearer $GITHUB_TOKEN" \
        -H "X-GitHub-Api-Version: 2022-11-28" \
        "https://api.github.com/repos/$repo/dispatches" \
        -d "$payload")
    
    local http_code=$(echo "$response" | tail -n1)
    local response_body=$(echo "$response" | head -n -1)
    
    if [ "$http_code" -eq 204 ]; then
        log_success "Successfully triggered release for $repo"
    else
        log_error "Failed to trigger release for $repo (HTTP $http_code)"
        if [ -n "$response_body" ]; then
            echo "$response_body" | jq . 2>/dev/null || echo "$response_body"
        fi
        exit 1
    fi
}

main() {
    echo -e "${BLUE}"
    echo "┌─────────────────────────────────────────┐"
    echo "│     🌲 PinePods Firewood Release        │"
    echo "│        Cross-Repo Trigger Script       │"
    echo "└─────────────────────────────────────────┘"
    echo -e "${NC}"
    
    validate_version
    check_github_token
    
    # Create payload
    local payload
    payload=$(jq -n \
        --arg version "$VERSION" \
        --arg triggered_by "${GITHUB_ACTOR:-$(git config user.name)}" \
        --arg trigger_repo "${GITHUB_REPOSITORY:-unknown}" \
        --arg trigger_sha "${GITHUB_SHA:-$(git rev-parse HEAD 2>/dev/null || echo 'unknown')}" \
        '{
            event_type: "release",
            client_payload: {
                version: $version,
                triggered_by: $triggered_by,
                trigger_repo: $trigger_repo,
                trigger_sha: $trigger_sha,
                timestamp: now
            }
        }')
    
    log_info "Prepared payload for version $VERSION"
    
    # Trigger release on firewood repository
    trigger_github_dispatch "madeofpendletonwool/pinepods-firewood" "$payload"
    
    log_success "Release trigger completed successfully!"
    
    if [ "$DRY_RUN" = false ]; then
        echo ""
        log_info "You can monitor the release progress at:"
        echo "https://github.com/madeofpendletonwool/pinepods-firewood/actions"
        echo ""
        log_info "Once complete, binaries will be available at:"
        echo "https://github.com/madeofpendletonwool/pinepods-firewood/releases/tag/v$VERSION"
    fi
}

# Check if jq is available
if ! command -v jq >/dev/null 2>&1; then
    log_error "jq is required but not installed"
    log_error "Please install jq: https://stedolan.github.io/jq/download/"
    exit 1
fi

main "$@"