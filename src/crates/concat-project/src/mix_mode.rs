// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! Jianying `MixMode` resources for the draft exporter (the project →
//! YingDraft `ProjectManifest` → `jy generate` direction).
//!
//! A `VideoClip`'s `mix_mode` has four string fields: a per-instance `id`
//! (the exporter mints that), and three fields that name a *resource*:
//! `name`, `effect_id`, `resource_id`. The `name` is Jianying's `nameId`,
//! which is **not** our kebab-case [`Blend::name`] with `-` turned into `_`.
//! Only `soft-light → soft_light` and `color-dodge → color_dodge` happen to
//! follow that rule; the other eight are internal names such as
//! `over_lay`, `glare_pc` and `color_filter`. A string rewrite silently
//! points eight modes at resources that do not exist.
//!
//! So this module is a lookup table, built from the real
//! `MixMode.json`/`resourceList` values (Jianying 11.6.0.14552), recorded in
//! `bridge/departments/data/EXPORT_FIELDS.md` §0.6.2. `normal` (and the
//! empty string) writes no `mix_mode`; `add` has no Jianying resource and
//! degrades to no `mix_mode` with a note from the caller.
//!
//! The table is indexed by position in [`Blend::ALL`], so adding a blend
//! mode without extending it is a compile error rather than a silent
//! miss, and no second copy of the mode list is kept here.

use concat_core::timeline::Blend;

/// The three resource-bearing fields of a Jianying `mix_mode`.
///
/// The fourth field, `id`, is an instance id minted by the exporter and is
/// deliberately not part of this table: it does not take part in resource
/// lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MixModeResource {
    /// Written to `mix_mode.name`: Jianying's `nameId`, an internal name
    /// for eight of the ten modes.
    pub name_id: &'static str,
    /// Written to `mix_mode.effect_id`.
    pub effect_id: &'static str,
    /// Written to `mix_mode.resource_id`.
    pub resource_id: &'static str,
}

/// The ten real resources, at the same indices as [`Blend::ALL`].
///
/// `None` at `Normal` (write no `mix_mode`) and at `Add` (no Jianying
/// resource; degrade to `normal` and report). `static`, not `const`, so
/// [`for_blend`] can hand out `&'static` references without copying.
pub static MIX_MODE_TABLE: [Option<MixModeResource>; Blend::ALL.len()] = [
    None, // normal / "" — no mix_mode
    Some(MixModeResource {
        name_id: "multiply_blend_mode",
        effect_id: "871333",
        resource_id: "6758325895519277582",
    }),
    Some(MixModeResource {
        name_id: "color_filter",
        effect_id: "871339",
        resource_id: "6758325170760323597",
    }),
    None, // add — no Jianying resource, degrade to normal
    Some(MixModeResource {
        name_id: "bright_en",
        effect_id: "871341",
        resource_id: "6758324919789949453",
    }),
    Some(MixModeResource {
        name_id: "dark_en",
        effect_id: "871342",
        resource_id: "6758324839670354445",
    }),
    Some(MixModeResource {
        name_id: "over_lay",
        effect_id: "871340",
        resource_id: "6758324989931295240",
    }),
    Some(MixModeResource {
        name_id: "soft_light",
        effect_id: "871337",
        resource_id: "6758325439212556814",
    }),
    Some(MixModeResource {
        name_id: "glare_pc",
        effect_id: "871338",
        resource_id: "6758325264670790152",
    }),
    Some(MixModeResource {
        name_id: "darken_color",
        effect_id: "871335",
        resource_id: "6758325724848853518",
    }),
    Some(MixModeResource {
        name_id: "color_dodge",
        effect_id: "871334",
        resource_id: "6758325800031752712",
    }),
    Some(MixModeResource {
        name_id: "linear_deepening",
        effect_id: "871336",
        resource_id: "6758325619253056013",
    }),
];

/// The resource a blend mode needs in `mix_mode`, if any.
///
/// `Normal` and `Add` return `None`: the exporter writes no `mix_mode`
/// for them.
pub fn for_blend(blend: Blend) -> Option<&'static MixModeResource> {
    let index = Blend::ALL.iter().position(|mode| *mode == blend)?;
    MIX_MODE_TABLE[index].as_ref()
}

/// Look up by the `blend` string stored in a document.
///
/// Uses [`Blend::parse`], so both the kebab-case name we write
/// (`"soft-light"`) and an underscored spelling from the other side
/// (`"soft_light"`) resolve. `""` and `"normal"` are Normal and need no
/// `mix_mode`; an unknown name also falls back to Normal rather than
/// inventing a resource.
pub fn for_name(name: &str) -> Option<&'static MixModeResource> {
    for_blend(Blend::parse(name))
}

