# MCP Client Setup Guide

Complete setup instructions for connecting Octocode to popular AI assistants via the Model Context Protocol (MCP).

## What You Get

When connected, your AI assistant can:
- **Search code by meaning** — "Find authentication code" returns relevant functions, not just keyword matches
- **Navigate dependencies** — "What files depend on payment.rs?" queries the knowledge graph
- **View code structure** — Extract signatures, classes, and imports without reading entire files
- **Search by pattern** — Find `.unwrap()` calls, `new` instantiations, specific AST patterns

## Available Tools

| Tool | Description |
|------|-------------|
| `semantic_search` | Search codebase by meaning — finds code by what it does, not exact symbol names |
| `view_signatures` | Extract function signatures, class definitions, and declarations from files |
| `graphrag` | Query code relationships, dependencies, and architecture |
| `structural_search` | Search or rewrite code by AST structure using ast-grep pattern syntax |

---

## Octomind (Recommended)

[Octomind](https://octomind.run) is our companion AI agent runtime. It comes with Octocode pre-configured — zero setup required.

```bash
# Install Octomind
curl -fsSL https://raw.githubusercontent.com/muvon/octomind/master/install.sh | bash

# Run with Octocode pre-wired
octomind run developer:rust
```

Octomind automatically:
- ✅ Installs and configures Octocode
- ✅ Manages API keys (asks once, stores permanently)
- ✅ Spins up the MCP server for your project
- ✅ Loads specialist prompts for your domain

No config files to edit. No MCP setup. Just run and start coding.

---

## Claude Code (CLI)

Claude Code is Anthropic's command-line AI assistant. It supports MCP via the `/mcp` command.

### Command-Line Setup (Recommended)

```bash
# Add Octocode to Claude Code
claude mcp add octocode -- octocode mcp --path /path/to/your/project

# Verify connection
claude
> /mcp
# You should see octocode in the list
```

### Manual Config Setup

If you prefer editing the config file directly:

**Config location:** `~/.claude.json`

```json
{
  "mcpServers": {
    "octocode": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/your/project"]
    }
  }
}
```

### Usage

```bash
claude
> /mcp
# Lists available MCP servers

> Search my codebase for authentication logic
# Claude uses semantic_search automatically

> What files depend on src/payment/mod.rs?
# Claude uses graphrag to query dependencies
```

---

## Claude Desktop

Claude Desktop is Anthropic's GUI application for macOS and Windows.

### Setup

**macOS config:** `~/Library/Application Support/Claude/claude_desktop_config.json`

**Windows config:** `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "octocode": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/your/project"]
    }
  }
}
```

### Multiple Projects

```json
{
  "mcpServers": {
    "octocode-rust": {
      "command": "octocode",
      "args": ["mcp", "--path", "/projects/rust-app"]
    },
    "octocode-python": {
      "command": "octocode",
      "args": ["mcp", "--path", "/projects/python-api"]
    }
  }
}
```

### Verify Connection

1. Restart Claude Desktop after editing config
2. Look for the 🔨 icon in the bottom-right corner
3. Click it to see available tools

---

## Cursor

Cursor is an AI-powered code editor (VS Code fork) with native MCP support.

### Setup

**Method 1: Settings UI (Easiest)**

1. Open Cursor Settings (`Cmd+,` on macOS, `Ctrl+,` on Windows/Linux)
2. Navigate to **MCP Servers**
3. Click **Add New Global MCP Server**
4. Paste the config:

```json
{
  "mcpServers": {
    "octocode": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/your/project"]
    }
  }
}
```

**Method 2: Config File**

**Config location:** `~/.cursor/mcp.json` (global) or `.cursor/mcp.json` (project)

```json
{
  "mcpServers": {
    "octocode": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/your/project"]
    }
  }
}
```

### Usage

Restart Cursor after adding the config. The Octocode tools will be available in the AI chat.

---

## Windsurf

Windsurf is an AI-powered IDE from Codeium with MCP support.

### Setup

1. Open Windsurf Settings (`Cmd+,` on macOS, `Ctrl+,` on Windows/Linux)
2. Navigate to **MCP** section
3. Add new MCP server configuration:

```json
{
  "mcpServers": {
    "octocode": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/your/project"]
    }
  }
}
```

### Project-Specific Config

Create `.windsurf/mcp.json` in your project root:

```json
{
  "mcpServers": {
    "octocode": {
      "command": "octocode",
      "args": ["mcp", "--path", "."]
    }
  }
}
```

---

## VS Code (with Cline or Continue)

VS Code supports MCP through extensions like Cline and Continue.dev.

### Cline Extension

1. Install [Cline](https://marketplace.visualstudio.com/items?itemName=saoudrizwan.claude-dev) from VS Code marketplace
2. Open Cline sidebar
3. Click the MCP icon in the header
4. Add server configuration:

```json
{
  "mcpServers": {
    "octocode": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/your/project"]
    }
  }
}
```

### Continue.dev Extension

1. Install [Continue](https://continue.dev/) extension
2. Open Continue sidebar
3. Click the gear icon → **Edit config.json**
4. Add MCP server:

```json
{
  "mcpServers": {
    "octocode": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/your/project"]
    }
  }
}
```

---

## Zed Editor

Zed is a high-performance editor with built-in MCP support.

### Setup

**Config location:** `~/.config/zed/settings.json`

```json
{
  "mcp_servers": {
    "octocode": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/your/project"]
    }
  }
}
```

---

<details>
<summary><strong>📦 More MCP Clients</strong></summary>

### Replit

Replit supports MCP in AI-assisted coding mode.

1. Open your Repl
2. Go to **AI Settings**
3. Add MCP server configuration:

```json
{
  "mcpServers": {
    "octocode": {
      "command": "octocode",
      "args": ["mcp", "--path", "."]
    }
  }
}
```

### Amazon Q

Amazon Q Developer supports MCP in IDE extensions.

1. Open Amazon Q settings
2. Navigate to **MCP Servers**
3. Add configuration:

```json
{
  "mcpServers": {
    "octocode": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/your/project"]
    }
  }
}
```

### Sourcegraph Cody

Cody supports MCP in VS Code and JetBrains extensions.

1. Open Cody settings
2. Navigate to **MCP** section
3. Add server configuration

### Goose

Goose is a CLI AI assistant with MCP support.

```bash
# Add Octocode to Goose
goose mcp add octocode -- octocode mcp --path /path/to/your/project
```

### Gemini Code Assist

Google's Gemini Code Assist supports MCP.

1. Open Gemini Code Assist settings
2. Navigate to **MCP Servers**
3. Add configuration

### Gemini CLI

Google's Gemini CLI supports MCP.

```bash
# Add Octocode
gemini mcp add octocode -- octocode mcp --path /path/to/your/project
```

### Open WebUI

Open WebUI is an open-source web interface for LLMs with MCP support.

1. Open Settings → **MCP Servers**
2. Add configuration:

```json
{
  "mcpServers": {
    "octocode": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/your/project"]
    }
  }
}
```

### Jan AI

Jan is a local AI assistant with MCP support.

1. Open Jan settings
2. Navigate to **MCP Servers**
3. Add server configuration

### Raycast

Raycast is a productivity launcher with AI features and MCP support.

1. Open Raycast AI settings
2. Navigate to **MCP Servers**
3. Add configuration

</details>

---

## HTTP Mode (Custom Integrations)

For web-based integrations or testing, Octocode can run as an HTTP server:

```bash
# Start HTTP server
octocode mcp --bind 0.0.0.0:12345 --path /path/to/your/project

# Now accessible at http://localhost:12345
```

### HTTP API

Send JSON-RPC requests to the HTTP endpoint:

```bash
curl -X POST http://localhost:12345 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "tools/call",
    "params": {
      "name": "semantic_search",
      "arguments": {
        "query": "authentication",
        "mode": "code",
        "max_results": 5
      }
    }
  }'
```

---

## Troubleshooting

### MCP Server Not Appearing

**Symptom:** The Octocode server doesn't show up in your MCP client.

**Solutions:**
1. Verify Octocode is installed: `octocode --version`
2. Check the path in your config is absolute (not relative)
3. Restart your MCP client after config changes
4. Check logs for errors

### Tools Not Working

**Symptom:** MCP server appears but tools return errors.

**Solutions:**
1. Ensure your project is indexed: `cd /your/project && octocode index`
2. Check API keys are set: `echo $VOYAGE_API_KEY`
3. Verify the project path in MCP config matches your indexed project

### Permission Errors

**Symptom:** "Permission denied" or "Command not found" errors.

**Solutions:**
1. Ensure `octocode` is in your PATH: `which octocode`
2. Use absolute path in config: `"command": "/usr/local/bin/octocode"`
3. Check file permissions: `ls -la $(which octocode)`

### Multiple Projects

**Symptom:** Want to use Octocode with multiple projects.

**Solution:** Add multiple server entries with different names:

```json
{
  "mcpServers": {
    "octocode-work": {
      "command": "octocode",
      "args": ["mcp", "--path", "/home/user/work-project"]
    },
    "octocode-personal": {
      "command": "octocode",
      "args": ["mcp", "--path", "/home/user/personal-project"]
    }
  }
}
```

### Debug Mode

Enable debug logging to troubleshoot issues:

```bash
octocode mcp --path /your/project --debug
```

This logs all MCP requests and responses to help diagnose problems.

---

## Environment Variables

Octocode respects these environment variables:

```bash
# Required: Embedding provider API key
export VOYAGE_API_KEY="your-key"        # Default provider
export OPENAI_API_KEY="your-key"        # Alternative
export JINA_API_KEY="your-key"          # Alternative
export GOOGLE_API_KEY="your-key"        # Alternative

# Optional: LLM provider for advanced features
export OPENROUTER_API_KEY="your-key"    # Multi-provider LLM
export ANTHROPIC_API_KEY="your-key"     # Claude models
export DEEPSEEK_API_KEY="your-key"      # DeepSeek models
```

Set these in your shell profile (`~/.bashrc`, `~/.zshrc`) or MCP client config:

```json
{
  "mcpServers": {
    "octocode": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/project"],
      "env": {
        "VOYAGE_API_KEY": "your-key"
      }
    }
  }
}
```

---

## Next Steps

- **[Commands Reference](COMMANDS.md)** — Learn all Octocode CLI commands
- **[Configuration](CONFIGURATION.md)** — Customize indexing and search behavior
- **[Architecture](ARCHITECTURE.md)** — Understand how Octocode works under the hood
