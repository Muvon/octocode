#!/bin/sh

# Octocode Installation Script
# Universal installation script that works on Unix, Linux, macOS, and Windows
# Works with: bash, zsh, sh, Git Bash, WSL, MSYS2

set -e

# Configuration
REPO="muvon/octocode"
BINARY_NAME="octocode"
INSTALL_DIR="${OCTOCODE_INSTALL_DIR:-$HOME/.local/bin}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Logging functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if a command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Detect platform and architecture
detect_platform() {
    local os arch
    
    # Detect OS
    case "$(uname -s)" in
        Linux*)     os="unknown-linux" ;;
        Darwin*)    os="apple-darwin" ;;
        CYGWIN*|MINGW*|MSYS*)    os="pc-windows" ;;
        *)          log_error "Unsupported operating system: $(uname -s)"; exit 1 ;;
    esac
    
    # Detect architecture
    case "$(uname -m)" in
        x86_64|amd64)   arch="x86_64" ;;
        arm64|aarch64)  arch="aarch64" ;;
        *)              log_error "Unsupported architecture: $(uname -m)"; exit 1 ;;
    esac
    
    # Determine target triple and preferred variant
    case "$os-$arch" in
        unknown-linux-x86_64)    echo "x86_64-unknown-linux-musl" ;;   # Static musl binary
        unknown-linux-aarch64)   echo "aarch64-unknown-linux-musl" ;;  # ARM64 Linux support
        apple-darwin-x86_64)     echo "x86_64-apple-darwin" ;;
        apple-darwin-aarch64)    echo "aarch64-apple-darwin" ;;
        pc-windows-x86_64)       echo "x86_64-pc-windows-msvc" ;;       # MSVC for better compatibility
        pc-windows-aarch64)      echo "aarch64-pc-windows-msvc" ;;      # ARM64 Windows support
        *)                       log_error "Unsupported platform: $os-$arch"; exit 1 ;;
    esac
}

# Get the latest release version
get_latest_version() {
    if command_exists curl; then
        curl -s "https://api.github.com/repos/$REPO/releases/latest" | \
            grep '"tag_name":' | \
            sed -E 's/.*"([^"]+)".*/\1/'
    elif command_exists wget; then
        wget -qO- "https://api.github.com/repos/$REPO/releases/latest" | \
            grep '"tag_name":' | \
            sed -E 's/.*"([^"]+)".*/\1/'
    else
        log_error "Neither curl nor wget found. Please install one of them."
        exit 1
    fi
}

# Download and extract binary
download_and_install() {
    local version="$1"
    local target="$2"
    local tmp_dir
    
    # Create temporary directory (compatible with all systems)
    if command -v mktemp >/dev/null 2>&1; then
        tmp_dir=$(mktemp -d)
    else
        # Fallback for systems without mktemp
        tmp_dir="/tmp/octocode-install-$$"
        mkdir -p "$tmp_dir"
    fi
    
    # Ensure cleanup on exit
    trap "rm -rf '$tmp_dir'" EXIT INT TERM
    
    log_info "Downloading $BINARY_NAME $version for $target..."
    
    # Determine file extension and extract command
    local ext="tar.gz"
    local extract_cmd="tar xzf"
    local binary_name="$BINARY_NAME"
    
    if [ "$target" != "${target#*windows}" ]; then
        ext="zip"
        binary_name="${BINARY_NAME}.exe"
        # Check for unzip command
        if command -v unzip >/dev/null 2>&1; then
            extract_cmd="unzip -q"
        else
            log_error "unzip command not found. Please install unzip to extract Windows binaries."
            exit 1
        fi
    fi
    
    local filename="${BINARY_NAME}-${version}-${target}.${ext}"
    local url="https://github.com/$REPO/releases/download/$version/$filename"
    
    log_info "Downloading from: $url"
    
    # Download with fallback options
    if command -v curl >/dev/null 2>&1; then
        if ! curl -fsSL "$url" -o "$tmp_dir/$filename"; then
            log_error "Download failed with curl"
            exit 1
        fi
    elif command -v wget >/dev/null 2>&1; then
        if ! wget -q "$url" -O "$tmp_dir/$filename"; then
            log_error "Download failed with wget"
            exit 1
        fi
    else
        log_error "Neither curl nor wget found. Please install one of them."
        exit 1
    fi
    
    # Extract
    log_info "Extracting binary..."
    cd "$tmp_dir" || exit 1
    
    if ! $extract_cmd "$filename"; then
        log_error "Failed to extract archive"
        exit 1
    fi
    
    # Find the binary
    local binary_path="$tmp_dir/$binary_name"
    
    if [ ! -f "$binary_path" ]; then
        log_error "Binary '$binary_name' not found in archive"
        log_error "Archive contents:"
        ls -la "$tmp_dir/"
        exit 1
    fi
    
    # Create install directory
    if [ ! -d "$INSTALL_DIR" ]; then
        if ! mkdir -p "$INSTALL_DIR"; then
            log_error "Failed to create install directory: $INSTALL_DIR"
            exit 1
        fi
    fi
    
    # Install binary
    log_info "Installing to $INSTALL_DIR..."
    local target_path="$INSTALL_DIR/$binary_name"
    
    if ! cp "$binary_path" "$target_path"; then
        log_error "Failed to copy binary to $target_path"
        exit 1
    fi
    
    if ! chmod +x "$target_path"; then
        log_error "Failed to make binary executable"
        exit 1
    fi
    
    log_success "$BINARY_NAME installed successfully!"
}

