//! How much recall returns and how hard it looks: a token budget and a depth, instead of one fixed hit count.
//!
//! Hindsight's recall takes `max_tokens` and a `budget` level (low, mid, high) for the same reason: a quick lookup in the middle of a turn
//! and a thorough search for a hard question should not cost the same. `Depth` sets how many candidates are considered, how far the graph is
//! walked and whether the reranker runs; `pack` fits what was found into the token budget, best first, and says what it left out.

/// A rough token count: four characters to a token, which is close enough for fitting a budget and needs no tokenizer.
pub fn tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Depth {
    /// A quick lookup: few hits, one hop, no reranker.
    Low,
    #[default]
    Mid,
    /// A thorough search for a hard question.
    High,
}

impl Depth {
    pub fn parse(s: &str) -> Option<Depth> {
        match s.trim().to_lowercase().as_str() {
            "low" | "quick" => Some(Depth::Low),
            "mid" | "medium" => Some(Depth::Mid),
            "high" | "deep" => Some(Depth::High),
            _ => None,
        }
    }

    /// How many hits the ranking keeps.
    pub fn hits(self) -> usize {
        match self {
            Depth::Low => 5,
            Depth::Mid => 8,
            Depth::High => 14,
        }
    }

    /// How many hops of the memory graph are followed from the direct hits.
    pub fn hops(self) -> usize {
        match self {
            Depth::Low => 1,
            Depth::Mid => 3,
            Depth::High => 4,
        }
    }

    /// Whether the model reranker runs (when one is configured).
    pub fn rerank(self) -> bool {
        self != Depth::Low
    }

    /// The token budget when none is asked for: what the fixed caps used to allow.
    pub fn default_tokens(self) -> usize {
        match self {
            Depth::Low => 1_000,
            Depth::Mid => 2_000,
            Depth::High => 4_000,
        }
    }
}

/// The most a budget can be: a larger ask is a mistake, and recall is not a way to fill the context.
pub const MAX_TOKENS: usize = 8_000;
/// The least: below this not even one memory fits.
pub const MIN_TOKENS: usize = 100;

pub fn clamp_tokens(asked: Option<usize>, depth: Depth) -> usize {
    asked.unwrap_or_else(|| depth.default_tokens()).clamp(MIN_TOKENS, MAX_TOKENS)
}

/// What fits: the lines in order until the next would pass the budget. The first is always kept, cut to fit if it must be, so a search that
/// found something never answers with nothing. The second number is how many lines did not fit.
pub fn pack(lines: &[String], max_tokens: usize) -> (Vec<String>, usize) {
    let mut out: Vec<String> = Vec::new();
    let mut used = 0;
    for (i, line) in lines.iter().enumerate() {
        let t = tokens(line);
        if used + t > max_tokens {
            if i == 0 {
                let keep: String = line.chars().take(max_tokens * 4).collect();
                out.push(format!("{keep}…"));
                return (out, lines.len() - 1);
            }
            return (out, lines.len() - i);
        }
        used += t;
        out.push(line.clone());
    }
    (out, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_about_a_quarter_of_the_characters() {
        assert_eq!((tokens(""), tokens("abc"), tokens("abcd"), tokens("abcde")), (0, 1, 1, 2));
        assert_eq!(tokens(&"é".repeat(8)), 2, "characters, not bytes");
    }

    #[test]
    fn depth_is_read_loosely_and_defaults_to_the_middle() {
        assert_eq!((Depth::parse("LOW"), Depth::parse(" deep "), Depth::parse("medium"), Depth::parse("max")), (Some(Depth::Low), Some(Depth::High), Some(Depth::Mid), None));
        assert_eq!(Depth::default(), Depth::Mid);
        assert_eq!(Depth::Mid.hits(), 8, "the old fixed cap");
        assert_eq!(Depth::Mid.hops(), 3, "the old fixed depth");
        assert!(Depth::Mid.rerank() && Depth::High.rerank() && !Depth::Low.rerank());
        assert!(Depth::Low.hits() < Depth::Mid.hits() && Depth::Mid.hits() < Depth::High.hits());
    }

    #[test]
    fn the_budget_is_asked_for_or_defaulted_and_always_bounded() {
        assert_eq!(clamp_tokens(None, Depth::Mid), 2_000);
        assert_eq!(clamp_tokens(None, Depth::High), 4_000);
        assert_eq!(clamp_tokens(Some(500), Depth::High), 500);
        assert_eq!(clamp_tokens(Some(1), Depth::Mid), MIN_TOKENS);
        assert_eq!(clamp_tokens(Some(1_000_000), Depth::Mid), MAX_TOKENS);
    }

    #[test]
    fn packing_keeps_the_best_that_fit_and_counts_the_rest() {
        let lines: Vec<String> = ["a".repeat(40), "b".repeat(40), "c".repeat(40), "d".repeat(8)].into();
        assert_eq!(pack(&lines, 100), (lines.clone(), 0), "all fit");
        assert_eq!(pack(&lines, 25), (lines[..2].to_vec(), 2), "ten tokens each: two fit, the rest are counted");
        assert_eq!(pack(&lines, 30).1, 1, "a smaller later line does not jump the queue past one that did not fit");
        assert_eq!(pack(&[], 100), (vec![], 0));
    }

    #[test]
    fn the_first_line_is_cut_to_fit_rather_than_dropped() {
        let (out, left) = pack(&["x".repeat(1_000), "y".to_string()], 10);
        assert_eq!(left, 1);
        assert_eq!(out.len(), 1);
        assert!(out[0].ends_with('…') && tokens(&out[0]) <= 11, "{}", out[0].len());
    }
}
