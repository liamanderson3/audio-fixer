mod ffmpeg;
mod ffprobe;
mod logger;
mod scanner;

use anyhow::{anyhow, Result};
use clap::Parser;
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::env;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use ffmpeg::{convert_video_file, ConversionOptions};
use ffprobe::probe_file_with_retry;
use logger::Logger;
use scanner::{scan_inputs, DEFAULT_EXTENSIONS};

#[derive(Parser, Debug)]
#[command(
    name = "audio-fixer",
    author = "Antigravity AI",
    version = "0.1.0",
    about = "Checks video files (AVI, MKV, MP4, etc.) for 5.1 audio (downmixes to 4.1) and ensures all video files are converted to MP4 format while maintaining quality."
)]
pub struct Cli {
    /// Input video files or directories to scan (passed as %F by qBittorrent)
    #[arg(required = true, value_name = "INPUTS")]
    pub inputs: Vec<PathBuf>,

    /// Output directory for converted MP4 files (default: replace files in-place)
    #[arg(short = 'o', long = "output-dir", value_name = "DIR")]
    pub output_dir: Option<PathBuf>,

    /// Replace original files in-place (deletes source .mkv/.avi after .mp4 creation)
    #[arg(short = 'i', long = "in-place", default_value_t = true)]
    pub in_place: bool,

    /// Suffix appended to output MP4 filenames (e.g. "_4.1")
    #[arg(short = 's', long = "suffix", default_value = "")]
    pub suffix: String,

    /// Overwrite existing output MP4 files
    #[arg(short = 'w', long = "overwrite")]
    pub overwrite: bool,

    /// Scan directories recursively
    #[arg(short = 'r', long = "recursive", default_value_t = true)]
    pub recursive: bool,

    /// Video file extensions to scan (comma-separated, e.g. avi,mkv,mp4)
    #[arg(short = 'e', long = "ext", value_delimiter = ',')]
    pub extensions: Vec<String>,

    /// Audio bitrate for 4.1 AAC output (e.g., 384k, 448k, 512k)
    #[arg(short = 'b', long = "audio-bitrate", default_value = "384k")]
    pub audio_bitrate: String,

    /// Audio codec for output (aac, ac3)
    #[arg(long = "audio-codec", default_value = "aac")]
    pub audio_codec: String,

    /// Perform a dry run (scan & check audio without converting)
    #[arg(long = "dry-run")]
    pub dry_run: bool,

    /// Force video stream copy (error if video stream cannot be copied into MP4)
    #[arg(long = "force-video-copy")]
    pub force_video_copy: bool,

    /// Always transcode video to H.264 (CRF 18) instead of stream copy
    #[arg(long = "transcode-video")]
    pub transcode_video: bool,

    /// Run process in background (daemonized mode for qBittorrent integration)
    #[arg(short = 'd', long = "detach")]
    pub detach: bool,

    /// Internal flag used for background worker execution
    #[arg(long = "no-detach-internal", hide = true)]
    pub no_detach_internal: bool,

    /// Log file path (default: ~/.audio-fixer/audio-fixer.log)
    #[arg(short = 'l', long = "log-file")]
    pub log_file: Option<PathBuf>,

    /// Delete original source file after successful conversion to MP4
    #[arg(long = "delete-source")]
    pub delete_source: bool,

    /// Maximum retry attempts for locked files or missing paths
    #[arg(long = "max-retries", default_value_t = 5)]
    pub max_retries: u32,

    /// Initial delay in milliseconds between retries
    #[arg(long = "retry-delay-ms", default_value_t = 1000)]
    pub retry_delay_ms: u64,

    /// Custom path to ffmpeg executable
    #[arg(long = "ffmpeg-path", default_value = "ffmpeg")]
    pub ffmpeg_path: String,

    /// Custom path to ffprobe executable
    #[arg(long = "ffprobe-path", default_value = "ffprobe")]
    pub ffprobe_path: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize persistent Logger
    let logger = Logger::init(cli.log_file.clone());

    // Always record invocation details on startup
    let raw_args = env::args().collect::<Vec<_>>().join(" ");
    logger.log(&format!("🚀 Invoked audio-fixer with command line: {}", raw_args));

    // Handle daemonization / background execution if --detach is specified
    if cli.detach && !cli.no_detach_internal {
        return daemonize(&cli, &logger);
    }

    let start_time = Instant::now();

    let header_title = "🎬 Audio Fixer & MP4 Converter".bold().cyan().to_string();
    let header_line = "================================".cyan().to_string();
    println!("{}", header_title);
    println!("{}", header_line);
    logger.log(&header_title);
    logger.log(&header_line);

