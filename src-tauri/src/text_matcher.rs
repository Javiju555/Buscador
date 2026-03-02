pub fn normalize(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut previous_was_space = false;

    for character in value.to_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            result.push(character);
            previous_was_space = false;
            continue;
        }

        if !previous_was_space {
            result.push(' ');
            previous_was_space = true;
        }
    }

    result.trim().to_string()
}

pub fn split_terms(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .filter(|term| term.len() > 1)
        .map(ToOwned::to_owned)
        .collect()
}

pub fn score(query_normalized: &str, query_terms: &[String], fields: &[&str]) -> i32 {
    if query_normalized.is_empty() {
        return 0;
    }

    fields
        .iter()
        .map(|field| score_field(query_normalized, query_terms, field))
        .max()
        .unwrap_or(0)
}

fn score_field(query_normalized: &str, query_terms: &[String], field: &str) -> i32 {
    if field.is_empty() {
        return 0;
    }

    let mut score = 0;

    if field == query_normalized {
        return 420;
    }

    if field.starts_with(query_normalized) {
        score = score.max(330 - query_normalized.len() as i32);
    }

    if let Some(position) = field.find(query_normalized) {
        score = score.max(245 - (position as i32).min(140));
    }

    let mut hits = 0;

    for term in query_terms {
        if let Some(position) = field.find(term) {
            hits += 1;
            score += 56 - (position as i32).min(42);
            if position == 0 || field.as_bytes()[position - 1] == b' ' {
                score += 18;
            }
        }
    }

    if hits == query_terms.len() && hits > 0 {
        score += 76;
    }

    if let Some(fuzzy) = fuzzy_subsequence_score(query_normalized, field) {
        score = score.max(120 + fuzzy);
    }

    for term in query_terms {
        if let Some(term_fuzzy) = fuzzy_subsequence_score(term, field) {
            score += term_fuzzy / 4;
        }
    }

    score.max(0)
}

fn fuzzy_subsequence_score(needle: &str, haystack: &str) -> Option<i32> {
    if needle.is_empty() || haystack.is_empty() {
        return None;
    }

    let needle_bytes = needle.as_bytes();
    let haystack_bytes = haystack.as_bytes();

    let mut needle_index = 0usize;
    let mut first_match = None::<usize>;
    let mut last_match = None::<usize>;
    let mut score = 0i32;

    for (index, &character) in haystack_bytes.iter().enumerate() {
        if needle_index >= needle_bytes.len() || character != needle_bytes[needle_index] {
            continue;
        }

        if first_match.is_none() {
            first_match = Some(index);
        }

        score += 18;

        if index == 0 || is_boundary(haystack_bytes[index - 1]) {
            score += 12;
        }

        if let Some(previous) = last_match {
            let gap = index - previous;
            if gap == 1 {
                score += 20;
            } else {
                score -= (gap.min(12) as i32) * 2;
            }
        }

        last_match = Some(index);
        needle_index += 1;
        if needle_index >= needle_bytes.len() {
            break;
        }
    }

    if needle_index < needle_bytes.len() {
        return None;
    }

    let first = first_match.unwrap_or(0);
    let last = last_match.unwrap_or(first);
    let span = last.saturating_sub(first) + 1;

    score += (needle_bytes.len() as i32) * 7;
    score += 72 - (span as i32).min(72);
    score += 36 - (first as i32).min(36);

    Some(score.max(0))
}

fn is_boundary(character: u8) -> bool {
    matches!(character, b' ' | b'_' | b'-' | b'/' | b'\\' | b'.')
}
