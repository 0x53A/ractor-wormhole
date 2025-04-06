use rand::Rng;

/// Generates random user aliases in the format "adjective noun"
pub struct AliasGenerator {
    adjectives: Vec<&'static str>,
    nouns: Vec<&'static str>,
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

        Self { adjectives, nouns }
    }

    /// Generates a random alias
    pub fn generate(&self) -> String {
        let mut rng = rand::thread_rng();

        let adjective = self.adjectives[rng.gen_range(0..self.adjectives.len())];
        let noun = self.nouns[rng.gen_range(0..self.nouns.len())];

        format!("{} {}", adjective, noun)
    }

    /// Generates a random alias with capitalized words
    pub fn generate_capitalized(&self) -> String {
        let alias = self.generate();
        alias
            .split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            })
            .collect::<Vec<String>>()
            .join(" ")
    }
}