    // 1. Resolve & verify dependencies (handles Homebrew / GUI app PATH limits)
    let ffmpeg_bin = resolve_binary_path(&cli.ffmpeg_path);
    let ffprobe_bin = resolve_binary_path(&cli.ffprobe_path);

    if let Err(e) = verify_dependency(&ffmpeg_bin, "ffmpeg") {
        logger.log(&format!("❌ Dependency Error: {}", e));
        return Err(e);
    }

    if let Err(e) = verify_dependency(&ffprobe_bin, "ffprobe") {
        logger.log(&format!("❌ Dependency Error: {}", e));
        return Err(e);
    }

    // 2. Extensions setup
    let exts = if cli.extensions.is_empty() {
        DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect()
    } else {
        cli.extensions.clone()
    };

    // 3. Scan inputs
    let scan_msg = format!("🔍 Scanning inputs for video files ({})", exts.join(", ").yellow());
    println!("{}", scan_msg);
    logger.log(&scan_msg);

    let files = scan_inputs(&cli.inputs, &exts, cli.recursive);

    if files.is_empty() {
        let no_files_msg = "⚠️  No matching video files found.".yellow().to_string();
        println!("{}", no_files_msg);
        logger.log(&no_files_msg);
        return Ok(());
    }

    let found_msg = format!(
        "📁 Found {} video file(s) to analyze.\n",
        files.len().to_string().bold().green()
    );
    println!("{}", found_msg);
    logger.log(&found_msg);

    let is_in_place = cli.output_dir.is_none() && cli.in_place;
    let should_delete_source = cli.delete_source || is_in_place;
    let should_overwrite = cli.overwrite || is_in_place;

    let opts = ConversionOptions {
        audio_bitrate: cli.audio_bitrate.clone(),
        audio_codec: cli.audio_codec.clone(),
        ffmpeg_bin: ffmpeg_bin.clone(),
        overwrite: should_overwrite,
        force_video_copy: cli.force_video_copy,
        transcode_video: cli.transcode_video,
        delete_source: should_delete_source,
    };

    let pb = ProgressBar::new(files.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );

    let mut count_5_1 = 0;
    let mut count_converted = 0;
    let mut count_skipped = 0;
    let mut count_failed = 0;
    let mut errors = Vec::new();