#[cfg(test)]
mod tests {
    use super::{MIX_MODE_TABLE, for_blend, for_name};
    use concat_core::timeline::Blend;

    /// Pin all three resource fields of one mode against the spec table.
    fn check(blend: Blend, name_id: &str, effect_id: &str, resource_id: &str) {
        let got = for_blend(blend).expect("a resource");
        assert_eq!(got.name_id, name_id, "{}", blend.name());
        assert_eq!(got.effect_id, effect_id, "{}", blend.name());
        assert_eq!(got.resource_id, resource_id, "{}", blend.name());
        assert_eq!(Some(got), MIX_MODE_TABLE[blend as usize].as_ref());
    }

    /// The two modes where `-` → `_` really is the Jianying name. These
    /// are the ones the old, wrong string-rewrite rule happened to get
    /// right; pin every field so a future edit cannot drift.
    #[test]
    fn hyphen_modes_are_underscored_names() {
        check(
            Blend::SoftLight,
            "soft_light",
            "871337",
            "6758325439212556814",
        );
        check(
            Blend::ColorDodge,
            "color_dodge",
            "871334",
            "6758325800031752712",
        );
        assert_eq!(for_name("soft-light"), for_blend(Blend::SoftLight));
        assert_eq!(for_name("color-dodge"), for_blend(Blend::ColorDodge));
    }

    /// Eight names are Jianying-internal; a `-` → `_` rewrite would
    /// produce a resource that does not exist. Pin all eight verbatim,
    /// and guard against anyone "simplifying" the table back to a string
    /// rewrite.
    #[test]
    fn internal_names_are_looked_up_not_derived() {
        check(
            Blend::Multiply,
            "multiply_blend_mode",
            "871333",
            "6758325895519277582",
        );
        check(
            Blend::Screen,
            "color_filter",
            "871339",
            "6758325170760323597",
        );
        check(Blend::Lighten, "bright_en", "871341", "6758324919789949453");
        check(Blend::Darken, "dark_en", "871342", "6758324839670354445");
        check(Blend::Overlay, "over_lay", "871340", "6758324989931295240");
        check(
            Blend::HardLight,
            "glare_pc",
            "871338",
            "6758325264670790152",
        );
        check(
            Blend::ColorBurn,
            "darken_color",
            "871335",
            "6758325724848853518",
        );
        check(
            Blend::LinearBurn,
            "linear_deepening",
            "871336",
            "6758325619253056013",
        );
        assert_ne!(for_blend(Blend::Overlay).unwrap().name_id, "overlay");
        assert_ne!(for_blend(Blend::HardLight).unwrap().name_id, "hard_light");
        assert_ne!(for_blend(Blend::Screen).unwrap().name_id, "screen");
        assert_ne!(for_blend(Blend::Multiply).unwrap().name_id, "multiply");
    }

    /// `normal`, `""` and `add` write no `mix_mode`, and neither does an
    /// unknown name. `add` is the one that should also raise a note in
    /// the export report; here it is enough that it never invents a
    /// resource.
    #[test]
    fn modes_without_a_resource_write_nothing() {
        assert_eq!(for_blend(Blend::Normal), None);
        assert_eq!(for_name(""), None);
        assert_eq!(for_name("normal"), None);
        assert_eq!(for_name("add"), None);
        assert_eq!(for_blend(Blend::Add), None);
        assert_eq!(for_name("something-new"), None);
    }

    /// The table stays the same length as the engine's mode list, and
    /// every non-resource slot is one of the two intentional `None`s.
    /// Adding a thirteenth mode without updating the table will not
    /// compile; this test also guards against an accidental `None` in a
    /// slot that should have a resource.
    #[test]
    fn table_covers_every_blend_mode() {
        assert_eq!(MIX_MODE_TABLE.len(), Blend::ALL.len());
        let mut resources = 0;
        for (index, blend) in Blend::ALL.iter().enumerate() {
            match for_blend(*blend) {
                Some(resource) => {
                    resources += 1;
                    assert_eq!(Some(resource), MIX_MODE_TABLE[index].as_ref());
                }
                None => assert!(matches!(*blend, Blend::Normal | Blend::Add)),
            }
        }
        assert_eq!(resources, 10);
    }

    /// The underscored spelling from Jianying's own `MixMode.json`
    /// resolves for the two modes where it is the real `nameId`, too.
    #[test]
    fn underscored_input_resolves_to_the_same_resource() {
        assert_eq!(for_name("soft_light"), for_blend(Blend::SoftLight));
        assert_eq!(for_name("color_dodge"), for_blend(Blend::ColorDodge));
    }
}
