use reqwest::blocking::Client;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct IamResponse {
    access_token: String,
    // add expiration etc
}

pub(crate) fn get_bearer(api_key: String) -> Result<String, Box<dyn std::error::Error>> {
    // todo: 
    // - check if bearer token is in the cache
    // - if not, call the callback to get the token and cache the token
    // - if yes, check if the token is expired
    // - if expired, renew the token
    // - if not expired, use the cached token


    let client = Client::new();

    let params = [
        ("grant_type", "urn:ibm:params:oauth:grant-type:apikey"),
        ("apikey", &api_key),
    ];

    let resp = client
        .post("https://iam.cloud.ibm.com/identity/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&params)
        .send()?;

    if resp.status().is_success() {
        // println!("Response: {:?}", resp);
        let iam_response: IamResponse = resp.json()?;
        // println!("Received access token: {}", iam_response.access_token);
        Ok(iam_response.access_token)
    } else {
        let err_text = resp.text()?;
        eprintln!("Failed to get token: {}", err_text);
        Err(format!("Failed to get token: {}", err_text).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use tokio::runtime::Runtime;

    fn get_bearer_with_url(api_key: String, base_url: &str) -> Result<String, Box<dyn std::error::Error>> {
        let client = Client::new();
        let params = [
            ("grant_type", "urn:ibm:params:oauth:grant-type:apikey"),
            ("apikey", &api_key),
        ];
        let resp = client
            .post(&format!("{}/identity/token", base_url))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&params)
            .send()?;

        if resp.status().is_success() {
            let iam_response: IamResponse = resp.json()?;
            Ok(iam_response.access_token)
        } else {
            let err_text = resp.text()?;
            Err(format!("Failed to get token: {}", err_text).into())
        }
    }

    #[test]
    fn test_get_bearer_success() {
        let rt = Runtime::new().unwrap();
        let mock_server = rt.block_on(MockServer::start());

        let response_body = r#"{
            "access_token": "mock_access_token"
        }"#;
        rt.block_on(
            Mock::given(method("POST"))
                .and(path("/identity/token"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_raw(response_body, "application/json"),
                )
                .mount(&mock_server),
        );

        let api_key = "mock_api_key".to_string();
        let result = get_bearer_with_url(api_key, &mock_server.uri());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "mock_access_token");
    }

    #[test]
    fn test_get_bearer_failure() {
        let rt = Runtime::new().unwrap();
        let mock_server = rt.block_on(MockServer::start());

        // Mock a 400 error with plain text body
        rt.block_on(
            Mock::given(method("POST"))
                .and(path("/identity/token"))
                .respond_with(
                    ResponseTemplate::new(400).set_body_string("Invalid API key"),
                )
                .mount(&mock_server),
        );

        let api_key = "invalid_api_key".to_string();
        let result = get_bearer_with_url(api_key, &mock_server.uri());
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Failed to get token: Invalid API key"
        );
    }

    #[test]
    fn test_get_bearer_invalid_json() {
        let rt = Runtime::new().unwrap();
        let mock_server = rt.block_on(MockServer::start());

        // Mock a 200 OK with invalid JSON payload
        let invalid_response_body = r#"{
            "invalid_field": "value"
        }"#;
        rt.block_on(
            Mock::given(method("POST"))
                .and(path("/identity/token"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_raw(invalid_response_body, "application/json"),
                )
                .mount(&mock_server),
        );

        let api_key = "mock_api_key".to_string();
        let result = get_bearer_with_url(api_key, &mock_server.uri());
        assert!(result.is_err());

        let err_message = result.unwrap_err().to_string();
        assert!(
            err_message.contains("missing field `access_token`")
                || err_message.contains("error decoding response body"),
            "Unexpected error message: {}",
            err_message
        );
    }
}
