use std::process::Command;
use tempfile::tempdir;

#[test]
fn test_end_to_end_conversion() {
    let dir = tempdir().unwrap();
    let avi_5_1 = dir.path().join("sample_5.1.avi");
    let mkv_5_1 = dir.path().join("sample_5.1.mkv");
    let mkv_2_0 = dir.path().join("sample_2.0.mkv");
    let mp4_2_0 = dir.path().join("sample_2.0.mp4");
    let out_dir = dir.path().join("converted");

    // 1. Generate test files using ffmpeg
    // 5.1 AVI
    let status = Command::new("ffmpeg")
        .args(&[
            "-y", "-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=24",
            "-f", "lavfi", "-i", "sine=frequency=1000:duration=1",
            "-filter_complex", "[1:a]pan=5.1|c0=c0|c1=c0|c2=c0|c3=c0|c4=c0|c5=c0[aout]",
            "-map", "0:v", "-map", "[aout]", "-c:v", "mpeg4", "-c:a", "ac3",
            avi_5_1.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to create test AVI");
    assert!(status.status.success());

    // 5.1 MKV
    let status = Command::new("ffmpeg")
        .args(&[
            "-y", "-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=24",
            "-f", "lavfi", "-i", "sine=frequency=800:duration=1",
            "-filter_complex", "[1:a]pan=5.1|c0=c0|c1=c0|c2=c0|c3=c0|c4=c0|c5=c0[aout]",
            "-map", "0:v", "-map", "[aout]", "-c:v", "libx264", "-c:a", "aac",
            mkv_5_1.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to create test MKV");
    assert!(status.status.success());

    // 2.0 Stereo MKV (Non-5.1 MKV file, should be converted to MP4)
    let status = Command::new("ffmpeg")
        .args(&[
            "-y", "-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=24",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=1",
            "-c:v", "libx264", "-c:a", "aac",
            mkv_2_0.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to create test 2.0 MKV");
    assert!(status.status.success());

    // 2.0 Stereo MP4 (Already MP4 and non-5.1, should be skipped)
    let status = Command::new("ffmpeg")
        .args(&[
            "-y", "-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=24",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=1",
            "-c:v", "libx264", "-c:a", "aac",
            mp4_2_0.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to create test MP4");
    assert!(status.status.success());

    // 2. Run audio-fixer binary
    let cargo_bin = env!("CARGO_BIN_EXE_audio-fixer");
    let output = Command::new(cargo_bin)
        .arg("-o")
        .arg(&out_dir)
        .arg(&avi_5_1)
        .arg(&mkv_5_1)
        .arg(&mkv_2_0)
        .arg(&mp4_2_0)
        .output()
        .expect("Failed to run audio-fixer");

    assert!(output.status.success(), "audio-fixer failed: {}", String::from_utf8_lossy(&output.stderr));

    // 3. Verify output files
    let out_avi_mp4 = out_dir.join("sample_5.1.mp4");
    let out_mkv_mp4 = out_dir.join("sample_5.1_mkv.mp4");
    let out_mkv_2_0_mp4 = out_dir.join("sample_2.0.mp4");

    assert!(out_avi_mp4.exists(), "Converted AVI MP4 should exist");
    assert!(out_mkv_mp4.exists(), "Converted 5.1 MKV MP4 should exist");
    assert!(out_mkv_2_0_mp4.exists(), "Converted 2.0 MKV MP4 should exist");

    // Check audio channels on 5.1 converted file via ffprobe
    let ffprobe_out = Command::new("ffprobe")
        .args(&[
            "-v", "error", "-show_entries", "stream=channels,channel_layout,width,height,r_frame_rate",
            "-of", "json", out_avi_mp4.to_str().unwrap(),
        ])
        .output()
        .expect("ffprobe failed on output file");

    let probe_json: serde_json::Value = serde_json::from_slice(&ffprobe_out.stdout).unwrap();
    let streams = probe_json["streams"].as_array().unwrap();

    let video_stream = streams.iter().find(|s| s["width"].is_number()).unwrap();
    let audio_stream = streams.iter().find(|s| s["channels"].is_number()).unwrap();

    assert_eq!(video_stream["width"], 320);
    assert_eq!(video_stream["height"], 240);
    assert_eq!(video_stream["r_frame_rate"], "24/1");
    assert_eq!(audio_stream["channels"], 5);
}

#[test]
fn test_skip_already_converted_output() {
    let dir = tempdir().unwrap();
    let mkv_source = dir.path().join("movie.mkv");
    let mp4_existing = dir.path().join("movie.mp4");

    // Create mkv source
    let status = Command::new("ffmpeg")
        .args(&[
            "-y", "-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=24",
            "-f", "lavfi", "-i", "sine=frequency=1000:duration=1",
            "-filter_complex", "[1:a]pan=5.1|c0=c0|c1=c0|c2=c0|c3=c0|c4=c0|c5=c0[aout]",
            "-map", "0:v", "-map", "[aout]", "-c:v", "libx264", "-c:a", "aac",
            mkv_source.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to create test MKV");
    assert!(status.status.success());

    // Create existing target mp4 with 2.0 stereo audio (already converted)
    let status = Command::new("ffmpeg")
        .args(&[
            "-y", "-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=24",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=1",
            "-c:v", "libx264", "-c:a", "aac",
            mp4_existing.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to create existing target MP4");
    assert!(status.status.success());

    // Run audio-fixer on mkv_source
    let cargo_bin = env!("CARGO_BIN_EXE_audio-fixer");
    let output = Command::new(cargo_bin)
        .arg(&mkv_source)
        .output()
        .expect("Failed to run audio-fixer");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Skipped"), "Should skip converting since target MP4 already exists with non-5.1 audio");
}

#[test]
fn test_detach_background_daemonization() {
    let dir = tempdir().unwrap();
    let mkv_2_0 = dir.path().join("daemon_sample.mkv");
    let log_file = dir.path().join("daemon.log");

    // Create test file
    let status = Command::new("ffmpeg")
        .args(&[
            "-y", "-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=24",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=1",
            "-c:v", "libx264", "-c:a", "aac",
            mkv_2_0.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to create test MKV");
    assert!(status.status.success());

    // Run audio-fixer with --detach (-d)
    let cargo_bin = env!("CARGO_BIN_EXE_audio-fixer");
    let output = Command::new(cargo_bin)
        .arg("-d")
        .arg("-l")
        .arg(&log_file)
        .arg(&mkv_2_0)
        .output()
        .expect("Failed to run audio-fixer in background mode");

    assert!(output.status.success(), "Detached parent process failed: {}", String::from_utf8_lossy(&output.stderr));

    // Wait briefly for background process to finish converting
    std::thread::sleep(std::time::Duration::from_millis(1500));

    let out_mp4 = dir.path().join("daemon_sample.mp4");
    assert!(out_mp4.exists(), "Daemonized background worker should have converted daemon_sample.mkv to .mp4");
}
