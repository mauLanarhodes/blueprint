use bp_icons::{
    IconError, MAX_SVG_BYTES, import_zip, install_pack, load_packs, search, validate_svg,
};
use bp_model::{CloudProvider, IconKind};
use std::fs;
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

// These simple fixtures are authored for tests, not copied provider artwork.
const SVG: &str = "<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"64\"><path fill=\"#123456\" d=\"M2 2H62V62H2Z\"/></svg>\n";
const SVG_ALT: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 64 64\"><circle cx=\"32\" cy=\"32\" r=\"30\" fill=\"blue\"/></svg>";

fn archive(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, data) in entries {
        writer
            .start_file(*path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(data.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn scratch() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "bp-icons-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn official_aws_naming_preserves_bytes_and_picks_largest_variant() {
    let bytes = archive(&[
        (
            "Architecture-Service-Icons/Arch_Compute/32/Arch_Amazon-EC2_32.svg",
            SVG_ALT,
        ),
        (
            "Architecture-Service-Icons/Arch_Compute/64/Arch_Amazon-EC2_64.svg",
            SVG,
        ),
        (
            "Resource-Icons/Res_Storage/Res_Amazon-S3_Bucket_48.svg",
            SVG,
        ),
        ("Architecture-Group-Icons/Arch_AWS-Cloud_32.svg", SVG),
        ("__MACOSX/._junk.svg", "junk"),
        ("Readme.pdf", "not an SVG"),
    ]);
    let pack = import_zip(&bytes, CloudProvider::Aws, "2026.10").unwrap();
    assert_eq!(pack.icons.len(), 3);
    let ec2 = search(&pack, "compute vm")[0];
    assert_eq!(ec2.name, "Amazon EC2");
    assert_eq!(
        ec2.reference.as_str(),
        "aws/compute-service-amazon-ec2@2026.10"
    );
    assert_eq!(ec2.svg.as_ref(), SVG);
    assert_eq!(
        ec2.source_path,
        "Architecture-Service-Icons/Arch_Compute/64/Arch_Amazon-EC2_64.svg"
    );
    assert_eq!(search(&pack, "bucket")[0].kind, IconKind::Resource);
    assert!(pack.icons.iter().any(|i| i.kind == IconKind::Group));
}

#[test]
fn azure_names_categories_and_common_aliases() {
    let bytes = archive(&[
        (
            "Azure_Public_Service_Icons/Icons/compute/10021-icon-service-Virtual-Machines.svg",
            SVG,
        ),
        (
            "Azure_Public_Service_Icons/Icons/storage/10002-icon-service-Blob-Storage.svg",
            SVG,
        ),
    ]);
    let pack = import_zip(&bytes, CloudProvider::Azure, "v25").unwrap();
    let vm = search(&pack, "virtualmachine")[0];
    assert_eq!(vm.name, "Virtual Machines");
    assert_eq!(vm.category, "Compute");
    assert_eq!(
        vm.reference.as_str(),
        "azure/compute-service-virtual-machines@v25"
    );
    assert_eq!(search(&pack, "object storage")[0].name, "Blob Storage");
    assert_eq!(search(&pack, "").len(), 2);
}

#[test]
fn azure_same_name_assets_survive_import_install_and_reload_in_any_zip_order() {
    // V24 contains these two pairs of filenames. Artwork here is authored for
    // this regression test; preserve different vendor assets even if their
    // names or SVG bytes match.
    let entries = [
        (
            "Azure_Public_Service_Icons/Icons/compute/00330-icon-service-Workspaces.svg",
            SVG,
        ),
        (
            "Azure_Public_Service_Icons/Icons/compute/00400-icon-service-Workspaces.svg",
            SVG_ALT,
        ),
        (
            "Azure_Public_Service_Icons/Icons/networking/02302-icon-service-Load-Balancer-Hub.svg",
            SVG,
        ),
        (
            "Azure_Public_Service_Icons/Icons/networking/029029174-icon-service-Load-Balancer-Hub.svg",
            SVG,
        ),
        (
            "Azure_Public_Service_Icons/Icons/compute/00999-icon-service-Workspaces-00330.svg",
            SVG,
        ),
    ];
    let pack = import_zip(&archive(&entries), CloudProvider::Azure, "v24").unwrap();
    let reversed: Vec<_> = entries.iter().copied().rev().collect();
    assert_eq!(
        pack,
        import_zip(&archive(&reversed), CloudProvider::Azure, "v24").unwrap()
    );
    assert_eq!(pack.icons.len(), entries.len());
    assert!(pack.warnings.is_empty(), "distinct assets are not skipped");
    let expected_ids = [
        "compute-service-workspaces--00330@v24",
        "compute-service-workspaces--00400@v24",
        "networking-service-load-balancer-hub--02302@v24",
        "networking-service-load-balancer-hub--029029174@v24",
        "compute-service-workspaces-00330@v24",
    ];
    for ((path, svg), id) in entries.iter().zip(expected_ids) {
        let icon = pack
            .icons
            .iter()
            .find(|icon| icon.source_path == *path)
            .unwrap();
        assert_eq!(icon.reference, bp_model::ShapeRef::new("azure", id));
        assert_eq!(icon.svg.as_ref(), *svg);
    }
    assert_eq!(search(&pack, "load balancer hub").len(), 2);
    assert_eq!(
        pack.icons
            .iter()
            .filter(|icon| icon.name == "Workspaces")
            .count(),
        2
    );
    let root = scratch();
    install_pack(&root, &pack).unwrap();
    install_pack(&root, &pack).unwrap();
    assert_eq!(load_packs(&root).unwrap(), vec![pack]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn azure_conflicting_artwork_with_the_same_vendor_identity_still_fails() {
    let entries = [
        ("a/Icons/compute/00330-icon-service-Workspaces.svg", SVG),
        ("b/Icons/compute/00330-icon-service-Workspaces.svg", SVG_ALT),
    ];
    assert!(matches!(
        import_zip(&archive(&entries), CloudProvider::Azure, "v24"),
        Err(IconError::Collision { .. })
    ));
}

#[test]
fn azure_unambiguous_ids_stay_compatible_and_vendor_ids_are_validated() {
    let entries = [("Icons/compute/10021-icon-service-Virtual-Machines.svg", SVG)];
    let mut pack = import_zip(&archive(&entries), CloudProvider::Azure, "v24").unwrap();
    assert_eq!(
        pack.icons[0].reference.as_str(),
        "azure/compute-service-virtual-machines@v24"
    );
    let root = scratch();
    install_pack(&root, &pack).unwrap();
    assert_eq!(load_packs(&root).unwrap(), vec![pack.clone()]);
    pack.icons[0].reference =
        bp_model::ShapeRef::new("azure", "compute-service-virtual-machines--99999@v24");
    assert!(matches!(
        install_pack(&root, &pack),
        Err(IconError::Catalog(_))
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn service_names_do_not_turn_services_into_groups_or_resources() {
    let aws = import_zip(&archive(&[
        ("Architecture-Service-Icons/Arch_Management-Governance/64/Arch_AWS-Resource-Groups_64.svg", SVG),
        ("Architecture-Service-Icons/Arch_Management-Governance/64/Arch_AWS-Resource-Explorer_64.svg", SVG),
    ]), CloudProvider::Aws, "v1").unwrap();
    assert!(aws.icons.iter().all(|icon| icon.kind == IconKind::Service));
    let azure = import_zip(
        &archive(&[("Icons/general/10007-icon-service-Resource-Groups.svg", SVG)]),
        CloudProvider::Azure,
        "v1",
    )
    .unwrap();
    assert_eq!(azure.icons[0].kind, IconKind::Service);
}

#[test]
fn aws_category_sizes_and_themed_resource_folders_have_stable_names() {
    let pack = import_zip(&archive(&[
        ("Category-Icons_07312026/Arch-Category_16/Arch-Category_Compute_16.svg", SVG_ALT),
        ("Category-Icons_07312026/Arch-Category_64/Arch-Category_Compute_64.svg", SVG),
        ("Resource-Icons_07312026/Res_General-Icons/Res_48_Light/Res_Server_48_Light.svg", SVG),
        ("Resource-Icons_07312026/Res_General-Icons/Res_48_Dark/Res_Server_48_Dark.svg", SVG_ALT),
        ("Architecture-Service-Icons/Arch_Storage/64/Arch_Amazon-Simple-Storage-Service_64.svg", SVG),
    ]), CloudProvider::Aws, "v1").unwrap();
    assert_eq!(pack.icons.len(), 4);
    let category = pack
        .icons
        .iter()
        .find(|icon| icon.name == "Compute")
        .unwrap();
    assert_eq!(category.category, "Categories");
    assert_eq!(category.svg.as_ref(), SVG);
    for theme in ["Light", "Dark"] {
        let icon = pack
            .icons
            .iter()
            .find(|icon| icon.name == format!("Server {theme}"))
            .unwrap();
        assert_eq!(icon.category, "General Icons");
        assert!(!icon.name.contains("48"));
    }
    assert!(
        search(&pack, "bucket")
            .iter()
            .any(|icon| icon.name == "Amazon Simple Storage Service")
    );
}

#[test]
fn stable_independent_of_zip_order() {
    let small = ("Arch_Compute/32/Arch_Amazon-EC2_32.svg", SVG_ALT);
    let large = ("Arch_Compute/64/Arch_Amazon-EC2_64.svg", SVG);
    assert_eq!(
        import_zip(&archive(&[small, large]), CloudProvider::Aws, "v1").unwrap(),
        import_zip(&archive(&[large, small]), CloudProvider::Aws, "v1").unwrap()
    );
}

#[test]
fn rejects_paths_symlinks_and_ambiguous_artwork() {
    for path in [
        "../icon.svg",
        "/icon.svg",
        "C:/icon.svg",
        "folder\\icon.svg",
    ] {
        assert!(matches!(
            import_zip(&archive(&[(path, SVG)]), CloudProvider::Aws, "v1"),
            Err(IconError::UnsafePath(_))
        ));
    }
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .add_symlink("linked.svg", "/etc/passwd", SimpleFileOptions::default())
        .unwrap();
    assert!(matches!(
        import_zip(
            &writer.finish().unwrap().into_inner(),
            CloudProvider::Aws,
            "v1"
        ),
        Err(IconError::UnsafePath(_))
    ));
    let collision = archive(&[
        ("a/Arch_Compute/64/Arch_Amazon-EC2_64.svg", SVG),
        ("b/Arch_Compute/64/Arch_Amazon-EC2_64.svg", SVG_ALT),
    ]);
    assert!(matches!(
        import_zip(&collision, CloudProvider::Aws, "v1"),
        Err(IconError::Collision { .. })
    ));
}

#[test]
fn rejects_empty_malformed_unsafe_and_oversized_svgs() {
    assert!(matches!(
        import_zip(
            &archive(&[("readme.txt", "text")]),
            CloudProvider::Aws,
            "v1"
        ),
        Err(IconError::Empty(_))
    ));
    assert!(matches!(
        import_zip(&archive(&[("icon.svg", "<svg")]), CloudProvider::Aws, "v1"),
        Err(IconError::Svg { .. })
    ));
    for content in [
        "<script>alert(1)</script>",
        "<path d=\"M0 0H10V10Z\"/><text x=\"1\" y=\"10\">label</text>",
        "<path d=\"M0 0H10V10Z\"/><tspan>label</tspan>",
        "<path d=\"M0 0H10V10Z\"/><textPath href=\"#p\">label</textPath>",
        "<path onclick=\"alert(1)\" d=\"M0 0H10V10Z\"/>",
        "<image href=\"file:///etc/passwd\"/>",
        "<use href=\"https://example.org/icon.svg#icon\"/>",
        "<style>@import url('https://example.org/style.css')</style>",
        "<path fill=\"url(https://example.org/a.svg)\" d=\"M0 0H10V10Z\"/>",
    ] {
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"64\">{content}</svg>"
        );
        assert!(validate_svg(&svg).is_err(), "accepted {content}");
    }
    let large = "x".repeat(MAX_SVG_BYTES + 1);
    assert!(matches!(
        import_zip(&archive(&[("large.svg", &large)]), CloudProvider::Aws, "v1"),
        Err(IconError::Limit(_))
    ));
}

#[test]
fn allows_local_gradient_and_use_definitions() {
    validate_svg("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"64\"><defs><linearGradient id=\"g\"><stop stop-color=\"red\"/><stop offset=\"1\" stop-color=\"blue\"/></linearGradient><path id=\"p\" d=\"M0 0H64V64Z\"/></defs><use href=\"#p\" fill=\"url(#g)\"/></svg>").unwrap();
}

#[test]
fn installs_versions_atomically_and_refuses_same_version_changes() {
    let root = scratch();
    let bytes = archive(&[("Arch_Compute/64/Arch_Amazon-EC2_64.svg", SVG)]);
    let first = import_zip(&bytes, CloudProvider::Aws, "v1").unwrap();
    let second = import_zip(&bytes, CloudProvider::Aws, "v2").unwrap();
    assert_eq!(install_pack(&root, &first).unwrap(), root.join("aws/v1"));
    install_pack(&root, &first).unwrap();
    install_pack(&root, &second).unwrap();
    assert_eq!(load_packs(&root).unwrap(), vec![first.clone(), second]);
    let changed = import_zip(
        &archive(&[("Arch_Compute/64/Arch_Amazon-EC2_64.svg", SVG_ALT)]),
        CloudProvider::Aws,
        "v1",
    )
    .unwrap();
    assert!(matches!(
        install_pack(&root, &changed),
        Err(IconError::AlreadyInstalled { .. })
    ));
    assert_eq!(load_packs(&root).unwrap()[0], first);
    assert_eq!(fs::read_dir(root.join("aws")).unwrap().count(), 2);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_unsafe_versions_and_invalid_catalog_metadata() {
    let bytes = archive(&[("Arch_Compute/64/Arch_Amazon-EC2_64.svg", SVG)]);
    for version in ["", "..", "../v1", "v1/new", "version with spaces"] {
        assert!(matches!(
            import_zip(&bytes, CloudProvider::Aws, version),
            Err(IconError::InvalidVersion(_))
        ));
    }
    let root = scratch();
    let mut pack = import_zip(&bytes, CloudProvider::Aws, "v1").unwrap();
    pack.icons[0].pack_version = "v2".into();
    assert!(matches!(
        install_pack(&root, &pack),
        Err(IconError::Catalog(_))
    ));
    assert!(!root.exists());
}

#[test]
fn concurrent_identical_installs_publish_one_complete_catalog() {
    let root = scratch();
    let pack = import_zip(
        &archive(&[("Arch_Compute/64/Arch_Amazon-EC2_64.svg", SVG)]),
        CloudProvider::Aws,
        "v1",
    )
    .unwrap();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| install_pack(&root, &pack)))
            .collect();
        for handle in handles {
            handle.join().unwrap().unwrap();
        }
    });
    assert_eq!(load_packs(&root).unwrap(), vec![pack]);
    assert_eq!(fs::read_dir(root.join("aws")).unwrap().count(), 1);
    fs::remove_dir_all(root).unwrap();
}
