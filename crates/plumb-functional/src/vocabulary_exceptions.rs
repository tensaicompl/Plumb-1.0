//! The authoritative English singularization exception tables of the pilot vocabulary
//! normalization (plan S1.5, Hotfix 030). Both tables are sorted; no other exception exists.

/// Irregular plural -> singular, sorted by plural.
pub(crate) const IRREGULAR_PLURALS: [(&str, &str); 14] = [
    ("analyses", "analysis"),
    ("buses", "bus"),
    ("children", "child"),
    ("feet", "foot"),
    ("geese", "goose"),
    ("indices", "index"),
    ("matrices", "matrix"),
    ("men", "man"),
    ("mice", "mouse"),
    ("people", "person"),
    ("statuses", "status"),
    ("teeth", "tooth"),
    ("vertices", "vertex"),
    ("women", "woman"),
];

/// Invariant and mass nouns that are never singularized, sorted.
pub(crate) const INVARIANT_NOUNS: [&str; 10] = [
    "data",
    "equipment",
    "gas",
    "information",
    "metadata",
    "news",
    "series",
    "software",
    "species",
    "staff",
];
