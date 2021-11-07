use std::marker::PhantomData;

use crate::mini_parser::input::{Find, SplitAt};
use crate::mini_parser::{parsers, Parser};

pub struct Alphanumeric0<I, C> {
    _phantom: PhantomData<fn(I, &mut C)>,
}

impl<I, C> Default for Alphanumeric0<I, C> {
    fn default() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<I, C> Parser<I, C> for Alphanumeric0<I, C>
where
    I: Find<Item = char> + SplitAt,
{
    type Output = I::Prefix;

    fn parse(&mut self, input: I, context: &mut C) -> Result<(Self::Output, I), ()> {
        parsers::take_while(|c: char| c.is_ascii_alphanumeric()).parse(input, context)
    }
}

pub fn alphanumeric0<I, C>(input: I, context: &mut C) -> Result<(I::Prefix, I), ()>
where
    I: Find<Item = char> + SplitAt,
{
    Alphanumeric0::default().parse(input, context)
}
