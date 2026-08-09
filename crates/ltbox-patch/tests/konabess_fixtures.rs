//! Real-image KonaBess import coverage.
//!
//! The repository does not carry firmware or export fixtures. Opt in with:
//!
//! ```powershell
//! $env:KONABESS_TB322_IMAGE = 'D:\fixtures\tb322\vendor_boot.img'
//! $env:KONABESS_TB322_EXPORT = 'D:\fixtures\konabess-sun.txt'
//! $env:KONABESS_TB321_IMAGE = 'D:\fixtures\tb321\vendor_boot.img'
//! $env:KONABESS_TB321_EXPORTS = 'D:\fixtures\a.txt;D:\fixtures\b.txt'
//! cargo test -p ltbox-patch --test konabess_fixtures -- --ignored --nocapture
//! ```

use std::env;
use std::path::PathBuf;

use ltbox_patch::konabess::{
    GpuTable, KonaBessExport, classify_vendor_boot_dtbs, extract_vendor_boot_dtbs,
    parse_fdt_gpu_info, read_export, replace_vendor_boot_dtb,
};

fn required_path(name: &str) -> PathBuf {
    PathBuf::from(env::var_os(name).unwrap_or_else(|| panic!("{name} must be set")))
}

fn changed_cell_count(before: &GpuTable, after: &GpuTable) -> usize {
    before
        .groups
        .iter()
        .map(|before_group| {
            let after_group = after
                .groups
                .iter()
                .find(|group| group.id == before_group.id)
                .expect("matching group id");
            before_group
                .header_properties
                .iter()
                .map(|property| {
                    (
                        property,
                        after_group
                            .header_properties
                            .iter()
                            .find(|candidate| candidate.name == property.name)
                            .expect("matching header property"),
                    )
                })
                .chain(before_group.levels.iter().flat_map(|before_level| {
                    let after_level = after_group
                        .levels
                        .iter()
                        .find(|level| level.id == before_level.id)
                        .expect("matching level id");
                    before_level.properties.iter().map(|property| {
                        (
                            property,
                            after_level
                                .properties
                                .iter()
                                .find(|candidate| candidate.name == property.name)
                                .expect("matching level property"),
                        )
                    })
                }))
                .map(|(before_property, after_property)| {
                    assert_eq!(before_property.cells.len(), after_property.cells.len());
                    before_property
                        .cells
                        .iter()
                        .zip(&after_property.cells)
                        .filter(|(before, after)| before != after)
                        .count()
                })
                .sum::<usize>()
        })
        .sum()
}

#[test]
#[ignore = "requires locally supplied vendor_boot and KonaBess exports"]
fn real_vendor_boot_images_match_known_shapes_and_apply_exactly() {
    let sun_image = std::fs::read(required_path("KONABESS_TB322_IMAGE")).unwrap();
    let sun_export = read_export(&required_path("KONABESS_TB322_EXPORT")).unwrap();
    let sun_classified = classify_vendor_boot_dtbs(&sun_image, &sun_export).unwrap();
    let sun_matches = sun_classified
        .iter()
        .filter(|blob| blob.structurally_matches)
        .map(|blob| blob.info.index)
        .collect::<Vec<_>>();
    assert_eq!(sun_classified.len(), 13);
    assert_eq!(sun_matches, [2, 4, 6, 8]);
    assert!(
        sun_classified
            .iter()
            .all(|blob| blob.info.chip.as_deref() == Some("sun"))
    );
    println!("TB322FC: blobs=13 structural_matches={sun_matches:?}");

    let original_blobs = extract_vendor_boot_dtbs(&sun_image).unwrap();
    let original_info = parse_fdt_gpu_info(&original_blobs[2]).unwrap();
    let original_table = original_info.table.unwrap();
    let self_export = KonaBessExport {
        chip: "sun".into(),
        description: "fixture round-trip".into(),
        table: original_table.clone(),
    };
    let round_trip = replace_vendor_boot_dtb(&sun_image, 2, &self_export).unwrap();
    let round_trip_blobs = extract_vendor_boot_dtbs(&round_trip).unwrap();
    assert_eq!(round_trip_blobs[2], original_blobs[2]);
    println!("TB322FC blob 2: same-table round-trip is byte-identical");

    let applied = replace_vendor_boot_dtb(&sun_image, 2, &sun_export).unwrap();
    let applied_blobs = extract_vendor_boot_dtbs(&applied).unwrap();
    let applied_table = parse_fdt_gpu_info(&applied_blobs[2])
        .unwrap()
        .table
        .unwrap();
    assert_eq!(applied_table, sun_export.table);
    let changed_cells = changed_cell_count(&original_table, &applied_table);
    assert_eq!(changed_cells, 6);
    assert_eq!(
        original_blobs
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != 2)
            .map(|(_, blob)| blob)
            .collect::<Vec<_>>(),
        applied_blobs
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != 2)
            .map(|(_, blob)| blob)
            .collect::<Vec<_>>()
    );
    println!("TB322FC blob 2: export applied, changed_u32_cells={changed_cells}");

    let pineapple_image = std::fs::read(required_path("KONABESS_TB321_IMAGE")).unwrap();
    let export_paths = env::split_paths(
        &env::var_os("KONABESS_TB321_EXPORTS").expect("KONABESS_TB321_EXPORTS must be set"),
    )
    .collect::<Vec<_>>();
    assert_eq!(export_paths.len(), 4);
    for export_path in export_paths {
        let export = read_export(&export_path).unwrap();
        let classified = classify_vendor_boot_dtbs(&pineapple_image, &export).unwrap();
        assert_eq!(classified.len(), 11);
        assert!(classified.iter().all(|blob| !blob.structurally_matches));
        assert!(classified.iter().all(|blob| {
            blob.info.chip.as_deref() == Some("pineapple") || blob.info.gpu_shape.is_none()
        }));
        println!(
            "TB321FU {}: blobs=11 structural_matches=[]",
            export_path.display()
        );
    }
}
