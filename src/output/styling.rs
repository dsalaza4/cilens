use console::style;

/// Styling helpers for terminal output
pub fn bright_yellow(text: impl std::fmt::Display) -> console::StyledObject<String> {
    style(text.to_string()).bright().yellow()
}

pub fn bright_green(text: impl std::fmt::Display) -> console::StyledObject<String> {
    style(text.to_string()).bright().green()
}

pub fn cyan(text: impl std::fmt::Display) -> console::StyledObject<String> {
    style(text.to_string()).cyan()
}

pub fn dim(text: impl std::fmt::Display) -> console::StyledObject<String> {
    style(text.to_string()).dim()
}

pub fn bright(text: impl std::fmt::Display) -> console::StyledObject<String> {
    style(text.to_string()).bright()
}

pub fn magenta_bold(text: impl std::fmt::Display) -> console::StyledObject<String> {
    style(text.to_string()).magenta().bold()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bright_yellow() {
        let styled = bright_yellow("test");
        assert!(styled.to_string().contains("test"));
    }

    #[test]
    fn test_bright_yellow_with_number() {
        let styled = bright_yellow(42);
        assert!(styled.to_string().contains("42"));
    }

    #[test]
    fn test_bright_green() {
        let styled = bright_green("success");
        assert!(styled.to_string().contains("success"));
    }

    #[test]
    fn test_cyan() {
        let styled = cyan("info");
        assert!(styled.to_string().contains("info"));
    }

    #[test]
    fn test_dim() {
        let styled = dim("subtle");
        assert!(styled.to_string().contains("subtle"));
    }

    #[test]
    fn test_bright() {
        let styled = bright("highlight");
        assert!(styled.to_string().contains("highlight"));
    }

    #[test]
    fn test_magenta_bold() {
        let styled = magenta_bold("important");
        assert!(styled.to_string().contains("important"));
    }

    #[test]
    fn test_styling_with_empty_string() {
        let _ = cyan("").to_string();
    }

    #[test]
    fn test_styling_with_unicode() {
        let styled = bright("✨ emoji ✨");
        assert!(styled.to_string().contains("✨"));
    }
}
