#!/bin/bash
LOG_FILE="$HOME/.audio-fixer/audio-fixer.log"
mkdir -p "$HOME/.audio-fixer"

echo "[$(date '+%Y-%m-%d %H:%M:%S')] ⚡ qBittorrent completion trigger fired for: $1" >> "$LOG_FILE"
/Users/liam/Documents/repo/audio-fixer/target/release/audio-fixer -d "$1" >> "$LOG_FILE" 2>&1
