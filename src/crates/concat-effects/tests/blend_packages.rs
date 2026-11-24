// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah

//! External blend loading, refusal, identity and startup-snapshot behaviour.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use concat_effects::{Catalogue, Manifest, Package};

const MANIFEST: &str = include_str!("fixtures/blend-photocopy/effect.toml");
const SOURCE: &str = include_str!("fixtures/blend-photocopy/photocopy.wgsl");

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "bobbycat-blend-packages-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).expect("a new test directory");
        Self(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_package(root: &Path, folder: &str, manifest: &str, source: &str) -> PathBuf {
    let dir = root.join(folder);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("effect.toml"), manifest).unwrap();
    std::fs::write(dir.join("photocopy.wgsl"), source).unwrap();
    dir
}

#[test]
fn a_bad_shader_is_reported_without_hiding_a_good_blend() {
    let root = Directory::new();
    write_package(&root.0, "good", MANIFEST, SOURCE);
    write_package(
        &root.0,
        "bad",
        &MANIFEST.replace("plug.photocopy", "example.bad"),
        "\n\nfn blend( ?",
    );
    let mut catalogue = Catalogue::new();
    let errors = catalogue.load_dir(&root.0);
    assert_eq!(errors.len(), 1, "{errors:?}");
    let error = errors[0].to_string();
    assert!(error.contains("photocopy.wgsl"), "{error}");
    assert!(error.contains(":3:"), "{error}");
    assert_eq!(
        catalogue.blends().collect::<Vec<_>>(),
        vec![("plug.photocopy", "Photocopy")]
    );
    assert!(catalogue.blend_pass(" plug.photocopy ").is_some());
    assert!(catalogue.blend_pass("normal").is_none());
    assert!(catalogue.blend_pass("unknown.mode").is_none());
}

#[test]
fn duplicate_ids_and_builtin_aliases_cannot_replace_a_blend() {
    let mut catalogue = Catalogue::new();
    let first = Package::from_sources(MANIFEST, None, Some(SOURCE)).unwrap();
    catalogue.add(first.clone()).unwrap();
    assert!(
        catalogue
            .add(first)
            .unwrap_err()
            .to_string()
            .contains("already taken")
    );
    let reserved = MANIFEST
        .replace(
            "kind = \"blend\"",
            "kind = \"blend\"\naliases = [\"soft-light\"]",
        )
        .replace("plug.photocopy", "example.reserved");
    let package = Package::from_sources(&reserved, None, Some(SOURCE)).unwrap();
    assert!(
        catalogue
            .add(package)
            .unwrap_err()
            .to_string()
            .contains("built-in blend")
    );
    assert_eq!(
        catalogue
            .blend_pass("plug.photocopy")
            .unwrap()
            .source
            .as_ref(),
        SOURCE
    );
}

#[test]
fn only_the_first_install_supplies_external_blends() {
    let root = Directory::new();
    let first = write_package(&root.0, "first", MANIFEST, SOURCE);
    let ordinary = root.0.join("ordinary");
    std::fs::create_dir(&ordinary).unwrap();
    let ordinary_manifest = "format = 2\n[effect]\nid = \"example.ordinary\"\nname = \"Before\"\nkind = \"effect\"\n[wgsl]\nentry = \"effect.wgsl\"\n";
    std::fs::write(ordinary.join("effect.toml"), ordinary_manifest).unwrap();
    std::fs::write(
        ordinary.join("effect.wgsl"),
        "fn effect(uv: vec2<f32>) -> vec4<f32> { return sample(uv); }",
    )
    .unwrap();
    let mut catalogue = Catalogue::new();
    let errors = catalogue.install_dir(&root.0);
    assert!(errors.is_empty(), "{errors:?}");
    let frozen = catalogue.blend_pass("plug.photocopy").unwrap();
    std::fs::write(first.join("photocopy.wgsl"), "invalid new source").unwrap();
    std::fs::write(
        ordinary.join("effect.toml"),
        ordinary_manifest.replace("Before", "After"),
    )
    .unwrap();
    write_package(
        &root.0,
        "second",
        &MANIFEST.replace("plug.photocopy", "example.new"),
        SOURCE,
    );
    assert!(
        catalogue.install_dir(&root.0).is_empty(),
        "changed/new blends wait for restart"
    );
    assert_eq!(catalogue.blend_pass("plug.photocopy"), Some(frozen.clone()));
    assert!(catalogue.blend_pass("example.new").is_none());
    assert_eq!(
        catalogue
            .get("example.ordinary")
            .unwrap()
            .manifest
            .effect
            .name,
        "After",
        "ordinary effects still reload"
    );
    std::fs::remove_dir_all(first).unwrap();
    assert!(catalogue.install_dir(&root.0).is_empty());
    assert_eq!(catalogue.blend_pass("plug.photocopy"), Some(frozen));
    let mut restarted = Catalogue::new();
    assert!(restarted.install_dir(&root.0).is_empty());
    assert!(restarted.blend_pass("plug.photocopy").is_none());
    assert!(restarted.blend_pass("example.new").is_some());
}

#[test]
fn manifests_refuse_other_compositing_contracts_and_path_escapes() {
    for change in [
        MANIFEST.replace(
            "entry = \"blend\"",
            "entry = \"blend\"\nreads_ground = false",
        ),
        MANIFEST.replace("entry = \"blend\"", "entry = \"blend\"\nnon_linear = false"),
        MANIFEST.replace("photocopy.wgsl", "../photocopy.wgsl"),
        MANIFEST.replace("kind = \"blend\"", "kind = \"effect\""),
    ] {
        assert!(Manifest::parse(&change).is_err(), "{change}");
    }
}
