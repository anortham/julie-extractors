use super::Visibility;

/// Determines visibility from a slice of modifier strings using the standard default of `Visibility::Public`.
pub fn visibility_from_modifiers(modifiers: &[String]) -> Visibility {
    visibility_from_modifiers_with_default(modifiers, Visibility::Public)
}

/// Determines visibility from a slice of modifier strings with an explicit default fallback.
pub fn visibility_from_modifiers_with_default(
    modifiers: &[String],
    default: Visibility,
) -> Visibility {
    let has = |predicate: fn(&str) -> bool| modifiers.iter().any(|m| predicate(m.as_str()));

    if has(|m| m.eq_ignore_ascii_case("public")) {
        return Visibility::Public;
    }
    if has(|m| m.eq_ignore_ascii_case("fileprivate")) {
        return Visibility::FilePrivate;
    }
    if has(|m| {
        let lower = m.to_ascii_lowercase();
        lower == "private" || lower.starts_with("private[")
    }) {
        return Visibility::Private;
    }
    if has(|m| {
        let lower = m.to_ascii_lowercase();
        lower == "protected" || lower.starts_with("protected[")
    }) {
        return Visibility::Protected;
    }
    if has(|m| m.eq_ignore_ascii_case("internal")) {
        return Visibility::Internal;
    }
    if has(|m| m.eq_ignore_ascii_case("friend")) {
        return Visibility::Private;
    }

    default
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_strings(slice: &[&str]) -> Vec<String> {
        slice.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_visibility_from_modifiers_table() {
        let cases: Vec<(&[&str], Visibility)> = vec![
            // Standard lowercase modifiers
            (&["public"], Visibility::Public),
            (&["private"], Visibility::Private),
            (&["protected"], Visibility::Protected),
            (&["internal"], Visibility::Internal),
            (&["fileprivate"], Visibility::FilePrivate),
            (&["open"], Visibility::Public),
            (&["protected", "open"], Visibility::Protected),
            (&["friend"], Visibility::Private),
            // Titlecase / Pascalcase modifiers (VB.NET, etc.)
            (&["Public"], Visibility::Public),
            (&["Private"], Visibility::Private),
            (&["Protected"], Visibility::Protected),
            (&["Friend"], Visibility::Private),
            // Scala scoped visibility
            (&["private[this]"], Visibility::Private),
            (&["private[pkg]"], Visibility::Private),
            (&["protected[this]"], Visibility::Protected),
            (&["protected[pkg]"], Visibility::Protected),
            // Combined modifiers
            (&["public", "static"], Visibility::Public),
            (&["static", "final", "private"], Visibility::Private),
            (&["protected", "internal"], Visibility::Protected),
            (&["private", "protected"], Visibility::Private),
            (&["protected", "friend"], Visibility::Protected),
            // Modifiers with no visibility tokens fallback to default (Public)
            (&[], Visibility::Public),
            (&["static"], Visibility::Public),
            (&["async", "override"], Visibility::Public),
        ];

        for (modifiers, expected) in cases {
            let mod_strings = to_strings(modifiers);
            assert_eq!(
                visibility_from_modifiers(&mod_strings),
                expected,
                "Failed for modifiers: {:?}",
                modifiers
            );
        }
    }

    #[test]
    fn test_visibility_from_modifiers_with_default() {
        // Non-visibility modifier lists fallback to specified default
        assert_eq!(
            visibility_from_modifiers_with_default(&[], Visibility::Private),
            Visibility::Private
        );
        let non_vis = to_strings(&["static", "readonly"]);
        assert_eq!(
            visibility_from_modifiers_with_default(&non_vis, Visibility::Private),
            Visibility::Private
        );
        assert_eq!(
            visibility_from_modifiers_with_default(&[], Visibility::Public),
            Visibility::Public
        );

        // Explicit modifier overrides the default fallback
        let pub_mod = to_strings(&["public"]);
        assert_eq!(
            visibility_from_modifiers_with_default(&pub_mod, Visibility::Private),
            Visibility::Public
        );
        let priv_mod = to_strings(&["private"]);
        assert_eq!(
            visibility_from_modifiers_with_default(&priv_mod, Visibility::Public),
            Visibility::Private
        );
        let prot_mod = to_strings(&["protected"]);
        assert_eq!(
            visibility_from_modifiers_with_default(&prot_mod, Visibility::Private),
            Visibility::Protected
        );
    }

    #[test]
    fn test_per_language_default_rules() {
        struct LanguageDefaultCase {
            language: &'static str,
            default: Visibility,
            differ_from_shared: bool,
        }

        let per_language_table = [
            LanguageDefaultCase {
                language: "java",
                default: Visibility::Private,
                differ_from_shared: true,
            },
            LanguageDefaultCase {
                language: "csharp",
                default: Visibility::Private,
                differ_from_shared: true,
            },
            LanguageDefaultCase {
                language: "kotlin",
                default: Visibility::Public,
                differ_from_shared: false,
            },
            LanguageDefaultCase {
                language: "php",
                default: Visibility::Public,
                differ_from_shared: false,
            },
            LanguageDefaultCase {
                language: "razor",
                default: Visibility::Public,
                differ_from_shared: false,
            },
            LanguageDefaultCase {
                language: "scala",
                default: Visibility::Public,
                differ_from_shared: false,
            },
            LanguageDefaultCase {
                language: "swift",
                default: Visibility::Public,
                differ_from_shared: false,
            },
        ];

        let shared_default = Visibility::Public;

        for case in per_language_table {
            if case.differ_from_shared {
                assert_ne!(
                    case.default, shared_default,
                    "Expected {} default to differ from shared default",
                    case.language
                );
            } else {
                assert_eq!(
                    case.default, shared_default,
                    "Expected {} default to match shared default",
                    case.language
                );
            }
        }
    }
}
