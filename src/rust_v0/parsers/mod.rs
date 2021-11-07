use crate::mini_parser::combinators::{
    alt, delimited, many0, map_opt, map_opt_with_context, opt, preceded, terminated, tuple,
};
use crate::mini_parser::input::{Find, SplitAt, StripPrefix};
use crate::mini_parser::parsers::{alphanumeric0, digit1, lower_hex_digit0, tag, take};
use crate::mini_parser::Parser;
use crate::rust_v0::{
    Abi, BasicType, Const, ConstFields, ConstStr, DynBounds, DynTrait, DynTraitAssocBinding, FnSig, GenericArg,
    Identifier, ImplPath, Path, Symbol, Type,
};
use num_traits::{CheckedNeg, PrimInt};
use std::borrow::Cow;
use std::collections::HashMap;
use std::convert::TryInto;
use std::rc::Rc;
use std::str;

#[cfg(test)]
mod tests;

#[derive(Default)]
struct Context<'a> {
    paths: HashMap<usize, Rc<Path<'a>>>,
    types: HashMap<usize, Rc<Type<'a>>>,
    consts: HashMap<usize, Rc<Const<'a>>>,
}

#[derive(Clone)]
struct IndexedStr<'a> {
    index: usize,
    data: &'a str,
}

impl<'a> IndexedStr<'a> {
    fn new(data: &'a str) -> Self {
        Self { index: 0, data }
    }
}

impl Find for IndexedStr<'_> {
    type Item = char;

    fn find(&self, pattern: impl FnMut(Self::Item) -> bool) -> usize {
        self.data.find(pattern).unwrap_or_else(|| self.data.len())
    }
}

impl<'a> SplitAt for IndexedStr<'a> {
    type Prefix = &'a str;

    fn split_at(self, index: usize) -> Option<(Self::Prefix, Self)> {
        let (left, right) = SplitAt::split_at(self.data, index)?;

        Some((
            left,
            Self {
                index: self.index + left.len(),
                data: right,
            },
        ))
    }
}

impl<'a, 'b> StripPrefix<&'b str> for IndexedStr<'a> {
    type Prefix = &'a str;

    fn strip_prefix(self, prefix: &'b str) -> Option<(Self::Prefix, Self)> {
        let (left, right) = StripPrefix::strip_prefix(self.data, prefix)?;

        Some((
            left,
            Self {
                index: self.index + left.len(),
                data: right,
            },
        ))
    }
}

fn opt_u64<I, C>(parser: impl Parser<I, C, Output = u64>) -> impl Parser<I, C, Output = u64>
where
    I: Clone,
{
    parser.opt().map_opt(|num| {
        Some(match num {
            None => 0,
            Some(num) => num.checked_add(1)?,
        })
    })
}

// References:
//
// - <https://github.com/rust-lang/rustc-demangle/blob/main/src/v0.rs>.
// - <https://github.com/michaelwoerister/std-mangle-rs/blob/master/src/ast_demangle.rs>.
// - <https://github.com/rust-lang/rust/blob/master/compiler/rustc_symbol_mangling/src/v0.rs>.
// - <https://rust-lang.github.io/rfcs/2603-rust-symbol-name-mangling-v0.html>.

pub fn parse_symbol(input: &str) -> Result<(Symbol, &str), ()> {
    parse_symbol_inner(IndexedStr::new(input), &mut Context::default()).map(|(symbol, suffix)| (symbol, suffix.data))
}

fn parse_symbol_inner<'a>(
    input: IndexedStr<'a>,
    context: &mut Context<'a>,
) -> Result<(Symbol<'a>, IndexedStr<'a>), ()> {
    tuple((opt(parse_decimal_number), parse_path, opt(parse_path)))
        .map(|(version, path, instantiating_crate)| Symbol {
            version,
            path,
            instantiating_crate,
        })
        .parse(input, context)
}

fn parse_path<'a>(input: IndexedStr<'a>, context: &mut Context<'a>) -> Result<(Rc<Path<'a>>, IndexedStr<'a>), ()> {
    let index = input.index;

    alt((
        preceded(tag("C"), parse_identifier).map(Path::CrateRoot),
        preceded(tag("M"), parse_impl_path.and(parse_type))
            .map(|(impl_path, type_)| Path::InherentImpl { impl_path, type_ }),
        preceded(tag("X"), tuple((parse_impl_path, parse_type, parse_path))).map(|(impl_path, type_, trait_)| {
            Path::TraitImpl {
                impl_path,
                type_,
                trait_,
            }
        }),
        preceded(tag("Y"), parse_type.and(parse_path)).map(|(type_, trait_)| Path::TraitDefinition { type_, trait_ }),
        preceded(tag("N"), tuple((take(1_usize), parse_path, parse_identifier))).map(|(namespace, path, name)| {
            Path::Nested {
                namespace: namespace.as_bytes()[0],
                path,
                name,
            }
        }),
        delimited(tag("I"), parse_path.and(many0(parse_generic_arg)), tag("E"))
            .map(|(path, generic_args)| Path::Generic { path, generic_args }),
    ))
    .map(Rc::new)
    .or(map_opt_with_context(parse_back_ref, |back_ref, context| {
        context.paths.get(&back_ref).cloned()
    }))
    .inspect_with_context(|result, context| {
        context.paths.insert(index, Rc::clone(result));
    })
    .parse(input, context)
}

