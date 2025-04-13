use reqwest::Client;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct IamResponse {
    access_token: String,
    // add expiration etc
}

pub(crate) async fn get_bearer(api_key: String) -> Result<String, Box<dyn std::error::Error>> {
    let client = Client::new();

    let params = [
        ("grant_type", "urn:ibm:params:oauth:grant-type:apikey"),
        ("apikey", &api_key),
    ];

    let resp = client
        .post("https://iam.cloud.ibm.com/identity/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&params)
        .send()
        .await?;

    if resp.status().is_success() {
        println!("Response: {:?}", resp);
        let iam_response: IamResponse = resp.json().await?;
        println!("Received access token: {}", iam_response.access_token);
        Ok(iam_response.access_token)
    } else {
        let err_text = resp.text().await?;
        eprintln!("Failed to get token: {}", err_text);
        Err(format!("Failed to get token: {}", err_text).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn get_bearer_with_mock_url(
        api_key: String,
        mock_url: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let client = Client::new();

        let params = [
            ("grant_type", "urn:ibm:params:oauth:grant-type:apikey"),
            ("apikey", &api_key),
        ];

        let resp = client
            .post(format!("{}/identity/token", mock_url))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&params)
            .send()
            .await?;

        if resp.status().is_success() {
            let iam_response: IamResponse = resp.json().await?;
            return Ok(iam_response.access_token);
        } else {
            let err_text = resp.text().await?;
            return Err(format!("Failed to get token: {}", err_text).into());
        }
    }

    #[tokio::test]
    async fn test_get_bearer_success() {
        let mock_server = MockServer::start().await;

        let response_body = r#"{
            "access_token": "mock_access_token"
        }"#;

        Mock::given(method("POST"))
            .and(path("/identity/token"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(response_body, "application/json"),
            )
            .mount(&mock_server)
            .await;

        let api_key = "mock_api_key".to_string();
        let result = get_bearer_with_mock_url(api_key, &mock_server.uri()).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "mock_access_token");
    }

    #[tokio::test]
    async fn test_get_bearer_failure() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/identity/token"))
            .respond_with(ResponseTemplate::new(400).set_body_string("Invalid API key"))
            .mount(&mock_server)
            .await;

        let api_key = "invalid_api_key".to_string();
        let result = get_bearer_with_mock_url(api_key, &mock_server.uri()).await;

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Failed to get token: Invalid API key"
        );
    }

    #[tokio::test]
    async fn test_get_bearer_invalid_json() {
        let mock_server = MockServer::start().await;

        let invalid_response_body = r#"{
            "invalid_field": "value"
        }"#;

        Mock::given(method("POST"))
            .and(path("/identity/token"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(invalid_response_body, "application/json"),
            )
            .mount(&mock_server)
            .await;

        let api_key = "mock_api_key".to_string();
        let result = get_bearer_with_mock_url(api_key, &mock_server.uri()).await;

        assert!(result.is_err());

        if let Err(err) = result {
            let err_message = err.to_string();
            assert!(
                err_message.contains("missing field `access_token`")
                    || err_message.contains("error decoding response body"),
                "Unexpected error message: {}",
                err_message
            );
        }
    }
}
