pub struct Token(String);

impl From<&str> for Token {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl Token {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<redacted>")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_from_str_creates_token() {
        let token_str = "ghp_1234567890abcdefghijklmnopqrstuvwxyz";
        let token = Token::from(token_str);

        assert_eq!(token.as_str(), token_str);
    }

    #[test]
    fn test_token_from_empty_string() {
        let token = Token::from("");

        assert_eq!(token.as_str(), "");
    }

    #[test]
    fn test_token_from_str_with_special_characters() {
        let token_str = "tok_!@#$%^&*()_+-=[]{}|;:',.<>?/~`";
        let token = Token::from(token_str);

        assert_eq!(token.as_str(), token_str);
    }

    #[test]
    fn test_token_from_str_with_unicode() {
        let token_str = "token_with_unicode_🔐_🚀";
        let token = Token::from(token_str);

        assert_eq!(token.as_str(), token_str);
    }

    #[test]
    fn test_token_from_str_with_whitespace() {
        let token_str = "token with spaces\tand\ttabs\nand\nnewlines";
        let token = Token::from(token_str);

        assert_eq!(token.as_str(), token_str);
    }

    #[test]
    fn test_token_from_long_string() {
        let token_str = "a".repeat(10000);
        let token = Token::from(token_str.as_str());

        assert_eq!(token.as_str(), token_str);
        assert_eq!(token.as_str().len(), 10000);
    }

    #[test]
    fn test_token_as_str_returns_reference() {
        let original = "test_token_123";
        let token = Token::from(original);
        let retrieved = token.as_str();

        // Verify it's the same content
        assert_eq!(retrieved, original);

        // Verify we can get multiple references
        let retrieved2 = token.as_str();
        assert_eq!(retrieved, retrieved2);
    }

    #[test]
    fn test_token_debug_redacts_value() {
        let sensitive_token = "ghp_very_secret_token_do_not_log";
        let token = Token::from(sensitive_token);

        let debug_output = format!("{token:?}");

        // Ensure the actual token value is not in the debug output
        assert_eq!(debug_output, "<redacted>");
        assert!(!debug_output.contains(sensitive_token));
        assert!(!debug_output.contains("ghp_"));
        assert!(!debug_output.contains("secret"));
    }

    #[test]
    fn test_token_debug_does_not_expose_empty_token() {
        let token = Token::from("");
        let debug_output = format!("{token:?}");

        assert_eq!(debug_output, "<redacted>");
    }

    #[test]
    fn test_multiple_tokens_are_independent() {
        let token1 = Token::from("token_one");
        let token2 = Token::from("token_two");

        assert_eq!(token1.as_str(), "token_one");
        assert_eq!(token2.as_str(), "token_two");
        assert_ne!(token1.as_str(), token2.as_str());
    }

    #[test]
    fn test_token_owns_its_string() {
        let token = {
            let temp_string = String::from("temporary_token");
            Token::from(temp_string.as_str())
            // temp_string goes out of scope here
        };

        // Token should still be valid because it owns a copy
        assert_eq!(token.as_str(), "temporary_token");
    }

    #[test]
    fn test_token_from_string_literal() {
        let token = Token::from("literal_token");
        assert_eq!(token.as_str(), "literal_token");
    }

    #[test]
    fn test_token_from_borrowed_string() {
        let owned_string = String::from("owned_token");
        let token = Token::from(owned_string.as_str());

        assert_eq!(token.as_str(), "owned_token");
        // Original string should still be usable
        assert_eq!(owned_string, "owned_token");
    }

    #[test]
    fn test_real_world_github_token_format() {
        let github_token = "ghp_16C7e42F292c6912E7710c838347Ae178B4a";
        let token = Token::from(github_token);

        assert_eq!(token.as_str(), github_token);
        assert_eq!(format!("{token:?}"), "<redacted>");
    }

    #[test]
    fn test_real_world_gitlab_token_format() {
        let gitlab_token = "glpat-xxxxxxxxxxxxxxxxxxxx";
        let token = Token::from(gitlab_token);

        assert_eq!(token.as_str(), gitlab_token);
        assert_eq!(format!("{token:?}"), "<redacted>");
    }

    #[test]
    fn test_real_world_generic_bearer_token() {
        let bearer_token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let token = Token::from(bearer_token);

        assert_eq!(token.as_str(), bearer_token);
        assert_eq!(format!("{token:?}"), "<redacted>");
    }

    #[test]
    fn test_token_can_be_used_in_vec() {
        let tokens = [
            Token::from("token1"),
            Token::from("token2"),
            Token::from("token3"),
        ];

        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].as_str(), "token1");
        assert_eq!(tokens[1].as_str(), "token2");
        assert_eq!(tokens[2].as_str(), "token3");
    }

    #[test]
    fn test_token_debug_in_struct() {
        #[derive(Debug)]
        #[allow(dead_code)]
        struct ApiClient {
            token: Token,
            endpoint: String,
        }

        let client = ApiClient {
            token: Token::from("super_secret_token"),
            endpoint: String::from("https://api.example.com"),
        };

        let debug_output = format!("{client:?}");

        // Ensure the token is redacted in the struct's debug output
        assert!(debug_output.contains("<redacted>"));
        assert!(!debug_output.contains("super_secret_token"));
        assert!(debug_output.contains("https://api.example.com"));
    }

    #[test]
    fn test_token_as_str_lifetime_bound_to_token() {
        let token = Token::from("test_token");
        let str_ref = token.as_str();

        // This verifies that the string reference is valid as long as token is valid
        assert_eq!(str_ref.len(), 10);
        assert_eq!(str_ref, "test_token");
    }
}