fn parse_impl_path<'a>(input: IndexedStr<'a>, context: &mut Context<'a>) -> Result<(ImplPath<'a>, IndexedStr<'a>), ()> {
    opt_u64(parse_disambiguator)
        .and(parse_path)
        .map(|(disambiguator, path)| ImplPath { disambiguator, path })
        .parse(input, context)
}

fn parse_identifier<'a>(
    input: IndexedStr<'a>,
    context: &mut Context<'a>,
) -> Result<(Identifier<'a>, IndexedStr<'a>), ()> {
    opt_u64(parse_disambiguator)
        .and(parse_undisambiguated_identifier)
        .map(|(disambiguator, name)| Identifier { disambiguator, name })
        .parse(input, context)
}

fn parse_disambiguator<'a>(input: IndexedStr<'a>, context: &mut Context<'a>) -> Result<(u64, IndexedStr<'a>), ()> {
    preceded(tag("s"), parse_base62_number).parse(input, context)
}

fn parse_undisambiguated_identifier<'a>(
    input: IndexedStr<'a>,
    context: &mut Context<'a>,
) -> Result<(Cow<'a, str>, IndexedStr<'a>), ()> {
    tuple((
        opt(tag("u")).map(|tag| tag.is_some()),
        parse_decimal_number,
        opt(tag("_")),
    ))
    .flat_map(|(is_punycode, length, _)| {
        map_opt(take(length), move |name: &str| {
            Some(if is_punycode {
                let mut buffer = name.as_bytes().to_vec();

                if let Some(c) = buffer.iter_mut().rfind(|&&mut c| c == b'_') {
                    *c = b'-';
                }

                Cow::Owned(punycode::decode(str::from_utf8(&buffer).ok()?).ok()?)
            } else {
                Cow::Borrowed(name)
            })
        })
    })
    .parse(input, context)
}

fn parse_generic_arg<'a>(
    input: IndexedStr<'a>,
    context: &mut Context<'a>,
) -> Result<(GenericArg<'a>, IndexedStr<'a>), ()> {
    alt((
        parse_lifetime.map(GenericArg::Lifetime),
        parse_type.map(GenericArg::Type),
        preceded(tag("K"), parse_const).map(GenericArg::Const),
    ))
    .parse(input, context)
}

fn parse_lifetime<'a>(input: IndexedStr<'a>, context: &mut Context<'a>) -> Result<(u64, IndexedStr<'a>), ()> {
    preceded(tag("L"), parse_base62_number).parse(input, context)
}

fn parse_binder<'a>(input: IndexedStr<'a>, context: &mut Context<'a>) -> Result<(u64, IndexedStr<'a>), ()> {
    preceded(tag("G"), parse_base62_number).parse(input, context)
}

fn parse_type<'a>(input: IndexedStr<'a>, context: &mut Context<'a>) -> Result<(Rc<Type<'a>>, IndexedStr<'a>), ()> {
    let index = input.index;

    alt((
        parse_basic_type.map(Type::Basic),
        parse_path.map(Type::Named),
        preceded(tag("A"), parse_type.and(parse_const)).map(|(type_, length)| Type::Array(type_, length)),
        preceded(tag("S"), parse_type).map(Type::Slice),
        delimited(tag("T"), many0(parse_type), tag("E")).map(Type::Tuple),
        preceded(
            tag("R"),
            opt(parse_lifetime).map(Option::unwrap_or_default).and(parse_type),
        )
        .map(|(lifetime, type_)| Type::Ref { lifetime, type_ }),
        preceded(
            tag("Q"),
            opt(parse_lifetime).map(Option::unwrap_or_default).and(parse_type),
        )
        .map(|(lifetime, type_)| Type::RefMut { lifetime, type_ }),
        preceded(tag("P"), parse_type).map(Type::PtrConst),
        preceded(tag("O"), parse_type).map(Type::PtrMut),
        preceded(tag("F"), parse_fn_sig).map(Type::Fn),
        preceded(tag("D"), parse_dyn_bounds.and(parse_lifetime))
            .map(|(dyn_bounds, lifetime)| Type::DynTrait { dyn_bounds, lifetime }),
    ))
    .map(Rc::new)
    .or(map_opt_with_context(parse_back_ref, |back_ref, context| {
        context.types.get(&back_ref).cloned()
    }))
    .inspect_with_context(|result, context| {
        context.types.insert(index, Rc::clone(result));
    })
    .parse(input, context)
}

