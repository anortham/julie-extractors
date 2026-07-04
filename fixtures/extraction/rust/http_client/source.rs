// reqwest client fixture: static-URL scoped and builder calls plus silent
// dynamic cases. Backs the rust `http.client_request.v1` capability row.
use reqwest::Client;

/// Static-URL requests: the scoped convenience free function and the builder
/// verb form both emit `http.client_request.v1`.
pub async fn load() -> Result<(), reqwest::Error> {
    reqwest::get("https://api.example.com/users").await?;
    let client = Client::new();
    client.post("https://api.example.com/items").await?;
    reqwest::Client::new().delete("/users/1").await?;
    Ok(())
}

/// Dynamic URLs stay silent (M2): `format!` and concatenation emit nothing.
pub async fn dynamic(id: u32) -> Result<(), reqwest::Error> {
    reqwest::get(format!("https://api.example.com/users/{id}")).await?;
    reqwest::get(&("https://x/".to_owned() + "y")).await?;
    Ok(())
}
