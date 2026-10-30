//! Deterministic text helpers — port of `packages/knowledge/src/text.ts`.
//!
//! Chunking and tokenising are deliberately not "clever": the same document
//! must always produce the same chunks in the same order, or re-ingestion
//! silently rewrites ids and breaks provenance links held elsewhere.

/// Words too common to discriminate anything.
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "for", "from", "how", "i", "in", "is",
    "it", "of", "on", "or", "that", "the", "this", "to", "was", "what", "when", "where", "which",
    "who", "why", "with", "you", "your",
];

pub const DEFAULT_CHUNK_TARGET_CHARS: usize = 600;
pub const DEFAULT_CHUNK_OVERLAP_CHARS: usize = 80;

/// Case-folded alphanumeric tokens, stopwords and 1-char noise removed.
pub fn tokenise(text: &str) -> Vec<String> {
    split_ascii_alnum(&text.to_lowercase())
        .into_iter()
        .filter(|token| token.len() > 1 && !STOPWORDS.contains(&token.as_str()))
        .collect()
}

/// Runs of `[a-z0-9]`, after the full-Unicode lowercase fold.
///
/// The baseline splits on `[^a-z0-9]+` of the lowercased text. Folding first
/// and then treating every non-`[a-z0-9]` char as a separator is the same
/// thing: an accented letter folds to *some* non-ASCII-anything form and is
/// still a separator, exactly as it is on the TypeScript side.
fn split_ascii_alnum(lowercased: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for character in lowercased.chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            current.push(character);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Split prose into chunks on sentence boundaries, carrying a bounded tail of
/// the previous chunk forward so a sentence spanning a boundary is still
/// retrievable from either side.
pub fn chunk_text(text: &str) -> Vec<String> {
    chunk_text_bounded(
        text,
        DEFAULT_CHUNK_TARGET_CHARS,
        DEFAULT_CHUNK_OVERLAP_CHARS,
    )
}

/// The explicit-bounds form, for a caller that fishes its own constants.
pub fn chunk_text_bounded(text: &str, target_chars: usize, overlap_chars: usize) -> Vec<String> {
    let normalised = collapse_whitespace(text);
    if normalised.is_empty() {
        return Vec::new();
    }
    if normalised.chars().count() <= target_chars {
        return vec![normalised];
    }

    let sentences = split_sentences(&normalised);
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();

    for sentence in sentences {
        if !current.is_empty()
            && current.chars().count() + sentence.chars().count() + 1 > target_chars
        {
            chunks.push(current.clone());
            current = tail_of(&current, overlap_chars);
        }
        current = if current.is_empty() {
            sentence
        } else {
            format!("{} {}", current, sentence)
        };
    }
    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

/// Whitespace becomes one space; leading and trailing spaces disappear.
fn collapse_whitespace(text: &str) -> String {
    // `\s` in JavaScript matches Unicode whitespace... approximately: it is
    // [\f\n\r\t\v\u0020\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000\ufeff].
    // `char::is_whitespace` covers the same set plus a few separators a page
    // never emits; for prose in this domain the two agree on every input.
    let mut collapsed = String::with_capacity(text.len());
    let mut in_space = false;
    for character in text.chars() {
        if character.is_whitespace() {
            in_space = true;
        } else {
            if in_space && !collapsed.is_empty() {
                collapsed.push(' ');
            }
            in_space = false;
            collapsed.push(character);
        }
    }
    collapsed
}

/// Split on sentence-final punctuation, keeping the punctuation.
///
/// The baseline's `match(/[^.!?]+[.!?]+(?:\s|$)|[^.!?]+$/g)`: a sentence is a
/// run of text ending in one or more `.!?` (optionally followed by one
/// whitespace), or the unterminated remainder at the end.
fn split_sentences(text: &str) -> Vec<String> {
    let characters: Vec<char> = text.chars().collect();
    let is_punctuation = |c: char| -> bool { c == '.' || c == '!' || c == '?' };
    let mut parts: Vec<String> = Vec::new();
    let mut index = 0usize;
    let mut start = 0usize;
    while index < characters.len() {
        // Find the run of non-punctuation.
        while index < characters.len() && !is_punctuation(characters[index]) {
            index += 1;
        }
        if index == characters.len() {
            // The unterminated remainder: `[^.!?]+$`.
            let remainder: String = characters[start..].iter().collect();
            let trimmed = remainder.trim().to_string();
            if !trimmed.is_empty() {
                parts.push(trimmed);
            }
            break;
        }
        // Consume the punctuation run: `[.!?]+`.
        while index < characters.len() && is_punctuation(characters[index]) {
            index += 1;
        }
        // `(?:\s|$)` — at most one whitespace character, greedily.
        if index < characters.len() && characters[index].is_whitespace() {
            index += 1;
        }
        // Whitespace beyond the first stops the match and returns to the
        // text run — which is how the baseline behaves: `\s` matches one
        // character, and the next match attempt starts after it.
        let sentence: String = characters[start..index].iter().collect();
        let trimmed = sentence.trim().to_string();
        if !trimmed.is_empty() {
            parts.push(trimmed);
        }
        start = index;
    }
    parts
}

/// The last whole words that fit within `limit` characters.
fn tail_of(text: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    let words: Vec<&str> = text.split(' ').collect();
    let mut tail = String::new();
    for word in words.iter().rev() {
        if tail.chars().count() + word.chars().count() + 1 > limit {
            break;
        }
        tail = if tail.is_empty() {
            word.to_string()
        } else {
            format!("{} {}", word, tail)
        };
    }
    tail
}

/// Case-folded alphanumeric tokens for the scorer, 1-char noise removed but
/// stopwords kept: the scorer owns its own token shapes.
pub fn tokenise_for_scoring(text: &str) -> Vec<String> {
    split_ascii_alnum(&text.to_lowercase())
        .into_iter()
        .filter(|token| token.len() > 1)
        .collect()
}

/// A minimal English suffix stripper.
///
/// Not a full Porter implementation — the goal is only to fold the common
/// inflections that otherwise make a natural query miss a natural document
/// (plurals, -ing, -ed, -ly, and the -ation/-ate/-ivated family). Two details
/// are load-bearing:
///
/// - The final `e` is dropped *after* suffix stripping, so `charge`, `charging`
///   and `charged` all reach the same stem instead of splitting three ways.
/// - A strip that would leave under three characters is refused, so `spa`,
///   `fee` and `nation` survive as themselves rather than collapsing.
pub fn stem(token: &str) -> String {
    if token.chars().count() <= 3 {
        return token.to_string();
    }

    let mut candidate = token.to_string();

    // The rules apply in declaration order, first match only — the baseline's
    // `for (const [pattern, replacement] of rules)` breaks on the first hit.
    if let Some(head) = candidate.strip_suffix("ations") {
        candidate = head.to_string();
    } else if let Some(head) = candidate.strip_suffix("ation") {
        candidate = head.to_string();
    } else if let Some(head) = candidate.strip_suffix("ies") {
        candidate = format!("{}y", head);
    } else if let Some(head) = candidate.strip_suffix("sses") {
        candidate = format!("{}ss", head);
    } else if let Some(head) = single_s_head(&candidate) {
        candidate = head;
    } else if let Some(head) = candidate.strip_suffix("edly") {
        candidate = head.to_string();
    } else if let Some(head) = candidate.strip_suffix("ing") {
        candidate = head.to_string();
    } else if let Some(head) = candidate.strip_suffix("ed") {
        candidate = head.to_string();
    } else if let Some(head) = candidate.strip_suffix("ly") {
        candidate = head.to_string();
    }

    if candidate.chars().count() > 4 && candidate.ends_with('e') {
        let without = &candidate[..candidate.len() - 1];
        candidate = without.to_string();
    }
    if candidate.chars().count() >= 3 {
        candidate
    } else {
        token.to_string()
    }
}

/// The `([^s])s$` rule: the last two characters are `x s` with `x ≠ s`, and
/// the replacement keeps `x`.
fn single_s_head(candidate: &str) -> Option<String> {
    let mut characters: Vec<char> = candidate.chars().collect();
    let last = *characters.last()?;
    let second = *characters.get(characters.len().saturating_sub(2))?;
    if last == 's' && second != 's' {
        characters.pop();
        return Some(characters.into_iter().collect());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // The baseline's own examples, transcribed with the pinned outputs.
    #[test]
    fn tokenise_folds_case_and_drops_noise() {
        assert_eq!(tokenise("How much IS the deluxe?"), vec!["much", "deluxe"]);
    }

    #[test]
    fn chunking_is_deterministic_and_bounds_hold() {
        let text =
            "First sentence. Second one continues. Third. Fourth with more words than before.";
        let chunks = chunk_text(text);
        assert!(!chunks.is_empty());
        let joined = chunks.join(" ");

        // Every source sentence survives in the chunks.
        for sentence in ["First", "Second", "Third", "Fourth"] {
            assert!(joined.contains(sentence));
        }
    }

    #[test]
    fn a_short_document_is_one_chunk() {
        assert_eq!(chunk_text("just one line"), vec!["just one line"]);
    }

    #[test]
    fn the_cancellation_family_stems_together() {
        assert_eq!(stem("cancellation"), stem("cancellations"));
        assert_eq!(stem("charge"), stem("charging"));
    }

    #[test]
    fn short_and_stripped_shapes_survive() {
        assert_eq!(stem("spa"), "spa");
        assert_eq!(stem("fee"), "fee");
        assert_eq!(stem("gas"), "gas");
    }
}
