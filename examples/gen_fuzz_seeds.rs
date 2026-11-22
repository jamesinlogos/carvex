//! Generate committed fuzz seeds from the shared fixture builders (H-14).
//!
//! Run with `cargo run --example gen_fuzz_seeds`. The generated files are
//! deterministic and are committed under `fuzz/corpus/<target>/`.

use carvex::image::fuzz_support::{
    build_e01, build_vhd_dynamic, build_vhd_dynamic_with_hole, build_vhd_fixed, build_vmdk_flat,
    build_vmdk_sparse, build_vmdk_sparse_with_hole, hostile_e01_huge_chunk,
    hostile_vmdk_descriptor_x09, hostile_vmdk_x08, mixed_payload,
};
use std::fs;
use std::path::{Path, PathBuf};

fn write(target: &str, name: &str, bytes: &[u8]) {
    let dir: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fuzz")
        .join("corpus")
        .join(target);
    fs::create_dir_all(&dir).expect("create fuzz corpus directory");
    fs::write(dir.join(name), bytes).expect("write fuzz seed");
}

fn main() {
    let payload = mixed_payload();
    let e01_plain = build_e01(&payload, false);
    let e01_zlib = build_e01(&payload, true);
    let vhd_fixed = build_vhd_fixed(&payload);
    let vhd_dynamic = build_vhd_dynamic(&payload);
    let (vhd_hole, _) = build_vhd_dynamic_with_hole();
    let (vmdk_flat_desc, _) = build_vmdk_flat(&payload);
    let vmdk_sparse = build_vmdk_sparse(&payload);
    let (vmdk_hole, _) = build_vmdk_sparse_with_hole();
    let x07 = hostile_e01_huge_chunk();
    let x08 = hostile_vmdk_x08();
    let x09 = hostile_vmdk_descriptor_x09();

    write("fuzz_e01", "fixture_e01_plain.E01", &e01_plain);
    write("fuzz_e01", "fixture_e01_zlib.E01", &e01_zlib);
    write("fuzz_e01", "hostile_x07_huge_chunk.E01", &x07);

    write("fuzz_vhd", "fixture_vhd_fixed.vhd", &vhd_fixed);
    write("fuzz_vhd", "fixture_vhd_dynamic.vhd", &vhd_dynamic);
    write("fuzz_vhd", "fixture_vhd_hole.vhd", &vhd_hole);

    write(
        "fuzz_vmdk",
        "fixture_vmdk_flat.vmdk",
        vmdk_flat_desc.as_bytes(),
    );
    write("fuzz_vmdk", "fixture_vmdk_sparse.vmdk", &vmdk_sparse);
    write("fuzz_vmdk", "fixture_vmdk_hole.vmdk", &vmdk_hole);
    write("fuzz_vmdk", "hostile_x08_grain_tables.vmdk", &x08);
    write("fuzz_vmdk", "hostile_x09_external.vmdk", &x09);

    write(
        "fuzz_split_names",
        "seed_segment_names.txt",
        b"disk.001\ndisk.002\ndisk.0001\n",
    );
    write(
        "fuzz_split_names",
        "seed_odd_extensions.txt",
        b"fuzz.1\nfuzz.abc\nfuzz.-1\n",
    );

    write("fuzz_open_image", "fixture_raw.bin", &payload);
    write("fuzz_open_image", "fixture_e01.E01", &e01_plain);
    write("fuzz_open_image", "fixture_vhd.vhd", &vhd_dynamic);
    write("fuzz_open_image", "fixture_vmdk.vmdk", &vmdk_sparse);
    write("fuzz_open_image", "hostile_x07.E01", &x07);
    write("fuzz_open_image", "hostile_x08.vmdk", &x08);
    write("fuzz_open_image", "hostile_x09.vmdk", &x09);

    write("fuzz_fragment_join", "fixture_mixed.bin", &payload);
    let mut fragment = vec![0u8; 4096];
    fragment.extend_from_slice(b"\xff\xd8\xff\xe0\x00\x10JFIF\x00");
    fragment.extend_from_slice(&[0x33; 512]);
    fragment.extend_from_slice(b"\xff\xd9");
    write("fuzz_fragment_join", "fixture_jpeg.bin", &fragment);
}
