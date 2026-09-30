use crate::rng::Rng;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NameStyle {
    Hex,
    Alpha,
    Mixed,
    Unicode,
    Short,
}

impl NameStyle {
    pub fn all() -> [NameStyle; 5] {
        [
            NameStyle::Hex,
            NameStyle::Alpha,
            NameStyle::Mixed,
            NameStyle::Unicode,
            NameStyle::Short,
        ]
    }
}

const HEX: &[char] = &[
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];
pub const HEX_CHARS: &[char] = HEX;
const LOWER: &[char] = &[
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z',
];
const UPPER: &[char] = &[
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S',
    'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
];
const DIGITS: &[char] = &['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'];
const UNICODE: &[char] = &[
    '\u{0430}', '\u{0435}', '\u{043E}', '\u{0443}', '\u{0441}', '\u{044F}', '\u{03B1}', '\u{03B2}',
    '\u{03B3}', '\u{03B4}', '\u{03B5}', '\u{03B6}', '\u{03B7}', '\u{03B8}', '\u{03B9}', '\u{03BA}',
    '\u{03BB}', '\u{03BC}', '\u{03BD}', '\u{03BE}', '\u{03BF}', '\u{03C0}', '\u{03C1}', '\u{03C3}',
    '\u{03C4}', '\u{03C5}', '\u{03C6}', '\u{03C7}',
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NameState {
    Fresh,
    Used,
}

const RESERVED: &[&str] = &[
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "arguments",
    "eval",
    "undefined",
    "NaN",
    "Infinity",
    "Object",
    "Array",
    "String",
    "Number",
    "Boolean",
    "Symbol",
    "Math",
    "JSON",
];

#[derive(Debug, Clone)]
pub struct NameFactory {
    pub style: NameStyle,
    pub prefix: String,
    used: std::collections::HashSet<String>,
    issued: Vec<String>,
    state: NameState,
}

impl NameFactory {
    pub fn new(rng: &mut Rng) -> NameFactory {
        let style = *rng.pick(&NameStyle::all()).unwrap_or(&NameStyle::Hex);
        let prefix = match style {
            NameStyle::Hex => {
                let mut p = String::from("_0x");
                p.push_str(&rng.sample_string(HEX, 2));
                p
            }
            NameStyle::Short => String::from("_"),
            _ => String::new(),
        };
        NameFactory {
            style,
            prefix,
            used: std::collections::HashSet::new(),
            issued: Vec::new(),
            state: NameState::Fresh,
        }
    }

    pub fn style(&self) -> NameStyle {
        self.style
    }

    pub fn issued(&self) -> &[String] {
        &self.issued
    }

    pub fn reserve(&mut self, name: &str) {
        self.used.insert(name.to_string());
    }

    pub fn reserved(&self, name: &str) -> bool {
        self.used.contains(name)
    }

    pub fn set_state(&mut self, used: bool) {
        self.state = if used {
            NameState::Used
        } else {
            NameState::Fresh
        };
    }

    pub fn next(&mut self, rng: &mut Rng) -> String {
        for _ in 0..4096 {
            let candidate = self.compose(rng);
            if RESERVED.contains(&candidate.as_str()) {
                continue;
            }
            if self.used.insert(candidate.clone()) {
                self.issued.push(candidate.clone());
                return candidate;
            }
        }
        let mut fallback = format!("{}x", self.prefix);
        while !self.used.insert(fallback.clone()) {
            fallback.push('x');
        }
        self.issued.push(fallback.clone());
        fallback
    }

    pub fn next_many(&mut self, rng: &mut Rng, count: usize) -> Vec<String> {
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(self.next(rng));
        }
        out
    }

    fn compose(&self, rng: &mut Rng) -> String {
        let mut candidate = self.prefix.clone();
        match self.style {
            NameStyle::Hex => {
                let length = match self.state {
                    NameState::Fresh => rng.range(4, 7),
                    NameState::Used => rng.range(3, 5),
                };
                candidate.push_str(&rng.sample_string(HEX, length));
            }
            NameStyle::Alpha => {
                let length = match self.state {
                    NameState::Fresh => rng.range(7, 12),
                    NameState::Used => rng.range(5, 8),
                };
                candidate.push_str(&rng.sample_string(LOWER, length));
            }
            NameStyle::Mixed => {
                let length = match self.state {
                    NameState::Fresh => rng.range(5, 9),
                    NameState::Used => rng.range(3, 6),
                };
                for index in 0..length {
                    if index == 0 {
                        let pool = if rng.chance(50) { LOWER } else { UPPER };
                        let pick = rng.below(pool.len());
                        if let Some(c) = pool.get(pick) {
                            candidate.push(*c);
                        }
                        continue;
                    }
                    let pool = match (index + rng.below(3)) % 3 {
                        0 => LOWER,
                        1 => UPPER,
                        _ => DIGITS,
                    };
                    let pick = rng.below(pool.len());
                    if let Some(c) = pool.get(pick) {
                        candidate.push(*c);
                    }
                }
            }
            NameStyle::Unicode => {
                let length = match self.state {
                    NameState::Fresh => rng.range(4, 8),
                    NameState::Used => rng.range(3, 6),
                };
                candidate.push_str(&rng.sample_string(UNICODE, length));
            }
            NameStyle::Short => {
                let length = rng.range(1, 2);
                candidate.push_str(&rng.sample_string(LOWER, length));
            }
        }
        candidate
    }
}
