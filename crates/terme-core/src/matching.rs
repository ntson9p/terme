//! How well a typed word matches a candidate name.

/// Match quality, best first. The order of the variants is the ranking order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MatchKind {
    Exact,
    Prefix,
    /// Prefix, ignoring case.
    PrefixNoCase,
    /// Each typed letter starts a part of the name: `--ae` → `--allow-empty`.
    WordStart,
    /// The typed letters appear in order, starting with the same first letter.
    Subsequence,
    /// One letter wrong, missing, extra or swapped: `comit` → `commit`.
    Typo,
    /// The typed word appears in the description, not the name.
    Description,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub kind: MatchKind,
    /// Char indices in the candidate that matched the typed text.
    pub positions: Vec<usize>,
}

/// Leading dashes, which must match literally.
fn dash_len(chars: &[char]) -> usize {
    chars.iter().take_while(|&&c| c == '-').count()
}

fn is_separator(c: char) -> bool {
    matches!(c, '-' | '_' | '.' | '/' | ':' | '@' | ' ')
}

/// Match `query` against a candidate name. An empty query matches everything
/// as a prefix.
#[must_use]
pub fn match_name(query: &str, candidate: &str) -> Option<Match> {
    let q: Vec<char> = query.chars().collect();
    let c: Vec<char> = candidate.chars().collect();
    if q.len() > c.len() + 1 {
        return None;
    }
    if q == c {
        return Some(Match {
            kind: MatchKind::Exact,
            positions: (0..c.len()).collect(),
        });
    }
    if c.starts_with(&q) {
        return Some(Match {
            kind: MatchKind::Prefix,
            positions: (0..q.len()).collect(),
        });
    }
    if q.len() <= c.len()
        && q.iter()
            .zip(&c)
            .all(|(a, b)| a.to_lowercase().eq(b.to_lowercase()))
    {
        return Some(Match {
            kind: MatchKind::PrefixNoCase,
            positions: (0..q.len()).collect(),
        });
    }
    // The fuzzy kinds need the same leading dashes and first letter, so that
    // `-x` never fuzzy-matches `-v` and `--a` doesn't match everything with an `a`.
    let qd = dash_len(&q);
    let cd = dash_len(&c);
    if qd != cd || q.len() <= qd + 1 || c.get(cd) != q.get(qd) {
        return typo(&q, &c, qd);
    }
    if let Some(positions) = word_starts(&q, &c, qd) {
        return Some(Match {
            kind: MatchKind::WordStart,
            positions,
        });
    }
    if let Some(positions) = subsequence(&q, &c, qd) {
        return Some(Match {
            kind: MatchKind::Subsequence,
            positions,
        });
    }
    typo(&q, &c, qd)
}

fn word_starts(q: &[char], c: &[char], dashes: usize) -> Option<Vec<usize>> {
    let mut positions: Vec<usize> = (0..dashes).collect();
    let mut ci = dashes;
    for (n, &qc) in q[dashes..].iter().enumerate() {
        let found = (ci..c.len()).find(|&i| {
            c[i] == qc && (n == 0 && i == dashes || i > dashes && is_separator(c[i - 1]))
        })?;
        positions.push(found);
        ci = found + 1;
    }
    Some(positions)
}

fn subsequence(q: &[char], c: &[char], dashes: usize) -> Option<Vec<usize>> {
    let mut positions: Vec<usize> = (0..dashes).collect();
    let mut ci = dashes;
    for &qc in &q[dashes..] {
        let found = (ci..c.len()).find(|&i| c[i] == qc)?;
        positions.push(found);
        ci = found + 1;
    }
    Some(positions)
}

fn typo(q: &[char], c: &[char], dashes: usize) -> Option<Match> {
    if q.len() - dashes.min(q.len()) < 3 {
        return None;
    }
    // Compare against the whole name and against prefixes about as long as
    // the query, so `comi` (for `commi`) also counts.
    let lengths = [c.len(), q.len(), q.len() + 1];
    let close = lengths
        .iter()
        .filter(|&&n| n <= c.len())
        .any(|&n| damerau_levenshtein(q, &c[..n]) <= 1);
    close.then(|| Match {
        kind: MatchKind::Typo,
        positions: Vec::new(),
    })
}

/// Edit distance with adjacent swaps counting as one edit.
fn damerau_levenshtein(a: &[char], b: &[char]) -> usize {
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=m {
        d[0][j] = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = best;
        }
    }
    d[n][m]
}

/// Whether `word` (3+ letters, dashes ignored) appears as a whole word or word
/// start in `description`, case-insensitively.
#[must_use]
pub fn match_description(word: &str, description: &str) -> bool {
    let word = word.trim_start_matches('-').to_lowercase();
    if word.chars().count() < 3 {
        return false;
    }
    let description = description.to_lowercase();
    description
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(&word))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(q: &str, c: &str) -> Option<MatchKind> {
        match_name(q, c).map(|m| m.kind)
    }

    #[test]
    fn match_kinds() {
        assert_eq!(kind("commit", "commit"), Some(MatchKind::Exact));
        assert_eq!(kind("com", "commit"), Some(MatchKind::Prefix));
        assert_eq!(kind("", "commit"), Some(MatchKind::Prefix));
        assert_eq!(kind("Doc", "docker"), Some(MatchKind::PrefixNoCase));
        assert_eq!(kind("--ae", "--allow-empty"), Some(MatchKind::WordStart));
        assert_eq!(kind("chpk", "cherry-pick"), Some(MatchKind::Subsequence));
        assert_eq!(kind("comit", "commit"), Some(MatchKind::Subsequence));
        assert_eq!(kind("commti", "commit"), Some(MatchKind::Typo));
        assert_eq!(kind("sttaus", "status"), Some(MatchKind::Typo));
        assert_eq!(kind("xyz", "commit"), None);
    }

    #[test]
    fn short_options_never_match_fuzzily() {
        assert_eq!(kind("-x", "-v"), None);
        assert_eq!(kind("-x", "--extract"), None);
        assert_eq!(kind("-", "-x"), Some(MatchKind::Prefix));
    }

    #[test]
    fn fuzzy_needs_the_same_first_letter() {
        assert_eq!(kind("--a", "--create"), None);
        assert_eq!(kind("--am", "--allow-empty"), Some(MatchKind::Subsequence));
    }

    #[test]
    fn positions_point_at_matched_chars() {
        let m = match_name("--ae", "--allow-empty").unwrap();
        assert_eq!(m.positions, [0, 1, 2, 8]);
    }

    #[test]
    fn descriptions() {
        assert!(match_description("--port", "publish a container's port to the host"));
        assert!(match_description("--port", "all exposed ports"));
        assert!(!match_description("--po", "publish a port"));
        assert!(!match_description("--port", "support"));
    }
}
