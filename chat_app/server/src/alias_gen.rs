use rand::{SeedableRng, rngs::SmallRng, seq::IndexedRandom};

/// Generates random user aliases in the format "adjective noun"
pub struct AliasGenerator {
    adjectives: Vec<&'static str>,
    nouns: Vec<&'static str>,
    rng: SmallRng,
}

static_assertions::assert_impl_all!(AliasGenerator: Send, Sync);

impl Default for AliasGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl AliasGenerator {
    /// Creates a new AliasGenerator with predefined lists of adjectives and nouns
    pub fn new() -> Self {
        let adjectives = vec![
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

        let nouns = vec![
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

        let rng = rand::rngs::SmallRng::from_os_rng();

        Self {
            adjectives,
            nouns,
            rng,
        }
    }

    fn get_pair(&mut self) -> (&'static str, &'static str) {
        let adjective = self.adjectives.choose(&mut self.rng).unwrap();
        let noun = self.nouns.choose(&mut self.rng).unwrap();

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
