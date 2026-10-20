#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
exec /mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe \
    -NoProfile -ExecutionPolicy Bypass \
    -File "$(wslpath -w "$project_dir/scripts/build-windows.ps1")" "$@"
