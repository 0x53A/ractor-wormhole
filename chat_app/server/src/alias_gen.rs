use rand::{
    SeedableRng,
    rngs::SmallRng,
    seq::{IndexedRandom, SliceRandom},
};

/// Generates random user aliases in the format "adjective noun"
pub struct AliasGenerator {
    adjectives: Vec<&'static str>,
    nouns: Vec<&'static str>,
    rng: SmallRng,

    generated_count: usize,
}

static_assertions::assert_impl_all!(AliasGenerator: Send, Sync);

impl Default for AliasGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl AliasGenerator {
    // note: the total number of unique combinations is 30 * 30 = 900
    const ARRAY_LEN: usize = 30;

    const ADJECTIVES: [&str; Self::ARRAY_LEN] = [
        "smirking",
        "corrupted",
        "vengeful",
        "iron",
        "grim",
        "bloody",
        "warp-touched",
        "eternal",
        "skulking",
        "ancient",
        "dark",
        "chaotic",
        "gilded",
        "forgotten",
        "zealous",
        "stalwart",
        "unseen",
        "feral",
        "stoic",
        "mechanized",
        "blessed",
        "cursed",
        "frenzied",
        "implacable",
        "ruinous",
        "scarred",
        "undying",
        "blighted",
        "relentless",
        "shrieking",
    ];

    const NOUNS: [&str; Self::ARRAY_LEN] = [
        "gutterrunner",
        "berserker",
        "arbiter",
        "inquisitor",
        "crusader",
        "guardian",
        "dreadnought",
        "reaper",
        "warbringer",
        "mechanicus",
        "prophet",
        "skaven",
        "paladin",
        "heretic",
        "cultist",
        "specter",
        "slayer",
        "executioner",
        "scribe",
        "marauder",
        "warpsmith",
        "ratcatcher",
        "plague-bearer",
        "voidwalker",
        "magus",
        "commissar",
        "avatar",
        "stormcaller",
        "enforcer",
        "nightstalker",
    ];

    /// Creates a new AliasGenerator with predefined lists of adjectives and nouns
    pub fn new() -> Self {
        let mut rng = rand::rngs::SmallRng::from_os_rng();

        let mut adjectives = Self::ADJECTIVES.to_vec();
        adjectives.shuffle(&mut rng);

        let mut nouns = Self::NOUNS.to_vec();
        nouns.shuffle(&mut rng);

        Self {
            adjectives,
            nouns,
            rng,
            generated_count: 0,
        }
    }

    fn get_pair(&mut self) -> (&'static str, &'static str) {
        let offset = self.generated_count / Self::ARRAY_LEN;

        let adjective_index = self.generated_count % Self::ARRAY_LEN;
        let noun_index = (self.generated_count + offset) % Self::ARRAY_LEN;

        let adjective = self.adjectives[adjective_index];
        let noun = self.nouns[noun_index];

        self.generated_count += 1;

        (adjective, noun)
    }

    /// Generates a random alias
    pub fn _generate(&mut self) -> String {
        let (adjective, noun) = self.get_pair();
        format!("{} {}", adjective, noun)
    }

    fn capitalize_first_letter(word: &str) -> String {
        let mut chars = word.chars();
        let first = chars.next().unwrap().to_uppercase();
        let rest: String = chars.collect();
        format!("{}{}", first, rest)
    }

    /// Generates a random alias with capitalized words
    pub fn generate_capitalized(&mut self) -> String {
        let (adjective, noun) = self.get_pair();
        let adjective = Self::capitalize_first_letter(adjective);
        let noun = Self::capitalize_first_letter(noun);
        format!("{} {}", adjective, noun)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alias_generator_generate_capitalized() {
        let mut generator = AliasGenerator::new();
        let alias = generator.generate_capitalized();
        assert!(!alias.is_empty());
        let split = alias.split_whitespace();
        assert!(split.clone().count() == 2);
        for word in split {
            assert!(!word.is_empty());
            assert!(word.chars().next().unwrap().is_uppercase());
        }
    }

    #[test]
    fn test_alias_generator_unique() {
        let mut generator = AliasGenerator::new();
        let alias1 = generator.generate_capitalized();
        let alias2 = generator.generate_capitalized();
        assert_ne!(alias1, alias2);
    }
}
