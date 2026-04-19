pub const GENERAL_TOPIC: &str = "Allgemein / Sonstiges";
pub const FORMAT_TOPIC: &str = "Rezensionen, Essays & Interviews";

struct TopicRule {
    label: &'static str,
    keywords: &'static [&'static str],
    section_hints: &'static [&'static str],
}

const MIN_CONFIDENT_SCORE: usize = 5;
const MIN_SECTION_FALLBACK_SCORE: usize = 3;

const TOPIC_RULES: &[TopicRule] = &[
    TopicRule {
        label: "Politik & Staat",
        keywords: &[
            "politik",
            "staat",
            "regierung",
            "demokr",
            "parlament",
            "polizei",
            "verfassung",
            "recht",
            "regime",
            "herrschaft",
            "imperial",
            "faschis",
            "öffentlichkeit",
            "oeffentlichkeit",
        ],
        section_hints: &["politik", "zeitgeschichte"],
    },
    TopicRule {
        label: "Wirtschaft & Arbeit",
        keywords: &[
            "wirtschaft",
            "arbeit",
            "arbeits",
            "kapital",
            "kapitalismus",
            "markt",
            "neoliberal",
            "tarif",
            "lohn",
            "unternehmen",
            "finanz",
            "ökonomie",
            "oekonomie",
            "ungleichheit",
        ],
        section_hints: &["wirtschaft", "recht"],
    },
    TopicRule {
        label: "Digitalisierung & KI",
        keywords: &[
            "digital",
            "digitalisierung",
            "ki",
            "künstliche intelligenz",
            "kunstliche intelligenz",
            "algorithm",
            "daten",
            "datenanalyse",
            "plattform",
            "software",
            "internet",
            "maschine",
            "automatis",
            "technologie",
            "kyber",
        ],
        section_hints: &["technik"],
    },
    TopicRule {
        label: "Theorie & Methode",
        keywords: &[
            "theorie",
            "sozialtheorie",
            "methode",
            "methodik",
            "mixed-method",
            "statistik",
            "hermeneutik",
            "philosoph",
            "anthropologie",
            "soziologie",
            "kritische theorie",
            "wissenssoziologie",
            "wissenschaft",
            "hochschule",
            "universit",
            "forschung",
        ],
        section_hints: &["gesellschaftstheorie", "anthropologie", "wissenschaft"],
    },
    TopicRule {
        label: "Kultur & Medien",
        keywords: &[
            "kultur",
            "medien",
            "medientheorie",
            "literatur",
            "film",
            "musik",
            "kunst",
            "ästhetik",
            "aesthetik",
            "öffentlichkeit",
            "oeffentlichkeit",
        ],
        section_hints: &["kultur", "medien"],
    },
    TopicRule {
        label: "Ungleichheit & Gesellschaft",
        keywords: &[
            "gesellschaft",
            "ungleichheit",
            "geschlecht",
            "gender",
            "gleichstellung",
            "frau",
            "frauen",
            "femin",
            "rassismus",
            "migration",
            "identität",
            "identitaet",
            "emotion",
            "gefühl",
            "gefuhl",
            "selbst",
            "alltag",
            "familie",
            "soziale",
            "soziales",
        ],
        section_hints: &["soziales leben"],
    },
    TopicRule {
        label: "Geschichte & Erinnerung",
        keywords: &[
            "geschichte",
            "histor",
            "erinner",
            "zeitgeschichte",
            "genealogie",
            "archiv",
            "jahrhundert",
        ],
        section_hints: &["zeitgeschichte"],
    },
    TopicRule {
        label: "Natur & Ökologie",
        keywords: &[
            "natur",
            "umwelt",
            "ökologie",
            "oekologie",
            "ökofemin",
            "oekofemin",
            "klima",
            "anthropozän",
            "anthropozan",
        ],
        section_hints: &["ökologie", "oekologie"],
    },
];

pub fn built_in_topic_labels() -> Vec<&'static str> {
    let mut labels = TOPIC_RULES
        .iter()
        .map(|rule| rule.label)
        .collect::<Vec<_>>();
    labels.push(FORMAT_TOPIC);
    labels.push(GENERAL_TOPIC);
    labels
}

pub fn generated_topic_from_fields(
    title: &str,
    subtitle: &str,
    section: &str,
    url: &str,
) -> String {
    let title_lower = title.to_lowercase();
    let subtitle_lower = subtitle.to_lowercase();
    let section_lower = section.to_lowercase();
    let url_lower = url.to_lowercase();

    let mut scored_topics = TOPIC_RULES
        .iter()
        .enumerate()
        .map(|(index, rule)| {
            let mut score = 0usize;
            for keyword in rule.keywords {
                if title_lower.contains(keyword) {
                    score += 5;
                }
                if subtitle_lower.contains(keyword) {
                    score += 4;
                }
                if section_lower.contains(keyword) {
                    score += 2;
                }
                if url_lower.contains(keyword) {
                    score += 1;
                }
            }
            for hint in rule.section_hints {
                if section_lower.contains(hint) {
                    score += 3;
                }
            }
            (index, rule.label, score)
        })
        .collect::<Vec<_>>();

    scored_topics.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    let best = scored_topics
        .first()
        .copied()
        .unwrap_or((usize::MAX, GENERAL_TOPIC, 0));

    if best.2 >= MIN_CONFIDENT_SCORE {
        return best.1.to_owned();
    }

    if is_format_section(&section_lower) {
        return FORMAT_TOPIC.to_owned();
    }

    if best.2 >= MIN_SECTION_FALLBACK_SCORE {
        return best.1.to_owned();
    }

    GENERAL_TOPIC.to_owned()
}

fn is_format_section(section_lower: &str) -> bool {
    [
        "essay",
        "interview",
        "besprech",
        "rezension",
        "zeitschriftenschau",
    ]
    .iter()
    .any(|needle| section_lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct TopicFixtureCase {
        title: String,
        subtitle: String,
        section: String,
        url: String,
        expected: String,
    }

    #[test]
    fn topic_cases_match_fixture_expectations() {
        let raw = include_str!("../tests/fixtures/topic_cases.json");
        let cases: Vec<TopicFixtureCase> =
            serde_json::from_str(raw).expect("topic fixture json should parse");

        for case in cases {
            let actual =
                generated_topic_from_fields(&case.title, &case.subtitle, &case.section, &case.url);
            assert_eq!(
                actual, case.expected,
                "topic mismatch for title '{}'",
                case.title
            );
        }
    }
}
