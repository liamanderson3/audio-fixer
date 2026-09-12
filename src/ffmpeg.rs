use anyhow::{anyhow, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use crate::ffprobe::ProbeOutput;

#[derive(Debug, Clone)]
pub struct ConversionOptions {
    pub audio_bitrate: String,
    pub audio_codec: String,
    pub ffmpeg_bin: String,
    pub overwrite: bool,
    pub force_video_copy: bool,
    pub transcode_video: bool,
    pub delete_source: bool,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self {
            audio_bitrate: "384k".to_string(),
            audio_codec: "aac".to_string(),
            ffmpeg_bin: "ffmpeg".to_string(),
            overwrite: false,
            force_video_copy: false,
            transcode_video: false,
            delete_source: false,
        }
    }
}

#[allow(dead_code)]
pub struct ConversionResult {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub video_copied: bool,
    pub audio_downmixed: bool,
    pub duration_secs: Option<f64>,
}

pub fn convert_video_file<P: AsRef<Path>>(
    input_path: P,
    output_path: P,
    probe: &ProbeOutput,
    opts: &ConversionOptions,
) -> Result<ConversionResult> {
    let input = input_path.as_ref();
    let output = output_path.as_ref();

    if output.exists() && !opts.overwrite {
        return Err(anyhow!(
            "Output file '{}' already exists. Use --overwrite to replace it.",
            output.display()
        ));
    }

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }

    // Temporary file for safe atomic writing
    let temp_output = output.with_extension("tmp_converting.mp4");
    if temp_output.exists() {
        let _ = fs::remove_file(&temp_output);
    }

    let has_5_1 = probe.has_5_1_audio();
    let try_copy = !opts.transcode_video;
    let mut video_copied = false;

    let copy_result = if try_copy {
        let res = run_ffmpeg_command(input, &temp_output, probe, opts, true, has_5_1);
        if res.is_ok() {
            video_copied = true;
            Ok(())
        } else {
            res
        }
    } else {
        Err(anyhow!("Transcode forced"))
    };

    if !video_copied {
        if opts.force_video_copy {
            let _ = fs::remove_file(&temp_output);
            return Err(anyhow!(
                "Video stream copy failed for '{}' and --force-video-copy was specified: {}",
                input.display(),
                copy_result.unwrap_err()
            ));
        }

        // Fallback: Transcode video with libx264 high quality CRF 18
        run_ffmpeg_command(input, &temp_output, probe, opts, false, has_5_1)?;
    }

    // Rename temp file to final output path atomically
    if fs::rename(&temp_output, output).is_err() {
        fs::copy(&temp_output, output)?;
        let _ = fs::remove_file(&temp_output);
    }

    // Delete source file if requested and output is distinct
    if opts.delete_source && input.exists() && input != output {
        let _ = fs::remove_file(input);
    }

    Ok(ConversionResult {
        input_path: input.to_path_buf(),
        output_path: output.to_path_buf(),
        video_copied,
        audio_downmixed: has_5_1,
        duration_secs: probe
            .format
            .as_ref()
            .and_then(|f| f.duration.as_deref())
            .and_then(|d| d.parse::<f64>().ok()),
    })
}

fn run_ffmpeg_command(
    input: &Path,
    temp_output: &Path,
    probe: &ProbeOutput,
    opts: &ConversionOptions,
    copy_video: bool,
    downmix_5_1: bool,
) -> Result<()> {
    let mut cmd = Command::new(&opts.ffmpeg_bin);
    cmd.arg("-y")
        .arg("-i")
        .arg(input);

    // Map video streams
    if !probe.get_video_streams().is_empty() {
        cmd.arg("-map").arg("0:v");
        if copy_video {
            cmd.arg("-c:v").arg("copy");
        } else {
            cmd.arg("-c:v")
                .arg("libx264")
                .arg("-preset")
                .arg("slow")
                .arg("-crf")
                .arg("18")
                .arg("-pix_fmt")
                .arg("yuv420p");
        }
    }

    // Map audio streams
    cmd.arg("-map").arg("0:a");

    let pan_filter = "pan=4.1|c0=c0+0.707*c2|c1=c1+0.707*c2|c3=c3|c4=0.707*c4+0.707*c5";

    for (i, stream) in probe.streams.iter().filter(|s| s.codec_type == "audio").enumerate() {
        if downmix_5_1 && stream.is_5_1_audio() {
            cmd.arg(format!("-af:a:{}", i)).arg(pan_filter);
            cmd.arg(format!("-c:a:{}", i)).arg(&opts.audio_codec);
            cmd.arg(format!("-b:a:{}", i)).arg(&opts.audio_bitrate);
        } else {
            // Keep non-5.1 audio streams or encode to AAC for MP4 container compatibility
            cmd.arg(format!("-c:a:{}", i)).arg("aac");
        }
    }

    cmd.arg("-movflags").arg("+faststart");
    cmd.arg(temp_output);

    let output = cmd
        .output()
        .map_err(|e| anyhow!("Failed to spawn ffmpeg: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("ffmpeg failed with exit code {:?}: {}", output.status.code(), stderr));
    }

    Ok(())
}
