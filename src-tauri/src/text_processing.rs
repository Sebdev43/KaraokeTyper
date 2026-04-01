//! Text processing pipeline for transcription improvement.
//!
//! Applied after Whisper transcription, before Ollama (if enabled).
//! Inspired by Handy's approach: custom words + filler removal + stutter collapse.

/// Build a Whisper initial_prompt from custom words to bias recognition.
/// The prompt tells Whisper "these words are likely to appear" which
/// significantly improves recognition of names, technical terms, etc.
pub fn build_initial_prompt(custom_words: &[String], language: &str) -> String {
    if custom_words.is_empty() {
        return String::new();
    }

    // Whisper's initial_prompt has a 224-token limit.
    // Put the most important words (custom vocabulary) near the end
    // where they have the strongest influence on the decoder.
    let prefix = match language {
        "fr" => "Vocabulaire:",
        "en" => "Vocabulary:",
        "de" => "Wortschatz:",
        "es" => "Vocabulario:",
        _ => "Vocabulary:",
    };

    format!("{} {}", prefix, custom_words.join(", "))
}

/// Full text processing pipeline.
/// Order: filler removal → stutter collapse → custom words → whitespace normalization.
pub fn process_transcription(
    text: &str,
    custom_words: &[String],
    threshold: f32,
    filler_removal: bool,
    stutter_collapse: bool,
) -> String {
    if text.is_empty() {
        return String::new();
    }

    let mut result = text.to_string();

    if filler_removal {
        result = remove_filler_words(&result);
    }

    if stutter_collapse {
        result = collapse_stutters(&result);
    }

    if !custom_words.is_empty() {
        result = apply_custom_words(&result, custom_words, threshold);
    }

    // Final whitespace normalization
    normalize_whitespace(&result)
}

/// Remove filler words common in French and English speech.
/// Preserves punctuation attached to surrounding words.
fn remove_filler_words(text: &str) -> String {
    let fillers_fr = [
        "euh", "heu", "bah", "ben", "beh", "hein", "hmm", "hm", "mm", "ah", "oh", "eh",
    ];
    let fillers_en = [
        "uh", "um", "uhm", "umm", "hmm", "hm", "mm", "ah", "oh", "eh",
        "like", // only when standalone filler
    ];

    let words: Vec<&str> = text.split_whitespace().collect();
    let mut result: Vec<&str> = Vec::with_capacity(words.len());

    for word in &words {
        let clean = word
            .trim_matches(|c: char| c.is_ascii_punctuation())
            .to_lowercase();

        let is_filler =
            fillers_fr.contains(&clean.as_str()) || fillers_en.contains(&clean.as_str());

        if !is_filler {
            result.push(word);
        }
    }

    result.join(" ")
}

/// Collapse stutters: 3+ consecutive repetitions of short words (1-2 chars)
/// become a single instance. E.g., "je je je veux" → "je veux"
fn collapse_stutters(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 3 {
        return text.to_string();
    }

    let mut result: Vec<&str> = Vec::with_capacity(words.len());
    let mut i = 0;

    while i < words.len() {
        let word = words[i];
        let word_lower = word.to_lowercase();

        // Only collapse very short words (1-2 chars) — longer repeats may be intentional
        if word_lower.len() <= 2 {
            let mut repeat_count = 1;
            while i + repeat_count < words.len()
                && words[i + repeat_count].to_lowercase() == word_lower
            {
                repeat_count += 1;
            }

            if repeat_count >= 3 {
                // Stutter detected: keep only one instance
                result.push(word);
                i += repeat_count;
                continue;
            }
        }

        result.push(word);
        i += 1;
    }

    result.join(" ")
}

