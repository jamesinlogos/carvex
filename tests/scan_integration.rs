//! Library-level integration tests for the scanning pipeline.

mod common;

use carvex::fs;
use carvex::io::ImageSource;
use carvex::scan::{run_scan, AlignMode, ScanContext, ScanInput, ScanMode, ScanOptions};
use carvex::signatures::{FileType, Status, Validation, ValidationMode};
use common::{fat32_volume, mixed_raw_image, pdf_min, png_1x1, scan_options, MemSource, JPEG_1X1};
use tempfile::TempDir;

fn run(
    image: Vec<u8>,
    opts: ScanOptions,
    fs_info: Option<&fs::FsInfo>,
    name: &str,
) -> (TempDir, carvex::scan::ScanReport) {
    let src = MemSource::new(image);
    let dir = tempfile::tempdir().unwrap();
    let ctx = ScanContext {
        argv: vec![name.to_string()],
        ..Default::default()
    };
    let volumes: &[fs::FsInfo] = fs_info.map(std::slice::from_ref).unwrap_or(&[]);
    let input = ScanInput {
        source: &src,
        image: name,
        image_identity: None,
        image_len: src.len(),
        volumes,
        extra_extents: &[],
        out_dir: dir.path(),
    };
    let report = run_scan(&input, &opts, &ctx).unwrap();
    (dir, report)
}

#[test]
fn raw_scan_carves_jpeg_png_and_pdf() {
    let image = mixed_raw_image();
    let mut opts = scan_options(
        vec![FileType::Jpeg, FileType::Png, FileType::Pdf],
        ScanMode::Raw,
    );
    opts.min_size = 1;
    let (dir, report) = run(image.clone(), opts, None, "mixed.raw");

    assert_eq!(report.mode, "raw");
    assert_eq!(report.files.len(), 3, "files: {:#?}", report.files);
    let types: Vec<_> = report.files.iter().map(|f| f.file_type).collect();
    assert_eq!(
        types,
        vec![FileType::Jpeg, FileType::Png, FileType::Pdf],
        "files must be ordered by offset"
    );
    for file in &report.files {
        assert_eq!(file.status, Status::Complete);
        assert_eq!(file.validation, Validation::Structure);
        assert!(file.score >= 50);
        let path = dir.path().join(&file.path);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len() as u64, file.size);
        assert_eq!(carvex::util::sha256_hex(&bytes), file.sha256);
        assert!(file.path.starts_with(&format!("{:05}_", file.seq)));
        assert!(file.path.ends_with(file.file_type.ext()));
    }
    let jpeg = std::fs::read(dir.path().join(&report.files[0].path)).unwrap();
    let png = std::fs::read(dir.path().join(&report.files[1].path)).unwrap();
    let pdf = std::fs::read(dir.path().join(&report.files[2].path)).unwrap();
    assert_eq!(jpeg, JPEG_1X1);
    assert_eq!(png, png_1x1());
    assert_eq!(pdf, pdf_min());
}

#[test]
fn decode_validation_accepts_the_fixtures() {
    let image = mixed_raw_image();
    let mut opts = scan_options(
        vec![FileType::Jpeg, FileType::Png, FileType::Pdf],
        ScanMode::Raw,
    );
    opts.min_size = 1;
    opts.validate = ValidationMode::Decode;
    let (_dir, report) = run(image, opts, None, "decode.raw");
    assert_eq!(report.files.len(), 3);
    for file in &report.files {
        // H-19 stage 1 / E-1: PDF deep validation is lexical-only and reports
        // Unverified until a real PDF parser is integrated. JPEG/PNG fixtures
        // still fully decode.
        let expected = if file.file_type == FileType::Pdf {
            Validation::Unverified
        } else {
            Validation::Decoded
        };
        assert_eq!(file.validation, expected, "{}: {:?}", file.path, file.notes);
        let expected_validated = if expected == Validation::Decoded {
            file.carved_size
        } else {
            0
        };
        assert_eq!(file.validated_bytes, expected_validated);
    }
}

