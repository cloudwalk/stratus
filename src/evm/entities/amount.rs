use ethabi::Token;
use primitive_types::U256;

#[derive(Debug, Clone, Default, derive_more::From)]
pub struct Amount(U256);

impl From<usize> for Amount {
    fn from(value: usize) -> Self {
        U256::from(value).into()
    }
}

impl From<Amount> for Token {
    fn from(value: Amount) -> Self {
        Token::Uint(value.0)
    }
}
