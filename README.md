# 🎬 Audio Fixer & MP4 Converter (qBittorrent Integrated)

A high-performance Rust application designed for standalone use or direct integration with **qBittorrent**. 

It inspects completed torrent downloads, downmixes 5.1 surround sound to **4.1 surround audio**, ensures all video files are converted to **`.mp4` format**, and daemonizes in the background so qBittorrent never freezes or waits.

---

## ✨ Features

- **qBittorrent Integration (`%F` Token)**: Handles both single video files (e.g. `/downloads/Movie.mkv`) and multi-file torrent directories (e.g. `/downloads/Season 01/`).
- **Instant Background Daemonization (`--detach` / `-d`)**: Immediately forks a background worker process and exits with status `0` in `<5ms`, ensuring qBittorrent's internal event loop never blocks or freezes.
- **File Lock & Missing Path Retry Loop**: Includes exponential backoff retries to handle qBittorrent pre-allocation, file moving, or disk locking cleanly.
- **Universal MP4 Container Conversion**: 
  - **5.1 Audio Videos**: Downmixed to 4.1 AAC audio + remuxed/converted to `.mp4`.
  - **Non-5.1 Videos (`.mkv`, `.avi`, `.mov`, `.wmv`, etc.)**: Remuxed to `.mp4` format while preserving existing audio and video quality.
  - **Already `.mp4` & Non-5.1**: Skipped to avoid redundant processing.
- **Zero-Loss Video Passthrough**: Uses `-c:v copy` by default to preserve 100% of original video framerate, resolution, and quality.
- **Source Cleanup (`--delete-source`)**: Optionally removes the original `.mkv` or `.avi` file after successful conversion to `.mp4`.

---

## ⚙️ Setting Up with qBittorrent

In **qBittorrent**:

1. Go to **Preferences** -> **Downloads** -> **Run external program on torrent completion**.
2. Enable the checkbox and paste the following command:

```bash
/path/to/audio-fixer -d "%F"
```

*(Replace `/path/to/audio-fixer` with the absolute path to your binary, e.g. `/Users/liam/Documents/repo/audio-fixer/target/release/audio-fixer`)*

### Optional qBittorrent Command Variants:

- **Delete original source file after conversion**:
  ```bash
  /path/to/audio-fixer -d --delete-source "%F"
  ```

- **Specify custom log file location**:
  ```bash
  /path/to/audio-fixer -d -l /var/log/audio-fixer.log "%F"
  ```

- **Output converted files to a specific destination folder**:
  ```bash
  /path/to/audio-fixer -d -o /path/to/media/library "%F"
  ```

---

## 📖 Command-Line Interface

```text
Usage: audio-fixer [OPTIONS] <INPUTS>...

Arguments:
  <INPUTS>...  Input video files or directories (passed as %F by qBittorrent)

Options:
  -d, --detach               Run process in background daemon mode for qBittorrent
  -l, --log-file <PATH>      Log file path for detached background mode
      --delete-source        Delete original file after successful conversion to MP4
  -o, --output-dir <DIR>     Output directory for converted MP4 files
  -s, --suffix <SUFFIX>      Suffix appended to output MP4 filenames (default: "")
  -w, --overwrite            Overwrite existing output MP4 files
  -r, --recursive            Scan directories recursively (default: true)
  -e, --ext <EXT>            Video extensions to scan (default: avi,mkv,mp4,mov,wmv,flv,webm,m4v,ts,mts)
  -b, --audio-bitrate <BIT>  Audio bitrate for 4.1 AAC output (default: "384k")
      --max-retries <INT>    Max retries for locked files or missing paths (default: 5)
      --retry-delay-ms <INT> Initial backoff delay in ms (default: 1000)
      --dry-run              Inspect files and report without converting
      --force-video-copy     Fail if video stream copy fails (disable auto-transcode fallback)
      --transcode-video      Force H.264 video transcode (CRF 18) instead of stream copy
  -h, --help                 Print help information
  -V, --version              Print version information
```

---

## 🧪 Running Tests

```bash
cargo test
```