fn parse_basic_type<'a>(input: IndexedStr<'a>, context: &mut Context<'a>) -> Result<(BasicType, IndexedStr<'a>), ()> {
    map_opt(take(1_usize), |s: &str| match s.as_bytes()[0] {
        b'a' => Some(BasicType::I8),
        b'b' => Some(BasicType::Bool),
        b'c' => Some(BasicType::Char),
        b'd' => Some(BasicType::F64),
        b'e' => Some(BasicType::Str),
        b'f' => Some(BasicType::F32),
        b'h' => Some(BasicType::U8),
        b'i' => Some(BasicType::Isize),
        b'j' => Some(BasicType::Usize),
        b'l' => Some(BasicType::I32),
        b'm' => Some(BasicType::U32),
        b'n' => Some(BasicType::I128),
        b'o' => Some(BasicType::U128),
        b's' => Some(BasicType::I16),
        b't' => Some(BasicType::U16),
        b'u' => Some(BasicType::Unit),
        b'v' => Some(BasicType::Ellipsis),
        b'x' => Some(BasicType::I64),
        b'y' => Some(BasicType::U64),
        b'z' => Some(BasicType::Never),
        b'p' => Some(BasicType::Placeholder),
        _ => None,
    })
    .parse(input, context)
}

fn parse_fn_sig<'a>(input: IndexedStr<'a>, context: &mut Context<'a>) -> Result<(FnSig<'a>, IndexedStr<'a>), ()> {
    tuple((
        opt_u64(parse_binder),
        opt(tag("U")).map(|u| u.is_some()),
        opt(preceded(tag("K"), parse_abi)),
        terminated(many0(parse_type), tag("E")),
        parse_type,
    ))
    .map(|(bound_lifetimes, is_unsafe, abi, argument_types, return_type)| FnSig {
        bound_lifetimes,
        is_unsafe,
        abi,
        argument_types,
        return_type,
    })
    .parse(input, context)
}

fn parse_abi<'a>(input: IndexedStr<'a>, context: &mut Context<'a>) -> Result<(Abi<'a>, IndexedStr<'a>), ()> {
    tag("C")
        .map(|_| Abi::C)
        .or(parse_undisambiguated_identifier.map(Abi::Named))
        .parse(input, context)
}

fn parse_dyn_bounds<'a>(
    input: IndexedStr<'a>,
    context: &mut Context<'a>,
) -> Result<(DynBounds<'a>, IndexedStr<'a>), ()> {
    opt_u64(parse_binder)
        .and(terminated(many0(parse_dyn_trait), tag("E")))
        .map(|(bound_lifetimes, dyn_traits)| DynBounds {
            bound_lifetimes,
            dyn_traits,
        })
        .parse(input, context)
}

fn parse_dyn_trait<'a>(input: IndexedStr<'a>, context: &mut Context<'a>) -> Result<(DynTrait<'a>, IndexedStr<'a>), ()> {
    parse_path
        .and(many0(parse_dyn_trait_assoc_binding))
        .map(|(path, dyn_trait_assoc_bindings)| DynTrait {
            path,
            dyn_trait_assoc_bindings,
        })
        .parse(input, context)
}

fn parse_dyn_trait_assoc_binding<'a>(
    input: IndexedStr<'a>,
    context: &mut Context<'a>,
) -> Result<(DynTraitAssocBinding<'a>, IndexedStr<'a>), ()> {
    preceded(tag("p"), parse_undisambiguated_identifier.and(parse_type))
        .map(|(name, type_)| DynTraitAssocBinding { name, type_ })
        .parse(input, context)
}

