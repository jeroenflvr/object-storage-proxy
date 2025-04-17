use crate::parsers::credentials::parse_token_from_header;



pub fn validate_request(header: &str) -> Result<bool, String> {

    if header.is_empty() {
        return Err("Header is empty".to_string());
    }

    if !header.starts_with("AWS4-HMAC-SHA256 Credential=") {
        return Err("Invalid header format".to_string());
    }

    let token = parse_token_from_header(header).map_err(|_| "Failed to parse token")?;
    let (_, token) = token;

    if token.is_empty() {
        return Err("Token is empty".to_string());
    }

    Ok(true)
}