    for file in &files {
        let display_name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(file.to_str().unwrap_or(""));

        pb.set_message(format!("Checking {}", display_name));

        match probe_file_with_retry(file, &ffprobe_bin, cli.max_retries, cli.retry_delay_ms) {
            Ok(probe) => {
                let has_5_1 = probe.has_5_1_audio();
                let is_mp4 = file
                    .extension()
                    .and_then(|e| e.to_str())
                    .map_or(false, |ext| ext.eq_ignore_ascii_case("mp4"));

                let out_path = compute_output_path(
                    file,
                    cli.output_dir.as_deref(),
                    &cli.suffix,
                    should_overwrite,
                );

                // Check if target output file already exists and is already converted
                if file != &out_path && out_path.exists() {
                    if is_target_already_converted(&out_path, &ffprobe_bin, cli.max_retries, cli.retry_delay_ms) {
                        count_skipped += 1;
                        let skip_line = format!(
                            "⏭️  {} - {} ('{}' already exists with converted audio/container)",
                            display_name,
                            "Skipped".yellow(),
                            out_path.file_name().and_then(|n| n.to_str()).unwrap_or("output.mp4")
                        );

                        pb.suspend(|| println!("{}", skip_line));
                        logger.log(&skip_line);

                        // If in-place replacement is active and target MP4 is already converted, clean up original source file
                        if should_delete_source && file.exists() {
                            let _ = std::fs::remove_file(file);
                        }

                        pb.inc(1);
                        continue;
                    }
                }

                let needs_conversion = has_5_1 || !is_mp4;

                if needs_conversion {
                    if has_5_1 {
                        count_5_1 += 1;
                    }

                    let audio_desc = probe
                        .streams
                        .iter()
                        .filter(|s| s.codec_type == "audio")
                        .map(|s| s.audio_description())
                        .collect::<Vec<_>>()
                        .join(", ");

                    let status_reason = match (has_5_1, is_mp4) {
                        (true, false) => "5.1 Surround & MP4 Conversion".bold().green(),
                        (true, true) => "5.1 Surround Downmix".bold().green(),
                        (false, false) => "Container Conversion to MP4".bold().blue(),
                        (false, true) => unreachable!(),
                    };

                    let line = format!(
                        "🔊 {} [{}] - {}",
                        display_name.bold(),
                        status_reason,
                        audio_desc.dimmed()
                    );

                    pb.suspend(|| println!("{}", line));
                    logger.log(&line);

                    if cli.dry_run {
                        let dry_line = format!(
                            "   {} (Dry Run)",
                            "[DRY RUN] Would convert to MP4".cyan()
                        );
                        pb.suspend(|| println!("{}", dry_line));
                        logger.log(&dry_line);
                        count_converted += 1;
                    } else {
                        pb.set_message(format!("Converting {}", display_name));

                        match convert_video_file(file, &out_path, &probe, &opts) {
                            Ok(res) => {
                                count_converted += 1;
                                let copy_str = if res.video_copied {
                                    "Stream Copy (Passthrough)".bold().green()
                                } else {
                                    "Transcoded H.264".yellow()
                                };

                                let audio_str = if res.audio_downmixed {
                                    format!("4.1 AAC {}", cli.audio_bitrate.bold())
                                } else {
                                    "AAC Passthrough/Encode".to_string()
                                };

                                let deleted_str = if should_delete_source && file != &res.output_path {
                                    " [Replaced In-Place]".green()
                                } else {
                                    "".into()
                                };

                                let out_line = format!(
                                    "   ✅ Output: {} (Video: {}, Audio: {}){}",
                                    res.output_path.display().to_string().cyan(),
                                    copy_str,
                                    audio_str,
                                    deleted_str
                                );

                                pb.suspend(|| println!("{}", out_line));
                                logger.log(&out_line);
                            }
                            Err(e) => {
                                count_failed += 1;
                                let err_msg = format!("Failed converting {}: {}", file.display(), e);
                                errors.push(err_msg.clone());
                                let err_line = format!("   ❌ Error: {}", e.to_string().red());
                                pb.suspend(|| println!("{}", err_line));
                                logger.log(&err_line);
                            }
                        }
                    }
                } else {
                    count_skipped += 1;
                    let all_audio_desc = probe
                        .streams
                        .iter()
                        .filter(|s| s.codec_type == "audio")
                        .map(|s| s.audio_description())
                        .collect::<Vec<_>>()
                        .join(", ");

                    let skip_line = format!(
                        "⏭️  {} - {} ({})",
                        display_name,
                        "Skipped (Already MP4 with non-5.1 audio)".yellow(),
                        if all_audio_desc.is_empty() {
                            "No audio stream".to_string()
                        } else {
                            all_audio_desc
                        }
                    );

                    pb.suspend(|| println!("{}", skip_line));
                    logger.log(&skip_line);
                }
            }
            Err(e) => {
                count_failed += 1;
                let err_msg = format!("Failed probing {}: {}", file.display(), e);
                errors.push(err_msg.clone());
                let err_line = format!("❌ Error probing {}: {}", display_name.bold(), e.to_string().red());
                pb.suspend(|| println!("{}", err_line));
                logger.log(&err_line);
            }
        }

        pb.inc(1);
    }

    pb.finish_and_clear();

    let duration = start_time.elapsed();

    // Summary report
    let sum_header = "\n📊 Summary Report".bold().cyan().to_string();
    let sum_div = "=================".cyan().to_string();
    let sum_scanned = format!("Total scanned files : {}", files.len());
    let sum_5_1 = format!("5.1 Audio files     : {}", count_5_1.to_string().bold().green());
    let sum_proc = format!(
        "Processed / Converted: {}",
        if cli.dry_run {
            format!("{} (Dry Run)", count_converted).cyan()
        } else {
            count_converted.to_string().bold().green()
        }
    );
    let sum_skipped = format!("Skipped             : {}", count_skipped.to_string().yellow());
    let sum_failed = format!(
        "Failures            : {}",
        if count_failed > 0 {
            count_failed.to_string().bold().red()
        } else {
            "0".bold().green()
        }
    );
    let sum_time = format!("Elapsed Time        : {:.2?}", duration);

    println!("{}", sum_header);
    println!("{}", sum_div);
    println!("{}", sum_scanned);
    println!("{}", sum_5_1);
    println!("{}", sum_proc);
    println!("{}", sum_skipped);
    println!("{}", sum_failed);
    println!("{}", sum_time);

    logger.log(&sum_header);
    logger.log(&sum_div);
    logger.log(&sum_scanned);
    logger.log(&sum_5_1);
    logger.log(&sum_proc);
    logger.log(&sum_skipped);
    logger.log(&sum_failed);
    logger.log(&sum_time);

    if !errors.is_empty() {
        let err_hdr = "\n⚠️  Errors Encountered:".bold().red().to_string();
        println!("{}", err_hdr);
        logger.log(&err_hdr);
        for err in &errors {
            let err_item = format!("  - {}", err.red());
            println!("{}", err_item);
            logger.log(&err_item);
        }
    }

