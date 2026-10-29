use crate::ports::TokenSource;
use crate::{Result, Token};

/// [`TokenSource`] with a fixed answer.
#[derive(Clone, Debug, Default)]
pub struct FakeToken(pub Option<String>);

impl TokenSource for FakeToken {
    fn token(&self) -> Result<Option<Token>> {
        self.0.as_deref().map(Token::new).transpose()
    }

    fn describe(&self) -> String {
        "fake token".into()
    }
}
