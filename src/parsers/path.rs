use nom::{
    IResult,
    Parser,
    character::complete::char,
    combinator::{eof, map, rest},
    sequence::preceded,
    branch::alt,
    bytes::complete::take_while1,
};



pub(crate) fn parse_path(input: &str) -> IResult<&str, (&str, &str)> {
    let (_remaining, (_, bucket, rest)) = (
        char('/'),
        take_while1(|c| c != '/'), // bucket
        alt((
            preceded(char('/'), rest),         // rest of the path
            map(eof, |_| ""),                  // no path after bucket
        )),
    ).parse(input)?;

    let rest_path = if rest.is_empty() {
        "/"
    } else {
        // recover the slash before `rest`
        &input[input.find(rest).unwrap() - 1..]
    };

    Ok(("", (bucket, rest_path)))
}