/// Apply custom word corrections using fuzzy matching.
/// For each word in the transcription, check if it's close to any custom word.
/// Uses normalized Levenshtein distance + Soundex phonetic bonus.
fn apply_custom_words(text: &str, custom_words: &[String], threshold: f32) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut result: Vec<String> = Vec::with_capacity(words.len());

    for word in &words {
        // Extract the core word (without surrounding punctuation)
        let (prefix_punct, core, suffix_punct) = split_punctuation(word);
        let core_lower = core.to_lowercase();

        // Skip very short words (articles, prepositions) — they shouldn't be corrected
        if core_lower.len() <= 2 {
            result.push(word.to_string());
            continue;
        }

        let mut best_match: Option<(&str, f32)> = None;

        for cw in custom_words {
            let cw_lower = cw.to_lowercase();

            // Length check: skip if lengths differ by more than 30%
            let len_ratio = core_lower.len() as f32 / cw_lower.len() as f32;
            if !(0.7..=1.3).contains(&len_ratio) {
                continue;
            }

            // Normalized Levenshtein distance (0.0 = identical, 1.0 = completely different)
            let lev_dist = strsim::normalized_levenshtein(&core_lower, &cw_lower);
            // Convert similarity (0-1) to distance (0-1)
            let distance = 1.0 - lev_dist as f32;

            // Soundex phonetic bonus: if same soundex, reduce distance by 30%
            let soundex_bonus = if soundex(&core_lower) == soundex(&cw_lower) {
                0.7
            } else {
                1.0
            };

            let adjusted_distance = distance * soundex_bonus;

            if adjusted_distance < threshold
                && (best_match.is_none() || adjusted_distance < best_match.unwrap().1)
            {
                best_match = Some((cw.as_str(), adjusted_distance));
            }
        }

        if let Some((replacement, _dist)) = best_match {
            // Preserve the original case pattern
            let corrected = match_case_pattern(core, replacement);
            result.push(format!("{}{}{}", prefix_punct, corrected, suffix_punct));
        } else {
            result.push(word.to_string());
        }
    }

    result.join(" ")
}

/// Simple Soundex implementation for phonetic matching.
/// Returns a 4-character code (e.g., "R163" for "Robert").
fn soundex(word: &str) -> String {
    if word.is_empty() {
        return "0000".to_string();
    }

    let chars: Vec<char> = word.chars().collect();
    let first = chars[0].to_uppercase().next().unwrap_or('0');

    let code_for = |c: char| -> Option<char> {
        match c {
            'b' | 'f' | 'p' | 'v' => Some('1'),
            'c' | 'g' | 'j' | 'k' | 'q' | 's' | 'x' | 'z' => Some('2'),
            'd' | 't' => Some('3'),
            'l' => Some('4'),
            'm' | 'n' => Some('5'),
            'r' => Some('6'),
            _ => None, // vowels and h, w, y
        }
    };

    let mut result = String::with_capacity(4);
    result.push(first);

    let mut last_code = code_for(chars[0].to_lowercase().next().unwrap_or(' '));

    for &ch in &chars[1..] {
        let code = code_for(ch.to_lowercase().next().unwrap_or(' '));
        if let Some(c) = code {
            if Some(c) != last_code {
                result.push(c);
                if result.len() == 4 {
                    break;
                }
            }
        }
        last_code = code;
    }

    // Pad with zeros
    while result.len() < 4 {
        result.push('0');
    }

    result
}

/// Split a word into (leading punctuation, core word, trailing punctuation)
fn split_punctuation(word: &str) -> (&str, &str, &str) {
    let bytes = word.as_bytes();
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_punctuation())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_punctuation())
        .map(|p| p + 1)
        .unwrap_or(0);

    if start >= end {
        return (word, "", "");
    }

    (&word[..start], &word[start..end], &word[end..])
}

/// Match the case pattern of the original word onto the replacement.
fn match_case_pattern(original: &str, replacement: &str) -> String {
    if original.chars().all(|c| c.is_uppercase()) {
        // ALL CAPS
        replacement.to_uppercase()
    } else if original.starts_with(|c: char| c.is_uppercase()) {
        // Title Case
        let mut chars = replacement.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().to_string() + chars.as_str(),
            None => String::new(),
        }
    } else {
        // lowercase
        replacement.to_string()
    }
}

