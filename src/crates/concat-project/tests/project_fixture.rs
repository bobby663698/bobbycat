// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! The smallest project worth loading: one video track, one text track, one
//! audio track, one clip each, and the two media entries the clips point at.
//!
//! A fixture is only useful while it is stable, so this test does two things
//! beyond reading it. It asserts the version it was written to, so a
//! migration cannot quietly rewrite it into something else. And it writes it
//! back out and reads it again, because a document that does not round-trip
//! is one that will drift the first time somebody saves it.
//!
//! `fixtures/minimal-project.json` is read by `include_str!`, so it travels
//! in the test binary: no test needs to find a file at run time.
//!
//! What it is for: subtitle import, bookmarks and scene migration all need a
//! project with a picture lane, a title lane and a sound lane, and none of
//! them should have to build one by hand. Keep it minimal - every field
//! added here is a field every future migration has to keep working.

use concat_project::model::ClipKind;
use concat_project::{
    DOCUMENT_VERSION, DocumentSettings, document_version, from_document, to_document,
};

const MINIMAL: &str = include_str!("fixtures/minimal-project.json");

#[test]
fn the_minimal_project_reads_with_one_track_of_each_kind() {
    let document: serde_json::Value = serde_json::from_str(MINIMAL).expect("the fixture is JSON");

    // Written at the shape this build writes, so no migration has anything
    // to do to it. If this fails, the fixture needs bringing forward - not
    // the assertion relaxing.
    assert_eq!(
        document_version(&document),
        DOCUMENT_VERSION,
        "the fixture is from another version of the document"
    );

    let project = from_document(&document).expect("the fixture opens");
    assert_eq!(project.timelines.len(), 1, "one timeline");
    assert_eq!(project.media.len(), 2, "a picture and a voice");

    let timeline = &project.timelines[0];
    assert_eq!(timeline.tracks.len(), 3, "three lanes");
    assert_eq!(
        timeline.clips.len(),
        3,
        "a clip was held back rather than read"
    );

    for kind in [ClipKind::Video, ClipKind::Text, ClipKind::Audio] {
        let count = timeline
            .clips
            .iter()
            .filter(|clip| clip.kind == kind)
            .count();
        assert_eq!(count, 1, "{kind:?} has {count} clips, not 1");
    }
}

#[test]
fn the_minimal_project_survives_a_round_trip() {
    let document: serde_json::Value = serde_json::from_str(MINIMAL).expect("the fixture is JSON");
    let project = from_document(&document).expect("the fixture opens");
    let settings = DocumentSettings {
        name: "Minimal".to_owned(),
        width: 1920,
        height: 1080,
        rate_num: 30,
        rate_den: 1,
        canvas: [0, 0, 0, 255],
    };

    let written = to_document(&settings, &project);
    let again = from_document(&written).expect("what it writes, it reads");

    assert_eq!(again.timelines.len(), 1);
    assert_eq!(again.timelines[0].tracks.len(), 3);
    assert_eq!(again.timelines[0].clips.len(), 3);
    for kind in [ClipKind::Video, ClipKind::Text, ClipKind::Audio] {
        let count = again.timelines[0]
            .clips
            .iter()
            .filter(|clip| clip.kind == kind)
            .count();
        assert_eq!(count, 1, "{kind:?} lost on the way through: {count}");
    }
}