# Check if install directory is in PATH
check_path() {
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) 
            return 0
            ;;
        *)
            log_warning "$INSTALL_DIR is not in your PATH"
            log_info "Add the following line to your shell profile (.bashrc, .zshrc, .profile, etc.):"
            printf "export PATH=\"%s:\$PATH\"\n" "$INSTALL_DIR"
            echo ""
            log_info "Or run the following command to add it to your current session:"
            printf "export PATH=\"%s:\$PATH\"\n" "$INSTALL_DIR"
            echo ""
            ;;
    esac
}

# Verify installation
verify_installation() {
    local binary_name="$BINARY_NAME"
    
    # Add .exe extension for Windows
    case "$(uname -s)" in
        CYGWIN*|MINGW*|MSYS*) binary_name="${BINARY_NAME}.exe" ;;
    esac
    
    local binary_path="$INSTALL_DIR/$binary_name"
    
    if [ -x "$binary_path" ]; then
        log_success "Installation verified!"
        log_info "Run '$BINARY_NAME --version' to check the installed version"
        
        # Try to run the binary if it's in PATH
        if command -v "$BINARY_NAME" >/dev/null 2>&1; then
            local version_output
            if version_output=$("$BINARY_NAME" --version 2>/dev/null); then
                log_info "Installed version: $version_output"
            fi
        fi
    else
        log_error "Installation verification failed: $binary_path not found or not executable"
        exit 1
    fi
}

# Main installation function
main() {
    local version target
    
    log_info "Installing $BINARY_NAME..."
    
    # Parse command line arguments
    while [ $# -gt 0 ]; do
        case $1 in
            --version)
                version="$2"
                shift 2
                ;;
            --target)
                target="$2"
                shift 2
                ;;
            --install-dir)
                INSTALL_DIR="$2"
                shift 2
                ;;
            --help|-h)
                cat << 'EOF'
Octocode Installation Script

USAGE:
    install.sh [OPTIONS]

OPTIONS:
    --version <VERSION>     Install specific version (default: latest)
    --target <TARGET>       Install for specific target platform
    --install-dir <DIR>     Installation directory (default: $HOME/.local/bin)
    --help, -h              Show this help message

EXAMPLES:
    ./install.sh                                          # Install latest version
    ./install.sh --version 0.1.0                         # Install specific version
    ./install.sh --install-dir /usr/local/bin             # Install to custom directory
    ./install.sh --target x86_64-unknown-linux-musl      # Install for specific target

SUPPORTED TARGETS:
    x86_64-unknown-linux-musl    Linux x86_64 (static, recommended)
    aarch64-unknown-linux-musl   Linux ARM64 (static)
    x86_64-apple-darwin          macOS x86_64
    aarch64-apple-darwin         macOS ARM64
    x86_64-pc-windows-msvc       Windows x86_64
    aarch64-pc-windows-msvc      Windows ARM64

ENVIRONMENT VARIABLES:
    OCTOCODE_INSTALL_DIR         Override default installation directory
    OCTOCODE_VERSION            Override version to install

EXAMPLES WITH ENVIRONMENT VARIABLES:
    export OCTOCODE_INSTALL_DIR=/opt/bin
    ./install.sh

    curl -fsSL https://raw.githubusercontent.com/muvon/octocode/main/install.sh | sh
    curl -fsSL https://raw.githubusercontent.com/muvon/octocode/main/install.sh | sh -s -- --version 0.1.0

EOF
                exit 0
                ;;
            *)
                log_error "Unknown option: $1"
                log_info "Use --help for usage information"
                exit 1
                ;;
        esac
    done
    
    # Override with environment variables if set
    version="${version:-$OCTOCODE_VERSION}"
    INSTALL_DIR="${INSTALL_DIR:-$OCTOCODE_INSTALL_DIR}"
    
    # Detect platform if not specified
    if [ -z "$target" ]; then
        target=$(detect_platform)
        log_info "Detected platform: $target"
    fi
    
    # Get latest version if not specified
    if [ -z "$version" ]; then
        log_info "Fetching latest release information..."
        version=$(get_latest_version)
        if [ -z "$version" ]; then
            log_error "Failed to get latest version"
            exit 1
        fi
        log_info "Latest version: $version"
    fi
    
    # Download and install
    download_and_install "$version" "$target"
    
    # Check PATH
    check_path
    
    # Verify installation
    verify_installation
    
    log_success "Installation complete!"
    echo ""
    log_info "To get started, run: $BINARY_NAME --help"
}

# Run main function
main "$@"