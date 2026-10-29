//! Lightweight TeX-to-Unicode projection for inline formulas.
//!
//! This is not a full LaTeX engine. It keeps the formula readable in plain
//! text by mapping common commands, fractions, roots and super/subscripts.

pub fn format_math(tex: &str) -> String {
    let chars: Vec<char> = tex.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '\\' => {
                index += 1;
                let Some(&next) = chars.get(index) else {
                    break;
                };
                if next == '\\' {
                    out.push(' ');
                    index += 1;
                    continue;
                }
                if !next.is_ascii_alphabetic() {
                    match next {
                        ',' | ';' | ':' => out.push(' '),
                        '!' => {}
                        _ => out.push(next),
                    }
                    index += 1;
                    continue;
                }
                let start = index;
                while index < chars.len() && chars[index].is_ascii_alphabetic() {
                    index += 1;
                }
                let command: String = chars[start..index].iter().collect();
                match command.as_str() {
                    "frac" => {
                        let (numerator, after_num) = take_group(&chars, index);
                        let (denominator, after_den) = take_group(&chars, after_num);
                        index = after_den;
                        let numerator = format_math(&numerator);
                        let denominator = format_math(&denominator);
                        out.push_str(&wrap_math(&numerator));
                        out.push('/');
                        out.push_str(&wrap_math(&denominator));
                    }
                    "sqrt" => {
                        let (body, after) = take_group(&chars, index);
                        index = after;
                        out.push('√');
                        out.push_str(&wrap_math(&format_math(&body)));
                    }
                    "text" | "mathrm" | "mathbf" | "mathit" | "operatorname" | "mbox" => {
                        let (body, after) = take_group(&chars, index);
                        index = after;
                        out.push_str(&format_math(&body));
                    }
                    _ => match math_symbol(&command) {
                        Some(symbol) => out.push_str(symbol),
                        None => out.push_str(&command),
                    },
                }
            }
            '^' => {
                let (body, after) = take_group(&chars, index + 1);
                index = after;
                out.push_str(&script(&format_math(&body), true));
            }
            '_' => {
                let (body, after) = take_group(&chars, index + 1);
                index = after;
                out.push_str(&script(&format_math(&body), false));
            }
            '{' | '}' => index += 1,
            ch => {
                out.push(ch);
                index += 1;
            }
        }
    }
    out.trim().to_string()
}

fn take_group(chars: &[char], start: usize) -> (String, usize) {
    if chars.get(start) != Some(&'{') {
        return (
            chars
                .get(start)
                .map(|ch| ch.to_string())
                .unwrap_or_default(),
            (start + 1).min(chars.len()),
        );
    }
    let mut depth = 0usize;
    let mut end = start;
    for (offset, ch) in chars[start..].iter().enumerate() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + offset;
                    break;
                }
            }
            _ => {}
        }
    }
    (chars[start + 1..end].iter().collect(), end + 1)
}

fn wrap_math(value: &str) -> String {
    if value
        .chars()
        .any(|ch| matches!(ch, '+' | '-' | ' ' | '=' | '/'))
    {
        format!("({value})")
    } else {
        value.to_string()
    }
}

