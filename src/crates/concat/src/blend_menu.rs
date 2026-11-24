// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah

//! The inspector's blend choices, with the document id beside every label.

use concat_core::timeline::{Blend, card_by_id};

pub(crate) struct BlendMenu {
    pub labels: Vec<String>,
    pub selected: usize,
    ids: Vec<String>,
}

impl BlendMenu {
    pub fn new<'a>(
        current: &str,
        external: impl IntoIterator<Item = (&'a str, &'a str)>,
        labels: [String; Blend::ALL.len()],
    ) -> Self {
        let mut labels = Vec::from(labels);
        let mut ids: Vec<String> = Blend::ALL
            .into_iter()
            .map(|mode| {
                if mode == Blend::Normal {
                    String::new()
                } else {
                    mode.name().to_owned()
                }
            })
            .collect();
        for (id, label) in external {
            ids.push(id.to_owned());
            labels.push(label.to_owned());
        }
        let canonical = card_by_id(current).map_or(current, |card| {
            if card.mode == Blend::Normal {
                ""
            } else {
                card.mode.name()
            }
        });
        let selected = ids
            .iter()
            .position(|id| id == canonical)
            .unwrap_or_else(|| {
                // A missing package stays selected by its original id. Merely
                // opening the inspector must never rewrite it to Normal.
                ids.push(current.to_owned());
                labels.push(format!("⚠ {current}"));
                ids.len() - 1
            });
        Self {
            labels,
            selected,
            ids,
        }
    }

    pub fn id(&self, index: usize) -> Option<&str> {
        self.ids.get(index).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels() -> [String; Blend::ALL.len()] {
        Blend::ALL.map(|mode| mode.name().to_owned())
    }

    #[test]
    fn an_external_card_is_the_thirteenth_choice_and_reopens_by_id() {
        let menu = BlendMenu::new("", [("plug.photocopy", "Photocopy")], labels());
        assert_eq!(menu.labels.len(), 13);
        assert_eq!(menu.id(12), Some("plug.photocopy"));
        let reopened = BlendMenu::new(
            menu.id(12).unwrap(),
            [("plug.photocopy", "Photocopy")],
            labels(),
        );
        assert_eq!(reopened.selected, 12);
        assert_eq!(reopened.labels[12], "Photocopy");
    }

    #[test]
    fn a_missing_card_keeps_its_document_id_in_a_warning_choice() {
        let menu = BlendMenu::new("plug.photocopy", [], labels());
        assert_eq!(menu.selected, 12);
        assert_eq!(menu.id(menu.selected), Some("plug.photocopy"));
        assert_eq!(menu.labels[menu.selected], "⚠ plug.photocopy");
        assert_eq!(menu.id(0), Some(""));
        assert_eq!(menu.id(13), None);
    }

    #[test]
    fn built_in_names_and_aliases_keep_their_existing_slots() {
        for mode in Blend::ALL {
            let menu = BlendMenu::new(mode.name(), [], labels());
            assert_eq!(menu.labels.len(), Blend::ALL.len());
            assert_eq!(menu.selected, mode.index());
            assert_eq!(menu.labels.len(), menu.ids.len());
            let alias = mode.name().replace('-', "_");
            assert_eq!(BlendMenu::new(&alias, [], labels()).selected, mode.index());
        }
    }
}