#[test]
fn scan_is_deterministic_across_thread_counts() {
    let image = mixed_raw_image();
    let mut single = scan_options(FileType::all().to_vec(), ScanMode::Raw);
    single.min_size = 1;
    single.threads = 1;
    single.chunk_size = 64 * 1024;
    let mut parallel = single.clone();
    parallel.threads = 4;
    parallel.chunk_size = 128 * 1024;

    let (dir_a, a) = run(image.clone(), single, None, "det.raw");
    let (dir_b, b) = run(image, parallel, None, "det.raw");

    assert_eq!(a.files.len(), b.files.len());
    assert_eq!(
        serde_json::to_value(&a.files).unwrap(),
        serde_json::to_value(&b.files).unwrap(),
        "file records must not depend on worker count or chunk size"
    );
    for file in &a.files {
        let left = std::fs::read(dir_a.path().join(&file.path)).unwrap();
        let right = std::fs::read(dir_b.path().join(&file.path)).unwrap();
        assert_eq!(left, right);
    }
}

#[test]
fn filesystem_aware_scan_uses_free_extents_only() {
    let mut image = fat32_volume();
    let png = png_1x1();
    // Cluster 2 is allocated by the fixture FAT; cluster 3 starts at 10,240.
    let offset = 10_240; // start of free cluster 3
    image[offset..offset + png.len()].copy_from_slice(&png);

    let src = MemSource::new(image.clone());
    let fs_info = fs::detect(&src, src.len(), None, None).unwrap();
    let mut opts = scan_options(vec![FileType::Png], ScanMode::Auto);
    opts.min_size = 1;
    let (_dir, report) = run(image, opts, Some(&fs_info), "fat32.img");

    assert_eq!(report.mode, "unallocated");
    assert!(report.filesystem.is_some());
    assert_eq!(report.files.len(), 1, "files: {:#?}", report.files);
    assert_eq!(report.files[0].offset, offset as u64);
    assert_eq!(report.files[0].cluster, Some(3));
    assert_eq!(report.files[0].allocated, Some(false));
    assert_eq!(report.files[0].partition_index, None);
}

#[test]
fn nested_images_are_suppressed_before_writing() {
    let inner = png_1x1();
    let mut pdf = b"%PDF-1.4\n".to_vec();
    pdf.extend_from_slice(format!("1 0 obj\n<< /Length {} >>\nstream\n", inner.len()).as_bytes());
    pdf.extend_from_slice(&inner);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    let xref = pdf.len();
    pdf.extend_from_slice(
        b"xref\n0 2\n0000000000 65535 f \n0000000009 00000 n \ntrailer\n<< /Size 2 /Root 1 0 R >>\n",
    );
    pdf.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());

    let mut image = vec![0u8; 4096];
    image.extend_from_slice(&pdf);
    image.extend_from_slice(&vec![0u8; 4096]);

    let mut opts = scan_options(vec![FileType::Png, FileType::Pdf], ScanMode::Raw);
    opts.min_size = 1;
    let (dir, report) = run(image, opts, None, "nested.raw");

    assert_eq!(report.files.len(), 1, "files: {:#?}", report.files);
    assert_eq!(report.files[0].file_type, FileType::Pdf);
    assert_eq!(report.stats.suppressed_nested, 1);
    let written: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(written.len(), 1);
    assert!(written[0].ends_with(".pdf"));
}

#[test]
fn alignment_restricts_candidate_starts_without_changing_results_on_aligned_data() {
    let mut image = vec![0u8; 512];
    image.extend_from_slice(&pdf_min());
    image.extend_from_slice(&vec![0u8; 512]);

    let mut opts = scan_options(vec![FileType::Pdf], ScanMode::Raw);
    opts.min_size = 1;
    opts.align = AlignMode::Sector;
    let (_dir, report) = run(image, opts, None, "aligned.raw");
    assert_eq!(report.files.len(), 1);
    assert_eq!(report.files[0].offset % 512, 0);
}
