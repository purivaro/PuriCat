//! Identifier quoting. Values always travel as bind parameters; identifiers cannot,
//! so every name that reaches SQL text goes through one of these functions.

use crate::{Error, Result};

fn check(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::Invalid("Empty identifier".into()));
    }
    if name.contains('\0') {
        return Err(Error::Invalid("Identifier contains a NUL byte".into()));
    }
    Ok(())
}

/// MySQL: `` `name` `` with backticks doubled.
pub fn mysql(name: &str) -> Result<String> {
    check(name)?;
    Ok(format!("`{}`", name.replace('`', "``")))
}

/// PostgreSQL: `"name"` with double quotes doubled.
pub fn postgres(name: &str) -> Result<String> {
    check(name)?;
    Ok(format!("\"{}\"", name.replace('"', "\"\"")))
}

/// SQL Server: `[name]` with closing brackets doubled.
pub fn sqlserver(name: &str) -> Result<String> {
    check(name)?;
    Ok(format!("[{}]", name.replace(']', "]]")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_plain_and_unicode_names() {
        assert_eq!(mysql("users").unwrap(), "`users`");
        assert_eq!(postgres("ผู้ใช้").unwrap(), "\"ผู้ใช้\"");
        assert_eq!(sqlserver("Order Details").unwrap(), "[Order Details]");
    }

    #[test]
    fn escapes_the_quote_character_itself() {
        assert_eq!(
            mysql("a`; DROP TABLE x; --").unwrap(),
            "`a``; DROP TABLE x; --`"
        );
        assert_eq!(postgres("a\"b").unwrap(), "\"a\"\"b\"");
        assert_eq!(sqlserver("a]b").unwrap(), "[a]]b]");
        // The other engines' quote characters are ordinary characters.
        assert_eq!(mysql("a\"b]").unwrap(), "`a\"b]`");
    }

    #[test]
    fn rejects_empty_and_nul() {
        assert!(mysql("").is_err());
        assert!(postgres("a\0b").is_err());
        assert!(sqlserver("").is_err());
    }
}
