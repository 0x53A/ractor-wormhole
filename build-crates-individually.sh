#!/usr/bin/env bash

# Enabled features may leak between crates in a workspace,
# this build script builds each crate individually
# to verify that they don't accidentally depend on features that aren't actually enabled for the specific crate.

set -euo pipefail

# Get the root directory
root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Find all Cargo.toml files but exclude the workspace root one
mapfile -t cargo_files < <(find "$root_dir" -name "Cargo.toml" -not -path "$root_dir/Cargo.toml" -not -path "*/target/*")

# Track failures
failed_builds=()
total_crates=${#cargo_files[@]}
success_count=0

# Process each Cargo.toml file
for cargo_file in "${cargo_files[@]}"; do
    crate_dir="$(dirname "$cargo_file")"
    crate_name="$(basename "$crate_dir")"

    echo -e "\033[36mBuilding crate: $crate_name in $crate_dir\033[0m"

    if (cd "$crate_dir" && cargo build); then
        echo -e "\033[32mSuccessfully built crate: $crate_name\033[0m"
        ((success_count++)) || true
    else
        echo -e "\033[31mFailed to build crate: $crate_name\033[0m"
        failed_builds+=("$crate_name|$crate_dir")
    fi

    echo "" # Empty line for readability
done

# Print summary report
echo -e "\033[36m===== BUILD SUMMARY =====\033[0m"
echo -e "\033[36mTotal crates: $total_crates\033[0m"
echo -e "\033[32mSuccessful builds: $success_count\033[0m"

if [ ${#failed_builds[@]} -gt 0 ]; then
    echo -e "\033[31mFailed builds: ${#failed_builds[@]}\033[0m"
    echo ""
    echo -e "\033[31mFAILED CRATES:\033[0m"

    for failure in "${failed_builds[@]}"; do
        name="${failure%%|*}"
        dir="${failure##*|}"
        echo -e "\033[31m  - $name\033[0m"
        echo -e "\033[31m    Directory: $dir\033[0m"
    done

    echo ""
    echo -e "\033[31mBuild process completed with errors.\033[0m"
    exit 1
else
    echo -e "\033[32mAll crates built successfully!\033[0m"
    exit 0
fi
