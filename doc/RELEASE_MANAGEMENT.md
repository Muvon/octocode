# Release Management Guide

Complete guide to Octocode's AI-powered release management system with automatic version calculation and changelog generation.

## Overview

Octocode provides intelligent release automation that analyzes your commit history using conventional commits to determine appropriate semantic version bumps and generates structured changelogs. It supports multiple project types and integrates seamlessly with your git workflow.

## Key Features

- **AI Version Calculation**: Analyzes commit history to determine semantic version bumps
- **Automatic Changelog**: Generates structured changelogs from commit messages
- **Multi-Project Support**: Works with Rust, Node.js, PHP, and Go projects
- **Git Integration**: Creates release commits and annotated tags automatically
- **Dry Run Mode**: Preview changes before execution
- **Conventional Commits**: Supports conventional commit format for precise version calculation

## Quick Start

### Basic Release Workflow

```bash
# 1. Preview what would be done (recommended first step)
octocode release --dry-run

# 2. Create the release
octocode release

# 3. Push to remote
git push origin main --tags
```

### With Confirmation

```bash
# Skip confirmation prompt for automation
octocode release --yes

# Force a specific version (bypasses AI calculation)
octocode release --force-version "2.0.0"

# Use custom changelog file
octocode release --changelog "HISTORY.md"
```

## Supported Project Types

### Rust Projects (Cargo.toml)

```toml
[package]
name = "my-project"
version = "0.1.0"  # Updated automatically
```

**Files updated:**
- `Cargo.toml` - Package version
- `CHANGELOG.md` - Release notes

### Node.js Projects (package.json)

```json
{
  "name": "my-project",
  "version": "0.1.0"
}
```

**Files updated:**
- `package.json` - Package version
- `CHANGELOG.md` - Release notes

### PHP Projects (composer.json)

```json
{
  "name": "vendor/my-project",
  "version": "0.1.0"
}
```

**Files updated:**
- `composer.json` - Package version
- `CHANGELOG.md` - Release notes

### Go Projects (go.mod)

```go
module github.com/user/my-project

go 1.21
```

**Files updated:**
- `VERSION` file - Version string
- `CHANGELOG.md` - Release notes

**Note**: Go projects use a `VERSION` file since `go.mod` doesn't contain version information.

## How It Works

### 1. Project Detection

Octocode automatically detects your project type by scanning for:
- `Cargo.toml` → Rust project
- `package.json` → Node.js project
- `composer.json` → PHP project
- `go.mod` → Go project

### 2. Version Analysis

Extracts current version from:
- Project files (Cargo.toml, package.json, etc.)
- Git tags (if no version in project files)
- Defaults to 0.1.0 for new projects

### 3. Commit Analysis

Analyzes commits since the last release using:
- Git log between current HEAD and last version tag
- Conventional commit format parsing
- Commit message categorization

### 4. AI Version Calculation

Uses LLM to determine appropriate version bump based on:
- Conventional commit types
- Breaking change indicators
- Commit message content
- Project context

### 5. Changelog Generation

Creates structured changelog with:
- Categorized changes (Features, Bug Fixes, etc.)
- Commit references
- Breaking change highlights
- Release date

### 6. File Updates and Git Operations

- Updates project version files
- Adds changelog entry
- Creates release commit
- Creates annotated git tag

## Conventional Commits Support

### Commit Types and Version Bumps

| Commit Type | Version Bump | Example |
|-------------|--------------|---------|
| `feat:` | Minor (0.1.0 → 0.2.0) | `feat: add user authentication` |
| `fix:` | Patch (0.1.0 → 0.1.1) | `fix: resolve login timeout issue` |
| `BREAKING CHANGE` | Major (0.1.0 → 1.0.0) | `feat!: redesign API endpoints` |
| `chore:` | Patch | `chore: update dependencies` |
| `docs:` | Patch | `docs: update API documentation` |
| `style:` | Patch | `style: fix code formatting` |
| `refactor:` | Patch | `refactor: simplify auth logic` |
| `test:` | Patch | `test: add integration tests` |
| `perf:` | Patch | `perf: optimize database queries` |

### Breaking Changes

Breaking changes trigger major version bumps:

