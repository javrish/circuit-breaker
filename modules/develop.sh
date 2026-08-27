#!/usr/bin/env bash
#
# Initialize Dagger SDK for all modules in this directory
#
# Usage: ./develop.sh [module...]
#   No args: Initialize all modules
#   With args: Initialize only specified modules
#
# Examples:
#   ./develop.sh          # All modules
#   ./develop.sh vcs      # Just vcs
#   ./develop.sh lint policy  # Just lint and policy

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

modules=()

if [[ $# -gt 0 ]]; then
    modules=("$@")
else
    for dir in */; do
        [[ -f "${dir}dagger.json" ]] && modules+=("${dir%/}")
    done
fi

if [[ ${#modules[@]} -eq 0 ]]; then
    echo "No modules found with dagger.json"
    exit 1
fi

echo "Initializing ${#modules[@]} module(s): ${modules[*]}"
echo

for module in "${modules[@]}"; do
    if [[ ! -d "$module" ]]; then
        echo "❌ Module not found: $module"
        continue
    fi

    if [[ ! -f "$module/dagger.json" ]]; then
        echo "❌ No dagger.json in: $module"
        continue
    fi

    echo "→ $module"
    (cd "$module" && dagger develop)
    echo "✓ $module"
    echo
done

echo "Done!"