    let done_msg = "\n✨ Done!".bold().green().to_string();
    println!("{}", done_msg);
    logger.log(&done_msg);
    logger.log(&format!("Log saved to {}", logger.log_path().display()));

    Ok(())
}

fn daemonize(_cli: &Cli, logger: &Logger) -> Result<()> {
    let current_exe = env::current_exe()?;
    let log_path = logger.log_path().to_path_buf();

    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| anyhow!("Failed to open log file {}: {}", log_path.display(), e))?;

    let mut args: Vec<String> = env::args().collect();
    args.retain(|a| a != "-d" && a != "--detach");
    args.push("--no-detach-internal".to_string());

    let child = Command::new(&current_exe)
        .args(&args[1..])
        .stdout(Stdio::from(log_file.try_clone()?))
        .stderr(Stdio::from(log_file))
        .spawn()
        .map_err(|e| anyhow!("Failed to spawn background worker process: {}", e))?;

    let msg = format!(
        "[audio-fixer] Launched background worker process (PID: {}). Logging to {}. Parent process exiting now.",
        child.id(),
        log_path.display()
    );

    println!("{}", msg);
    logger.log(&msg);

    Ok(())
}

fn is_target_already_converted(
    target_path: &Path,
    ffprobe_bin: &str,
    max_retries: u32,
    retry_delay_ms: u64,
) -> bool {
    if !target_path.exists() {
        return false;
    }

    match probe_file_with_retry(target_path, ffprobe_bin, max_retries, retry_delay_ms) {
        Ok(probe) => {
            let has_video = !probe.get_video_streams().is_empty();
            let has_5_1 = probe.has_5_1_audio();
            has_video && !has_5_1
        }
        Err(_) => false,
    }
}

fn resolve_binary_path(cmd: &str) -> String {
    if Command::new(cmd).arg("-version").output().map_or(false, |o| o.status.success()) {
        return cmd.to_string();
    }

    let candidate_paths = [
        format!("/opt/homebrew/bin/{}", cmd),
        format!("/usr/local/bin/{}", cmd),
        format!("/usr/bin/{}", cmd),
    ];

    for path_str in &candidate_paths {
        if Path::new(path_str).exists() {
            if Command::new(path_str).arg("-version").output().map_or(false, |o| o.status.success()) {
                return path_str.clone();
            }
        }
    }

    cmd.to_string()
}

fn verify_dependency(cmd: &str, name: &str) -> Result<()> {
    match Command::new(cmd).arg("-version").output() {
        Ok(out) if out.status.success() => Ok(()),
        _ => Err(anyhow!(
            "Required dependency '{}' was not found or failed to run using command '{}'. Please ensure {} is installed and accessible.",
            name, cmd, cmd
        )),
    }
}

fn compute_output_path(
    input: &Path,
    output_dir: Option<&Path>,
    suffix: &str,
    overwrite: bool,
) -> PathBuf {
    let file_stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let input_ext = input
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let target_dir = output_dir.map(PathBuf::from).unwrap_or_else(|| {
        input.parent().unwrap_or(Path::new(".")).to_path_buf()
    });

    let is_mp4 = input_ext == "mp4";

    let filename = if is_mp4 && suffix.is_empty() && !overwrite {
        format!("{}_4.1.mp4", file_stem)
    } else {
        format!("{}{}.mp4", file_stem, suffix)
    };

    target_dir.join(filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_output_path_avi_to_mp4() {
        let input = Path::new("/path/to/movie.avi");
        let out = compute_output_path(input, None, "", false);
        assert_eq!(out, PathBuf::from("/path/to/movie.mp4"));
    }

    #[test]
    fn test_compute_output_path_mkv_with_output_dir() {
        let input = Path::new("/path/to/movie.mkv");
        let out_dir = Path::new("/output/folder");
        let out = compute_output_path(input, Some(out_dir), "", false);
        assert_eq!(out, PathBuf::from("/output/folder/movie.mp4"));
    }

    #[test]
    fn test_compute_output_path_mp4_default_suffix() {
        let input = Path::new("/path/to/movie.mp4");
        let out = compute_output_path(input, None, "", false);
        assert_eq!(out, PathBuf::from("/path/to/movie_4.1.mp4"));
    }

    #[test]
    fn test_compute_output_path_custom_suffix() {
        let input = Path::new("/path/to/movie.mkv");
        let out = compute_output_path(input, None, "_converted", false);
        assert_eq!(out, PathBuf::from("/path/to/movie_converted.mp4"));
    }
}
