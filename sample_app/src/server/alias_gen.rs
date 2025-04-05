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
            "sparkly", "azure", "golden", "swift", "clever", "gentle", "brave", "mighty",
            "crimson", "emerald", "vibrant", "cosmic", "fluffy", "peaceful", "mystic", "radiant",
            "curious", "witty", "silent", "jolly", "bold", "wise", "humble", "noble", "fierce",
            "happy", "calm", "wild", "friendly",
        ];

        let nouns = vec![
            "dolphin",
            "eagle",
            "tiger",
            "panda",
            "phoenix",
            "turtle",
            "dragon",
            "wolf",
            "falcon",
            "koala",
            "panther",
            "fox",
            "rabbit",
            "raccoon",
            "squirrel",
            "bear",
            "lynx",
            "owl",
            "unicorn",
            "raven",
            "whale",
            "butterfly",
            "hawk",
            "leopard",
            "lion",
            "otter",
            "shark",
            "elephant",
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
