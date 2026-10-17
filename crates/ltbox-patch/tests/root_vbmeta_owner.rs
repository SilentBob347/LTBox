use std::path::Path;

use avbtool_rs::builder::{ChainPartitionSpec, PropertySpec, VbmetaImageArgs, make_vbmeta_image};
use avbtool_rs::info::DescriptorInfo;
use ltbox_patch::avb::{self, VbmetaOwner};

const KEY: &str = "testkey_rsa2048";
// Deliberately different from the key's default: rebuilding must retain this.
const ALGORITHM: &str = "SHA512_RSA2048";

fn hash(target: &str) -> DescriptorInfo {
    DescriptorInfo::Hash {
        image_size: 4096,
        hash_algorithm: "sha256".into(),
        partition_name: target.into(),
        salt: vec![1, 2],
        digest: vec![3; 32],
        flags: 0,
    }
}

fn chain() -> DescriptorInfo {
    DescriptorInfo::ChainPartition {
        rollback_index_location: 2,
        partition_name: "vbmeta_system".into(),
        public_key: avbtool_rs::crypto::extract_public_key(KEY).unwrap(),
        flags: 0,
    }
}

#[test]
fn owner_decision_accepts_old_and_new_layouts() {
    let top = [chain(), hash("init_boot")];
    assert_eq!(
        avb::decide_root_vbmeta_owner("init_boot", &top, Some(&[hash("pvmfw")])).unwrap(),
        VbmetaOwner::Vbmeta
    );
    assert_eq!(
        avb::decide_root_vbmeta_owner(
            "init_boot",
            &[chain()],
            Some(&[hash("init_boot"), hash("pvmfw")])
        )
        .unwrap(),
        VbmetaOwner::VbmetaSystem
    );
    assert_eq!(
        avb::decide_root_vbmeta_owner("init_boot", &[hash("init_boot")], None).unwrap(),
        VbmetaOwner::Vbmeta
    );
    assert_eq!(VbmetaOwner::Vbmeta.lun().unwrap(), 4);
    assert_eq!(VbmetaOwner::VbmetaSystem.lun().unwrap(), 0);
}

#[test]
fn owner_decision_rejects_none_duplicate_both_and_uninspected_chain() {
    assert!(avb::decide_root_vbmeta_owner("init_boot", &[chain()], Some(&[])).is_err());
    assert!(
        avb::decide_root_vbmeta_owner("init_boot", &[hash("init_boot"), hash("init_boot")], None)
            .is_err()
    );
    assert!(
        avb::decide_root_vbmeta_owner(
            "init_boot",
            &[chain()],
            Some(&[hash("init_boot"), hash("init_boot")])
        )
        .is_err()
    );
    assert!(
        avb::decide_root_vbmeta_owner(
            "init_boot",
            &[chain(), hash("init_boot")],
            Some(&[hash("init_boot")])
        )
        .is_err()
    );
    assert!(
        avb::decide_root_vbmeta_owner("init_boot", &[chain(), hash("init_boot")], None).is_err()
    );
    assert!(avb::decide_root_vbmeta_owner("init_boot", &[], Some(&[hash("init_boot")])).is_err());
    assert!(
        avb::decide_root_vbmeta_owner("init_boot", &[chain(), chain()], Some(&[hash("init_boot")]))
            .is_err()
    );
}

#[test]
fn unusual_system_chains_only_block_a_chained_root_hash_owner() {
    let mut non_ab = chain();
    if let DescriptorInfo::ChainPartition { flags, .. } = &mut non_ab {
        *flags = 1;
    }
    for chains in [vec![non_ab], vec![chain(), chain()]] {
        let mut top = chains.clone();
        top.push(hash("init_boot"));
        assert_eq!(
            avb::decide_root_vbmeta_owner("init_boot", &top, Some(&[hash("pvmfw")])).unwrap(),
            VbmetaOwner::Vbmeta
        );
        // Neither an invalid chain nor a top-level hash hides a second owner.
        assert!(
            avb::decide_root_vbmeta_owner("init_boot", &top, Some(&[hash("init_boot")])).is_err()
        );
        assert!(
            avb::decide_root_vbmeta_owner("init_boot", &chains, Some(&[hash("init_boot")]))
                .is_err()
        );
        assert!(avb::decide_root_vbmeta_owner("init_boot", &chains, Some(&[])).is_err());
    }
}