fn script(value: &str, superscript: bool) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        let mapped = if superscript {
            match ch {
                '0' => Some('⁰'),
                '1' => Some('¹'),
                '2' => Some('²'),
                '3' => Some('³'),
                '4' => Some('⁴'),
                '5' => Some('⁵'),
                '6' => Some('⁶'),
                '7' => Some('⁷'),
                '8' => Some('⁸'),
                '9' => Some('⁹'),
                '+' => Some('⁺'),
                '-' => Some('⁻'),
                '=' => Some('⁼'),
                '(' => Some('⁽'),
                ')' => Some('⁾'),
                'a' => Some('ᵃ'),
                'b' => Some('ᵇ'),
                'c' => Some('ᶜ'),
                'd' => Some('ᵈ'),
                'e' => Some('ᵉ'),
                'f' => Some('ᶠ'),
                'g' => Some('ᵍ'),
                'h' => Some('ʰ'),
                'i' => Some('ⁱ'),
                'j' => Some('ʲ'),
                'k' => Some('ᵏ'),
                'l' => Some('ˡ'),
                'm' => Some('ᵐ'),
                'n' => Some('ⁿ'),
                'o' => Some('ᵒ'),
                'p' => Some('ᵖ'),
                'r' => Some('ʳ'),
                's' => Some('ˢ'),
                't' => Some('ᵗ'),
                'u' => Some('ᵘ'),
                'v' => Some('ᵛ'),
                'w' => Some('ʷ'),
                'x' => Some('ˣ'),
                'y' => Some('ʸ'),
                'z' => Some('ᶻ'),
                _ => None,
            }
        } else {
            match ch {
                '0' => Some('₀'),
                '1' => Some('₁'),
                '2' => Some('₂'),
                '3' => Some('₃'),
                '4' => Some('₄'),
                '5' => Some('₅'),
                '6' => Some('₆'),
                '7' => Some('₇'),
                '8' => Some('₈'),
                '9' => Some('₉'),
                '+' => Some('₊'),
                '-' => Some('₋'),
                '=' => Some('₌'),
                '(' => Some('₍'),
                ')' => Some('₎'),
                'a' => Some('ₐ'),
                'e' => Some('ₑ'),
                'h' => Some('ₕ'),
                'i' => Some('ᵢ'),
                'j' => Some('ⱼ'),
                'k' => Some('ₖ'),
                'l' => Some('ₗ'),
                'm' => Some('ₘ'),
                'n' => Some('ₙ'),
                'o' => Some('ₒ'),
                'p' => Some('ₚ'),
                'r' => Some('ᵣ'),
                's' => Some('ₛ'),
                't' => Some('ₜ'),
                'u' => Some('ᵤ'),
                'v' => Some('ᵥ'),
                'x' => Some('ₓ'),
                _ => None,
            }
        };
        match mapped {
            Some(mapped) => out.push(mapped),
            None => {
                return if superscript {
                    format!("^{value}")
                } else {
                    format!("_{value}")
                }
            }
        }
    }
    out
}

fn math_symbol(command: &str) -> Option<&'static str> {
    Some(match command {
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" | "varepsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "pi" => "π",
        "rho" => "ρ",
        "sigma" => "σ",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        "infty" => "∞",
        "pm" => "±",
        "mp" => "∓",
        "times" => "×",
        "div" => "÷",
        "cdot" => "·",
        "leq" | "le" => "≤",
        "geq" | "ge" => "≥",
        "neq" | "ne" => "≠",
        "approx" => "≈",
        "equiv" => "≡",
        "to" | "rightarrow" => "→",
        "leftarrow" => "←",
        "Rightarrow" => "⇒",
        "Leftarrow" => "⇐",
        "leftrightarrow" => "↔",
        "sum" => "∑",
        "prod" => "∏",
        "int" => "∫",
        "partial" => "∂",
        "nabla" => "∇",
        "in" => "∈",
        "notin" => "∉",
        "subset" => "⊂",
        "subseteq" => "⊆",
        "supset" => "⊃",
        "cup" => "∪",
        "cap" => "∩",
        "forall" => "∀",
        "exists" => "∃",
        "neg" => "¬",
        "angle" => "∠",
        "perp" => "⊥",
        "parallel" => "∥",
        "degree" => "°",
        "left" | "right" | "quad" | "qquad" => "",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_commands() {
        assert_eq!(format_math(r"\alpha + \beta \leq \infty"), "α + β ≤ ∞");
        assert_eq!(format_math(r"\frac{1}{2}"), "1/2");
        assert_eq!(format_math(r"\frac{x+1}{2}"), "(x+1)/2");
        assert_eq!(format_math(r"\sqrt{2}"), "√2");
        assert_eq!(format_math(r"x^{2} + y_{1}"), "x² + y₁");
        assert_eq!(format_math(r"\text{速度} = \frac{d}{t}"), "速度 = d/t");
    }

    #[test]
    fn keeps_unknown_commands_readable() {
        assert_eq!(format_math(r"\unknown + 1"), "unknown + 1");
        assert_eq!(format_math(r"a^{?}"), "a^?");
        assert_eq!(format_math(r"a_{?}"), "a_?");
    }
}
