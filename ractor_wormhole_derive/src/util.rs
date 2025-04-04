use quote::spanned::Spanned;

type ParseResult<T> = Result<T, venial::Error>;

pub fn error_fn<T>(msg: impl AsRef<str>, tokens: T) -> venial::Error
where
    T: Spanned,
{
    venial::Error::new_at_span(tokens.__span(), msg.as_ref())
}

pub fn bail_fn<R, T>(msg: impl AsRef<str>, tokens: T) -> ParseResult<R>
where
    T: Spanned,
{
    // TODO: using T: Spanned often only highlights the first tokens of the symbol, e.g. #[attr] in a function.
    // Could use function.name; possibly our own trait to get a more meaningful span... or change upstream in venial.

    Err(error_fn(msg, tokens))
}

macro_rules! bail {
    ($tokens:expr, $format_string:literal $($rest:tt)*) => {
        $crate::util::bail_fn(format!($format_string $($rest)*), $tokens)
    }
}

pub(crate) use bail;
