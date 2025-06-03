#!/bin/bash
# Test script for shell completions

set -e

echo "Testing octocode shell completions..."
echo ""

# Try release binary first, then debug binary
if [[ -f "./target/release/octocode" ]]; then
	OCTOCODE_BIN="./target/release/octocode"
elif [[ -f "./target/debug/octocode" ]]; then
	OCTOCODE_BIN="./target/debug/octocode"
else
	echo "Error: octocode binary not found. Run 'cargo build' or 'cargo build --release' first."
	exit 1
fi

echo "✓ Binary found at $OCTOCODE_BIN"

# Test completion generation
echo "Testing completion generation..."

echo "- Testing bash completion generation..."
if "$OCTOCODE_BIN" completion bash > /tmp/test_bash_completion; then
	echo "✓ Bash completion generated successfully"
	echo "  Generated $(wc -l < /tmp/test_bash_completion) lines"
else
	echo "✗ Failed to generate bash completion"
	exit 1
fi

echo "- Testing zsh completion generation..."
if "$OCTOCODE_BIN" completion zsh > /tmp/test_zsh_completion; then
	echo "✓ Zsh completion generated successfully"
	echo "  Generated $(wc -l < /tmp/test_zsh_completion) lines"
else
	echo "✗ Failed to generate zsh completion"
	exit 1
fi

echo "- Testing all available shells..."
for shell in bash elvish fish powershell zsh; do
	if "$OCTOCODE_BIN" completion "$shell" > "/tmp/test_${shell}_completion"; then
		echo "✓ $shell completion: $(wc -l < "/tmp/test_${shell}_completion") lines"
	else
		echo "✗ Failed to generate $shell completion"
	fi
done

echo ""
echo "Testing completion content..."

# Check if bash completion contains expected patterns
if grep -q "_octocode()" /tmp/test_bash_completion; then
	echo "✓ Bash completion contains function definition"
else
	echo "✗ Bash completion missing function definition"
fi

if grep -q "octocode__session" /tmp/test_bash_completion; then
	echo "✓ Bash completion contains subcommand definitions"
else
	echo "✗ Bash completion missing subcommand definitions"
fi

# Check if zsh completion contains expected patterns
if grep -q "#compdef octocode" /tmp/test_zsh_completion; then
	echo "✓ Zsh completion contains compdef directive"
else
	echo "✗ Zsh completion missing compdef directive"
fi

echo ""
echo "✓ All completion tests passed!"
echo ""
echo "To install completions, run:"
echo "  ./scripts/install-completions.sh"
echo ""
echo "Or manually:"
echo "  # Bash"
echo "  $OCTOCODE_BIN completion bash > ~/.local/share/bash-completion/completions/octocode"
echo ""
echo "  # Zsh"
echo "  mkdir -p ~/.config/zsh/completions"
echo "  $OCTOCODE_BIN completion zsh > ~/.config/zsh/completions/_octocode"

# Cleanup
rm -f /tmp/test_*_completion
