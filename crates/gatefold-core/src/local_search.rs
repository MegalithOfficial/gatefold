use std::fmt::Write;

use nucleo_matcher::{
    Config, Matcher,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};

use crate::model::{AlbumRef, TrackInfo};

pub trait SearchDocument {
    fn search_document(&self) -> String;
}

pub fn ranked_indices<T: SearchDocument>(items: &[T], query: &str) -> Vec<usize> {
    let query = query.trim();
    if query.is_empty() {
        return (0..items.len()).collect();
    }

    let pattern = Pattern::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Substring,
    );
    let candidates = items
        .iter()
        .enumerate()
        .map(|(index, item)| Candidate {
            index,
            text: item.search_document(),
        })
        .collect::<Vec<_>>();
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut matches = pattern.match_list(candidates, &mut matcher);
    matches.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| left.index.cmp(&right.index))
    });

    matches
        .into_iter()
        .map(|(candidate, _)| candidate.index)
        .collect()
}

struct Candidate {
    index: usize,
    text: String,
}

impl AsRef<str> for Candidate {
    fn as_ref(&self) -> &str {
        &self.text
    }
}

impl SearchDocument for AlbumRef {
    fn search_document(&self) -> String {
        let mut document = self.name.clone();
        for artist in &self.artists {
            document.push(' ');
            document.push_str(&artist.name);
        }
        let _ = write!(document, " {}", self.year);
        document
    }
}

impl SearchDocument for TrackInfo {
    fn search_document(&self) -> String {
        let mut document = self.name.clone();
        for artist in &self.artists {
            document.push(' ');
            document.push_str(&artist.name);
        }
        document
    }
}

#[cfg(test)]
mod tests {
    use super::{SearchDocument, ranked_indices};

    struct Item(&'static str);

    impl SearchDocument for Item {
        fn search_document(&self) -> String {
            self.0.to_owned()
        }
    }

    #[test]
    fn words_match_in_any_order_and_ignore_accents() {
        let items = [
            Item("Happier Than Ever Billie Eilish 2021"),
            Item("Renaissance Beyoncé 2022"),
        ];
        assert_eq!(ranked_indices(&items, "eilish happier"), vec![0]);
        assert_eq!(ranked_indices(&items, "beyonce"), vec![1]);
    }

    #[test]
    fn scattered_letters_do_not_match() {
        let items = [Item("Blood on the Dance Floor")];
        assert!(ranked_indices(&items, "bad").is_empty());
    }

    #[test]
    fn ranks_stronger_matches_first() {
        let items = [
            Item("Bury a Friend Billie Eilish"),
            Item("Bad Guy Billie Eilish"),
            Item("Blue Billie Eilish"),
        ];
        let matches = ranked_indices(&items, "bad guy");
        assert_eq!(matches.first(), Some(&1));
    }

    #[test]
    fn empty_query_preserves_source_order() {
        let items = [Item("Third"), Item("First"), Item("Second")];
        assert_eq!(ranked_indices(&items, "  "), vec![0, 1, 2]);
    }
}
