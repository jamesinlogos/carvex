//! Command-line contract: exit codes, report schema compatibility, and the
//! guarantee that the source image is never modified.

mod common;

use common::mixed_raw_image;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_carvex")
}

fn image_file(dir: &Path) -> PathBuf {
    let path = dir.join("source.img");
    fs::write(&path, mixed_raw_image()).unwrap();
    path
}

fn run(args: &[&str]) -> Output {
    Command::new(bin()).args(args).output().unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn successful_raw_scan_returns_zero_and_leaves_the_source_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let image = image_file(dir.path());
    let before = fs::read(&image).unwrap();
    let out_dir = dir.path().join("out");
    let report_path = dir.path().join("report.json");
    let csv_path = dir.path().join("report.csv");

    let out = run(&[
        "scan",
        image.to_str().unwrap(),
        "-o",
        out_dir.to_str().unwrap(),
        "--mode",
        "raw",
        "--no-fs",
        "--quiet",
        "--min-size",
        "1",
        "--json-report",
        report_path.to_str().unwrap(),
        "--csv-report",
        csv_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(fs::read(&image).unwrap(), before, "source image changed");

    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    assert_eq!(report["schema_version"], 2);
    assert_eq!(report["mode"], "raw");
    assert_eq!(
        report["tool"],
        format!("CarveX {}", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(report["files"].as_array().unwrap().len(), 3);
    assert!(report["input"]["unchanged"].as_bool().unwrap());
    assert_eq!(report["options"]["align"], "none");
    assert_eq!(report["options"]["validate"], "structure");
    assert_eq!(report["options"]["max_truncated_size"], 16 * 1024 * 1024);
    for file in report["files"].as_array().unwrap() {
        assert!(file["validation"].is_string());
        assert!(file["validated_bytes"].is_number());
        assert!(file["lba"].is_number());
        assert!(file["extent_id"].is_number());
        assert!(file["source"].is_string());
        assert!(file["fragments"].is_array());
    }

    let csv = fs::read_to_string(&csv_path).unwrap();
    let header = csv.lines().next().unwrap();
    let legacy =
        "seq,type,offset,size,carved_size,sha256,status,score,width,height,repaired,path,notes";
    assert_eq!(
        header,
        format!(
            "{legacy},validation,validated_bytes,lba,cluster,partition_index,extent_id,allocated,source,fragments"
        )
    );
    assert_eq!(csv.lines().count(), 4);
}

#[test]
fn report_schema_one_has_the_v01_shape() {
    let dir = tempfile::tempdir().unwrap();
    let image = image_file(dir.path());
    let out_dir = dir.path().join("out");
    let report_path = dir.path().join("report.json");

    let out = run(&[
        "scan",
        image.to_str().unwrap(),
        "-o",
        out_dir.to_str().unwrap(),
        "--mode",
        "raw",
        "--no-fs",
        "--quiet",
        "--min-size",
        "1",
        "--report-schema",
        "1",
        "--json-report",
        report_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));

    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    let mut keys: Vec<_> = report.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "bytes_scanned",
            "duration_ms",
            "files",
            "filesystem",
            "image",
            "image_size",
            "mode",
            "output_dir",
            "started_at_unix",
            "stats",
            "throughput_mib_s",
            "tool",
        ]
    );
    let file = &report["files"][0];
    let mut file_keys: Vec<_> = file.as_object().unwrap().keys().cloned().collect();
    file_keys.sort();
    assert_eq!(
        file_keys,
        vec![
            "carved_size",
            "height",
            "notes",
            "offset",
            "path",
            "repaired",
            "score",
            "seq",
            "sha256",
            "size",
            "status",
            "type",
            "width",
        ]
    );
    let stats = report["stats"].as_object().unwrap();
    assert!(!stats.contains_key("too_large_truncated"));
    assert!(!stats.contains_key("trimmed_tail"));
}

#[test]
fn auto_mode_without_a_filesystem_falls_back_to_raw_with_exit_three() {
    let dir = tempfile::tempdir().unwrap();
    let image = image_file(dir.path());
    let out_dir = dir.path().join("out");
    let report_path = dir.path().join("report.json");

    let out = run(&[
        "scan",
        image.to_str().unwrap(),
        "-o",
        out_dir.to_str().unwrap(),
        "--quiet",
        "--min-size",
        "1",
        "--json-report",
        report_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(3), "stderr: {}", stderr(&out));
    assert!(
        stderr(&out).contains("filesystem detection failed; used raw scan"),
        "stderr: {}",
        stderr(&out)
    );
    assert!(!stdout(&out).contains("panicked"));
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    assert_eq!(report["mode"], "raw");
    assert!(!report["warnings"].as_array().unwrap().is_empty());
}

#[test]
fn unallocated_mode_without_a_filesystem_is_a_runtime_error() {
    let dir = tempfile::tempdir().unwrap();
    let image = image_file(dir.path());
    let out = run(&[
        "scan",
        image.to_str().unwrap(),
        "-o",
        dir.path().join("out").to_str().unwrap(),
        "--mode",
        "unallocated",
        "--quiet",
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("filesystem detection failed"));
}

#[test]
fn non_empty_output_directory_requires_force() {
    let dir = tempfile::tempdir().unwrap();
    let image = image_file(dir.path());
    let out_dir = dir.path().join("out");
    fs::create_dir(&out_dir).unwrap();
    fs::write(out_dir.join("keep.txt"), b"user data").unwrap();

    let out = run(&[
        "scan",
        image.to_str().unwrap(),
        "-o",
        out_dir.to_str().unwrap(),
        "--mode",
        "raw",
        "--no-fs",
        "--quiet",
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("is not empty"));

    let out = run(&[
        "scan",
        image.to_str().unwrap(),
        "-o",
        out_dir.to_str().unwrap(),
        "--mode",
        "raw",
        "--no-fs",
        "--quiet",
        "--force",
        "--min-size",
        "1",
    ]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(out_dir.join("keep.txt").exists());
}

#[test]
fn usage_errors_exit_two() {
    let out = run(&["scan", "x.img", "-o", "out", "--types", "bogus"]);
    assert_eq!(out.status.code(), Some(2));
    let out = run(&["scan", "x.img", "-o", "out", "--not-a-flag"]);
    assert_eq!(out.status.code(), Some(2));
    let out = run(&["info"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn info_and_signatures_have_stable_exit_codes() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("plain.img");
    fs::write(&image, vec![0u8; 64 * 1024]).unwrap();
    let out = run(&["info", image.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));

    let out = run(&["signatures"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("JPEG"));
    assert!(stdout(&out).contains("PNG"));
    assert!(stdout(&out).contains("PDF"));
}

#[test]
fn emitted_report_matches_the_published_schema() {
    let schema: serde_json::Value = serde_json::from_slice(
        &fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/report.schema.json")).unwrap(),
    )
    .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let image = image_file(dir.path());
    let report_path = dir.path().join("report.json");
    let out = run(&[
        "scan",
        image.to_str().unwrap(),
        "-o",
        dir.path().join("out").to_str().unwrap(),
        "--mode",
        "raw",
        "--no-fs",
        "--quiet",
        "--min-size",
        "1",
        "--json-report",
        report_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();

    let keys = |v: &serde_json::Value| {
        let mut k: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    };
    let schema_props = |path: &str| {
        let mut k: Vec<_> = schema
            .pointer(path)
            .unwrap_or_else(|| panic!("schema pointer missing: {path}"))
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        k.sort();
        k
    };
    // B-06 fields are optional so default reports keep their historical JSON
    // shape.  Validate required fields and reject unknown keys for those defs.
    let assert_schema_keys = |def_path: &str, value: &serde_json::Value, optional: bool| {
        let actual = keys(value);
        let props = schema_props(&format!("{def_path}/properties"));
        if !optional {
            assert_eq!(actual, props);
            return;
        }
        let def = schema.pointer(def_path).unwrap();
        let required: Vec<_> = def
            .pointer("/required")
            .and_then(|v| v.as_array())
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        for key in &required {
            assert!(actual.contains(key), "missing required key {key}");
        }
        for key in &actual {
            assert!(props.contains(key), "unknown key {key}");
        }
    };

    assert_schema_keys("", &report, false);
    assert_schema_keys("/properties/tool_info", &report["tool_info"], false);
    assert_schema_keys("/properties/input", &report["input"], false);
    assert_schema_keys("/$defs/options", &report["options"], true);
    assert_schema_keys("/$defs/stats", &report["stats"], false);
    assert_schema_keys("/$defs/file", &report["files"][0], true);
}

#[test]
fn fragment_recovery_is_opt_in_and_reported_only_when_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let image = image_file(dir.path());
    let report_path = dir.path().join("bgc-report.json");
    let out = run(&[
        "scan",
        image.to_str().unwrap(),
        "-o",
        dir.path().join("bgc-out").to_str().unwrap(),
        "--mode",
        "raw",
        "--no-fs",
        "--quiet",
        "--min-size",
        "1",
        "--fragment-recovery",
        "--max-gap",
        "4",
        "--max-frag-candidates",
        "32",
        "--frag-time-limit",
        "250",
        "--json-report",
        report_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    assert_eq!(report["options"]["fragment_recovery"], true);
    assert_eq!(report["options"]["max_gap"], 4);
    assert_eq!(report["options"]["max_frag_candidates"], 32);
    assert_eq!(report["options"]["frag_time_limit_ms"], 250);
    assert!(report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["code"] == "FRAGMENT_RECOVERY_SKIPPED"));
    for file in report["files"].as_array().unwrap() {
        assert!(file.get("checks").is_none());
        assert!(file.get("confidence").is_none());
        assert_eq!(file["source"], "carve");
    }

    let rejected = run(&[
        "scan",
        image.to_str().unwrap(),
        "-o",
        dir.path().join("bad-out").to_str().unwrap(),
        "--max-gap",
        "4",
    ]);
    assert_eq!(
        rejected.status.code(),
        Some(1),
        "stderr: {}",
        stderr(&rejected)
    );
    assert!(stderr(&rejected).contains("--fragment-recovery"));
}

/// A-07 step 2: a default scan reads both volumes and the gap, so nothing is
/// reported as skipped and the run exits 0.
#[test]
fn multi_volume_default_scan_reads_every_volume_and_gap() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("multi.img");
    fs::write(
        &image,
        common::mbr_two_fat32(common::JPEG_1X1, &common::png_1x1()),
    )
    .unwrap();
    let out_dir = dir.path().join("out");
    let report_path = dir.path().join("report.json");

    let out = run(&[
        "scan",
        image.to_str().unwrap(),
        "-o",
        out_dir.to_str().unwrap(),
        "--quiet",
        "--min-size",
        "1",
        "--json-report",
        report_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(!err.contains("VOLUME_NOT_SCANNED"), "stderr: {err}");
    assert!(!err.contains("GAP_NOT_SCANNED"), "stderr: {err}");

    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    assert_eq!(report["mode"], "unallocated");
    assert_eq!(report["options"]["partition"], "all");
    assert_eq!(report["options"]["include_gaps"], true);

    let volumes = report["volumes"].as_array().unwrap();
    assert_eq!(volumes.len(), 2, "both volumes must be listed");
    assert!(volumes.iter().all(|v| v["scanned"] == true));

    let regions = report["regions"].as_array().unwrap();
    let gap = regions
        .iter()
        .find(|r| r["kind"] == "gap")
        .expect("the 1 MiB gap must appear in regions[]");
    assert_eq!(gap["scanned"], true);
    assert_eq!(gap["len"], 1024 * 1024);

    // Both the JPEG and the PNG volume were carved into their own directory.
    let files = report["files"].as_array().unwrap();
    assert_eq!(files.len(), 2, "files: {files:#?}");
    let paths: Vec<&str> = files.iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert!(paths.iter().any(|p| p.starts_with("part1/")), "{paths:?}");
    assert!(paths.iter().any(|p| p.starts_with("part2/")), "{paths:?}");
}

/// A-07 step 2: `--partition largest --no-gaps` restores the historical
/// single-volume run, including its coded warnings and exit 3.
#[test]
fn legacy_default_can_be_restored_with_largest_and_no_gaps() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("multi.img");
    fs::write(
        &image,
        common::mbr_two_fat32(common::JPEG_1X1, &common::png_1x1()),
    )
    .unwrap();
    let report_path = dir.path().join("report.json");

    let out = run(&[
        "scan",
        image.to_str().unwrap(),
        "-o",
        dir.path().join("out").to_str().unwrap(),
        "--partition",
        "largest",
        "--no-gaps",
        "--quiet",
        "--min-size",
        "1",
        "--json-report",
        report_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(3), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("VOLUME_NOT_SCANNED"), "stderr: {err}");
    assert!(err.contains("GAP_NOT_SCANNED"), "stderr: {err}");

    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    assert_eq!(report["options"]["partition"], "largest");
    assert_eq!(report["options"]["include_gaps"], false);
    let volumes = report["volumes"].as_array().unwrap();
    assert_eq!(volumes[0]["scanned"], true);
    assert_eq!(volumes[1]["scanned"], false);
    let files = report["files"].as_array().unwrap();
    assert_eq!(files.len(), 1, "files: {files:#?}");
}

/// A-07: with `--no-gaps`, `--partition all` scans every volume into its own
/// `part<N>/` sub-directory but still leaves the gap unread (exit 3).
#[test]
fn partition_all_writes_each_volume_into_its_own_subdirectory() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("multi.img");
    fs::write(
        &image,
        common::mbr_two_fat32(common::JPEG_1X1, &common::png_1x1()),
    )
    .unwrap();
    let out_dir = dir.path().join("out");
    let report_path = dir.path().join("report.json");

    let out = run(&[
        "scan",
        image.to_str().unwrap(),
        "-o",
        out_dir.to_str().unwrap(),
        "--partition",
        "all",
        "--no-gaps",
        "--quiet",
        "--min-size",
        "1",
        "--json-report",
        report_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(3), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("GAP_NOT_SCANNED"), "stderr: {err}");
    assert!(!err.contains("VOLUME_NOT_SCANNED"), "stderr: {err}");

    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    let volumes = report["volumes"].as_array().unwrap();
    assert_eq!(volumes.len(), 2);
    assert_eq!(volumes[0]["scanned"], true);
    assert_eq!(volumes[1]["scanned"], true);

    let files = report["files"].as_array().unwrap();
    assert_eq!(files.len(), 2, "files: {files:#?}");
    let paths: Vec<&str> = files.iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert!(paths.iter().any(|p| p.starts_with("part1/")), "{paths:?}");
    assert!(paths.iter().any(|p| p.starts_with("part2/")), "{paths:?}");
    for p in &paths {
        assert!(out_dir.join(p).exists(), "missing output {p}");
    }
    assert_eq!(report["options"]["partition"], "all");
    assert_eq!(report["options"]["include_gaps"], false);
}

/// A-07: `--include-gaps` stays accepted after step 2 (gaps are on by default)
/// and still finishes with exit 0 and an honest `regions[]`.
#[test]
fn include_gaps_finishes_with_exit_zero() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("multi.img");
    fs::write(
        &image,
        common::mbr_two_fat32(common::JPEG_1X1, &common::png_1x1()),
    )
    .unwrap();
    let report_path = dir.path().join("report.json");

    let out = run(&[
        "scan",
        image.to_str().unwrap(),
        "-o",
        dir.path().join("out").to_str().unwrap(),
        "--partition",
        "all",
        "--include-gaps",
        "--quiet",
        "--min-size",
        "1",
        "--json-report",
        report_path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(!err.contains("GAP_NOT_SCANNED"), "stderr: {err}");
    assert!(!err.contains("VOLUME_NOT_SCANNED"), "stderr: {err}");

    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    let regions = report["regions"].as_array().unwrap();
    // Gaps and both volumes' free space are read now; the allocated clusters
    // stay unread by design (that is what `auto`/`unallocated` means).
    assert!(
        regions
            .iter()
            .any(|r| r["kind"] == "gap" && r["scanned"] == true),
        "regions: {regions:#?}"
    );
    assert!(
        regions
            .iter()
            .any(|r| r["kind"] == "free" && r["scanned"] == true),
        "regions: {regions:#?}"
    );
    assert!(
        regions
            .iter()
            .any(|r| r["kind"] == "allocated" && r["scanned"] == false),
        "regions: {regions:#?}"
    );
    assert_eq!(report["options"]["include_gaps"], true);
}
