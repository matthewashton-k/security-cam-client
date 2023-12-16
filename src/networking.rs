use std::collections::HashMap;
use std::error::Error;
use reqwest::redirect::Policy;
use reqwest::Url;
use security_cam_common::encryption::*;
use security_cam_common::shuttle_runtime::tokio::fs::File;

pub struct Client<'a> {
    addr: Url,
    username: &'a str,
    password: &'a str,
    client: reqwest::Client,
    encryptor: EncryptDecrypt,
}

impl<'a> Client<'a> {
    pub async fn new(addr: &'a str, username: &'a str, password: &'a str) -> Client<'a> {
        // doesnt need to be a recoverable error because if it fails then we want our whole program to exit anyways
        let client_with_cookies = reqwest::Client::builder().redirect(Policy::limited(2)).cookie_store(true).build().expect("couldn't build http client");
        let (key, salt) = generate_key(password).expect("couldnt generate keystream");
        let encryptor = EncryptDecrypt {
            key: Some(key),
            salt: Some(salt),
            file: File::create("first.mp4").await.expect("couldn't create first mp4 file")
        };
        Client {
            addr: Url::parse(addr).expect("invalid url"),
            username,
            password,
            client:  client_with_cookies,
            encryptor
        }
    }

    pub async fn login(&self) -> Result<(), Box<dyn Error>> {
        let mut params = HashMap::new();
        params.insert("username", self.username);
        params.insert("password", self.password);
        let resp = self.client.post(self.addr.join("login")?.as_str()).form(&params).send().await?;

        // later on change the webserver to alter the response code if the login fails
        if !resp.text().await?.contains("Logout") {
            return Err("login failed".into());
        }

        Ok(())
    }

    pub async fn logout(&self) -> Result<(), Box<dyn Error>> {
        let resp = self.client.get(self.addr.join("logout")?.as_str()).send().await?;

        // later on change the webserver to alter the response code if the logout fails
        if !resp.text().await?.contains("Login") {
            return Err("logout failed".into());
        }
        todo!()
    }
    pub async fn send_and_delete(&self, filename: String) -> Result<(), Box<dyn Error>> {
        todo!("");
    }
}

#[cfg(test)]
mod tests {
    use security_cam_common::shuttle_runtime::tokio;

    /// only run this test while the server is active
    #[tokio::test]
    async fn test_login() {
        let client = super::Client::new("http://127.0.0.1:8000", "admin", "pass").await;
        client.login().await.unwrap();
    }

    #[tokio::test]
    async fn test_bad_login() {
        let client = super::Client::new("http://127.0.0.1:8000", "admin", "badpass").await;
        client.login().await.unwrap_err();
    }
}