fn parse_const<'a>(input: IndexedStr<'a>, context: &mut Context<'a>) -> Result<(Rc<Const<'a>>, IndexedStr<'a>), ()> {
    let index = input.index;

    alt((
        preceded(tag("a"), parse_const_int.map(Const::I8)),
        preceded(tag("h"), parse_const_int.map(Const::U8)),
        preceded(tag("i"), parse_const_int.map(Const::Isize)),
        preceded(tag("j"), parse_const_int.map(Const::Usize)),
        preceded(tag("l"), parse_const_int.map(Const::I32)),
        preceded(tag("m"), parse_const_int.map(Const::U32)),
        preceded(tag("n"), parse_const_int.map(Const::I128)),
        preceded(tag("o"), parse_const_int.map(Const::U128)),
        preceded(tag("s"), parse_const_int.map(Const::I16)),
        preceded(tag("t"), parse_const_int.map(Const::U16)),
        preceded(tag("x"), parse_const_int.map(Const::I64)),
        preceded(tag("y"), parse_const_int.map(Const::U64)),
        preceded(
            tag("b"),
            map_opt(parse_const_int::<u8>, |result| match result {
                0 => Some(Const::Bool(false)),
                1 => Some(Const::Bool(true)),
                _ => None,
            }),
        ),
        preceded(
            tag("c"),
            map_opt(parse_const_int::<u32>, |result| result.try_into().ok().map(Const::Char)),
        ),
        preceded(tag("e"), parse_const_str.map(Const::Str)),
        preceded(tag("R"), parse_const.map(Const::Ref)),
        preceded(tag("Q"), parse_const.map(Const::RefMut)),
        delimited(tag("A"), many0(parse_const), tag("E")).map(Const::Array),
        delimited(tag("T"), many0(parse_const), tag("E")).map(Const::Tuple),
        preceded(tag("V"), parse_path.and(parse_const_fields))
            .map(|(path, fields)| Const::NamedStruct { path, fields }),
        tag("p").map(|_| Const::Placeholder),
    ))
    .map(Rc::new)
    .or(map_opt_with_context(parse_back_ref, |back_ref, context| {
        context.consts.get(&back_ref).cloned()
    }))
    .inspect_with_context(|result, context| {
        context.consts.insert(index, Rc::clone(result));
    })
    .parse(input, context)
}

fn parse_const_fields<'a>(
    input: IndexedStr<'a>,
    context: &mut Context<'a>,
) -> Result<(ConstFields<'a>, IndexedStr<'a>), ()> {
    alt((
        tag("U").map(|_| ConstFields::Unit),
        delimited(tag("T"), many0(parse_const), tag("E")).map(ConstFields::Tuple),
        delimited(tag("S"), many0(parse_identifier.and(parse_const)), tag("E")).map(ConstFields::Struct),
    ))
    .parse(input, context)
}

fn parse_const_int<'a, T>(input: IndexedStr<'a>, context: &mut Context<'a>) -> Result<(T, IndexedStr<'a>), ()>
where
    T: CheckedNeg + PrimInt,
{
    terminated(
        map_opt(opt(tag("n")).and(lower_hex_digit0), |(is_negative, data): (_, &str)| {
            let base = T::from_str_radix(data, 16).ok();

            if is_negative.is_none() {
                base
            } else {
                base.and_then(|value| value.checked_neg())
            }
        }),
        tag("_"),
    )
    .parse(input, context)
}

fn parse_const_str<'a>(input: IndexedStr<'a>, context: &mut Context<'a>) -> Result<(ConstStr<'a>, IndexedStr<'a>), ()> {
    map_opt(terminated(lower_hex_digit0, tag("_")), |s: &str| {
        (s.len() % 2 == 0).then(|| ConstStr(s))
    })
    .parse(input, context)
}

fn parse_base62_number<'a>(input: IndexedStr<'a>, context: &mut Context<'a>) -> Result<(u64, IndexedStr<'a>), ()> {
    map_opt(terminated(alphanumeric0, tag("_")), |num: &str| {
        if num.is_empty() {
            Some(0)
        } else {
            let mut value = 0_u64;

            for c in num.bytes() {
                let digit = match c {
                    b'0'..=b'9' => c - b'0',
                    b'a'..=b'z' => 10 + (c - b'a'),
                    _ => 36 + (c - b'A'),
                };

                value = value.checked_mul(62)?;
                value = value.checked_add(digit.into())?;
            }

            value.checked_add(1)
        }
    })
    .parse(input, context)
}

fn parse_back_ref<'a>(input: IndexedStr<'a>, context: &mut Context<'a>) -> Result<(usize, IndexedStr<'a>), ()> {
    map_opt(preceded(tag("B"), parse_base62_number), |num| num.try_into().ok()).parse(input, context)
}

fn parse_decimal_number<'a, T>(input: IndexedStr<'a>, context: &mut Context<'a>) -> Result<(T, IndexedStr<'a>), ()>
where
    T: PrimInt,
{
    map_opt(tag("0").or(digit1), |num: &str| T::from_str_radix(num, 10).ok()).parse(input, context)
}
