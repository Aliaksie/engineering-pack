//! One hook's context under the host's output ceiling. A host replaces or cuts a hook answer
//! over its cap, so the model gets a preview or nothing; a context that would pass the ceiling
//! loses whole pieces instead, least important first, and says which.

/// One part of a hook's context. `rank` orders what stays: lower is kept longer.
pub struct Piece {
    /// What the piece is, in the words the model is told when it is left out.
    pub name: &'static str,
    pub rank: u8,
    pub text: String,
}

#[derive(Debug, PartialEq)]
pub struct Fitted {
    pub text: String,
    /// Names of the pieces left out, in the order they were given.
    pub dropped: Vec<&'static str>,
    /// The one remaining piece was itself larger than the ceiling and was cut.
    pub truncated: bool,
}

const CUT: &str = "\n[truncated; use the runtime checkpoint/route CLI for full data]";

fn joined(pieces: &[&Piece], dropped: &[&'static str]) -> String {
    let mut text = pieces
        .iter()
        .map(|piece| piece.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if !dropped.is_empty() {
        text.push_str(&format!(
            "\n[Craft left out, to stay within the host's hook output limit: {}]",
            dropped.join(", ")
        ));
    }
    text
}

/// `pieces` joined by line, within `ceiling` UTF-8 bytes. Empty pieces are skipped. Bytes are
/// at least as many as characters, so a byte ceiling also holds a host cap counted in
/// characters.
pub fn fit(pieces: Vec<Piece>, ceiling: usize) -> Fitted {
    let mut kept: Vec<&Piece> = pieces.iter().filter(|p| !p.text.is_empty()).collect();
    let mut dropped: Vec<&'static str> = Vec::new();
    loop {
        let text = joined(&kept, &dropped);
        if text.len() <= ceiling {
            return Fitted {
                text,
                dropped,
                truncated: false,
            };
        }
        if kept.len() <= 1 {
            break;
        }
        // The least important piece goes; among equals, the later one.
        let (at, _) = kept
            .iter()
            .enumerate()
            .max_by_key(|(index, piece)| (piece.rank, *index))
            .expect("more than one piece");
        let gone = kept.remove(at).name;
        // Names stay in the order the pieces were given.
        dropped = pieces
            .iter()
            .map(|piece| piece.name)
            .filter(|name| *name == gone || dropped.contains(name))
            .collect();
    }
    let text = joined(&kept, &dropped);
    let room = ceiling.saturating_sub(CUT.len());
    let end = (0..=room.min(text.len()))
        .rev()
        .find(|at| text.is_char_boundary(*at))
        .unwrap_or(0);
    Fitted {
        text: format!("{}{CUT}", &text[..end]),
        dropped,
        truncated: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(name: &'static str, rank: u8, bytes: usize) -> Piece {
        Piece {
            name,
            rank,
            text: name[..1].repeat(bytes),
        }
    }

    #[test]
    fn a_context_within_the_ceiling_is_joined_unchanged() {
        let fitted = fit(
            vec![piece("a", 0, 3), piece("b", 5, 0), piece("c", 9, 2)],
            6,
        );
        assert_eq!(fitted.text, "aaa\ncc");
        assert!(fitted.dropped.is_empty() && !fitted.truncated);
    }

    #[test]
    fn over_the_ceiling_whole_pieces_go_least_important_first() {
        let pieces = || {
            vec![
                piece("session", 0, 100),
                piece("planning", 7, 400),
                piece("resume", 1, 300),
                piece("plans", 5, 400),
            ]
        };
        let fitted = fit(pieces(), 900);
        assert_eq!(fitted.dropped, ["planning"]);
        assert!(fitted.text.starts_with(&format!(
            "{}\n{}\n{}\n[Craft left out",
            "s".repeat(100),
            "r".repeat(300),
            "p".repeat(400)
        )));
        assert!(fitted.text.ends_with("limit: planning]") && fitted.text.len() <= 900);
        let fitted = fit(pieces(), 600);
        assert_eq!(fitted.dropped, ["planning", "plans"]);
        assert!(fitted.text.len() <= 600 && !fitted.truncated);
        // The same pieces always give the same bytes.
        assert_eq!(fit(pieces(), 600), fitted);
    }

    #[test]
    fn one_piece_larger_than_the_ceiling_is_cut_at_a_character() {
        // The cut falls inside the four-byte character.
        let room = 200 - CUT.len();
        let text = format!("{}\u{1f642}tail", "x".repeat(room - 2));
        let fitted = fit(
            vec![
                Piece {
                    name: "resume",
                    rank: 1,
                    text,
                },
                piece("plans", 5, 80),
            ],
            200,
        );
        assert!(fitted.truncated && fitted.text.len() <= 200);
        assert_eq!(fitted.dropped, ["plans"]);
        assert_eq!(fitted.text, format!("{}{CUT}", "x".repeat(room - 2)));
    }
}
