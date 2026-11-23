// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! The fixture that came with the MCPP handoff, run against the expansion
//! layer rather than argued over. Command counts and the three numeric traps
//! are the contract's acceptance criteria.

use concat_autoedit::{
    Brief, Candidate, ExpandError, ParseError, Probe, db_to_linear, expand, guard_pending_asr,
    parse, prompt,
};
use concat_project::Command;
use concat_project::commands::TrimEdge;

const PLAN: &str = include_str!("fixtures/plan_v1_sample.json");

fn probes() -> Vec<Probe> {
    vec![
        Probe {
            asset_id: "m1".to_owned(),
            path: "asset://m1.mp4".to_owned(),
            duration: 45.0,
            width: Some(1920),
            height: Some(1080),
            has_audio: true,
        },
        Probe {
            asset_id: "m2".to_owned(),
            path: "asset://m2.mp4".to_owned(),
            duration: 12.0,
            width: Some(1080),
            height: Some(1920),
            has_audio: false,
        },
        Probe {
            asset_id: "m3".to_owned(),
            path: "asset://m3.mp3".to_owned(),
            duration: 30.0,
            width: None,
            height: None,
            has_audio: true,
        },
    ]
}

#[test]
fn the_fixture_expands_to_sixteen_and_eight_commands() {
    let result = expand(PLAN, &probes(), (1920, 1080)).expect("the fixture expands");
    assert_eq!(result.candidates.len(), 2);
    assert_eq!(result.candidates[0].commands.len(), 16);
    assert_eq!(result.candidates[1].commands.len(), 8);
    assert_eq!(result.candidates[0].id, "hook-open");
    assert_eq!(result.candidates[1].id, "body-proof");
    // Both features without a command are reported, not dropped.
    assert_eq!(result.unsupported.len(), 2);
    assert!(result.unsupported[0].starts_with("crop("));
    assert!(result.unsupported[1].starts_with("duck_under_speech"));
}

#[test]
fn the_numbers_match_the_fixture() {
    let result = expand(PLAN, &probes(), (1920, 1080)).expect("the fixture expands");
    let commands = &result.candidates[0].commands;

    let tail_deltas: Vec<f64> = commands
        .iter()
        .filter_map(|command| match command {
            Command::TrimClip {
                edge: TrimEdge::End,
                delta,
                ..
            } => Some(*delta),
            _ => None,
        })
        .collect();
    assert_eq!(tail_deltas, vec![26.5, 5.0, 19.5]);

    let muted: Vec<Option<bool>> = commands
        .iter()
        .filter_map(|command| match command {
            Command::UpdateClip { patch, .. } => Some(patch.muted),
            _ => None,
        })
        .collect();
    assert_eq!(
        muted.into_iter().flatten().collect::<Vec<_>>(),
        vec![false, true]
    );

    let volume = commands.iter().find_map(|command| match command {
        Command::UpdateClip { patch, .. } => patch.volume,
        _ => None,
    });
    assert_eq!(volume, Some(0.1259));
    assert_eq!(db_to_linear(-18.0), 0.1259);

    let offset_y = commands.iter().find_map(|command| match command {
        Command::AddTextClip { offset_y, .. } => *offset_y,
        _ => None,
    });
    assert_eq!(offset_y, Some(-0.35));

    let scale = commands.iter().find_map(|command| match command {
        Command::SetClipTransform { scale, .. } => *scale,
        _ => None,
    });
    assert!(scale.is_some_and(|scale| (scale - 1.7778).abs() < 0.001));

    // The second candidate reuses both media ids but mints new clip ids.
    let second = &result.candidates[1].commands;
    let adds = second
        .iter()
        .filter(|command| matches!(command, Command::AddMedia { .. }))
        .count();
    assert_eq!(adds, 0, "AddMedia is plan-wide, not per candidate");
    assert_eq!(result.candidates[0].audio_assets, vec!["m1", "m3"]);
}

#[test]
fn a_candidate_is_one_batch_and_undoes_in_one_step() {
    let result = expand(PLAN, &probes(), (1920, 1080)).expect("the fixture expands");
    match result.candidates[0].as_batch() {
        Command::Batch { commands } => assert_eq!(commands.len(), 16),
        other => panic!("a candidate must be a single Batch, got {other:?}"),
    }
}

#[test]
fn v3_fields_are_a_hard_error() {
    let v3 = PLAN.replace("\"position\": \"top\"", "\"position\": \"top\", \"x\": 0.5");
    assert!(matches!(
        expand(&v3, &probes(), (1920, 1080)),
        Err(ExpandError::Json(_))
    ));
    let other_version = PLAN.replace("ip_plan_v1", "ip_plan_v3");
    assert!(matches!(
        expand(&other_version, &probes(), (1920, 1080)),
        Err(ExpandError::Version(_))
    ));
}

#[test]
fn blocked_is_a_normal_exit_with_the_gap_text() {
    let reply = format!("BLOCKED: {}", "x".repeat(1300));
    match parse(&reply) {
        Err(ParseError::Blocked(gap)) => assert_eq!(gap.chars().count(), 1200),
        other => panic!("BLOCKED must carry the gap, got {other:?}"),
    }
    let candidates = parse("```json\n[{\"id\":\"a\",\"title\":\"A\",\"commands\":[{\"op\":\"trimClip\",\"clipId\":\"c1\",\"edge\":\"start\",\"delta\":0.5}]}]\n```")
        .expect("a fenced array still parses");
    assert_eq!(candidates.len(), 1);
}

#[test]
fn the_prompt_carries_all_twelve_contract_rules() {
    let text = prompt(&Brief::default(), "{}");
    for number in 1..=12 {
        assert!(
            text.contains(&format!("\n{number}. ")),
            "rule {number} is missing"
        );
    }
}

#[test]
fn the_asr_gate_blocks_sound_but_not_a_silent_picture() {
    let candidate = Candidate::new("uses-pending-audio", "Uses it", Vec::new());
    let silent = Candidate {
        audio_assets: Vec::new(),
        ..candidate.clone()
    };
    let sounding = Candidate {
        audio_assets: vec!["m1".to_owned()],
        ..candidate
    };
    let pending = vec!["m1".to_owned()];
    assert!(guard_pending_asr(&silent, &pending).is_none());
    let rejection = guard_pending_asr(&sounding, &pending).expect("the sounding use is blocked");
    assert!(rejection.why.contains("asr_timing_review"));
    assert_eq!(rejection.command, None);
}