/// Normalize whitespace: collapse multiple spaces, trim.
fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Filler removal ---

    #[test]
    fn test_remove_filler_words_french() {
        let result = remove_filler_words("euh je veux euh installer ça");
        assert_eq!(result, "je veux installer ça");
    }

    #[test]
    fn test_remove_filler_words_english() {
        let result = remove_filler_words("um I want to uh install that");
        assert_eq!(result, "I want to install that");
    }

    #[test]
    fn test_remove_filler_preserves_real_words() {
        let result = remove_filler_words("le chat mange la souris");
        assert_eq!(result, "le chat mange la souris");
    }

    #[test]
    fn test_remove_filler_empty() {
        assert_eq!(remove_filler_words(""), "");
        assert_eq!(remove_filler_words("euh"), "");
    }

    // --- Stutter collapse ---

    #[test]
    fn test_collapse_stutters_basic() {
        let result = collapse_stutters("je je je veux partir");
        assert_eq!(result, "je veux partir");
    }

    #[test]
    fn test_collapse_stutters_no_stutter() {
        let result = collapse_stutters("je veux partir maintenant");
        assert_eq!(result, "je veux partir maintenant");
    }

    #[test]
    fn test_collapse_stutters_double_not_collapsed() {
        // Only 3+ repetitions are collapsed, not 2
        let result = collapse_stutters("je je veux partir");
        assert_eq!(result, "je je veux partir");
    }

    #[test]
    fn test_collapse_stutters_long_word_not_collapsed() {
        // Words longer than 3 chars are never collapsed (likely intentional repetition)
        let result = collapse_stutters("oui oui oui bien sûr");
        assert_eq!(result, "oui oui oui bien sûr");
    }

    // --- Soundex ---

    #[test]
    fn test_soundex_basic() {
        assert_eq!(soundex("robert"), "R163");
        assert_eq!(soundex("rupert"), "R163");
    }

    #[test]
    fn test_soundex_empty() {
        assert_eq!(soundex(""), "0000");
    }

    #[test]
    fn test_soundex_similar_french() {
        // "claude" and "clode" should have same soundex
        assert_eq!(soundex("claude"), soundex("clode"));
    }

    // --- Custom words ---

    #[test]
    fn test_apply_custom_words_exact_match() {
        // Exact match → custom word replaces with its canonical casing
        let custom = vec!["Claude".to_string()];
        let result = apply_custom_words("claude est un modèle", &custom, 0.3);
        assert_eq!(result, "Claude est un modèle");
    }

    #[test]
    fn test_apply_custom_words_fuzzy_match() {
        let custom = vec!["Whisper".to_string()];
        let result = apply_custom_words("le moteur Wisper est rapide", &custom, 0.3);
        assert_eq!(result, "le moteur Whisper est rapide");
    }

    #[test]
    fn test_apply_custom_words_preserves_punctuation() {
        let custom = vec!["KaraokeTyper".to_string()];
        let result = apply_custom_words("j'utilise KaraokeTypr.", &custom, 0.3);
        assert_eq!(result, "j'utilise KaraokeTyper.");
    }

    #[test]
    fn test_apply_custom_words_no_match_when_too_different() {
        let custom = vec!["Whisper".to_string()];
        let result = apply_custom_words("le chat mange", &custom, 0.3);
        assert_eq!(result, "le chat mange");
    }

    #[test]
    fn test_apply_custom_words_case_preservation() {
        let custom = vec!["xdotool".to_string()];
        let result = apply_custom_words("EXDOTOAL est installé", &custom, 0.4);
        assert_eq!(result, "XDOTOOL est installé");
    }

    // --- Split punctuation ---

    #[test]
    fn test_split_punctuation_basic() {
        assert_eq!(split_punctuation("hello"), ("", "hello", ""));
        assert_eq!(split_punctuation("hello."), ("", "hello", "."));
        assert_eq!(split_punctuation("(hello)"), ("(", "hello", ")"));
        assert_eq!(split_punctuation("\"hello\","), ("\"", "hello", "\","));
    }

    // --- Case pattern ---

    #[test]
    fn test_match_case_pattern() {
        assert_eq!(match_case_pattern("Hello", "world"), "World");
        assert_eq!(match_case_pattern("HELLO", "world"), "WORLD");
        assert_eq!(match_case_pattern("hello", "World"), "World");
    }

    // --- Initial prompt ---

    #[test]
    fn test_build_initial_prompt_french() {
        let words = vec!["Claude".to_string(), "Tauri".to_string()];
        let prompt = build_initial_prompt(&words, "fr");
        assert_eq!(prompt, "Vocabulaire: Claude, Tauri");
    }

    #[test]
    fn test_build_initial_prompt_empty() {
        let prompt = build_initial_prompt(&[], "fr");
        assert_eq!(prompt, "");
    }

    // --- Full pipeline ---

    #[test]
    fn test_process_transcription_full_pipeline() {
        let custom = vec!["Whisper".to_string()];
        let result = process_transcription(
            "euh je je je veux installer Wisper",
            &custom,
            0.3,
            true, // filler removal
            true, // stutter collapse
        );
        assert_eq!(result, "je veux installer Whisper");
    }

    #[test]
    fn test_process_transcription_empty() {
        let result = process_transcription("", &[], 0.3, true, true);
        assert_eq!(result, "");
    }

    #[test]
    fn test_process_transcription_no_processing() {
        let result = process_transcription(
            "bonjour le monde",
            &[],
            0.3,
            false, // no filler removal
            false, // no stutter collapse
        );
        assert_eq!(result, "bonjour le monde");
    }
}
