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
        ("apikey", &api_key)
    ];

    let resp = client.post("https://iam.cloud.ibm.com/identity/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&params)
        .send()
        .await?;

    if resp.status().is_success() {
        println!("Response: {:?}", resp);
        let iam_response: IamResponse = resp.json().await?;
        println!("Received access token: {}", iam_response.access_token);
        return Ok(iam_response.access_token)

    } else {
        let err_text = resp.text().await?;
        eprintln!("Failed to get token: {}", err_text);
        return Err(format!("Failed to get token: {}", err_text).into());
    }

}