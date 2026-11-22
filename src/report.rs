use crate::scan::{CarvedFile, ScanReport};
use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

/// Write the JSON report atomically.
///
/// `schema` 1 removes the additive v2 fields so existing consumers keep the
/// exact v0.1 shape; `schema` 2 is the default.
pub fn write_json(report: &ScanReport, path: &Path, schema: u8) -> Result<()> {
    ensure_parent(path)?;
    let mut value = serde_json::to_value(report)
        .with_context(|| format!("cannot serialize JSON report '{}'", path.display()))?;
    if schema == 1 {
        strip_v2_fields(&mut value);
    }
    let bytes = serde_json::to_vec_pretty(&value)
        .with_context(|| format!("cannot serialize JSON report '{}'", path.display()))?;
    write_atomic(path, &bytes)
}

fn strip_v2_fields(value: &mut serde_json::Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    for key in [
        "schema_version",
        "tool_info",
        "input",
        "argv",
        "options",
        "case_id",
        "examiner",
        "started_at",
        "warnings",
        "volumes",
        "regions",
        "unreadable_regions",
        "nondeterministic_stop",
    ] {
        obj.remove(key);
    }
    if let Some(stats) = obj.get_mut("stats").and_then(|v| v.as_object_mut()) {
        stats.remove("too_large_truncated");
        stats.remove("trimmed_tail");
    }
    if let Some(files) = obj.get_mut("files").and_then(|v| v.as_array_mut()) {
        for file in files {
            let Some(file) = file.as_object_mut() else {
                continue;
            };
            for key in [
                "validation",
                "validated_bytes",
                "lba",
                "cluster",
                "partition_index",
                "extent_id",
                "allocated",
                "source",
                "fragments",
            ] {
                file.remove(key);
            }
        }
    }
}

/// Write the CSV report.  Existing columns keep their order; new columns are
/// appended so spreadsheets written for v0.1 can still read the prefix.
pub fn write_csv(report: &ScanReport, path: &Path) -> Result<()> {
    ensure_parent(path)?;
    let f = File::create(path)
        .with_context(|| format!("cannot create CSV report '{}'", path.display()))?;
    let mut w = BufWriter::new(f);
    writeln!(
        w,
        "seq,type,offset,size,carved_size,sha256,status,score,width,height,repaired,path,notes,\
         validation,validated_bytes,lba,cluster,partition_index,extent_id,allocated,source,fragments"
    )?;
    for f in &report.files {
        writeln!(
            w,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            f.seq,
            f.file_type.ext(),
            f.offset,
            f.size,
            f.carved_size,
            f.sha256,
            status_text(f),
            f.score,
            f.width.map(|v| v.to_string()).unwrap_or_default(),
            f.height.map(|v| v.to_string()).unwrap_or_default(),
            f.repaired,
            csv_escape(&f.path),
            csv_escape(&f.notes.join("; ")),
            validation_text(f),
            f.validated_bytes,
            f.lba,
            f.cluster.map(|v| v.to_string()).unwrap_or_default(),
            f.partition_index.map(|v| v.to_string()).unwrap_or_default(),
            f.extent_id,
            f.allocated.map(|v| v.to_string()).unwrap_or_default(),
            csv_escape(&f.source),
            csv_escape(
                &f.fragments
                    .iter()
                    .map(|fr| format!("{}+{}", fr.offset, fr.len))
                    .collect::<Vec<_>>()
                    .join(";")
            )
        )?;
    }
    w.flush()?;
    Ok(())
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!(
        "{}.tmp{}",
        path.extension().and_then(|e| e.to_str()).unwrap_or("json"),
        std::process::id()
    ));
    std::fs::write(&tmp, data)
        .with_context(|| format!("cannot write temporary report '{}'", tmp.display()))?;
    if path.exists() {
        std::fs::remove_file(path)
            .with_context(|| format!("cannot replace report '{}'", path.display()))?;
    }
    std::fs::rename(&tmp, path)
        .with_context(|| format!("cannot rename '{}' to '{}'", tmp.display(), path.display()))?;
    Ok(())
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("cannot create report directory '{}'", parent.display())
            })?;
        }
    }
    Ok(())
}

fn status_text(f: &CarvedFile) -> &'static str {
    use crate::signatures::Status;
    match f.status {
        Status::Complete => "complete",
        Status::Truncated => "truncated",
        Status::Repaired => "repaired",
    }
}

fn validation_text(f: &CarvedFile) -> &'static str {
    use crate::signatures::Validation;
    match f.validation {
        Validation::Structure => "structure",
        Validation::Decoded => "decoded",
        Validation::Partial => "partial",
        Validation::Unverified => "unverified",
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