fn write_payload(path: &Path, target: &str) {
    std::fs::write(path, vec![0x41; 4096]).unwrap();
    avbtool_rs::footer::add_hash_footer(
        path,
        &avbtool_rs::footer::HashFooterArgs {
            partition_size: Some(128 * 1024),
            dynamic_partition_size: false,
            partition_name: target.into(),
            hash_algorithm: "sha256".into(),
            salt: Some(vec![1, 2]),
            chain_partitions: Vec::new(),
            algorithm_name: "NONE".into(),
            key_spec: None,
            public_key_metadata: None,
            rollback_index: 0,
            flags: 0,
            rollback_index_location: 0,
            properties: Vec::new(),
            kernel_cmdlines: Vec::new(),
            include_descriptors_from_images: Vec::new(),
            release_string: None,
            append_to_release_string: None,
            output_vbmeta_image: None,
            do_not_append_vbmeta_image: false,
            use_persistent_digest: false,
            do_not_use_ab: false,
        },
    )
    .unwrap();
}

fn args() -> VbmetaImageArgs {
    VbmetaImageArgs {
        algorithm_name: ALGORITHM.into(),
        key_spec: Some(KEY.into()),
        public_key_metadata: None,
        rollback_index: 1_785_888_000,
        flags: 0,
        rollback_index_location: 0,
        properties: vec![PropertySpec {
            key: "com.android.build.system.fingerprint".into(),
            value: b"Lenovo/TB520FU/TB520FU:17/CP2A/test:user/test-keys".to_vec(),
        }],
        kernel_cmdlines: Vec::new(),
        extra_descriptors: Vec::new(),
        include_descriptors_from_images: Vec::new(),
        chain_partitions: Vec::new(),
        release_string: None,
        append_to_release_string: None,
        padding_size: 4096,
    }
}

/// Exercise the same owner rebuild used by Root, without downloads or a device.
fn exercise_rebuild(dir: &Path, real_firmware: bool) {
    use std::io::{Read, Seek, Write};
    let top = dir.join("vbmeta.img");
    let system = dir.join("vbmeta_system.img");
    let payload = dir.join("init_boot.img");
    let original_top = std::fs::read(&top).unwrap();
    let before = avbtool_rs::image::inspect_avb_image(&system).unwrap();
    let old_hash = avb::hash_descriptor(&system, "init_boot").unwrap();
    let old_pvmfw = avb::hash_descriptor(&system, "pvmfw").unwrap();
    assert_eq!(
        avb::detect_root_vbmeta_owner(dir, "init_boot").unwrap(),
        VbmetaOwner::VbmetaSystem
    );
    let stock_info = avb::extract_image_avb_info(&payload).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&payload)
        .unwrap();
    // Mutate authenticated payload bytes, keeping a real boot header intact.
    file.seek(std::io::SeekFrom::Start(4095)).unwrap();
    let mut byte = [0];
    file.read_exact(&mut byte).unwrap();
    byte[0] ^= 1;
    file.seek(std::io::SeekFrom::Start(4095)).unwrap();
    file.write_all(&byte).unwrap();
    drop(file);
    avb::add_hash_footer(&payload, &stock_info, None, None).unwrap();
    let output = dir.join("vbmeta_system.rebuilt.img");
    avb::rebuild_root_vbmeta_owner(&output, &system, &payload, "init_boot", KEY).unwrap();
    let after = avbtool_rs::image::inspect_avb_image(&output).unwrap();
    assert_ne!(
        avb::hash_descriptor(&output, "init_boot").unwrap(),
        old_hash
    );
    assert_eq!(
        avb::hash_descriptor(&output, "init_boot").unwrap(),
        avb::hash_descriptor(&payload, "init_boot").unwrap()
    );
    assert_eq!(avb::hash_descriptor(&output, "pvmfw").unwrap(), old_pvmfw);
    let unrelated = |descriptors: Vec<DescriptorInfo>| {
        descriptors.into_iter().filter(|d| !matches!(d, DescriptorInfo::Hash { partition_name, .. } if partition_name == "init_boot")).collect::<Vec<_>>()
    };
    let mut preserved = unrelated(before.descriptors);
    for descriptor in unrelated(after.descriptors.clone()) {
        let index = preserved
            .iter()
            .position(|value| value == &descriptor)
            .expect("a non-target descriptor changed value");
        preserved.swap_remove(index);
    }
    assert!(preserved.is_empty(), "a non-target descriptor was removed");
    assert_eq!(after.header.rollback_index, before.header.rollback_index);
    assert_eq!(
        after.header.rollback_index_location,
        before.header.rollback_index_location
    );
    assert_eq!(after.header.flags, before.header.flags);
    assert_eq!(after.algorithm_name, before.algorithm_name);
    assert_eq!(std::fs::read(&top).unwrap(), original_top);
    // For a real mixed owner, the full verifier would require every logical
    // partition (and its hashtree). Verify only the vbmeta signature directly.
    let blob = avbtool_rs::image::load_vbmeta_blob(&output).unwrap();
    let header = &after.header;
    let aux_start = 256 + header.authentication_data_block_size as usize;
    let mut signed = blob[..256].to_vec();
    signed
        .extend_from_slice(&blob[aux_start..aux_start + header.auxiliary_data_block_size as usize]);
    let signature_start = 256 + header.signature_offset as usize;
    let signature = &blob[signature_start..signature_start + header.signature_size as usize];
    let public = avbtool_rs::crypto::load_key_from_spec(KEY).unwrap();
    // The public key API verifies PKCS#1 signing of the authenticated metadata.
    let key = avbtool_rs::crypto::AvbPublicKey::decode(&public.encode_public_key()).unwrap();
    let algorithm = avbtool_rs::crypto::lookup_algorithm_by_name(&after.algorithm_name).unwrap();
    assert!(key.verify(algorithm, signature, &signed).unwrap());
    let digest = avbtool_rs::crypto::compute_hash_for_algorithm(algorithm, &signed).unwrap();
    let hash_start = 256 + header.hash_offset as usize;
    assert_eq!(
        &blob[hash_start..hash_start + header.hash_size as usize],
        digest
    );
    let key_start = aux_start + header.public_key_offset as usize;
    assert_eq!(
        &blob[key_start..key_start + header.public_key_size as usize],
        public.encode_public_key()
    );
    if !real_firmware {
        avbtool_rs::verify::verify_image(
            &output,
            &avbtool_rs::verify::VerifyImageOptions {
                key_blob: Some(public.encode_public_key()),
                expected_chain_partitions: Vec::new(),
                follow_chain_partitions: false,
                accept_zeroed_hashtree: false,
            },
        )
        .unwrap();
    }
}

