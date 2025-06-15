#!/bin/bash

set -e

echo "=== Rust-Analyzer LSP Protocol Test ==="
echo

# Check if rust-analyzer is available
if ! command -v rust-analyzer &> /dev/null; then
    echo "❌ rust-analyzer not found. Please install it first:"
    echo "   rustup component add rust-analyzer"
    exit 1
fi

echo "✅ rust-analyzer found"
echo "📁 Working directory: $(pwd)"
echo

# Step 1: Create the initialize message
echo "📝 Step 1: Creating initialize message..."

INIT_MSG='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"capabilities":{},"workspaceFolders":[{"uri":"file://'$(pwd)'","name":"test"}]}}'

echo "Initialize message:"
echo "$INIT_MSG"
echo

# Calculate exact length
INIT_LEN=$(echo -n "$INIT_MSG" | wc -c | tr -d ' ')
echo "📏 Message length: $INIT_LEN bytes"
echo

# Step 2: Create the initialized notification
echo "📝 Step 2: Creating initialized notification..."

INITIALIZED_MSG='{"jsonrpc":"2.0","method":"initialized","params":{}}'

echo "Initialized message:"
echo "$INITIALIZED_MSG"
echo

INITIALIZED_LEN=$(echo -n "$INITIALIZED_MSG" | wc -c | tr -d ' ')
echo "📏 Message length: $INITIALIZED_LEN bytes"
echo

# Step 3: Create properly formatted LSP file
echo "📝 Step 3: Creating LSP protocol file..."

{
    echo -n "Content-Length: $INIT_LEN"
    echo -ne "\r\n\r\n"
    echo -n "$INIT_MSG"
    echo -n "Content-Length: $INITIALIZED_LEN"
    echo -ne "\r\n\r\n"
    echo -n "$INITIALIZED_MSG"
} > lsp_messages.bin

echo "✅ LSP messages file created"

# Step 4: Show the raw file content for debugging
echo
echo "📋 Step 4: Raw file content (hexdump first 200 bytes):"
hexdump -C lsp_messages.bin | head -10

echo
echo "📋 File size: $(wc -c < lsp_messages.bin) bytes"

# Step 5: Test with rust-analyzer
echo
echo "🚀 Step 5: Testing with rust-analyzer..."

timeout 10s rust-analyzer < lsp_messages.bin > ra_output.json 2> ra_error.log || {
    echo "⚠️  rust-analyzer test failed or timed out"
    echo
    echo "❌ Error output:"
    cat ra_error.log
    echo
    echo "📤 Output (if any):"
    cat ra_output.json
    echo
    exit 1
}

echo "✅ rust-analyzer completed without errors"

# Step 6: Analyze the response
echo
echo "📊 Step 6: Analyzing response..."

if [ -s ra_output.json ]; then
    echo "✅ rust-analyzer responded with output"
    echo
    echo "📤 Response content:"
    cat ra_output.json
    echo
    
    # Check if it's valid JSON
    if jq . ra_output.json > /dev/null 2>&1; then
        echo "✅ Response is valid JSON"
        
        # Check for initialize response
        if jq -e '.result.capabilities' ra_output.json > /dev/null 2>&1; then
            echo "✅ Found initialize response with capabilities"
        else
            echo "❌ No capabilities found in response"
        fi
    else
        echo "❌ Response is not valid JSON"
    fi
else
    echo "❌ No response from rust-analyzer"
fi

echo
echo "🧹 Cleanup..."
rm -f lsp_messages.bin ra_output.json ra_error.log

echo "✅ Test complete"