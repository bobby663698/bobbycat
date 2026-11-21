// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! One clip per blend mode, in menu order, across two timelines: the fixture
//! that pins the twelve storage values the document holds.
//!
//! `minimal-project.json` is the smallest thing that opens; this one is the
//! widest thing that still has to. Twelve clips carry the twelve modes, so a
//! migration that renames, drops or reorders one fails here rather than in
//! a saved project six months old. The second timeline exists for the same
//! reason: a document holds more than one, each with its own frame, and
//! nothing else in the tree reads a two-timeline file.
//!
//! The expected values come from `Blend::ALL` and `Blend::name`, not from a
//! second copy of the list - a copy would be one more thing to forget when
//! a thirteenth mode arrives. `normal` is the exception worth knowing: it
//! writes no `blend` field at all, which is why the first clip has none.

use concat_core::timeline::Blend;
use concat_project::{
    DOCUMENT_VERSION, DocumentSettings, document_version, from_document, to_document,
};

const ALL_BLENDS: &str = include_str!("fixtures/all-blends-project.json");

/// The project-level settings a save needs. The fixture's first timeline is
/// 1920x1080 at 30, and this is what a save would write for that frame.
fn settings() -> DocumentSettings {
    DocumentSettings {
        name: "Every blend".to_owned(),
        width: 1920,
        height: 1080,
        rate_num: 30,
        rate_den: 1,
        canvas: [0, 0, 0, 255],
    }
}

#[test]
fn the_fixture_reads_as_two_timelines() {
    let document: serde_json::Value =
        serde_json::from_str(ALL_BLENDS).expect("the fixture is JSON");

    assert_eq!(
        document_version(&document),
        DOCUMENT_VERSION,
        "the fixture is from another version of the document"
    );

    let project = from_document(&document).expect("the fixture opens");
    assert_eq!(project.timelines.len(), 2, "two timelines");
    assert_eq!(project.media.len(), 1, "one picture");
    assert_eq!(project.active_timeline_id, "tl-blends");
}

#[test]
fn every_blend_mode_is_stored_exactly_as_the_engine_names_it() {
    let document: serde_json::Value =
        serde_json::from_str(ALL_BLENDS).expect("the fixture is JSON");
    let project = from_document(&document).expect("the fixture opens");

    let timeline = &project.timelines[0];
    assert_eq!(
        timeline.clips.len(),
        Blend::ALL.len(),
        "one clip per mode, no more and no fewer"
    );

    let stored: Vec<&str> = timeline
        .clips
        .iter()
        .map(|clip| clip.blend.as_str())
        .collect();
    // `Blend::name` says "normal"; a document says nothing at all, because
    // the field's absence is what normal means on the wire. The two are
    // deliberately different and an exporter has to keep them apart - this
    // is the one place that difference is written down as an assertion.
    let expected: Vec<&str> = Blend::ALL
        .iter()
        .map(|blend| match blend {
            Blend::Normal => "",
            named => named.name(),
        })
        .collect();
    assert_eq!(stored, expected, "the twelve storage values, in menu order");

    // `normal` is the one mode that writes nothing: an empty string in the
    // struct, no `blend` key in the document.
    assert_eq!(stored[0], "", "normal is written as no field at all");
    let raw = &document["timelines"][0]["clips"][0];
    assert!(
        raw.get("blend").is_none(),
        "normal must not write a blend field: {raw}"
    );
}

#[test]
fn the_blend_fixture_round_trips_through_a_save() {
    let document: serde_json::Value =
        serde_json::from_str(ALL_BLENDS).expect("the fixture is JSON");
    let project = from_document(&document).expect("the fixture opens");

    let written = to_document(&settings(), &project);
    let again = from_document(&written).expect("what it wrote reopens");

    assert_eq!(
        again.timelines.len(),
        project.timelines.len(),
        "a save kept every timeline"
    );
    assert_eq!(again.active_timeline_id, project.active_timeline_id);

    for (before, after) in project.timelines.iter().zip(again.timelines.iter()) {
        assert_eq!(before.id, after.id);
        assert_eq!(before.clips.len(), after.clips.len(), "{}", before.name);
        for (a, b) in before.clips.iter().zip(after.clips.iter()) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.blend, b.blend, "clip {} changed mode", a.id);
            assert_eq!(a.start, b.start, "clip {} moved", a.id);
            assert_eq!(a.duration, b.duration, "clip {} changed length", a.id);
        }
    }
}

#[test]
fn the_second_timeline_keeps_its_own_frame() {
    let document: serde_json::Value =
        serde_json::from_str(ALL_BLENDS).expect("the fixture is JSON");
    let project = from_document(&document).expect("the fixture opens");

    let second = &project.timelines[1];
    assert_eq!(second.id, "tl-second");
    assert_eq!(second.clips.len(), 1);
    assert_eq!(second.video.width, 1280, "its own width");
    assert_eq!(second.video.height, 720, "its own height");
    assert_eq!(second.video.rate_num, 24, "its own rate");
    assert_eq!(second.clips[0].blend, "overlay");
}