```bash
# Using exclamation mark
feat!: redesign user API endpoints

# Using BREAKING CHANGE footer
feat: add new authentication system

BREAKING CHANGE: The old auth endpoints have been removed
```

### Scoped Commits

Scopes help organize changelog entries:

```bash
feat(auth): add OAuth2 support
fix(database): resolve connection pool issues
docs(api): update endpoint documentation
```

## Command Options

### Basic Commands

```bash
# Preview release (no changes made)
octocode release --dry-run

# Create release with prompts
octocode release

# Create release without prompts
octocode release --yes
```

### Version Control

```bash
# Force specific version (bypasses AI calculation)
octocode release --force-version "2.0.0"
octocode release --force-version "1.5.0-beta.1"

# Specify version type manually
octocode release --version-type major
octocode release --version-type minor
octocode release --version-type patch
```

### Changelog Options

```bash
# Use custom changelog file
octocode release --changelog "HISTORY.md"
octocode release --changelog "docs/RELEASES.md"

# Skip changelog generation
octocode release --no-changelog
```

### Git Options

```bash
# Custom commit message
octocode release --commit-message "Release v{version}"

# Custom tag format
octocode release --tag-format "v{version}"
octocode release --tag-format "release-{version}"

# Skip git tag creation
octocode release --no-tag
```

## Example Workflows

### Standard Development Workflow

```bash
# 1. Development with conventional commits
git add .
octocode commit  # AI generates conventional commit

# 2. More development...
git add .
octocode commit

# 3. Ready for release
octocode release --dry-run  # Preview
octocode release            # Create release

# 4. Deploy
git push origin main --tags
```

### Pre-release Workflow

```bash
# Create beta release
octocode release --force-version "1.0.0-beta.1"

# Test the beta...

# Create release candidate
octocode release --force-version "1.0.0-rc.1"

# Final release
octocode release --force-version "1.0.0"
```

### Hotfix Workflow

```bash
# On main branch, fix critical bug
git add .
octocode commit  # AI generates "fix: critical security vulnerability"

# Create patch release
octocode release  # AI determines patch bump (e.g., 1.0.0 → 1.0.1)
```

### Feature Release Workflow

```bash
# Develop features with conventional commits
git add .
octocode commit  # "feat: add user dashboard"

git add .
octocode commit  # "feat: add data export functionality"

git add .
octocode commit  # "fix: resolve dashboard loading issue"

# Create minor release
octocode release  # AI determines minor bump (e.g., 1.0.0 → 1.1.0)
```

## Changelog Format

### Generated Changelog Structure

```markdown
# Changelog

## [1.2.0] - 2025-01-27

### ✨ Features

- Add user dashboard with analytics
- Implement data export functionality
- Add OAuth2 authentication support

### 🐛 Bug Fixes

- Fix dashboard loading timeout issue
- Resolve authentication token refresh bug
- Fix data export CSV formatting

### 📚 Documentation

- Update API documentation
- Add user guide for new dashboard

### 🔧 Maintenance

- Update dependencies to latest versions
- Improve test coverage
- Refactor authentication module

### ⚠️ Breaking Changes

- Removed deprecated `/api/v1/auth` endpoint
- Changed user profile data structure

## [1.1.0] - 2025-01-15

...
```

### Customizing Changelog Format

```bash
# Use custom changelog template
octocode release --changelog-template "templates/release.md"

# Custom section headers
octocode config --changelog-sections "Features,Fixes,Changes"
```

## Integration with CI/CD

### GitHub Actions

```yaml
name: Release
on:
  push:
    branches: [main]
    paths-ignore: ['CHANGELOG.md']

jobs:
  release:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
        with:
          fetch-depth: 0  # Need full history for release analysis

      - name: Install Octocode
        run: curl -fsSL https://raw.githubusercontent.com/muvon/octocode/main/install.sh | sh

      - name: Create Release
        env:
          OPENROUTER_API_KEY: ${{ secrets.OPENROUTER_API_KEY }}
        run: |
          octocode release --yes
          git push origin main --tags
```

### GitLab CI