#[test]
fn a17_owner_rebuild_preserves_other_hashes_properties_and_rollback() {
    let temp = tempfile::tempdir().unwrap();
    write_payload(&temp.path().join("init_boot.img"), "init_boot");
    write_payload(&temp.path().join("pvmfw.img"), "pvmfw");
    let mut system = args();
    system.include_descriptors_from_images = vec![
        temp.path().join("init_boot.img"),
        temp.path().join("pvmfw.img"),
    ];
    make_vbmeta_image(&temp.path().join("vbmeta_system.img"), &system).unwrap();
    let mut top = args();
    top.rollback_index = 0;
    top.properties.clear();
    top.chain_partitions.push(ChainPartitionSpec {
        partition_name: "vbmeta_system".into(),
        rollback_index_location: 2,
        public_key: avbtool_rs::crypto::extract_public_key(KEY).unwrap(),
        flags: 0,
    });
    make_vbmeta_image(&temp.path().join("vbmeta.img"), &top).unwrap();
    exercise_rebuild(temp.path(), false);
}

#[test]
fn a17_root_pipeline_outputs_only_the_detected_system_owner() {
    use ltbox_patch::root_pipeline::{
        RootFamily, RootImageTarget, RootPipelineConfig, RootProvider, RootVersion,
        build_patched_artifacts,
    };
    use std::io::Write;

    ltbox_patch::boot::register_magiskboot_host(env!("CARGO_BIN_EXE_ltbox-magiskboot").into())
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("work");
    let output = temp.path().join("out");
    std::fs::create_dir(&work).unwrap();
    // Create an actual ramdisk archive and a kernel-free Android v4 init_boot.
    std::fs::write(work.join("stock_init"), b"stock init").unwrap();
    ltbox_patch::boot::cpio_checked(&work, "ramdisk.cpio", &["add 0755 init stock_init"]).unwrap();
    let ramdisk = std::fs::read(work.join("ramdisk.cpio")).unwrap();
    let mut image = vec![0u8; 4096];
    image[..8].copy_from_slice(b"ANDROID!");
    image[12..16].copy_from_slice(&(ramdisk.len() as u32).to_le_bytes());
    image[20..24].copy_from_slice(&1584u32.to_le_bytes());
    image[40..44].copy_from_slice(&4u32.to_le_bytes());
    image.extend_from_slice(&ramdisk);
    image.resize(image.len().next_multiple_of(4096), 0);
    let root = work.join("init_boot.img");
    write_payload(&root, "init_boot");
    let footer = avb::extract_image_avb_info(&root).unwrap();
    std::fs::write(&root, image).unwrap();
    avb::add_hash_footer(&root, &footer, None, None).unwrap();
    write_payload(&work.join("pvmfw.img"), "pvmfw");
    let mut system = args();
    system.include_descriptors_from_images = vec![root.clone(), work.join("pvmfw.img")];
    make_vbmeta_image(&work.join("vbmeta_system.img"), &system).unwrap();
    let mut top = args();
    top.rollback_index = 0;
    top.properties.clear();
    top.extra_descriptors.push(chain());
    let top_path = work.join("vbmeta.img");
    make_vbmeta_image(&top_path, &top).unwrap();
    let original_top = std::fs::read(&top_path).unwrap();
    let original_hash = avb::hash_descriptor(&root, "init_boot").unwrap();

    // Staged local inputs keep the pipeline offline. These ELF headers are
    // validated and packed into the ramdisk, never executed by the test.
    for (name, kind) in [("init", 3u16), ("kernelsu.ko", 1u16)] {
        let mut elf = vec![0; 64];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2;
        elf[5] = 1;
        elf[16..18].copy_from_slice(&kind.to_le_bytes());
        elf[18..20].copy_from_slice(&183u16.to_le_bytes());
        std::fs::write(work.join(name), elf).unwrap();
    }
    let mut apk = zip::ZipWriter::new(std::fs::File::create(work.join("manager.apk")).unwrap());
    apk.start_file(
        "AndroidManifest.xml",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    apk.write_all(b"manifest").unwrap();
    apk.finish().unwrap();
    let owner = avb::detect_root_vbmeta_owner(&work, "init_boot").unwrap();
    assert_eq!(owner, VbmetaOwner::VbmetaSystem);
    let config = RootPipelineConfig {
        local_ksu: None,
        family: RootFamily::KernelSU,
        provider: RootProvider::KernelSULocal,
        version: RootVersion::Stable,
        root_image_target: RootImageTarget::InitBoot,
        rebuild_vbmeta: true,
        vbmeta_owner: Some(VbmetaOwner::VbmetaSystem),
        work_dir: work,
        output_dir: output.clone(),
        loader: Default::default(),
        slot_suffix: "_b".into(),
        preinit_device: String::new(),
        gki_kernel_zip: None,
        kernel_version: None,
        kernel_gki_branch: None,
        gki_mode: false,
        kpm_paths: Vec::new(),
        superkey: String::new(),
        magisk_forks_apk: None,
        nightly_run_id: None,
        release_tag: None,
    };
    let artifacts = build_patched_artifacts(&config, false, &mut Vec::new()).unwrap();
    assert_eq!(
        artifacts.vbmeta_partition.as_deref(),
        Some("vbmeta_system_b")
    );
    assert_eq!(artifacts.vbmeta_owner, Some(VbmetaOwner::VbmetaSystem));
    let owner_image = artifacts.patched_vbmeta.unwrap();
    assert_eq!(owner_image, output.join("vbmeta_system.img"));
    assert_ne!(
        avb::hash_descriptor(&artifacts.patched_root_image, "init_boot").unwrap(),
        original_hash
    );
    avb::verify_root_backup(
        &artifacts.patched_root_image,
        Some(&owner_image),
        "init_boot",
    )
    .unwrap();
    assert!(!output.join("vbmeta.img").exists());
    assert_eq!(std::fs::read(top_path).unwrap(), original_top);
}

#[test]
#[ignore = "set LTBOX_A17_FIRMWARE to the A17 firmware image directory"]
fn real_a17_owner_detection_and_rebuild() {
    let firmware = std::env::var_os("LTBOX_A17_FIRMWARE")
        .expect("LTBOX_A17_FIRMWARE must name an image directory");
    let firmware = Path::new(&firmware);
    let temp = tempfile::tempdir().unwrap();
    for name in ["vbmeta.img", "vbmeta_system.img", "init_boot.img"] {
        std::fs::copy(firmware.join(name), temp.path().join(name)).unwrap();
    }
    // Stock Lenovo keys cannot be used by LTBox. Simulate testkey firmware
    // entirely in temporary copies, including the top-level chain's pinned key.
    avb::resign_image(&temp.path().join("vbmeta_system.img"), KEY, ALGORITHM, None).unwrap();
    let top = temp.path().join("vbmeta.img");
    let resigned_top = temp.path().join("top.resigned.img");
    avb::rebuild_vbmeta_with_chain_key_overrides(
        &resigned_top,
        &top,
        &["vbmeta_system"],
        KEY,
        KEY,
        ALGORITHM,
    )
    .unwrap();
    std::fs::rename(resigned_top, top).unwrap();
    exercise_rebuild(temp.path(), true);
}
