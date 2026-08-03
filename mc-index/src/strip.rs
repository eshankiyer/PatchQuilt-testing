/// Replaces the contents of comments and string/char literals with spaces.
///
/// Preserves line breaks and overall byte length so downstream brace-depth tracking and regex
/// matching never see tokens that only exist inside a comment or a literal.
#[must_use]
pub fn strip_comments_and_literals(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    out.push(if bytes[i] == b'\n' { '\n' } else { ' ' });
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                out.push(' ');
                out.push(' ');
                i += 2;
                while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                    out.push(if bytes[i] == b'\n' { '\n' } else { ' ' });
                    i += 1;
                }
                if i < bytes.len() {
                    out.push(' ');
                    out.push(' ');
                    i += 2;
                }
            }
            b'"' if bytes.get(i + 1) == Some(&b'"') && bytes.get(i + 2) == Some(&b'"') => {
                out.push_str("   ");
                i += 3;
                while i < bytes.len()
                    && !(bytes[i] == b'"'
                        && bytes.get(i + 1) == Some(&b'"')
                        && bytes.get(i + 2) == Some(&b'"'))
                {
                    out.push(if bytes[i] == b'\n' { '\n' } else { ' ' });
                    i += 1;
                }
                if i < bytes.len() {
                    out.push_str("   ");
                    i += 3;
                }
            }
            b'"' => {
                out.push(' ');
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        out.push(' ');
                        out.push(' ');
                        i += 2;
                        continue;
                    }
                    out.push(if bytes[i] == b'\n' { '\n' } else { ' ' });
                    i += 1;
                }
                if i < bytes.len() {
                    out.push(' ');
                    i += 1;
                }
            }
            b'\'' => {
                out.push(' ');
                i += 1;
                while i < bytes.len() && bytes[i] != b'\'' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        out.push(' ');
                        out.push(' ');
                        i += 2;
                        continue;
                    }
                    out.push(' ');
                    i += 1;
                }
                if i < bytes.len() {
                    out.push(' ');
                    i += 1;
                }
            }
            _ => {
                out.push(b as char);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::strip_comments_and_literals;

    #[test]
    fn line_comment_blanked() {
        let out = strip_comments_and_literals("int a; // { not real\nint b;");
        assert!(!out.contains('{'));
        assert!(out.contains("int a;"));
        assert!(out.contains("int b;"));
    }

    #[test]
    fn block_comment_blanked_across_lines() {
        let out = strip_comments_and_literals("/* { \n } */int a;");
        assert!(!out.contains('{'));
        assert!(!out.contains('}'));
        assert!(out.contains("int a;"));
    }

    #[test]
    fn string_literal_blanked() {
        let out = strip_comments_and_literals(r#"String s = "{ not brace }";"#);
        assert!(!out.contains('{'));
        assert!(!out.contains('}'));
    }

    #[test]
    fn escaped_quote_inside_string_handled() {
        let out = strip_comments_and_literals(r#"String s = "a\"{b";"#);
        assert!(!out.contains('{'));
    }

    #[test]
    fn char_literal_blanked() {
        let out = strip_comments_and_literals("char c = '{';");
        assert!(!out.contains('{'));
    }

    #[test]
    fn preserves_length_per_line_for_non_comment_code() {
        let src = "class Foo {\n  int x;\n}";
        let out = strip_comments_and_literals(src);
        assert_eq!(src.lines().count(), out.lines().count());
    }
}
