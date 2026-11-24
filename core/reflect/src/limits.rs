//! Required reflection limits parsed from the strict reflect.toml subset.

/// Caller-supplied resource limits, with no defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Maximum path length.
    pub max_path_length: usize,
    /// Maximum text file size in bytes.
    pub max_text_file_bytes: u64,
    /// Maximum list length.
    pub max_list_length: usize,
    /// Maximum nesting depth.
    pub max_nesting_depth: usize,
}

/// Configuration failures; all line numbers are one-based.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LimitsError {
    /// No limits table was supplied.
    MissingTable,
    /// A required key was not supplied.
    MissingKey {
        /// The missing key.
        key: &'static str,
    },
    /// A key is not part of the supported subset.
    UnknownKey {
        /// The offending line.
        line: usize,
        /// The unknown key.
        key: String,
    },
    /// A key occurs more than once.
    DuplicateKey {
        /// The second occurrence's line.
        line: usize,
        /// The repeated key.
        key: String,
    },
    /// A table is unknown or repeated.
    UnknownTable {
        /// The offending line.
        line: usize,
    },
    /// An assignment precedes the limits table.
    KeyOutsideTable {
        /// The offending line.
        line: usize,
    },
    /// A value is not a canonical integer fitting its field.
    BadValue {
        /// The offending line.
        line: usize,
        /// The affected key.
        key: String,
    },
    /// A limit must be positive.
    Zero {
        /// The offending line.
        line: usize,
        /// The affected key.
        key: String,
    },
    /// A line is not a supported assignment.
    BadLine {
        /// The offending line.
        line: usize,
    },
}

impl std::fmt::Display for LimitsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingTable => f.write_str("missing [limits] table"),
            Self::MissingKey { key } => write!(f, "missing limits key: {key}"),
            Self::UnknownKey { line, key } => write!(f, "line {line}: unknown key {key}"),
            Self::DuplicateKey { line, key } => write!(f, "line {line}: duplicate key {key}"),
            Self::UnknownTable { line } => write!(f, "line {line}: unknown or repeated table"),
            Self::KeyOutsideTable { line } => write!(f, "line {line}: key outside [limits]"),
            Self::BadValue { line, key } => write!(f, "line {line}: bad value for {key}"),
            Self::Zero { line, key } => write!(f, "line {line}: zero limit for {key}"),
            Self::BadLine { line } => write!(f, "line {line}: malformed limits assignment"),
        }
    }
}

impl std::error::Error for LimitsError {}

impl Limits {
    /// Parses required limits without defaults; the caller reads the file.
    pub fn parse(text: &str) -> Result<Limits, LimitsError> {
        const KEYS: [&str; 4] = [
            "max_path_length",
            "max_text_file_bytes",
            "max_list_length",
            "max_nesting_depth",
        ];
        fn trim(s: &str) -> &str {
            s.trim_matches([' ', '\t'])
        }
        let mut table = false;
        let mut values = [None; 4];
        for (index, raw) in text.split('\n').enumerate() {
            let line = index + 1;
            let entry = trim(raw.strip_suffix('\r').unwrap_or(raw));
            if entry.is_empty() || entry.starts_with('#') {
                continue;
            }
            if entry.starts_with('[') {
                if entry != "[limits]" || table {
                    return Err(LimitsError::UnknownTable { line });
                }
                table = true;
                continue;
            }
            let (key, value) = entry.split_once('=').ok_or(LimitsError::BadLine { line })?;
            let key = trim(key);
            if key.is_empty() {
                return Err(LimitsError::BadLine { line });
            }
            if !table {
                return Err(LimitsError::KeyOutsideTable { line });
            }
            let slot = KEYS
                .iter()
                .position(|known| *known == key)
                .ok_or_else(|| LimitsError::UnknownKey {
                    line,
                    key: key.into(),
                })?;
            if values[slot].is_some() {
                return Err(LimitsError::DuplicateKey {
                    line,
                    key: key.into(),
                });
            }
            let value = trim(value.split_once('#').map_or(value, |(value, _)| value));
            let bad_value = || LimitsError::BadValue {
                line,
                key: key.into(),
            };
            if value.is_empty()
                || !value.bytes().all(|b| b.is_ascii_digit())
                || (value.len() > 1 && value.starts_with('0'))
            {
                return Err(bad_value());
            }
            let number = value.parse::<u64>().map_err(|_| bad_value())?;
            if slot != 1 && usize::try_from(number).is_err() {
                return Err(bad_value());
            }
            if number == 0 {
                return Err(LimitsError::Zero {
                    line,
                    key: key.into(),
                });
            }
            values[slot] = Some(number);
        }
        if !table {
            return Err(LimitsError::MissingTable);
        }
        for (key, value) in KEYS.into_iter().zip(values) {
            if value.is_none() {
                return Err(LimitsError::MissingKey { key });
            }
        }
        // Every slot is present and each usize conversion was checked above.
        Ok(Self {
            max_path_length: values[0].unwrap() as usize,
            max_text_file_bytes: values[1].unwrap(),
            max_list_length: values[2].unwrap() as usize,
            max_nesting_depth: values[3].unwrap() as usize,
        })
    }
}