```yaml
release:
  stage: release
  script:
    - curl -fsSL https://raw.githubusercontent.com/muvon/octocode/main/install.sh | sh
    - octocode release --yes
    - git push origin main --tags
  only:
    - main
  variables:
    OPENROUTER_API_KEY: $OPENROUTER_API_KEY
```

### Manual Automation

```bash
#!/bin/bash
# release.sh - Automated release script

set -e

echo "🚀 Starting release process..."

# Ensure we're on main branch
git checkout main
git pull origin main

# Run tests
echo "🧪 Running tests..."
cargo test  # or npm test, composer test, go test

# Create release
echo "📦 Creating release..."
octocode release --dry-run
read -p "Continue with release? (y/N) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    octocode release --yes
    git push origin main --tags
    echo "✅ Release complete!"
else
    echo "❌ Release cancelled"
    exit 1
fi
```

## Configuration

### Release Configuration

```bash
# Set default changelog file
octocode config --changelog-file "RELEASES.md"

# Set default commit message format
octocode config --release-commit-format "chore: release v{version}"

# Set default tag format
octocode config --release-tag-format "v{version}"
```

### Configuration File

```toml
[release]
changelog_file = "CHANGELOG.md"
commit_message = "chore: release v{version}"
tag_format = "v{version}"
auto_push = false
require_conventional_commits = true

[openrouter]
model = "openai/gpt-4o-mini"  # Model for version calculation
```

## Advanced Features

### Pre-release Hooks

```bash
# Run tests before release
octocode release --pre-hook "cargo test"
octocode release --pre-hook "npm run test"

# Multiple hooks
octocode release --pre-hook "cargo test" --pre-hook "cargo clippy"
```

### Post-release Hooks

```bash
# Deploy after release
octocode release --post-hook "deploy.sh"

# Notify team
octocode release --post-hook "notify-team.sh"
```

### Custom Version Calculation

```bash
# Use different model for version calculation
octocode config --release-model "anthropic/claude-3.5-sonnet"

# Custom version calculation prompt
octocode release --version-prompt "Calculate version based on semantic changes"
```

## Troubleshooting

### Version Calculation Issues

**Problem**: AI suggests wrong version bump

**Solutions:**
1. Use `--force-version` to override
2. Improve commit message quality
3. Use conventional commit format consistently
4. Try different LLM model

### Git Integration Issues

**Problem**: Git operations fail

**Solutions:**
1. Ensure git repository is clean
2. Check git credentials and permissions
3. Verify branch permissions
4. Check for conflicting tags

### Project Detection Issues

**Problem**: Project type not detected

**Solutions:**
1. Ensure project files exist (Cargo.toml, package.json, etc.)
2. Run from project root directory
3. Check file permissions
4. Manually specify project type

### Changelog Generation Issues

**Problem**: Changelog format is incorrect

**Solutions:**
1. Check conventional commit format
2. Verify commit message quality
3. Use custom changelog template
4. Check LLM model configuration

## Best Practices

### Commit Message Quality

```bash
# Good conventional commits
feat(auth): add OAuth2 authentication support
fix(database): resolve connection pool timeout issue
docs(api): update authentication endpoint documentation

# Poor commits (avoid)
fix stuff
update code
changes
```

### Release Timing

1. **Regular Releases**: Schedule regular releases (weekly/monthly)
2. **Feature Releases**: Release when significant features are complete
3. **Hotfix Releases**: Release immediately for critical bugs
4. **Pre-releases**: Use for testing major changes

### Version Strategy

1. **Semantic Versioning**: Follow semver strictly
2. **Breaking Changes**: Clearly mark breaking changes
3. **Pre-releases**: Use for beta testing
4. **LTS Versions**: Consider long-term support versions

### Changelog Maintenance

1. **Clear Descriptions**: Write clear, user-focused descriptions
2. **Breaking Changes**: Highlight breaking changes prominently
3. **Migration Guides**: Include migration instructions for major versions
4. **Regular Updates**: Keep changelog up to date with each release

For more information, see:
- [Commands Reference](COMMANDS.md) - Complete command documentation
- [Advanced Usage](ADVANCED_USAGE.md) - Advanced workflows
- [Configuration](CONFIGURATION.md) - Configuration options
