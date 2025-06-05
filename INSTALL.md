# Installation Guide

## Quick Install

### Universal Script (Unix/Linux/macOS/Windows)
```bash
curl -fsSL https://raw.githubusercontent.com/muvon/octocode/main/install.sh | sh
```

**Works on:**
- Linux (any distribution, x86_64 and ARM64)
- macOS (Intel and Apple Silicon)  
- Windows (x86_64 and ARM64 via Git Bash, WSL, MSYS2, Cygwin)
- Any Unix-like system with `/bin/sh`

### Installation Options
```bash
# Install specific version
curl -fsSL https://raw.githubusercontent.com/muvon/octocode/main/install.sh | sh -s -- --version 0.1.0

# Install to custom directory
curl -fsSL https://raw.githubusercontent.com/muvon/octocode/main/install.sh | sh -s -- --install-dir /usr/local/bin

# Install for specific target
curl -fsSL https://raw.githubusercontent.com/muvon/octocode/main/install.sh | sh -s -- --target x86_64-unknown-linux-musl

# Environment variables
export OCTOCODE_INSTALL_DIR=/opt/bin
curl -fsSL https://raw.githubusercontent.com/muvon/octocode/main/install.sh | sh
```

## Manual Installation

### Download Pre-built Binaries

Download the appropriate binary for your platform from the [latest release](https://github.com/muvon/octocode/releases/latest):

| Platform | Architecture | Download |
|----------|--------------|----------|
| Linux | x86_64 (static) | `octocode-VERSION-x86_64-unknown-linux-musl.tar.gz` |
| Linux | ARM64 (static) | `octocode-VERSION-aarch64-unknown-linux-musl.tar.gz` |
| Windows | x86_64 | `octocode-VERSION-x86_64-pc-windows-msvc.zip` |
| Windows | ARM64 | `octocode-VERSION-aarch64-pc-windows-msvc.zip` |
| macOS | x86_64 | `octocode-VERSION-x86_64-apple-darwin.tar.gz` |
| macOS | ARM64 | `octocode-VERSION-aarch64-apple-darwin.tar.gz` |

### Extract and Install

#### Unix/Linux/macOS
```bash
# Extract the archive
tar xzf octocode-VERSION-TARGET.tar.gz

# Move to a directory in your PATH
mv octocode ~/.local/bin/
# or
sudo mv octocode /usr/local/bin/
```

#### Windows
```bash
# In Git Bash, WSL, or MSYS2
tar xzf octocode-VERSION-x86_64-pc-windows-gnu.zip

# Move to a directory in your PATH
mv octocode.exe ~/.local/bin/
# or copy to Windows PATH directory
cp octocode.exe /c/Windows/System32/  # (requires admin)
```

## Install from Source

### Prerequisites
- Rust 1.87.0 or later
- Protocol Buffers compiler (`protoc`)

### Install protoc

#### Ubuntu/Debian
```bash
sudo apt-get install protobuf-compiler
```

#### macOS
```bash
brew install protobuf
```

#### Windows
```bash
# In Git Bash or WSL
choco install protoc
# or download from: https://github.com/protocolbuffers/protobuf/releases
```

### Build and Install
```bash
# Clone the repository
git clone https://github.com/muvon/octocode.git
cd octocode

# Build and install
cargo install --path .
```

## Using Cargo

If you have Rust installed, you can install directly from crates.io:

```bash
cargo install octocode
```

## Verify Installation

```bash
octocode --version
```

## Package Managers

### Homebrew (macOS)
```bash
# Coming soon
brew install octocode
```

### Chocolatey (Windows)
```bash
# In Git Bash, PowerShell, or Command Prompt
# Coming soon
choco install octocode
```

### Scoop (Windows)
```bash
# In PowerShell or Command Prompt
# Coming soon
scoop install octocode
```

## Custom Installation Options

### Install Script Options

The installation script supports several options and works on all platforms:

```bash
# Install specific version
curl -fsSL https://raw.githubusercontent.com/muvon/octocode/main/install.sh | sh -s -- --version 0.1.0

# Install to custom directory
curl -fsSL https://raw.githubusercontent.com/muvon/octocode/main/install.sh | sh -s -- --install-dir /usr/local/bin

# Install for specific target
curl -fsSL https://raw.githubusercontent.com/muvon/octocode/main/install.sh | sh -s -- --target x86_64-unknown-linux-musl

# Show help
curl -fsSL https://raw.githubusercontent.com/muvon/octocode/main/install.sh | sh -s -- --help
```

### Environment Variables

- `OCTOCODE_INSTALL_DIR`: Override default installation directory
- `OCTOCODE_VERSION`: Override version to install

Example:
```bash
export OCTOCODE_INSTALL_DIR=/opt/octocode/bin
curl -fsSL https://raw.githubusercontent.com/muvon/octocode/main/install.sh | sh
```

## Troubleshooting

### Permission Denied
If you get permission denied errors, make sure the installation directory is writable or use `sudo`:

```bash
sudo curl -fsSL https://raw.githubusercontent.com/muvon/octocode/main/install.sh | sh -s -- --install-dir /usr/local/bin
```

### Binary Not in PATH
If the binary is not found after installation, add the installation directory to your PATH:

```bash
# For bash/zsh
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc

# For fish
echo 'set -gx PATH $HOME/.local/bin $PATH' >> ~/.config/fish/config.fish
```

### musl vs glibc (Linux)
- Use `x86_64-unknown-linux-musl` for maximum compatibility (static linking)
- Use `x86_64-unknown-linux-gnu` if you need glibc-specific features

### macOS Gatekeeper
If macOS prevents running the binary, you may need to:

```bash
# Remove quarantine attribute
xattr -d com.apple.quarantine /path/to/octocode

# Or allow in System Preferences > Security & Privacy
```